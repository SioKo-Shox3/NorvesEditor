/**
 * Pure reducer unit tests.
 *
 * These tests cover state transitions for all action types.
 * They are the substitute for a real GUI round-trip test;
 * true end-to-end testing requires a running Tauri process + engine
 * (see plan §10 manual acceptance).
 */

import { describe, it, expect } from 'vitest';
import {
  assetKeyForEntry,
  bridgeReducer,
  INITIAL_STATE,
  type BridgeAction,
  type BridgeState,
} from '../store.js';
import type { AssetManifestPayload, AssetResolveResult } from '@norves/bridge-ui';

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

function applyAction(action: BridgeAction, state: BridgeState = INITIAL_STATE): BridgeState {
  return bridgeReducer(state, action);
}

function resolveResult(status: AssetResolveResult['status']): AssetResolveResult {
  return {
    status,
    source: status === 'successCooked' ? 'cooked' : 'none',
    normalizedLogicalPath: 'textures/hero.png',
  };
}

// -------------------------------------------------------------------------
// workspaceOpened / workspaceClosed
// -------------------------------------------------------------------------

describe('workspaceOpened / workspaceClosed', () => {
  it('stores the opened workspace payload', () => {
    const workspace = {
      rootPath: 'C:/Project',
      assetsRoot: 'C:/Project/Assets',
      name: 'Project',
    };
    const next = applyAction({ type: 'workspaceOpened', payload: workspace });
    expect(next.workspace).toEqual(workspace);
  });

  it('clears the workspace', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      workspace: {
        rootPath: 'C:/Project',
        assetsRoot: 'C:/Project/Assets',
        name: 'Project',
      },
    };
    const next = applyAction({ type: 'workspaceClosed' }, state);
    expect(next.workspace).toBeUndefined();
  });

  it('does not clear workspace on bridge disconnect', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      workspace: {
        rootPath: 'C:/Project',
        assetsRoot: 'C:/Project/Assets',
        name: 'Project',
      },
      connection: { status: 'connected' },
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: false, reason: 'closed' } },
      state,
    );
    expect(next.workspace).toEqual(state.workspace);
  });

  it('workspaceError sets lastError without touching connection status', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected', sessionId: 's1' },
    };
    const next = applyAction(
      { type: 'workspaceError', payload: { error: { kind: 'process', message: 'Assets missing' } } },
      state,
    );
    expect(next.lastError).toEqual({ kind: 'process', message: 'Assets missing' });
    // The Bridge connection is independent of workspace failures.
    expect(next.connection).toEqual(state.connection);
  });
});

// -------------------------------------------------------------------------
// assetManifestLoaded / assetSelected / assetManifestError
// -------------------------------------------------------------------------

