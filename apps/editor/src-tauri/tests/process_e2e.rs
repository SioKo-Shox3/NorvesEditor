//! Engine process launch / kill / relaunch end-to-end test (Phase J5).
//!
//! # Scope (HONEST)
//!
//! This test validates the external mock-engine launch / READY-handshake /
//! kill / relaunch CONTRACT that the backend's `launch_engine` and `stop_engine`
//! commands depend on.  Specifically it exercises:
//!
//! * Backend-style port injection: pick a free port, spawn
//!   `norves_mock_engine --bridge-port <port>`, read the `READY <port>` line
//!   from stdout, and verify the port in the line matches the injected value.
//! * Bridge reachability: after READY, `connect_with_retry` + `bridge.hello`
//!   succeeds (the engine is live and speaking the protocol).
//! * Kill + relaunch: kill the first engine instance (Child::kill), then spawn
//!   a second on a fresh port and confirm the same READY + connect + hello
//!   cycle works — proving relaunch-after-kill is clean.
//!
//! # What this does NOT cover
//!
//! This test does NOT execute the Tauri command layer (`process_runtime.rs` --
//! `launch_engine` / `stop_engine`).  Those commands take a concrete
//! `AppHandle<Wry>` (the `#[default_runtime]` resolves bare `AppHandle` to
//! `AppHandle<Wry>`); a `MockRuntime` app's handle is a type mismatch and
//! linking real Wry into the test binary fails to load on the Windows harness
//! (`STATUS_ENTRYPOINT_NOT_FOUND`, WebView2Loader), breaking all lib tests.
//! Making the commands and the load-bearing connect/relay code runtime-generic
//! is too much churn/risk for alpha.
//!
//! The Tauri command orchestration is instead covered by:
//! * J1 pure unit tests in `process.rs` and `process_runtime.rs` (all of
//!   `process::*`, including `monitor_should_emit_exit`).
//! * Implementation review (subagent).
//! * Section 10 manual GUI acceptance (launch -> play -> stop -> relaunch
//!   against the real editor + mock engine).
//!
//! # Opt-in
//!
//! Set `NORVES_ENGINE_PATH` to the absolute path of the compiled mock engine.
//! When the variable is unset or does not point to an existing file the test
//! prints a `[SKIP]` message and returns immediately (passes).  CI / manual
//! verification sets the variable so the real run is gated.

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use norves_bridge_core::{
    CorrelationId, MethodName, ResponsePayload, ValidatedEnvelope, VersionString,
};
use norves_bridge_editor_client::{
    connect_with_retry, parse_hello_result, HelloParams, RequestError, RetryConfig,
};

/// How long to wait for the engine's `READY <port>` stdout line.
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// How long each bridge request is allowed to take.
const RECV_TIMEOUT: Duration = Duration::from_secs(5);

/// RAII guard: kills and reaps the child engine on drop, on EVERY exit path
/// (success, early return, or a panicking assertion). Without this a failed
/// assertion can leave the engine holding the port on Windows.
struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Claims an OS-assigned ephemeral `127.0.0.1` port, then releases it so the
/// engine can bind it. The connect retry loop in `connect_with_retry` absorbs
/// the release / re-bind race.
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

/// Spawns the mock engine on `port`, waits up to `READY_TIMEOUT` for its
/// `READY <port>` stdout line, and asserts the line matches the injected port.
/// Returns the RAII-guarded child once READY is confirmed.
///
/// stdout is read on a dedicated std thread so the wait can be bounded by a
/// channel `recv_timeout` rather than blocking forever on a child that never
/// prints READY.
fn spawn_engine_on_free_port(exe: &str) -> (ChildGuard, u16) {
    let port = pick_free_port();

    let mut child = Command::new(exe)
        .arg("--bridge-port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn mock engine {exe:?}: {e}"));

    // Take stdout BEFORE moving child into the guard so the borrow is resolved.
    // The guard is constructed immediately after spawn so that ANY panic or
    // early-return below (READY timeout, port-equality assert, etc.) unwinds
    // through a live ChildGuard and kills + reaps the child.  This mirrors the
    // ordering in conformance.rs (spawn -> take stdout -> guard -> READY wait).
    let stdout = child.stdout.take().expect("child stdout piped");
    let guard = ChildGuard { child };

    // Read READY on a separate thread so we can apply a timeout via a channel.
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

    // Wait for the READY line under READY_TIMEOUT.
    let ready_line = rx
        .recv_timeout(READY_TIMEOUT)
        .unwrap_or_else(|_| panic!("mock engine did not print READY within {READY_TIMEOUT:?}"));

    assert!(
        ready_line.starts_with("READY"),
        "expected READY line, got: {ready_line:?}"
    );

    // Verify the port in the READY line matches what we injected.
    let parts: Vec<&str> = ready_line.split_whitespace().collect();
    assert_eq!(
        parts.len(),
        2,
        "READY line must have exactly two tokens: {ready_line:?}"
    );
    let ready_port: u16 = parts[1]
        .parse()
        .unwrap_or_else(|_| panic!("READY port token is not a u16: {ready_line:?}"));
    assert_eq!(
        ready_port, port,
        "READY line advertised port {ready_port} but we injected {port}"
    );

    (guard, port)
}

