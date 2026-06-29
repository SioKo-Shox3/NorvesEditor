// IPC contract types shared between Rust backend and frontend.
// These mirror Rust DTO structs in apps/editor/src-tauri/src/dto.rs.

/**
 * Payload returned by bridge_connect / bridge_disconnect / bridge_reconnect
 * Tauri commands, and emitted on the bridge:connection-state event.
 *
 * // Mirrors apps/editor/src-tauri/src/dto.rs ConnectionStatePayload
 */
export interface ConnectionStatePayload {
  connected: boolean;
  sessionId?: string;
  serverName?: string;
  endpoint?: string;
  reason?: string;
}

/**
 * Payload returned by workspace_open / workspace_get.
 *
 * // Mirrors apps/editor/src-tauri/src/dto.rs WorkspacePayload
 */
export interface WorkspacePayload {
  rootPath: string;
  assetsRoot: string;
  name: string;
}
