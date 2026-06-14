//! The canonical Bridge wire envelope.
//!
//! [`Envelope`] mirrors `envelope.schema.json` exactly: a flat object with
//! `additionalProperties: false`. Structural rules that JSON Schema expresses
//! via `allOf`/`if`/`then` (which kinds may carry which fields) are not
//! expressible by `serde` field presence alone, so they live in
//! [`Envelope::validate`].
//!
//! Three kinds — request, response, event — share one struct. Decoding only
//! enforces marker/pattern/`deny_unknown_fields`; cross-field structural rules
//! require an explicit [`Envelope::validate`] call.

use serde::{Deserialize, Serialize};

use crate::common::{is_namespaced_token, VersionString};
use crate::error::{BridgeError, CodecError};

/// Protocol marker constant for the NorvesEditor Bridge.
pub const BRIDGE_MARKER: &str = "norves.editor.bridge";

/// Validating wrapper for the `bridge` marker field.
///
/// Deserializes only from the exact constant [`BRIDGE_MARKER`]; any other value
/// is rejected. Serializes back to that constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BridgeMarker;

impl TryFrom<String> for BridgeMarker {
    type Error = CodecError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value == BRIDGE_MARKER {
            Ok(BridgeMarker)
        } else {
            Err(CodecError::InvalidBridgeMarker {
                expected: BRIDGE_MARKER,
                actual: value,
            })
        }
    }
}

impl From<BridgeMarker> for String {
    fn from(_: BridgeMarker) -> Self {
        BRIDGE_MARKER.to_owned()
    }
}

/// Envelope discriminator. Schema: `enum ["request", "response", "event"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A method request.
    Request,
    /// A response to a request.
    Response,
    /// A one-way event.
    Event,
}

/// Request/response correlation id. Schema: non-empty string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CorrelationId(String);

impl CorrelationId {
    /// Returns the underlying id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CorrelationId {
    type Error = CodecError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            Err(CodecError::InvalidField {
                field: "correlationId",
                value,
            })
        } else {
            Ok(CorrelationId(value))
        }
    }
}

impl From<CorrelationId> for String {
    fn from(value: CorrelationId) -> Self {
        value.0
    }
}

/// Namespaced method name, e.g. `runtime.play`.
/// Schema pattern: `^[a-z][a-zA-Z0-9]*\.[a-zA-Z0-9]+$`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MethodName(String);

impl MethodName {
    /// Returns the underlying method name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for MethodName {
    type Error = CodecError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_namespaced_token(&value) {
            Ok(MethodName(value))
        } else {
            Err(CodecError::InvalidField {
                field: "methodName",
                value,
            })
        }
    }
}

impl From<MethodName> for String {
    fn from(value: MethodName) -> Self {
        value.0
    }
}

/// Namespaced event name, e.g. `log.message`.
/// Schema pattern: `^[a-z][a-zA-Z0-9]*\.[a-zA-Z0-9]+$`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EventName(String);

impl EventName {
    /// Returns the underlying event name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EventName {
    type Error = CodecError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_namespaced_token(&value) {
            Ok(EventName(value))
        } else {
            Err(CodecError::InvalidField {
                field: "eventName",
                value,
            })
        }
    }
}

impl From<EventName> for String {
    fn from(value: EventName) -> Self {
        value.0
    }
}

// TODO(D3): provide a type-safe ValidatedEnvelope enum after validate() so
// that the kind-dependent field combinations are encoded in the type system
// instead of re-checked at runtime.

