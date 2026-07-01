// @vitest-environment jsdom
/**
 * Bridge hooks lifecycle tests.
 *
 * Tests the real hook bodies (not just wrappers) using @testing-library/react's
 * renderHook + act. Covers:
 *   (a) useBridgeSubscriptions: all UnlistenFns are called on unmount (no leak).
 *   (b) useBridgeSubscriptions: StrictMode-style cleanup before subscribe
 *       resolves still unlistens.
 *   (c) useBridgeActions: invokeCommand rejection inside connect() maps to
 *       lastError + status 'error' WITHOUT throwing to render.
 *   (d) useBridgeActions: launch() invokes launch_engine and maps errors.
 *   (e) useBridgeActions: stopProcess() invokes stop_engine and maps errors.
 *
 * The hooks are mounted inside their real BridgeProvider so the full
 * dispatch -> reducer -> state path is exercised. useBridgeActions() performs
 * NO event subscription, so action tests do not need the listen mock for mount.
 */

import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import React from 'react';
import { renderHook, act } from '@testing-library/react';

// -------------------------------------------------------------------------
// Mock Tauri APIs before any imports that pull them in
// -------------------------------------------------------------------------

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import * as tauriCore from '@tauri-apps/api/core';
import * as tauriEvent from '@tauri-apps/api/event';
import { useBridgeSubscriptions, useBridgeActions } from '../useBridge.js';
import { BridgeProvider, useBridgeDispatch, useBridgeState } from '../../state/BridgeContext.js';
import { assetKeyForEntry } from '../../state/store.js';
import type { AssetResolveResult } from '@norves/bridge-ui';

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

/** Total event subscriptions that useBridgeSubscriptions registers (must equal BRIDGE_EVENTS entries). */
const EXPECTED_SUBSCRIPTION_COUNT = 11;

/**
 * Setup listen mock: each call returns a unique unlisten fn.
 * Returns the array of unlisten mocks so callers can assert on them.
 */
function setupListenMock(): Mock[] {
  const unlistenFns: Mock[] = [];
  (tauriEvent.listen as Mock).mockImplementation(() => {
    const fn = vi.fn();
    unlistenFns.push(fn);
    return Promise.resolve(fn);
  });
  return unlistenFns;
}

/** Wrapper that provides real BridgeProvider so the hook has its context. */
function wrapper({ children }: { children: React.ReactNode }): React.JSX.Element {
  return React.createElement(BridgeProvider, null, children);
}

function resolveResult(
  status: AssetResolveResult['status'],
  logicalPath = 'textures/hero.png',
): AssetResolveResult {
  return {
    status,
    source: status === 'successCooked' ? 'cooked' : 'none',
    normalizedLogicalPath: logicalPath,
    reason: status === 'cookedEntryHashMismatch' ? 'hash mismatch' : undefined,
  };
}

function createDeferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

// -------------------------------------------------------------------------
// (a) Unmount cleanup: all UnlistenFns are called
// -------------------------------------------------------------------------

describe('useBridgeSubscriptions lifecycle — unmount cleanup', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it(`calls every UnlistenFn (${EXPECTED_SUBSCRIPTION_COUNT} total) on unmount`, async () => {
    const unlistenFns = setupListenMock();

    const { unmount } = renderHook(() => useBridgeSubscriptions(), { wrapper });

    // Wait for all subscribeEvent Promises to resolve
    await act(async () => {
      await Promise.resolve();
    });

    expect(tauriEvent.listen).toHaveBeenCalledTimes(EXPECTED_SUBSCRIPTION_COUNT);
    expect(unlistenFns).toHaveLength(EXPECTED_SUBSCRIPTION_COUNT);

    // No unlisten called yet
    for (const fn of unlistenFns) {
      expect(fn).not.toHaveBeenCalled();
    }

    unmount();

    // Every unlisten fn must be called exactly once
    for (const fn of unlistenFns) {
      expect(fn).toHaveBeenCalledOnce();
    }
  });
});

// -------------------------------------------------------------------------
// (b) StrictMode-style: cleanup fires before subscribe Promises resolve
// -------------------------------------------------------------------------

describe('useBridgeSubscriptions lifecycle — late-resolving subscription cleanup', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('unlistens late-resolved subscriptions when cleanup ran first', async () => {
    // Collect resolve callbacks so we control when Promises settle
    const resolvers: Array<(fn: Mock) => void> = [];
    const unlistenFns: Mock[] = [];

    (tauriEvent.listen as Mock).mockImplementation(() => {
      return new Promise<Mock>((resolve) => {
        resolvers.push(resolve);
      });
    });

    // Mount the hook
    const { unmount } = renderHook(() => useBridgeSubscriptions(), { wrapper });

    // Unmount BEFORE any Promise resolves — simulates StrictMode effect cleanup
    unmount();

    // Now resolve all subscriptions
    await act(async () => {
      for (const resolve of resolvers) {
        const fn = vi.fn();
        unlistenFns.push(fn);
        resolve(fn);
      }
      // Flush microtasks
      await Promise.resolve();
      await Promise.resolve();
    });

    // All late-resolved unlisten fns must still be called (aborted path)
    for (const fn of unlistenFns) {
      expect(fn).toHaveBeenCalledOnce();
    }
  });
});

// -------------------------------------------------------------------------
// (c) invokeCommand rejection -> lastError + status 'error', no throw
// -------------------------------------------------------------------------

describe('useBridgeActions — error mapping', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('connect() rejection maps to lastError + connection status error without throwing', async () => {
    // Simulate Tauri returning a serde-tagged BackendError
    const fakeErr = { kind: 'CONNECT_FAILED', message: 'Connection refused' };
    (tauriCore.invoke as Mock).mockRejectedValue(fakeErr);

    // We need access to state, so render a combined hook
    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });

    // Wait for event subscriptions to settle
    await act(async () => {
      await Promise.resolve();
    });

    // Call connect — must NOT throw
    await act(async () => {
      await expect(result.current.actions.connect(9001)).resolves.toBeUndefined();
    });

    // State should reflect the error
    expect(result.current.state.connection.status).toBe('error');
    expect(result.current.state.lastError).toMatchObject({
      kind: 'CONNECT_FAILED',
      message: 'Connection refused',
    });
  });
});

