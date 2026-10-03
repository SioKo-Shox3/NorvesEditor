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
import { useUndoRedoKeybindings } from '../useUndoRedoKeybindings.js';
import { BridgeProvider, useBridgeDispatch, useBridgeState } from '../../state/BridgeContext.js';
import { assetKeyForEntry } from '../../state/store.js';
import { BRIDGE_COMMANDS, BRIDGE_EVENTS } from '@norves/bridge-ui';
import type {
  AssetResolveResult,
  ConnectionStatePayload,
  EditAppliedPayload,
  EditHistorySummary,
  ObjectSnapshot,
  SceneGetTreeResult,
} from '@norves/bridge-ui';

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

/** 未接続時に登録するBridgeイベント購読数。編集サービス購読は接続時に加わる。 */
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

function emitBridgeEvent(name: string, payload: unknown): void {
  const call = (tauriEvent.listen as Mock).mock.calls.find((entry) => entry[0] === name);
  const handler = call?.[1] as ((event: { payload: unknown }) => void) | undefined;
  if (handler === undefined) {
    throw new Error(`イベント購読が見つかりません: ${name}`);
  }
  handler({ payload });
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

describe('useBridgeSubscriptions — 編集サービスイベント', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('購読開始時に履歴を取得し、改訂が古いイベントを捨て、欠落時に再同期する', async () => {
    const unlistenFns = setupListenMock();
    const summary0: EditHistorySummary = {
      generation: 7,
      historyRevision: 0,
      appliedRevision: 0,
      canUndo: false,
      canRedo: false,
      undoHeadId: null,
      undoRevision: 0,
      undoGroup: null,
      redoHeadId: null,
      redoRevision: 0,
      redoGroup: null,
      pending: false,
    };
    const summary3: EditHistorySummary = {
      ...summary0,
      historyRevision: 3,
      appliedRevision: 3,
      canUndo: true,
      undoHeadId: 3,
      undoRevision: 3,
    };
    let historyCallCount = 0;
    (tauriCore.invoke as Mock).mockImplementation((command: string) => {
      if (command === BRIDGE_COMMANDS.editGetHistory) {
        historyCallCount += 1;
        return Promise.resolve(historyCallCount === 1 ? summary0 : summary3);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    function useTestHook() {
      useBridgeSubscriptions();
      const dispatch = useBridgeDispatch();
      const state = useBridgeState();
      return { dispatch, state };
    }

    const { result, unmount } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      emitBridgeEvent(BRIDGE_EVENTS.connectionState, {
        connected: true,
        sessionId: 'session-1',
      } satisfies ConnectionStatePayload);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect((tauriEvent.listen as Mock).mock.calls).toHaveLength(13);
    expect(historyCallCount).toBe(1);
    expect(result.current.state.editHistorySummary?.appliedRevision).toBe(0);

    await act(async () => {
      result.current.dispatch({ type: 'objectSelected', id: 'n-1' });
      result.current.dispatch({
        type: 'sceneTreeLoaded',
        root: { id: 'root', children: [{ id: 'n-1', name: 'Old' }] },
        editServiceGeneration: 7,
        appliedRevision: 0,
      });
      result.current.dispatch({
        type: 'objectSnapshotLoaded',
        snapshot: { objectId: 'n-1', properties: [{ name: 'Name', value: 'Old' }] },
        editServiceGeneration: 7,
        appliedRevision: 0,
      });
    });

    const rename: EditAppliedPayload = {
      operation: 'setProperty',
      objectId: 'n-1',
      property: 'Name',
      value: 'New',
      newId: null,
      source: 'mcp',
      groupId: 'mcp-7-1',
      generation: 7,
      sequence: 1,
      historyRevision: 1,
      appliedRevision: 1,
    };
    await act(async () => { emitBridgeEvent(BRIDGE_EVENTS.editApplied, rename); });
    expect(result.current.state.sceneTree?.children?.[0]?.name).toBe('New');
    expect(result.current.state.objectSnapshot?.properties[0]?.value).toBe('New');

    const staleGeneration = { ...rename, generation: 6, sequence: 99, appliedRevision: 99 };
    await act(async () => { emitBridgeEvent(BRIDGE_EVENTS.editApplied, staleGeneration); });
    const staleRevision = { ...rename, sequence: 0, value: 'Stale' };
    await act(async () => { emitBridgeEvent(BRIDGE_EVENTS.editApplied, staleRevision); });
    expect(result.current.state.editAppliedRevision).toBe(1);
    expect(result.current.state.objectSnapshot?.properties[0]?.value).toBe('New');

    historyCallCount = 1;
    const missingRevision = {
      ...rename,
      groupId: 'mcp-7-3',
      sequence: 3,
      historyRevision: 3,
      appliedRevision: 3,
      value: 'Latest',
    };
    await act(async () => {
      emitBridgeEvent(BRIDGE_EVENTS.editApplied, missingRevision);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(historyCallCount).toBe(2);
    expect(result.current.state.editHistorySummary?.appliedRevision).toBe(3);
    expect(result.current.state.editHistorySummary?.historyRevision).toBe(3);
    expect(result.current.state.sceneRefreshRequired).toBe(true);
    expect(result.current.state.objectSnapshotRefreshVersion).toBeGreaterThan(0);
    unmount();
    for (const unlisten of unlistenFns) {
      expect(unlisten).toHaveBeenCalledOnce();
    }
  });

  it('再接続後は新しい接続の世代と改訂で履歴要約を受け入れる', async () => {
    const unlistenFns = setupListenMock();
    const firstSummary: EditHistorySummary = {
      generation: 7,
      historyRevision: 4,
      appliedRevision: 4,
      canUndo: true,
      canRedo: false,
      undoHeadId: 4,
      undoRevision: 4,
      undoGroup: null,
      redoHeadId: null,
      redoRevision: 0,
      redoGroup: null,
      pending: false,
    };
    const nextSummary: EditHistorySummary = {
      ...firstSummary,
      generation: 2,
      historyRevision: 0,
      appliedRevision: 0,
      canUndo: false,
      undoHeadId: null,
      undoRevision: 0,
    };
    const synchronizedSummary: EditHistorySummary = {
      ...nextSummary,
      historyRevision: 1,
      appliedRevision: 1,
      canUndo: true,
      undoHeadId: 1,
      undoRevision: 1,
    };
    const nextSummaryFetch = createDeferred<EditHistorySummary>();
    const synchronizedSummaryFetch = createDeferred<EditHistorySummary>();
    let historyCallCount = 0;
    (tauriCore.invoke as Mock).mockImplementation((command: string) => {
      if (command === BRIDGE_COMMANDS.editGetHistory) {
        historyCallCount += 1;
        if (historyCallCount === 1) return Promise.resolve(firstSummary);
        return historyCallCount === 2 ? nextSummaryFetch.promise : synchronizedSummaryFetch.promise;
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    function useTestHook() {
      useBridgeSubscriptions();
      const dispatch = useBridgeDispatch();
      const state = useBridgeState();
      return { dispatch, state };
    }

    const { result, unmount } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      emitBridgeEvent(BRIDGE_EVENTS.connectionState, {
        connected: true,
        sessionId: 'session-1',
      } satisfies ConnectionStatePayload);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(result.current.state.editHistorySummary?.generation).toBe(7);

    await act(async () => {
      emitBridgeEvent(BRIDGE_EVENTS.connectionState, {
        connected: false,
        reason: 'closed',
      } satisfies ConnectionStatePayload);
      await Promise.resolve();
    });
    await act(async () => {
      emitBridgeEvent(BRIDGE_EVENTS.connectionState, {
        connected: true,
        sessionId: 'session-2',
      } satisfies ConnectionStatePayload);
      await Promise.resolve();
      await Promise.resolve();
    });

    await act(async () => {
      emitBridgeEvent(BRIDGE_EVENTS.editApplied, {
        operation: 'setProperty',
        objectId: 'n-1',
        property: 'Name',
        value: 'ancienne session',
        newId: null,
        source: 'mcp',
        groupId: 'mcp-7-5',
        generation: 7,
        sequence: 5,
        historyRevision: 5,
        appliedRevision: 5,
      } satisfies EditAppliedPayload);
    });
    expect(result.current.state.editServiceGeneration).toBeUndefined();
    expect(result.current.state.editAppliedRevision).toBeUndefined();
    await act(async () => {
      emitBridgeEvent(BRIDGE_EVENTS.editApplied, {
        operation: 'setProperty',
        objectId: 'n-1',
        property: 'Name',
        value: '新しい接続の変更',
        newId: null,
        source: 'mcp',
        groupId: 'mcp-2-1',
        generation: 2,
        sequence: 1,
        historyRevision: 1,
        appliedRevision: 1,
      } satisfies EditAppliedPayload);
    });

    await act(async () => {
      nextSummaryFetch.resolve(nextSummary);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(historyCallCount).toBe(3);
    await act(async () => {
      synchronizedSummaryFetch.resolve(synchronizedSummary);
      await Promise.resolve();
    });
    expect(result.current.state.editHistorySummary?.generation).toBe(2);
    expect(result.current.state.editHistorySummary?.appliedRevision).toBe(1);
    unmount();
    for (const unlisten of unlistenFns) {
      expect(unlisten).toHaveBeenCalledOnce();
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

describe('useBridgeActions — connection command/event ordering', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  function useTestHook() {
    const actions = useBridgeActions();
    const dispatch = useBridgeDispatch();
    const state = useBridgeState();
    return { actions, dispatch, state };
  }

  it.each<{
    name: string;
    command: string;
    args: Record<string, unknown> | undefined;
    invoke: (actions: ReturnType<typeof useBridgeActions>) => Promise<void>;
  }>([
    {
      name: 'connect',
      command: BRIDGE_COMMANDS.connect,
      args: { port: 9001 },
      invoke: (actions) => actions.connect(9001),
    },
    {
      name: 'reconnect',
      command: BRIDGE_COMMANDS.reconnect,
      args: undefined,
      invoke: (actions) => actions.reconnect(),
    },
    {
      name: 'launch',
      command: BRIDGE_COMMANDS.launchEngine,
      args: undefined,
      invoke: (actions) => actions.launch(),
    },
  ])('$name does not overwrite a newer disconnected event with its stale result', async ({
    command,
    args,
    invoke,
  }) => {
    const deferred = createDeferred<ConnectionStatePayload>();
    (tauriCore.invoke as Mock).mockReturnValue(deferred.promise);
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    const request = invoke(result.current.actions);
    act(() => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: false, reason: 'peer closed' },
      });
    });
    await act(async () => {
      deferred.resolve({
        connected: true,
        sessionId: 'stale-session',
        capabilities: [{ name: 'asset.reload' }],
      });
      await request;
    });

    expect(tauriCore.invoke).toHaveBeenCalledOnce();
    expect(tauriCore.invoke).toHaveBeenCalledWith(command, args);
    expect(result.current.state.connection.status).toBe('disconnected');
    expect(result.current.state.connection.capabilityNames).toBeUndefined();
  });

  it('disconnect does not overwrite a newer connected event with its stale result', async () => {
    const deferred = createDeferred<ConnectionStatePayload>();
    (tauriCore.invoke as Mock).mockReturnValue(deferred.promise);
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: {
          connected: true,
          sessionId: 'session-1',
          capabilities: [{ name: 'asset.read' }],
        },
      });
    });

    const request = result.current.actions.disconnect();
    act(() => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: {
          connected: true,
          sessionId: 'session-2',
          capabilities: [{ name: 'asset.reload' }],
        },
      });
    });
    await act(async () => {
      deferred.resolve({ connected: false, reason: 'stale disconnect result' });
      await request;
    });

    expect(tauriCore.invoke).toHaveBeenCalledOnce();
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.disconnect, undefined);
    expect(result.current.state.connection.status).toBe('connected');
    expect(result.current.state.connection.sessionId).toBe('session-2');
    expect(result.current.state.connection.capabilityNames).toEqual(new Set(['asset.reload']));
  });
});

// -------------------------------------------------------------------------
// (d) launch() — invokes BRIDGE_COMMANDS.launchEngine, follows events,
//                maps rejection to lastError without throwing
// -------------------------------------------------------------------------

describe('useBridgeActions — launch', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('launch() invokes launch_engine with no args and follows the authoritative event state', async () => {
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
      const dispatch = useBridgeDispatch();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => {
      await expect(result.current.actions.launch()).resolves.toBeUndefined();
    });

    // invoke must have been called with 'launch_engine' and no args (no second param or empty obj)
    expect(tauriCore.invoke).toHaveBeenCalledWith('launch_engine', undefined);
    expect(result.current.state.connection.status).toBe('connecting');

    act(() => {
      result.current.dispatch({ type: 'connectionStateChanged', payload: fakePayload });
    });

    // Store reflects the backend event, not the command's return payload.
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
// (h2) runtime asset manifest reload — capability guard + success lifecycle
// -------------------------------------------------------------------------

describe('useBridgeActions — reloadAssetRuntime guard and success lifecycle', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  function useTestHook() {
    const actions = useBridgeActions();
    const dispatch = useBridgeDispatch();
    const state = useBridgeState();
    return { actions, dispatch, state };
  }

  it('does not invoke reload while disconnected, capability-absent, sessionless, or unsupported', async () => {
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => { await Promise.resolve(); });

    await act(async () => { await result.current.actions.reloadAssetRuntime(); });
    act(() => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's1', capabilities: [{ name: 'asset.read' }] },
      });
    });
    await act(async () => { await result.current.actions.reloadAssetRuntime(); });
    act(() => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: '', capabilities: [{ name: 'asset.reload' }] },
      });
    });
    await act(async () => { await result.current.actions.reloadAssetRuntime(); });
    act(() => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's2', capabilities: [{ name: 'asset.reload' }] },
      });
      result.current.dispatch({ type: 'assetReloadUnsupported' });
    });
    await act(async () => { await result.current.actions.reloadAssetRuntime(); });

    expect(tauriCore.invoke).not.toHaveBeenCalled();
  });

  it('invokes asset_reload_manifest exactly once with no arguments when capability-gated', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue({ accepted: true });
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's1', capabilities: [{ name: 'asset.reload' }] },
      });
    });

    await act(async () => { await result.current.actions.reloadAssetRuntime(); });

    expect(tauriCore.invoke).toHaveBeenCalledOnce();
    expect(tauriCore.invoke).toHaveBeenCalledWith('asset_reload_manifest');
  });

  it('accepted success clears only the runtime error and never reads the offline manifest', async () => {
    const manifest = {
      version: 1,
      manifestPath: 'C:/Project/manifest.json',
      assets: [],
    };
    const offlineError = { kind: 'asset', message: 'offline parse failed' };
    (tauriCore.invoke as Mock).mockResolvedValue({ accepted: true });
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's1', capabilities: [{ name: 'asset.reload' }] },
      });
      result.current.dispatch({ type: 'assetManifestLoaded', payload: manifest });
      result.current.dispatch({ type: 'assetManifestError', payload: { error: offlineError } });
      result.current.dispatch({
        type: 'assetReloadFailed',
        payload: { error: { kind: 'asset', message: 'old runtime failure' } },
      });
    });

    await act(async () => { await result.current.actions.reloadAssetRuntime(); });

    expect(result.current.state.assetReloadError).toBeUndefined();
    expect(result.current.state.assetError).toEqual(offlineError);
    expect(result.current.state.assetManifest).toEqual(manifest);
    expect(
      (tauriCore.invoke as Mock).mock.calls.some(([command]) => command === 'asset_read_manifest'),
    ).toBe(false);
  });

  it('accepted false records a stable runtime error without changing offline asset state', async () => {
    const manifest = {
      version: 1,
      manifestPath: 'C:/Project/manifest.json',
      assets: [],
    };
    const offlineError = { kind: 'asset', message: 'offline parse failed' };
    (tauriCore.invoke as Mock).mockResolvedValue({ accepted: false });
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's1', capabilities: [{ name: 'asset.reload' }] },
      });
      result.current.dispatch({ type: 'assetManifestLoaded', payload: manifest });
      result.current.dispatch({ type: 'assetManifestError', payload: { error: offlineError } });
    });

    await act(async () => { await result.current.actions.reloadAssetRuntime(); });

    expect(result.current.state.assetReloadError).toEqual({
      kind: 'asset',
      message: 'Engine rejected runtime asset manifest reload.',
    });
    expect(result.current.state.assetError).toEqual(offlineError);
    expect(result.current.state.assetManifest).toEqual(manifest);
  });

  it('ignores a late accepted success after the connection session changes', async () => {
    const deferred = createDeferred<{ accepted: boolean }>();
    (tauriCore.invoke as Mock).mockReturnValue(deferred.promise);
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's1', capabilities: [{ name: 'asset.reload' }] },
      });
    });

    const request = result.current.actions.reloadAssetRuntime();
    act(() => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's2', capabilities: [{ name: 'asset.reload' }] },
      });
      result.current.dispatch({
        type: 'assetReloadFailed',
        payload: { error: { kind: 'asset', message: 'new session failure' } },
      });
    });
    await act(async () => {
      deferred.resolve({ accepted: true });
      await request;
    });

    expect(result.current.state.assetReloadError).toEqual({
      kind: 'asset',
      message: 'new session failure',
    });
  });

  it('degrades METHOD_NOT_SUPPORTED without changing connection or offline asset state', async () => {
    const manifest = {
      version: 1,
      manifestPath: 'C:/Project/manifest.json',
      assets: [],
    };
    const offlineError = { kind: 'asset', message: 'offline parse failed' };
    (tauriCore.invoke as Mock).mockRejectedValue({
      kind: 'engine',
      code: 'METHOD_NOT_SUPPORTED',
      message: 'runtime reload unavailable',
    });
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's1', capabilities: [{ name: 'asset.reload' }] },
      });
      result.current.dispatch({ type: 'assetManifestLoaded', payload: manifest });
      result.current.dispatch({ type: 'assetManifestError', payload: { error: offlineError } });
    });

    await act(async () => { await result.current.actions.reloadAssetRuntime(); });

    expect(result.current.state.assetReloadUnsupported).toBe(true);
    expect(result.current.state.assetReloadError).toBeUndefined();
    expect(result.current.state.connection.status).toBe('connected');
    expect(result.current.state.lastError).toBeUndefined();
    expect(result.current.state.assetError).toEqual(offlineError);
    expect(result.current.state.assetManifest).toEqual(manifest);
  });

  it('records a generic reload exception as a runtime reload error only', async () => {
    const backendError = { kind: 'request', message: 'runtime reload timed out' };
    (tauriCore.invoke as Mock).mockRejectedValue(backendError);
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's1', capabilities: [{ name: 'asset.reload' }] },
      });
    });

    await act(async () => { await result.current.actions.reloadAssetRuntime(); });

    expect(result.current.state.assetReloadError).toEqual(backendError);
    expect(result.current.state.assetReloadUnsupported).toBe(false);
    expect(result.current.state.connection.status).toBe('connected');
    expect(result.current.state.lastError).toBeUndefined();
  });

  it('ignores a late reload exception from an old connection session', async () => {
    const deferred = createDeferred<{ accepted: boolean }>();
    (tauriCore.invoke as Mock).mockReturnValue(deferred.promise);
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's1', capabilities: [{ name: 'asset.reload' }] },
      });
    });

    const request = result.current.actions.reloadAssetRuntime();
    act(() => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's2', capabilities: [{ name: 'asset.reload' }] },
      });
      result.current.dispatch({
        type: 'assetReloadFailed',
        payload: { error: { kind: 'asset', message: 'new session failure' } },
      });
    });
    await act(async () => {
      deferred.reject({
        kind: 'engine',
        code: 'METHOD_NOT_SUPPORTED',
        message: 'old session reload unavailable',
      });
      await request;
    });

    expect(result.current.state.assetReloadUnsupported).toBe(false);
    expect(result.current.state.assetReloadError).toEqual({
      kind: 'asset',
      message: 'new session failure',
    });
  });

  it('dismisses only the runtime reload error', async () => {
    const manifest = {
      version: 1,
      manifestPath: 'C:/Project/manifest.json',
      assets: [],
    };
    const offlineError = { kind: 'asset', message: 'offline parse failed' };
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({ type: 'assetManifestLoaded', payload: manifest });
      result.current.dispatch({ type: 'assetManifestError', payload: { error: offlineError } });
      result.current.dispatch({
        type: 'assetReloadFailed',
        payload: { error: { kind: 'asset', message: 'runtime reload failed' } },
      });
    });

    act(() => { result.current.actions.dismissAssetReloadError(); });

    expect(result.current.state.assetReloadError).toBeUndefined();
    expect(result.current.state.assetError).toEqual(offlineError);
    expect(result.current.state.assetManifest).toEqual(manifest);
  });

  it('re-enables reload for a fresh session after METHOD_NOT_SUPPORTED degradation', async () => {
    (tauriCore.invoke as Mock)
      .mockRejectedValueOnce({
        kind: 'engine',
        code: 'METHOD_NOT_SUPPORTED',
        message: 'runtime reload unavailable',
      })
      .mockResolvedValueOnce({ accepted: true });
    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's1', capabilities: [{ name: 'asset.reload' }] },
      });
    });

    await act(async () => { await result.current.actions.reloadAssetRuntime(); });
    expect(result.current.state.assetReloadUnsupported).toBe(true);
    expect(tauriCore.invoke).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 's2', capabilities: [{ name: 'asset.reload' }] },
      });
    });
    expect(result.current.state.assetReloadUnsupported).toBe(false);

    await act(async () => { await result.current.actions.reloadAssetRuntime(); });

    expect(tauriCore.invoke).toHaveBeenCalledTimes(2);
    expect(result.current.state.assetReloadError).toBeUndefined();
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
// 編集は画面で履歴化せず、Tauri入口へ捕捉DTOを渡す。
function makeHistorySummary(overrides: Partial<EditHistorySummary> = {}): EditHistorySummary {
  return {
    generation: 7,
    historyRevision: 3,
    appliedRevision: 11,
    canUndo: false,
    canRedo: false,
    undoHeadId: null,
    undoRevision: 0,
    undoGroup: null,
    redoHeadId: null,
    redoRevision: 0,
    redoGroup: null,
    pending: false,
    ...overrides,
  };
}

