// IPC contract types shared between Rust backend and frontend.
// These mirror Rust DTO structs in apps/editor/src-tauri/src/dto.rs.

import type { CapabilityDescriptor } from '@norves/bridge-types';

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
  capabilities?: CapabilityDescriptor[];
  reason?: string;
}

/** エンジンのパスの出所。環境変数 > 保存済みの設定 > 既定値 の順に採用される。 */
export type EnginePathSource = 'env' | 'settings' | 'default';

/**
 * get_engine_settings / pick_engine_path / clear_engine_path / set_engine_args が返す値。
 *
 * // apps/editor/src-tauri/src/dto.rs の EngineSettingsPayload と同じ形
 */
export interface EngineSettingsPayload {
  /** launch_engine が次に使うパス。 */
  effectivePath: string;
  source: EnginePathSource;
  /** 保存済みのパス。未設定なら null。 */
  savedPath: string | null;
  /** 保存済みの起動引数(1 要素 = 1 引数)。launch_engine は --bridge-port より前に渡す。 */
  savedArgs: string[];
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

/** 画面の値スナップショットに適用改訂を添えて編集コマンドへ渡す。 */
export interface UiPropertyCapture {
  generation: number;
  revision: number;
  value: unknown;
}

/** 画面のツリーに適用改訂を添えて親変更コマンドへ渡す。nullはシーン直下。 */
export interface UiParentCapture {
  generation: number;
  revision: number;
  parentId: string | null;
}

export type EditSource = 'ui' | 'mcp';

export interface EditGroupSummary {
  id: string;
  name: string;
  source: EditSource;
  count: number;
  createdAt: number;
}

export interface EditPendingGroup {
  id: string;
  name: string;
  direction: 'undo' | 'redo';
  source: EditSource;
  createdAt: number;
  totalCount: number;
  completedCount: number;
  outcomeUnknown: boolean;
  retryAllowed: boolean;
}

export interface EditDiscardResult {
  groupId: string;
  completedCount: number;
  totalCount: number;
  outcomeUnknown: boolean;
  changesRemain: boolean;
}

/** 編集サービスが初期取得と変更イベントで共有する履歴要約。 */
export interface EditHistorySummary {
  generation: number | null;
  historyRevision: number;
  appliedRevision: number;
  canUndo: boolean;
  canRedo: boolean;
  undoHeadId: number | null;
  undoRevision: number;
  undoGroup: EditGroupSummary | null;
  redoHeadId: number | null;
  redoRevision: number;
  redoGroup: EditGroupSummary | null;
  pending: boolean;
  pendingGroup?: EditPendingGroup | null;
}

export type EditAppliedOperation =
  | 'createObject'
  | 'deleteObject'
  | 'duplicateObject'
  | 'reparentObject'
  | 'setProperty'
  | 'componentAdd'
  | 'componentRemove'
  | 'undo'
  | 'redo';

/** 編集サービスが適用した変更を画面へ伝える。 */
export interface EditAppliedPayload {
  operation: EditAppliedOperation;
  objectId: string | null;
  property: string | null;
  value: unknown | null;
  newId: string | null;
  source: EditSource;
  groupId: string;
  generation: number;
  sequence: number;
  historyRevision: number;
  appliedRevision: number;
}

/**
 * One asset entry returned by asset_read_manifest.
 *
 * // Mirrors apps/editor/src-tauri/src/dto.rs AssetEntryDto
 */
export interface AssetEntry {
  logicalPath: string;
  kind: string;
  variant?: string;
  format?: string;
  sourceHash?: string;
  cookedPackage?: string;
  entryName?: string;
  entryType?: string;
  cookedHash?: string;
  cookedVersion?: number;
}

/**
 * Payload returned by asset_read_manifest.
 *
 * // Mirrors apps/editor/src-tauri/src/dto.rs AssetManifestPayload
 */
export interface AssetManifestPayload {
  version: number;
  manifestPath: string;
  assets: AssetEntry[];
}

export type AssetResolveStatus =
  | 'successCooked'
  | 'successLoose'
  | 'invalidRequest'
  | 'invalidManifest'
  | 'looseReadFailed'
  | 'cookedPackageReadFailed'
  | 'cookedPackageParseFailed'
  | 'cookedEntryMissing'
  | 'cookedEntryHashMismatch';

export type AssetResolveSource =
  | 'none'
  | 'cooked'
  | 'loose'
  | 'debugLooseFallback';

/**
 * Result returned by asset_resolve / asset.resolve.
 *
 * Wire shape mirrors bridge/spec/schema/methods/asset.resolve.result.schema.json.
 */
export interface AssetResolveResult {
  status: AssetResolveStatus;
  source: AssetResolveSource;
  normalizedLogicalPath: string;
  requiresExplicitLog?: boolean;
  fallbackAction?: string;
  failureKind?: string;
  reason?: string;
}

/**
 * Result returned by asset_get_manifest / asset.getManifest.
 *
 * Wire shape mirrors bridge/spec/schema/methods/asset.getManifest.result.schema.json.
 */
export interface AssetManifestResult {
  version: number;
  entries: AssetEntry[];
  totalCount: number;
  page?: number;
  pageSize?: number;
}