// -------------------------------------------------------------------------
// (d) launch() — invokes BRIDGE_COMMANDS.launchEngine, dispatches payload,
//                maps rejection to lastError without throwing
// -------------------------------------------------------------------------

describe('useBridgeActions — launch', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('launch() invokes launch_engine with no args and dispatches the returned ConnectionStatePayload', async () => {
    const fakePayload = {
      connected: true,
      sessionId: 'sess-abc',
      serverName: 'NorvesLib',
      endpoint: '127.0.0.1:9001',
      reason: undefined,
    };
    (tauriCore.invoke as Mock).mockResolvedValue(fakePayload);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.launch()).resolves.toBeUndefined();
    });

    // invoke must have been called with 'launch_engine' and no args (no second param or empty obj)
    expect(tauriCore.invoke).toHaveBeenCalledWith('launch_engine', undefined);

    // Store must reflect connected state
    expect(result.current.state.connection.status).toBe('connected');
    expect(result.current.state.connection.sessionId).toBe('sess-abc');
  });

  it('launch() rejection maps to lastError + connection status error without throwing', async () => {
    const fakeErr = { kind: 'process', message: 'Engine binary not found' };
    (tauriCore.invoke as Mock).mockRejectedValue(fakeErr);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    // Must NOT throw
    await act(async () => {
      await expect(result.current.actions.launch()).resolves.toBeUndefined();
    });

    expect(result.current.state.connection.status).toBe('error');
    expect(result.current.state.lastError).toMatchObject({
      kind: 'process',
      message: 'Engine binary not found',
    });
  });
});

// -------------------------------------------------------------------------
// (e) stopProcess() — invokes BRIDGE_COMMANDS.stopEngine with no args
// -------------------------------------------------------------------------

describe('useBridgeActions — stopProcess', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('stopProcess() invokes stop_engine with no args and resolves without throwing', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue(null);

    const { result } = renderHook(() => useBridgeActions(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.stopProcess()).resolves.toBeUndefined();
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('stop_engine', undefined);
  });

  it('stopProcess() rejection maps to lastError without throwing', async () => {
    const fakeErr = { kind: 'process', message: 'Process already dead' };
    (tauriCore.invoke as Mock).mockRejectedValue(fakeErr);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.stopProcess()).resolves.toBeUndefined();
    });

    expect(result.current.state.lastError).toMatchObject({
      kind: 'process',
      message: 'Process already dead',
    });
  });
});

// -------------------------------------------------------------------------
// scene edit actions — invoke + refresh + degradation
// -------------------------------------------------------------------------

describe('useBridgeActions — scene edit actions', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('createObject invokes scene_create_object, refreshes the tree, and selects newId', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_create_object') return Promise.resolve({ accepted: true, newId: 'n-new' });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root', children: [{ id: 'n-new' }] } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.createObject('root', 'object')).resolves.toEqual({
        accepted: true,
        newId: 'n-new',
      });
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('scene_create_object', {
      parentId: 'root',
      kind: 'object',
    });
    expect(tauriCore.invoke).toHaveBeenCalledWith('scene_get_tree', undefined);
    expect(result.current.state.selectedObjectId).toBe('n-new');
    expect(result.current.state.sceneTree?.id).toBe('root');
  });

  it('deleteObject clears selection/snapshot only when accepted', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_delete_object') return Promise.resolve({ accepted: true });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({ type: 'objectSelected', id: 'n-1' });
      result.current.dispatch({
        type: 'objectSnapshotLoaded',
        snapshot: { objectId: 'n-1', properties: [{ name: 'label', value: 'x' }] },
      });
    });

    await act(async () => {
      await expect(result.current.actions.deleteObject('n-1')).resolves.toEqual({ accepted: true });
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('scene_delete_object', { objectId: 'n-1' });
    expect(result.current.state.selectedObjectId).toBeUndefined();
    expect(result.current.state.objectSnapshot).toBeUndefined();
  });

  it('reparentObject omits newParentId for root moves and keeps selection', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_reparent_object') return Promise.resolve({ accepted: true });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root', children: [{ id: 'n-1' }] } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({ type: 'objectSelected', id: 'n-1' });
    });

    await act(async () => {
      await expect(result.current.actions.reparentObject('n-1')).resolves.toEqual({ accepted: true });
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('scene_reparent_object', { objectId: 'n-1' });
    expect(result.current.state.selectedObjectId).toBe('n-1');
  });

  it('duplicateObject invokes scene_duplicate_object, refreshes the tree, and selects newId', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_duplicate_object') return Promise.resolve({ accepted: true, newId: 'n-copy' });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root', children: [{ id: 'n-1' }, { id: 'n-copy' }] } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.duplicateObject('n-1')).resolves.toEqual({
        accepted: true,
        newId: 'n-copy',
      });
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('scene_duplicate_object', { objectId: 'n-1' });
    expect(tauriCore.invoke).toHaveBeenCalledWith('scene_get_tree', undefined);
    expect(result.current.state.selectedObjectId).toBe('n-copy');
    expect(result.current.state.sceneTree?.id).toBe('root');
  });

  it('duplicateObject on METHOD_NOT_SUPPORTED marks sceneEditUnsupported and returns { accepted: false }', async () => {
    const engineErr = { kind: 'engine', code: 'METHOD_NOT_SUPPORTED', message: 'no scene edit' };
    (tauriCore.invoke as Mock).mockRejectedValue(engineErr);

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({ type: 'connectionStateChanged', payload: { connected: true, sessionId: 's' } });
    });

    await act(async () => {
      await expect(result.current.actions.duplicateObject('n-1')).resolves.toEqual({ accepted: false });
    });

    expect(result.current.state.sceneEditUnsupported).toBe(true);
    expect(result.current.state.connection.status).toBe('connected');
    expect(result.current.state.lastError).toBeUndefined();
  });

  it('METHOD_NOT_SUPPORTED marks sceneEditUnsupported without changing lastError or connection status', async () => {
    const engineErr = { kind: 'engine', code: 'METHOD_NOT_SUPPORTED', message: 'no scene edit' };
    (tauriCore.invoke as Mock).mockRejectedValue(engineErr);

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({ type: 'connectionStateChanged', payload: { connected: true, sessionId: 's' } });
    });

    await act(async () => {
      await expect(result.current.actions.createObject()).resolves.toEqual({ accepted: false });
    });

    expect(result.current.state.sceneEditUnsupported).toBe(true);
    expect(result.current.state.connection.status).toBe('connected');
    expect(result.current.state.lastError).toBeUndefined();
  });
});
// -------------------------------------------------------------------------
// (f) getObjectSnapshot / getSchemaSnapshot — invoke + dispatch + degradation
// -------------------------------------------------------------------------

