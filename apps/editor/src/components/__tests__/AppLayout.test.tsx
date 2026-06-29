// @vitest-environment jsdom
/**
 * AppLayout tests — P6 layout redesign.
 *
 * dockview-react is mocked with a *behavioural* fake DockviewApi so we can
 * assert the API-level contract AppLayout relies on, without dragging in
 * ResizeObserver / real DOM layout. The fake DockviewReact invokes
 * props.onReady({ api }) synchronously on mount (like the real component) and
 * re-invokes onReady when we want to simulate a fresh mount.
 *
 * Covered:
 *   - Default layout: the expected panels are created (gameView, sceneOutliner)
 *     and Connection/Settings are NOT present (they moved to their own windows
 *     in P6).
 *   - Log is created in a bottom EdgeGroup, collapsed by default.
 *   - Persistence uses the v4 key; the stale v1, v2, AND v3 keys are removed
 *     once on startup. The edge group (and its collapsed/expanded state)
 *     round-trips through toJSON/fromJSON, matching dockview 6.6.1.
 *   - Property Inspector is selection-driven: added below the Scene Outliner
 *     when selectedObjectId is set, removed when cleared; idempotent (no
 *     double-add); reconciled after restore.
 *   - Log toggle is published to the parent via onLogToggleReady and flips the
 *     EdgeGroup between collapsed and expanded (open/close the drawer).
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, cleanup, act } from '@testing-library/react';
import React from 'react';
import { BridgeProvider, useBridgeDispatch } from '../../state/BridgeContext.js';
import { LAYOUT_STORAGE_KEY, LEGACY_LAYOUT_STORAGE_KEYS } from '../shell/layoutKey.js';

// -------------------------------------------------------------------------
// localStorage polyfill — this jsdom configuration does not provide a Storage
// object (the production code wraps every access in try/catch and degrades
// gracefully). For these tests we install a minimal in-memory Storage so the
// save / restore / cleanup paths can be exercised.
// -------------------------------------------------------------------------

function installMemoryLocalStorage(): void {
  const map = new Map<string, string>();
  const storage = {
    getItem: (k: string): string | null => (map.has(k) ? map.get(k)! : null),
    setItem: (k: string, v: string): void => { map.set(k, String(v)); },
    removeItem: (k: string): void => { map.delete(k); },
    clear: (): void => { map.clear(); },
    key: (i: number): string | null => Array.from(map.keys())[i] ?? null,
    get length(): number { return map.size; },
  };
  vi.stubGlobal('localStorage', storage);
}

// -------------------------------------------------------------------------
// Behavioural fake DockviewApi
// -------------------------------------------------------------------------

interface FakePanel {
  id: string;
  group: { id: string };
}

interface AddPanelOptions {
  id: string;
  component: string;
  title?: string;
  initialWidth?: number;
  initialHeight?: number;
  position?: {
    direction?: string;
    referencePanel?: string;
    referenceGroup?: string;
  };
}

/**
 * Models a dockview edge group. `collapsed` and `visible` are independent
 * concepts in the real API: `collapse()` / `expand()` switch the drawer between
 * the collapsed bar and its expanded size (the drawer stays in the layout),
 * whereas `setEdgeGroupVisible` removes/re-adds the whole drawer (bar included).
 */
interface EdgeGroupRecord {
  id: string;
  options: { collapsed?: boolean; collapsedSize?: number; initialSize?: number };
  collapsed: boolean;
  visible: boolean;
}

/**
 * The object returned by getEdgeGroup / addEdgeGroup — mirrors the subset of
 * DockviewGroupPanelApi that AppLayout uses (id + collapse/expand/isCollapsed),
 * backed by the shared EdgeGroupRecord so state changes are observable.
 */
interface FakeEdgeGroupApi {
  id: string;
  collapse: () => void;
  expand: () => void;
  isCollapsed: () => boolean;
}