function useActionHook() {
  const dispatch = useBridgeDispatch();
  const actions = useBridgeActions();
  const state = useBridgeState();
  return { actions, dispatch, state };
}

function useKeyedActionHook() {
  const result = useActionHook();
  useUndoRedoKeybindings();
  return result;
}

function seedHistory(
  dispatch: ReturnType<typeof useBridgeDispatch>,
  summary: EditHistorySummary = makeHistorySummary(),
): void {
  dispatch({ type: 'connectionStateChanged', payload: { connected: true, sessionId: 's1' } });
  dispatch({ type: 'editHistorySummaryReceived', summary });
}

function seedPropertySnapshot(
  dispatch: ReturnType<typeof useBridgeDispatch>,
  objectId: string,
  property: string,
  value: unknown,
): void {
  dispatch({ type: 'objectSelected', id: objectId });
  dispatch({
    type: 'objectSnapshotLoaded',
    snapshot: { objectId, properties: [{ name: property, value: value as never }] },
  });
}

describe('useBridgeActions — 編集サービス履歴とのIPC境界', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('親変更前の親・世代・改訂を同期捕捉し、Tauriへ渡す', async () => {
    const response = createDeferred<{ accepted: boolean }>();
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === BRIDGE_COMMANDS.sceneReparentObject) return response.promise;
      if (cmd === BRIDGE_COMMANDS.sceneGetTree) return Promise.resolve({ root: { id: 'root' } });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => useActionHook(), { wrapper });
    await act(async () => {
      seedHistory(result.current.dispatch);
      result.current.dispatch({
        type: 'sceneTreeLoaded',
        root: { id: 'root', children: [{ id: 'n-2', children: [{ id: 'n-1' }] }] },
      });
    });

    let pending!: Promise<unknown>;
    act(() => { pending = result.current.actions.reparentObject('n-1'); });
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.sceneReparentObject, {
      objectId: 'n-1',
      capture: { generation: 7, revision: 11, parentId: 'n-2' },
    });

    act(() => {
      result.current.dispatch({
        type: 'sceneTreeChangedLive',
        payload: { changedNodes: [{ id: 'root', children: [{ id: 'n-1' }, { id: 'n-2' }] }] },
      });
    });
    await act(async () => {
      response.resolve({ accepted: true });
      await pending;
    });
  });

  it('シーン直下の親をnullとして捕捉し、値編集も旧値・適用改訂を渡す', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string, args: unknown) => {
      if (cmd === BRIDGE_COMMANDS.sceneReparentObject) return Promise.resolve({ accepted: true });
      if (cmd === BRIDGE_COMMANDS.sceneGetTree) return Promise.resolve({ root: { id: 'root' } });
      if (cmd === BRIDGE_COMMANDS.objectSetProperty) return Promise.resolve({ accepted: true, appliedValue: 'New' });
      return Promise.reject(new Error(`unexpected command ${cmd} ${String(args)}`));
    });

    const { result } = renderHook(() => useActionHook(), { wrapper });
    await act(async () => {
      seedHistory(result.current.dispatch);
      result.current.dispatch({
        type: 'sceneTreeLoaded',
        root: { id: 'root', children: [{ id: 'n-1' }] },
      });
      seedPropertySnapshot(result.current.dispatch, 'n-1', 'Name', 'Old');
    });

    await act(async () => { await result.current.actions.reparentObject('n-1', 'n-2'); });
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.sceneReparentObject, {
      objectId: 'n-1',
      newParentId: 'n-2',
      capture: { generation: 7, revision: 11, parentId: null },
    });

    await act(async () => { await result.current.actions.setObjectProperty('n-1', 'Name', 'New'); });
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.objectSetProperty, {
      objectId: 'n-1',
      property: 'Name',
      value: 'New',
      capture: { generation: 7, revision: 11, value: 'Old' },
    });
    expect(result.current.state.objectSnapshot?.properties[0]?.value).toBe('New');
  });

  it('旧値が無い場合も書き込みを続け、履歴捕捉は付けない', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === BRIDGE_COMMANDS.objectSetProperty) return Promise.resolve({ accepted: true, appliedValue: 'New' });
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => useActionHook(), { wrapper });
    await act(async () => {
      seedHistory(result.current.dispatch);
      seedPropertySnapshot(result.current.dispatch, 'other', 'Name', 'Old');
    });
    await act(async () => { await result.current.actions.setObjectProperty('n-1', 'Name', 'New'); });

    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.objectSetProperty, {
      objectId: 'n-1', property: 'Name', value: 'New',
    });
  });

  it('捕捉情報不足の拒否後はsnapshotを再取得して再操作を案内し、自動再送しない', async () => {
    const staleCapture = {
      kind: 'request',
      message: '編集前の表示が古く、履歴の旧値を安全に補正できません。対象を再取得してから操作してください。',
    };
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === BRIDGE_COMMANDS.objectSetProperty) return Promise.reject(staleCapture);
      if (cmd === BRIDGE_COMMANDS.objectGetSnapshot) {
        return Promise.resolve({ objectId: 'n-1', properties: [{ name: 'Name', value: '最新値' }] });
      }
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => useActionHook(), { wrapper });
    await act(async () => {
      seedHistory(result.current.dispatch);
      seedPropertySnapshot(result.current.dispatch, 'n-1', 'Name', '古い値');
    });
    await act(async () => {
      await expect(result.current.actions.setObjectProperty('n-1', 'Name', '変更値')).rejects.toBe(staleCapture);
    });

    expect(tauriCore.invoke).toHaveBeenCalledTimes(2);
    expect(tauriCore.invoke).toHaveBeenNthCalledWith(1, BRIDGE_COMMANDS.objectSetProperty, {
      objectId: 'n-1', property: 'Name', value: '変更値',
      capture: { generation: 7, revision: 11, value: '古い値' },
    });
    expect(tauriCore.invoke).toHaveBeenNthCalledWith(2, BRIDGE_COMMANDS.objectGetSnapshot, { objectId: 'n-1' });
    expect(result.current.state.objectSnapshot?.properties[0]?.value).toBe('最新値');
    expect(result.current.state.lastError?.message).toContain('再取得してから操作してください');
    expect(result.current.state.connection.status).toBe('connected');
  });
});