describe('asset manifest state', () => {
  const manifest: AssetManifestPayload = {
    version: 1,
    manifestPath: 'C:/Project/manifest.json',
    assets: [
      {
        logicalPath: 'textures/hero.png',
        kind: 'texture',
        variant: 'default',
        format: 'png',
        sourceHash: 'source-1',
        cookedPackage: 'textures.pkg',
        entryName: 'hero',
        entryType: 'texture2d',
        cookedHash: 'cooked-1',
        cookedVersion: 3,
      },
      {
        logicalPath: 'materials/hero.mat',
        kind: 'material',
        variant: 'mobile',
      },
    ],
  };

  it('stores the loaded manifest payload', () => {
    const next = applyAction({ type: 'assetManifestLoaded', payload: manifest });
    expect(next.assetManifest).toEqual(manifest);
  });

  it('sets selectedAssetKey to the selected key', () => {
    const key = assetKeyForEntry(manifest.assets[0]!);
    const next = applyAction({ type: 'assetSelected', key });
    expect(next.selectedAssetKey).toBe(key);
  });

  it('clears manifest and selection when assetManifestCleared is dispatched', () => {
    const key = assetKeyForEntry(manifest.assets[0]!);
    const state: BridgeState = {
      ...INITIAL_STATE,
      assetManifest: manifest,
      selectedAssetKey: key,
      assetResolveByKey: {
        [key]: resolveResult('successCooked'),
      },
      assetCapabilitySupported: true,
    };
    const next = applyAction({ type: 'assetManifestCleared' }, state);
    expect(next.assetManifest).toBeUndefined();
    expect(next.selectedAssetKey).toBeUndefined();
    expect(next.assetResolveByKey).toBeUndefined();
    expect(next.assetCapabilitySupported).toBeUndefined();
  });

  it('preserves selection when a reloaded manifest still contains the selected asset', () => {
    const key = assetKeyForEntry(manifest.assets[1]!);
    const state: BridgeState = {
      ...INITIAL_STATE,
      selectedAssetKey: key,
    };
    const next = applyAction({ type: 'assetManifestLoaded', payload: manifest }, state);
    expect(next.selectedAssetKey).toBe(key);
  });

  it('drops selection when a reloaded manifest no longer contains the selected asset', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      selectedAssetKey: assetKeyForEntry({ logicalPath: 'missing.asset', variant: undefined }),
    };
    const next = applyAction({ type: 'assetManifestLoaded', payload: manifest }, state);
    expect(next.selectedAssetKey).toBeUndefined();
  });

  it('assetManifestError sets assetError, isolated from lastError and connection', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected', sessionId: 's1' },
      lastError: { kind: 'engine', message: 'unrelated bridge error' },
    };
    const next = applyAction(
      {
        type: 'assetManifestError',
        payload: { error: { kind: 'asset', message: 'manifest parse failed' } },
      },
      state,
    );
    expect(next.assetError).toEqual({ kind: 'asset', message: 'manifest parse failed' });
    // Asset failures must not pollute the shared lastError or the connection.
    expect(next.lastError).toEqual(state.lastError);
    expect(next.connection).toEqual(state.connection);
  });

  it('assetManifestLoaded clears a prior assetError', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      assetError: { kind: 'asset', message: 'previous load failed' },
    };
    const next = applyAction(
      {
        type: 'assetManifestLoaded',
        payload: { version: 1, manifestPath: 'C:/Project/manifest.json', assets: [] },
      },
      state,
    );
    expect(next.assetError).toBeUndefined();
  });

  it('assetManifestLoaded clears stale live health while keeping capability verdict', () => {
    const key = assetKeyForEntry(manifest.assets[0]!);
    const state: BridgeState = {
      ...INITIAL_STATE,
      selectedAssetKey: key,
      assetResolveByKey: {
        [key]: resolveResult('successCooked'),
      },
      assetCapabilitySupported: true,
    };
    const next = applyAction({ type: 'assetManifestLoaded', payload: manifest }, state);
    expect(next.assetResolveByKey).toBeUndefined();
    expect(next.assetCapabilitySupported).toBe(true);
  });

  it('assetErrorDismissed clears assetError', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      assetError: { kind: 'asset', message: 'manifest parse failed' },
    };
    const next = applyAction({ type: 'assetErrorDismissed' }, state);
    expect(next.assetError).toBeUndefined();
  });

  it('workspace close clears asset manifest state', () => {
    const key = assetKeyForEntry(manifest.assets[0]!);
    const state: BridgeState = {
      ...INITIAL_STATE,
      workspace: {
        rootPath: 'C:/Project',
        assetsRoot: 'C:/Project/Assets',
        name: 'Project',
      },
      assetManifest: manifest,
      selectedAssetKey: key,
      assetResolveByKey: {
        [key]: resolveResult('successCooked'),
      },
      assetCapabilitySupported: true,
    };
    const next = applyAction({ type: 'workspaceClosed' }, state);
    expect(next.workspace).toBeUndefined();
    expect(next.assetManifest).toBeUndefined();
    expect(next.selectedAssetKey).toBeUndefined();
    expect(next.assetResolveByKey).toBeUndefined();
    expect(next.assetCapabilitySupported).toBeUndefined();
  });

  it('workspace change clears stale asset health', () => {
    const key = assetKeyForEntry(manifest.assets[0]!);
    const state: BridgeState = {
      ...INITIAL_STATE,
      workspace: {
        rootPath: 'C:/OldProject',
        assetsRoot: 'C:/OldProject/Assets',
        name: 'OldProject',
      },
      assetManifest: manifest,
      selectedAssetKey: key,
      assetResolveByKey: {
        [key]: resolveResult('successCooked'),
      },
      assetCapabilitySupported: true,
    };
    const next = applyAction(
      {
        type: 'workspaceOpened',
        payload: {
          rootPath: 'C:/Project',
          assetsRoot: 'C:/Project/Assets',
          name: 'Project',
        },
      },
      state,
    );
    expect(next.assetManifest).toBeUndefined();
    expect(next.selectedAssetKey).toBeUndefined();
    expect(next.assetResolveByKey).toBeUndefined();
    expect(next.assetCapabilitySupported).toBeUndefined();
  });

  it('bridge disconnect preserves offline asset manifest state', () => {
    const key = assetKeyForEntry(manifest.assets[0]!);
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      assetManifest: manifest,
      selectedAssetKey: key,
      assetResolveByKey: {
        [key]: resolveResult('successCooked'),
      },
      assetResolveErrorByKey: { [key]: { kind: 'request', message: 'timeout' } },
      assetCapabilitySupported: true,
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: false, reason: 'closed' } },
      state,
    );
    expect(next.assetManifest).toEqual(manifest);
    expect(next.selectedAssetKey).toBe(key);
    expect(next.assetResolveByKey).toBeUndefined();
    expect(next.assetResolveErrorByKey).toBeUndefined();
    expect(next.assetCapabilitySupported).toBeUndefined();
  });

  it('assetResolveError records per-key failure, isolated from assetError/connection', () => {
    const key = assetKeyForEntry(manifest.assets[0]!);
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected', sessionId: 's1' },
      assetManifest: manifest,
      selectedAssetKey: key,
      // A stale success exists for this key; the error must supersede it.
      assetResolveByKey: { [key]: resolveResult('successCooked') },
    };
    const next = applyAction(
      { type: 'assetResolveError', key, payload: { error: { kind: 'request', message: 'timeout' } } },
      state,
    );
    expect(next.assetResolveErrorByKey?.[key]).toEqual({ kind: 'request', message: 'timeout' });
    expect(next.assetResolveByKey).toBeUndefined();
    // Per-asset live failure must not raise the manifest banner or touch the connection.
    expect(next.assetError).toBeUndefined();
    expect(next.connection).toEqual(state.connection);
  });

  it('assetResolveLoaded clears a prior per-key resolve error', () => {
    const key = assetKeyForEntry(manifest.assets[0]!);
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      assetResolveErrorByKey: { [key]: { kind: 'request', message: 'timeout' } },
    };
    const next = applyAction(
      { type: 'assetResolveLoaded', key, result: resolveResult('successCooked') },
      state,
    );
    expect(next.assetResolveByKey?.[key]).toEqual(resolveResult('successCooked'));
    expect(next.assetResolveErrorByKey).toBeUndefined();
  });
});