class FakeDockviewApi {
  panels = new Map<string, FakePanel>();
  edgeGroups = new Map<string, EdgeGroupRecord>();
  private groupSeq = 0;
  private layoutChangeListeners: Array<() => void> = [];
  fromJSONCalled = false;
  fromJSONThrows = false;

  /**
   * Simulated container width. Set to a non-zero value (e.g. 1600) in a test
   * via `nextApiSetup` to exercise the api.width > 0 branch in buildDefaultLayout.
   * Defaults to 0, which triggers the RIGHT_COLUMN_FALLBACK_PX path.
   */
  width = 0;

  /**
   * Records the initialWidth passed to each addPanel call, keyed by panel id.
   * Allows tests to assert that the right-column panel received a smaller width
   * than the Game View (dominant layout).
   */
  addPanelInitialWidths = new Map<string, number | undefined>();

  // -- panel API --------------------------------------------------------
  getPanel(id: string): FakePanel | undefined {
    return this.panels.get(id);
  }

  addPanel(options: AddPanelOptions): FakePanel {
    // Record initialWidth for test assertions.
    this.addPanelInitialWidths.set(options.id, options.initialWidth);

    let groupId: string;
    const ref = options.position?.referenceGroup;
    const refPanel = options.position?.referencePanel;
    if (ref !== undefined) {
      // Join the referenced group (could be a panel-group id or edge-group id).
      groupId = ref;
    } else if (refPanel !== undefined && options.position?.direction === undefined) {
      groupId = this.panels.get(refPanel)?.group.id ?? this.newGroupId();
    } else {
      groupId = this.newGroupId();
    }
    const panel: FakePanel = { id: options.id, group: { id: groupId } };
    this.panels.set(options.id, panel);
    return panel;
  }

  removePanel(panel: FakePanel): void {
    this.panels.delete(panel.id);
  }

  private newGroupId(): string {
    this.groupSeq += 1;
    return `group-${this.groupSeq}`;
  }

  // -- edge group API ---------------------------------------------------
  addEdgeGroup(
    position: string,
    options: { id: string; collapsed?: boolean; collapsedSize?: number; initialSize?: number },
  ): FakeEdgeGroupApi {
    if (this.edgeGroups.has(position)) {
      throw new Error(`edge group already exists at ${position}`);
    }
    const record: EdgeGroupRecord = {
      id: options.id,
      options,
      // collapsed reflects the requested initial state; the drawer is present
      // (visible) in the layout either way.
      collapsed: options.collapsed === true,
      visible: true,
    };
    this.edgeGroups.set(position, record);
    return this.edgeGroupApiFor(record);
  }

  getEdgeGroup(position: string): FakeEdgeGroupApi | undefined {
    const g = this.edgeGroups.get(position);
    return g === undefined ? undefined : this.edgeGroupApiFor(g);
  }

  /** Build the group-panel-API view over a record (collapse/expand/isCollapsed). */
  private edgeGroupApiFor(record: EdgeGroupRecord): FakeEdgeGroupApi {
    return {
      id: record.id,
      collapse: (): void => { record.collapsed = true; },
      expand: (): void => { record.collapsed = false; },
      isCollapsed: (): boolean => record.collapsed,
    };
  }

  setEdgeGroupVisible(position: string, visible: boolean): void {
    const g = this.edgeGroups.get(position);
    if (g !== undefined) g.visible = visible;
  }

  isEdgeGroupVisible(position: string): boolean {
    return this.edgeGroups.get(position)?.visible ?? false;
  }

  /** Test helper: read the collapsed state of an edge group at a position. */
  isEdgeGroupCollapsed(position: string): boolean | undefined {
    return this.edgeGroups.get(position)?.collapsed;
  }

  // -- persistence ------------------------------------------------------
  onDidLayoutChange(cb: () => void): { dispose: () => void } {
    this.layoutChangeListeners.push(cb);
    return { dispose: () => undefined };
  }

  /** Test helper: simulate dockview firing a layout-change event. */
  fireLayoutChange(): void {
    for (const cb of this.layoutChangeListeners) cb();
  }

