/**
 * windowManager tests — secondary-window open / dedup behaviour.
 *
 * Mocks @tauri-apps/api/webviewWindow so no real Tauri runtime is needed.
 * Covers:
 *   (a) an existing window is focused (getByLabel → setFocus), never re-created.
 *   (b) a missing window is created with the correct label / url / decorations
 *       and tauri://created + tauri://error are subscribed once each.
 *   (c) a focus failure on the existing-window path is swallowed (non-fatal).
 */

import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest';

// -------------------------------------------------------------------------
// Mock the WebviewWindow class. vi.mock is hoisted above imports, so the shared
// state it captures must be created via vi.hoisted (also hoisted) to avoid the
// "cannot access before initialization" TDZ error. The constructor records its
// args so we can assert on the label + options; getByLabel is a vi.fn the tests
// prime per case.
// -------------------------------------------------------------------------

interface ConstructorCall {
  label: string;
  options: Record<string, unknown>;
}

const mocks = vi.hoisted(() => {
  const getByLabel = vi.fn();
  const once = vi.fn(
    (_event: string, _handler: (event: unknown) => void): Promise<() => void> =>
      Promise.resolve(() => undefined),
  );
  const constructorCalls: ConstructorCall[] = [];
  return { getByLabel, once, constructorCalls };
});

const getByLabelMock = mocks.getByLabel;
const onceMock = mocks.once;
const constructorCalls = mocks.constructorCalls;

vi.mock('@tauri-apps/api/webviewWindow', () => {
  class FakeWebviewWindow {
    label: string;
    once = mocks.once;
    constructor(label: string, options: Record<string, unknown>) {
      this.label = label;
      mocks.constructorCalls.push({ label, options });
    }
    static getByLabel = mocks.getByLabel;
  }
  return { WebviewWindow: FakeWebviewWindow };
});

import { openSecondaryWindow } from '../windowManager.js';

// -------------------------------------------------------------------------
// Setup
// -------------------------------------------------------------------------

beforeEach(() => {
  vi.clearAllMocks();
  constructorCalls.length = 0;
});

// -------------------------------------------------------------------------
// (a) existing window → focus, no re-create
// -------------------------------------------------------------------------

describe('openSecondaryWindow — dedup', () => {
  it('focuses an existing window instead of creating a new one', async () => {
    const setFocus: Mock = vi.fn(() => Promise.resolve());
    getByLabelMock.mockResolvedValue({ label: 'connection', setFocus });

    await openSecondaryWindow('connection');

    expect(getByLabelMock).toHaveBeenCalledWith('connection');
    expect(setFocus).toHaveBeenCalledTimes(1);
    // No new window constructed.
    expect(constructorCalls).toHaveLength(0);
  });

  it('swallows a setFocus rejection on the existing-window path', async () => {
    const setFocus: Mock = vi.fn(() => Promise.reject(new Error('focus failed')));
    getByLabelMock.mockResolvedValue({ label: 'settings', setFocus });
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);

    // Must not reject.
    await expect(openSecondaryWindow('settings')).resolves.toBeUndefined();
    expect(setFocus).toHaveBeenCalledTimes(1);
    expect(constructorCalls).toHaveLength(0);

    errSpy.mockRestore();
  });
});

// -------------------------------------------------------------------------
// (b) missing window → create with correct options
// -------------------------------------------------------------------------

describe('openSecondaryWindow — create', () => {
  it('creates the connection window with the right label, url and decorations', async () => {
    getByLabelMock.mockResolvedValue(null);

    await openSecondaryWindow('connection');

    expect(getByLabelMock).toHaveBeenCalledWith('connection');
    expect(constructorCalls).toHaveLength(1);
    const call = constructorCalls[0]!;
    expect(call.label).toBe('connection');
    expect(call.options.url).toBe('index.html?window=connection');
    expect(call.options.decorations).toBe(false);
    expect(call.options.resizable).toBe(true);
    expect(call.options.title).toBe('Connection');

    // tauri://created and tauri://error are each subscribed once.
    const events = onceMock.mock.calls.map((c) => c[0]);
    expect(events).toContain('tauri://created');
    expect(events).toContain('tauri://error');
  });

  it('creates the settings window with the settings route', async () => {
    getByLabelMock.mockResolvedValue(null);

    await openSecondaryWindow('settings');

    expect(constructorCalls).toHaveLength(1);
    const call = constructorCalls[0]!;
    expect(call.label).toBe('settings');
    expect(call.options.url).toBe('index.html?window=settings');
    expect(call.options.title).toBe('Settings');
  });
});