describe('useBridgeActions — getObjectSnapshot', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('invokes object_get_snapshot with { objectId } and stores the snapshot', async () => {
    const snapshot = {
      objectId: 'n-1',
      name: 'NodeA',
      kind: 'object',
      properties: [{ name: 'label', value: 'x', valueType: 'string' }],
    };
    (tauriCore.invoke as Mock).mockResolvedValue(snapshot);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.getObjectSnapshot('n-1')).resolves.toBeUndefined();
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('object_get_snapshot', { objectId: 'n-1' });
    expect(result.current.state.objectSnapshot?.objectId).toBe('n-1');
  });

  it('maps METHOD_NOT_SUPPORTED to objectUnsupported (not a user error)', async () => {
    const engineErr = { kind: 'engine', code: 'METHOD_NOT_SUPPORTED', message: 'no object query' };
    (tauriCore.invoke as Mock).mockRejectedValue(engineErr);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.getObjectSnapshot('n-1')).resolves.toBeUndefined();
    });

    expect(result.current.state.objectUnsupported).toBe(true);
    // Not surfaced as a connection error.
    expect(result.current.state.connection.status).not.toBe('error');
  });

  it('maps a non-engine error to lastError without throwing', async () => {
    const err = { kind: 'request', message: 'timeout' };
    (tauriCore.invoke as Mock).mockRejectedValue(err);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.getObjectSnapshot('n-1')).resolves.toBeUndefined();
    });

    expect(result.current.state.lastError).toMatchObject({ kind: 'request', message: 'timeout' });
  });
});

describe('useBridgeActions — getSchemaSnapshot', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('invokes schema_get_snapshot and stores the type descriptors', async () => {
    const schema = { types: [{ typeName: 'TypeA', properties: [{ name: 'x', valueType: 'number' }] }] };
    (tauriCore.invoke as Mock).mockResolvedValue(schema);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.getSchemaSnapshot()).resolves.toBeUndefined();
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('schema_get_snapshot', undefined);
    expect(result.current.state.schemaTypes?.[0]?.typeName).toBe('TypeA');
  });

  it('maps METHOD_NOT_SUPPORTED to objectUnsupported', async () => {
    const engineErr = { kind: 'engine', code: 'METHOD_NOT_SUPPORTED', message: 'no schema query' };
    (tauriCore.invoke as Mock).mockRejectedValue(engineErr);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.getSchemaSnapshot()).resolves.toBeUndefined();
    });

    expect(result.current.state.objectUnsupported).toBe(true);
  });
});

// -------------------------------------------------------------------------
// (g) workspace helpers — invoke + store updates
// -------------------------------------------------------------------------

describe('useBridgeActions — workspace helpers', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('openWorkspace() invokes workspace_open and stores the returned workspace', async () => {
    const workspace = {
      rootPath: 'C:/Project',
      assetsRoot: 'C:/Project/Assets',
      name: 'Project',
    };
    (tauriCore.invoke as Mock).mockResolvedValue(workspace);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.openWorkspace('C:/Project')).resolves.toBeUndefined();
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('workspace_open', { rootPath: 'C:/Project' });
    expect(result.current.state.workspace).toEqual(workspace);
  });

  it('getWorkspace() clears workspace when backend returns null', async () => {
    const workspace = {
      rootPath: 'C:/Project',
      assetsRoot: 'C:/Project/Assets',
      name: 'Project',
    };
    (tauriCore.invoke as Mock)
      .mockResolvedValueOnce(workspace)
      .mockResolvedValueOnce(null);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await result.current.actions.openWorkspace('C:/Project');
    });
    expect(result.current.state.workspace).toEqual(workspace);

    await act(async () => {
      await result.current.actions.getWorkspace();
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('workspace_get');
    expect(result.current.state.workspace).toBeUndefined();
  });

  it('closeWorkspace() invokes workspace_close and clears the store', async () => {
    const workspace = {
      rootPath: 'C:/Project',
      assetsRoot: 'C:/Project/Assets',
      name: 'Project',
    };
    (tauriCore.invoke as Mock)
      .mockResolvedValueOnce(workspace)
      .mockResolvedValueOnce(undefined);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await result.current.actions.openWorkspace('C:/Project');
    });
    expect(result.current.state.workspace).toEqual(workspace);

    await act(async () => {
      await result.current.actions.closeWorkspace();
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('workspace_close');
    expect(result.current.state.workspace).toBeUndefined();
  });

  it('openWorkspace() rejection maps to lastError without throwing', async () => {
    const fakeErr = { kind: 'process', message: 'workspace Assets directory is missing' };
    (tauriCore.invoke as Mock).mockRejectedValue(fakeErr);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.openWorkspace('C:/Project')).resolves.toBeUndefined();
    });

    expect(result.current.state.lastError).toMatchObject({
      kind: 'process',
      message: 'workspace Assets directory is missing',
    });
    // Workspace errors are editor-local and MUST NOT flip the Bridge connection
    // status to 'error' (workspace is independent of the engine connection).
    expect(result.current.state.connection.status).toBe('disconnected');
  });
});

