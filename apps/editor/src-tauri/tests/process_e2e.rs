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
use std::sync::{mpsc, Arc};
use std::time::Duration;

use norves_bridge_core::{
    CorrelationId, EngineState, MethodName, ResponsePayload, RuntimeState, ValidatedEnvelope,
    VersionString,
};
use norves_bridge_editor_client::{
    connect_with_retry, parse_hello_result, parse_log_message, parse_status_result, HelloParams,
    RequestError, RetryConfig,
};
use tokio::sync::broadcast;

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

/// Builds a generic request [`ValidatedEnvelope`] with the given `id`, `method`,
/// and optional params map. Reuses the same version string as [`hello_envelope`].
fn request_envelope(
    id: &str,
    method: &str,
    params: Option<serde_json::Map<String, serde_json::Value>>,
) -> ValidatedEnvelope {
    let version = VersionString::try_from("0.1".to_owned()).expect("0.1 is a valid version");
    ValidatedEnvelope::Request {
        version,
        id: CorrelationId::try_from(id.to_owned()).expect("non-empty id"),
        method: MethodName::try_from(method.to_owned()).expect("namespaced method"),
        params,
        session_id: None,
        seq: None,
    }
}

/// Sends `envelope` through `handle` and extracts the `serde_json::Value` result,
/// panicking with `step` in the message on any failure.
async fn send_and_expect_result(
    handle: &norves_bridge_editor_client::DispatchHandle,
    envelope: ValidatedEnvelope,
    step: &str,
) -> serde_json::Value {
    let response = tokio::time::timeout(RECV_TIMEOUT, handle.request(envelope, RECV_TIMEOUT))
        .await
        .unwrap_or_else(|_| panic!("[{step}] timed out waiting for response"))
        .unwrap_or_else(|e: RequestError| panic!("[{step}] request error: {e}"));
    match response {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(err) => {
            panic!("[{step}] engine returned protocol error: {err:?}")
        }
    }
}

