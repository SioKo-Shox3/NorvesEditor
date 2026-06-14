//! Workstream G / G5: transport-level disconnect / reconnect / error handling,
//! observed end-to-end across a real local WebSocket against the C++ engine SDK
//! test harness (`ws_test_server`).
//!
//! This is the abnormal-path sibling of `ws_roundtrip.rs` (G4). Where G4 proves
//! the happy round trip, G5 proves the *connection lifecycle* is observable:
//!
//!   1. **Disconnect detection** — when the harness is killed mid-session, every
//!      pending/future `request` on the handle resolves to
//!      [`RequestError::ConnectionClosed`] and the event stream closes (the
//!      dispatcher task exits on terminal close, per its contract).
//!   2. **Reconnect** — after the harness restarts on the *same* port,
//!      [`connect_with_retry`] establishes a fresh [`DispatchHandle`] and the
//!      handshake succeeds again.
//!   3. **Connect-retry bound** — against a port with nothing listening,
//!      `connect_with_retry` gives up with [`ConnectError::Timeout`] within its
//!      budget (no infinite loop).
//!   4. **Malformed frame is non-fatal** — an undecodable inbound frame is
//!      logged-and-dropped by the dispatcher; subsequent requests still resolve.
//!   5. **`ReconnectManager` re-dials in place** — `reconnect()` tears down the
//!      old handle (so requests on it fail with `ConnectionClosed`) and
//!      establishes a fresh, usable handle against the same live harness.
//!
//! # Running it
//!
//! Opt-in via the `NORVES_WS_TEST_SERVER` env var (absolute path to the compiled
//! harness). Unset => the cross-process scenarios skip (the pure connect-retry
//! bound test still runs, since it needs no harness). See `ws_roundtrip.rs` for
//! the same convention and the flakiness defenses copied here.

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use norves_bridge_core::{ResponsePayload, ValidatedEnvelope, VersionString};
use norves_bridge_editor_client::{
    connect_with_retry, parse_hello_result, ConnectError, DispatchHandle, HelloParams,
    ReconnectManager, RequestError, RetryConfig,
};
use tokio::sync::broadcast;

/// Generous-but-finite bound for every blocking await in the test.
const RECV_TIMEOUT: Duration = Duration::from_secs(5);

/// The wire protocol version used throughout.
fn version() -> VersionString {
    VersionString::try_from("0.1".to_owned()).expect("0.1 is a valid version")
}

