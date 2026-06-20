// @vitest-environment jsdom
/**
 * AppTitleBar container tests — subscription lifecycle and unmount guard.
 *
 * Verifies:
 *   1. On mount: onResized is subscribed and isMaximized() is queried.
 *   2. On unmount: the unlisten function is called exactly once.
 *   3. Race: if unmount happens before onResized resolves, the deferred
 *      unlisten is still called when the Promise settles (no leak).
 *
 * Mocks @tauri-apps/api/window so getCurrentWindow() returns a stub.
 * Follows the vi.mock pattern from windowControls.test.ts.
 */

import { describe, it, expect, vi, afterEach, type Mock } from 'vitest';
import { render, act, cleanup } from '@testing-library/react';

// -------------------------------------------------------------------------
// Stubs set up before module imports (vi.mock is hoisted)
// -------------------------------------------------------------------------

const unlistenSpy = vi.fn();
const onResizedSpy: Mock = vi.fn(() => Promise.resolve(unlistenSpy));
const isMaximizedSpy: Mock = vi.fn(() => Promise.resolve(false));
const minimizeSpy = vi.fn(() => Promise.resolve());
const toggleMaximizeSpy = vi.fn(() => Promise.resolve());
const closeSpy = vi.fn(() => Promise.resolve());

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(() => ({
    onResized: onResizedSpy,
    isMaximized: isMaximizedSpy,
    minimize: minimizeSpy,
    toggleMaximize: toggleMaximizeSpy,
    close: closeSpy,
  })),
}));

import { AppTitleBar } from '../AppTitleBar.js';

afterEach(() => {
  // cleanup() first so any unlisten calls from the unmount are recorded,
  // then clearAllMocks() resets counts before the next test starts.
  cleanup();
  vi.clearAllMocks();
});

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

describe('AppTitleBar subscription lifecycle', () => {
  it('subscribes to onResized and queries isMaximized on mount', async () => {
    await act(async () => {
      render(<AppTitleBar title="NorvesEditor" />);
      // Drain the two Promise chains inside useEffect:
      //   isWindowMaximized().then(setMaximized)  and
      //   getCurrentWindow().onResized(...).then(fn => { unlisten = fn })
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(onResizedSpy).toHaveBeenCalledTimes(1);
    expect(isMaximizedSpy).toHaveBeenCalledTimes(1);
  });

  it('calls unlisten exactly once when unmounted after subscription resolves', async () => {
    let unmount!: () => void;

    await act(async () => {
      ({ unmount } = render(<AppTitleBar title="NorvesEditor" />));
      // Let the onResized Promise resolve so unlisten is captured.
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    // Subscription is now captured. Unmounting must call unlisten once.
    act(() => {
      unmount();
    });

    expect(unlistenSpy).toHaveBeenCalledTimes(1);
  });

  it('calls unlisten when unmount races ahead of onResized resolution', async () => {
    // Make onResized return a Promise we control manually.
    let resolveOnResized!: (fn: () => void) => void;
    const pendingPromise = new Promise<() => void>((resolve) => {
      resolveOnResized = resolve;
    });
    onResizedSpy.mockReturnValueOnce(pendingPromise);

    let unmount!: () => void;
    await act(async () => {
      ({ unmount } = render(<AppTitleBar title="NorvesEditor" />));
    });

    // Unmount before onResized resolves — cancelled flag becomes true.
    act(() => {
      unmount();
    });

    expect(unlistenSpy).not.toHaveBeenCalled();

    // Resolve the pending subscription — the .then() branch should immediately
    // call unlisten because cancelled is already true.
    await act(async () => {
      resolveOnResized(unlistenSpy);
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(unlistenSpy).toHaveBeenCalledTimes(1);
  });
});
