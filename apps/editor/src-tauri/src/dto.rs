//! DTOs the backend *synthesizes* and serializes to the frontend.
//!
//! Where the backend forwards a raw engine `result` / event `params`
//! ([`serde_json::Value`]), no struct exists here on purpose — re-modeling would
//! risk drift from the wire schema. The synthesized payloads defined here use
//! camelCase to match the TS convention.

use norves_bridge_core::CapabilityDescriptor;
use serde::{Deserialize, Serialize};

use crate::process::EnginePathSource;

/// Payload of the `bridge:connection-state` event AND the value returned by
/// `bridge_connect` / `bridge_reconnect`.
///
/// `connected = true` carries `session_id` / `server_name` / `endpoint` plus the
/// authoritative capability descriptors; `connected = false` carries an
/// optional `reason` and omits capabilities.
// P6: mirror this shape in a TS type (camelCase fields).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatePayload {
    /// Whether a live, handshaken connection currently exists.
    pub connected: bool,
    /// Session id assigned by the engine at handshake (connected only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Engine endpoint product name from the handshake (connected only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// The `ws://` endpoint dialed (connected only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Authoritative capabilities for this connected session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<CapabilityDescriptor>>,
    /// Human-readable reason for a disconnect (disconnected only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// エンジン設定のコマンド(`get_engine_settings` / `pick_engine_path` / `clear_engine_path`)が返す値。
// bridge-ui/src/ipc-types.ts の EngineSettingsPayload と形を揃える。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EngineSettingsPayload {
    /// `launch_engine` が次に使うパス(環境変数 > 設定 > 既定値 で解決したもの)。
    pub effective_path: String,
    /// `effective_path` の出所。
    pub source: EnginePathSource,
    /// 保存済みのパス。未設定なら `None`。
    pub saved_path: Option<String>,
    /// 保存済みの起動引数。未設定なら空。
    pub saved_args: Vec<String>,
}

/// `get_mcp_settings` と設定変更コマンドが返す、秘密を含まない MCP 状態。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpSettingsPayload {
    pub enabled: bool,
    pub port: u16,
    pub write_mode: crate::mcp::McpWriteMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_root_id: Option<String>,
    pub state: McpServerStateDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// loopback MCP サーバーの公開状態。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum McpServerStateDto {
    Disabled,
    Running,
    BindFailed,
    StorageFailed,
}

/// 明示的な秘密表示コマンドだけが返す MCP トークン。
#[derive(Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpTokenPayload {
    pub token: String,
}

/// main画面で一度だけ承認できるMCP書き込み確認。
#[derive(Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpConfirmationRequestDto {
    pub id: String,
    pub tool_name: String,
    pub method: String,
    pub target_ids: Vec<String>,
    pub target_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<EditSourceDto>,
    pub undo_available: bool,
    pub clears_history: bool,
    pub history_generation: Option<u64>,
    pub history_revision: u64,
    pub undo_head_id: Option<u64>,
    pub expires_at: u64,
}

/// Payload returned by workspace management commands.
// Phase A: mirror this shape in bridge-ui/src/ipc-types.ts.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePayload {
    pub root_path: String,
    pub assets_root: String,
    pub name: String,
}

/// One asset entry returned by the offline manifest reader.
// Phase B: mirror this shape in bridge-ui/src/ipc-types.ts.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssetEntryDto {
    pub logical_path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooked_package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooked_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooked_version: Option<u32>,
}

/// Payload returned by `asset_read_manifest`.
// Phase B: mirror this shape in bridge-ui/src/ipc-types.ts.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssetManifestPayload {
    pub version: u32,
    pub manifest_path: String,
    pub assets: Vec<AssetEntryDto>,
}

/// 画面から値編集へ渡す、表示スナップショット由来の捕捉値。
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiPropertyCaptureDto {
    pub generation: u64,
    pub revision: u64,
    pub value: serde_json::Value,
}

/// 画面から親変更へ渡す、表示ツリー由来の捕捉値。`None` はシーン直下。
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiParentCaptureDto {
    pub generation: u64,
    pub revision: u64,
    pub parent_id: Option<String>,
}

/// 編集・履歴の出どころ。Bridge wire protocolの値ではない。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EditSourceDto {
    Ui,
    Mcp,
}

