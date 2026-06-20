// @vitest-environment jsdom
/**
 * App routing tests — the query-parameter window router.
 *
 * App() resolves the route via resolveWindowRoute(window.location.search) and
 * renders one of three roots:
 *   - 'main' → the full shell (toolbar + AppLayout).
 *   - 'connection' / 'settings' → the minimal SecondaryWindowRoot.
 *
 * Rather than mutate jsdom's read-only window.location, we mock the windowRoute
 * module so resolveWindowRoute returns a fixed route per test. This isolates
 * App()'s branch wiring (the unit under test here); the raw query-parameter
 * parsing is covered separately by windowRoute.test.ts. dockview-react and the
 * Tauri modules are mocked so neither a real layout nor a Tauri runtime is run.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
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

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(() => Promise.resolve(null)),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => undefined)),
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

// App imports windowManager (value import), which imports webviewWindow at the
// module top level; mock it so loading App does not pull in the real Tauri IPC
// module. The toolbar-open path is not exercised here (covered by
// windowManager.test.ts), so a no-op class is sufficient.
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  WebviewWindow: class {
    static getByLabel = vi.fn(() => Promise.resolve(null));
    once = vi.fn(() => Promise.resolve(() => undefined));
  },
}));

// Stub dockview-react so the main-route AppLayout mounts without a real layout.
// onReady is fired from a useEffect (NOT during render) to match the real
// component — calling it during render would re-enter setState in BridgeRoot's
// onLogToggleReady and loop forever.
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
      // Fire once on mount.
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);
    return React.createElement('div', { 'data-testid': 'dockview-root' });
  },
}));

import App from '../App.js';

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

/** Minimal in-memory localStorage so AppLayout's persistence paths don't throw. */
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

async function renderApp(route: WindowRoute): Promise<void> {
  routeMock.current = route;
  await act(async () => {
    render(React.createElement(App));
    await Promise.resolve();
  });
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

describe('App window routing', () => {
  it('renders the main shell (toolbar + dockview) for the main route', async () => {
    await renderApp('main');
    expect(document.querySelector('[role="toolbar"]')).toBeTruthy();
    expect(document.querySelector('[data-testid="dockview-root"]')).toBeTruthy();
  });

  it('renders the Connection secondary window for the connection route', async () => {
    await renderApp('connection');
    // Secondary shell: no toolbar, no dockview, the Connection panel only.
    expect(document.querySelector('[role="toolbar"]')).toBeNull();
    expect(document.querySelector('[data-testid="dockview-root"]')).toBeNull();
    expect(document.body.textContent).toContain('Bridge port');
  });

  it('renders the Settings secondary window for the settings route', async () => {
    await renderApp('settings');
    expect(document.querySelector('[role="toolbar"]')).toBeNull();
    expect(document.querySelector('[data-testid="dockview-root"]')).toBeNull();
    // Stable marker: Settings panel body text (not the reset button, which may move in P6).
    expect(document.body.textContent).toContain('Editor settings are not yet available');
  });
});
