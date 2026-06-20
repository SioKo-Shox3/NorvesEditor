//! Phase 0: backward-compatibility proof for the protocol 0.1 -> 0.2 MINOR bump.
//!
//! The editor now offers `["0.2", "0.1"]` in `bridge.hello`. This test proves
//! that a **legacy 0.1-only engine** — one that supports only `"0.1"` — still
//! negotiates successfully, falling back to `protocolVersion: "0.1"`.
//!
//! Why a hand-rolled fake instead of the C++ harness: after the SDK bump the C++
//! `SupportedProtocolVersions` is `{"0.2", "0.1"}`, so the C++ test server can no
//! longer reproduce a 0.1-*only* engine. We therefore model the legacy engine in
//! pure Rust over a [`loopback_pair`], exercising the real [`Dispatcher`] and the
//! real `HelloParams` / `parse_hello_result` editor handshake path.
//!
//! The fake lives entirely in this test file (never under `src/`). It does the
//! version negotiation the way a real 0.1-only engine would: it reads the offered
//! `params.protocolVersions`, intersects them with its own single supported
//! version `"0.1"`, selects that, and echoes it. Both the envelope `version` and
//! the negotiated `result.protocolVersion` are stamped `"0.1"` — the legacy wire.

use std::time::Duration;

use norves_bridge_core::{
    decode_typed, encode_envelope, CorrelationId, Envelope, ResponsePayload, ValidatedEnvelope,
    VersionString,
};
use norves_bridge_editor_client::{
    loopback_pair, parse_hello_result, Dispatcher, HelloParams, LoopbackTransport, Transport,
};

/// Generous-but-finite bound for every blocking receive, so a stall fails fast.
const RECV_TIMEOUT: Duration = Duration::from_secs(5);

/// The single protocol version the legacy fake engine supports.
const LEGACY_VERSION: &str = "0.1";

/// Builds a request-kind [`ValidatedEnvelope`]. The editor stamps its envelope
/// `version` with the *current* protocol generation regardless of negotiation;
/// `"0.2"` here mirrors the editor backend's fixed `PROTOCOL_VERSION`.
fn request(
    id: &str,
    method: &str,
    params: Option<serde_json::Map<String, serde_json::Value>>,
) -> ValidatedEnvelope {
    ValidatedEnvelope::Request {
        version: VersionString::try_from("0.2".to_owned()).expect("0.2 is a valid version"),
        id: CorrelationId::try_from(id.to_owned()).expect("non-empty id"),
        method: norves_bridge_core::MethodName::try_from(method.to_owned())
            .expect("namespaced method"),
        params,
        session_id: None,
        seq: None,
    }
}

/// Encodes a `result`-kind response frame stamped with the legacy envelope
/// `version` ("0.1"), as a real 0.1-only engine would emit.
fn legacy_response_frame(id: &CorrelationId, result: serde_json::Value) -> String {
    let env: Envelope = ValidatedEnvelope::Response {
        version: VersionString::try_from(LEGACY_VERSION.to_owned())
            .expect("0.1 is a valid version"),
        id: id.clone(),
        payload: ResponsePayload::Result(result),
        session_id: None,
        seq: None,
    }
    .into();
    encode_envelope(&env).expect("legacy response envelope encodes")
}

/// A minimal legacy 0.1-only engine: it answers exactly one `bridge.hello`,
/// performing real version negotiation against its single supported version.
///
/// Returns the offered `protocolVersions` the client sent (so the test can
/// assert the editor actually offered `["0.2", "0.1"]`), or `None` if the engine
/// stopped before a well-formed hello arrived.
async fn run_legacy_engine(mut engine: LoopbackTransport) -> Option<Vec<String>> {
    loop {
        let frame = match engine.recv().await {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => return None,
        };
        let envelope = match decode_typed(&frame) {
            Ok(envelope) => envelope,
            Err(_) => continue,
        };
        let ValidatedEnvelope::Request {
            id, method, params, ..
        } = envelope
        else {
            continue;
        };
        if method.as_str() != "bridge.hello" {
            continue;
        }

        // Read the offered versions the editor sent, in preference order.
        let offered: Vec<String> = params
            .as_ref()
            .and_then(|p| p.get("protocolVersions"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();

        // Real negotiation: a 0.1-only engine selects "0.1" iff it was offered.
        // The editor offering ["0.2", "0.1"] means "0.1" is present, so the
        // legacy engine accepts and falls back to it.
        let selected = if offered.iter().any(|v| v == LEGACY_VERSION) {
            LEGACY_VERSION
        } else {
            // The editor failed to offer 0.1 — a real legacy engine would reply
            // PROTOCOL_VERSION_UNSUPPORTED. The test asserts this never happens.
            return Some(offered);
        };

        let result = serde_json::json!({
            "sessionId": "sess-legacy-1",
            "protocolVersion": selected,
            "server": {
                "name": "LegacyEngine",
                "version": "0.0.9",
                "engine": "legacy"
            }
        });
        let _ = engine.send(legacy_response_frame(&id, result)).await;
        return Some(offered);
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

/// The editor offers `["0.2", "0.1"]`; a 0.1-only engine negotiates the fallback
/// `"0.1"` and the editor records `protocol_version == "0.1"`.
#[tokio::test]
async fn editor_negotiates_down_to_v01_with_legacy_engine() {
    let (client_transport, engine_transport) = loopback_pair(16);
    let engine = tokio::spawn(run_legacy_engine(engine_transport));

    let handle = Dispatcher::spawn(client_transport);

    // Offer the real editor preference list: 0.2 first, 0.1 fallback.
    let offered = vec![
        VersionString::try_from("0.2".to_owned()).expect("0.2 valid"),
        VersionString::try_from("0.1".to_owned()).expect("0.1 valid"),
    ];
    let hello_params = HelloParams::new("NorvesEditor", offered)
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
        ResponsePayload::Error(err) => {
            panic!("legacy engine rejected the handshake (no common version?): {err:?}")
        }
    };
    let outcome = parse_hello_result(&hello_result).expect("hello result parses");

    // The core assertion: negotiation fell back to 0.1 with the legacy engine.
    assert_eq!(
        outcome.protocol_version.as_str(),
        "0.1",
        "editor should record the negotiated fallback version 0.1"
    );
    assert_eq!(outcome.session_id, "sess-legacy-1");
    assert_eq!(outcome.server_name, "LegacyEngine");
    assert_eq!(outcome.server_engine.as_deref(), Some("legacy"));

    handle.shutdown().await;

    // Confirm the editor actually offered ["0.2", "0.1"] in preference order, so
    // the fallback we observed is a real negotiation, not a coincidence.
    let offered_seen = with_timeout("legacy engine to finish", engine)
        .await
        .expect("legacy engine task joins")
        .expect("legacy engine saw a bridge.hello");
    assert_eq!(
        offered_seen,
        vec!["0.2".to_owned(), "0.1".to_owned()],
        "editor must offer 0.2 first then 0.1 as fallback"
    );
}