// -------------------------------------------------------------------------
// (h) asset manifest helpers — invoke + store updates + editor-local errors
// -------------------------------------------------------------------------

describe('useBridgeActions — asset manifest helpers', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('readAssetManifest() invokes asset_read_manifest and stores the manifest', async () => {
    const manifest = {
      version: 1,
      manifestPath: 'C:/Project/manifest.json',
      assets: [
        {
          logicalPath: 'textures/hero.png',
          kind: 'texture',
          variant: 'default',
        },
      ],
    };
    (tauriCore.invoke as Mock).mockResolvedValue(manifest);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(
        result.current.actions.readAssetManifest('C:/Project/manifest.json'),
      ).resolves.toBeUndefined();
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith(
      'asset_read_manifest',
      { manifestPath: 'C:/Project/manifest.json' },
    );
    expect(result.current.state.assetManifest).toEqual(manifest);
  });

  it('readAssetManifest() rejection sets assetError without changing connection status', async () => {
    const fakeErr = { kind: 'asset', message: 'manifest parse failed' };
    (tauriCore.invoke as Mock).mockRejectedValue(fakeErr);

    function useTestHook() {
      const actions = useBridgeActions();
      const dispatch = useBridgeDispatch();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    act(() => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's1' },
      });
    });
    expect(result.current.state.connection.status).toBe('connected');

    await act(async () => {
      await expect(
        result.current.actions.readAssetManifest('C:/Project/broken.json'),
      ).resolves.toBeUndefined();
    });

    expect(result.current.state.assetError).toMatchObject(fakeErr);
    expect(result.current.state.lastError).toBeUndefined();
    expect(result.current.state.connection.status).toBe('connected');
  });

  it('selectAsset() and clearAssetManifest() dispatch local asset state changes', async () => {
    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    act(() => {
      result.current.actions.selectAsset('["textures/hero.png","default"]');
    });
    expect(result.current.state.selectedAssetKey).toBe('["textures/hero.png","default"]');

    act(() => {
      result.current.actions.clearAssetManifest();
    });
    expect(result.current.state.selectedAssetKey).toBeUndefined();
    expect(result.current.state.assetManifest).toBeUndefined();
  });
});

// -------------------------------------------------------------------------
// (i) asset resolve helper — live health overlay + degradation + race guard
// -------------------------------------------------------------------------

