//! Phase D5b: end-to-end loopback round trip against an in-test mock engine,
//! with **no Tauri**. This is the Workstream D "Done" bar: the runtime can talk
//! to a loopback mock endpoint, performing the `bridge.hello` handshake, an
//! `engine.getStatus` query, and receiving a `log.message` event — all driven
//! through the real [`Dispatcher`] over a [`loopback_pair`].
//!
//! # Determinism (B4)
//!
//! Event delivery is the only async race worth guarding. We remove it by
//! ordering: the client subscribes to events **before** the mock is ever asked
//! to emit one (the mock only sends `log.message` *after* it has answered a
//! `log.subscribe` request, which the client issues after subscribing). Every
//! receive is bounded by [`tokio::time::timeout`] so a logic bug surfaces as a
//! fast failure rather than a hang. The mock task is `abort`ed at the end so it
//! never leaks past the test.
//!
//! The mock lives entirely in this test file — it is **not** production code and
//! must never live under `src/`. It hand-writes responses that satisfy the
//! relevant fixture/result schemas (`bridge.hello.result`,
//! `engine.getStatus.result`, `log.message.params`) without reading or adding
//! any fixture.

use std::time::Duration;

use norves_bridge_core::{
    decode_typed, encode_envelope, CorrelationId, Envelope, EventName, MethodName, ResponsePayload,
    ValidatedEnvelope, VersionString,
};
use norves_bridge_editor_client::{
    loopback_pair, parse_hello_result, parse_log_message, parse_status_result, Dispatcher,
    HelloParams, LoopbackTransport, Transport,
};

/// Short, generous-but-finite bound for every blocking receive in the test, so
/// a stuck dispatcher or mock fails fast instead of hanging the suite.
const RECV_TIMEOUT: Duration = Duration::from_secs(5);

/// Builds the wire version string used everywhere in this test.
fn version() -> VersionString {
    VersionString::try_from("0.1".to_owned()).expect("0.1 is a valid version")
}

