// @vitest-environment jsdom
/**
 * layoutReset tests — cross-window layout-reset plumbing.
 *
 * @tauri-apps/api/event is mocked (emit / listen) so no Tauri runtime is needed.
 * Covered:
 *   - clearSavedLayoutAndReload: removes the v3 layout key AND reloads the page.
 *   - requestLayoutReset: emits the LAYOUT_RESET_EVENT (and nothing else).
 *   - subscribeLayoutReset: registers a listener for LAYOUT_RESET_EVENT and
 *     returns the UnlistenFn; the handler fires when the event is delivered.
 */

import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';

// -------------------------------------------------------------------------
// Mock @tauri-apps/api/event before importing the module under test.
// -------------------------------------------------------------------------

vi.mock('@tauri-apps/api/event', () => ({
  emit: vi.fn(() => Promise.resolve()),
  listen: vi.fn(() => Promise.resolve(() => undefined)),
}));

import * as tauriEvent from '@tauri-apps/api/event';
import {
  clearSavedLayoutAndReload,
  requestLayoutReset,
  subscribeLayoutReset,
  LAYOUT_RESET_EVENT,
} from '../layoutReset.js';
import { LAYOUT_STORAGE_KEY } from '../../components/shell/layoutKey.js';

// -------------------------------------------------------------------------
// In-memory localStorage (jsdom here does not provide a Storage object).
// -------------------------------------------------------------------------

function installMemoryLocalStorage(): void {
  const map = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (k: string): string | null => (map.has(k) ? map.get(k)! : null),
    setItem: (k: string, v: string): void => { map.set(k, String(v)); },
    removeItem: (k: string): void => { map.delete(k); },
    clear: (): void => { map.clear(); },
    key: (i: number): string | null => Array.from(map.keys())[i] ?? null,
    get length(): number { return map.size; },
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  installMemoryLocalStorage();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

// -------------------------------------------------------------------------
// clearSavedLayoutAndReload
// -------------------------------------------------------------------------

describe('clearSavedLayoutAndReload', () => {
  it('removes the v3 layout key and reloads the window', () => {
    localStorage.setItem(LAYOUT_STORAGE_KEY, '{"saved":true}');
    const reload = vi.fn();
    // jsdom's location.reload is not implemented; stub it.
    vi.stubGlobal('location', { ...window.location, reload });

    clearSavedLayoutAndReload();

    expect(localStorage.getItem(LAYOUT_STORAGE_KEY)).toBeNull();
    expect(reload).toHaveBeenCalledOnce();
    // Active key is v3.
    expect(LAYOUT_STORAGE_KEY).toBe('norveseditor-layout-v3');
  });

  it('still reloads when localStorage.removeItem throws', () => {
    const reload = vi.fn();
    vi.stubGlobal('location', { ...window.location, reload });
    vi.stubGlobal('localStorage', {
      removeItem: () => { throw new Error('blocked'); },
    });

    expect(() => clearSavedLayoutAndReload()).not.toThrow();
    expect(reload).toHaveBeenCalledOnce();
  });
});

// -------------------------------------------------------------------------
// requestLayoutReset
// -------------------------------------------------------------------------

describe('requestLayoutReset', () => {
  it('emits the layout-reset event (and does not reload or touch localStorage)', async () => {
    localStorage.setItem(LAYOUT_STORAGE_KEY, '{"saved":true}');
    const reload = vi.fn();
    vi.stubGlobal('location', { ...window.location, reload });

    await requestLayoutReset();

    expect(tauriEvent.emit as Mock).toHaveBeenCalledOnce();
    expect(tauriEvent.emit as Mock).toHaveBeenCalledWith(LAYOUT_RESET_EVENT);
    // The requesting window must NOT clear its own layout or reload itself.
    expect(localStorage.getItem(LAYOUT_STORAGE_KEY)).toBe('{"saved":true}');
    expect(reload).not.toHaveBeenCalled();
  });
});

// -------------------------------------------------------------------------
// subscribeLayoutReset
// -------------------------------------------------------------------------

describe('subscribeLayoutReset', () => {
  it('listens for the layout-reset event and returns the unlisten fn', async () => {
    const unlisten = vi.fn();
    (tauriEvent.listen as Mock).mockResolvedValueOnce(unlisten);

    const handler = vi.fn();
    const returned = await subscribeLayoutReset(handler);

    expect(tauriEvent.listen as Mock).toHaveBeenCalledOnce();
    expect((tauriEvent.listen as Mock).mock.calls[0]?.[0]).toBe(LAYOUT_RESET_EVENT);
    expect(returned).toBe(unlisten);
  });

  it('invokes the handler when the event is delivered', async () => {
    let captured: ((event: unknown) => void) | undefined;
    (tauriEvent.listen as Mock).mockImplementationOnce((_name: string, cb: (event: unknown) => void) => {
      captured = cb;
      return Promise.resolve(() => undefined);
    });

    const handler = vi.fn();
    await subscribeLayoutReset(handler);

    expect(handler).not.toHaveBeenCalled();
    // Simulate the event arriving.
    captured?.({ payload: undefined });
    expect(handler).toHaveBeenCalledOnce();
  });
});