describe('asset resolve live health state', () => {
  it('assetResolveLoaded stores result by asset key and marks capability supported', () => {
    const key = assetKeyForEntry({ logicalPath: 'textures/hero.png', variant: 'default' });
    const result = resolveResult('successCooked');
    const next = applyAction({ type: 'assetResolveLoaded', key, result });
    expect(next.assetResolveByKey?.[key]).toEqual(result);
    expect(next.assetCapabilitySupported).toBe(true);
  });

  it('assetResolveUnsupported marks capability unsupported and clears stale health', () => {
    const key = assetKeyForEntry({ logicalPath: 'textures/hero.png', variant: 'default' });
    const state: BridgeState = {
      ...INITIAL_STATE,
      assetResolveByKey: {
        [key]: resolveResult('successCooked'),
      },
      assetCapabilitySupported: true,
    };
    const next = applyAction({ type: 'assetResolveUnsupported' }, state);
    expect(next.assetCapabilitySupported).toBe(false);
    expect(next.assetResolveByKey).toBeUndefined();
  });

  it('assetResolveCleared clears health and capability verdict', () => {
    const key = assetKeyForEntry({ logicalPath: 'textures/hero.png', variant: 'default' });
    const state: BridgeState = {
      ...INITIAL_STATE,
      assetResolveByKey: {
        [key]: resolveResult('successCooked'),
      },
      assetCapabilitySupported: true,
    };
    const next = applyAction({ type: 'assetResolveCleared' }, state);
    expect(next.assetResolveByKey).toBeUndefined();
    expect(next.assetCapabilitySupported).toBeUndefined();
  });

  it('connectionStateChanged(connected:true) resets capability to unknown', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      assetCapabilitySupported: false,
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: true, sessionId: 's1' } },
      state,
    );
    expect(next.assetCapabilitySupported).toBeUndefined();
  });
});

// -------------------------------------------------------------------------
// connectionStateChanged
// -------------------------------------------------------------------------

describe('connectionStateChanged', () => {
  it('sets status to connected and fills session info', () => {
    const next = applyAction({
      type: 'connectionStateChanged',
      payload: {
        connected: true,
        sessionId: 'sess-1',
        serverName: 'MockEngine',
        endpoint: 'ws://127.0.0.1:9001',
      },
    });
    expect(next.connection.status).toBe('connected');
    expect(next.connection.sessionId).toBe('sess-1');
    expect(next.connection.serverName).toBe('MockEngine');
    expect(next.connection.endpoint).toBe('ws://127.0.0.1:9001');
  });

  it('sets status to disconnected when connected is false', () => {
    const next = applyAction({
      type: 'connectionStateChanged',
      payload: { connected: false, reason: 'Socket closed' },
    });
    expect(next.connection.status).toBe('disconnected');
    expect(next.connection.reason).toBe('Socket closed');
  });

  it('clears lastError on successful connection', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      lastError: { message: 'old error' },
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: true } },
      state,
    );
    expect(next.lastError).toBeUndefined();
  });

  it('preserves lastError on disconnect', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      lastError: { message: 'old error' },
      connection: { status: 'connected' },
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: false } },
      state,
    );
    expect(next.lastError).toEqual({ message: 'old error' });
  });
});

// -------------------------------------------------------------------------
// commandPending
// -------------------------------------------------------------------------

describe('commandPending', () => {
  it('moves disconnected -> connecting', () => {
    const next = applyAction({ type: 'commandPending' });
    expect(next.connection.status).toBe('connecting');
  });

  it('does not overwrite connected status', () => {
    const state: BridgeState = { ...INITIAL_STATE, connection: { status: 'connected' } };
    const next = applyAction({ type: 'commandPending' }, state);
    expect(next.connection.status).toBe('connected');
  });

  it('does not overwrite error status', () => {
    const state: BridgeState = { ...INITIAL_STATE, connection: { status: 'error' } };
    const next = applyAction({ type: 'commandPending' }, state);
    expect(next.connection.status).toBe('error');
  });
});