describe('useBridgeActions — resolveAsset', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('invokes asset_resolve and stores the selected asset result by Phase B key', async () => {
    const resultPayload = resolveResult('successCooked');
    (tauriCore.invoke as Mock).mockResolvedValue(resultPayload);

    function useTestHook() {
      const actions = useBridgeActions();
      const dispatch = useBridgeDispatch();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    const key = assetKeyForEntry({ logicalPath: 'textures/hero.png', variant: 'default' });
    act(() => {
      result.current.dispatch({ type: 'assetSelected', key });
    });

    await act(async () => {
      await expect(
        result.current.actions.resolveAsset('textures/hero.png', 'texture', 'default'),
      ).resolves.toBeUndefined();
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('asset_resolve', {
      logicalPath: 'textures/hero.png',
      kind: 'texture',
      variant: 'default',
    });
    expect(result.current.state.assetResolveByKey?.[key]).toEqual(resultPayload);
    expect(result.current.state.assetCapabilitySupported).toBe(true);
  });

  it('maps METHOD_NOT_SUPPORTED to unsupported capability without a connection error', async () => {
    const engineErr = { kind: 'engine', code: 'METHOD_NOT_SUPPORTED', message: 'no asset query' };
    (tauriCore.invoke as Mock).mockRejectedValue(engineErr);

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(
        result.current.actions.resolveAsset('textures/hero.png', 'texture', 'default'),
      ).resolves.toBeUndefined();
    });

    expect(result.current.state.assetCapabilitySupported).toBe(false);
    expect(result.current.state.assetResolveByKey).toBeUndefined();
    expect(result.current.state.connection.status).not.toBe('error');
    expect(result.current.state.lastError).toBeUndefined();
  });

  it('maps a selected asset resolve error to per-key assetResolveError, not the manifest banner', async () => {
    const fakeErr = { kind: 'request', message: 'timeout' };
    (tauriCore.invoke as Mock).mockRejectedValue(fakeErr);

    function useTestHook() {
      const actions = useBridgeActions();
      const dispatch = useBridgeDispatch();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    const key = assetKeyForEntry({ logicalPath: 'textures/hero.png', variant: 'default' });
    act(() => {
      result.current.dispatch({ type: 'connectionStateChanged', payload: { connected: true } });
      result.current.dispatch({ type: 'assetSelected', key });
    });

    await act(async () => {
      await expect(
        result.current.actions.resolveAsset('textures/hero.png', 'texture', 'default'),
      ).resolves.toBeUndefined();
    });

    // The per-asset health failure is recorded by key...
    expect(result.current.state.assetResolveErrorByKey?.[key]).toMatchObject(fakeErr);
    // ...and must NOT raise the manifest-level banner, lastError, or flip the connection.
    expect(result.current.state.assetError).toBeUndefined();
    expect(result.current.state.lastError).toBeUndefined();
    expect(result.current.state.connection.status).toBe('connected');
  });

  it('discards a stale success when selection changes before asset_resolve returns', async () => {
    const deferred = createDeferred<AssetResolveResult>();
    (tauriCore.invoke as Mock).mockReturnValue(deferred.promise);

    function useTestHook() {
      const actions = useBridgeActions();
      const dispatch = useBridgeDispatch();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    const keyA = assetKeyForEntry({ logicalPath: 'textures/a.png', variant: 'default' });
    const keyB = assetKeyForEntry({ logicalPath: 'textures/b.png', variant: 'default' });

    act(() => {
      result.current.dispatch({ type: 'assetSelected', key: keyA });
    });
    const request = result.current.actions.resolveAsset('textures/a.png', 'texture', 'default');

    act(() => {
      result.current.dispatch({ type: 'assetSelected', key: keyB });
    });

    await act(async () => {
      deferred.resolve(resolveResult('successCooked', 'textures/a.png'));
      await request;
    });

    expect(result.current.state.selectedAssetKey).toBe(keyB);
    expect(result.current.state.assetResolveByKey?.[keyA]).toBeUndefined();
    expect(result.current.state.assetResolveByKey?.[keyB]).toBeUndefined();
  });

  it('discards a stale non-supported error when selection changes before asset_resolve fails', async () => {
    const deferred = createDeferred<AssetResolveResult>();
    (tauriCore.invoke as Mock).mockReturnValue(deferred.promise);

    function useTestHook() {
      const actions = useBridgeActions();
      const dispatch = useBridgeDispatch();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    const keyA = assetKeyForEntry({ logicalPath: 'textures/a.png', variant: 'default' });
    const keyB = assetKeyForEntry({ logicalPath: 'textures/b.png', variant: 'default' });

    act(() => {
      result.current.dispatch({ type: 'assetSelected', key: keyA });
    });
    const request = result.current.actions.resolveAsset('textures/a.png', 'texture', 'default');

    act(() => {
      result.current.dispatch({ type: 'assetSelected', key: keyB });
    });

    await act(async () => {
      deferred.reject({ kind: 'request', message: 'timeout' });
      await request;
    });

    expect(result.current.state.selectedAssetKey).toBe(keyB);
    expect(result.current.state.assetError).toBeUndefined();
  });

  it('discards a stale result when the connection generation changes mid-flight', async () => {
    const deferred = createDeferred<AssetResolveResult>();
    (tauriCore.invoke as Mock).mockReturnValue(deferred.promise);

    function useTestHook() {
      const actions = useBridgeActions();
      const dispatch = useBridgeDispatch();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    const key = assetKeyForEntry({ logicalPath: 'textures/a.png', variant: 'default' });

    // Connection generation 1, asset selected, resolve started on this connection.
    act(() => {
      result.current.dispatch({ type: 'connectionStateChanged', payload: { connected: true, sessionId: 's1' } });
      result.current.dispatch({ type: 'assetSelected', key });
    });
    const request = result.current.actions.resolveAsset('textures/a.png', 'texture', 'default');

    // Reconnect (new session) while the same asset stays selected.
    act(() => {
      result.current.dispatch({ type: 'connectionStateChanged', payload: { connected: true, sessionId: 's2' } });
    });

    await act(async () => {
      deferred.resolve(resolveResult('successCooked', 'textures/a.png'));
      await request;
    });

    // The old connection's result must NOT leak into the new connection.
    expect(result.current.state.selectedAssetKey).toBe(key);
    expect(result.current.state.assetResolveByKey?.[key]).toBeUndefined();
    expect(result.current.state.assetCapabilitySupported).toBeUndefined();
  });
});

// -------------------------------------------------------------------------
// (j) scene-edit undo/redo (Phase U1)
// -------------------------------------------------------------------------

/** Marks the store as connected so undo/redo guards allow issuing commands. */
function connect(dispatch: ReturnType<typeof useBridgeDispatch>): void {
  dispatch({ type: 'connectionStateChanged', payload: { connected: true, sessionId: 's1' } });
}

describe('useBridgeActions — undo/redo recording (accepted only)', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('createObject records an undoable create only when accepted:true with a newId', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_create_object') return Promise.resolve({ accepted: true, newId: 'n-new' });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root', children: [{ id: 'n-new' }] } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await result.current.actions.createObject('root', 'object');
    });

    expect(result.current.state.undoStack).toEqual([
      { kind: 'create', createdId: 'n-new', parentId: 'root', objectKind: 'object' },
    ]);
    expect(result.current.state.redoStack).toEqual([]);
  });

  it('createObject records NOTHING when accepted:false', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_create_object') return Promise.resolve({ accepted: false });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await result.current.actions.createObject('root');
    });

    expect(result.current.state.undoStack).toEqual([]);
  });

  it('duplicateObject records an undoable duplicate keyed by newId when accepted', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_duplicate_object') return Promise.resolve({ accepted: true, newId: 'n-copy' });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root', children: [{ id: 'n-1' }, { id: 'n-copy' }] } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await result.current.actions.duplicateObject('n-1', 'root');
    });

    expect(result.current.state.undoStack).toEqual([
      { kind: 'duplicate', createdId: 'n-copy', sourceId: 'n-1', parentId: 'root' },
    ]);
  });

  it('reparentObject captures oldParent SYNCHRONOUSLY before the tree refresh (B1)', async () => {
    // Seed a tree where n-1 lives under n-2. The reparent moves it to the root;
    // the getSceneTree refresh returns the MOVED tree. The recorded oldParentId
    // must still be 'n-2' (captured before issuing), not derived from the moved
    // tree.
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_reparent_object') return Promise.resolve({ accepted: true });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root', children: [{ id: 'n-1' }, { id: 'n-2' }] } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({
        type: 'sceneTreeLoaded',
        root: { id: 'root', children: [{ id: 'n-2', children: [{ id: 'n-1' }] }] },
      });
    });

    await act(async () => {
      await result.current.actions.reparentObject('n-1', undefined);
    });

    expect(result.current.state.undoStack).toEqual([
      { kind: 'reparent', objectId: 'n-1', oldParentId: 'n-2', newParentId: undefined },
    ]);
  });

  it('reparentObject of a root-level object records oldParentId undefined (B2)', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_reparent_object') return Promise.resolve({ accepted: true });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root', children: [{ id: 'n-2', children: [{ id: 'n-1' }] }] } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      // n-1 starts as a direct child of the scene root.
      result.current.dispatch({
        type: 'sceneTreeLoaded',
        root: { id: 'root', children: [{ id: 'n-1' }, { id: 'n-2' }] },
      });
    });

    await act(async () => {
      await result.current.actions.reparentObject('n-1', 'n-2');
    });

    expect(result.current.state.undoStack).toEqual([
      { kind: 'reparent', objectId: 'n-1', oldParentId: undefined, newParentId: 'n-2' },
    ]);
  });

  it('deleteObject clears both undo/redo stacks on accepted:true', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_delete_object') return Promise.resolve({ accepted: true });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({ type: 'recordSceneEdit', command: { kind: 'create', createdId: 'a' } });
    });
    expect(result.current.state.undoStack).toHaveLength(1);

    await act(async () => {
      await result.current.actions.deleteObject('a');
    });

    expect(result.current.state.undoStack).toEqual([]);
    expect(result.current.state.redoStack).toEqual([]);
  });
});

