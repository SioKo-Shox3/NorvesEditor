//! `bridge.hello` handshake domain types (sans-I/O).
//!
//! This module owns the editor side of the handshake: the request params we
//! send ([`HelloParams`]) and the typed extraction of the engine's reply
//! ([`parse_hello_result`] -> [`HelloOutcome`]). Per the Phase D4 plan the core
//! crate deliberately does **not** give `bridge.hello` a strong result type;
//! the editor client pulls the fields it needs out of the
//! [`serde_json::Value`] result here.
//!
//! Wire shape follows `bridge.hello.params.schema.json` and
//! `bridge.hello.result.schema.json`. Version negotiation failure handling
//! follows `error-model.md`: an id-bearing error response carrying
//! `PROTOCOL_VERSION_UNSUPPORTED`, with `error.data.offered` / `.supported`
//! version arrays (informative, SHOULD-present, so absence is tolerated).

use norves_bridge_core::{BridgeError, CapabilityToken, ErrorCode, VersionString};
use serde::Serialize;
use serde_json::{Map, Value};

/// Role of the connecting peer. Schema enum is `["editor"]`; for the alpha the
/// editor is always the client that says hello.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HelloRole {
    /// `editor`
    Editor,
}

/// `bridge.hello` request params produced by the editor.
///
/// Mirrors `bridge.hello.params.schema.json`: `role`, `clientName`,
/// `protocolVersions` are required; `clientVersion` and `capabilities` are
/// optional. Field names serialize as wire `camelCase`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloParams {
    /// Fixed `"editor"` role for the alpha.
    pub role: HelloRole,
    /// Human-readable client product name, e.g. `NorvesEditor`.
    pub client_name: String,
    /// Optional client product version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    /// Protocol versions the client supports, in preference order.
    pub protocol_versions: Vec<VersionString>,
    /// Optional capability tokens the client offers or requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<CapabilityToken>>,
}

impl HelloParams {
    /// Builds the minimal `bridge.hello` params: a `role`, client name, and the
    /// supported protocol versions, with no client version or capabilities.
    pub fn new(client_name: impl Into<String>, protocol_versions: Vec<VersionString>) -> Self {
        HelloParams {
            role: HelloRole::Editor,
            client_name: client_name.into(),
            client_version: None,
            protocol_versions,
            capabilities: None,
        }
    }

    /// Serializes these params into the `serde_json::Map` that an envelope's
    /// `params` field expects.
    ///
    /// Returns [`HandshakeError::Serialize`] if serialization fails or does not
    /// yield a JSON object (it always does for this struct, but the error path
    /// keeps this free of `unwrap`/`expect`).
    pub fn to_params(&self) -> Result<Map<String, Value>, HandshakeError> {
        match serde_json::to_value(self)? {
            Value::Object(map) => Ok(map),
            other => Err(HandshakeError::UnexpectedShape(format!(
                "hello params serialized to non-object JSON: {other}"
            ))),
        }
    }
}

/// Successfully negotiated handshake outcome extracted from a `bridge.hello`
/// result payload.
#[derive(Debug, Clone, PartialEq)]
pub struct HelloOutcome {
    /// Session id assigned by the engine for this connection.
    pub session_id: String,
    /// Protocol version the engine selected.
    pub protocol_version: VersionString,
    /// Engine endpoint product/integration name.
    pub server_name: String,
    /// Optional engine endpoint version.
    pub server_version: Option<String>,
    /// Optional generic engine identifier (free-form label).
    pub server_engine: Option<String>,
}