// -------------------------------------------------------------------------
// statusUpdated
// -------------------------------------------------------------------------

describe('statusUpdated', () => {
  it('sets engineState and runtimeState', () => {
    const next = applyAction({
      type: 'statusUpdated',
      payload: {
        engineState: 'running',
        runtimeState: 'playing',
        engineName: 'NorvesLib',
        title: 'My Scene',
      },
    });
    expect(next.engineState).toBe('running');
    expect(next.runtimeState).toBe('playing');
    expect(next.engineName).toBe('NorvesLib');
    expect(next.title).toBe('My Scene');
  });
});

// -------------------------------------------------------------------------
// logAppended
// -------------------------------------------------------------------------

describe('logAppended', () => {
  it('appends a log entry with the given id', () => {
    const next = applyAction({
      type: 'logAppended',
      payload: { level: 'info', message: 'Hello', timestamp: '2026-06-14T10:00:00Z' },
      id: 42,
    });
    expect(next.logs).toHaveLength(1);
    expect(next.logs[0]).toMatchObject({ id: 42, level: 'info', message: 'Hello' });
  });

  it('caps logs at 1000 entries (drops oldest)', () => {
    // Start with 999 existing entries
    const baseState: BridgeState = {
      ...INITIAL_STATE,
      logs: Array.from({ length: 999 }, (_, i) => ({
        id: i,
        level: 'debug' as const,
        message: `msg${i}`,
      })),
    };

    // Append one more (total = 1000, no cap yet)
    let s = applyAction(
      { type: 'logAppended', payload: { level: 'info', message: 'entry-1000' }, id: 1000 },
      baseState,
    );
    expect(s.logs).toHaveLength(1000);

    // Append another (total would be 1001 -> cap triggers, drops oldest)
    s = applyAction(
      { type: 'logAppended', payload: { level: 'warn', message: 'entry-1001' }, id: 1001 },
      s,
    );
    expect(s.logs).toHaveLength(1000);
    expect(s.logs[0]?.id).toBe(1);       // oldest dropped is id=0
    expect(s.logs[999]?.id).toBe(1001);  // newest is 1001
  });
});

// -------------------------------------------------------------------------
// runtimeStateChanged
// -------------------------------------------------------------------------

describe('runtimeStateChanged', () => {
  it('updates runtimeState', () => {
    const next = applyAction({
      type: 'runtimeStateChanged',
      payload: { state: 'playing', previous: 'edit' },
    });
    expect(next.runtimeState).toBe('playing');
  });
});

// -------------------------------------------------------------------------
// engineStatusChanged
// -------------------------------------------------------------------------

describe('engineStatusChanged', () => {
  it('updates engineState', () => {
    const next = applyAction({
      type: 'engineStatusChanged',
      payload: { engineState: 'running', runtimeState: 'edit' },
    });
    expect(next.engineState).toBe('running');
    expect(next.runtimeState).toBe('edit');
  });

  it('preserves existing runtimeState if not provided', () => {
    const state: BridgeState = { ...INITIAL_STATE, runtimeState: 'playing' };
    const next = applyAction(
      { type: 'engineStatusChanged', payload: { engineState: 'ready' } },
      state,
    );
    expect(next.runtimeState).toBe('playing');
  });
});

// -------------------------------------------------------------------------
// errorReported
// -------------------------------------------------------------------------

describe('errorReported', () => {
  it('sets lastError and status to error', () => {
    const next = applyAction({
      type: 'errorReported',
      payload: { error: { code: 'SOME_ERR', message: 'Something went wrong' } },
    });
    expect(next.lastError).toEqual({ kind: 'SOME_ERR', message: 'Something went wrong' });
    expect(next.connection.status).toBe('error');
  });
});

// -------------------------------------------------------------------------
// engineProcessExited
// -------------------------------------------------------------------------

describe('engineProcessExited', () => {
  it('clears engineState, runtimeState and sets connection to disconnected', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      engineState: 'running',
      runtimeState: 'playing',
      connection: { status: 'connected' },
    };
    const next = applyAction(
      { type: 'engineProcessExited', payload: { exitCode: 0 } },
      state,
    );
    expect(next.engineState).toBeUndefined();
    expect(next.runtimeState).toBeUndefined();
    expect(next.connection.status).toBe('disconnected');
  });

  it('clears live asset health but keeps offline manifest and selection', () => {
    const manifest: AssetManifestPayload = {
      version: 1,
      manifestPath: 'C:/Project/manifest.json',
      assets: [{ logicalPath: 'textures/hero.png', kind: 'texture', variant: 'default' }],
    };
    const key = assetKeyForEntry(manifest.assets[0]!);
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      assetManifest: manifest,
      selectedAssetKey: key,
      assetResolveByKey: {
        [key]: resolveResult('successCooked'),
      },
      assetCapabilitySupported: true,
    };
    const next = applyAction(
      { type: 'engineProcessExited', payload: { exitCode: 0 } },
      state,
    );
    expect(next.assetManifest).toEqual(manifest);
    expect(next.selectedAssetKey).toBe(key);
    expect(next.assetResolveByKey).toBeUndefined();
    expect(next.assetCapabilitySupported).toBeUndefined();
  });
});

