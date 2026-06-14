//! Workstream H-D: protocol conformance runner.
//!
//! This is the alpha "protocol changes are always caught" gate. It launches the
//! residential C++ mock engine (`norves_mock_engine`, H-A) as a real OS process,
//! connects the real [`WsClientTransport`] / [`Dispatcher`] over the wire, drives
//! a declarative method/event sequence, and asserts that every response is, under
//! a per-step field mask, value-equal to the spec's positive fixture
//! (`bridge/spec/fixtures`, which is the single source of truth).
//!
//! # Why this catches protocol drift
//!
//! The expected value comes from the spec fixture, not from a copy maintained
//! next to the test. Every field that is NOT listed in a step's `ignore` mask is
//! compared with strict `serde_json::Value` equality. So a change to a
//! non-ignored field on either side (engine response or spec fixture) makes the
//! values diverge and the run fails. The `ignore` masks cover only genuinely
//! non-deterministic or known engine-vs-spec content differences, documented in
//! the scenario JSON.
//!
//! # Running it
//!
//! Opt-in via `NORVES_MOCK_ENGINE` (absolute path to the compiled mock engine).
//! Unset => the test prints a skip notice and returns, so `cargo test` stays
//! green on a machine without the C++ build. CI sets the var so the run is real;
//! "done" requires a real (non-skipped) pass.
//!
//! # Flakiness defenses
//!
//! Inherited from `ws_roundtrip.rs` (G4): an OS-assigned ephemeral port, a wait
//! for the engine's `READY` stdout line, a connect retry loop over the listen
//! edge, a timeout around every await, and a RAII guard that kills + reaps the
//! child on every exit path (relevant on Windows, where an orphaned listener
//! would hold the port).

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use norves_bridge_core::{
    CorrelationId, MethodName, ResponsePayload, ValidatedEnvelope, VersionString,
};
use norves_bridge_editor_client::{Dispatcher, WsClientTransport};
use serde_json::Value;

/// Generous-but-finite bound for every blocking await in the test.
const RECV_TIMEOUT: Duration = Duration::from_secs(5);

/// The wire protocol version used throughout.
fn version() -> VersionString {
    VersionString::try_from("0.1".to_owned()).expect("0.1 is a valid version")
}

/// RAII guard: kills and reaps the child engine on drop, on EVERY exit path
/// (success, early return, or a panicking assertion). Without this a failed
/// assertion would leave the engine holding the port on Windows.
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
/// child can bind it. The connect retry loop covers the release/re-bind gap.
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

/// Spawns the mock engine on `port` and waits (up to `timeout`) for its
/// `READY <port>` line on stdout. Returns the RAII-guarded child once ready.
///
/// stdout is read on a dedicated std thread so the wait can be bounded by a
/// channel `recv_timeout` rather than blocking forever on a child that never
/// prints READY.
fn spawn_ready_engine(exe: &str, port: u16, timeout: Duration) -> ChildGuard {
    let mut child = Command::new(exe)
        .arg("--bridge-port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn mock engine {exe:?}: {e}"));

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
            Err(_) => panic!("mock engine did not print READY within {timeout:?}"),
        }
    }
}

/// Connects [`WsClientTransport`] with retry + backoff, covering the window
/// between READY and the listener actually accepting. Gives up after `overall`.
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
fn request_envelope(
    id: &str,
    method: &str,
    params: Option<serde_json::Map<String, Value>>,
) -> ValidatedEnvelope {
    ValidatedEnvelope::Request {
        version: version(),
        id: CorrelationId::try_from(id.to_owned()).expect("non-empty id"),
        method: MethodName::try_from(method.to_owned()).expect("namespaced method"),
        params,
        session_id: None,
        seq: None,
    }
}

// --- scenario model -------------------------------------------------------

