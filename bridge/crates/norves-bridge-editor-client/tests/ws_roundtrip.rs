//! Workstream G / G4: cross-process, real-WebSocket end-to-end round trip.
//!
//! This is the network sibling of `loopback_roundtrip.rs` (D5b). Instead of an
//! in-process mock on a loopback channel, it launches the C++ engine SDK test
//! harness (`ws_test_server`, built under bridge/cpp/engine-sdk/tests) as a
//! **separate OS process**, connects the real [`WsClientTransport`] (G2) to it
//! over `ws://127.0.0.1:<port>`, spawns the real [`Dispatcher`], and drives the
//! full handshake / status / runtime-control / log-streaming round trip across
//! the wire.
//!
//! # Running it
//!
//! The test is **opt-in**: it needs the compiled C++ harness, whose absolute
//! path is supplied via the `NORVES_WS_TEST_SERVER` env var. When that var is
//! unset the test prints a skip notice and returns (so `cargo test` stays green
//! on a machine without the C++ build). When set, it runs for real.
//!
//! # Flakiness defenses
//!
//! Cross-process socket tests are prone to races; every one is guarded:
//!   * **Port**: an OS-assigned ephemeral port (`127.0.0.1:0`) avoids fixed-port
//!     collisions. The brief TOCTOU gap between releasing it and the child
//!     re-binding it is absorbed by the connect retry loop.
//!   * **Startup**: we wait for the harness's `READY <port>` stdout line (with a
//!     timeout) before dialing, so we never race the bind.
//!   * **Connect**: dialing retries with backoff until the listener is up.
//!   * **Every await** is bounded by a timeout, so a logic bug fails fast
//!     instead of hanging CI.
//!   * **Teardown**: a RAII guard kills + waits the child on every exit path
//!     (success, early return, or panic), so no harness process leaks — relevant
//!     on Windows where an orphaned listener would hold the port.

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use norves_bridge_core::VersionString;
use norves_bridge_core::{EngineState, LogLevel, ResponsePayload, RuntimeState, ValidatedEnvelope};
use norves_bridge_editor_client::{
    parse_hello_result, parse_log_message, parse_status_result, Dispatcher, HelloParams,
    WsClientTransport,
};

/// Generous-but-finite bound for every blocking await in the test.
const RECV_TIMEOUT: Duration = Duration::from_secs(5);

/// Number of `log.message` events the harness emits per subscribe (must match
/// `kLogBurst` in ws_test_server.cpp). The test asserts all of them arrive in
/// order.
const LOG_BURST: usize = 3;

/// The wire protocol version used throughout.
fn version() -> VersionString {
    VersionString::try_from("0.1".to_owned()).expect("0.1 is a valid version")
}

/// RAII guard: kills and reaps the child harness on drop, on EVERY exit path
/// (success, early return, or a panicking assertion). Without this a failed
/// assertion would leave the harness holding the port on Windows.
struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Best effort: the child may already have exited (e.g. after the client
        // closed the socket). Ignore errors — we only need it gone.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Claims an OS-assigned ephemeral `127.0.0.1` port, then releases it so the
/// child can bind it. The connect retry loop covers the release/re-bind gap.
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

/// Spawns the harness on `port` and waits (up to `timeout`) for its
/// `READY <port>` line on stdout. Returns the RAII-guarded child once ready.
///
/// stdout is read on a dedicated std thread so the wait can be bounded by a
/// channel `recv_timeout` rather than blocking forever on a child that never
/// prints READY.
fn spawn_ready_harness(exe: &str, port: u16, timeout: Duration) -> ChildGuard {
    let mut child = Command::new(exe)
        .arg("--bridge-port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn harness {exe:?}: {e}"));

    let stdout = child.stdout.take().expect("child stdout piped");
    let guard = ChildGuard { child };

    // Read stdout lines off-thread; forward the first READY line (or EOF) to the
    // main thread so we can bound the wait with recv_timeout.
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let is_ready = line.starts_with("READY");
                    // Ignore a closed receiver (main thread moved on).
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
            // A non-READY line (shouldn't happen on stdout) — keep waiting.
            Ok(_) => continue,
            Err(_) => panic!("harness did not print READY within {timeout:?}"),
        }
    }
}