// -------------------------------------------------------------------------
// dismissError
// -------------------------------------------------------------------------

describe('dismissError', () => {
  it('clears lastError when set', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      lastError: { kind: 'process', message: 'engine executable not found' },
    };
    const next = applyAction({ type: 'dismissError' }, state);
    expect(next.lastError).toBeUndefined();
  });

  it('leaves connection unchanged when clearing lastError', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'error' },
      lastError: { kind: 'connect', message: 'refused' },
    };
    const next = applyAction({ type: 'dismissError' }, state);
    // lastError cleared, connection.status unchanged
    expect(next.lastError).toBeUndefined();
    expect(next.connection.status).toBe('error');
  });

  it('is a no-op when lastError is already undefined', () => {
    const next = applyAction({ type: 'dismissError' });
    expect(next.lastError).toBeUndefined();
    expect(next.connection.status).toBe('disconnected');
  });
});

// -------------------------------------------------------------------------
// objectSelected
// -------------------------------------------------------------------------

describe('objectSelected', () => {
  it('sets selectedObjectId to the given id', () => {
    const next = applyAction({ type: 'objectSelected', id: 'obj-123' });
    expect(next.selectedObjectId).toBe('obj-123');
  });

  it('sets selectedObjectId to undefined (deselect)', () => {
    const state: BridgeState = { ...INITIAL_STATE, selectedObjectId: 'obj-123' };
    const next = applyAction({ type: 'objectSelected', id: undefined }, state);
    expect(next.selectedObjectId).toBeUndefined();
  });

  it('overwrites a previous selection', () => {
    const state: BridgeState = { ...INITIAL_STATE, selectedObjectId: 'obj-111' };
    const next = applyAction({ type: 'objectSelected', id: 'obj-222' }, state);
    expect(next.selectedObjectId).toBe('obj-222');
  });

  it('does not mutate other state fields', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      engineState: 'running',
      runtimeState: 'playing',
    };
    const next = applyAction({ type: 'objectSelected', id: 'obj-abc' }, state);
    expect(next.engineState).toBe('running');
    expect(next.runtimeState).toBe('playing');
    expect(next.selectedObjectId).toBe('obj-abc');
  });
});

// -------------------------------------------------------------------------
// sceneTreeLoaded / sceneTreeUnsupported
// -------------------------------------------------------------------------

describe('sceneTreeLoaded', () => {
  it('stores the root node and clears any unsupported marker', () => {
    const state: BridgeState = { ...INITIAL_STATE, sceneUnsupported: true };
    const next = applyAction(
      { type: 'sceneTreeLoaded', root: { id: 'n-0', name: 'Root', children: [{ id: 'n-1' }] } },
      state,
    );
    expect(next.sceneTree?.id).toBe('n-0');
    expect(next.sceneTree?.children?.[0]?.id).toBe('n-1');
    expect(next.sceneUnsupported).toBe(false);
  });
});

describe('sceneTreeUnsupported', () => {
  it('sets the unsupported marker and drops any stale tree', () => {
    const state: BridgeState = { ...INITIAL_STATE, sceneTree: { id: 'n-0' } };
    const next = applyAction({ type: 'sceneTreeUnsupported' }, state);
    expect(next.sceneUnsupported).toBe(true);
    expect(next.sceneTree).toBeUndefined();
  });
});

describe('scene snapshot is cleared on disconnect', () => {
  it('connectionStateChanged(connected:false) drops sceneTree + selection', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: { id: 'n-0' },
      selectedObjectId: 'n-0',
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: false, reason: 'closed' } },
      state,
    );
    expect(next.sceneTree).toBeUndefined();
    expect(next.selectedObjectId).toBeUndefined();
  });

  it('connectionStateChanged(connected:true) clears a stale unsupported marker', () => {
    // A fresh connection re-probes the engine, so the prior "unsupported" verdict
    // must not leak across connections.
    const state: BridgeState = { ...INITIAL_STATE, sceneUnsupported: true };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: true, sessionId: 's' } },
      state,
    );
    expect(next.sceneUnsupported).toBe(false);
  });

  it('connectionStateChanged(connected:true) preserves an existing tree', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      sceneTree: { id: 'n-0' },
      selectedObjectId: 'n-0',
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: true, sessionId: 's' } },
      state,
    );
    expect(next.sceneTree?.id).toBe('n-0');
    expect(next.selectedObjectId).toBe('n-0');
  });

  it('engineProcessExited drops sceneTree + selection', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: { id: 'n-0' },
      selectedObjectId: 'n-0',
    };
    const next = applyAction(
      { type: 'engineProcessExited', payload: { exitCode: 0 } },
      state,
    );
    expect(next.sceneTree).toBeUndefined();
    expect(next.selectedObjectId).toBeUndefined();
  });
});

