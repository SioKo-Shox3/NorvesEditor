// SOURCE OF TRUTH for Tauri IPC names. Kept in lock-step with
// apps/editor/src-tauri/src/protocol_names.rs -- verified by
// scripts/check-protocol-names.mjs.

/**
 * Tauri event name constants (backend → frontend).
 *
 * Each value is the kebab-case `bridge:` prefixed string that:
 *   - the Rust backend emits via `app_handle.emit()`
 *   - the frontend listens to via `subscribeEvent()`
 *
 * These MUST stay byte-for-byte equal to the constants in
 * `apps/editor/src-tauri/src/protocol_names.rs`.
 */
export const BRIDGE_EVENTS = {
  connectionState: 'bridge:connection-state',
  statusChanged: 'bridge:status-changed',
  runtimeStateChanged: 'bridge:runtime-state-changed',
  logMessage: 'bridge:log-message',
  errorReported: 'bridge:error-reported',
  engineProcessExited: 'bridge:engine-process-exited',
  viewportStateChanged: 'bridge:viewport-state-changed',
  bridgeConnected: 'bridge:bridge-connected',
  bridgeDisconnected: 'bridge:bridge-disconnected',
  sceneTreeChanged: 'bridge:scene-tree-changed',
  objectChanged: 'bridge:object-changed',
} as const;

/** Union of all valid Tauri event name strings. */
export type BridgeEventName = (typeof BRIDGE_EVENTS)[keyof typeof BRIDGE_EVENTS];