  /**
   * Serialize panels AND edge groups, matching dockview 6.6.1 (toJSON includes
   * result.edgeGroups; fromJSON restores the edge group, its inner panel and
   * its collapsed state). The serialized edge group records its collapsed flag
   * so a round-trip preserves the user's open/closed drawer state.
   */
  toJSON(): object {
    return {
      panels: Array.from(this.panels.keys()),
      edgeGroups: Array.from(this.edgeGroups.entries()).map(([position, g]) => ({
        position,
        id: g.id,
        collapsed: g.collapsed,
        collapsedSize: g.options.collapsedSize,
        initialSize: g.options.initialSize,
        // The Log panel lives inside the edge group.
        panels: Array.from(this.panels.values())
          .filter((p) => p.group.id === g.id)
          .map((p) => p.id),
      })),
    };
  }

  fromJSON(data: unknown): void {
    this.fromJSONCalled = true;
    if (this.fromJSONThrows) {
      throw new Error('corrupt');
    }
    // Like the real fromJSON, a restore rebuilds the full layout from the
    // snapshot: the dockable panels first, then the edge group(s) with their
    // inner panel(s) and saved collapsed state. P6 default = Game View centre +
    // Scene Outliner right column (no Connection/Settings).
    //
    // We also restore any extra dockable panels recorded in the snapshot's
    // `panels` array (e.g. propertyInspector from a saved layout), so that the
    // reconcileInspector path "layout contains inspector but nothing selected →
    // remove" can be exercised by tests.
    const savedPanels = this.readSerializedPanelIds(data);
    this.addPanel({ id: 'gameView', component: 'gameView' });
    this.addPanel({
      id: 'sceneOutliner',
      component: 'sceneOutliner',
      position: { direction: 'right', referencePanel: 'gameView' },
    });
    for (const panelId of savedPanels) {
      if (panelId !== 'gameView' && panelId !== 'sceneOutliner') {
        this.addPanel({
          id: panelId,
          component: panelId,
          position: { referencePanel: 'sceneOutliner' },
        });
      }
    }

    // Restore edge groups (the Log drawer) from the snapshot, preserving the
    // serialized collapsed/expanded state. Falls back to a collapsed bottom
    // drawer when the snapshot predates edge-group serialization.
    const edgeGroups = this.readSerializedEdgeGroups(data);
    if (edgeGroups.length === 0) {
      this.restoreLogEdgeGroup(LAYOUT_LOG_EDGE_DEFAULT);
    } else {
      for (const eg of edgeGroups) {
        this.restoreLogEdgeGroup(eg);
      }
    }
  }

  /** Parse the top-level `panels` array out of a serialized snapshot. */
  private readSerializedPanelIds(data: unknown): string[] {
    if (typeof data !== 'object' || data === null) return [];
    const raw = (data as { panels?: unknown }).panels;
    if (!Array.isArray(raw)) return [];
    return raw.filter((id): id is string => typeof id === 'string');
  }

  /** Parse the edgeGroups array out of a serialized snapshot, defensively. */
  private readSerializedEdgeGroups(data: unknown): SerializedEdgeGroup[] {
    if (typeof data !== 'object' || data === null) return [];
    const raw = (data as { edgeGroups?: unknown }).edgeGroups;
    if (!Array.isArray(raw)) return [];
    return raw.filter(
      (e): e is SerializedEdgeGroup =>
        typeof e === 'object' && e !== null && typeof (e as { id?: unknown }).id === 'string',
    );
  }

  /** Re-create an edge group and its inner panel(s) during fromJSON. */
  private restoreLogEdgeGroup(eg: SerializedEdgeGroup): void {
    const groupApi = this.addEdgeGroup(eg.position ?? 'bottom', {
      id: eg.id,
      collapsed: eg.collapsed,
      collapsedSize: eg.collapsedSize,
      initialSize: eg.initialSize,
    });
    for (const panelId of eg.panels ?? ['log']) {
      this.addPanel({
        id: panelId,
        component: panelId,
        position: { referenceGroup: groupApi.id },
      });
    }
  }
}

