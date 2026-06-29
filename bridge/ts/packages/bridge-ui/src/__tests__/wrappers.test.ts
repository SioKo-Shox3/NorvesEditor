import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest';
import {
  invokeCommand,
  subscribeEvent,
  workspaceOpen,
  workspaceGet,
  workspaceClose,
  assetReadManifest,
} from '../index.js';

// Mock @tauri-apps/api/core and @tauri-apps/api/event before the module is
// imported so vitest intercepts the ESM imports.

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

// Lazily import the mocked modules to access the mock functions.
import * as tauriCore from '@tauri-apps/api/core';
import * as tauriEvent from '@tauri-apps/api/event';

describe('invokeCommand', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('delegates to @tauri-apps/api/core invoke with the given name and args', async () => {
    const expected = { ok: true };
    (tauriCore.invoke as Mock).mockResolvedValue(expected);

    const args = { sessionId: 'sess-1' };
    const result = await invokeCommand<{ ok: boolean }>('bridge_hello', args);

    expect(tauriCore.invoke).toHaveBeenCalledOnce();
    expect(tauriCore.invoke).toHaveBeenCalledWith('bridge_hello', args);
    expect(result).toEqual(expected);
  });

  it('forwards the name without args when none supplied', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue(null);

    await invokeCommand('engine_get_status');

    expect(tauriCore.invoke).toHaveBeenCalledWith('engine_get_status', undefined);
  });
});

describe('workspace command wrappers', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('workspaceOpen invokes the workspace open command with rootPath', async () => {
    const payload = {
      rootPath: 'C:/Project',
      assetsRoot: 'C:/Project/Assets',
      name: 'Project',
    };
    (tauriCore.invoke as Mock).mockResolvedValue(payload);

    const result = await workspaceOpen('C:/Project');

    expect(tauriCore.invoke).toHaveBeenCalledWith('workspace_open', { rootPath: 'C:/Project' });
    expect(result).toEqual(payload);
  });

  it('workspaceGet invokes the workspace get command without args', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue(null);

    const result = await workspaceGet();

    expect(tauriCore.invoke).toHaveBeenCalledWith('workspace_get');
    expect(result).toBeNull();
  });

  it('workspaceClose invokes the workspace close command without args', async () => {
    (tauriCore.invoke as Mock).mockResolvedValue(undefined);

    await workspaceClose();

    expect(tauriCore.invoke).toHaveBeenCalledWith('workspace_close');
  });
});

describe('asset manifest command wrapper', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('assetReadManifest invokes the manifest reader command with manifestPath', async () => {
    const payload = {
      version: 1,
      manifestPath: 'C:/Project/manifest.json',
      assets: [],
    };
    (tauriCore.invoke as Mock).mockResolvedValue(payload);

    const result = await assetReadManifest('C:/Project/manifest.json');

    expect(tauriCore.invoke).toHaveBeenCalledWith(
      'asset_read_manifest',
      { manifestPath: 'C:/Project/manifest.json' },
    );
    expect(result).toEqual(payload);
  });
});

describe('subscribeEvent', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('calls listen with the given event name', async () => {
    const unlisten = vi.fn();
    (tauriEvent.listen as Mock).mockResolvedValue(unlisten);

    const handler = vi.fn();
    const result = await subscribeEvent('bridge://connected', handler);

    expect(tauriEvent.listen).toHaveBeenCalledOnce();
    const [eventName] = (tauriEvent.listen as Mock).mock.calls[0] as [string, unknown];
    expect(eventName).toBe('bridge://connected');
    expect(result).toBe(unlisten);
  });

  it('unwraps event.payload and passes it to the handler', async () => {
    const unlisten = vi.fn();
    // Capture the listener callback so we can invoke it manually.
    let capturedCallback: ((e: { payload: unknown }) => void) | undefined;
    (tauriEvent.listen as Mock).mockImplementation(
      (_name: string, cb: (e: { payload: unknown }) => void) => {
        capturedCallback = cb;
        return Promise.resolve(unlisten);
      },
    );

    const handler = vi.fn();
    await subscribeEvent<{ level: string }>('log://message', handler);

    expect(capturedCallback).toBeDefined();

    const fakePayload = { level: 'info' };
    capturedCallback!({ payload: fakePayload });

    expect(handler).toHaveBeenCalledOnce();
    expect(handler).toHaveBeenCalledWith(fakePayload);
  });
});
