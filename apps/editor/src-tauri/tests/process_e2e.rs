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
    connect_with_retry, parse_hello_result, parse_log_message, parse_object_snapshot_result,
    parse_scene_tree_result, parse_schema_snapshot_result, parse_set_property_result,
    parse_status_result, parse_thumbnail_result, HelloParams, RequestError, RetryConfig,
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
    let version = VersionString::try_from("0.2".to_owned()).expect("0.2 is a valid version");
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
    let version = VersionString::try_from("0.2".to_owned()).expect("0.2 is a valid version");
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

/// scene.getTree round-trip contract against the mock engine.
///
/// Opt-in via `NORVES_ENGINE_PATH` (the mock engine path; distinct from
/// `NORVES_NORVESLIB_ENGINE_PATH` used by the runtime-control test and from
/// `NORVES_MOCK_ENGINE` used by the conformance runner). Skips (passes) when
/// unset or non-file so `cargo test` stays green without the C++ build.
///
/// Proves the read path the Outliner depends on: after `bridge.hello`, a
/// `scene.getTree` request returns a result whose `{ root }` shape parses via
/// `parse_scene_tree_result` and matches the mock's static demo tree
/// (Root -> NodeA / GroupNode -> NodeB), value-equal to the spec fixture.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_scene_get_tree_contract() {
    let exe = match std::env::var("NORVES_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_scene_get_tree_contract: \
                 set NORVES_ENGINE_PATH to the norves_mock_engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_scene_get_tree_contract: NORVES_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    let (_guard, port) = spawn_engine_on_free_port(&exe);
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

    // bridge.hello first (establishes the session).
    let hello_value = send_and_expect_result(&handle, hello_envelope(), "bridge.hello").await;
    parse_hello_result(&hello_value)
        .unwrap_or_else(|e| panic!("[bridge.hello] parse_hello_result failed: {e}"));

    // scene.getTree round trip.
    let tree_value = send_and_expect_result(
        &handle,
        request_envelope("sc-tree", "scene.getTree", Some(serde_json::Map::new())),
        "scene.getTree",
    )
    .await;
    let tree = parse_scene_tree_result(&tree_value)
        .unwrap_or_else(|e| panic!("[scene.getTree] parse_scene_tree_result failed: {e}"));

    // Mock demo tree: Root (n-0) -> [NodeA (n-1), GroupNode (n-2) -> NodeB (n-3)].
    assert_eq!(tree.root.id, "n-0", "[scene.getTree] unexpected root id");
    assert_eq!(tree.root.name.as_deref(), Some("Root"));
    assert_eq!(
        tree.root.children.len(),
        2,
        "[scene.getTree] expected two top-level children, got {}",
        tree.root.children.len()
    );
    assert_eq!(tree.root.children[0].id, "n-1");
    assert_eq!(tree.root.children[1].id, "n-2");
    assert_eq!(
        tree.root.children[1].children.len(),
        1,
        "[scene.getTree] GroupNode should have one child"
    );
    assert_eq!(tree.root.children[1].children[0].id, "n-3");
    eprintln!(
        "[PASS] scene.getTree: root={} with nested demo tree",
        tree.root.id
    );

    handle.shutdown().await;
    eprintln!("[PASS] engine_scene_get_tree_contract: all steps passed");
}

/// `viewport.getThumbnail` round-trip contract against the mock engine (Phase 7b).
///
/// Opt-in via `NORVES_ENGINE_PATH` (the mock engine path; distinct from
/// `NORVES_NORVESLIB_ENGINE_PATH` and from `NORVES_MOCK_ENGINE`). Skips (passes)
/// when unset or non-file so `cargo test` stays green without the C++ build.
///
/// Proves the large-payload pull path the Game View depends on: after
/// `bridge.hello`, a `viewport.getThumbnail` request returns a result whose
/// `{ imageBase64, mimeType, width?, height? }` shape parses via
/// `parse_thumbnail_result`, carries a PNG (mimeType == "image/png", base64 bytes
/// beginning with the PNG signature), and is value-equal to the mock's static
/// thumbnail / the spec fixture. The image is an inline base64 snapshot copy, not
/// a live engine pointer (docs/memory-buffer-policy.md: pull, PNG, max 640x360,
/// 256 KiB hard cap, <= 1 fps).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_viewport_get_thumbnail_contract() {
    let exe = match std::env::var("NORVES_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_viewport_get_thumbnail_contract: \
                 set NORVES_ENGINE_PATH to the norves_mock_engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_viewport_get_thumbnail_contract: \
             NORVES_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    let (_guard, port) = spawn_engine_on_free_port(&exe);
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

    // bridge.hello first (establishes the session).
    let hello_value = send_and_expect_result(&handle, hello_envelope(), "bridge.hello").await;
    parse_hello_result(&hello_value)
        .unwrap_or_else(|e| panic!("[bridge.hello] parse_hello_result failed: {e}"));

    // viewport.getThumbnail round trip, capped at the policy resolution (the mock
    // ignores the caps and returns its static 2x2 PNG, which is within them).
    let mut params = serde_json::Map::new();
    params.insert("maxWidth".to_owned(), serde_json::Value::from(640u32));
    params.insert("maxHeight".to_owned(), serde_json::Value::from(360u32));
    let thumb_value = send_and_expect_result(
        &handle,
        request_envelope("vp-thumb", "viewport.getThumbnail", Some(params)),
        "viewport.getThumbnail",
    )
    .await;
    let thumb = parse_thumbnail_result(&thumb_value)
        .unwrap_or_else(|e| panic!("[viewport.getThumbnail] parse_thumbnail_result failed: {e}"));

    // PNG format + a non-empty base64 snapshot (the PNG magic header base64-encodes
    // to the "iVBOR" prefix). Value-equal to the mock's static thumbnail.
    assert_eq!(
        thumb.mime_type, "image/png",
        "[viewport.getThumbnail] expected PNG mimeType"
    );
    assert!(
        thumb.image_base64.starts_with("iVBOR"),
        "[viewport.getThumbnail] imageBase64 should be a base64 PNG (got prefix {:?})",
        &thumb.image_base64[..thumb.image_base64.len().min(8)]
    );
    assert_eq!(thumb.width, Some(2), "[viewport.getThumbnail] width");
    assert_eq!(thumb.height, Some(2), "[viewport.getThumbnail] height");
    // The base64 image stays comfortably within the 256 KiB hard cap
    // (~342 KiB once base64-encoded); the mock's 2x2 PNG is ~100 bytes.
    assert!(
        thumb.image_base64.len() < 342 * 1024,
        "[viewport.getThumbnail] base64 image exceeded the documented cap"
    );

    handle.shutdown().await;
    eprintln!(
        "[PASS] engine_viewport_get_thumbnail_contract: mimeType={} base64_len={}",
        thumb.mime_type,
        thumb.image_base64.len()
    );
}

