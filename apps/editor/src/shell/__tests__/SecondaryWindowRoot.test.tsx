// @vitest-environment jsdom
/**
 * SecondaryWindowRoot tests — routed panel rendering + per-window subscription.
 *
 * Each secondary window is its own React tree that subscribes to the bridge
 * events exactly once. These tests verify:
 *   (a) target='connection' renders the Connection panel.
 *   (b) target='settings' renders the Settings panel.
 *   (c) useBridgeSubscriptions registers exactly one subscription set per
 *       window (listen called once per bridge event), matching the main-window
 *       invariant on a per-window basis.
 *
 * Tauri modules are mocked: event.listen (subscriptions), window.getCurrentWindow
 * (AppTitleBar), core.invoke (panel actions). No real Tauri runtime is used.
 */

import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import { render, cleanup, act } from '@testing-library/react';

// -------------------------------------------------------------------------
// Tauri mocks (hoisted before module imports)
// -------------------------------------------------------------------------

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(() => Promise.resolve(null)),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => undefined)),
  emit: vi.fn(() => Promise.resolve()),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(() => ({
    onResized: vi.fn(() => Promise.resolve(() => undefined)),
    isMaximized: vi.fn(() => Promise.resolve(false)),
    minimize: vi.fn(() => Promise.resolve()),
    toggleMaximize: vi.fn(() => Promise.resolve()),
    close: vi.fn(() => Promise.resolve()),
  })),
}));

import * as tauriEvent from '@tauri-apps/api/event';
import { BridgeProvider } from '../../state/BridgeContext.js';
import { SecondaryWindowRoot } from '../SecondaryWindowRoot.js';
import React from 'react';

/** Subscription count must equal the bridge-event set (same as the main window). */
const EXPECTED_SUBSCRIPTION_COUNT = 11;

function renderRoute(target: 'connection' | 'settings'): void {
  render(
    React.createElement(BridgeProvider, null,
      React.createElement(SecondaryWindowRoot, { target }),
    ),
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
});

// -------------------------------------------------------------------------
// (a) / (b) routed panel rendering
// -------------------------------------------------------------------------

describe('SecondaryWindowRoot routed panel', () => {
  it("renders the Connection panel for target='connection'", async () => {
    await act(async () => {
      renderRoute('connection');
      await Promise.resolve();
    });
    // The Connection panel has a "Bridge port" label unique to it.
    expect(document.body.textContent).toContain('Bridge port');
    // Confirm the Settings panel is not rendered.
    expect(document.body.textContent).not.toContain('Workspace root');
  });

  it("renders the Settings panel for target='settings'", async () => {
    await act(async () => {
      renderRoute('settings');
      await Promise.resolve();
    });
    // The Settings panel body text is unique to it (stable across layout changes).
    expect(document.body.textContent).toContain('Workspace root');
    // Confirm the Connection panel is not rendered.
    expect(document.body.textContent).not.toContain('Bridge port');
  });
});

// -------------------------------------------------------------------------
// (c) per-window subscription count
// -------------------------------------------------------------------------

describe('SecondaryWindowRoot subscription', () => {
  it('registers exactly one bridge subscription set for the window', async () => {
    await act(async () => {
      renderRoute('connection');
      // Let the async subscribeEvent Promises settle.
      await Promise.resolve();
    });

    expect((tauriEvent.listen as Mock).mock.calls).toHaveLength(EXPECTED_SUBSCRIPTION_COUNT);
  });
});