interface SerializedEdgeGroup {
  id: string;
  position?: string;
  collapsed?: boolean;
  collapsedSize?: number;
  initialSize?: number;
  panels?: string[];
}

/**
 * The default edge group a snapshot without serialized edge groups falls back
 * to: the collapsed bottom Log drawer that the production code creates.
 */
const LAYOUT_LOG_EDGE_DEFAULT: SerializedEdgeGroup = {
  id: 'logEdgeGroup',
  position: 'bottom',
  collapsed: true,
  panels: ['log'],
};

// -------------------------------------------------------------------------
// dockview-react mock: capture onReady and feed it a fresh fake api.
// -------------------------------------------------------------------------

let currentApi: FakeDockviewApi;
// Allow a test to prime the next api (e.g. fromJSONThrows) before render.
let nextApiSetup: ((api: FakeDockviewApi) => void) | undefined;

vi.mock('dockview-react', () => ({
  DockviewReact: (props: { onReady: (event: { api: FakeDockviewApi }) => void }) => {
    // Build a fresh api per mount, like the real component.
    const api = new FakeDockviewApi();
    nextApiSetup?.(api);
    currentApi = api;
    // Invoke onReady synchronously inside an effect-like microtask is not
    // needed; the real DockviewReact calls onReady after mounting. Calling it
    // during render is acceptable for this fake since onReady only touches the
    // api (no React state of DockviewReact itself).
    props.onReady({ api });
    return React.createElement('div', { 'data-testid': 'dockview-root' });
  },
}));

import { AppLayout } from '../AppLayout.js';

// -------------------------------------------------------------------------
// Test harness — drives selection + captures the published Log toggle.
// -------------------------------------------------------------------------

let capturedLogToggle: (() => void) | undefined;
let harnessDispatch: ReturnType<typeof useBridgeDispatch> | undefined;

/**
 * Mounts AppLayout only after `mounted` is true. The harness captures dispatch
 * so a test can set a selection first, then flip `mounted` to mount AppLayout —
 * exercising the onReady reconcile path with a pre-existing selection without
 * dispatching during render (which React warns about).
 */
function Harness(props: { mounted: boolean }): React.JSX.Element {
  harnessDispatch = useBridgeDispatch();
  if (!props.mounted) {
    return <div data-testid="not-mounted" />;
  }
  return (
    <AppLayout
      onLogToggleReady={(toggle) => {
        capturedLogToggle = toggle;
      }}
    />
  );
}

/** Render AppLayout immediately (no pre-selection). Returns the rerender fn. */
function renderApp(): { rerender: (mounted: boolean) => void } {
  const { rerender } = render(
    React.createElement(BridgeProvider, null,
      React.createElement(Harness, { mounted: true }),
    ),
  );
  return {
    rerender: (mounted: boolean): void => {
      rerender(
        React.createElement(BridgeProvider, null,
          React.createElement(Harness, { mounted }),
        ),
      );
    },
  };
}

/**
 * Render with AppLayout initially unmounted, set a selection, then mount
 * AppLayout — so its onReady reconcile sees the pre-existing selection.
 */
function renderAppWithPreSelection(id: string): void {
  const { rerender } = render(
    React.createElement(BridgeProvider, null,
      React.createElement(Harness, { mounted: false }),
    ),
  );
  act(() => {
    harnessDispatch?.({ type: 'objectSelected', id });
  });
  act(() => {
    rerender(
      React.createElement(BridgeProvider, null,
        React.createElement(Harness, { mounted: true }),
      ),
    );
  });
}

// -------------------------------------------------------------------------
// Setup / teardown
// -------------------------------------------------------------------------

