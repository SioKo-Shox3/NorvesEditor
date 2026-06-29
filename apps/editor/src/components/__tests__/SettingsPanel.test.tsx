// @vitest-environment jsdom
/**
 * SettingsPanel tests — layout-reset relay (P6).
 *
 * The Settings window cannot reset the MAIN window's layout directly, so its
 * reset button must emit a cross-window request (requestLayoutReset) and must
 * NOT touch its own localStorage or reload itself. layoutReset is mocked so we
 * assert only that the right relay function is called.
 */

import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react';
import { SettingsPanel } from '../SettingsPanel.js';
import type { IDockviewPanelProps } from 'dockview-react';
import { LAYOUT_STORAGE_KEY } from '../shell/layoutKey.js';
import { BridgeProvider } from '../../state/BridgeContext.js';

// -------------------------------------------------------------------------
// Mock the layoutReset relay so we can assert on requestLayoutReset.
// -------------------------------------------------------------------------

vi.mock('../../shell/layoutReset.js', () => ({
  requestLayoutReset: vi.fn(() => Promise.resolve()),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const { requestLayoutReset } = await import('../../shell/layoutReset.js');
const tauriCore = await import('@tauri-apps/api/core');

// -------------------------------------------------------------------------
// In-memory localStorage so we can prove the panel never touches it.
// -------------------------------------------------------------------------

function installMemoryLocalStorage(): Map<string, string> {
  const map = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (k: string): string | null => (map.has(k) ? map.get(k)! : null),
    setItem: (k: string, v: string): void => { map.set(k, String(v)); },
    removeItem: (k: string): void => { map.delete(k); },
    clear: (): void => { map.clear(); },
    key: (i: number): string | null => Array.from(map.keys())[i] ?? null,
    get length(): number { return map.size; },
  });
  return map;
}

beforeEach(() => {
  vi.clearAllMocks();
  // SettingsPanel rehydrates on mount via workspace_get; default it to "no
  // workspace" so layout-reset tests are unaffected by the rehydrate call.
  (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
    if (cmd === 'workspace_get') return Promise.resolve(null);
    return Promise.resolve(undefined);
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function renderPanel(): void {
  render(
    <BridgeProvider>
      <SettingsPanel {...({} as IDockviewPanelProps)} />
    </BridgeProvider>,
  );
}

describe('SettingsPanel layout reset (P6)', () => {
  it('emits a layout-reset request when the reset button is clicked', () => {
    renderPanel();
    fireEvent.click(screen.getByRole('button', { name: 'レイアウトをリセット' }));
    expect(requestLayoutReset as Mock).toHaveBeenCalledOnce();
  });

  it('does NOT clear its own localStorage or reload the window on reset', () => {
    const store = installMemoryLocalStorage();
    store.set(LAYOUT_STORAGE_KEY, '{"saved":true}');
    const reload = vi.fn();
    vi.stubGlobal('location', { ...window.location, reload });

    renderPanel();
    fireEvent.click(screen.getByRole('button', { name: 'レイアウトをリセット' }));

    // The reset is relayed to the main window; this window leaves its own
    // storage untouched and does not reload itself.
    expect(store.get(LAYOUT_STORAGE_KEY)).toBe('{"saved":true}');
    expect(reload).not.toHaveBeenCalled();
    expect(requestLayoutReset as Mock).toHaveBeenCalledOnce();
  });

  it('opens a workspace from the path input and displays the backend payload', async () => {
    const workspace = {
      rootPath: 'C:/Project',
      assetsRoot: 'C:/Project/Assets',
      name: 'Project',
    };
    // Mount rehydrate sees no workspace; opening returns the backend payload.
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'workspace_get') return Promise.resolve(null);
      if (cmd === 'workspace_open') return Promise.resolve(workspace);
      return Promise.resolve(undefined);
    });

    renderPanel();
    fireEvent.change(screen.getByLabelText('Workspace root'), {
      target: { value: 'C:/Project' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Open Workspace' }));

    await waitFor(() => {
      expect(screen.getByText('Project')).toBeTruthy();
    });
    expect(tauriCore.invoke as Mock).toHaveBeenCalledWith(
      'workspace_open',
      { rootPath: 'C:/Project' },
    );
    expect(screen.getByText('C:/Project/Assets')).toBeTruthy();
  });

  it('closes the current workspace', async () => {
    const workspace = {
      rootPath: 'C:/Project',
      assetsRoot: 'C:/Project/Assets',
      name: 'Project',
    };
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'workspace_get') return Promise.resolve(null);
      if (cmd === 'workspace_open') return Promise.resolve(workspace);
      return Promise.resolve(undefined);
    });

    renderPanel();
    fireEvent.change(screen.getByLabelText('Workspace root'), {
      target: { value: 'C:/Project' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Open Workspace' }));

    await waitFor(() => {
      expect(screen.getByText('Project')).toBeTruthy();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Close Workspace' }));

    await waitFor(() => {
      expect(screen.getByText('None')).toBeTruthy();
    });
    expect(tauriCore.invoke as Mock).toHaveBeenCalledWith('workspace_close');
  });

  it('rehydrates the current workspace from the backend on mount', async () => {
    const workspace = {
      rootPath: 'C:/Existing',
      assetsRoot: 'C:/Existing/Assets',
      name: 'Existing',
    };
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'workspace_get') return Promise.resolve(workspace);
      return Promise.resolve(undefined);
    });

    renderPanel();

    // No user interaction: the mount rehydrate alone must surface the backend
    // workspace and enable Close.
    await waitFor(() => {
      expect(screen.getByText('Existing')).toBeTruthy();
    });
    expect(tauriCore.invoke as Mock).toHaveBeenCalledWith('workspace_get');
    expect(
      (screen.getByRole('button', { name: 'Close Workspace' }) as HTMLButtonElement).disabled,
    ).toBe(false);
  });
});