/// object.getSnapshot + schema.getSnapshot round-trip contract against the mock
/// engine.
///
/// Opt-in via `NORVES_ENGINE_PATH` (the mock engine path; distinct from
/// `NORVES_NORVESLIB_ENGINE_PATH` and from `NORVES_MOCK_ENGINE`). Skips (passes)
/// when unset or non-file so `cargo test` stays green without the C++ build.
///
/// Proves the read paths the Inspector depends on: after `bridge.hello`, an
/// `object.getSnapshot` request for `n-1` returns a property bag that parses via
/// `parse_object_snapshot_result` (covering scalar/array/object/null values), and
/// a `schema.getSnapshot` request returns type descriptors that parse via
/// `parse_schema_snapshot_result`. Both are value-equal to the spec fixtures.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_object_and_schema_snapshot_contract() {
    let exe = match std::env::var("NORVES_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_object_and_schema_snapshot_contract: \
                 set NORVES_ENGINE_PATH to the norves_mock_engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_object_and_schema_snapshot_contract: \
             NORVES_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    let (_guard, port) = spawn_engine_on_free_port(&exe);
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

    // bridge.hello first (establishes the session).
    let hello_value = send_and_expect_result(&handle, hello_envelope(), "bridge.hello").await;
    parse_hello_result(&hello_value)
        .unwrap_or_else(|e| panic!("[bridge.hello] parse_hello_result failed: {e}"));

    // object.getSnapshot round trip for the demo object n-1.
    let mut object_params = serde_json::Map::new();
    object_params.insert(
        "objectId".to_owned(),
        serde_json::Value::String("n-1".to_owned()),
    );
    let object_value = send_and_expect_result(
        &handle,
        request_envelope("obj-snap", "object.getSnapshot", Some(object_params)),
        "object.getSnapshot",
    )
    .await;
    let snapshot = parse_object_snapshot_result(&object_value)
        .unwrap_or_else(|e| panic!("[object.getSnapshot] parse failed: {e}"));
    assert_eq!(
        snapshot.object_id, "n-1",
        "[object.getSnapshot] unexpected objectId"
    );
    // The demo bag covers every property-value kind the Inspector renders.
    assert!(
        snapshot.properties.iter().any(|p| p.value.is_string()),
        "[object.getSnapshot] expected a string-valued property"
    );
    assert!(
        snapshot.properties.iter().any(|p| p.value.is_null()),
        "[object.getSnapshot] expected a null-valued property"
    );
    assert!(
        snapshot.properties.iter().any(|p| p.value.is_array()),
        "[object.getSnapshot] expected an array-valued property"
    );
    assert!(
        snapshot.properties.iter().any(|p| p.value.is_object()),
        "[object.getSnapshot] expected an object-valued property"
    );
    eprintln!(
        "[PASS] object.getSnapshot: objectId={} with {} properties",
        snapshot.object_id,
        snapshot.properties.len()
    );

    // schema.getSnapshot round trip.
    let schema_value = send_and_expect_result(
        &handle,
        request_envelope(
            "schema-snap",
            "schema.getSnapshot",
            Some(serde_json::Map::new()),
        ),
        "schema.getSnapshot",
    )
    .await;
    let schema = parse_schema_snapshot_result(&schema_value)
        .unwrap_or_else(|e| panic!("[schema.getSnapshot] parse failed: {e}"));
    assert!(
        !schema.types.is_empty(),
        "[schema.getSnapshot] expected at least one type descriptor"
    );
    assert_eq!(schema.types[0].type_name, "TypeA");
    eprintln!(
        "[PASS] schema.getSnapshot: {} type descriptor(s)",
        schema.types.len()
    );

    handle.shutdown().await;
    eprintln!("[PASS] engine_object_and_schema_snapshot_contract: all steps passed");
}