/// 取り消し・やり直しの先頭まとまりを表示する要約。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditGroupSummaryDto {
    pub id: String,
    pub name: String,
    pub source: EditSourceDto,
    pub count: usize,
    /// グループ開始時刻。Unix epochからのミリ秒。
    pub created_at: u64,
}

/// 再試行または破棄が必要な、部分適用状態のまとまり。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditPendingGroupDto {
    pub id: String,
    pub name: String,
    pub direction: String,
    pub source: EditSourceDto,
    pub created_at: u64,
    pub total_count: usize,
    pub completed_count: usize,
    pub outcome_unknown: bool,
    pub retry_allowed: bool,
}

/// 破棄後に残る適用状態。自動で巻き戻さない。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditDiscardResultDto {
    pub group_id: String,
    pub completed_count: usize,
    pub total_count: usize,
    pub outcome_unknown: bool,
    pub changes_remain: bool,
}

/// 画面初期取得と履歴変更イベントで共有するサービス要約。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditHistorySummaryDto {
    pub generation: Option<u64>,
    pub history_revision: u64,
    pub applied_revision: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_head_id: Option<u64>,
    pub undo_revision: u64,
    pub undo_group: Option<EditGroupSummaryDto>,
    pub redo_head_id: Option<u64>,
    pub redo_revision: u64,
    pub redo_group: Option<EditGroupSummaryDto>,
    pub pending: bool,
    pub pending_group: Option<EditPendingGroupDto>,
}

/// 共通サービスが適用した編集を画面へ伝えるイベント。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EditAppliedDto {
    pub operation: String,
    pub object_id: Option<String>,
    pub property: Option<String>,
    pub value: Option<serde_json::Value>,
    pub new_id: Option<String>,
    pub source: EditSourceDto,
    pub group_id: String,
    pub generation: u64,
    pub sequence: u64,
    pub history_revision: u64,
    pub applied_revision: u64,
}

impl ConnectionStatePayload {
    /// Builds the connected-state payload from a completed handshake.
    pub fn connected(
        session_id: String,
        server_name: String,
        endpoint: String,
        capabilities: Vec<CapabilityDescriptor>,
    ) -> Self {
        ConnectionStatePayload {
            connected: true,
            session_id: Some(session_id),
            server_name: Some(server_name),
            endpoint: Some(endpoint),
            capabilities: Some(capabilities),
            reason: None,
        }
    }

