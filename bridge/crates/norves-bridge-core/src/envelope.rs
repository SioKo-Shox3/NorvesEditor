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

/// A structurally validated envelope with kind-dependent fields encoded in the
/// type system.
///
/// Where [`Envelope`] is a flat struct whose per-kind field combinations are
/// only enforced at runtime by [`Envelope::validate`], `ValidatedEnvelope`
/// lifts those combinations into the type system: each variant carries exactly
/// the fields its kind permits, with the `Option`s that `validate` guarantees
/// present already unwrapped.
///
/// Construct it via [`TryFrom<Envelope>`], which runs [`Envelope::validate`]
/// internally and therefore inherits its structural guarantees. Convert back
/// with [`From<ValidatedEnvelope>`] for [`Envelope`]; the two directions are
/// round-trip equivalent.
///
/// `#[non_exhaustive]` so that adding a future kind is not a breaking change.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ValidatedEnvelope {
    /// A method request. `id` and `method` are always present.
    Request {
        /// Protocol version string, `MAJOR.MINOR`.
        version: VersionString,
        /// Request/response correlation id.
        id: CorrelationId,
        /// Method name being invoked.
        method: MethodName,
        /// Optional method payload.
        params: Option<serde_json::Map<String, serde_json::Value>>,
        /// Optional session id assigned during the handshake.
        session_id: Option<String>,
        /// Optional monotonically increasing per-connection sequence number.
        seq: Option<u64>,
    },
    /// A response to a request. Carries exactly one of result / error via
    /// [`ResponsePayload`].
    Response {
        /// Protocol version string, `MAJOR.MINOR`.
        version: VersionString,
        /// Request/response correlation id.
        id: CorrelationId,
        /// The success or error payload (exactly one).
        payload: ResponsePayload,
        /// Optional session id assigned during the handshake.
        session_id: Option<String>,
        /// Optional monotonically increasing per-connection sequence number.
        seq: Option<u64>,
    },
    /// A one-way event. `event` is always present.
    Event {
        /// Protocol version string, `MAJOR.MINOR`.
        version: VersionString,
        /// Event name.
        event: EventName,
        /// Optional event payload.
        params: Option<serde_json::Map<String, serde_json::Value>>,
        /// Optional session id assigned during the handshake.
        session_id: Option<String>,
        /// Optional monotonically increasing per-connection sequence number.
        seq: Option<u64>,
    },
}

/// The mutually-exclusive payload of a response [`ValidatedEnvelope::Response`].
///
/// `Envelope::validate` guarantees a response carries exactly one of `result`
/// or `error`; this enum makes that exclusivity total instead of a runtime
/// invariant.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponsePayload {
    /// Success payload (the envelope's `result` field).
    Result(serde_json::Value),
    /// Error payload (the envelope's `error` field).
    Error(BridgeError),
}

impl TryFrom<Envelope> for ValidatedEnvelope {
    type Error = CodecError;

    /// Validates `env` via [`Envelope::validate`] and lifts it into the typed
    /// representation.
    ///
    /// Reuses [`Envelope::validate`] for all structural checks, so this never
    /// duplicates the per-kind rules. After validation succeeds the `Option`s
    /// that the validated kind guarantees present are unwrapped; the defensive
    /// `Err(violation(...))` arms below can only be reached if `validate` and
    /// this destructuring disagree, which the round-trip tests pin down.
    fn try_from(env: Envelope) -> Result<Self, <Self as TryFrom<Envelope>>::Error> {
        env.validate()?;

        let Envelope {
            bridge: _,
            version,
            kind,
            id,
            method,
            event,
            params,
            result,
            error,
            session_id,
            seq,
        } = env;

        match kind {
            Kind::Request => match (id, method) {
                (Some(id), Some(method)) => Ok(ValidatedEnvelope::Request {
                    version,
                    id,
                    method,
                    params,
                    session_id,
                    seq,
                }),
                // `validate` guarantees both are present for a request.
                _ => Err(violation("request envelope requires `id` and `method`")),
            },
            Kind::Response => {
                let id = match id {
                    Some(id) => id,
                    // `validate` guarantees `id` is present for a response.
                    None => return Err(violation("response envelope requires `id`")),
                };
                let payload = match (result, error) {
                    (Some(result), None) => ResponsePayload::Result(result),
                    (None, Some(error)) => ResponsePayload::Error(error),
                    // `validate` guarantees exactly one of result / error.
                    _ => {
                        return Err(violation(
                            "response envelope requires exactly one of `result` or `error`",
                        ))
                    }
                };
                Ok(ValidatedEnvelope::Response {
                    version,
                    id,
                    payload,
                    session_id,
                    seq,
                })
            }
            Kind::Event => match event {
                Some(event) => Ok(ValidatedEnvelope::Event {
                    version,
                    event,
                    params,
                    session_id,
                    seq,
                }),
                // `validate` guarantees `event` is present for an event.
                None => Err(violation("event envelope requires `event`")),
            },
        }
    }
}

