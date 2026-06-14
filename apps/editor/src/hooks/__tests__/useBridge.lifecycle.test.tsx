// @vitest-environment jsdom
/**
 * useBridge hook lifecycle tests.
 *
 * Tests the real hook body (not just wrappers) using @testing-library/react's
 * renderHook + act. Covers:
 *   (a) All UnlistenFns are called on unmount (no leak).
 *   (b) StrictMode-style: cleanup before subscribe resolves still unlistens.
 *   (c) invokeCommand rejection inside connect() maps to lastError + status
 *       'error' WITHOUT throwing to render.
 *
 * The hook is mounted inside its real BridgeProvider so the full
 * dispatch -> reducer -> state path is exercised.
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
import { useBridge } from '../useBridge.js';
import { BridgeProvider, useBridgeState } from '../../state/BridgeContext.js';

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

/** Total event subscriptions that useBridge registers (must equal BRIDGE_EVENTS entries). */
const EXPECTED_SUBSCRIPTION_COUNT = 9;

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

describe('useBridge lifecycle — unmount cleanup', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it(`calls every UnlistenFn (${EXPECTED_SUBSCRIPTION_COUNT} total) on unmount`, async () => {
    const unlistenFns = setupListenMock();

    const { unmount } = renderHook(() => useBridge(), { wrapper });

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

describe('useBridge lifecycle — late-resolving subscription cleanup', () => {
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
    const { unmount } = renderHook(() => useBridge(), { wrapper });

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

describe('useBridge actions — error mapping', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('connect() rejection maps to lastError + connection status error without throwing', async () => {
    // Set up listen (required for hook mount)
    setupListenMock();

    // Simulate Tauri returning a serde-tagged BackendError
    const fakeErr = { kind: 'CONNECT_FAILED', message: 'Connection refused' };
    (tauriCore.invoke as Mock).mockRejectedValue(fakeErr);

    // We need access to state, so render a combined hook
    function useTestHook() {
      const actions = useBridge();
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