/// Failure of the `bridge.hello` handshake, whether structural (the result was
/// malformed) or negotiated (the engine rejected our offered versions).
#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    /// The engine could select none of the offered protocol versions
    /// (`PROTOCOL_VERSION_UNSUPPORTED`). `offered` / `supported` are recovered
    /// from `error.data` when present; absent arrays yield empty vectors.
    #[error(
        "protocol version negotiation failed (offered: {offered:?}, supported: {supported:?})"
    )]
    VersionUnsupported {
        /// Versions the client offered, per `error.data.offered` (if present).
        offered: Vec<String>,
        /// Versions the engine supports, per `error.data.supported` (if present).
        supported: Vec<String>,
    },

    /// A required result field was missing.
    #[error("missing required field: {0}")]
    MissingField(String),

    /// A result field was present but had the wrong type or an invalid value.
    #[error("invalid field: {0}")]
    InvalidField(String),

    /// The engine returned a wire error other than version negotiation.
    #[error("engine returned error response: {0:?}")]
    Engine(BridgeError),

    /// The result payload did not have the expected JSON shape.
    #[error("unexpected payload shape: {0}")]
    UnexpectedShape(String),

    /// Serializing the request params failed.
    #[error("failed to serialize hello params: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Extracts a [`HelloOutcome`] from a successful `bridge.hello` `result` value.
///
/// Required: `sessionId`, `protocolVersion` (validated as [`VersionString`]),
/// `server.name`. Optional `server.version` / `server.engine` are kept when
/// present. This handles only the success (`result`) branch; an error response
/// is mapped to [`HandshakeError`] by [`hello_error_to_handshake`].
pub fn parse_hello_result(result: &Value) -> Result<HelloOutcome, HandshakeError> {
    let obj = result.as_object().ok_or_else(|| {
        HandshakeError::UnexpectedShape("hello result is not an object".to_owned())
    })?;

    let session_id = required_str(obj, "sessionId")?.to_owned();

    let protocol_version_raw = required_str(obj, "protocolVersion")?;
    let protocol_version = VersionString::try_from(protocol_version_raw.to_owned())
        .map_err(|_| HandshakeError::InvalidField("protocolVersion".to_owned()))?;

    let server = obj
        .get("server")
        .ok_or_else(|| HandshakeError::MissingField("server".to_owned()))?
        .as_object()
        .ok_or_else(|| HandshakeError::InvalidField("server".to_owned()))?;

    let server_name = required_str(server, "name")?.to_owned();
    let server_version = optional_str(server, "version")?.map(str::to_owned);
    let server_engine = optional_str(server, "engine")?.map(str::to_owned);

    Ok(HelloOutcome {
        session_id,
        protocol_version,
        server_name,
        server_version,
        server_engine,
    })
}