describe('useBridgeActions — undo/redo execution (S6, id-instability)', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('undo of a create issues scene_delete_object with the createdId and commits', async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    (tauriCore.invoke as Mock).mockImplementation((cmd: string, args: unknown) => {
      calls.push({ cmd, args });
      if (cmd === 'scene_delete_object') return Promise.resolve({ accepted: true });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({
        type: 'recordSceneEdit',
        command: { kind: 'create', createdId: 'n-new', parentId: 'root', objectKind: 'object' },
      });
    });

    await act(async () => {
      await result.current.actions.undo();
    });

    // Delete was issued directly with the createdId...
    expect(calls.some((c) => c.cmd === 'scene_delete_object')).toBe(true);
    expect(tauriCore.invoke).toHaveBeenCalledWith('scene_delete_object', { objectId: 'n-new' });
    // ...and the entry moved to the redo stack (undoCommitted).
    expect(result.current.state.undoStack).toEqual([]);
    expect(result.current.state.redoStack).toEqual([
      { kind: 'create', createdId: 'n-new', parentId: 'root', objectKind: 'object' },
    ]);
  });

  it('undo of a create (internal delete) does NOT clear the redo stack (S6)', async () => {
    // If undo went through the PUBLIC deleteObject wrapper it would dispatch
    // sceneEditHistoryCleared and wipe the redo stack. It must not.
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_delete_object') return Promise.resolve({ accepted: true });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({
        type: 'recordSceneEdit',
        command: { kind: 'create', createdId: 'n-new' },
      });
    });

    await act(async () => {
      await result.current.actions.undo();
    });

    // The redo stack survived the internal delete — proof it did not go through
    // the public deleteObject (which clears history).
    expect(result.current.state.redoStack).toEqual([{ kind: 'create', createdId: 'n-new' }]);
    expect(result.current.state.sceneEditUnsupported).not.toBe(true);
  });

  it('redo re-creates and a subsequent undo targets the NEW id (id-instability cycle)', async () => {
    // create → undo (delete) → redo (re-create with a NEW id) → undo (delete NEW id).
    let createCount = 0;
    const deleteTargets: string[] = [];
    (tauriCore.invoke as Mock).mockImplementation((cmd: string, args: { objectId?: string }) => {
      if (cmd === 'scene_create_object') {
        createCount += 1;
        return Promise.resolve({ accepted: true, newId: `n-${createCount}` });
      }
      if (cmd === 'scene_delete_object') {
        deleteTargets.push(args.objectId!);
        return Promise.resolve({ accepted: true });
      }
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { connect(result.current.dispatch); });

    // 1) create → newId n-1
    await act(async () => { await result.current.actions.createObject('root', 'object'); });
    expect(result.current.state.undoStack).toEqual([
      { kind: 'create', createdId: 'n-1', parentId: 'root', objectKind: 'object' },
    ]);

    // 2) undo → deletes n-1, entry moves to redo
    await act(async () => { await result.current.actions.undo(); });
    expect(deleteTargets).toEqual(['n-1']);
    expect(result.current.state.redoStack).toEqual([
      { kind: 'create', createdId: 'n-1', parentId: 'root', objectKind: 'object' },
    ]);

    // 3) redo → re-creates with a NEW id n-2; undo stack entry now carries n-2
    await act(async () => { await result.current.actions.redo(); });
    expect(result.current.state.undoStack).toEqual([
      { kind: 'create', createdId: 'n-2', parentId: 'root', objectKind: 'object' },
    ]);

    // 4) undo again → must delete the NEW id n-2, not the stale n-1
    await act(async () => { await result.current.actions.undo(); });
    expect(deleteTargets).toEqual(['n-1', 'n-2']);
  });

  it('undo accepted:false drops the entry and sets lastError', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_delete_object') return Promise.resolve({ accepted: false });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({ type: 'recordSceneEdit', command: { kind: 'create', createdId: 'stale' } });
    });

    await act(async () => { await result.current.actions.undo(); });

    expect(result.current.state.undoStack).toEqual([]);
    expect(result.current.state.lastError?.message).toBeTruthy();
  });

  it('redo accepted:false drops the entry and sets lastError', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_create_object') return Promise.resolve({ accepted: false });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({ type: 'redoCommitted' }); // no-op (empty), just to set up
      result.current.dispatch({ type: 'recordSceneEdit', command: { kind: 'create', createdId: 'a' } });
      result.current.dispatch({ type: 'undoCommitted' }); // move 'a' to redo stack
    });
    expect(result.current.state.redoStack).toEqual([{ kind: 'create', createdId: 'a' }]);

    await act(async () => { await result.current.actions.redo(); });

    expect(result.current.state.redoStack).toEqual([]);
    expect(result.current.state.lastError?.message).toBeTruthy();
  });

  it('undo is a no-op when disconnected', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue({ accepted: true });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      // Not connected; seed an undo entry directly.
      result.current.dispatch({ type: 'recordSceneEdit', command: { kind: 'create', createdId: 'a' } });
    });

    await act(async () => { await result.current.actions.undo(); });

    // Nothing issued; the stack is unchanged.
    expect(tauriCore.invoke).not.toHaveBeenCalledWith('scene_delete_object', expect.anything());
    expect(result.current.state.undoStack).toEqual([{ kind: 'create', createdId: 'a' }]);
  });
});

