//! JSON codec entry points for the Bridge wire [`Envelope`].
//!
//! These thin wrappers funnel all envelope (de)serialization through one place
//! so callers do not reach for `serde_json` directly. They cover only the
//! *envelope* layer; per-method / per-event payload (`params` / `result`)
//! schema validation is a later phase (see the payload fixtures, currently
//! accepted unchecked at this layer).
//!
//! * [`decode_envelope`] — deserialize only (marker / pattern / unknown-field
//!   rules from `serde`), without the kind-dependent cross-field checks.
//! * [`decode_validated`] — [`decode_envelope`] followed by
//!   [`Envelope::validate`], rejecting structurally invalid envelopes.
//! * [`encode_envelope`] — serialize an [`Envelope`] back to a JSON string.

use crate::envelope::{Envelope, ValidatedEnvelope};
use crate::error::CodecError;

/// Deserializes a JSON string into an [`Envelope`].
///
/// Enforces only what `serde` can: the `bridge` marker constant, validated
/// newtype patterns, and `deny_unknown_fields`. The kind-dependent structural
/// rules are **not** applied here — use [`decode_validated`] when those must
/// hold.
///
/// # Errors
///
/// Returns [`CodecError::Json`] on malformed JSON or a deserialization failure
/// (including newtype pattern / marker rejections surfaced by `serde`).
pub fn decode_envelope(json: &str) -> Result<Envelope, CodecError> {
    let envelope = serde_json::from_str(json)?;
    Ok(envelope)
}

/// Serializes an [`Envelope`] to a compact JSON string.
///
/// # Errors
///
/// Returns [`CodecError::Json`] if serialization fails.
pub fn encode_envelope(env: &Envelope) -> Result<String, CodecError> {
    let json = serde_json::to_string(env)?;
    Ok(json)
}

/// Deserializes a JSON string into an [`Envelope`] and applies the
/// kind-dependent structural rules via [`Envelope::validate`].
///
/// This is the strict decode path: both the `serde` layer and
/// [`Envelope::validate`] must succeed.
///
/// # Errors
///
/// Returns [`CodecError::Json`] on a deserialization failure, or the error from
/// [`Envelope::validate`] (typically [`CodecError::StructuralViolation`]) when
/// the envelope is structurally invalid for its `kind`.
pub fn decode_validated(json: &str) -> Result<Envelope, CodecError> {
    let envelope = decode_envelope(json)?;
    envelope.validate()?;
    Ok(envelope)
}

/// Deserializes a JSON string directly into a type-safe [`ValidatedEnvelope`].
///
/// Equivalent to [`decode_envelope`] followed by
/// [`ValidatedEnvelope::try_from`], which runs [`Envelope::validate`]
/// internally. The kind-dependent field combinations are thus encoded in the
/// returned type rather than re-checked by the caller.
///
/// # Errors
///
/// Returns [`CodecError::Json`] on a deserialization failure, or the error from
/// [`Envelope::validate`] (typically [`CodecError::StructuralViolation`]) when
/// the envelope is structurally invalid for its `kind`.
pub fn decode_typed(json: &str) -> Result<ValidatedEnvelope, CodecError> {
    let envelope = decode_envelope(json)?;
    ValidatedEnvelope::try_from(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST: &str = r#"{
        "bridge": "norves.editor.bridge",
        "version": "0.1",
        "kind": "request",
        "id": "req-42",
        "method": "runtime.play",
        "params": {}
    }"#;

    #[test]
    fn decode_then_encode_round_trips_via_value() {
        let original: serde_json::Value = serde_json::from_str(REQUEST).unwrap();
        let env = decode_validated(REQUEST).unwrap();
        let encoded = encode_envelope(&env).unwrap();
        let round_trip: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(original, round_trip);
    }

    #[test]
    fn decode_envelope_skips_structural_rules() {
        // Event carrying `id` is structurally invalid but deserializes fine.
        let json = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "event",
            "id": "req-42",
            "event": "log.message"
        }"#;
        assert!(decode_envelope(json).is_ok());
        assert!(decode_validated(json).is_err());
    }

    #[test]
    fn decode_envelope_propagates_json_error() {
        assert!(matches!(
            decode_envelope("{ not json"),
            Err(CodecError::Json(_))
        ));
    }

    #[test]
    fn decode_typed_lifts_valid_envelope() {
        let typed = decode_typed(REQUEST).unwrap();
        assert!(matches!(
            typed,
            crate::envelope::ValidatedEnvelope::Request { .. }
        ));
    }

    #[test]
    fn decode_typed_rejects_structural_violation() {
        // Event carrying `id` deserializes but fails validation.
        let json = r#"{
            "bridge": "norves.editor.bridge",
            "version": "0.1",
            "kind": "event",
            "id": "req-42",
            "event": "log.message"
        }"#;
        assert!(matches!(
            decode_typed(json),
            Err(CodecError::StructuralViolation(_))
        ));
    }
}
