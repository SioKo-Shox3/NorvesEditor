//! DTOs the backend *synthesizes* and serializes to the frontend.
//!
//! Where the backend forwards a raw engine `result` / event `params`
//! ([`serde_json::Value`]), no struct exists here on purpose — re-modeling would
//! risk drift from the wire schema. The synthesized payloads defined here use
//! camelCase to match the TS convention.

use serde::Serialize;

/// Payload of the `bridge:connection-state` event AND the value returned by
/// `bridge_connect` / `bridge_reconnect`.
///
/// `connected = true` carries `session_id` / `server_name` / `endpoint` from the
/// completed handshake; `connected = false` carries an optional `reason`.
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
    /// Human-readable reason for a disconnect (disconnected only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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

impl ConnectionStatePayload {
    /// Builds the connected-state payload from a completed handshake.
    pub fn connected(session_id: String, server_name: String, endpoint: String) -> Self {
        ConnectionStatePayload {
            connected: true,
            session_id: Some(session_id),
            server_name: Some(server_name),
            endpoint: Some(endpoint),
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
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norves_bridge_core::EngineState;

    #[test]
    fn connected_payload_serializes_camel_case() {
        let payload = ConnectionStatePayload::connected(
            "sess-1".to_owned(),
            "MockEngine".to_owned(),
            "ws://127.0.0.1:8123".to_owned(),
        );
        let json = serde_json::to_value(&payload).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "connected": true,
                "sessionId": "sess-1",
                "serverName": "MockEngine",
                "endpoint": "ws://127.0.0.1:8123"
            })
        );
    }

    #[test]
    fn disconnected_payload_serializes_reason_and_omits_connected_fields() {
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
}