/// RAII guard: kills and reaps the child harness on drop, on EVERY exit path.
/// Without this a failed assertion would leave the harness holding the port on
/// Windows.
struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    /// Kills and waits the child *now*, deterministically, so a follow-up
    /// same-port restart does not race a still-alive listener. Safe to call more
    /// than once (kill/wait on an already-reaped child is a no-op error we drop).
    fn kill_and_wait(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

/// Claims an OS-assigned ephemeral `127.0.0.1` port, then releases it so the
/// child can bind it.
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

/// Spawns the harness on `port` with `extra_args` and waits (up to `timeout`)
/// for its `READY <port>` line on stdout. Returns the RAII-guarded child.
fn spawn_ready_harness(exe: &str, port: u16, extra_args: &[&str], timeout: Duration) -> ChildGuard {
    let mut command = Command::new(exe);
    command
        .arg("--bridge-port")
        .arg(port.to_string())
        .args(extra_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn harness {exe:?}: {e}"));

    let stdout = child.stdout.take().expect("child stdout piped");
    let guard = ChildGuard { child };

    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let is_ready = line.starts_with("READY");
                    let _ = tx.send(line);
                    if is_ready {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO);
        match rx.recv_timeout(remaining) {
            Ok(line) if line.starts_with("READY") => return guard,
            Ok(_) => continue,
            Err(_) => panic!("harness did not print READY within {timeout:?}"),
        }
    }
}

/// Awaits `fut` with [`RECV_TIMEOUT`], panicking with `what` on expiry.
async fn with_timeout<F, T>(what: &str, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    match tokio::time::timeout(RECV_TIMEOUT, fut).await {
        Ok(value) => value,
        Err(_) => panic!("timed out waiting for {what}"),
    }
}

/// Builds a request-kind envelope.
fn request(
    id: &str,
    method: &str,
    params: Option<serde_json::Map<String, serde_json::Value>>,
) -> ValidatedEnvelope {
    use norves_bridge_core::{CorrelationId, MethodName};
    ValidatedEnvelope::Request {
        version: version(),
        id: CorrelationId::try_from(id.to_owned()).expect("non-empty id"),
        method: MethodName::try_from(method.to_owned()).expect("namespaced method"),
        params,
        session_id: None,
        seq: None,
    }
}

/// A short, eager retry config for the cross-process connect: nothing else is
/// competing for the loopback port, so small backoffs keep the test fast.
fn fast_retry() -> RetryConfig {
    RetryConfig {
        initial_backoff: Duration::from_millis(25),
        max_backoff: Duration::from_millis(250),
        max_elapsed: Duration::from_secs(10),
        jitter: false,
    }
}

/// Runs the full handshake on `handle`, asserting the harness's deterministic
/// hello result. Proves the connection is live and usable.
async fn assert_hello_ok(handle: &DispatchHandle, id: &str) {
    let hello_params = HelloParams::new("NorvesEditor", vec![version()])
        .to_params()
        .expect("hello params serialize to a JSON object");
    let response = with_timeout(
        "bridge.hello response",
        handle.request(
            request(id, "bridge.hello", Some(hello_params)),
            RECV_TIMEOUT,
        ),
    )
    .await
    .expect("hello request resolves");
    let result = match response {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(err) => panic!("hello returned an engine error: {err:?}"),
    };
    let outcome = parse_hello_result(&result).expect("hello result parses");
    assert_eq!(outcome.session_id, "sess-mock-1");
    assert_eq!(outcome.server_name, "MockEngine");
}

/// Reads the harness path from the env, or returns None (caller skips).
fn harness_exe() -> Option<String> {
    match std::env::var("NORVES_WS_TEST_SERVER") {
        Ok(path) => Some(path),
        Err(_) => {
            eprintln!(
                "skipped: set NORVES_WS_TEST_SERVER to the ws_test_server executable path to run \
                 the cross-process reconnect scenarios"
            );
            None
        }
    }
}

/// Scenario 1: killing the harness mid-session is observable — the handle's
/// `request` then fails with `ConnectionClosed` and the event stream closes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn killing_harness_is_observed_as_connection_closed() {
    let Some(exe) = harness_exe() else { return };

    let port = pick_free_port();
    let mut guard = spawn_ready_harness(&exe, port, &[], Duration::from_secs(10));
    let url = format!("ws://127.0.0.1:{port}");

    let handle = with_timeout(
        "connect_with_retry",
        connect_with_retry(&url, &fast_retry()),
    )
    .await
    .expect("connects to the harness");
    let mut events = handle.subscribe_events();

    // Confirm the connection is live before tearing it down.
    assert_hello_ok(&handle, "req-hello-1").await;

    // Kill the harness: the client transport sees the socket drop, the dispatcher
    // task observes terminal close and exits, failing all pending and closing the
    // broadcast.
    guard.kill_and_wait();

    // A subsequent request must resolve to ConnectionClosed (not hang). This is
    // the primary, contract-level observation of a terminated connection.
    let err = with_timeout(
        "request after kill",
        handle.request(
            request("req-after-kill", "engine.getStatus", None),
            RECV_TIMEOUT,
        ),
    )
    .await
    .expect_err("request after kill must fail");
    assert!(
        matches!(err, RequestError::ConnectionClosed),
        "expected ConnectionClosed, got {err:?}"
    );

    // The event stream closes once no live sender remains. The dispatcher task
    // has exited (so its sender dropped), but `DispatchHandle` itself holds a
    // broadcast sender clone; the stream therefore stays open until the last
    // handle is dropped too. Drop every handle, then the receiver must observe
    // `Closed` (after draining any already-buffered events).
    drop(handle);
    loop {
        match with_timeout("event stream close", events.recv()).await {
            Ok(_event) => continue, // drain any straggler then re-check
            Err(broadcast::error::RecvError::Closed) => break,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

/// Scenario 2: after a disconnect, restarting the harness on the same port and
/// dialing again establishes a fresh, usable connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_after_restart_on_same_port_succeeds() {
    let Some(exe) = harness_exe() else { return };

    let port = pick_free_port();
    let url = format!("ws://127.0.0.1:{port}");

    // First connection.
    let mut first_guard = spawn_ready_harness(&exe, port, &[], Duration::from_secs(10));
    let first = with_timeout("first connect", connect_with_retry(&url, &fast_retry()))
        .await
        .expect("first connects");
    assert_hello_ok(&first, "req-hello-first").await;

    // Drop the first connection and kill+wait the harness deterministically so
    // the port is released before we restart on it.
    first.shutdown().await;
    first_guard.kill_and_wait();
    drop(first_guard);

    // Restart on the SAME port. The harness's bind-retry absorbs any transient
    // bind failure during port release.
    let _second_guard = spawn_ready_harness(&exe, port, &[], Duration::from_secs(10));
    let second = with_timeout("second connect", connect_with_retry(&url, &fast_retry()))
        .await
        .expect("reconnects on the same port");

    // The reconnected handle must work end-to-end.
    assert_hello_ok(&second, "req-hello-second").await;
    second.shutdown().await;
}

/// Scenario 3: against a port with nothing listening, connect_with_retry gives
/// up within its budget instead of looping forever. Needs no harness, so it runs
/// even when NORVES_WS_TEST_SERVER is unset.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_with_retry_is_bounded_when_nothing_listens() {
    // Claim then release an ephemeral port: every connect attempt is refused.
    let port = pick_free_port();
    let url = format!("ws://127.0.0.1:{port}");
    let cfg = RetryConfig {
        initial_backoff: Duration::from_millis(20),
        max_backoff: Duration::from_millis(100),
        max_elapsed: Duration::from_millis(400),
        jitter: false,
    };

    let start = Instant::now();
    let result = connect_with_retry(&url, &cfg).await;
    let elapsed = start.elapsed();

    match result {
        Err(ConnectError::Timeout {
            elapsed: budget, ..
        }) => {
            assert_eq!(budget, cfg.max_elapsed);
        }
        Ok(_) => panic!("connect to an unreachable port must not succeed"),
    }
    // Bounded: well under a hang threshold (budget is 400ms; allow generous slack
    // for a refused-connect attempt that overshoots the deadline by one attempt).
    assert!(
        elapsed < Duration::from_secs(5),
        "retry loop was not bounded: took {elapsed:?}"
    );
}

/// Scenario 4: a malformed inbound frame is logged-and-dropped (non-fatal); the
/// dispatcher keeps serving subsequent requests.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_frame_is_non_fatal() {
    let Some(exe) = harness_exe() else { return };

    let port = pick_free_port();
    // The harness injects one malformed frame right after the first response.
    let _guard = spawn_ready_harness(&exe, port, &["--inject-malformed"], Duration::from_secs(10));
    let url = format!("ws://127.0.0.1:{port}");

    let handle = with_timeout(
        "connect_with_retry",
        connect_with_retry(&url, &fast_retry()),
    )
    .await
    .expect("connects to the harness");

    // First request triggers the harness to also send the malformed frame.
    assert_hello_ok(&handle, "req-hello").await;

    // The dispatcher must have dropped the malformed frame without dying: a
    // follow-up request still resolves normally.
    let status = with_timeout(
        "engine.getStatus after malformed",
        handle.request(
            request("req-status", "engine.getStatus", None),
            RECV_TIMEOUT,
        ),
    )
    .await
    .expect("status resolves after a malformed frame");
    assert!(
        matches!(status, ResponsePayload::Result(_)),
        "expected a result after the malformed frame, got {status:?}"
    );

    handle.shutdown().await;
}

