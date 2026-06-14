//! Bridge error types.
//!
//! Two distinct error concepts live here:
//!
//! * [`BridgeError`] is the *wire* error object carried inside a response
//!   envelope (engine- or backend-originated). It mirrors the `error` `$def`
//!   in `envelope.schema.json`.
//! * [`CodecError`] is a *local* Rust failure raised while decoding or
//!   validating an envelope (deserialize failure, structural-constraint
//!   violation, pattern violation). It never travels over the wire.
//!
//! Per `error-model.md` the error-code registry is **open**: [`ErrorCode`] is a
//! validated string newtype, not a closed enum, so engines may advertise codes
//! beyond the core set defined here.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Screaming-snake-case error code, e.g. `BRIDGE_TRANSPORT_ERROR`.
///
/// Matches the schema pattern `^[A-Z][A-Z0-9_]*$`. The registry is open
/// (`error-model.md`), so this is a validated newtype rather than a closed
/// enum; unknown-but-well-formed codes round-trip unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ErrorCode(String);

impl ErrorCode {
    /// Returns the underlying code string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(value: &str) -> bool {
        let mut chars = value.chars();
        match chars.next() {
            Some(first) if first.is_ascii_uppercase() => {}
            _ => return false,
        }
        chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    }
}

impl TryFrom<String> for ErrorCode {
    type Error = CodecError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if Self::is_valid(&value) {
            Ok(ErrorCode(value))
        } else {
            Err(CodecError::InvalidErrorCode(value))
        }
    }
}

impl From<ErrorCode> for String {
    fn from(value: ErrorCode) -> Self {
        value.0
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Core error codes defined by the generic Bridge protocol.
///
/// These are provided as helpers only; the registry is open, so engines may
/// use additional codes. See `error-model.md`.
impl ErrorCode {
    /// The peer requested a protocol version this endpoint cannot speak.
    pub fn protocol_version_unsupported() -> ErrorCode {
        ErrorCode("PROTOCOL_VERSION_UNSUPPORTED".to_owned())
    }

    /// The requested method is not supported by the engine in its current state.
    pub fn method_not_supported() -> ErrorCode {
        ErrorCode("METHOD_NOT_SUPPORTED".to_owned())
    }

    /// A transport-level failure occurred on the Bridge connection.
    pub fn bridge_transport_error() -> ErrorCode {
        ErrorCode("BRIDGE_TRANSPORT_ERROR".to_owned())
    }
}

/// Wire error object carried in a response envelope's `error` field.
///
/// Mirrors the `error` `$def` of `envelope.schema.json`
/// (`additionalProperties: false`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeError {
    /// Symbolic, screaming-snake-case error code.
    pub code: ErrorCode,
    /// Human-readable error message.
    pub message: String,
    /// Optional structured, error-code-specific detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Local failure raised while decoding or validating a Bridge envelope.
///
/// Distinct from [`BridgeError`]: this represents a Rust-side processing
/// failure (malformed input, structural-constraint violation), never an
/// engine-originated wire error. All decode-path errors funnel into this type.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// JSON (de)serialization failed.
    #[error("JSON (de)serialization failed: {0}")]
    Json(#[from] serde_json::Error),

    /// The `bridge` marker was not the expected constant.
    #[error("invalid bridge marker: expected {expected:?}, got {actual:?}")]
    InvalidBridgeMarker {
        /// Expected marker constant.
        expected: &'static str,
        /// Marker value actually seen.
        actual: String,
    },

    /// The `version` string did not match the `MAJOR.MINOR` pattern.
    #[error("invalid version string: {0:?}")]
    InvalidVersion(String),

    /// An error code did not match the `^[A-Z][A-Z0-9_]*$` pattern.
    #[error("invalid error code: {0:?}")]
    InvalidErrorCode(String),

    /// A validated string newtype failed its pattern / length constraint.
    #[error("invalid {field}: {value:?}")]
    InvalidField {
        /// Name of the field that failed validation.
        field: &'static str,
        /// The offending value.
        value: String,
    },

    /// An envelope violated a kind-dependent structural constraint.
    #[error("envelope structural violation: {0}")]
    StructuralViolation(String),
}