describe('useBridgeActions — undo/redo IPCと実行中ガード', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('undo/redoは履歴要約の先頭IDと改訂をTauriへ渡す', async () => {
    const updated = makeHistorySummary({ canUndo: false, canRedo: true, redoHeadId: 53, redoRevision: 9 });
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === BRIDGE_COMMANDS.editUndo || cmd === BRIDGE_COMMANDS.editRedo) return Promise.resolve(null);
      if (cmd === BRIDGE_COMMANDS.editGetHistory) return Promise.resolve(updated);
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => useActionHook(), { wrapper });
    await act(async () => { seedHistory(result.current.dispatch, makeHistorySummary({ canUndo: true, undoHeadId: 42, undoRevision: 8 })); });
    await act(async () => { await result.current.actions.undo(); });
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.editUndo, {
      expectedHeadId: 42, expectedRevision: 8,
    });
    expect(result.current.state.editHistorySummary).toEqual(updated);

    await act(async () => {
      result.current.dispatch({ type: 'editHistorySummaryReceived', summary: updated });
      await result.current.actions.redo();
    });
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.editRedo, {
      expectedHeadId: 53, expectedRevision: 9,
    });
    expect(tauriCore.invoke).not.toHaveBeenCalledWith(BRIDGE_COMMANDS.sceneDeleteObject, expect.anything());
    expect(tauriCore.invoke).not.toHaveBeenCalledWith(BRIDGE_COMMANDS.objectSetProperty, expect.anything());
  });

  it('undo/redoのIPC拒否は履歴を画面で変更せず、エラーと新しい要約を表示する', async () => {
    const undoError = { kind: 'request', message: '取り消しカーソルが古い' };
    const redoError = { kind: 'request', message: 'やり直しカーソルが古い' };
    const afterUndo = makeHistorySummary({ canUndo: false, canRedo: true, redoHeadId: 70, redoRevision: 4 });
    const afterRedo = makeHistorySummary({ canUndo: false, canRedo: false, redoHeadId: null, redoRevision: 5 });
    const summaries = [afterUndo, afterRedo];
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === BRIDGE_COMMANDS.editUndo) return Promise.reject(undoError);
      if (cmd === BRIDGE_COMMANDS.editRedo) return Promise.reject(redoError);
      if (cmd === BRIDGE_COMMANDS.editGetHistory) return Promise.resolve(summaries.shift());
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => useActionHook(), { wrapper });
    await act(async () => { seedHistory(result.current.dispatch, makeHistorySummary({ canUndo: true, undoHeadId: 69, undoRevision: 3 })); });
    await act(async () => { await result.current.actions.undo(); });
    expect(result.current.state.lastError?.message).toBe(undoError.message);
    expect(result.current.state.editHistorySummary).toEqual(afterUndo);

    await act(async () => { await result.current.actions.redo(); });
    expect(result.current.state.lastError?.message).toBe(redoError.message);
    expect(result.current.state.editHistorySummary).toEqual(afterRedo);
    expect(result.current.state.connection.status).toBe('connected');
  });

  it('play/pause/stopは共通編集サービスのTauri入口を呼ぶ', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue(null);
    const { result } = renderHook(() => useActionHook(), { wrapper });
    await act(async () => { seedHistory(result.current.dispatch); });
    await act(async () => {
      await result.current.actions.play();
      await result.current.actions.pause();
      await result.current.actions.stop();
    });
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.runtimePlay, undefined);
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.runtimePause, undefined);
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.runtimeStop, undefined);
  });

  it('履歴が空、未接続、未対応、処理中の undo/redo と runtime control は送らない', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue(null);
    const { result } = renderHook(() => useActionHook(), { wrapper });
    await act(async () => {
      await result.current.actions.undo();
      await result.current.actions.redo();
    });
    await act(async () => { seedHistory(result.current.dispatch, makeHistorySummary()); });
    await act(async () => {
      await result.current.actions.undo();
    });
    await act(async () => {
      result.current.dispatch({ type: 'editHistorySummaryReceived', summary: makeHistorySummary({ canUndo: true, undoHeadId: 8 }) });
      result.current.dispatch({ type: 'sceneEditUnsupported' });
    });
    await act(async () => {
      await result.current.actions.undo();
    });
    await act(async () => {
      result.current.dispatch({ type: 'editHistorySummaryReceived', summary: makeHistorySummary({ canUndo: true, undoHeadId: 8, pending: true }) });
    });
    await act(async () => {
      await result.current.actions.undo();
      await result.current.actions.redo();
      await result.current.actions.play();
      await result.current.actions.pause();
      await result.current.actions.stop();
    });
    expect(tauriCore.invoke).not.toHaveBeenCalledWith(BRIDGE_COMMANDS.editUndo, expect.anything());
    expect(tauriCore.invoke).not.toHaveBeenCalledWith(BRIDGE_COMMANDS.editRedo, expect.anything());
    expect(tauriCore.invoke).not.toHaveBeenCalledWith(BRIDGE_COMMANDS.runtimePlay, undefined);
    expect(tauriCore.invoke).not.toHaveBeenCalledWith(BRIDGE_COMMANDS.runtimePause, undefined);
    expect(tauriCore.invoke).not.toHaveBeenCalledWith(BRIDGE_COMMANDS.runtimeStop, undefined);
  });

  it('キーリピート中は先頭ID・改訂のundoを一度だけ送る', async () => {
    const pending = createDeferred<unknown>();
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === BRIDGE_COMMANDS.editUndo) return pending.promise;
      if (cmd === BRIDGE_COMMANDS.editGetHistory) return Promise.resolve(makeHistorySummary());
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    const { result } = renderHook(() => useKeyedActionHook(), { wrapper });
    await act(async () => {
      seedHistory(result.current.dispatch, makeHistorySummary({ canUndo: true, undoHeadId: 66, undoRevision: 12 }));
    });
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, bubbles: true }));
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, bubbles: true }));
    });
    expect(tauriCore.invoke).toHaveBeenCalledTimes(1);
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.editUndo, {
      expectedHeadId: 66, expectedRevision: 12,
    });

    await act(async () => { pending.resolve(null); await Promise.resolve(); });
  });
});
describe('useBridgeActions — snapshot取得の世代管理', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('同じ接続内でも後から開始した取得を優先し、StrictMode相当の古い応答を捨てる', async () => {
    const firstTree = createDeferred<SceneGetTreeResult>();
    const secondTree = createDeferred<SceneGetTreeResult>();
    const firstSnapshot = createDeferred<ObjectSnapshot>();
    const secondSnapshot = createDeferred<ObjectSnapshot>();
    const staleTree = createDeferred<SceneGetTreeResult>();
    const staleSnapshot = createDeferred<ObjectSnapshot>();
    const trees = [firstTree, secondTree, staleTree];
    const snapshots = [firstSnapshot, secondSnapshot, staleSnapshot];
    (tauriCore.invoke as Mock).mockImplementation((command: string) => {
      if (command === BRIDGE_COMMANDS.sceneGetTree) {
        return trees.shift()!.promise;
      }
      if (command === BRIDGE_COMMANDS.objectGetSnapshot) {
        return snapshots.shift()!.promise;
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    function useTestHook() {
      const dispatch = useBridgeDispatch();
      const actions = useBridgeActions();
      const state = useBridgeState();
      return { actions, dispatch, state };
    }

    const { result } = renderHook(() => useTestHook(), { wrapper });
    await act(async () => {
      result.current.dispatch({
        type: 'connectionStateChanged',
        payload: { connected: true, sessionId: 'session-1' },
      });
      result.current.dispatch({ type: 'objectSelected', id: 'n-1' });
      result.current.dispatch({
        type: 'editHistorySummaryReceived',
        summary: {
          generation: 1,
          historyRevision: 0,
          appliedRevision: 0,
          canUndo: false,
          canRedo: false,
          undoHeadId: null,
          undoRevision: 0,
          undoGroup: null,
          redoHeadId: null,
          redoRevision: 0,
          redoGroup: null,
          pending: false,
        },
      });
    });

    let oldTree!: Promise<void>;
    let newTree!: Promise<void>;
    let oldSnapshot!: Promise<void>;
    let newSnapshot!: Promise<void>;
    act(() => {
      oldTree = result.current.actions.getSceneTree();
      newTree = result.current.actions.getSceneTree();
      oldSnapshot = result.current.actions.getObjectSnapshot('n-1');
      newSnapshot = result.current.actions.getObjectSnapshot('n-1');
    });

    await act(async () => {
      secondTree.resolve({ root: { id: 'root', name: '最新' } });
      secondSnapshot.resolve({
        objectId: 'n-1',
        properties: [{ name: 'label', value: '最新' }],
      });
      await Promise.all([newTree, newSnapshot]);
    });
    await act(async () => {
      firstTree.resolve({ root: { id: 'root', name: '古い取得' } });
      firstSnapshot.resolve({
        objectId: 'n-1',
        properties: [{ name: 'label', value: '古い取得' }],
      });
      await Promise.all([oldTree, oldSnapshot]);
    });

    expect(result.current.state.sceneTree?.name).toBe('最新');
    expect(result.current.state.objectSnapshot?.properties[0]?.value).toBe('最新');

    let oldRevisionTree!: Promise<void>;
    let oldRevisionSnapshot!: Promise<void>;
    act(() => {
      oldRevisionTree = result.current.actions.getSceneTree();
      oldRevisionSnapshot = result.current.actions.getObjectSnapshot('n-1');
    });
    await act(async () => {
      result.current.dispatch({
        type: 'editApplied',
        payload: {
          operation: 'setProperty',
          objectId: 'n-1',
          property: 'label',
          value: 'イベント値',
          newId: null,
          source: 'mcp',
          groupId: 'mcp-1-1',
          generation: 1,
          sequence: 1,
          historyRevision: 1,
          appliedRevision: 1,
        },
      });
    });
    await act(async () => {
      staleTree.resolve({ root: { id: 'root', name: '古い改訂' } });
      staleSnapshot.resolve({
        objectId: 'n-1',
        properties: [{ name: 'label', value: '古い改訂' }],
      });
      await Promise.all([oldRevisionTree, oldRevisionSnapshot]);
    });

    expect(result.current.state.sceneTree?.name).toBe('最新');
    expect(result.current.state.sceneTreeAppliedRevision).toBe(1);
    expect(result.current.state.objectSnapshot?.properties[0]?.value).toBe('イベント値');
    expect(result.current.state.objectSnapshotAppliedRevision).toBe(1);
  });
});

// -------------------------------------------------------------------------
// (f) useBridgeActions: component structure edits (component.add / remove)
// -------------------------------------------------------------------------

describe('useBridgeActions — component structure edits', () => {
  // The invoke spy is shared across this file, so reset it per test: these
  // assertions count calls rather than only inspecting the last one.
  beforeEach(() => {
    (tauriCore.invoke as Mock).mockReset();
  });

  const wrapper = ({ children }: { children: React.ReactNode }): React.JSX.Element =>
    React.createElement(BridgeProvider, null, children);

  function useEditHook(): {
    actions: ReturnType<typeof useBridgeActions>;
    dispatch: ReturnType<typeof useBridgeDispatch>;
    state: ReturnType<typeof useBridgeState>;
  } {
    const actions = useBridgeActions();
    const dispatch = useBridgeDispatch();
    const state = useBridgeState();
    return { actions, dispatch, state };
  }

  /** Connect with the given capability tokens. */
  function connectWith(
    dispatch: ReturnType<typeof useBridgeDispatch>,
    names: string[],
    sessionId = 's1',
  ): void {
    dispatch({
      type: 'connectionStateChanged',
      payload: { connected: true, sessionId, capabilities: names.map((name) => ({ name })) },
    });
  }

  it('never invokes the engine without the component.edit capability', async () => {
    const { result } = renderHook(() => useEditHook(), { wrapper });
    // Disconnected.
    await act(async () => {
      expect(await result.current.actions.addComponent('n-2', 'camera')).toBe(false);
    });
    // Connected, but the engine did not advertise component.edit.
    act(() => connectWith(result.current.dispatch, ['object.edit']));
    await act(async () => {
      expect(await result.current.actions.addComponent('n-2', 'camera')).toBe(false);
      expect(await result.current.actions.removeComponent('component:n-2:1', 'n-2')).toBe(false);
    });

    expect(tauriCore.invoke).not.toHaveBeenCalled();
  });

  it('adds through component_add and re-reads the object from the engine', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'component_add') {
        return Promise.resolve({ accepted: true, componentId: 'component:n-2:9' });
      }
      if (cmd === 'object_get_snapshot') {
        return Promise.resolve({
          objectId: 'n-2',
          properties: [],
          components: [{ objectId: 'component:n-2:9', kind: 'camera' }],
        });
      }
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });
    const { result } = renderHook(() => useEditHook(), { wrapper });
    act(() => connectWith(result.current.dispatch, ['component.edit']));

    await act(async () => {
      expect(await result.current.actions.addComponent('n-2', 'camera')).toBe(true);
    });

    expect(tauriCore.invoke).toHaveBeenCalledWith('component_add', {
      objectId: 'n-2',
      kind: 'camera',
    });
    // The list comes from the engine's snapshot, not from the ack.
    expect(tauriCore.invoke).toHaveBeenCalledWith('object_get_snapshot', { objectId: 'n-2' });
    expect(result.current.state.objectSnapshot?.components).toHaveLength(1);
  });

  it('does not re-read when the engine refuses', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue({ accepted: false });
    const { result } = renderHook(() => useEditHook(), { wrapper });
    act(() => connectWith(result.current.dispatch, ['component.edit']));

    await act(async () => {
      expect(await result.current.actions.addComponent('n-2', 'camera')).toBe(false);
    });

    expect(tauriCore.invoke).toHaveBeenCalledOnce();
    expect(tauriCore.invoke).toHaveBeenCalledWith('component_add', {
      objectId: 'n-2',
      kind: 'camera',
    });
  });

  it('keeps a different component selected when one is detached', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'component_remove') return Promise.resolve({ accepted: true });
      if (cmd === 'object_get_snapshot') {
        return Promise.resolve({
          objectId: 'n-2',
          properties: [],
          // The detached one (…:2) is gone; the selected one (…:1) stays.
          components: [{ objectId: 'component:n-2:1', kind: 'camera' }],
        });
      }
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });
    const { result } = renderHook(() => useEditHook(), { wrapper });
    act(() => {
      connectWith(result.current.dispatch, ['component.edit']);
      result.current.dispatch({ type: 'objectSelected', id: 'n-2' });
      result.current.dispatch({ type: 'componentSelected', id: 'component:n-2:1' });
    });

    await act(async () => {
      expect(await result.current.actions.removeComponent('component:n-2:2', 'n-2')).toBe(true);
    });

    expect(result.current.state.selectedComponentId).toBe('component:n-2:1');
  });

  it('drops the selection when the detached component was the selected one', async () => {
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'component_remove') return Promise.resolve({ accepted: true });
      if (cmd === 'object_get_snapshot') {
        return Promise.resolve({ objectId: 'n-2', properties: [], components: [] });
      }
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });
    const { result } = renderHook(() => useEditHook(), { wrapper });
    act(() => {
      connectWith(result.current.dispatch, ['component.edit']);
      result.current.dispatch({ type: 'objectSelected', id: 'n-2' });
      result.current.dispatch({ type: 'componentSelected', id: 'component:n-2:1' });
    });

    await act(async () => {
      expect(await result.current.actions.removeComponent('component:n-2:1', 'n-2')).toBe(true);
    });

    expect(result.current.state.selectedComponentId).toBeUndefined();
  });

  it('discards a result that lands after the session changed', async () => {
    let resolveAdd: ((v: unknown) => void) | undefined;
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'component_add') {
        return new Promise((resolve) => {
          resolveAdd = resolve;
        });
      }
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });
    const { result } = renderHook(() => useEditHook(), { wrapper });
    act(() => connectWith(result.current.dispatch, ['component.edit'], 's1'));

    let pending: Promise<boolean> | undefined;
    act(() => {
      pending = result.current.actions.addComponent('n-2', 'camera');
    });
    act(() => connectWith(result.current.dispatch, ['component.edit'], 's2'));
    await act(async () => {
      resolveAdd?.({ accepted: true, componentId: 'component:n-2:9' });
      expect(await pending).toBe(false);
    });

    // No snapshot re-read for a result belonging to the previous session.
    expect(tauriCore.invoke).not.toHaveBeenCalledWith('object_get_snapshot', expect.anything());
  });
});
