// SOURCE OF TRUTH for Tauri IPC names. Kept in lock-step with
// apps/editor/src-tauri/src/protocol_names.rs -- verified by
// scripts/check-protocol-names.mjs.

import { invoke } from '@tauri-apps/api/core';
import type {
  AssetReloadManifestResult,
  SceneCreateObjectResult,
  SceneDeleteObjectResult,
  SceneDuplicateObjectResult,
  SceneReparentObjectResult,
} from '@norves/bridge-types';
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
  sceneCreateObject: 'scene_create_object',
  sceneDeleteObject: 'scene_delete_object',
  sceneReparentObject: 'scene_reparent_object',
  sceneDuplicateObject: 'scene_duplicate_object',
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
  assetReloadManifest: 'asset_reload_manifest',
} as const;

/** Union of all valid Tauri command name strings. */
export type BridgeCommandName = (typeof BRIDGE_COMMANDS)[keyof typeof BRIDGE_COMMANDS];

export async function sceneCreateObject(
  parentId?: string,
  kind?: string,
): Promise<SceneCreateObjectResult> {
  const args: { parentId?: string; kind?: string } = {};
  if (parentId !== undefined) {
    args.parentId = parentId;
  }
  if (kind !== undefined) {
    args.kind = kind;
  }
  return invoke<SceneCreateObjectResult>(BRIDGE_COMMANDS.sceneCreateObject, args);
}

export async function sceneDeleteObject(objectId: string): Promise<SceneDeleteObjectResult> {
  return invoke<SceneDeleteObjectResult>(BRIDGE_COMMANDS.sceneDeleteObject, { objectId });
}

export async function sceneReparentObject(
  objectId: string,
  newParentId?: string,
): Promise<SceneReparentObjectResult> {
  const args: { objectId: string; newParentId?: string } = { objectId };
  if (newParentId !== undefined) {
    args.newParentId = newParentId;
  }
  return invoke<SceneReparentObjectResult>(BRIDGE_COMMANDS.sceneReparentObject, args);
}

export async function sceneDuplicateObject(
  objectId: string,
  newParentId?: string,
): Promise<SceneDuplicateObjectResult> {
  const args: { objectId: string; newParentId?: string } = { objectId };
  if (newParentId !== undefined) {
    args.newParentId = newParentId;
  }
  return invoke<SceneDuplicateObjectResult>(BRIDGE_COMMANDS.sceneDuplicateObject, args);
}
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

export async function assetReloadManifest(): Promise<AssetReloadManifestResult> {
  return invoke<AssetReloadManifestResult>(BRIDGE_COMMANDS.assetReloadManifest);
}
