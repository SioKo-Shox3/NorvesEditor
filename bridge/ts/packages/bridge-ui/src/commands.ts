// SOURCE OF TRUTH for Tauri IPC names. Kept in lock-step with
// apps/editor/src-tauri/src/protocol_names.rs -- verified by
// scripts/check-protocol-names.mjs.

/**
 * Tauri command name constants.
 *
 * Each value is the snake_case string that both:
 *   - the frontend passes to `invoke()` (via invokeCommand)
 *   - the Rust backend exposes as a `#[tauri::command]` fn name
 *
 * These MUST stay byte-for-byte equal to the constants in
 * `apps/editor/src-tauri/src/protocol_names.rs`.
 */
export const BRIDGE_COMMANDS = {
  connect: 'bridge_connect',
  disconnect: 'bridge_disconnect',
  reconnect: 'bridge_reconnect',
  getStatus: 'get_status',
  sceneGetTree: 'scene_get_tree',
  objectGetSnapshot: 'object_get_snapshot',
  schemaGetSnapshot: 'schema_get_snapshot',
  runtimePlay: 'runtime_play',
  runtimePause: 'runtime_pause',
  runtimeStop: 'runtime_stop',
  focusViewport: 'focus_viewport',
  launchEngine: 'launch_engine',
  stopEngine: 'stop_engine',
} as const;

/** Union of all valid Tauri command name strings. */
export type BridgeCommandName = (typeof BRIDGE_COMMANDS)[keyof typeof BRIDGE_COMMANDS];
