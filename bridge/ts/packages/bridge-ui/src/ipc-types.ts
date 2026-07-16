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
