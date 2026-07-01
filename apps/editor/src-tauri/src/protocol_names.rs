//! SOURCE OF TRUTH mirror for Tauri IPC names. Kept in lock-step with
//! bridge/ts/packages/bridge-ui/src/{commands,events}.ts -- verified by
//! scripts/check-protocol-names.mjs.
//!
//! P5 wires the `events::*` constants into the backend event relay. The
//! `commands::*` constants stay as the source-of-truth assertions verified by
//! scripts/check-protocol-names.mjs and the tests below; the command handlers
//! themselves are matched to these values by fn NAME (Tauri derives the IPC
//! name from the fn), so the command consts have no runtime referent. The
//! `dead_code` allow keeps clippy -D warnings clean for those assertion-only
//! consts.
#![allow(dead_code)]

/// Tauri command names.
///
/// Each value MUST match the corresponding entry in BRIDGE_COMMANDS
/// (bridge/ts/packages/bridge-ui/src/commands.ts) byte-for-byte, and MUST
/// equal the Rust `#[tauri::command]` fn name P5 will define.
pub mod commands {
    pub const CONNECT: &str = "bridge_connect";
    pub const DISCONNECT: &str = "bridge_disconnect";
    pub const RECONNECT: &str = "bridge_reconnect";
    pub const GET_STATUS: &str = "get_status";
    pub const SCENE_GET_TREE: &str = "scene_get_tree";
    pub const SCENE_CREATE_OBJECT: &str = "scene_create_object";
    pub const SCENE_DELETE_OBJECT: &str = "scene_delete_object";
    pub const SCENE_REPARENT_OBJECT: &str = "scene_reparent_object";
    pub const SCENE_DUPLICATE_OBJECT: &str = "scene_duplicate_object";
    pub const OBJECT_GET_SNAPSHOT: &str = "object_get_snapshot";
    pub const OBJECT_SET_PROPERTY: &str = "object_set_property";
    pub const SCHEMA_GET_SNAPSHOT: &str = "schema_get_snapshot";
    pub const VIEWPORT_GET_THUMBNAIL: &str = "viewport_get_thumbnail";
    pub const RUNTIME_PLAY: &str = "runtime_play";
    pub const RUNTIME_PAUSE: &str = "runtime_pause";
    pub const RUNTIME_STOP: &str = "runtime_stop";
    pub const FOCUS_VIEWPORT: &str = "focus_viewport";
    pub const LAUNCH_ENGINE: &str = "launch_engine";
    pub const STOP_ENGINE: &str = "stop_engine";
    pub const WORKSPACE_OPEN: &str = "workspace_open";
    pub const WORKSPACE_GET: &str = "workspace_get";
    pub const WORKSPACE_CLOSE: &str = "workspace_close";
    pub const ASSET_READ_MANIFEST: &str = "asset_read_manifest";
    pub const ASSET_RESOLVE: &str = "asset_resolve";
    pub const ASSET_GET_MANIFEST: &str = "asset_get_manifest";
}

/// Tauri event names (backend -> frontend).
///
/// Each value MUST match the corresponding entry in BRIDGE_EVENTS
/// (bridge/ts/packages/bridge-ui/src/events.ts) byte-for-byte.
pub mod events {
    pub const CONNECTION_STATE: &str = "bridge:connection-state";
    pub const STATUS_CHANGED: &str = "bridge:status-changed";
    pub const RUNTIME_STATE_CHANGED: &str = "bridge:runtime-state-changed";
    pub const LOG_MESSAGE: &str = "bridge:log-message";
    pub const ERROR_REPORTED: &str = "bridge:error-reported";
    pub const ENGINE_PROCESS_EXITED: &str = "bridge:engine-process-exited";
    pub const VIEWPORT_STATE_CHANGED: &str = "bridge:viewport-state-changed";
    pub const BRIDGE_CONNECTED: &str = "bridge:bridge-connected";
    pub const BRIDGE_DISCONNECTED: &str = "bridge:bridge-disconnected";
    pub const SCENE_TREE_CHANGED: &str = "bridge:scene-tree-changed";
    pub const OBJECT_CHANGED: &str = "bridge:object-changed";
}

