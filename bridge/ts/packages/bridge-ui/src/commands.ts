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
  SetObjectPropertyResult,
} from '@norves/bridge-types';
import type {
  AssetManifestPayload,
  AssetManifestResult,
  AssetResolveResult,
  EditHistorySummary,
  EditDiscardResult,
  EngineSettingsPayload,
  McpSettingsPayload,
  McpTokenPayload,
  UiParentCapture,
  UiPropertyCapture,
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
  componentAdd: 'component_add',
  componentRemove: 'component_remove',
  editUndo: 'edit_undo',
  editRedo: 'edit_redo',
  editGetHistory: 'edit_get_history',
  editRetry: 'edit_retry',
  editDiscard: 'edit_discard',
  viewportGetThumbnail: 'viewport_get_thumbnail',
  runtimePlay: 'runtime_play',
  runtimePause: 'runtime_pause',
  runtimeStop: 'runtime_stop',
  focusViewport: 'focus_viewport',
  launchEngine: 'launch_engine',
  stopEngine: 'stop_engine',
  getEngineSettings: 'get_engine_settings',
  pickEnginePath: 'pick_engine_path',
  clearEnginePath: 'clear_engine_path',
  setEngineArgs: 'set_engine_args',
  getMcpSettings: 'get_mcp_settings',
  setMcpSettings: 'set_mcp_settings',
  getMcpToken: 'get_mcp_token',
  regenerateMcpToken: 'regenerate_mcp_token',
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
  capture?: UiParentCapture,
): Promise<SceneReparentObjectResult> {
  const args: { objectId: string; newParentId?: string; capture?: UiParentCapture } = { objectId };
  if (newParentId !== undefined) {
    args.newParentId = newParentId;
  }
  if (capture !== undefined) {
    args.capture = capture;
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

export async function objectSetProperty(
  objectId: string,
  property: string,
  value: unknown,
  capture?: UiPropertyCapture,
): Promise<SetObjectPropertyResult> {
  const args: {
    objectId: string;
    property: string;
    value: unknown;
    capture?: UiPropertyCapture;
  } = { objectId, property, value };
  if (capture !== undefined) {
    args.capture = capture;
  }
  return invoke<SetObjectPropertyResult>(BRIDGE_COMMANDS.objectSetProperty, args);
}

export async function editUndo(
  expectedHeadId: number | null,
  expectedRevision: number,
): Promise<unknown> {
  return invoke(BRIDGE_COMMANDS.editUndo, { expectedHeadId, expectedRevision });
}

export async function editRedo(
  expectedHeadId: number | null,
  expectedRevision: number,
): Promise<unknown> {
  return invoke(BRIDGE_COMMANDS.editRedo, { expectedHeadId, expectedRevision });
}

export async function editGetHistory(): Promise<EditHistorySummary> {
  return invoke<EditHistorySummary>(BRIDGE_COMMANDS.editGetHistory);
}

export async function editRetry(): Promise<unknown> {
  return invoke(BRIDGE_COMMANDS.editRetry);
}

export async function editDiscard(): Promise<EditDiscardResult> {
  return invoke<EditDiscardResult>(BRIDGE_COMMANDS.editDiscard);
}

export async function componentAdd(objectId: string, kind: string): Promise<unknown> {
  return invoke<unknown>(BRIDGE_COMMANDS.componentAdd, { objectId, kind });
}

export async function componentRemove(objectId: string): Promise<unknown> {
  return invoke<unknown>(BRIDGE_COMMANDS.componentRemove, { objectId });
}

export async function runtimePlay(): Promise<unknown> {
  return invoke<unknown>(BRIDGE_COMMANDS.runtimePlay);
}

export async function runtimePause(): Promise<unknown> {
  return invoke<unknown>(BRIDGE_COMMANDS.runtimePause);
}

export async function runtimeStop(): Promise<unknown> {
  return invoke<unknown>(BRIDGE_COMMANDS.runtimeStop);
}
// エンジンのパスは Rust 側のファイル選択ダイアログでだけ変わる。パス文字列を渡すコマンドは無い。
export async function getEngineSettings(): Promise<EngineSettingsPayload> {
  return invoke<EngineSettingsPayload>(BRIDGE_COMMANDS.getEngineSettings);
}

/** ダイアログを開いて選ばせる。キャンセルなら何も変えず、その時点の設定を返す。 */
export async function pickEnginePath(): Promise<EngineSettingsPayload> {
  return invoke<EngineSettingsPayload>(BRIDGE_COMMANDS.pickEnginePath);
}

export async function clearEnginePath(): Promise<EngineSettingsPayload> {
  return invoke<EngineSettingsPayload>(BRIDGE_COMMANDS.clearEnginePath);
}

/** 起動引数(1 要素 = 1 引数)を保存する。バックエンドが確かめ、空行を捨てた後の設定を返す。 */
export async function setEngineArgs(args: string[]): Promise<EngineSettingsPayload> {
  return invoke<EngineSettingsPayload>(BRIDGE_COMMANDS.setEngineArgs, { args });
}

/** MCPの有効状態、待受ポート、listener状態を取得する。秘密は返さない。 */
export async function getMcpSettings(): Promise<McpSettingsPayload> {
  return invoke<McpSettingsPayload>(BRIDGE_COMMANDS.getMcpSettings);
}

/** MCPの有効状態とloopbackポートを保存する。 */
export async function setMcpSettings(
  enabled: boolean,
  port: number,
): Promise<McpSettingsPayload> {
  return invoke<McpSettingsPayload>(BRIDGE_COMMANDS.setMcpSettings, { enabled, port });
}

/** MCPトークンを明示的に表示するときだけ呼び出す。 */
export async function getMcpToken(): Promise<McpTokenPayload> {
  return invoke<McpTokenPayload>(BRIDGE_COMMANDS.getMcpToken);
}

/** MCPトークンを作り直し、以前の認証と接続を失効させる。 */
export async function regenerateMcpToken(): Promise<McpSettingsPayload> {
  return invoke<McpSettingsPayload>(BRIDGE_COMMANDS.regenerateMcpToken);
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