/// Builds a `bridge.hello` request envelope suitable for sending over a
/// live dispatcher.
fn hello_envelope() -> ValidatedEnvelope {
    let version = VersionString::try_from("0.1".to_owned()).expect("0.1 is a valid version");
    let params = HelloParams::new("norves-process-e2e", vec![version.clone()])
        .to_params()
        .expect("hello params serialize to object");
    ValidatedEnvelope::Request {
        version,
        id: CorrelationId::try_from("e2e-hello-1".to_owned()).expect("non-empty id"),
        method: MethodName::try_from("bridge.hello".to_owned()).expect("namespaced method"),
        params: Some(params),
        session_id: None,
        seq: None,
    }
}

/// Connects to the engine at `port`, sends `bridge.hello`, and asserts it
/// succeeds.  Returns the session id on success.
async fn handshake_on_port(port: u16) -> String {
    let url = format!("ws://127.0.0.1:{port}");
    let cfg = RetryConfig {
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(500),
        max_elapsed: Duration::from_secs(5),
        jitter: false,
    };
    let handle = connect_with_retry(&url, &cfg)
        .await
        .unwrap_or_else(|e| panic!("connect_with_retry failed for {url}: {e}"));

    let response =
        tokio::time::timeout(RECV_TIMEOUT, handle.request(hello_envelope(), RECV_TIMEOUT))
            .await
            .unwrap_or_else(|_| panic!("bridge.hello timed out for {url}"))
            .unwrap_or_else(|e: RequestError| panic!("bridge.hello request error for {url}: {e}"));

    let result = match response {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(err) => {
            panic!("bridge.hello returned engine error for {url}: {err:?}")
        }
    };

    let outcome = parse_hello_result(&result)
        .unwrap_or_else(|e| panic!("parse_hello_result failed for {url}: {e}"));

    handle.shutdown().await;
    outcome.session_id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_launch_kill_relaunch_contract() {
    // Opt-in gate: without NORVES_ENGINE_PATH set (or when the path is not a
    // file), skip and pass rather than fail.  This keeps `cargo test` green on
    // machines without the C++ build.
    let exe = match std::env::var("NORVES_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_launch_kill_relaunch_contract: \
                 set NORVES_ENGINE_PATH to the norves_mock_engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_launch_kill_relaunch_contract: \
             NORVES_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    // --- Launch 1: port injection + READY handshake ---

    // spawn_engine_on_free_port already asserts the READY line matches the
    // injected port, so a pass here proves the --bridge-port / READY contract.
    let (guard1, port1) = spawn_engine_on_free_port(&exe);

    let session_id_1 = handshake_on_port(port1).await;
    assert!(
        !session_id_1.is_empty(),
        "first launch: session_id must be non-empty"
    );
    eprintln!("[PASS] Launch 1: port={port1} session={session_id_1}");

    // --- Kill the first engine ---

    // Drop the guard (which calls kill + wait) to release the port.  Because
    // Windows holds the socket until the process is reaped, we must wait before
    // binding the second engine on any port; pick_free_port obtains a fresh
    // ephemeral port anyway, so this is belt-and-suspenders.
    drop(guard1);
    eprintln!("[PASS] Kill 1: first engine reaped");

    // --- Launch 2: relaunch on a fresh port ---

    let (guard2, port2) = spawn_engine_on_free_port(&exe);

    // The second engine must get a different port (highly probable with ephemeral
    // ports; not a hard invariant, but a useful consistency check).
    // We do not assert port2 != port1 because in theory the OS could reassign the
    // same ephemeral port -- just log for evidence.
    eprintln!("[INFO] Relaunch: port1={port1} port2={port2}");

    let session_id_2 = handshake_on_port(port2).await;
    assert!(
        !session_id_2.is_empty(),
        "second launch: session_id must be non-empty"
    );
    eprintln!("[PASS] Launch 2 (relaunch): port={port2} session={session_id_2}");

    // RAII guard kills + reaps the second engine on drop.
    drop(guard2);
    eprintln!("[PASS] Kill 2: second engine reaped -- relaunch contract verified");
}