// -------------------------------------------------------------------------
// objectSnapshotLoaded / schemaSnapshotLoaded / objectSnapshotUnsupported
// -------------------------------------------------------------------------

describe('objectSnapshotLoaded', () => {
  it('stores the snapshot and clears any unsupported marker', () => {
    const state: BridgeState = { ...INITIAL_STATE, objectUnsupported: true };
    const next = applyAction(
      {
        type: 'objectSnapshotLoaded',
        snapshot: { objectId: 'n-1', name: 'NodeA', properties: [{ name: 'x', value: 1 }] },
      },
      state,
    );
    expect(next.objectSnapshot?.objectId).toBe('n-1');
    expect(next.objectSnapshot?.properties[0]?.value).toBe(1);
    expect(next.objectUnsupported).toBe(false);
  });
});

describe('schemaSnapshotLoaded', () => {
  it('stores the type descriptors', () => {
    const next = applyAction({
      type: 'schemaSnapshotLoaded',
      types: [{ typeName: 'TypeA', properties: [{ name: 'fieldOfView', valueType: 'number' }] }],
    });
    expect(next.schemaTypes?.[0]?.typeName).toBe('TypeA');
  });
});

describe('objectSnapshotUnsupported', () => {
  it('sets the unsupported marker and drops any stale snapshot', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      objectSnapshot: { objectId: 'n-1', properties: [] },
    };
    const next = applyAction({ type: 'objectSnapshotUnsupported' }, state);
    expect(next.objectUnsupported).toBe(true);
    expect(next.objectSnapshot).toBeUndefined();
  });
});

describe('objectPropertyApplied', () => {
  const seeded: BridgeState = {
    ...INITIAL_STATE,
    objectSnapshot: {
      objectId: 'n-1',
      name: 'NodeA',
      properties: [
        { name: 'fieldOfView', value: 60, valueType: 'number' },
        { name: 'position', value: [0, 0, 0], valueType: 'vector3' },
      ],
    },
  };

  it('replaces the named property value with the applied value', () => {
    const next = applyAction(
      { type: 'objectPropertyApplied', objectId: 'n-1', property: 'fieldOfView', appliedValue: 75 },
      seeded,
    );
    expect(next.objectSnapshot?.properties[0]?.value).toBe(75);
    // Other properties are untouched.
    expect(next.objectSnapshot?.properties[1]?.value).toEqual([0, 0, 0]);
  });

  it('applies a structured (array) value', () => {
    const next = applyAction(
      {
        type: 'objectPropertyApplied',
        objectId: 'n-1',
        property: 'position',
        appliedValue: [9, 8, 7],
      },
      seeded,
    );
    expect(next.objectSnapshot?.properties[1]?.value).toEqual([9, 8, 7]);
  });

  it('is a no-op when the snapshot objectId differs (late ack guard)', () => {
    const next = applyAction(
      { type: 'objectPropertyApplied', objectId: 'n-2', property: 'fieldOfView', appliedValue: 75 },
      seeded,
    );
    expect(next).toBe(seeded);
  });

  it('is a no-op when there is no snapshot', () => {
    const next = applyAction(
      { type: 'objectPropertyApplied', objectId: 'n-1', property: 'fieldOfView', appliedValue: 75 },
      INITIAL_STATE,
    );
    expect(next).toBe(INITIAL_STATE);
  });

  it('is a no-op when the property is absent', () => {
    const next = applyAction(
      { type: 'objectPropertyApplied', objectId: 'n-1', property: 'missing', appliedValue: 1 },
      seeded,
    );
    expect(next).toBe(seeded);
  });
});

describe('objectSelected clears the prior object snapshot', () => {
  it('drops objectSnapshot when the selection changes to a different id', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      selectedObjectId: 'n-1',
      objectSnapshot: { objectId: 'n-1', properties: [] },
    };
    const next = applyAction({ type: 'objectSelected', id: 'n-2' }, state);
    expect(next.selectedObjectId).toBe('n-2');
    expect(next.objectSnapshot).toBeUndefined();
  });

  it('drops objectSnapshot on deselect (id undefined)', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      selectedObjectId: 'n-1',
      objectSnapshot: { objectId: 'n-1', properties: [] },
    };
    const next = applyAction({ type: 'objectSelected', id: undefined }, state);
    expect(next.selectedObjectId).toBeUndefined();
    expect(next.objectSnapshot).toBeUndefined();
  });

  it('keeps objectSnapshot when re-selecting the same id', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      selectedObjectId: 'n-1',
      objectSnapshot: { objectId: 'n-1', properties: [] },
    };
    const next = applyAction({ type: 'objectSelected', id: 'n-1' }, state);
    expect(next.objectSnapshot?.objectId).toBe('n-1');
  });
});

