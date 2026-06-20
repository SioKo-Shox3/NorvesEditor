// @vitest-environment jsdom
/**
 * App layout-reset wiring tests (P6) — main-window listener lifecycle.
 *
 * The main window (BridgeRoot) registers a single layout-reset listener via
 * subscribeLayoutReset and tears it down (unlisten) on unmount. These tests
 * mock layoutReset so we can assert the subscribe/unlisten lifecycle without a
 * Tauri runtime. The secondary windows must NOT register this listener (only
 * the main window owns the layout), which we assert too.
 *
 * The Tauri modules and dockview are mocked the same way as App.routing.test so
 * App() mounts cleanly in jsdom.
 */

import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import { render, cleanup, act } from '@testing-library/react';
import React from 'react';
import type { WindowRoute } from '../shell/windowRoute.js';

// -------------------------------------------------------------------------
// Mocks (hoisted)
// -------------------------------------------------------------------------

const routeMock = vi.hoisted(() => ({ current: 'main' as WindowRoute }));

vi.mock('../shell/windowRoute.js', () => ({
  WINDOW_ROUTE_PARAM: 'window',
  resolveWindowRoute: (): WindowRoute => routeMock.current,
}));

// Spy on the layout-reset relay. subscribeLayoutReset resolves with an unlisten
// spy we can assert ran on unmount.
const unlistenSpy = vi.fn();
vi.mock('../shell/layoutReset.js', () => ({
  clearSavedLayoutAndReload: vi.fn(),
  requestLayoutReset: vi.fn(() => Promise.resolve()),
  subscribeLayoutReset: vi.fn(() => Promise.resolve(unlistenSpy)),
}));

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

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  WebviewWindow: class {
    static getByLabel = vi.fn(() => Promise.resolve(null));
    once = vi.fn(() => Promise.resolve(() => undefined));
  },
}));

vi.mock('dockview-react', () => ({
  DockviewReact: (props: { onReady: (event: { api: unknown }) => void }) => {
    React.useEffect(() => {
      const api = {
        getPanel: () => undefined,
        addPanel: () => ({ id: '', group: { id: '' } }),
        removePanel: () => undefined,
        getEdgeGroup: () => undefined,
        addEdgeGroup: () => ({
          id: 'logEdgeGroup',
          collapse: () => undefined,
          expand: () => undefined,
          isCollapsed: () => true,
        }),
        setEdgeGroupVisible: () => undefined,
        isEdgeGroupVisible: () => false,
        onDidLayoutChange: () => ({ dispose: () => undefined }),
        toJSON: () => ({}),
        fromJSON: () => undefined,
      };
      props.onReady({ api });
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);
    return React.createElement('div', { 'data-testid': 'dockview-root' });
  },
}));

import App from '../App.js';
import { subscribeLayoutReset } from '../shell/layoutReset.js';

// -------------------------------------------------------------------------
// Setup
// -------------------------------------------------------------------------

beforeEach(() => {
  vi.clearAllMocks();
  routeMock.current = 'main';
  installMemoryLocalStorage();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

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

async function renderApp(route: WindowRoute): Promise<{ unmount: () => void }> {
  routeMock.current = route;
  let result!: { unmount: () => void };
  await act(async () => {
    result = render(React.createElement(App));
    await Promise.resolve();
  });
  return result;
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

describe('App layout-reset listener (main window)', () => {
  it('subscribes to layout-reset on the main route', async () => {
    await renderApp('main');
    expect(subscribeLayoutReset as Mock).toHaveBeenCalledOnce();
  });

  it('unlistens the layout-reset subscription on unmount', async () => {
    const { unmount } = await renderApp('main');
    expect(unlistenSpy).not.toHaveBeenCalled();
    await act(async () => {
      unmount();
      // Let the async subscribe Promise (and the cleanup) settle.
      await Promise.resolve();
    });
    expect(unlistenSpy).toHaveBeenCalledOnce();
  });

  it('does NOT subscribe to layout-reset on a secondary route', async () => {
    await renderApp('settings');
    expect(subscribeLayoutReset as Mock).not.toHaveBeenCalled();
  });
});