impl From<ValidatedEnvelope> for Envelope {
    /// Flattens a [`ValidatedEnvelope`] back into a wire [`Envelope`].
    ///
    /// Because a `ValidatedEnvelope` can only be constructed through
    /// [`TryFrom<Envelope>`], which enforces [`Envelope::validate`], every
    /// `Envelope` produced here already satisfies those invariants and is thus
    /// guaranteed to pass [`Envelope::validate`] with `Ok`. The two conversions
    /// are round-trip equivalent (typed -> flat -> typed and
    /// flat -> typed -> flat preserve meaning).
    fn from(validated: ValidatedEnvelope) -> Self {
        match validated {
            ValidatedEnvelope::Request {
                version,
                id,
                method,
                params,
                session_id,
                seq,
            } => Envelope {
                bridge: BridgeMarker,
                version,
                kind: Kind::Request,
                id: Some(id),
                method: Some(method),
                event: None,
                params,
                result: None,
                error: None,
                session_id,
                seq,
            },
            ValidatedEnvelope::Response {
                version,
                id,
                payload,
                session_id,
                seq,
            } => {
                let (result, error) = match payload {
                    ResponsePayload::Result(result) => (Some(result), None),
                    ResponsePayload::Error(error) => (None, Some(error)),
                };
                Envelope {
                    bridge: BridgeMarker,
                    version,
                    kind: Kind::Response,
                    id: Some(id),
                    method: None,
                    event: None,
                    params: None,
                    result,
                    error,
                    session_id,
                    seq,
                }
            }
            ValidatedEnvelope::Event {
                version,
                event,
                params,
                session_id,
                seq,
            } => Envelope {
                bridge: BridgeMarker,
                version,
                kind: Kind::Event,
                id: None,
                method: None,
                event: Some(event),
                params,
                result: None,
                error: None,
                session_id,
                seq,
            },
        }
    }
}

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

    fn envelope_of(json: &str) -> Envelope {
        serde_json::from_str(json).expect("fixture JSON deserializes")
    }

    #[test]
    fn try_from_request_lifts_fields() {
        let validated = ValidatedEnvelope::try_from(envelope_of(REQUEST))
            .expect("valid request lifts to typed");
        match validated {
            ValidatedEnvelope::Request {
                version,
                id,
                method,
                params,
                session_id,
                seq,
            } => {
                assert_eq!(version.as_str(), "0.1");
                assert_eq!(id.as_str(), "req-42");
                assert_eq!(method.as_str(), "runtime.play");
                assert_eq!(params, Some(serde_json::Map::new()));
                assert_eq!(session_id, None);
                assert_eq!(seq, None);
            }
            other => panic!("expected Request, got {other:?}"),
        }
    }

    #[test]
    fn try_from_response_result_lifts_to_result_payload() {
        let validated = ValidatedEnvelope::try_from(envelope_of(RESPONSE_RESULT))
            .expect("valid response(result) lifts to typed");
        match validated {
            ValidatedEnvelope::Response {
                id,
                payload,
                session_id,
                ..
            } => {
                assert_eq!(id.as_str(), "req-1");
                assert_eq!(session_id.as_deref(), Some("sess-7f3a"));
                match payload {
                    ResponsePayload::Result(value) => {
                        assert_eq!(value, serde_json::json!({ "ok": true }));
                    }
                    ResponsePayload::Error(_) => panic!("expected Result payload"),
                }
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn try_from_response_error_lifts_to_error_payload() {
        let validated = ValidatedEnvelope::try_from(envelope_of(RESPONSE_ERROR))
            .expect("valid response(error) lifts to typed");
        match validated {
            ValidatedEnvelope::Response { payload, .. } => match payload {
                ResponsePayload::Error(err) => {
                    assert_eq!(err.code.as_str(), "METHOD_NOT_SUPPORTED");
                }
                ResponsePayload::Result(_) => panic!("expected Error payload"),
            },
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn try_from_event_lifts_fields() {
        let validated =
            ValidatedEnvelope::try_from(envelope_of(EVENT)).expect("valid event lifts to typed");
        match validated {
            ValidatedEnvelope::Event {
                event,
                session_id,
                seq,
                ..
            } => {
                assert_eq!(event.as_str(), "log.message");
                assert_eq!(session_id.as_deref(), Some("sess-7f3a"));
                assert_eq!(seq, Some(12));
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[test]
    fn try_from_rejects_structurally_invalid() {
        // Event carrying `id` is a structural violation.
        let bad_event = envelope_of(
            r#"{
                "bridge": "norves.editor.bridge",
                "version": "0.1",
                "kind": "event",
                "id": "req-42",
                "event": "log.message"
            }"#,
        );
        assert!(matches!(
            ValidatedEnvelope::try_from(bad_event),
            Err(CodecError::StructuralViolation(_))
        ));

        // Request missing `method` is a structural violation.
        let bad_request = envelope_of(
            r#"{
                "bridge": "norves.editor.bridge",
                "version": "0.1",
                "kind": "request",
                "id": "req-42"
            }"#,
        );
        assert!(matches!(
            ValidatedEnvelope::try_from(bad_request),
            Err(CodecError::StructuralViolation(_))
        ));

        // Response with both result and error is a structural violation.
        let bad_response = envelope_of(
            r#"{
                "bridge": "norves.editor.bridge",
                "version": "0.1",
                "kind": "response",
                "id": "req-42",
                "result": { "ok": true },
                "error": { "code": "METHOD_NOT_SUPPORTED", "message": "both" }
            }"#,
        );
        assert!(matches!(
            ValidatedEnvelope::try_from(bad_response),
            Err(CodecError::StructuralViolation(_))
        ));
    }

    #[test]
    fn from_validated_always_passes_validate() {
        for json in [REQUEST, RESPONSE_RESULT, RESPONSE_ERROR, EVENT] {
            let validated =
                ValidatedEnvelope::try_from(envelope_of(json)).expect("fixture lifts to typed");
            let restored = Envelope::from(validated);
            restored
                .validate()
                .expect("Envelope from ValidatedEnvelope must always validate Ok");
        }
    }

    #[test]
    fn typed_flat_typed_round_trip_is_identity() {
        for json in [REQUEST, RESPONSE_RESULT, RESPONSE_ERROR, EVENT] {
            let validated =
                ValidatedEnvelope::try_from(envelope_of(json)).expect("fixture lifts to typed");
            let restored = Envelope::from(validated.clone());
            let again = ValidatedEnvelope::try_from(restored).expect("flattened envelope re-lifts");
            assert_eq!(validated, again, "typed -> flat -> typed must be identity");
        }
    }

    #[test]
    fn flat_typed_flat_round_trip_is_identity() {
        for json in [REQUEST, RESPONSE_RESULT, RESPONSE_ERROR, EVENT] {
            let original = envelope_of(json);
            let validated =
                ValidatedEnvelope::try_from(original.clone()).expect("fixture lifts to typed");
            let restored = Envelope::from(validated);
            assert_eq!(original, restored, "flat -> typed -> flat must be identity");
        }
    }
}