    /// Builds the disconnected-state payload with an optional reason.
    pub fn disconnected(reason: Option<String>) -> Self {
        ConnectionStatePayload {
            connected: false,
            session_id: None,
            server_name: None,
            endpoint: None,
            capabilities: None,
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norves_bridge_core::{CapabilityDescriptor, EngineState};

    #[test]
    fn connection_capabilities_connected_payload_serializes_authoritative_capabilities() {
        let capability: CapabilityDescriptor = serde_json::from_value(serde_json::json!({
            "name": "asset.reload",
            "version": "0.2",
            "description": "Reload the asset manifest."
        }))
        .expect("valid capability descriptor");
        let payload = ConnectionStatePayload::connected(
            "sess-1".to_owned(),
            "MockEngine".to_owned(),
            "ws://127.0.0.1:8123".to_owned(),
            vec![capability],
        );
        let json = serde_json::to_value(&payload).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "connected": true,
                "sessionId": "sess-1",
                "serverName": "MockEngine",
                "endpoint": "ws://127.0.0.1:8123",
                "capabilities": [{
                    "name": "asset.reload",
                    "version": "0.2",
                    "description": "Reload the asset manifest."
                }]
            })
        );
    }

    #[test]
    fn connection_capabilities_disconnected_payload_omits_capabilities() {
        let payload = ConnectionStatePayload::disconnected(Some("peer closed".to_owned()));
        let json = serde_json::to_value(&payload).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "connected": false,
                "reason": "peer closed"
            })
        );
    }

    /// Drift guard: a core enum that may appear in any forwarded payload must
    /// serialize to its lowercase wire value, proving wire transparency. If this
    /// flips, every forwarded engine payload would be mis-typed downstream.
    #[test]
    fn core_enum_serializes_to_wire_value() {
        assert_eq!(
            serde_json::to_string(&EngineState::Ready).expect("serializes"),
            "\"ready\""
        );
    }

    #[test]
    fn workspace_payload_serializes_camel_case() {
        let payload = WorkspacePayload {
            root_path: "C:/Project".to_owned(),
            assets_root: "C:/Project/Assets".to_owned(),
            name: "Project".to_owned(),
        };
        let json = serde_json::to_value(&payload).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "rootPath": "C:/Project",
                "assetsRoot": "C:/Project/Assets",
                "name": "Project"
            })
        );
    }

    #[test]
    fn asset_manifest_payload_serializes_camel_case_and_omits_missing_options() {
        let payload = AssetManifestPayload {
            version: 1,
            manifest_path: "C:/Project/Cooked/manifest.json".to_owned(),
            assets: vec![AssetEntryDto {
                logical_path: "textures/hero.png".to_owned(),
                kind: "texture".to_owned(),
                variant: Some("default".to_owned()),
                format: None,
                source_hash: Some("source-hash".to_owned()),
                cooked_package: None,
                entry_name: None,
                entry_type: None,
                cooked_hash: None,
                cooked_version: None,
            }],
        };
        let json = serde_json::to_value(&payload).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "version": 1,
                "manifestPath": "C:/Project/Cooked/manifest.json",
                "assets": [{
                    "logicalPath": "textures/hero.png",
                    "kind": "texture",
                    "variant": "default",
                    "sourceHash": "source-hash"
                }]
            })
        );
    }

    #[test]
    fn ui_capture_keeps_json_null_and_root_parent_distinct_from_missing_capture() {
        let property: UiPropertyCaptureDto = serde_json::from_value(serde_json::json!({
            "generation": 7,
            "revision": 12,
            "value": null
        }))
        .expect("JSON nullを捕捉値として読み取れる");
        assert_eq!(property.generation, 7);
        assert_eq!(property.revision, 12);
        assert_eq!(property.value, serde_json::Value::Null);

        let parent: UiParentCaptureDto = serde_json::from_value(serde_json::json!({
            "generation": 7,
            "revision": 12,
            "parentId": null
        }))
        .expect("nullをシーン直下の捕捉値として読み取れる");
        assert_eq!(parent.parent_id, None);
        assert_eq!(
            serde_json::to_value(EditSourceDto::Ui).expect("UIの出どころを直列化できる"),
            serde_json::json!("ui")
        );
        assert_eq!(
            serde_json::to_value(EditSourceDto::Mcp).expect("MCPの出どころを直列化できる"),
            serde_json::json!("mcp")
        );
    }

    #[test]
    fn edit_applied_dto_serializes_all_required_revision_fields() {
        let payload = EditAppliedDto {
            operation: "setProperty".to_owned(),
            object_id: Some("object-1".to_owned()),
            property: Some("color".to_owned()),
            value: Some(serde_json::Value::Null),
            new_id: None,
            source: EditSourceDto::Ui,
            group_id: "edit-7-9".to_owned(),
            generation: 7,
            sequence: 9,
            history_revision: 13,
            applied_revision: 11,
        };
        assert_eq!(
            serde_json::to_value(payload).expect("適用イベントを直列化できる"),
            serde_json::json!({
                "operation": "setProperty",
                "objectId": "object-1",
                "property": "color",
                "value": null,
                "newId": null,
                "source": "ui",
                "groupId": "edit-7-9",
                "generation": 7,
                "sequence": 9,
                "historyRevision": 13,
                "appliedRevision": 11
            })
        );
    }
}

/// 操作の安全な要約。任意の引数・応答本文・所有者IDを保持しない。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpOperationDto {
    pub request_id: String,
    pub session_id: String,
    pub timestamp: u64,
    pub tool: String,
    /// 対象の種類と起動内で比較できる指紋。生の対象IDではない。
    pub target: String,
    pub summary: String,
    pub result: String,
    pub outcome: String,
    pub display_group_id: Option<String>,
    pub completed_count: usize,
    pub pending: bool,
    pub retry_allowed: bool,
    pub automatic_retry_allowed: bool,
    pub actor_finished: bool,
}

/// 初期取得と更新通知は同じ有界snapshot。古い改訂の到着はUI側で無視できる。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpOperationsDto {
    pub session_id: String,
    pub revision: u64,
    pub records: Vec<McpOperationDto>,
    pub storage_error: Option<String>,
}