describe('inspector data is cleared on disconnect / process exit', () => {
  it('connectionStateChanged(connected:false) drops objectSnapshot + schema', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      objectSnapshot: { objectId: 'n-1', properties: [] },
      schemaTypes: [{ typeName: 'TypeA' }],
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: false, reason: 'closed' } },
      state,
    );
    expect(next.objectSnapshot).toBeUndefined();
    expect(next.schemaTypes).toBeUndefined();
  });

  it('connectionStateChanged(connected:true) clears a stale objectUnsupported marker', () => {
    const state: BridgeState = { ...INITIAL_STATE, objectUnsupported: true };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: true, sessionId: 's' } },
      state,
    );
    expect(next.objectUnsupported).toBe(false);
  });

  it('engineProcessExited drops objectSnapshot + schema', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      objectSnapshot: { objectId: 'n-1', properties: [] },
      schemaTypes: [{ typeName: 'TypeA' }],
    };
    const next = applyAction({ type: 'engineProcessExited', payload: { exitCode: 0 } }, state);
    expect(next.objectSnapshot).toBeUndefined();
    expect(next.schemaTypes).toBeUndefined();
  });
});

// -------------------------------------------------------------------------
// viewportThumbnailLoaded / viewportThumbnailUnsupported (Phase 7b)
// -------------------------------------------------------------------------

describe('viewportThumbnailLoaded', () => {
  it('stores the thumbnail and clears any unsupported marker', () => {
    const state: BridgeState = { ...INITIAL_STATE, viewportThumbnailUnsupported: true };
    const next = applyAction(
      {
        type: 'viewportThumbnailLoaded',
        thumbnail: { imageBase64: 'AAAA', mimeType: 'image/png', width: 640, height: 360 },
      },
      state,
    );
    expect(next.viewportThumbnail?.imageBase64).toBe('AAAA');
    expect(next.viewportThumbnail?.mimeType).toBe('image/png');
    expect(next.viewportThumbnailUnsupported).toBe(false);
  });

  it('replaces a previous thumbnail wholesale', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      viewportThumbnail: { imageBase64: 'OLD', mimeType: 'image/png' },
    };
    const next = applyAction(
      { type: 'viewportThumbnailLoaded', thumbnail: { imageBase64: 'NEW', mimeType: 'image/png' } },
      state,
    );
    expect(next.viewportThumbnail?.imageBase64).toBe('NEW');
  });
});

describe('viewportThumbnailUnsupported', () => {
  it('sets the unsupported marker and drops any stale thumbnail', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      viewportThumbnail: { imageBase64: 'AAAA', mimeType: 'image/png' },
    };
    const next = applyAction({ type: 'viewportThumbnailUnsupported' }, state);
    expect(next.viewportThumbnailUnsupported).toBe(true);
    expect(next.viewportThumbnail).toBeUndefined();
  });
});

describe('viewport thumbnail is cleared on disconnect / process exit', () => {
  it('connectionStateChanged(connected:false) drops the thumbnail', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      viewportThumbnail: { imageBase64: 'AAAA', mimeType: 'image/png' },
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: false, reason: 'closed' } },
      state,
    );
    expect(next.viewportThumbnail).toBeUndefined();
  });

  it('connectionStateChanged(connected:true) clears a stale unsupported marker', () => {
    const state: BridgeState = { ...INITIAL_STATE, viewportThumbnailUnsupported: true };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: true, sessionId: 's' } },
      state,
    );
    expect(next.viewportThumbnailUnsupported).toBe(false);
  });

  it('engineProcessExited drops the thumbnail and verdict', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      viewportThumbnail: { imageBase64: 'AAAA', mimeType: 'image/png' },
      viewportThumbnailUnsupported: false,
    };
    const next = applyAction({ type: 'engineProcessExited', payload: { exitCode: 0 } }, state);
    expect(next.viewportThumbnail).toBeUndefined();
    expect(next.viewportThumbnailUnsupported).toBeUndefined();
  });
});

// -------------------------------------------------------------------------
// viewportStateChanged
// -------------------------------------------------------------------------

describe('viewportStateChanged', () => {
  it('updates viewportState from the event payload', () => {
    const next = applyAction({
      type: 'viewportStateChanged',
      payload: { state: 'focused', previous: 'hidden' },
    });
    expect(next.viewportState).toBe('focused');
  });

  it('overwrites a previously set viewportState', () => {
    const state: BridgeState = { ...INITIAL_STATE, viewportState: 'focused' };
    const next = applyAction(
      { type: 'viewportStateChanged', payload: { state: 'minimized' } },
      state,
    );
    expect(next.viewportState).toBe('minimized');
  });

  it('does not mutate other state fields', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      engineState: 'running',
      runtimeState: 'playing',
    };
    const next = applyAction(
      { type: 'viewportStateChanged', payload: { state: 'visible' } },
      state,
    );
    expect(next.engineState).toBe('running');
    expect(next.runtimeState).toBe('playing');
    expect(next.viewportState).toBe('visible');
  });
});