/// Builds a request-kind [`ValidatedEnvelope`] with a unique `id`, the given
/// method, and optional params. The dispatcher requires a request-kind envelope
/// whose `id` is unique among the caller's in-flight requests; each call site
/// here passes a distinct `id`.
fn request(
    id: &str,
    method: &str,
    params: Option<serde_json::Map<String, serde_json::Value>>,
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

/// Encodes a `result`-kind response frame for the given correlation `id`.
fn response_frame(id: &CorrelationId, result: serde_json::Value) -> String {
    let env: Envelope = ValidatedEnvelope::Response {
        version: version(),
        id: id.clone(),
        payload: ResponsePayload::Result(result),
        session_id: None,
        seq: None,
    }
    .into();
    encode_envelope(&env).expect("response envelope encodes")
}

/// Encodes a `log.message` event frame carrying the given monotonic `seq`.
fn log_event_frame(seq: u64) -> String {
    let mut params = serde_json::Map::new();
    params.insert("level".to_owned(), serde_json::json!("info"));
    params.insert("message".to_owned(), serde_json::json!("hello from mock"));
    params.insert("category".to_owned(), serde_json::json!("Engine"));

    let env: Envelope = ValidatedEnvelope::Event {
        version: version(),
        event: EventName::try_from("log.message".to_owned()).expect("namespaced event"),
        params: Some(params),
        session_id: None,
        seq: Some(seq),
    }
    .into();
    encode_envelope(&env).expect("event envelope encodes")
}

/// The minimal mock engine: it owns the engine-side transport, reads request
/// frames, and answers per method using the core codec. It deliberately knows
/// only the three methods this round trip exercises; anything else is ignored
/// (the real engine would error, but the test never sends one).
///
/// Returns when the transport reports EOF (the client side dropped), so the
/// spawning test can `abort` it without relying on a clean shutdown.
async fn run_mock_engine(mut engine: LoopbackTransport) {
    // Monotonic event seq, advanced only when we emit an event.
    let mut next_seq: u64 = 1;

    loop {
        let frame = match engine.recv().await {
            Ok(Some(frame)) => frame,
            // Clean peer close or transport error: the client is gone, so stop.
            Ok(None) | Err(_) => return,
        };

        // The mock uses the same core codec the client does. A frame we cannot
        // decode is dropped (never fatal); the test never sends such a frame.
        let envelope = match decode_typed(&frame) {
            Ok(envelope) => envelope,
            Err(_) => continue,
        };

        let ValidatedEnvelope::Request { id, method, .. } = envelope else {
            // The mock only ever receives requests in this test.
            continue;
        };

        match method.as_str() {
            "bridge.hello" => {
                let result = serde_json::json!({
                    "sessionId": "sess-mock-1",
                    "protocolVersion": "0.1",
                    "server": {
                        "name": "MockEngine",
                        "version": "0.1.0",
                        "engine": "mock"
                    }
                });
                if engine.send(response_frame(&id, result)).await.is_err() {
                    return;
                }
            }
            "engine.getStatus" => {
                let result = serde_json::json!({
                    "engineState": "ready",
                    "runtimeState": "edit",
                    "engineName": "MockEngine",
                    "engineVersion": "0.1.0",
                    "title": "Mock Game"
                });
                if engine.send(response_frame(&id, result)).await.is_err() {
                    return;
                }
            }
            "log.subscribe" => {
                // Acknowledge the subscription with a minimal empty-object
                // result, then emit one log.message event. Ordering the event
                // strictly after the ack keeps the round trip deterministic:
                // the client has already subscribed before it issued this
                // request, so the broadcast cannot miss the event.
                if engine
                    .send(response_frame(&id, serde_json::json!({})))
                    .await
                    .is_err()
                {
                    return;
                }
                let seq = next_seq;
                next_seq += 1;
                if engine.send(log_event_frame(seq)).await.is_err() {
                    return;
                }
            }
            // Out of scope for this round trip; ignore rather than error.
            _ => continue,
        }
    }
}

/// Awaits `fut` with the shared [`RECV_TIMEOUT`], panicking with `what` on
/// expiry so a stall is reported as a fast, descriptive failure.
async fn with_timeout<F, T>(what: &str, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    match tokio::time::timeout(RECV_TIMEOUT, fut).await {
        Ok(value) => value,
        Err(_) => panic!("timed out waiting for {what}"),
    }
}

#[tokio::test]
async fn loopback_mock_round_trip_hello_status_and_log() {
    // 1. Wire a client transport to an engine transport, and start the mock on
    //    the engine side.
    let (client_transport, engine_transport) = loopback_pair(16);
    let mock = tokio::spawn(run_mock_engine(engine_transport));

    // 2. Spawn the real dispatcher over the client transport.
    let handle = Dispatcher::spawn(client_transport);

    // 3. Subscribe to events *before* the handshake — i.e. before the mock can
    //    ever emit one — so no event is lost (B4 determinism).
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
    assert_eq!(outcome.server_engine.as_deref(), Some("mock"));

    // 5. engine.getStatus round trip.
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
    assert_eq!(
        snapshot.engine_state,
        norves_bridge_core::EngineState::Ready
    );
    assert_eq!(
        snapshot.runtime_state,
        norves_bridge_core::RuntimeState::Edit
    );
    assert_eq!(snapshot.engine_name.as_deref(), Some("MockEngine"));
    assert_eq!(snapshot.title.as_deref(), Some("Mock Game"));

    // 6. log.subscribe, then receive the log.message event. The subscription was
    //    established in step 3, so the event the mock emits after acking this
    //    request cannot be missed.
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

    let event = with_timeout("log.message event", events.recv())
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
    assert_eq!(log.level, norves_bridge_core::LogLevel::Info);
    assert_eq!(log.message, "hello from mock");

    // 7. Orderly shutdown of the dispatcher; then stop the mock so it cannot
    //    leak past the test.
    handle.shutdown().await;
    mock.abort();
    let _ = mock.await;
}