/// The canonical Bridge wire envelope.
///
/// Flat structure with `deny_unknown_fields`, mirroring `envelope.schema.json`.
/// Field *presence* rules per kind are enforced by [`Envelope::validate`], not
/// by deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// Protocol marker constant.
    pub bridge: BridgeMarker,
    /// Protocol version string, `MAJOR.MINOR`.
    pub version: VersionString,
    /// Envelope discriminator.
    pub kind: Kind,
    /// Request/response correlation id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<CorrelationId>,
    /// Method name on a request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<MethodName>,
    /// Event name on an event envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<EventName>,
    /// Method or event payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Map<String, serde_json::Value>>,
    /// Success payload on a response. Mutually exclusive with `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error payload on a response. Mutually exclusive with `result`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<BridgeError>,
    /// Optional session id assigned during the handshake.
    #[serde(rename = "sessionId", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Optional monotonically increasing per-connection sequence number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

impl Envelope {
    /// Enforces the kind-dependent structural constraints of
    /// `envelope.schema.json`'s `allOf`.
    ///
    /// Marker, version, kind, and pattern validity are already guaranteed by
    /// deserialization; this method adds the cross-field rules that `serde`
    /// cannot express:
    ///
    /// * **request** — `id` and `method` required; `result`, `error`, `event`
    ///   forbidden.
    /// * **response** — `id` required; exactly one of `result` / `error`;
    ///   `method`, `event`, `params` forbidden.
    /// * **event** — `event` required; `id`, `method`, `result`, `error`
    ///   forbidden.
    ///
    /// Returns [`CodecError::StructuralViolation`] describing the first
    /// violation found.
    pub fn validate(&self) -> Result<(), CodecError> {
        match self.kind {
            Kind::Request => {
                if self.id.is_none() {
                    return Err(violation("request envelope requires `id`"));
                }
                if self.method.is_none() {
                    return Err(violation("request envelope requires `method`"));
                }
                if self.result.is_some() {
                    return Err(violation("request envelope must not carry `result`"));
                }
                if self.error.is_some() {
                    return Err(violation("request envelope must not carry `error`"));
                }
                if self.event.is_some() {
                    return Err(violation("request envelope must not carry `event`"));
                }
            }
            Kind::Response => {
                if self.id.is_none() {
                    return Err(violation("response envelope requires `id`"));
                }
                match (self.result.is_some(), self.error.is_some()) {
                    (true, true) => {
                        return Err(violation(
                            "response envelope must not carry both `result` and `error`",
                        ));
                    }
                    (false, false) => {
                        return Err(violation(
                            "response envelope requires exactly one of `result` or `error`",
                        ));
                    }
                    _ => {}
                }
                if self.method.is_some() {
                    return Err(violation("response envelope must not carry `method`"));
                }
                if self.event.is_some() {
                    return Err(violation("response envelope must not carry `event`"));
                }
                if self.params.is_some() {
                    return Err(violation("response envelope must not carry `params`"));
                }
            }
            Kind::Event => {
                if self.event.is_none() {
                    return Err(violation("event envelope requires `event`"));
                }
                if self.id.is_some() {
                    return Err(violation("event envelope must not carry `id`"));
                }
                if self.method.is_some() {
                    return Err(violation("event envelope must not carry `method`"));
                }
                if self.result.is_some() {
                    return Err(violation("event envelope must not carry `result`"));
                }
                if self.error.is_some() {
                    return Err(violation("event envelope must not carry `error`"));
                }
            }
        }
        Ok(())
    }
}