/// Runtime control contract test against the real NorvesLib engine.
///
/// Opt-in via `NORVES_NORVESLIB_ENGINE_PATH`; skips (passes) when unset or
/// pointing at a non-file path. The env var is distinct from `NORVES_ENGINE_PATH`
/// (used by the mock engine) because the mock returns runtimeState=Edit and
/// never transitions, so play/pause/stop assertions would always fail against it.
///
/// Steps performed over a **single** persistent connection:
/// 1. bridge.hello   → session established
/// 2. engine.getStatus → engineState=Running, runtimeState=Edit
/// 3. runtime.play   → accepted=true
/// 4. engine.getStatus → runtimeState=Playing
/// 5. runtime.pause  → accepted=true
/// 6. engine.getStatus → runtimeState=Paused
/// 7. runtime.stop   → accepted=true
/// 8. engine.getStatus → runtimeState=Stopped
/// 9. runtime.focusViewport → focused key present (bool, value not fixed)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_runtime_control_contract() {
    // Opt-in gate: set NORVES_NORVESLIB_ENGINE_PATH to the real engine binary.
    let exe = match std::env::var("NORVES_NORVESLIB_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_runtime_control_contract: \
                 set NORVES_NORVESLIB_ENGINE_PATH to the NorvesLib engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_runtime_control_contract: \
             NORVES_NORVESLIB_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    // Launch the real engine and wait for its READY <port> line.
    let (_guard, port) = spawn_engine_on_free_port(&exe);

    let url = format!("ws://127.0.0.1:{port}");
    let cfg = RetryConfig {
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(500),
        max_elapsed: Duration::from_secs(5),
        jitter: false,
    };

    // Open a single persistent connection; do NOT shut it down between steps.
    let handle = connect_with_retry(&url, &cfg)
        .await
        .unwrap_or_else(|e| panic!("connect_with_retry failed for {url}: {e}"));

    // --- Step 1: bridge.hello ---
    let hello_value = send_and_expect_result(&handle, hello_envelope(), "bridge.hello").await;
    let hello_outcome = parse_hello_result(&hello_value)
        .unwrap_or_else(|e| panic!("[bridge.hello] parse_hello_result failed: {e}"));
    assert!(
        !hello_outcome.session_id.is_empty(),
        "[bridge.hello] session_id must be non-empty"
    );
    eprintln!(
        "[PASS] bridge.hello: session_id={}",
        hello_outcome.session_id
    );

    // --- Step 2: engine.getStatus (initial: Edit) ---
    let status_value = send_and_expect_result(
        &handle,
        request_envelope("rc-status-1", "engine.getStatus", None),
        "engine.getStatus#1",
    )
    .await;
    let snapshot = parse_status_result(&status_value)
        .unwrap_or_else(|e| panic!("[engine.getStatus#1] parse_status_result failed: {e}"));
    assert_eq!(
        snapshot.engine_state,
        EngineState::Running,
        "[engine.getStatus#1] expected engineState=Running, got {:?}",
        snapshot.engine_state
    );
    assert_eq!(
        snapshot.runtime_state,
        RuntimeState::Edit,
        "[engine.getStatus#1] expected runtimeState=Edit, got {:?}",
        snapshot.runtime_state
    );
    eprintln!(
        "[PASS] engine.getStatus#1: engineState={:?} runtimeState={:?}",
        snapshot.engine_state, snapshot.runtime_state
    );

    // --- Step 3: runtime.play ---
    let play_value = send_and_expect_result(
        &handle,
        request_envelope("rc-play", "runtime.play", None),
        "runtime.play",
    )
    .await;
    assert_eq!(
        play_value["accepted"].as_bool(),
        Some(true),
        "[runtime.play] expected accepted=true, got: {}",
        play_value
    );
    eprintln!("[PASS] runtime.play: accepted=true");

    // --- Step 4: engine.getStatus (Playing) ---
    let status_value = send_and_expect_result(
        &handle,
        request_envelope("rc-status-2", "engine.getStatus", None),
        "engine.getStatus#2",
    )
    .await;
    let snapshot = parse_status_result(&status_value)
        .unwrap_or_else(|e| panic!("[engine.getStatus#2] parse_status_result failed: {e}"));
    assert_eq!(
        snapshot.runtime_state,
        RuntimeState::Playing,
        "[engine.getStatus#2] expected runtimeState=Playing, got {:?}",
        snapshot.runtime_state
    );
    eprintln!("[PASS] engine.getStatus#2: runtimeState=Playing");

    // --- Step 5: runtime.pause ---
    let pause_value = send_and_expect_result(
        &handle,
        request_envelope("rc-pause", "runtime.pause", None),
        "runtime.pause",
    )
    .await;
    assert_eq!(
        pause_value["accepted"].as_bool(),
        Some(true),
        "[runtime.pause] expected accepted=true, got: {}",
        pause_value
    );
    eprintln!("[PASS] runtime.pause: accepted=true");

    // --- Step 6: engine.getStatus (Paused) ---
    let status_value = send_and_expect_result(
        &handle,
        request_envelope("rc-status-3", "engine.getStatus", None),
        "engine.getStatus#3",
    )
    .await;
    let snapshot = parse_status_result(&status_value)
        .unwrap_or_else(|e| panic!("[engine.getStatus#3] parse_status_result failed: {e}"));
    assert_eq!(
        snapshot.runtime_state,
        RuntimeState::Paused,
        "[engine.getStatus#3] expected runtimeState=Paused, got {:?}",
        snapshot.runtime_state
    );
    eprintln!("[PASS] engine.getStatus#3: runtimeState=Paused");

    // --- Step 7: runtime.stop ---
    let stop_value = send_and_expect_result(
        &handle,
        request_envelope("rc-stop", "runtime.stop", None),
        "runtime.stop",
    )
    .await;
    assert_eq!(
        stop_value["accepted"].as_bool(),
        Some(true),
        "[runtime.stop] expected accepted=true, got: {}",
        stop_value
    );
    eprintln!("[PASS] runtime.stop: accepted=true");

    // --- Step 8: engine.getStatus (Stopped) ---
    let status_value = send_and_expect_result(
        &handle,
        request_envelope("rc-status-4", "engine.getStatus", None),
        "engine.getStatus#4",
    )
    .await;
    let snapshot = parse_status_result(&status_value)
        .unwrap_or_else(|e| panic!("[engine.getStatus#4] parse_status_result failed: {e}"));
    assert_eq!(
        snapshot.runtime_state,
        RuntimeState::Stopped,
        "[engine.getStatus#4] expected runtimeState=Stopped, got {:?}",
        snapshot.runtime_state
    );
    eprintln!("[PASS] engine.getStatus#4: runtimeState=Stopped");

    // --- Step 9: runtime.focusViewport ---
    // Value is not fixed: headless / focus-competition may yield false.
    // Only key existence and bool type are asserted.
    let focus_value = send_and_expect_result(
        &handle,
        request_envelope("rc-focus", "runtime.focusViewport", None),
        "runtime.focusViewport",
    )
    .await;
    assert!(
        focus_value["focused"].as_bool().is_some(),
        "[runtime.focusViewport] expected 'focused' key with bool value, got: {}",
        focus_value
    );
    eprintln!(
        "[PASS] runtime.focusViewport: focused={:?}",
        focus_value["focused"].as_bool()
    );

    // Orderly shutdown of the connection.
    handle.shutdown().await;
    eprintln!("[PASS] engine_runtime_control_contract: all steps passed");
    // _guard drops here, killing the engine process.
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

// ---------------------------------------------------------------------------
// Helpers for event streaming tests
// ---------------------------------------------------------------------------

/// Waits for the next broadcast event whose `event` name matches `name`,
/// skipping non-matching events. Returns the `params` map on success, or
/// `None` when `timeout` elapses before a matching event arrives.
///
/// `Lagged` errors (slow consumer fell behind the broadcast ring) are treated
/// as advisory: the receiver resumes from the next available event and keeps
/// waiting. `Closed` (broadcaster gone) breaks the loop and returns `None`.
async fn wait_for_event(
    events: &mut broadcast::Receiver<Arc<ValidatedEnvelope>>,
    name: &str,
    timeout: Duration,
) -> Option<Option<serde_json::Map<String, serde_json::Value>>> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match tokio::time::timeout(remaining, events.recv()).await {
            Err(_elapsed) => return None,
            Ok(Ok(env)) => {
                if let ValidatedEnvelope::Event { event, params, .. } = &*env {
                    if event.as_str() == name {
                        return Some(params.clone());
                    }
                }
                // Non-matching event: keep waiting.
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                // Fell behind; resume reading from next available slot.
                continue;
            }
            Ok(Err(broadcast::error::RecvError::Closed)) => return None,
        }
    }
}