#[cfg(test)]
mod tests {
    use super::{commands, events};

    // -----------------------------------------------------------------------
    // Command literal assertions
    // -----------------------------------------------------------------------

    #[test]
    fn command_connect() {
        assert_eq!(commands::CONNECT, "bridge_connect");
    }

    #[test]
    fn command_disconnect() {
        assert_eq!(commands::DISCONNECT, "bridge_disconnect");
    }

    #[test]
    fn command_reconnect() {
        assert_eq!(commands::RECONNECT, "bridge_reconnect");
    }

    #[test]
    fn command_get_status() {
        assert_eq!(commands::GET_STATUS, "get_status");
    }

    #[test]
    fn command_scene_get_tree() {
        assert_eq!(commands::SCENE_GET_TREE, "scene_get_tree");
    }

    #[test]
    fn command_scene_create_object() {
        assert_eq!(commands::SCENE_CREATE_OBJECT, "scene_create_object");
    }

    #[test]
    fn command_scene_delete_object() {
        assert_eq!(commands::SCENE_DELETE_OBJECT, "scene_delete_object");
    }

    #[test]
    fn command_scene_reparent_object() {
        assert_eq!(commands::SCENE_REPARENT_OBJECT, "scene_reparent_object");
    }

    #[test]
    fn command_scene_duplicate_object() {
        assert_eq!(commands::SCENE_DUPLICATE_OBJECT, "scene_duplicate_object");
    }

    #[test]
    fn command_object_get_snapshot() {
        assert_eq!(commands::OBJECT_GET_SNAPSHOT, "object_get_snapshot");
    }

    #[test]
    fn command_object_set_property() {
        assert_eq!(commands::OBJECT_SET_PROPERTY, "object_set_property");
    }

    #[test]
    fn command_schema_get_snapshot() {
        assert_eq!(commands::SCHEMA_GET_SNAPSHOT, "schema_get_snapshot");
    }

    #[test]
    fn command_viewport_get_thumbnail() {
        assert_eq!(commands::VIEWPORT_GET_THUMBNAIL, "viewport_get_thumbnail");
    }

    #[test]
    fn command_runtime_play() {
        assert_eq!(commands::RUNTIME_PLAY, "runtime_play");
    }

    #[test]
    fn command_runtime_pause() {
        assert_eq!(commands::RUNTIME_PAUSE, "runtime_pause");
    }

    #[test]
    fn command_runtime_stop() {
        assert_eq!(commands::RUNTIME_STOP, "runtime_stop");
    }

    #[test]
    fn command_focus_viewport() {
        assert_eq!(commands::FOCUS_VIEWPORT, "focus_viewport");
    }

    #[test]
    fn command_launch_engine() {
        assert_eq!(commands::LAUNCH_ENGINE, "launch_engine");
    }

    #[test]
    fn command_stop_engine() {
        assert_eq!(commands::STOP_ENGINE, "stop_engine");
    }

    #[test]
    fn command_workspace_open() {
        assert_eq!(commands::WORKSPACE_OPEN, "workspace_open");
    }

    #[test]
    fn command_workspace_get() {
        assert_eq!(commands::WORKSPACE_GET, "workspace_get");
    }

    #[test]
    fn command_workspace_close() {
        assert_eq!(commands::WORKSPACE_CLOSE, "workspace_close");
    }

    #[test]
    fn command_asset_read_manifest() {
        assert_eq!(commands::ASSET_READ_MANIFEST, "asset_read_manifest");
    }

    #[test]
    fn command_asset_resolve() {
        assert_eq!(commands::ASSET_RESOLVE, "asset_resolve");
    }

    #[test]
    fn command_asset_get_manifest() {
        assert_eq!(commands::ASSET_GET_MANIFEST, "asset_get_manifest");
    }

    // -----------------------------------------------------------------------
    // Event literal assertions
    // -----------------------------------------------------------------------