/// Connects [`WsClientTransport`] with retry + linear-ish backoff, covering the
/// window between READY and the listener actually accepting. Gives up after
/// `overall` elapses.
async fn connect_with_retry(url: &str, overall: Duration) -> WsClientTransport {
    let deadline = Instant::now() + overall;
    let mut backoff = Duration::from_millis(50);
    loop {
        match WsClientTransport::connect(url).await {
            Ok(transport) => return transport,
            Err(err) => {
                if Instant::now() >= deadline {
                    panic!("could not connect to {url} within {overall:?}: {err:?}");
                }
                tokio::time::sleep(backoff).await;
                // Cap the backoff so the retry stays responsive near the listen
                // edge while not busy-spinning.
                backoff = (backoff * 2).min(Duration::from_millis(500));
            }
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

/// Builds a request-kind envelope with a unique id, method, and optional params.
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_cross_process_round_trip() {
    // Opt-in: without the compiled C++ harness path we skip (not fail), so the
    // suite stays green on a machine without the C++ build.
    let exe = match std::env::var("NORVES_WS_TEST_SERVER") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "ws_cross_process_round_trip skipped: set NORVES_WS_TEST_SERVER to the \
                 ws_test_server executable path to run it"
            );
            return;
        }
    };

    // 1. Launch the harness on a free port and wait for READY.
    let port = pick_free_port();
    let _guard = spawn_ready_harness(&exe, port, Duration::from_secs(10));
    let url = format!("ws://127.0.0.1:{port}");

    // 2. Connect the real WS transport (retrying the listen edge) and spawn the
    //    real dispatcher.
    let transport = connect_with_retry(&url, Duration::from_secs(5)).await;
    let handle = Dispatcher::spawn(transport);

    // 3. Subscribe to events BEFORE issuing log.subscribe so no event is missed.
    let mut events = handle.subscribe_events();

    // 4. bridge.hello round trip.
    let hello_params = HelloParams::new("NorvesEditor", vec![version()])
        .to_params()
        .expect("hello params serialize to a JSON object");
    let hello_response = with_timeout(
        "bridge.hello response",
        handle.request(
            request("req-hello", "bridge.hello", Some(hello_params)),
            RECV_TIMEOUT,
        ),
    )
    .await
    .expect("hello request resolves");
    let hello_result = match hello_response {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(err) => panic!("hello returned an engine error: {err:?}"),
    };
    let outcome = parse_hello_result(&hello_result).expect("hello result parses");
    assert_eq!(outcome.session_id, "sess-mock-1");
    assert_eq!(outcome.protocol_version.as_str(), "0.1");
    assert_eq!(outcome.server_name, "MockEngine");

    // 5. bridge.getCapabilities round trip. The harness's FakeAdapter advertises
    //    a single deterministic capability descriptor, so we assert a concrete
    //    success result whose `capabilities` array carries that descriptor
    //    (matching the bridge.getCapabilities.result schema) rather than just a
    //    non-error response.
    let capabilities_response = with_timeout(
        "bridge.getCapabilities response",
        handle.request(
            request("req-caps", "bridge.getCapabilities", None),
            RECV_TIMEOUT,
        ),
    )
    .await
    .expect("bridge.getCapabilities request resolves");
    let capabilities_result = match capabilities_response {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(err) => {
            panic!("bridge.getCapabilities returned an engine error: {err:?}")
        }
    };
    let capabilities = capabilities_result
        .get("capabilities")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| {
            panic!("getCapabilities result lacks a capabilities array, got {capabilities_result:?}")
        });
    assert_eq!(
        capabilities.len(),
        1,
        "expected exactly one capability descriptor, got {capabilities:?}"
    );
    assert_eq!(
        capabilities[0]
            .get("name")
            .and_then(serde_json::Value::as_str),
        Some("runtime.control"),
        "unexpected capability descriptor: {:?}",
        capabilities[0]
    );

    // 6. engine.getStatus round trip.
    let status_response = with_timeout(
        "engine.getStatus response",
        handle.request(
            request("req-status", "engine.getStatus", None),
            RECV_TIMEOUT,
        ),
    )
    .await
    .expect("status request resolves");
    let status_result = match status_response {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(err) => panic!("getStatus returned an engine error: {err:?}"),
    };
    let snapshot = parse_status_result(&status_result).expect("status result parses");
    assert_eq!(snapshot.engine_state, EngineState::Ready);
    assert_eq!(snapshot.runtime_state, RuntimeState::Edit);
    assert_eq!(snapshot.engine_name.as_deref(), Some("MockEngine"));

    // 7. runtime.play round trip.
    let play_response = with_timeout(
        "runtime.play response",
        handle.request(
            request("req-play", "runtime.play", Some(serde_json::Map::new())),
            RECV_TIMEOUT,
        ),
    )
    .await
    .expect("runtime.play request resolves");
    let play_result = match play_response {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(err) => panic!("runtime.play returned an engine error: {err:?}"),
    };
    assert_eq!(
        play_result
            .get("accepted")
            .and_then(serde_json::Value::as_bool),
        Some(true),
        "runtime.play should be accepted, got {play_result:?}"
    );

    // 8. log.subscribe, then receive the burst of log.message events. The
    //    harness emits LOG_BURST events back-to-back after acking; the SDK
    //    transport guarantees in-order delivery, so all must arrive in order.
    let subscribe_response = with_timeout(
        "log.subscribe response",
        handle.request(request("req-logsub", "log.subscribe", None), RECV_TIMEOUT),
    )
    .await
    .expect("log.subscribe request resolves");
    assert!(
        matches!(subscribe_response, ResponsePayload::Result(_)),
        "log.subscribe should resolve with a result"
    );

    for i in 0..LOG_BURST {
        let event = with_timeout(&format!("log.message event #{i}"), events.recv())
            .await
            .expect("event received without lag/close");
        let log = match &*event {
            ValidatedEnvelope::Event { event, params, .. } => {
                assert_eq!(event.as_str(), "log.message");
                let params = params.clone().expect("log.message carries params");
                parse_log_message(&serde_json::Value::Object(params)).expect("log params parse")
            }
            other => panic!("expected a log.message event, got {other:?}"),
        };
        assert_eq!(log.level, LogLevel::Info);
        assert_eq!(log.message, "Game started");
    }

    // 9. Orderly shutdown: closing the dispatcher drops the WS transport, which
    //    closes the socket; the harness's recv() then returns nullopt and its
    //    process exits. The RAII guard kills+waits it regardless.
    handle.shutdown().await;
}