// ---------------------------------------------------------------------------
// engine_event_streaming_contract
// ---------------------------------------------------------------------------

/// Server-initiated event streaming contract against the real NorvesLib engine.
///
/// Opt-in via `NORVES_NORVESLIB_ENGINE_PATH`; skips (passes) when unset or
/// pointing at a non-file path.  The env var is the same as
/// `engine_runtime_control_contract`.
///
/// Steps:
///  1. Spawn real engine, wait for READY.
///  2. connect_with_retry -> subscribe_events() BEFORE bridge.hello so no
///     event is missed.
///  3. bridge.hello -> session established.
///  4. log.subscribe (params: None -> empty object params allowed by schema)
///     -> subscriptionId non-empty string.
///  5. runtime.play -> accepted=true.
///  6. Wait for runtime.stateChanged event; assert params["state"]=="playing".
///  7. Wait for log.message event; parse with parse_log_message, assert
///     message non-empty and level is a valid LogLevel.
///  8. log.unsubscribe (params: {subscriptionId}) -> ok=true.
///  9. handle.shutdown().
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_event_streaming_contract() {
    // --- Opt-in gate ---
    let exe = match std::env::var("NORVES_NORVESLIB_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_event_streaming_contract: \
                 set NORVES_NORVESLIB_ENGINE_PATH to the NorvesLib engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_event_streaming_contract: \
             NORVES_NORVESLIB_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    // How long each event wait is allowed to take.
    const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

    // --- Step 1: spawn and wait for READY ---
    let (_guard, port) = spawn_engine_on_free_port(&exe);
    eprintln!("[PASS] engine_event_streaming_contract: READY port={port}");

    let url = format!("ws://127.0.0.1:{port}");
    let cfg = RetryConfig {
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(500),
        max_elapsed: Duration::from_secs(5),
        jitter: false,
    };

    // --- Step 2: connect and subscribe to events BEFORE hello ---
    let handle = connect_with_retry(&url, &cfg)
        .await
        .unwrap_or_else(|e| panic!("connect_with_retry failed for {url}: {e}"));

    // Subscribe before sending any request so no event can be lost.
    let mut events = handle.subscribe_events();

    // --- Step 3: bridge.hello ---
    let hello_value = send_and_expect_result(&handle, hello_envelope(), "bridge.hello").await;
    let hello_outcome = parse_hello_result(&hello_value)
        .unwrap_or_else(|e| panic!("[bridge.hello] parse_hello_result failed: {e}"));
    assert!(
        !hello_outcome.session_id.is_empty(),
        "[bridge.hello] session_id must be non-empty"
    );
    eprintln!(
        "[PASS] bridge.hello: session_id={}",
        hello_outcome.session_id
    );

    // --- Step 4: log.subscribe ---
    // Schema: params object is optional (all properties optional), so we send
    // an empty params map (Some({})) to satisfy "type": "object" while
    // providing no filter — equivalent to "subscribe to everything".
    let log_sub_value = send_and_expect_result(
        &handle,
        request_envelope("es-log-sub", "log.subscribe", Some(serde_json::Map::new())),
        "log.subscribe",
    )
    .await;
    let subscription_id = log_sub_value["subscriptionId"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("[log.subscribe] expected non-empty subscriptionId string, got: {log_sub_value}")
        })
        .to_owned();
    assert!(
        !subscription_id.is_empty(),
        "[log.subscribe] subscriptionId must be non-empty"
    );
    eprintln!("[PASS] log.subscribe: subscriptionId={subscription_id}");

    // --- Step 5: runtime.play ---
    let play_value = send_and_expect_result(
        &handle,
        request_envelope("es-play", "runtime.play", None),
        "runtime.play",
    )
    .await;
    assert_eq!(
        play_value["accepted"].as_bool(),
        Some(true),
        "[runtime.play] expected accepted=true, got: {play_value}"
    );
    eprintln!("[PASS] runtime.play: accepted=true");

    // --- Step 6: wait for runtime.stateChanged ---
    let state_changed_params = wait_for_event(&mut events, "runtime.stateChanged", EVENT_TIMEOUT)
        .await
        .unwrap_or_else(|| {
            panic!("[runtime.stateChanged] event did not arrive within {EVENT_TIMEOUT:?}")
        });
    let params_map =
        state_changed_params.unwrap_or_else(|| panic!("[runtime.stateChanged] params was None"));
    let state_val = params_map.get("state").unwrap_or_else(|| {
        panic!("[runtime.stateChanged] params missing 'state' key: {params_map:?}")
    });
    assert_eq!(
        state_val.as_str(),
        Some("playing"),
        "[runtime.stateChanged] expected state=playing, got: {state_val}"
    );
    eprintln!("[PASS] runtime.stateChanged: state=playing");

    // --- Step 7: wait for log.message ---
    let log_params = wait_for_event(&mut events, "log.message", EVENT_TIMEOUT)
        .await
        .unwrap_or_else(|| panic!("[log.message] event did not arrive within {EVENT_TIMEOUT:?}"));
    let log_params_map = log_params.unwrap_or_else(|| panic!("[log.message] params was None"));
    let log_value = serde_json::Value::Object(log_params_map);
    let log_msg = parse_log_message(&log_value)
        .unwrap_or_else(|e| panic!("[log.message] parse_log_message failed: {e}"));
    assert!(
        !log_msg.message.is_empty(),
        "[log.message] message must be non-empty"
    );
    // LogLevel is a closed enum (Trace/Debug/Info/Warn/Error); parse success
    // already proves the level is one of the five valid values.
    eprintln!(
        "[PASS] log.message: level={:?} message={:?}",
        log_msg.level, log_msg.message
    );

    // --- Step 8: log.unsubscribe ---
    // Schema: subscriptionId is required in params.
    let mut unsub_params = serde_json::Map::new();
    unsub_params.insert(
        "subscriptionId".to_owned(),
        serde_json::Value::String(subscription_id.clone()),
    );
    let unsub_value = send_and_expect_result(
        &handle,
        request_envelope("es-log-unsub", "log.unsubscribe", Some(unsub_params)),
        "log.unsubscribe",
    )
    .await;
    assert_eq!(
        unsub_value["ok"].as_bool(),
        Some(true),
        "[log.unsubscribe] expected ok=true, got: {unsub_value}"
    );
    eprintln!("[PASS] log.unsubscribe: ok=true");

    // --- Step 9: orderly shutdown ---
    handle.shutdown().await;
    eprintln!("[PASS] engine_event_streaming_contract: all steps passed");
    // _guard drops here, killing the engine process.
}