/// Scenario 5: `ReconnectManager::reconnect` re-dials in place against the *same*
/// live harness. This exercises the manager's documented contract that the old
/// handle is shut down *before* the new connection is established — there is no
/// overlap of two live dispatchers for one logical session.
///
/// # Why a single harness process suffices here
///
/// The harness's WebSocket server transport keeps listening across a client
/// disconnect: a peer close fires `LWS_CALLBACK_CLOSED`, which only clears the
/// active wsi (it does NOT shut the recv queue down), so the harness's recv loop
/// keeps blocking for the next frame rather than returning `nullopt` and exiting.
/// The single-connection posture (G3) rejects a *second simultaneous* client but
/// accepts a fresh one once the previous wsi is cleared. Because `reconnect()`
/// awaits the old handle's `shutdown()` (which drops the client transport and
/// tears the socket down) *before* dialing, the server has cleared the old wsi by
/// the time the new connect arrives; any residual timing slack is absorbed by
/// `connect_with_retry`'s retry budget.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_manager_redials_in_place_and_invalidates_old_handle() {
    let Some(exe) = harness_exe() else { return };

    let port = pick_free_port();
    let _guard = spawn_ready_harness(&exe, port, &[], Duration::from_secs(10));
    let url = format!("ws://127.0.0.1:{port}");

    let mut manager = ReconnectManager::new(url, fast_retry());

    // Initial connection via the manager; the handed-out handle must work.
    let old_handle = with_timeout("manager connect", manager.connect())
        .await
        .expect("manager connects to the harness");
    assert_hello_ok(&old_handle, "req-hello-mgr-1").await;

    // Hold a reference to the OLD handle across the reconnect so we can prove it
    // is invalidated afterwards.
    let new_handle = with_timeout("manager reconnect", manager.reconnect())
        .await
        .expect("manager reconnects in place");

    // The new handle is live and usable end-to-end.
    assert_hello_ok(&new_handle, "req-hello-mgr-2").await;

    // The manager's stored handle is the new one and is also usable.
    let stored = manager
        .handle()
        .expect("manager holds a handle after reconnect");
    assert_hello_ok(&stored, "req-hello-mgr-3").await;

    // The OLD handle must have been shut down by reconnect(): a request on it now
    // resolves to ConnectionClosed (its dispatcher task has exited).
    let err = with_timeout(
        "request on the old handle after reconnect",
        old_handle.request(
            request("req-old-after-reconnect", "engine.getStatus", None),
            RECV_TIMEOUT,
        ),
    )
    .await
    .expect_err("request on the old, reconnected-away handle must fail");
    assert!(
        matches!(err, RequestError::ConnectionClosed),
        "expected ConnectionClosed on the old handle, got {err:?}"
    );

    new_handle.shutdown().await;
}
