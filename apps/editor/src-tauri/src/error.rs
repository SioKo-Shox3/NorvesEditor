//! The single error type every Tauri command returns to the frontend.
//!
//! Tauri serializes a command's `Err` value to JSON and rejects the JS promise
//! with it, so [`BackendError`] is `Serialize`. It is a flat, tagged enum: the
//! frontend can branch on the `kind` tag and show a message. All bridge-layer
//! failures (`ConnectError`, `RequestError`, an engine `BridgeError`,
//! `HandshakeError`) are funneled into one of these variants here so the
//! command bodies stay thin.

use norves_bridge_core::BridgeError;
use norves_bridge_editor_client::{ConnectError, HandshakeError, RequestError};
use serde::Serialize;

/// Error returned by every `#[tauri::command]` in this crate.
///
/// `#[serde(tag = "kind", ...)]` gives the frontend a discriminated union it can
/// `switch` on. Field names are camelCase to match the TS convention.
// P6: mirror this shape in a TS type (discriminated union on `kind`).
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BackendError {
    /// A command needing a live connection was called while disconnected.
    NotConnected,
    /// `bridge_connect` was called while already connected or connecting.
    AlreadyConnected,
    /// The transport could not be established within the retry budget.
    #[serde(rename_all = "camelCase")]
    Connect { message: String },
    /// A request failed at the transport/timeout/encode layer (not an engine
    /// protocol error — that is [`BackendError::Engine`]).
    #[serde(rename_all = "camelCase")]
    Request { message: String },
    /// The engine answered with a protocol error response (`code` + `message`).
    #[serde(rename_all = "camelCase")]
    Engine { code: String, message: String },
    /// The `bridge.hello` handshake failed (malformed result or rejected
    /// version negotiation).
    #[serde(rename_all = "camelCase")]
    Handshake { message: String },
    /// A process-lifecycle failure (engine path resolution/validation, spawn,
    /// or the READY-handshake) before or around launching the engine.
    #[serde(rename_all = "camelCase")]
    Process { message: String },
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::NotConnected => write!(f, "not connected to the bridge"),
            BackendError::AlreadyConnected => {
                write!(f, "already connected or a connect is in progress")
            }
            BackendError::Connect { message } => write!(f, "connect failed: {message}"),
            BackendError::Request { message } => write!(f, "request failed: {message}"),
            BackendError::Engine { code, message } => {
                write!(f, "engine error {code}: {message}")
            }
            BackendError::Handshake { message } => write!(f, "handshake failed: {message}"),
            BackendError::Process { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for BackendError {}

impl From<ConnectError> for BackendError {
    fn from(err: ConnectError) -> Self {
        BackendError::Connect {
            message: err.to_string(),
        }
    }
}

impl From<RequestError> for BackendError {
    fn from(err: RequestError) -> Self {
        BackendError::Request {
            message: err.to_string(),
        }
    }
}

impl From<HandshakeError> for BackendError {
    fn from(err: HandshakeError) -> Self {
        BackendError::Handshake {
            message: err.to_string(),
        }
    }
}

impl From<BridgeError> for BackendError {
    /// Maps an engine protocol error response into [`BackendError::Engine`],
    /// preserving the stable error `code` string and human message for the UI.
    fn from(err: BridgeError) -> Self {
        BackendError::Engine {
            code: err.code.as_str().to_owned(),
            message: err.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norves_bridge_core::ErrorCode;

    #[test]
    fn not_connected_serializes_with_kind_tag() {
        let json = serde_json::to_value(BackendError::NotConnected).expect("serializes");
        assert_eq!(json, serde_json::json!({ "kind": "notConnected" }));
    }

    #[test]
    fn connect_serializes_message() {
        let json = serde_json::to_value(BackendError::Connect {
            message: "boom".to_owned(),
        })
        .expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({ "kind": "connect", "message": "boom" })
        );
    }

    #[test]
    fn bridge_error_maps_to_engine_variant() {
        let bridge_err = BridgeError {
            code: ErrorCode::method_not_supported(),
            message: "no such method".to_owned(),
            data: None,
        };
        let backend: BackendError = bridge_err.into();
        let json = serde_json::to_value(&backend).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "engine",
                "code": "METHOD_NOT_SUPPORTED",
                "message": "no such method"
            })
        );
    }
}