// -------------------------------------------------------------------------
// (k) setObjectProperty undo/redo recording (Phase U2)
// -------------------------------------------------------------------------

/** Combined hook exposing dispatch + actions + state for U2 property tests. */
function usePropHook() {
  const dispatch = useBridgeDispatch();
  const actions = useBridgeActions();
  const state = useBridgeState();
  return { actions, dispatch, state };
}

/** Seeds a connected store with a selected object snapshot holding one property. */
function seedSnapshot(
  dispatch: ReturnType<typeof useBridgeDispatch>,
  objectId: string,
  property: string,
  value: unknown,
): void {
  connect(dispatch);
  dispatch({ type: 'objectSelected', id: objectId });
  dispatch({
    type: 'objectSnapshotLoaded',
    snapshot: {
      objectId,
      properties: [{ name: property, value: value as never }],
    },
  });
}

describe('useBridgeActions — setObjectProperty recording (Phase U2)', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('records a setProperty undo entry with the captured old value and engine-applied new value', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'object_set_property') return Promise.resolve({ accepted: true, appliedValue: 'New' });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => usePropHook(), { wrapper });
    await act(async () => { seedSnapshot(result.current.dispatch, 'n-1', 'Name', 'Old'); });

    await act(async () => {
      await result.current.actions.setObjectProperty('n-1', 'Name', 'New');
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('object_set_property', {
      objectId: 'n-1',
      property: 'Name',
      value: 'New',
    });
    expect(result.current.state.undoStack).toEqual([
      { kind: 'setProperty', objectId: 'n-1', property: 'Name', oldValue: 'Old', newValue: 'New' },
    ]);
    // The snapshot reflects the engine-applied value.
    expect(result.current.state.objectSnapshot?.properties[0]?.value).toBe('New');
  });

  it('captures the old value SYNCHRONOUSLY, defeating an object.changed live-race (B1-analog)', async () => {
    // Use a deferred set-property promise so a live object.changed event can land
    // BETWEEN the synchronous old-value capture and the resolve. If capture were
    // deferred (reading the reducer post-await), the recorded oldValue would be
    // corrupted to the live event's value ('Live'). It must remain 'Old'.
    const deferred = createDeferred<{ accepted: boolean; appliedValue?: unknown }>();
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'object_set_property') return deferred.promise;
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => usePropHook(), { wrapper });
    await act(async () => { seedSnapshot(result.current.dispatch, 'n-1', 'Name', 'Old'); });

    // Start the write (synchronous capture happens here, reading 'Old').
    let request!: Promise<unknown>;
    act(() => {
      request = result.current.actions.setObjectProperty('n-1', 'Name', 'New');
    });

    // A live event overwrites the snapshot's property wholesale to 'Live' BEFORE
    // the write resolves — this is the race the synchronous capture defeats.
    act(() => {
      result.current.dispatch({
        type: 'objectChangedLive',
        payload: { objectId: 'n-1', properties: [{ name: 'Name', value: 'Live' as never }] },
      });
    });
    // Sanity: the store snapshot really did change under us mid-flight.
    expect(result.current.state.objectSnapshot?.properties[0]?.value).toBe('Live');

    await act(async () => {
      deferred.resolve({ accepted: true, appliedValue: 'New' });
      await request;
    });

    // The recorded oldValue is the pre-live 'Old', NOT the racing 'Live' value.
    expect(result.current.state.undoStack).toEqual([
      { kind: 'setProperty', objectId: 'n-1', property: 'Name', oldValue: 'Old', newValue: 'New' },
    ]);
  });

  it('records NOTHING when the applied value equals the old value (no-op skip)', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'object_set_property') return Promise.resolve({ accepted: true, appliedValue: 'Same' });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => usePropHook(), { wrapper });
    await act(async () => { seedSnapshot(result.current.dispatch, 'n-1', 'Name', 'Same'); });

    await act(async () => {
      await result.current.actions.setObjectProperty('n-1', 'Name', 'Same');
    });

    expect(result.current.state.undoStack).toEqual([]);
  });

  it('records NOTHING when no old value is available, but still performs the write', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'object_set_property') return Promise.resolve({ accepted: true, appliedValue: 'New' });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => usePropHook(), { wrapper });
    // Connected, but a DIFFERENT object is selected/snapshotted, so no old value
    // exists for n-1.
    await act(async () => { seedSnapshot(result.current.dispatch, 'other', 'Name', 'X'); });

    await act(async () => {
      await result.current.actions.setObjectProperty('n-1', 'Name', 'New');
    });

    // The write still proceeded...
    expect(tauriCore.invoke).toHaveBeenCalledWith('object_set_property', {
      objectId: 'n-1',
      property: 'Name',
      value: 'New',
    });
    // ...but nothing was recorded (no old value to invert to).
    expect(result.current.state.undoStack).toEqual([]);
  });
});

