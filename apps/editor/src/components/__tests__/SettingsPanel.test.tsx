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
import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import { SettingsPanel } from '../SettingsPanel.js';
import type { IDockviewPanelProps } from 'dockview-react';
import { LAYOUT_STORAGE_KEY } from '../shell/layoutKey.js';

// -------------------------------------------------------------------------
// Mock the layoutReset relay so we can assert on requestLayoutReset.
// -------------------------------------------------------------------------

vi.mock('../../shell/layoutReset.js', () => ({
  requestLayoutReset: vi.fn(() => Promise.resolve()),
}));

const { requestLayoutReset } = await import('../../shell/layoutReset.js');

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
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function renderPanel(): void {
  render(<SettingsPanel {...({} as IDockviewPanelProps)} />);
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
});