/// One declarative scenario step. Mirrors the JSON schema documented in
/// `bridge/conformance/runners/alpha_method_sequence.json`.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Step {
    Request {
        method: String,
        request_fixture: String,
        expect_fixture: String,
        #[serde(default)]
        ignore: Vec<String>,
    },
    Event {
        event: String,
        expect_fixture: String,
        #[serde(default)]
        ignore: Vec<String>,
    },
}

#[derive(Debug, serde::Deserialize)]
struct Scenario {
    scenario: String,
    steps: Vec<Step>,
}

// --- path resolution ------------------------------------------------------

/// Repo root, derived from the crate manifest dir
/// (`<root>/bridge/crates/norves-bridge-editor-client`).
fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(3)
        .expect("manifest dir has at least three ancestors")
        .to_path_buf()
}

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse JSON {}: {e}", path.display()))
}

/// Resolves a spec fixture path (relative to `bridge/spec/fixtures`) and loads it.
fn load_fixture(root: &Path, rel: &str) -> Value {
    let path = root.join("bridge").join("spec").join("fixtures").join(rel);
    read_json(&path)
}

// --- field-mask comparison ------------------------------------------------

/// Removes the value at `pointer` (a JSON Pointer like `/sessionId`) from `value`
/// if present. A missing path is a no-op: a mask may legitimately target a field
/// the engine omits (e.g. `/capabilities` absent from the mock hello result).
fn remove_pointer(value: &mut Value, pointer: &str) {
    let Some((parent_ptr, key)) = pointer.rsplit_once('/') else {
        return;
    };
    // parent_ptr is "" for a top-level "/key"; pointer_mut("") returns the root.
    let Some(parent) = value.pointer_mut(parent_ptr) else {
        return;
    };
    match parent {
        Value::Object(map) => {
            // JSON Pointer unescapes ~1 -> '/' and ~0 -> '~'.
            let key = key.replace("~1", "/").replace("~0", "~");
            map.remove(&key);
        }
        Value::Array(arr) => {
            if let Ok(index) = key.parse::<usize>() {
                if index < arr.len() {
                    arr.remove(index);
                }
            }
        }
        _ => {}
    }
}

/// Applies a list of ignore pointers, returning the masked copy.
fn apply_mask(value: &Value, ignore: &[String]) -> Value {
    let mut masked = value.clone();
    for pointer in ignore {
        remove_pointer(&mut masked, pointer);
    }
    masked
}

/// Asserts `actual` equals `expected` after masking both with `ignore`. Panics
/// with a full diff on mismatch (this is the conformance failure signal).
fn assert_field_masked_equal(label: &str, actual: &Value, expected: &Value, ignore: &[String]) {
    let actual_masked = apply_mask(actual, ignore);
    let expected_masked = apply_mask(expected, ignore);
    assert_eq!(
        actual_masked, expected_masked,
        "{label}: response diverged from the spec fixture after masking {ignore:?}\n  \
         actual (masked):   {actual_masked}\n  expected (masked): {expected_masked}\n  \
         actual (raw):      {actual}\n  expected (raw):    {expected}"
    );
}

/// Extracts a `/params` object from a fixture as an owned map (for sending a
/// request). Absent params is an empty object.
fn fixture_params(fixture: &Value) -> serde_json::Map<String, Value> {
    match fixture.get("params") {
        Some(Value::Object(map)) => map.clone(),
        Some(other) => panic!("fixture params is not an object: {other}"),
        None => serde_json::Map::new(),
    }
}

