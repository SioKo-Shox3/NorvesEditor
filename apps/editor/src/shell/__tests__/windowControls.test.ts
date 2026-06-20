/**
 * windowControls service tests.
 *
 * Mocks @tauri-apps/api/window so getCurrentWindow() returns a stub window
 * exposing minimize / toggleMaximize / close / isMaximized spies, following
 * the same vi.mock pattern as the bridge hook tests. Verifies each wrapper
 * calls getCurrentWindow() and the corresponding window method.
 *
 * No DOM is needed; runs in the default node environment.
 */

import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest';

// -------------------------------------------------------------------------
// Mock @tauri-apps/api/window before importing the service under test
// -------------------------------------------------------------------------

const minimize = vi.fn(() => Promise.resolve());
const toggleMaximize = vi.fn(() => Promise.resolve());
const close = vi.fn(() => Promise.resolve());
const isMaximized = vi.fn(() => Promise.resolve(false));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(() => ({
    minimize,
    toggleMaximize,
    close,
    isMaximized,
  })),
}));

import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  minimizeWindow,
  toggleMaximizeWindow,
  closeWindow,
  isWindowMaximized,
} from '../windowControls.js';

beforeEach(() => {
  vi.clearAllMocks();
});

describe('windowControls', () => {
  it('minimizeWindow resolves the current window and calls minimize()', async () => {
    await minimizeWindow();
    expect(getCurrentWindow as Mock).toHaveBeenCalledTimes(1);
    expect(minimize).toHaveBeenCalledTimes(1);
    expect(toggleMaximize).not.toHaveBeenCalled();
    expect(close).not.toHaveBeenCalled();
  });

  it('toggleMaximizeWindow resolves the current window and calls toggleMaximize()', async () => {
    await toggleMaximizeWindow();
    expect(getCurrentWindow as Mock).toHaveBeenCalledTimes(1);
    expect(toggleMaximize).toHaveBeenCalledTimes(1);
    expect(minimize).not.toHaveBeenCalled();
    expect(close).not.toHaveBeenCalled();
  });

  it('closeWindow resolves the current window and calls close()', async () => {
    await closeWindow();
    expect(getCurrentWindow as Mock).toHaveBeenCalledTimes(1);
    expect(close).toHaveBeenCalledTimes(1);
    expect(minimize).not.toHaveBeenCalled();
    expect(toggleMaximize).not.toHaveBeenCalled();
  });

  it('isWindowMaximized resolves the current window and returns isMaximized()', async () => {
    isMaximized.mockResolvedValueOnce(true);
    const result = await isWindowMaximized();
    expect(getCurrentWindow as Mock).toHaveBeenCalledTimes(1);
    expect(isMaximized).toHaveBeenCalledTimes(1);
    expect(result).toBe(true);
  });

  it('isWindowMaximized returns false when the window is not maximised', async () => {
    isMaximized.mockResolvedValueOnce(false);
    const result = await isWindowMaximized();
    expect(result).toBe(false);
  });
});
