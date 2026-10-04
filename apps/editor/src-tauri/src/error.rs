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
    /// エンジン設定の保存やファイル選択ダイアログの失敗。
    #[serde(rename_all = "camelCase")]
    Settings { message: String },
    /// 編集列が満杯で要求を受け付けられなかった。
    #[allow(dead_code)]
    EditQueueFull,
    /// 編集サービスが停止中で要求を受け付けられなかった。
    EditServiceStopping,
    /// Bridge呼び出しの開始前に要求が取り消された。
    EditCancelled,
    /// MCP の設定を安全に読み書きできなかった。
    #[allow(dead_code)]
    McpSettingsStorage,
    /// MCP トークンの保護・保存・読み出しに失敗した。
    #[allow(dead_code)]
    McpTokenStorage,
    /// MCP要求の認証改訂が失効した。
    McpAuthorizationRevoked,
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
            BackendError::Settings { message } => write!(f, "{message}"),
            BackendError::EditQueueFull => write!(f, "編集要求の待ち列が満杯です"),
            BackendError::EditServiceStopping => write!(f, "編集サービスは停止中です"),
            BackendError::EditCancelled => write!(f, "編集要求は取り消されました"),
            BackendError::McpSettingsStorage => {
                write!(f, "MCP の設定を安全に読み書きできませんでした")
            }
            BackendError::McpTokenStorage => {
                write!(f, "MCP トークンを安全に保護または保存できませんでした")
            }
            BackendError::McpAuthorizationRevoked => {
                write!(f, "MCP 要求の認証が更新または停止されました")
            }
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

    #[test]
    fn mcp_storage_errors_do_not_accept_secret_text() {
        let settings = BackendError::McpSettingsStorage;
        let token = BackendError::McpTokenStorage;
        let rendered = format!("{settings} {token}");
        assert!(!rendered.contains("token-value"));
        assert_eq!(
            serde_json::to_value(settings).expect("serializes"),
            serde_json::json!({ "kind": "mcpSettingsStorage" })
        );
        assert_eq!(
            serde_json::to_value(token).expect("serializes"),
            serde_json::json!({ "kind": "mcpTokenStorage" })
        );
    }
}