/// Extracts the `/result` object from a response fixture.
fn fixture_result(fixture: &Value) -> Value {
    fixture
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("response fixture lacks a result: {fixture}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn alpha_method_sequence_conforms_to_spec_fixtures() {
    // Opt-in: without the compiled mock engine path we skip (not fail), so the
    // suite stays green on a machine without the C++ build. CI sets the var so
    // the conformance run is real.
    let exe = match std::env::var("NORVES_MOCK_ENGINE") {
        Ok(path) => path,
        Err(_) => {
            eprintln!(
                "alpha_method_sequence_conforms_to_spec_fixtures skipped: set NORVES_MOCK_ENGINE \
                 to the norves_mock_engine executable path to run the conformance sequence"
            );
            return;
        }
    };

    let root = repo_root();

    // Load the declarative scenario.
    let scenario_path = root
        .join("bridge")
        .join("conformance")
        .join("runners")
        .join("alpha_method_sequence.json");
    let scenario: Scenario =
        serde_json::from_value(read_json(&scenario_path)).unwrap_or_else(|e| {
            panic!(
                "failed to deserialize scenario {}: {e}",
                scenario_path.display()
            )
        });
    assert_eq!(scenario.scenario, "alpha-method-sequence");
    assert!(!scenario.steps.is_empty(), "scenario has no steps");

    // 1. Launch the mock engine on a free port and wait for READY.
    let port = pick_free_port();
    let _guard = spawn_ready_engine(&exe, port, Duration::from_secs(10));
    let url = format!("ws://127.0.0.1:{port}");

    // 2. Connect the real WS transport (retrying the listen edge) and spawn the
    //    real dispatcher.
    let transport = connect_with_retry(&url, Duration::from_secs(5)).await;
    let handle = Dispatcher::spawn(transport);

    // 3. Subscribe to events BEFORE issuing any request so no event is missed.
    let mut events = handle.subscribe_events();

    let mut req_seq = 0u32;
    let mut next_id = |method: &str| {
        req_seq += 1;
        format!("conf-{method}-{req_seq}")
    };

    for step in &scenario.steps {
        match step {
            Step::Request {
                method,
                request_fixture,
                expect_fixture,
                ignore,
            } => {
                let req_fixture = load_fixture(&root, request_fixture);
                let params = fixture_params(&req_fixture);
                let expected = fixture_result(&load_fixture(&root, expect_fixture));

                let id = next_id(method);
                let response = with_timeout(
                    &format!("{method} response"),
                    handle.request(request_envelope(&id, method, Some(params)), RECV_TIMEOUT),
                )
                .await
                .unwrap_or_else(|e| panic!("{method} request did not resolve: {e:?}"));

                let actual = match response {
                    ResponsePayload::Result(value) => value,
                    ResponsePayload::Error(err) => {
                        panic!("{method} returned an engine error instead of a result: {err:?}")
                    }
                };
                assert_field_masked_equal(method, &actual, &expected, ignore);
            }
            Step::Event {
                event,
                expect_fixture,
                ignore,
            } => {
                // For the alpha sequence the only event is log.message, gated
                // behind a log.subscribe ack. Drive the subscribe, then receive.
                let sub_id = next_id("log.subscribe");
                let sub_response = with_timeout(
                    "log.subscribe response",
                    handle.request(
                        request_envelope(&sub_id, "log.subscribe", Some(serde_json::Map::new())),
                        RECV_TIMEOUT,
                    ),
                )
                .await
                .expect("log.subscribe request resolves");
                assert!(
                    matches!(sub_response, ResponsePayload::Result(_)),
                    "log.subscribe should resolve with a result, got {sub_response:?}"
                );

                let expected = load_fixture(&root, expect_fixture)
                    .get("params")
                    .cloned()
                    .unwrap_or_else(|| panic!("event fixture {expect_fixture} lacks params"));

                let envelope = with_timeout(&format!("{event} event"), events.recv())
                    .await
                    .expect("event received without lag/close");
                let actual = match &*envelope {
                    ValidatedEnvelope::Event {
                        event: name,
                        params,
                        ..
                    } => {
                        assert_eq!(name.as_str(), event, "unexpected event name");
                        let map = params.clone().expect("event carries params");
                        Value::Object(map)
                    }
                    other => panic!("expected a {event} event, got {other:?}"),
                };
                assert_field_masked_equal(event, &actual, &expected, ignore);
            }
        }
    }

    // Orderly shutdown; the RAII guard kills + reaps the engine regardless.
    handle.shutdown().await;
}