/// object.setProperty (write path) round-trip contract against the mock engine.
///
/// Opt-in via `NORVES_ENGINE_PATH` (the mock engine path; distinct from
/// `NORVES_MOCK_ENGINE`). Skips (passes) when unset or non-file so `cargo test`
/// stays green without the C++ build.
///
/// Proves the write path the Inspector depends on. After `bridge.hello`:
///   1. an `object.setProperty` for `n-1`/`fieldOfView=75` returns
///      `{accepted:true, appliedValue:75}` (the mock echoes the value);
///   2. a follow-up `object.getSnapshot` for `n-1` shows `fieldOfView` is now 75,
///      proving the mock's in-memory map was updated by the write;
///   3. a per-node `object.getSnapshot` for `n-2` (GroupNode) returns its own
///      additive demo property bag, proving the mock is per-objectId (not just
///      the single n-1 demo).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_object_set_property_contract() {
    let exe = match std::env::var("NORVES_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_object_set_property_contract: \
                 set NORVES_ENGINE_PATH to the norves_mock_engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_object_set_property_contract: NORVES_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    let (_guard, port) = spawn_engine_on_free_port(&exe);
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

    // bridge.hello first (establishes the session).
    let hello_value = send_and_expect_result(&handle, hello_envelope(), "bridge.hello").await;
    parse_hello_result(&hello_value)
        .unwrap_or_else(|e| panic!("[bridge.hello] parse_hello_result failed: {e}"));

    // 1. object.setProperty: write fieldOfView=75 on n-1.
    let mut set_params = serde_json::Map::new();
    set_params.insert(
        "objectId".to_owned(),
        serde_json::Value::String("n-1".to_owned()),
    );
    set_params.insert(
        "property".to_owned(),
        serde_json::Value::String("fieldOfView".to_owned()),
    );
    set_params.insert("value".to_owned(), serde_json::Value::from(75));
    let set_value = send_and_expect_result(
        &handle,
        request_envelope("obj-set", "object.setProperty", Some(set_params)),
        "object.setProperty",
    )
    .await;
    let ack = parse_set_property_result(&set_value)
        .unwrap_or_else(|e| panic!("[object.setProperty] parse failed: {e}"));
    assert!(ack.accepted, "[object.setProperty] expected accepted:true");
    assert_eq!(
        ack.applied_value,
        Some(serde_json::Value::from(75)),
        "[object.setProperty] expected appliedValue 75"
    );
    eprintln!("[PASS] object.setProperty: accepted with appliedValue 75");

    // 2. object.getSnapshot for n-1 now reflects the write (fieldOfView=75).
    let mut snap_params = serde_json::Map::new();
    snap_params.insert(
        "objectId".to_owned(),
        serde_json::Value::String("n-1".to_owned()),
    );
    let snap_value = send_and_expect_result(
        &handle,
        request_envelope("obj-after-set", "object.getSnapshot", Some(snap_params)),
        "object.getSnapshot",
    )
    .await;
    let snapshot = parse_object_snapshot_result(&snap_value)
        .unwrap_or_else(|e| panic!("[object.getSnapshot after set] parse failed: {e}"));
    let fov = snapshot
        .properties
        .iter()
        .find(|p| p.name == "fieldOfView")
        .unwrap_or_else(|| panic!("[object.getSnapshot after set] fieldOfView missing"));
    assert_eq!(
        fov.value,
        serde_json::Value::from(75),
        "[object.getSnapshot after set] fieldOfView should now be 75"
    );
    eprintln!("[PASS] object.getSnapshot after set: fieldOfView updated to 75");

    // 3. per-node getSnapshot: n-2 (GroupNode) returns its own additive demo bag.
    let mut group_params = serde_json::Map::new();
    group_params.insert(
        "objectId".to_owned(),
        serde_json::Value::String("n-2".to_owned()),
    );
    let group_value = send_and_expect_result(
        &handle,
        request_envelope("obj-n2", "object.getSnapshot", Some(group_params)),
        "object.getSnapshot",
    )
    .await;
    let group = parse_object_snapshot_result(&group_value)
        .unwrap_or_else(|e| panic!("[object.getSnapshot n-2] parse failed: {e}"));
    assert_eq!(group.object_id, "n-2", "[object.getSnapshot n-2] objectId");
    assert!(
        !group.properties.is_empty(),
        "[object.getSnapshot n-2] expected per-node demo properties"
    );
    eprintln!(
        "[PASS] object.getSnapshot n-2: per-node bag with {} properties",
        group.properties.len()
    );

    handle.shutdown().await;
    eprintln!("[PASS] engine_object_set_property_contract: all steps passed");
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

// ---------------------------------------------------------------------------
// engine_schema_snapshot_norveslib_contract
// ---------------------------------------------------------------------------

/// `schema.getSnapshot` round-trip contract against the real NorvesLib engine.
///
/// Opt-in via `NORVES_NORVESLIB_ENGINE_PATH`; skips (passes) when unset or
/// pointing at a non-file path.  The env var is shared with
/// `engine_runtime_control_contract` and `engine_event_streaming_contract`.
///
/// This test validates the schema pull path the Inspector depends on: after
/// `bridge.hello`, a `schema.getSnapshot` request returns type descriptors that
/// parse via `parse_schema_snapshot_result`.  Unlike the mock-engine test
/// `engine_object_and_schema_snapshot_contract`, no mock-specific type names
/// (e.g. "TypeA") are asserted — only generic structural invariants that hold
/// regardless of the concrete engine type registry.
///
/// Steps:
///  1. Spawn real engine, wait for READY.
///  2. connect_with_retry -> single persistent connection.
///  3. bridge.hello -> session established (session_id non-empty).
///  4. schema.getSnapshot (params: empty object) -> type descriptors parsed.
///  5. Assert types is non-empty (at least one type registered).
///  6. Assert every type_name is non-empty.
///  7. For each type that has properties, assert each property name and
///     value_type are non-empty.
///  8. handle.shutdown().
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_schema_snapshot_norveslib_contract() {
    // --- Opt-in gate ---
    let exe = match std::env::var("NORVES_NORVESLIB_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_schema_snapshot_norveslib_contract: \
                 set NORVES_NORVESLIB_ENGINE_PATH to the NorvesLib engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_schema_snapshot_norveslib_contract: \
             NORVES_NORVESLIB_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    // --- Step 1: spawn and wait for READY ---
    let (_guard, port) = spawn_engine_on_free_port(&exe);
    eprintln!("[PASS] engine_schema_snapshot_norveslib_contract: READY port={port}");

    let url = format!("ws://127.0.0.1:{port}");
    let cfg = RetryConfig {
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(500),
        max_elapsed: Duration::from_secs(5),
        jitter: false,
    };

    // --- Step 2: open a single persistent connection ---
    let handle = connect_with_retry(&url, &cfg)
        .await
        .unwrap_or_else(|e| panic!("connect_with_retry failed for {url}: {e}"));

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

    // --- Step 4: schema.getSnapshot ---
    let schema_value = send_and_expect_result(
        &handle,
        request_envelope("schema-snap", "schema.getSnapshot", Some(serde_json::Map::new())),
        "schema.getSnapshot",
    )
    .await;
    let schema = parse_schema_snapshot_result(&schema_value)
        .unwrap_or_else(|e| panic!("[schema.getSnapshot] parse_schema_snapshot_result failed: {e}"));

    // --- Step 5: at least one type must be registered ---
    assert!(
        !schema.types.is_empty(),
        "[schema.getSnapshot] expected at least one type descriptor, got empty types array"
    );

    // --- Step 6: every type_name must be non-empty ---
    for (i, td) in schema.types.iter().enumerate() {
        assert!(
            !td.type_name.is_empty(),
            "[schema.getSnapshot] types[{i}].type_name must be non-empty"
        );
    }

    // --- Step 7: for each type with properties, validate each property definition ---
    for (i, td) in schema.types.iter().enumerate() {
        for (j, prop_def) in td.properties.iter().enumerate() {
            assert!(
                !prop_def.name.is_empty(),
                "[schema.getSnapshot] types[{i}].properties[{j}].name must be non-empty"
            );
            assert!(
                !prop_def.value_type.is_empty(),
                "[schema.getSnapshot] types[{i}].properties[{j}].value_type must be non-empty"
            );
        }
    }

    // --- Step 8: orderly shutdown ---
    handle.shutdown().await;
    eprintln!(
        "[PASS] engine_schema_snapshot_norveslib_contract: {} type descriptor(s)",
        schema.types.len()
    );
    // _guard drops here, killing the engine process.
}

// ---------------------------------------------------------------------------
// engine_launch_info_schema_compliance_contract
// ---------------------------------------------------------------------------

/// Schema compliance contract for `engine.launchInfo` against the real NorvesLib engine.
///
/// Opt-in via `NORVES_NORVESLIB_ENGINE_PATH`; skips (passes) when unset or
/// pointing at a non-file path.  The env var is shared with
/// `engine_runtime_control_contract` and `engine_event_streaming_contract`.
///
/// This test explicitly contrasts the NorvesLib adapter's schema-compliant
/// `{pid, title}` result against the reference mock engine's non-compliant
/// `{launched: true}` response, verifying that the adapter returns
/// `additionalProperties:false` compliant output.
///
/// Steps:
///  1. Spawn real engine, wait for READY.
///  2. connect_with_retry -> single persistent connection.
///  3. bridge.hello -> session established (session_id non-empty).
///  4. engine.launchInfo (params: None) -> result with pid and title.
///  5. Assert pid is Some(i64) >= 0 (NorvesLib returns GetCurrentProcessId).
///  6. Assert title is Some(&str) and non-empty (no hardcoded value).
///  7. Assert "launched" key is absent (mock non-compliance artifact).
///  8. handle.shutdown().
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_launch_info_schema_compliance_contract() {
    // Opt-in gate: set NORVES_NORVESLIB_ENGINE_PATH to the real engine binary.
    let exe = match std::env::var("NORVES_NORVESLIB_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_launch_info_schema_compliance_contract: \
                 set NORVES_NORVESLIB_ENGINE_PATH to the NorvesLib engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_launch_info_schema_compliance_contract: \
             NORVES_NORVESLIB_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    // --- Step 1: spawn and wait for READY ---
    let (_guard, port) = spawn_engine_on_free_port(&exe);
    eprintln!("[PASS] engine_launch_info_schema_compliance_contract: READY port={port}");

    let url = format!("ws://127.0.0.1:{port}");
    let cfg = RetryConfig {
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(500),
        max_elapsed: Duration::from_secs(5),
        jitter: false,
    };

    // --- Step 2: open a single persistent connection ---
    let handle = connect_with_retry(&url, &cfg)
        .await
        .unwrap_or_else(|e| panic!("connect_with_retry failed for {url}: {e}"));

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

    // --- Step 4: engine.launchInfo ---
    // params schema has no properties and additionalProperties:false, so params=None is correct.
    let launch_info_value = send_and_expect_result(
        &handle,
        request_envelope("li-1", "engine.launchInfo", None),
        "engine.launchInfo",
    )
    .await;

    // --- Step 5: pid must be present and >= 0 ---
    // NorvesLib adapter returns GetCurrentProcessId() which is always a non-negative integer.
    let pid = launch_info_value["pid"].as_i64().unwrap_or_else(|| {
        panic!(
            "[engine.launchInfo] expected 'pid' as integer, got: {}",
            launch_info_value
        )
    });
    assert!(pid >= 0, "[engine.launchInfo] pid must be >= 0, got: {pid}");

    // --- Step 6: title must be present and non-empty ---
    // The exact string is engine-specific ("NorvesLib Game" or similar); we do not hardcode it.
    let title = launch_info_value["title"].as_str().unwrap_or_else(|| {
        panic!(
            "[engine.launchInfo] expected 'title' as string, got: {}",
            launch_info_value
        )
    });
    assert!(
        !title.is_empty(),
        "[engine.launchInfo] title must be non-empty"
    );

    // --- Step 7: "launched" key must be absent ---
    // The reference mock engine (norves_mock_engine) returns {launched: true}, which violates
    // additionalProperties:false in the schema. The NorvesLib adapter must NOT include this key.
    // Asserting its absence proves schema compliance and documents the mock/real contrast.
    assert!(
        launch_info_value.get("launched").is_none(),
        "[engine.launchInfo] 'launched' key must be absent (schema: additionalProperties:false); \
         got: {}",
        launch_info_value
    );

    eprintln!("[PASS] engine.launchInfo: pid={pid} title={title:?}");

    // --- Step 8: orderly shutdown ---
    handle.shutdown().await;
    eprintln!("[PASS] engine_launch_info_schema_compliance_contract: all steps passed");
    // _guard drops here, killing the engine process.
}

// ---------------------------------------------------------------------------
// engine_scene_object_norveslib_contract
// ---------------------------------------------------------------------------

/// `scene.getTree` + `object.getSnapshot` type-fidelity contract against the
/// real NorvesLib engine.
///
/// Opt-in via `NORVES_NORVESLIB_ENGINE_PATH`; skips (passes) when unset or
/// pointing at a non-file path.  The env var is shared with
/// `engine_runtime_control_contract`, `engine_event_streaming_contract`, and
/// `engine_schema_snapshot_norveslib_contract`.
///
/// NorvesLib starts in Edit runtime state and its GameMode(Rendering3DTest)
/// populates the World (Sphere / Ground / Light entities) within the first few
/// frames after READY. Because entity creation is asynchronous relative to
/// the Bridge READY signal, `scene.getTree` is polled (200 ms interval, up to
/// ~8 s) until the root node has at least one numeric-id child.
///
/// Steps:
///  1. Spawn real engine, wait for READY.
///  2. connect_with_retry -> single persistent connection.
///  3. bridge.hello -> session established (session_id non-empty).
///  4. Poll scene.getTree until root.children is non-empty (≤ 8 s, 200 ms step).
///     Timeout is a hard failure; actual tree content is printed to stderr.
///  5. Assert root.id == "scene-root"; assert root.children non-empty.
///  6. Locate the first Entity node (numeric id) anywhere in the tree.
///  7. object.getSnapshot for that entity id.
///  8. Assert snapshot.object_id == entity id; properties non-empty.
///  9. Type-fidelity assertions: at least one number-valued property, at least
///     one boolean-valued property, at least one array-valued property.
/// 10. handle.shutdown().
///
/// Finds the first node whose id parses as a u64 (engine Entity ids are
/// unsigned 64-bit integers serialised as decimal strings on the wire).
fn find_first_numeric_id_node(node: &norves_bridge_editor_client::SceneNode) -> Option<String> {
    if node.id.parse::<u64>().is_ok() {
        return Some(node.id.clone());
    }
    for child in &node.children {
        if let Some(id) = find_first_numeric_id_node(child) {
            return Some(id);
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_scene_object_norveslib_contract() {
    // --- Opt-in gate ---
    let exe = match std::env::var("NORVES_NORVESLIB_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_scene_object_norveslib_contract: \
                 set NORVES_NORVESLIB_ENGINE_PATH to the NorvesLib engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_scene_object_norveslib_contract: \
             NORVES_NORVESLIB_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    // --- Step 1: spawn and wait for READY ---
    let (_guard, port) = spawn_engine_on_free_port(&exe);
    eprintln!("[PASS] engine_scene_object_norveslib_contract: READY port={port}");

    let url = format!("ws://127.0.0.1:{port}");
    let cfg = RetryConfig {
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(500),
        max_elapsed: Duration::from_secs(5),
        jitter: false,
    };

    // --- Step 2: open a single persistent connection ---
    let handle = connect_with_retry(&url, &cfg)
        .await
        .unwrap_or_else(|e| panic!("connect_with_retry failed for {url}: {e}"));

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

    // --- Step 4: poll scene.getTree until root.children is non-empty ---
    // NorvesLib entity creation happens asynchronously in the first few frames
    // after READY; allow up to ~8 s (40 x 200 ms) for them to appear.
    const POLL_INTERVAL: Duration = Duration::from_millis(200);
    const POLL_MAX_ATTEMPTS: u32 = 40; // 40 * 200ms = 8000ms

    let mut last_tree = None;
    let mut tree_with_children = None;

    for attempt in 1..=POLL_MAX_ATTEMPTS {
        let tree_value = send_and_expect_result(
            &handle,
            request_envelope("sc-tree", "scene.getTree", Some(serde_json::Map::new())),
            &format!("scene.getTree#poll{attempt}"),
        )
        .await;
        let tree = parse_scene_tree_result(&tree_value)
            .unwrap_or_else(|e| panic!("[scene.getTree] parse_scene_tree_result failed: {e}"));

        eprintln!(
            "[POLL] scene.getTree attempt {attempt}/{POLL_MAX_ATTEMPTS}: \
             root.id={:?} children={}",
            tree.root.id,
            tree.root.children.len()
        );

        if !tree.root.children.is_empty() {
            tree_with_children = Some(tree);
            break;
        }

        last_tree = Some(tree);
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    let tree = match tree_with_children {
        Some(t) => t,
        None => {
            // Timeout: dump what we last received and fail with context.
            let last = last_tree.expect("at least one poll must have completed");
            eprintln!(
                "[TIMEOUT] scene.getTree: root.id={:?} children={} after ~8 s. \
                 Full root.children (first 5): {:?}",
                last.root.id,
                last.root.children.len(),
                &last.root.children[..last.root.children.len().min(5)]
            );
            panic!(
                "[scene.getTree] root.children was empty after {POLL_MAX_ATTEMPTS} polls \
                 (~8 s). The engine world may not have populated entities in time."
            );
        }
    };

    // --- Step 5: assert root invariants ---
    assert_eq!(
        tree.root.id, "scene-root",
        "[scene.getTree] expected root.id=scene-root, got {:?}",
        tree.root.id
    );
    assert!(
        !tree.root.children.is_empty(),
        "[scene.getTree] root.children must be non-empty"
    );
    eprintln!(
        "[PASS] scene.getTree: root.id={} root.children={}",
        tree.root.id,
        tree.root.children.len()
    );

    // --- Step 6: locate first Entity node (numeric id) ---
    let entity_id = find_first_numeric_id_node(&tree.root).unwrap_or_else(|| {
        panic!(
            "[scene.getTree] no node with a numeric (Entity) id found in the tree. \
             Root children: {:?}",
            tree.root
                .children
                .iter()
                .map(|n| &n.id)
                .collect::<Vec<_>>()
        )
    });
    eprintln!("[PASS] scene.getTree: first numeric-id entity={entity_id}");

    // --- Step 7: object.getSnapshot for the entity ---
    let mut obj_params = serde_json::Map::new();
    obj_params.insert(
        "objectId".to_owned(),
        serde_json::Value::String(entity_id.clone()),
    );
    let obj_value = send_and_expect_result(
        &handle,
        request_envelope("obj-snap", "object.getSnapshot", Some(obj_params)),
        "object.getSnapshot",
    )
    .await;
    let snapshot = parse_object_snapshot_result(&obj_value)
        .unwrap_or_else(|e| panic!("[object.getSnapshot] parse_object_snapshot_result failed: {e}"));

    // --- Step 8: basic snapshot invariants ---
    assert_eq!(
        snapshot.object_id, entity_id,
        "[object.getSnapshot] expected objectId={entity_id}, got {:?}",
        snapshot.object_id
    );
    assert!(
        !snapshot.properties.is_empty(),
        "[object.getSnapshot] properties must be non-empty for entity {entity_id}"
    );
    eprintln!(
        "[PASS] object.getSnapshot: objectId={} properties={}",
        snapshot.object_id,
        snapshot.properties.len()
    );

    // --- Step 9: type-fidelity assertions ---
    // SerializedValue -> wire JSON type mapping must be correct:
    //   ObjectId (u64)  -> JSON number
    //   bActive (bool)  -> JSON boolean
    //   Position/Vector3 -> JSON array
    let has_number = snapshot.properties.iter().any(|p| p.value.is_number());
    let has_boolean = snapshot.properties.iter().any(|p| p.value.is_boolean());
    let has_array = snapshot.properties.iter().any(|p| p.value.is_array());

    // Diagnostic: print each property name + value kind to aid debugging on failure.
    for p in &snapshot.properties {
        let kind = if p.value.is_number() {
            "number"
        } else if p.value.is_boolean() {
            "boolean"
        } else if p.value.is_string() {
            "string"
        } else if p.value.is_array() {
            "array"
        } else if p.value.is_object() {
            "object"
        } else {
            "null"
        };
        eprintln!("  property: name={:?} kind={kind} value={}", p.name, p.value);
    }

    assert!(
        has_number,
        "[object.getSnapshot] expected at least one number-valued property \
         (e.g. ObjectId); entity={entity_id}"
    );
    assert!(
        has_boolean,
        "[object.getSnapshot] expected at least one boolean-valued property \
         (e.g. bActive); entity={entity_id}"
    );
    assert!(
        has_array,
        "[object.getSnapshot] expected at least one array-valued property \
         (e.g. Position=Vector3); entity={entity_id}"
    );

    eprintln!(
        "[PASS] type-fidelity: entity={entity_id} \
         has_number={has_number} has_boolean={has_boolean} has_array={has_array}"
    );

    // --- Step 10: orderly shutdown ---
    handle.shutdown().await;
    eprintln!(
        "[PASS] engine_scene_object_norveslib_contract: \
         root_children={} entity_id={entity_id} properties={} \
         number={has_number} boolean={has_boolean} array={has_array}",
        tree.root.children.len(),
        snapshot.properties.len()
    );
    // _guard drops here, killing the engine process.
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_object_set_property_norveslib_contract() {
    // --- Opt-in gate ---
    let exe = match std::env::var("NORVES_NORVESLIB_ENGINE_PATH") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "[SKIP] engine_object_set_property_norveslib_contract: \
                 set NORVES_NORVESLIB_ENGINE_PATH to the NorvesLib engine executable to run this test"
            );
            return;
        }
    };

    if !std::path::Path::new(&exe).is_file() {
        eprintln!(
            "[SKIP] engine_object_set_property_norveslib_contract: \
             NORVES_NORVESLIB_ENGINE_PATH={exe:?} is not a file"
        );
        return;
    }

    // --- Step 1: spawn, connect, and bridge.hello ---
    let (_guard, port) = spawn_engine_on_free_port(&exe);
    eprintln!("[PASS] engine_object_set_property_norveslib_contract: READY port={port}");

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

    // --- Step 2: poll scene.getTree until root.children is non-empty ---
    const POLL_INTERVAL: Duration = Duration::from_millis(200);
    const POLL_MAX_ATTEMPTS: u32 = 40; // 40 * 200ms = 8000ms

    let mut last_tree = None;
    let mut tree_with_children = None;

    for attempt in 1..=POLL_MAX_ATTEMPTS {
        let tree_value = send_and_expect_result(
            &handle,
            request_envelope("set-sc-tree", "scene.getTree", Some(serde_json::Map::new())),
            &format!("scene.getTree#set-poll{attempt}"),
        )
        .await;
        let tree = parse_scene_tree_result(&tree_value)
            .unwrap_or_else(|e| panic!("[scene.getTree] parse_scene_tree_result failed: {e}"));

        eprintln!(
            "[POLL] scene.getTree setProperty attempt {attempt}/{POLL_MAX_ATTEMPTS}: \
             root.id={:?} children={}",
            tree.root.id,
            tree.root.children.len()
        );

        if !tree.root.children.is_empty() {
            tree_with_children = Some(tree);
            break;
        }

        last_tree = Some(tree);
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    let tree = match tree_with_children {
        Some(t) => t,
        None => {
            let last = last_tree.expect("at least one poll must have completed");
            eprintln!(
                "[TIMEOUT] scene.getTree: root.id={:?} children={} after ~8 s. \
                 Full root.children (first 5): {:?}",
                last.root.id,
                last.root.children.len(),
                &last.root.children[..last.root.children.len().min(5)]
            );
            panic!(
                "[scene.getTree] root.children was empty after {POLL_MAX_ATTEMPTS} polls \
                 (~8 s). The engine world may not have populated entities in time."
            );
        }
    };

    let entity_id = find_first_numeric_id_node(&tree.root).unwrap_or_else(|| {
        panic!(
            "[scene.getTree] no node with a numeric (Entity) id found in the tree. \
             Root children: {:?}",
            tree.root
                .children
                .iter()
                .map(|n| &n.id)
                .collect::<Vec<_>>()
        )
    });
    eprintln!("[PASS] scene.getTree: first numeric-id entity={entity_id}");

    // --- Step 3: object.getSnapshot and record Scale ---
    let mut obj_params = serde_json::Map::new();
    obj_params.insert(
        "objectId".to_owned(),
        serde_json::Value::String(entity_id.clone()),
    );
    let obj_value = send_and_expect_result(
        &handle,
        request_envelope("set-obj-snap", "object.getSnapshot", Some(obj_params)),
        "object.getSnapshot#before-set",
    )
    .await;
    let snapshot = parse_object_snapshot_result(&obj_value).unwrap_or_else(|e| {
        panic!("[object.getSnapshot before set] parse_object_snapshot_result failed: {e}")
    });
    assert_eq!(
        snapshot.object_id, entity_id,
        "[object.getSnapshot before set] expected objectId={entity_id}, got {:?}",
        snapshot.object_id
    );

    let scale = snapshot
        .properties
        .iter()
        .find(|p| p.name == "Scale" && p.value.as_array().map_or(false, |items| items.len() == 3))
        .unwrap_or_else(|| {
            let array3_properties = snapshot
                .properties
                .iter()
                .filter(|p| p.value.as_array().map_or(false, |items| items.len() == 3))
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>();
            panic!(
                "[object.getSnapshot before set] expected Scale as a 3-element array; \
                 available 3-element array properties: {:?}",
                array3_properties
            )
        });
    let original_scale = scale.value.clone();
    eprintln!("[PASS] object.getSnapshot before set: Scale={original_scale}");

    // --- Step 4: object.setProperty Scale=[2,3,4] and assert appliedValue ---
    let expected_scale = serde_json::json!([2, 3, 4]);
    let mut set_scale_params = serde_json::Map::new();
    set_scale_params.insert(
        "objectId".to_owned(),
        serde_json::Value::String(entity_id.clone()),
    );
    set_scale_params.insert(
        "property".to_owned(),
        serde_json::Value::String("Scale".to_owned()),
    );
    set_scale_params.insert("value".to_owned(), expected_scale.clone());
    let set_scale_value = send_and_expect_result(
        &handle,
        request_envelope(
            "set-scale",
            "object.setProperty",
            Some(set_scale_params),
        ),
        "object.setProperty#Scale",
    )
    .await;
    let scale_ack = parse_set_property_result(&set_scale_value)
        .unwrap_or_else(|e| panic!("[object.setProperty Scale] parse failed: {e}"));
    assert!(
        scale_ack.accepted,
        "[object.setProperty Scale] expected accepted:true, got: {}",
        set_scale_value
    );
    assert_eq!(
        scale_ack.applied_value.as_ref(),
        Some(&expected_scale),
        "[object.setProperty Scale] expected appliedValue [2,3,4], got: {}",
        set_scale_value
    );
    eprintln!("[PASS] object.setProperty Scale: accepted with appliedValue={expected_scale}");

    // --- Step 5: object.getSnapshot reflects Scale=[2,3,4] ---
    let mut after_scale_params = serde_json::Map::new();
    after_scale_params.insert(
        "objectId".to_owned(),
        serde_json::Value::String(entity_id.clone()),
    );
    let after_scale_value = send_and_expect_result(
        &handle,
        request_envelope(
            "set-obj-after-scale",
            "object.getSnapshot",
            Some(after_scale_params),
        ),
        "object.getSnapshot#after-Scale",
    )
    .await;
    let snapshot_after_scale = parse_object_snapshot_result(&after_scale_value).unwrap_or_else(|e| {
        panic!("[object.getSnapshot after Scale] parse_object_snapshot_result failed: {e}")
    });
    let scale_after = snapshot_after_scale
        .properties
        .iter()
        .find(|p| p.name == "Scale")
        .unwrap_or_else(|| panic!("[object.getSnapshot after Scale] Scale missing"));
    assert_eq!(
        &scale_after.value, &expected_scale,
        "[object.getSnapshot after Scale] Scale should be [2,3,4]"
    );
    let confirmed_scale = scale_after.value.clone();
    eprintln!("[PASS] object.getSnapshot after Scale: Scale={confirmed_scale}");

    // --- Step 6: bool property round-trip when one is exposed ---
    let mut bool_summary = "skipped:no_bool_property".to_owned();
    let bool_property = snapshot_after_scale
        .properties
        .iter()
        .find(|p| p.name == "bTickEnabled" && p.value.is_boolean())
        .or_else(|| {
            snapshot_after_scale
                .properties
                .iter()
                .find(|p| p.name == "bActive" && p.value.is_boolean())
        })
        .or_else(|| snapshot_after_scale.properties.iter().find(|p| p.value.is_boolean()));

    if let Some(bool_property) = bool_property {
        let bool_property_name = bool_property.name.clone();
        let current_bool = bool_property
            .value
            .as_bool()
            .expect("selected bool property has a boolean value");
        let expected_bool = serde_json::Value::Bool(!current_bool);

        let mut set_bool_params = serde_json::Map::new();
        set_bool_params.insert(
            "objectId".to_owned(),
            serde_json::Value::String(entity_id.clone()),
        );
        set_bool_params.insert(
            "property".to_owned(),
            serde_json::Value::String(bool_property_name.clone()),
        );
        set_bool_params.insert("value".to_owned(), expected_bool.clone());
        let set_bool_value = send_and_expect_result(
            &handle,
            request_envelope(
                "set-bool",
                "object.setProperty",
                Some(set_bool_params),
            ),
            "object.setProperty#bool",
        )
        .await;
        let bool_ack = parse_set_property_result(&set_bool_value)
            .unwrap_or_else(|e| panic!("[object.setProperty bool] parse failed: {e}"));
        assert!(
            bool_ack.accepted,
            "[object.setProperty bool] expected accepted:true, got: {}",
            set_bool_value
        );
        assert_eq!(
            bool_ack.applied_value.as_ref(),
            Some(&expected_bool),
            "[object.setProperty bool] expected appliedValue {}, got: {}",
            expected_bool,
            set_bool_value
        );

        let mut after_bool_params = serde_json::Map::new();
        after_bool_params.insert(
            "objectId".to_owned(),
            serde_json::Value::String(entity_id.clone()),
        );
        let after_bool_value = send_and_expect_result(
            &handle,
            request_envelope(
                "set-obj-after-bool",
                "object.getSnapshot",
                Some(after_bool_params),
            ),
            "object.getSnapshot#after-bool",
        )
        .await;
        let snapshot_after_bool = parse_object_snapshot_result(&after_bool_value).unwrap_or_else(|e| {
            panic!("[object.getSnapshot after bool] parse_object_snapshot_result failed: {e}")
        });
        let bool_after = snapshot_after_bool
            .properties
            .iter()
            .find(|p| p.name == bool_property_name)
            .unwrap_or_else(|| {
                panic!(
                    "[object.getSnapshot after bool] {} missing",
                    bool_property_name
                )
            });
        assert_eq!(
            &bool_after.value, &expected_bool,
            "[object.getSnapshot after bool] {} should be {}",
            bool_property_name, expected_bool
        );
        bool_summary = format!(
            "{}:{}->{} confirmed={}",
            bool_property_name, current_bool, expected_bool, bool_after.value
        );
        eprintln!(
            "[PASS] object.setProperty bool: property={} {}->{}",
            bool_property_name, current_bool, expected_bool
        );
    } else {
        eprintln!(
            "[SKIP] object.setProperty bool: no boolean property exposed for entity {entity_id}"
        );
    }

    // --- Step 7: orderly shutdown ---
    handle.shutdown().await;
    eprintln!(
        "[PASS] engine_object_set_property_norveslib_contract: \
         objectId={entity_id} Scale {original_scale}->{expected_scale} confirmed={confirmed_scale} \
         bool={bool_summary}"
    );
    // _guard drops here, killing the engine process.
}