fn violation(message: &str) -> CodecError {
    CodecError::StructuralViolation(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(json: &str) -> Envelope {
        let original: serde_json::Value = serde_json::from_str(json).unwrap();
        let env: Envelope = serde_json::from_str(json).unwrap();
        let reserialized = serde_json::to_value(&env).unwrap();
        assert_eq!(reserialized, original);
        env
    }

    const REQUEST: &str = r#"{
        "bridge": "norves.editor.bridge",
        "version": "0.1",
        "kind": "request",
        "id": "req-42",
        "method": "runtime.play",
        "params": {}
    }"#;

    const RESPONSE_RESULT: &str = r#"{
        "bridge": "norves.editor.bridge",
        "version": "0.1",
        "kind": "response",
        "id": "req-1",
        "sessionId": "sess-7f3a",
        "result": { "ok": true }
    }"#;

    const RESPONSE_ERROR: &str = r#"{
        "bridge": "norves.editor.bridge",
        "version": "0.1",
        "kind": "response",
        "id": "req-42",
        "error": {
            "code": "METHOD_NOT_SUPPORTED",
            "message": "unsupported",
            "data": { "method": "runtime.play" }
        }
    }"#;

    const EVENT: &str = r#"{
        "bridge": "norves.editor.bridge",
        "version": "0.1",
        "kind": "event",
        "event": "log.message",
        "sessionId": "sess-7f3a",
        "seq": 12,
        "params": { "level": "info", "message": "Game started" }
    }"#;

    #[test]
    fn positive_envelopes_round_trip_and_validate() {
        for json in [REQUEST, RESPONSE_RESULT, RESPONSE_ERROR, EVENT] {
            let env = round_trip(json);
            env.validate().unwrap();
        }
    }

    #[test]
    fn validate_rejects_event_with_id() {
        let json = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "event",
            "id": "req-42",
            "event": "log.message"
        }"#;
        let env: Envelope = serde_json::from_str(json).unwrap();
        assert!(matches!(
            env.validate(),
            Err(CodecError::StructuralViolation(_))
        ));
    }

    #[test]
    fn validate_rejects_request_with_result() {
        let json = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "request",
            "id": "req-42",
            "method": "runtime.play",
            "result": { "ok": true }
        }"#;
        let env: Envelope = serde_json::from_str(json).unwrap();
        assert!(matches!(
            env.validate(),
            Err(CodecError::StructuralViolation(_))
        ));
    }

    #[test]
    fn validate_rejects_response_with_result_and_error() {
        let json = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "response",
            "id": "req-42",
            "result": { "ok": true },
            "error": { "code": "METHOD_NOT_SUPPORTED", "message": "both" }
        }"#;
        let env: Envelope = serde_json::from_str(json).unwrap();
        assert!(matches!(
            env.validate(),
            Err(CodecError::StructuralViolation(_))
        ));
    }

    #[test]
    fn validate_rejects_response_without_result_or_error() {
        let json = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "response",
            "id": "req-42"
        }"#;
        let env: Envelope = serde_json::from_str(json).unwrap();
        assert!(matches!(
            env.validate(),
            Err(CodecError::StructuralViolation(_))
        ));
    }

    #[test]
    fn validate_rejects_missing_required_per_kind() {
        // request missing id and method
        let req = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "request",
            "params": {}
        }"#;
        assert!(serde_json::from_str::<Envelope>(req)
            .unwrap()
            .validate()
            .is_err());

        // response missing id
        let resp = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "response",
            "result": { "ok": true }
        }"#;
        assert!(serde_json::from_str::<Envelope>(resp)
            .unwrap()
            .validate()
            .is_err());

        // event missing event
        let evt = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "event",
            "params": {}
        }"#;
        assert!(serde_json::from_str::<Envelope>(evt)
            .unwrap()
            .validate()
            .is_err());
    }

    #[test]
    fn deserialize_rejects_unknown_field() {
        let json = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "request",
            "id": "req-42",
            "method": "runtime.play",
            "params": {},
            "metohd": "runtime.play"
        }"#;
        assert!(serde_json::from_str::<Envelope>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_wrong_bridge_marker() {
        let json = r#"{
            "bridge": "some.other.bridge",
            "version": "0.1",
            "kind": "request",
            "id": "req-42",
            "method": "runtime.play",
            "params": {}
        }"#;
        assert!(serde_json::from_str::<Envelope>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_invalid_kind() {
        let json = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "notification",
            "event": "log.message",
            "params": {}
        }"#;
        assert!(serde_json::from_str::<Envelope>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_error_missing_code() {
        let json = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "response",
            "id": "req-42",
            "error": { "message": "no code" }
        }"#;
        assert!(serde_json::from_str::<Envelope>(json).is_err());
    }
}