    #[test]
    fn event_connection_state() {
        assert_eq!(events::CONNECTION_STATE, "bridge:connection-state");
    }

    #[test]
    fn event_status_changed() {
        assert_eq!(events::STATUS_CHANGED, "bridge:status-changed");
    }

    #[test]
    fn event_runtime_state_changed() {
        assert_eq!(
            events::RUNTIME_STATE_CHANGED,
            "bridge:runtime-state-changed"
        );
    }

    #[test]
    fn event_log_message() {
        assert_eq!(events::LOG_MESSAGE, "bridge:log-message");
    }

    #[test]
    fn event_error_reported() {
        assert_eq!(events::ERROR_REPORTED, "bridge:error-reported");
    }

    #[test]
    fn event_engine_process_exited() {
        assert_eq!(
            events::ENGINE_PROCESS_EXITED,
            "bridge:engine-process-exited"
        );
    }

    #[test]
    fn event_viewport_state_changed() {
        assert_eq!(
            events::VIEWPORT_STATE_CHANGED,
            "bridge:viewport-state-changed"
        );
    }

    #[test]
    fn event_bridge_connected() {
        assert_eq!(events::BRIDGE_CONNECTED, "bridge:bridge-connected");
    }

    #[test]
    fn event_bridge_disconnected() {
        assert_eq!(events::BRIDGE_DISCONNECTED, "bridge:bridge-disconnected");
    }

    #[test]
    fn event_scene_tree_changed() {
        assert_eq!(events::SCENE_TREE_CHANGED, "bridge:scene-tree-changed");
    }

    #[test]
    fn event_object_changed() {
        assert_eq!(events::OBJECT_CHANGED, "bridge:object-changed");
    }

    // -----------------------------------------------------------------------
    // No-duplicate guards within each set
    // -----------------------------------------------------------------------

    #[test]
    fn commands_no_duplicates() {
        let all = [
            commands::CONNECT,
            commands::DISCONNECT,
            commands::RECONNECT,
            commands::GET_STATUS,
            commands::SCENE_GET_TREE,
            commands::SCENE_CREATE_OBJECT,
            commands::SCENE_DELETE_OBJECT,
            commands::SCENE_REPARENT_OBJECT,
            commands::SCENE_DUPLICATE_OBJECT,
            commands::OBJECT_GET_SNAPSHOT,
            commands::OBJECT_SET_PROPERTY,
            commands::SCHEMA_GET_SNAPSHOT,
            commands::VIEWPORT_GET_THUMBNAIL,
            commands::RUNTIME_PLAY,
            commands::RUNTIME_PAUSE,
            commands::RUNTIME_STOP,
            commands::FOCUS_VIEWPORT,
            commands::LAUNCH_ENGINE,
            commands::STOP_ENGINE,
            commands::WORKSPACE_OPEN,
            commands::WORKSPACE_GET,
            commands::WORKSPACE_CLOSE,
            commands::ASSET_READ_MANIFEST,
            commands::ASSET_RESOLVE,
            commands::ASSET_GET_MANIFEST,
        ];
        let mut sorted = all;
        sorted.sort_unstable();
        let dedup_len = sorted.windows(2).filter(|w| w[0] == w[1]).count();
        assert_eq!(dedup_len, 0, "duplicate command name found");
    }

    #[test]
    fn events_no_duplicates() {
        let all = [
            events::CONNECTION_STATE,
            events::STATUS_CHANGED,
            events::RUNTIME_STATE_CHANGED,
            events::LOG_MESSAGE,
            events::ERROR_REPORTED,
            events::ENGINE_PROCESS_EXITED,
            events::VIEWPORT_STATE_CHANGED,
            events::BRIDGE_CONNECTED,
            events::BRIDGE_DISCONNECTED,
            events::SCENE_TREE_CHANGED,
            events::OBJECT_CHANGED,
        ];
        let mut sorted = all;
        sorted.sort_unstable();
        let dedup_len = sorted.windows(2).filter(|w| w[0] == w[1]).count();
        assert_eq!(dedup_len, 0, "duplicate event name found");
    }
}