/// Maps an engine error response to a [`HandshakeError`].
///
/// A `PROTOCOL_VERSION_UNSUPPORTED` code becomes
/// [`HandshakeError::VersionUnsupported`], recovering `offered` / `supported`
/// from `error.data` when present (absent => empty). Any other code is wrapped
/// as [`HandshakeError::Engine`].
pub fn hello_error_to_handshake(error: BridgeError) -> HandshakeError {
    if error.code == ErrorCode::protocol_version_unsupported() {
        let (offered, supported) = match &error.data {
            Some(Value::Object(data)) => (
                string_array(data.get("offered")),
                string_array(data.get("supported")),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        HandshakeError::VersionUnsupported { offered, supported }
    } else {
        HandshakeError::Engine(error)
    }
}

/// Reads `value` as a JSON array of strings, ignoring non-string elements and
/// returning an empty vector when `value` is `None` or not an array.
fn string_array(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// Returns the string at `key`, erroring if absent or not a string.
fn required_str<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a str, HandshakeError> {
    match obj.get(key) {
        None => Err(HandshakeError::MissingField(key.to_owned())),
        Some(value) => value
            .as_str()
            .ok_or_else(|| HandshakeError::InvalidField(key.to_owned())),
    }
}

/// Returns the string at `key` if present; errors if present but not a string.
fn optional_str<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, HandshakeError> {
    match obj.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| HandshakeError::InvalidField(key.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norves_bridge_core::{ResponsePayload, ValidatedEnvelope};

    const RESPONSE_VALID: &str = r#"{
        "bridge": "norves.editor.bridge",
        "version": "0.1",
        "kind": "response",
        "id": "req-1",
        "sessionId": "sess-7f3a",
        "result": {
            "sessionId": "sess-7f3a",
            "protocolVersion": "0.1",
            "server": { "name": "MockEngine", "version": "0.1.0", "engine": "mock" },
            "capabilities": [
                { "name": "runtime.control", "version": "0.1" },
                { "name": "log.stream", "description": "Streams engine log lines." }
            ]
        }
    }"#;

    const RESPONSE_VERSION_UNSUPPORTED: &str = r#"{
        "bridge": "norves.editor.bridge",
        "version": "0.1",
        "kind": "response",
        "id": "req-1",
        "error": {
            "code": "PROTOCOL_VERSION_UNSUPPORTED",
            "message": "None of the offered protocol versions are supported by this engine.",
            "data": { "offered": ["2.0"], "supported": ["0.1", "1.0"] }
        }
    }"#;

    fn version(value: &str) -> VersionString {
        VersionString::try_from(value.to_owned()).expect("valid version string")
    }

    fn capability(value: &str) -> CapabilityToken {
        CapabilityToken::try_from(value.to_owned()).expect("valid capability token")
    }

    fn result_value(json: &str) -> Value {
        let env: norves_bridge_core::Envelope =
            serde_json::from_str(json).expect("fixture deserializes");
        match ValidatedEnvelope::try_from(env).expect("fixture validates") {
            ValidatedEnvelope::Response {
                payload: ResponsePayload::Result(value),
                ..
            } => value,
            other => panic!("expected result response, got {other:?}"),
        }
    }

    fn error_payload(json: &str) -> BridgeError {
        let env: norves_bridge_core::Envelope =
            serde_json::from_str(json).expect("fixture deserializes");
        match ValidatedEnvelope::try_from(env).expect("fixture validates") {
            ValidatedEnvelope::Response {
                payload: ResponsePayload::Error(err),
                ..
            } => err,
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn hello_params_serialize_has_role_camel_case_and_required() {
        let params = HelloParams {
            role: HelloRole::Editor,
            client_name: "NorvesEditor".to_owned(),
            client_version: Some("0.1.0".to_owned()),
            protocol_versions: vec![version("0.1")],
            capabilities: Some(vec![
                capability("runtime.control"),
                capability("log.stream"),
            ]),
        };
        let map = params.to_params().expect("serializes to object");

        assert_eq!(map.get("role").and_then(Value::as_str), Some("editor"));
        assert_eq!(
            map.get("clientName").and_then(Value::as_str),
            Some("NorvesEditor")
        );
        assert_eq!(
            map.get("clientVersion").and_then(Value::as_str),
            Some("0.1.0")
        );
        assert_eq!(
            map.get("protocolVersions"),
            Some(&serde_json::json!(["0.1"]))
        );
        assert_eq!(
            map.get("capabilities"),
            Some(&serde_json::json!(["runtime.control", "log.stream"]))
        );
        // required keys present
        for key in ["role", "clientName", "protocolVersions"] {
            assert!(map.contains_key(key), "missing required key {key}");
        }
    }

    #[test]
    fn hello_params_omits_optional_when_none() {
        let params = HelloParams::new("NorvesEditor", vec![version("0.1")]);
        let map = params.to_params().expect("serializes to object");
        assert!(!map.contains_key("clientVersion"));
        assert!(!map.contains_key("capabilities"));
        assert_eq!(map.get("role").and_then(Value::as_str), Some("editor"));
    }

    #[test]
    fn hello_params_matches_fixture_request_params_shape() {
        // Mirrors fixtures/methods/bridge.hello/positive/request-valid.json params.
        let params = HelloParams {
            role: HelloRole::Editor,
            client_name: "NorvesEditor".to_owned(),
            client_version: Some("0.1.0".to_owned()),
            protocol_versions: vec![version("0.1")],
            capabilities: Some(vec![
                capability("runtime.control"),
                capability("log.stream"),
            ]),
        };
        let produced = Value::Object(params.to_params().expect("serializes"));
        let expected = serde_json::json!({
            "role": "editor",
            "clientName": "NorvesEditor",
            "clientVersion": "0.1.0",
            "protocolVersions": ["0.1"],
            "capabilities": ["runtime.control", "log.stream"]
        });
        assert_eq!(produced, expected);
    }

    #[test]
    fn parse_hello_result_extracts_fields() {
        let outcome = parse_hello_result(&result_value(RESPONSE_VALID)).expect("parses");
        assert_eq!(outcome.session_id, "sess-7f3a");
        assert_eq!(outcome.protocol_version.as_str(), "0.1");
        assert_eq!(outcome.server_name, "MockEngine");
        assert_eq!(outcome.server_version.as_deref(), Some("0.1.0"));
        assert_eq!(outcome.server_engine.as_deref(), Some("mock"));
    }

    #[test]
    fn parse_hello_result_missing_session_id_errors() {
        let value = serde_json::json!({
            "protocolVersion": "0.1",
            "server": { "name": "MockEngine" }
        });
        assert!(matches!(
            parse_hello_result(&value),
            Err(HandshakeError::MissingField(f)) if f == "sessionId"
        ));
    }

    #[test]
    fn parse_hello_result_missing_protocol_version_errors() {
        let value = serde_json::json!({
            "sessionId": "sess-1",
            "server": { "name": "MockEngine" }
        });
        assert!(matches!(
            parse_hello_result(&value),
            Err(HandshakeError::MissingField(f)) if f == "protocolVersion"
        ));
    }

    #[test]
    fn parse_hello_result_invalid_protocol_version_errors() {
        let value = serde_json::json!({
            "sessionId": "sess-1",
            "protocolVersion": "not-a-version",
            "server": { "name": "MockEngine" }
        });
        assert!(matches!(
            parse_hello_result(&value),
            Err(HandshakeError::InvalidField(f)) if f == "protocolVersion"
        ));
    }

    #[test]
    fn parse_hello_result_missing_server_name_errors() {
        let value = serde_json::json!({
            "sessionId": "sess-1",
            "protocolVersion": "0.1",
            "server": {}
        });
        assert!(matches!(
            parse_hello_result(&value),
            Err(HandshakeError::MissingField(f)) if f == "name"
        ));
    }

    #[test]
    fn version_unsupported_recovers_offered_and_supported() {
        let err = hello_error_to_handshake(error_payload(RESPONSE_VERSION_UNSUPPORTED));
        match err {
            HandshakeError::VersionUnsupported { offered, supported } => {
                assert_eq!(offered, vec!["2.0".to_owned()]);
                assert_eq!(supported, vec!["0.1".to_owned(), "1.0".to_owned()]);
            }
            other => panic!("expected VersionUnsupported, got {other:?}"),
        }
    }

    #[test]
    fn version_unsupported_tolerates_missing_data() {
        let err = BridgeError {
            code: ErrorCode::protocol_version_unsupported(),
            message: "no data".to_owned(),
            data: None,
        };
        match hello_error_to_handshake(err) {
            HandshakeError::VersionUnsupported { offered, supported } => {
                assert!(offered.is_empty());
                assert!(supported.is_empty());
            }
            other => panic!("expected VersionUnsupported, got {other:?}"),
        }
    }

    #[test]
    fn non_version_error_is_engine_variant() {
        let err = BridgeError {
            code: ErrorCode::method_not_supported(),
            message: "unsupported".to_owned(),
            data: None,
        };
        assert!(matches!(
            hello_error_to_handshake(err),
            HandshakeError::Engine(_)
        ));
    }
}
