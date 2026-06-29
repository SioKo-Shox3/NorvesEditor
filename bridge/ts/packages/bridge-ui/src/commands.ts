// SOURCE OF TRUTH for Tauri IPC names. Kept in lock-step with
// apps/editor/src-tauri/src/protocol_names.rs -- verified by
// scripts/check-protocol-names.mjs.

import { invoke } from '@tauri-apps/api/core';
import type {
  AssetManifestPayload,
  AssetManifestResult,
  AssetResolveResult,
  WorkspacePayload,
} from './ipc-types.js';

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
  objectSetProperty: 'object_set_property',
  schemaGetSnapshot: 'schema_get_snapshot',
  viewportGetThumbnail: 'viewport_get_thumbnail',
  runtimePlay: 'runtime_play',
  runtimePause: 'runtime_pause',
  runtimeStop: 'runtime_stop',
  focusViewport: 'focus_viewport',
  launchEngine: 'launch_engine',
  stopEngine: 'stop_engine',
  workspaceOpen: 'workspace_open',
  workspaceGet: 'workspace_get',
  workspaceClose: 'workspace_close',
  assetReadManifest: 'asset_read_manifest',
  assetResolve: 'asset_resolve',
  assetGetManifest: 'asset_get_manifest',
} as const;

/** Union of all valid Tauri command name strings. */
export type BridgeCommandName = (typeof BRIDGE_COMMANDS)[keyof typeof BRIDGE_COMMANDS];

export async function workspaceOpen(rootPath: string): Promise<WorkspacePayload> {
  return invoke<WorkspacePayload>(BRIDGE_COMMANDS.workspaceOpen, { rootPath });
}

export async function workspaceGet(): Promise<WorkspacePayload | null> {
  return invoke<WorkspacePayload | null>(BRIDGE_COMMANDS.workspaceGet);
}

export async function workspaceClose(): Promise<void> {
  await invoke<void>(BRIDGE_COMMANDS.workspaceClose);
}

export async function assetReadManifest(manifestPath: string): Promise<AssetManifestPayload> {
  return invoke<AssetManifestPayload>(BRIDGE_COMMANDS.assetReadManifest, { manifestPath });
}

export async function assetResolve(
  logicalPath: string,
  kind?: string,
  variant?: string,
): Promise<AssetResolveResult> {
  const args: { logicalPath: string; kind?: string; variant?: string } = { logicalPath };
  if (kind !== undefined) {
    args.kind = kind;
  }
  if (variant !== undefined) {
    args.variant = variant;
  }
  return invoke<AssetResolveResult>(BRIDGE_COMMANDS.assetResolve, args);
}

export async function assetGetManifest(
  filter?: string,
  page?: number,
  pageSize?: number,
): Promise<AssetManifestResult> {
  const args: { filter?: string; page?: number; pageSize?: number } = {};
  if (filter !== undefined) {
    args.filter = filter;
  }
  if (page !== undefined) {
    args.page = page;
  }
  if (pageSize !== undefined) {
    args.pageSize = pageSize;
  }
  return invoke<AssetManifestResult>(BRIDGE_COMMANDS.assetGetManifest, args);
}