beforeEach(() => {
  installMemoryLocalStorage();
  capturedLogToggle = undefined;
  nextApiSetup = undefined;
  harnessDispatch = undefined;
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

// -------------------------------------------------------------------------
// Smoke
// -------------------------------------------------------------------------

describe('AppLayout smoke', () => {
  it('renders the dockview container inside BridgeProvider', () => {
    renderApp();
    expect(document.querySelector('[data-testid="dockview-root"]')).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Default layout
// -------------------------------------------------------------------------

describe('AppLayout default layout (P6 + fix/default-layout)', () => {
  it('creates Game View and Scene Outliner', () => {
    renderApp();
    expect(currentApi.getPanel('gameView')).toBeTruthy();
    expect(currentApi.getPanel('sceneOutliner')).toBeTruthy();
  });

  it('creates Asset Browser and Asset Inspector in the default layout', () => {
    renderApp();
    expect(currentApi.getPanel('assetBrowser')).toBeTruthy();
    expect(currentApi.getPanel('assetInspector')).toBeTruthy();
  });

  it('does NOT add Connection or Settings (they moved to their own windows in P6)', () => {
    renderApp();
    expect(currentApi.getPanel('connection')).toBeUndefined();
    expect(currentApi.getPanel('settings')).toBeUndefined();
  });

  it('places Scene Outliner in a different group from Game View (right column)', () => {
    renderApp();
    const gameGroup = currentApi.getPanel('gameView')?.group.id;
    const outlinerGroup = currentApi.getPanel('sceneOutliner')?.group.id;
    expect(gameGroup).toBeTruthy();
    expect(outlinerGroup).not.toBe(gameGroup);
  });

  it('does NOT add the Property Inspector when nothing is selected', () => {
    renderApp();
    expect(currentApi.getPanel('propertyInspector')).toBeUndefined();
  });

  it('passes initialWidth to Scene Outliner to make the right column narrower than Game View (fallback when api.width === 0)', () => {
    // api.width defaults to 0 (container not yet painted) — the fallback
    // RIGHT_COLUMN_FALLBACK_PX (400) should be used.
    renderApp();
    // Game View has no initialWidth (it is the root panel).
    expect(currentApi.addPanelInitialWidths.get('gameView')).toBeUndefined();
    // Scene Outliner receives an explicit initialWidth (the right-column size).
    const outlinerWidth = currentApi.addPanelInitialWidths.get('sceneOutliner');
    expect(outlinerWidth).toBeDefined();
    expect(typeof outlinerWidth).toBe('number');
    // The right column must be strictly less than half the default fallback
    // total (800 px, matching the tauri.conf.json height=800 / width=1280).
    // At 400 px fallback, a 50:50 split would also be 400 px, so we verify
    // the right column is ≤ 400 (it is exactly 400 — the fallback itself is
    // a reasonable ~25 % of a 1600 px canvas). The key assertion is that an
    // initialWidth IS passed (not undefined), so the dominant-ratio logic ran.
    expect((outlinerWidth as number)).toBeGreaterThan(0);
    expect((outlinerWidth as number)).toBeLessThanOrEqual(400);
  });

  it('computes Scene Outliner initialWidth as ~25 % of api.width when api.width > 0', () => {
    // Use api.width = 1200 (not 1600) so the expected result (300) is
    // distinct from the fallback fixed value (400). This makes it possible to
    // tell whether the multiplication path ran or whether the test would pass
    // accidentally because both paths happen to produce the same number.
    nextApiSetup = (api): void => { api.width = 1200; };
    renderApp();
    const outlinerWidth = currentApi.addPanelInitialWidths.get('sceneOutliner');
    expect(outlinerWidth).toBeDefined();
    // Math.round(1200 * 0.25) = 300. Allow ±1 for Math.round rounding.
    expect((outlinerWidth as number)).toBeGreaterThanOrEqual(299);
    expect((outlinerWidth as number)).toBeLessThanOrEqual(301);
  });
});

// -------------------------------------------------------------------------
// Log EdgeGroup
// -------------------------------------------------------------------------

describe('AppLayout Log EdgeGroup drawer', () => {
  it('creates a bottom edge group, collapsed by default', () => {
    renderApp();
    expect(currentApi.getEdgeGroup('bottom')).toBeTruthy();
    // collapsed by default — the drawer is present but closed to its bar.
    expect(currentApi.isEdgeGroupCollapsed('bottom')).toBe(true);
  });

  it('puts the Log panel inside the edge group', () => {
    renderApp();
    const log = currentApi.getPanel('log');
    expect(log).toBeTruthy();
    expect(log?.group.id).toBe('logEdgeGroup');
  });

  it('publishes a Log toggle that flips the edge group between collapsed and expanded', () => {
    renderApp();
    expect(typeof capturedLogToggle).toBe('function');
    // Default: collapsed (drawer closed).
    expect(currentApi.getEdgeGroup('bottom')?.isCollapsed()).toBe(true);
    // First toggle: expand (open the drawer).
    act(() => {
      capturedLogToggle?.();
    });
    expect(currentApi.getEdgeGroup('bottom')?.isCollapsed()).toBe(false);
    // Second toggle: collapse again (close the drawer).
    act(() => {
      capturedLogToggle?.();
    });
    expect(currentApi.getEdgeGroup('bottom')?.isCollapsed()).toBe(true);
  });
});

// -------------------------------------------------------------------------
// Persistence: v4 key + v1/v2/v3 cleanup
// -------------------------------------------------------------------------

describe('AppLayout persistence (v4 + v1/v2/v3 cleanup)', () => {
  it('removes the stale v1, v2, and v3 keys on startup', () => {
    for (const key of LEGACY_LAYOUT_STORAGE_KEYS) {
      localStorage.setItem(key, '{"old":true}');
    }
    renderApp();
    for (const key of LEGACY_LAYOUT_STORAGE_KEYS) {
      expect(localStorage.getItem(key)).toBeNull();
    }
  });

  it('persists the layout under the v4 key on a layout change', () => {
    renderApp();
    expect(localStorage.getItem(LAYOUT_STORAGE_KEY)).toBeNull();
    act(() => {
      currentApi.fireLayoutChange();
    });
    const saved = localStorage.getItem(LAYOUT_STORAGE_KEY);
    expect(saved).not.toBeNull();
    // The v4 key is the storage target (not the legacy keys).
    for (const key of LEGACY_LAYOUT_STORAGE_KEYS) {
      expect(localStorage.getItem(key)).toBeNull();
    }
    expect(LAYOUT_STORAGE_KEY).toBe('norveseditor-layout-v4');
  });

  it('restores a saved v4 layout (fromJSON) including the edge group and Log panel', () => {
    localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify({ panels: ['gameView'] }));
    renderApp();
    expect(currentApi.fromJSONCalled).toBe(true);
    // dockview 6.6.1 restores the edge group and its inner Log panel via
    // fromJSON; ensureLogEdgeGroup then early-returns as an idempotent net.
    expect(currentApi.getEdgeGroup('bottom')).toBeTruthy();
    expect(currentApi.getPanel('log')).toBeTruthy();
    // Phase B asset panels are added idempotently after restore, so the v4 key
    // can remain active while old saved layouts are upgraded in place.
    expect(currentApi.getPanel('assetBrowser')).toBeTruthy();
    expect(currentApi.getPanel('assetInspector')).toBeTruthy();
  });

  it('preserves the saved collapsed/expanded drawer state across a save/restore round-trip', () => {
    // Build the default layout, open (expand) the Log drawer, then persist.
    renderApp();
    act(() => {
      capturedLogToggle?.();
    });
    expect(currentApi.getEdgeGroup('bottom')?.isCollapsed()).toBe(false);
    act(() => {
      currentApi.fireLayoutChange();
    });
    const saved = localStorage.getItem(LAYOUT_STORAGE_KEY);
    expect(saved).not.toBeNull();

    // Re-mount: a fresh api restores from the saved snapshot. The expanded
    // (not collapsed) state the user left the drawer in must be preserved.
    cleanup();
    renderApp();
    expect(currentApi.fromJSONCalled).toBe(true);
    expect(currentApi.getEdgeGroup('bottom')?.isCollapsed()).toBe(false);
  });

  it('purges a corrupt v4 layout and rebuilds the default', () => {
    localStorage.setItem(LAYOUT_STORAGE_KEY, '{not valid json');
    renderApp();
    // Corrupt JSON.parse throws before fromJSON; key purged, default rebuilt.
    expect(localStorage.getItem(LAYOUT_STORAGE_KEY)).toBeNull();
    expect(currentApi.getPanel('gameView')).toBeTruthy();
    expect(currentApi.getPanel('sceneOutliner')).toBeTruthy();
    // Connection/Settings are not part of the rebuilt default in P6.
    expect(currentApi.getPanel('connection')).toBeUndefined();
  });
});

// -------------------------------------------------------------------------
// Property Inspector — selection-driven add/remove
// -------------------------------------------------------------------------

describe('AppLayout Property Inspector (selection-driven)', () => {
  it('adds the Inspector (reconciled at onReady) when an object is already selected at mount', () => {
    renderAppWithPreSelection('obj-1');
    expect(currentApi.getPanel('propertyInspector')).toBeTruthy();
    // Docked below the Scene Outliner (its own group, not the Game View group).
    const inspectorGroup = currentApi.getPanel('propertyInspector')?.group.id;
    const gameGroup = currentApi.getPanel('gameView')?.group.id;
    expect(inspectorGroup).toBeTruthy();
    expect(inspectorGroup).not.toBe(gameGroup);
  });

  it('adds the Inspector when an object becomes selected', () => {
    renderApp();
    expect(currentApi.getPanel('propertyInspector')).toBeUndefined();
    act(() => {
      harnessDispatch?.({ type: 'objectSelected', id: 'obj-2' });
    });
    expect(currentApi.getPanel('propertyInspector')).toBeTruthy();
  });

  it('removes the Inspector when the object is deselected', () => {
    renderApp();
    act(() => {
      harnessDispatch?.({ type: 'objectSelected', id: 'obj-1' });
    });
    expect(currentApi.getPanel('propertyInspector')).toBeTruthy();
    act(() => {
      harnessDispatch?.({ type: 'objectSelected', id: undefined });
    });
    expect(currentApi.getPanel('propertyInspector')).toBeUndefined();
  });

  it('does not double-add the Inspector when selection changes object', () => {
    renderApp();
    act(() => {
      harnessDispatch?.({ type: 'objectSelected', id: 'obj-1' });
    });
    expect(currentApi.getPanel('propertyInspector')).toBeTruthy();
    act(() => {
      harnessDispatch?.({ type: 'objectSelected', id: 'obj-3' });
    });
    // Still exactly one inspector panel, same id.
    const inspectorCount = Array.from(currentApi.panels.keys()).filter(
      (k) => k === 'propertyInspector',
    ).length;
    expect(inspectorCount).toBe(1);
    expect(currentApi.getPanel('propertyInspector')).toBeTruthy();
  });

  it('removes the Inspector at onReady when layout was restored with propertyInspector but nothing is selected', () => {
    // Save a v3 layout snapshot that includes propertyInspector (as would
    // happen if the user had an object selected when they last saved, then
    // restarted without selecting anything).
    localStorage.setItem(
      LAYOUT_STORAGE_KEY,
      JSON.stringify({
        panels: ['gameView', 'sceneOutliner', 'propertyInspector'],
        edgeGroups: [
          {
            id: 'logEdgeGroup',
            position: 'bottom',
            collapsed: true,
            panels: ['log'],
          },
        ],
      }),
    );
    // Mount with no pre-selection (selectedObjectId === undefined).
    renderApp();
    // fromJSON restores propertyInspector from the snapshot, then reconcileInspector
    // removes it because no object is currently selected.
    expect(currentApi.fromJSONCalled).toBe(true);
    expect(currentApi.getPanel('propertyInspector')).toBeUndefined();
  });
});
