/**
 * Bridge hook contract tests — invoke command names + subscribe lifecycle.
 *
 * Mocks @tauri-apps/api/core and @tauri-apps/api/event so tests run
 * without a real Tauri runtime. Uses the same pattern as bridge-ui's
 * existing wrappers.test.ts.
 *
 * These tests exercise the @norves/bridge-ui wrappers that the bridge hooks
 * (useBridgeActions / useBridgeSubscriptions) build on: invokeCommand call
 * names + the subscribeEvent unlisten lifecycle. Full action error-mapping
 * and subscription mount/unmount behaviour is covered in
 * useBridge.lifecycle.test.tsx.
 *
 * True end-to-end testing requires a running Tauri process + engine
 * (see plan §10 manual acceptance).
 */

import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';

// -------------------------------------------------------------------------
// Mock @tauri-apps/api before any imports that use it
// -------------------------------------------------------------------------

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import * as tauriCore from '@tauri-apps/api/core';
import * as tauriEvent from '@tauri-apps/api/event';
import { BRIDGE_COMMANDS } from '@norves/bridge-ui';

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

/** Make listen() return a unique unlisten fn per call. */
function setupListenMock(): Mock[] {
  const unlistenFns: Mock[] = [];
  (tauriEvent.listen as Mock).mockImplementation(() => {
    const fn = vi.fn();
    unlistenFns.push(fn);
    return Promise.resolve(fn);
  });
  return unlistenFns;
}

// -------------------------------------------------------------------------
// Tests for invokeCommand dispatch (not hook mount — avoids React dep)
// -------------------------------------------------------------------------

// We test the bridge-ui invoke wrappers directly (same contracts the hook uses).

describe('invokeCommand contract — connect', () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it('calls invoke with bridge_connect and {port}', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue({ connected: true });

    const { invokeCommand } = await import('@norves/bridge-ui');
    const result = await invokeCommand<{ connected: boolean }>(
      BRIDGE_COMMANDS.connect,
      { port: 9001 },
    );

    expect(tauriCore.invoke).toHaveBeenCalledOnce();
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.connect, { port: 9001 });
    expect(result.connected).toBe(true);
  });

  it('propagates rejection so error-mapping in the hook can catch it', async () => {
    const fakeErr = { kind: 'CONNECT_FAILED', message: 'refused' };
    (tauriCore.invoke as Mock).mockRejectedValue(fakeErr);

    const { invokeCommand } = await import('@norves/bridge-ui');
    await expect(
      invokeCommand(BRIDGE_COMMANDS.connect, { port: 9001 }),
    ).rejects.toEqual(fakeErr);
  });
});

describe('invokeCommand contract — runtime controls', () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it('runtime_play uses BRIDGE_COMMANDS.runtimePlay name', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue({ accepted: true });
    const { invokeCommand } = await import('@norves/bridge-ui');
    await invokeCommand(BRIDGE_COMMANDS.runtimePlay);
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.runtimePlay, undefined);
  });

  it('runtime_pause uses BRIDGE_COMMANDS.runtimePause name', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue({ accepted: true });
    const { invokeCommand } = await import('@norves/bridge-ui');
    await invokeCommand(BRIDGE_COMMANDS.runtimePause);
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.runtimePause, undefined);
  });

  it('runtime_stop uses BRIDGE_COMMANDS.runtimeStop name', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue({ accepted: true });
    const { invokeCommand } = await import('@norves/bridge-ui');
    await invokeCommand(BRIDGE_COMMANDS.runtimeStop);
    expect(tauriCore.invoke).toHaveBeenCalledWith(BRIDGE_COMMANDS.runtimeStop, undefined);
  });
});

describe('invokeCommand contract — viewport thumbnail', () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it('viewport_get_thumbnail uses BRIDGE_COMMANDS.viewportGetThumbnail with the dimension caps', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue({
      imageBase64: 'AAAA',
      mimeType: 'image/png',
    });
    const { invokeCommand } = await import('@norves/bridge-ui');
    const result = await invokeCommand<{ imageBase64: string; mimeType: string }>(
      BRIDGE_COMMANDS.viewportGetThumbnail,
      { maxWidth: 640, maxHeight: 360 },
    );
    expect(tauriCore.invoke).toHaveBeenCalledWith(
      BRIDGE_COMMANDS.viewportGetThumbnail,
      { maxWidth: 640, maxHeight: 360 },
    );
    expect(result.imageBase64).toBe('AAAA');
    expect(result.mimeType).toBe('image/png');
  });
});

// -------------------------------------------------------------------------
// subscribeEvent / unlisten lifecycle
// -------------------------------------------------------------------------

describe('subscribeEvent unlisten lifecycle', () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('listen is called for each subscribeEvent call and returns an unlisten fn', async () => {
    const unlistenFns = setupListenMock();
    const { subscribeEvent } = await import('@norves/bridge-ui');

    const fn1 = await subscribeEvent('bridge:connection-state', vi.fn());
    const fn2 = await subscribeEvent('bridge:log-message', vi.fn());

    expect(tauriEvent.listen).toHaveBeenCalledTimes(2);
    expect(unlistenFns).toHaveLength(2);

    // Calling unlisten functions should invoke the mocked fns
    fn1();
    fn2();
    expect(unlistenFns[0]).toHaveBeenCalledOnce();
    expect(unlistenFns[1]).toHaveBeenCalledOnce();
  });

  it('unlisten fns are distinct per subscription', async () => {
    const unlistenFns = setupListenMock();
    const { subscribeEvent } = await import('@norves/bridge-ui');

    const fn1 = await subscribeEvent('bridge:connection-state', vi.fn());
    const fn2 = await subscribeEvent('bridge:status-changed', vi.fn());

    expect(fn1).not.toBe(fn2);
    expect(unlistenFns[0]).not.toBe(unlistenFns[1]);
  });
});

// -------------------------------------------------------------------------
// Error extraction shape (tests the pattern used in useBridgeActions)
// -------------------------------------------------------------------------

describe('BackendError extraction', () => {
  it('extracts kind and message from a serde-tagged object', () => {
    const err = { kind: 'CONNECT_FAILED', message: 'Connection refused' };

    function extract(e: unknown): { kind?: string; message: string } {
      if (e !== null && typeof e === 'object') {
        const o = e as Record<string, unknown>;
        return {
          kind: typeof o['kind'] === 'string' ? o['kind'] : undefined,
          message: typeof o['message'] === 'string' ? o['message'] : String(e),
        };
      }
      return { message: String(e) };
    }

    const result = extract(err);
    expect(result.kind).toBe('CONNECT_FAILED');
    expect(result.message).toBe('Connection refused');
  });

  it('handles string errors gracefully', () => {
    function extract(e: unknown): { kind?: string; message: string } {
      if (e !== null && typeof e === 'object') {
        const o = e as Record<string, unknown>;
        return {
          kind: typeof o['kind'] === 'string' ? o['kind'] : undefined,
          message: typeof o['message'] === 'string' ? o['message'] : String(e),
        };
      }
      return { message: String(e) };
    }

    const result = extract('some string error');
    expect(result.kind).toBeUndefined();
    expect(result.message).toBe('some string error');
  });
});