describe('useBridgeActions — setProperty undo/redo execution (Phase U2)', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('undo of a setProperty re-sets the property to oldValue and commits', async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    (tauriCore.invoke as Mock).mockImplementation((cmd: string, args: unknown) => {
      calls.push({ cmd, args });
      if (cmd === 'object_set_property') return Promise.resolve({ accepted: true, appliedValue: (args as { value: unknown }).value });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => usePropHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({
        type: 'recordSceneEdit',
        command: { kind: 'setProperty', objectId: 'n-1', property: 'Name', oldValue: 'Old', newValue: 'New' },
      });
    });

    await act(async () => { await result.current.actions.undo(); });

    // Undo issued object_set_property with the OLD value.
    expect(calls.some((c) => c.cmd === 'object_set_property' && (c.args as { value: unknown }).value === 'Old')).toBe(true);
    // Entry moved to the redo stack (undoCommitted).
    expect(result.current.state.undoStack).toEqual([]);
    expect(result.current.state.redoStack).toEqual([
      { kind: 'setProperty', objectId: 'n-1', property: 'Name', oldValue: 'Old', newValue: 'New' },
    ]);
  });

  it('redo of a setProperty re-sets the property to newValue with NO newId (id-stable)', async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    (tauriCore.invoke as Mock).mockImplementation((cmd: string, args: unknown) => {
      calls.push({ cmd, args });
      if (cmd === 'object_set_property') return Promise.resolve({ accepted: true, appliedValue: (args as { value: unknown }).value });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => usePropHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({
        type: 'recordSceneEdit',
        command: { kind: 'setProperty', objectId: 'n-1', property: 'Name', oldValue: 'Old', newValue: 'New' },
      });
      result.current.dispatch({ type: 'undoCommitted' }); // move to redo stack
    });
    expect(result.current.state.redoStack).toHaveLength(1);

    await act(async () => { await result.current.actions.redo(); });

    // Redo issued object_set_property with the NEW value.
    expect(calls.some((c) => c.cmd === 'object_set_property' && (c.args as { value: unknown }).value === 'New')).toBe(true);
    // The entry returned to the undo stack UNCHANGED (id-stable, no newId).
    expect(result.current.state.undoStack).toEqual([
      { kind: 'setProperty', objectId: 'n-1', property: 'Name', oldValue: 'Old', newValue: 'New' },
    ]);
    expect(result.current.state.redoStack).toEqual([]);
  });

  it('redo-then-undo cycle stays stable and targets the correct value at each step (B-1)', async () => {
    const propValues: unknown[] = [];
    (tauriCore.invoke as Mock).mockImplementation((cmd: string, args: unknown) => {
      if (cmd === 'object_set_property') {
        propValues.push((args as { value: unknown }).value);
        return Promise.resolve({ accepted: true, appliedValue: (args as { value: unknown }).value });
      }
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => usePropHook(), { wrapper });
    await act(async () => { seedSnapshot(result.current.dispatch, 'n-1', 'x', 1); });

    // Forward edit: 1 → 2.
    await act(async () => { await result.current.actions.setObjectProperty('n-1', 'x', 2); });
    expect(result.current.state.undoStack).toEqual([
      { kind: 'setProperty', objectId: 'n-1', property: 'x', oldValue: 1, newValue: 2 },
    ]);

    await act(async () => { await result.current.actions.undo(); }); // re-set to 1
    await act(async () => { await result.current.actions.redo(); }); // re-set to 2
    await act(async () => { await result.current.actions.undo(); }); // re-set to 1

    // The forward write recorded value=2; then undo(1), redo(2), undo(1).
    expect(propValues).toEqual([2, 1, 2, 1]);
    // After the final undo the entry sits on the redo stack, unchanged.
    expect(result.current.state.undoStack).toEqual([]);
    expect(result.current.state.redoStack).toEqual([
      { kind: 'setProperty', objectId: 'n-1', property: 'x', oldValue: 1, newValue: 2 },
    ]);
  });

  it('undo of a setProperty with accepted:false drops the entry and sets lastError', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'object_set_property') return Promise.resolve({ accepted: false });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => usePropHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({
        type: 'recordSceneEdit',
        command: { kind: 'setProperty', objectId: 'n-1', property: 'Name', oldValue: 'Old', newValue: 'New' },
      });
    });

    await act(async () => { await result.current.actions.undo(); });

    expect(result.current.state.undoStack).toEqual([]);
    expect(result.current.state.lastError?.message).toBeTruthy();
  });

  it('redo of a setProperty with accepted:false drops the entry and sets lastError', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'object_set_property') return Promise.resolve({ accepted: false });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => usePropHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({
        type: 'recordSceneEdit',
        command: { kind: 'setProperty', objectId: 'n-1', property: 'Name', oldValue: 'Old', newValue: 'New' },
      });
      result.current.dispatch({ type: 'undoCommitted' }); // move to redo stack
    });

    await act(async () => { await result.current.actions.redo(); });

    expect(result.current.state.redoStack).toEqual([]);
    expect(result.current.state.lastError?.message).toBeTruthy();
  });

  it('delete purges a pending setProperty undo entry', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'scene_delete_object') return Promise.resolve({ accepted: true });
      if (cmd === 'scene_get_tree') return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => usePropHook(), { wrapper });
    await act(async () => {
      connect(result.current.dispatch);
      result.current.dispatch({
        type: 'recordSceneEdit',
        command: { kind: 'setProperty', objectId: 'n-1', property: 'Name', oldValue: 'Old', newValue: 'New' },
      });
    });
    expect(result.current.state.undoStack).toHaveLength(1);

    await act(async () => { await result.current.actions.deleteObject('n-1'); });

    expect(result.current.state.undoStack).toEqual([]);
    expect(result.current.state.redoStack).toEqual([]);
  });
});
