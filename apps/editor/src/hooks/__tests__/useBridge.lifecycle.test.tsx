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
import { BridgeProvider, useBridgeState } from '../../state/BridgeContext.js';

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
