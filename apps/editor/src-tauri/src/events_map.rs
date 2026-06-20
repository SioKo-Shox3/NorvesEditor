//! Pure mapping from a Bridge protocol event NAME to the Tauri event channel the
//! relay task emits on.
//!
//! Kept as a free function so it can be unit-tested without a runtime or a live
//! connection (the relay task body in `bridge_state` calls it). Unknown event
//! names return `None` — the relay logs and skips them rather than crashing.

use crate::protocol_names::events;

/// Maps a wire event name (e.g. `"log.message"`) to the Tauri UI event channel
/// constant (e.g. [`events::LOG_MESSAGE`]). Returns `None` for any event name
/// this backend does not relay.
pub fn ui_channel_for_event(event_name: &str) -> Option<&'static str> {
    match event_name {
        "log.message" => Some(events::LOG_MESSAGE),
        "runtime.stateChanged" => Some(events::RUNTIME_STATE_CHANGED),
        "engine.statusChanged" => Some(events::STATUS_CHANGED),
        "error.reported" => Some(events::ERROR_REPORTED),
        "engine.processExited" => Some(events::ENGINE_PROCESS_EXITED),
        "viewport.stateChanged" => Some(events::VIEWPORT_STATE_CHANGED),
        "bridge.connected" => Some(events::BRIDGE_CONNECTED),
        "bridge.disconnected" => Some(events::BRIDGE_DISCONNECTED),
        "scene.treeChanged" => Some(events::SCENE_TREE_CHANGED),
        "object.changed" => Some(events::OBJECT_CHANGED),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_log_message() {
        assert_eq!(
            ui_channel_for_event("log.message"),
            Some(events::LOG_MESSAGE)
        );
    }

    #[test]
    fn maps_runtime_state_changed() {
        assert_eq!(
            ui_channel_for_event("runtime.stateChanged"),
            Some(events::RUNTIME_STATE_CHANGED)
        );
    }

    #[test]
    fn maps_engine_status_changed() {
        assert_eq!(
            ui_channel_for_event("engine.statusChanged"),
            Some(events::STATUS_CHANGED)
        );
    }

    #[test]
    fn maps_error_reported() {
        assert_eq!(
            ui_channel_for_event("error.reported"),
            Some(events::ERROR_REPORTED)
        );
    }

    #[test]
    fn maps_engine_process_exited() {
        assert_eq!(
            ui_channel_for_event("engine.processExited"),
            Some(events::ENGINE_PROCESS_EXITED)
        );
    }

    #[test]
    fn maps_viewport_state_changed() {
        assert_eq!(
            ui_channel_for_event("viewport.stateChanged"),
            Some(events::VIEWPORT_STATE_CHANGED)
        );
    }

    #[test]
    fn maps_bridge_connected() {
        assert_eq!(
            ui_channel_for_event("bridge.connected"),
            Some(events::BRIDGE_CONNECTED)
        );
    }

    #[test]
    fn maps_bridge_disconnected() {
        assert_eq!(
            ui_channel_for_event("bridge.disconnected"),
            Some(events::BRIDGE_DISCONNECTED)
        );
    }

    #[test]
    fn maps_scene_tree_changed() {
        assert_eq!(
            ui_channel_for_event("scene.treeChanged"),
            Some(events::SCENE_TREE_CHANGED)
        );
    }

    #[test]
    fn maps_object_changed() {
        assert_eq!(
            ui_channel_for_event("object.changed"),
            Some(events::OBJECT_CHANGED)
        );
    }

    #[test]
    fn unknown_event_name_maps_to_none() {
        assert_eq!(ui_channel_for_event("totally.unknown"), None);
        assert_eq!(ui_channel_for_event(""), None);
    }
}