// -------------------------------------------------------------------------
// sceneTreeChangedLive (live event, protocol 0.2)
// -------------------------------------------------------------------------

describe('sceneTreeChangedLive', () => {
  const seeded: BridgeState = {
    ...INITIAL_STATE,
    sceneTree: {
      id: 'n-0',
      name: 'Root',
      children: [
        { id: 'n-1', name: 'NodeA' },
        { id: 'n-2', name: 'GroupNode', children: [{ id: 'n-3', name: 'NodeB' }] },
      ],
    },
  };

  it('merges a changed node by id, replacing it in place', () => {
    const next = applyAction(
      {
        type: 'sceneTreeChangedLive',
        payload: { changedNodes: [{ id: 'n-1', name: 'Renamed', kind: 'object' }] },
      },
      seeded,
    );
    expect(next.sceneTree?.children?.[0]).toEqual({ id: 'n-1', name: 'Renamed', kind: 'object' });
    // Sibling untouched.
    expect(next.sceneTree?.children?.[1]?.name).toBe('GroupNode');
  });

  it('merges a deeply nested changed node', () => {
    const next = applyAction(
      {
        type: 'sceneTreeChangedLive',
        payload: { changedNodes: [{ id: 'n-3', name: 'NodeB2' }] },
      },
      seeded,
    );
    expect(next.sceneTree?.children?.[1]?.children?.[0]).toEqual({ id: 'n-3', name: 'NodeB2' });
  });

  it('sets sceneRefreshRequired when fullRefreshRequired is true', () => {
    const next = applyAction(
      { type: 'sceneTreeChangedLive', payload: { fullRefreshRequired: true } },
      seeded,
    );
    expect(next.sceneRefreshRequired).toBe(true);
    // The stale tree is left intact until the refetch lands.
    expect(next.sceneTree).toBe(seeded.sceneTree);
  });

  it('is a no-op when no tree is in store yet', () => {
    const next = applyAction(
      {
        type: 'sceneTreeChangedLive',
        payload: { changedNodes: [{ id: 'n-1', name: 'X' }] },
      },
      INITIAL_STATE,
    );
    expect(next).toBe(INITIAL_STATE);
  });

  it('is a no-op when no changed node matches an id in the tree', () => {
    const next = applyAction(
      {
        type: 'sceneTreeChangedLive',
        payload: { changedNodes: [{ id: 'does-not-exist', name: 'X' }] },
      },
      seeded,
    );
    expect(next).toBe(seeded);
  });

  it('is a no-op when changedNodes is empty/absent and no full refresh', () => {
    const next = applyAction({ type: 'sceneTreeChangedLive', payload: {} }, seeded);
    expect(next).toBe(seeded);
  });
});

// -------------------------------------------------------------------------
// objectChangedLive (live event, protocol 0.2)
// -------------------------------------------------------------------------

describe('objectChangedLive', () => {
  const seeded: BridgeState = {
    ...INITIAL_STATE,
    selectedObjectId: 'n-1',
    objectSnapshot: {
      objectId: 'n-1',
      name: 'NodeA',
      kind: 'object',
      properties: [{ name: 'fieldOfView', value: 60, valueType: 'number' }],
    },
  };

  it('refreshes the snapshot when the changed object is selected', () => {
    const next = applyAction(
      {
        type: 'objectChangedLive',
        payload: {
          objectId: 'n-1',
          name: 'NodeA',
          kind: 'object',
          properties: [{ name: 'fieldOfView', value: 90, valueType: 'number' }],
        },
      },
      seeded,
    );
    expect(next.objectSnapshot?.properties[0]?.value).toBe(90);
    expect(next.objectSnapshot?.objectId).toBe('n-1');
  });

  it('keeps existing name/kind when the event omits them', () => {
    const next = applyAction(
      {
        type: 'objectChangedLive',
        payload: {
          objectId: 'n-1',
          properties: [{ name: 'fieldOfView', value: 33, valueType: 'number' }],
        },
      },
      seeded,
    );
    expect(next.objectSnapshot?.name).toBe('NodeA');
    expect(next.objectSnapshot?.kind).toBe('object');
  });

  it('is a no-op when the changed object is not the one in the snapshot', () => {
    const next = applyAction(
      {
        type: 'objectChangedLive',
        payload: { objectId: 'n-2', properties: [] },
      },
      seeded,
    );
    expect(next).toBe(seeded);
  });

  it('is a no-op when there is no snapshot', () => {
    const next = applyAction(
      {
        type: 'objectChangedLive',
        payload: { objectId: 'n-1', properties: [] },
      },
      INITIAL_STATE,
    );
    expect(next).toBe(INITIAL_STATE);
  });
});
