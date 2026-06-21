/**
 * AppLayout — dockview-based layout container for NorvesEditor.
 *
 * P6 layout redesign (updated in fix/default-layout: right column narrowed to
 * ~25 %). New default arrangement:
 *
 *   +------------------------------------------+----------------+
 *   |                                           | Scene Outliner |
 *   |                                           | (~25% width,   |
 *   |          Game View                        | right column)  |
 *   |          (~75% width, centre / largest)   +----------------+
 *   |                                           | [ Inspector ]  |
 *   |                                           | (below Scene   |
 *   |                                           |  Outliner, on  |
 *   |                                           |  selection)    |
 *   +------------------------------------------+----------------+
 *   |  Log (bottom EdgeGroup drawer, collapsed by default)       |
 *   +------------------------------------------------------------+
 *
 *   - Game View is the centre/root panel (~75 % of the width).
 *   - Scene Outliner sits at the top of the right column (~25 %).
 *   - The Property Inspector (when an object is selected) is added BELOW the
 *     Scene Outliner — it no longer depends on a Connection/Settings group.
 *   - Log lives in a bottom EdgeGroup drawer, collapsed by default; the main
 *     toolbar's "Log" toggle (P3) expands/collapses it.
 *
 * Connection and Settings are NO LONGER in the main window's dockview (P6):
 * they open in their own Tauri windows from the toolbar (see windowManager /
 * SecondaryWindowRoot). SecondaryWindowRoot imports those panels directly, so
 * the panel components stay; only the main-window dockview entries are removed.
 *
 * Property Inspector visibility:
 *   - Driven solely by selectedObjectId (engine-agnostic — no mock-specific
 *     names). When an object is selected the Inspector is added below the Scene
 *     Outliner; when deselected it is removed.
 *   - Idempotent: never double-adds (existence checked via getPanel).
 *   - Reconciled after restore so the restored layout matches the current
 *     selection (Inspector shown iff an object is selected).
 *
 * Layout persistence:
 *   - Saved to localStorage under LAYOUT_STORAGE_KEY (v4) on every change.
 *   - Restored on startup (fromJSON). Corrupt JSON purges the key and rebuilds.
 *   - On startup any stale legacy keys (v1, v2, v3) are removed once (cleanup).
 *
 * Layout reset is exposed by the toolbar (clearSavedLayoutAndReload) and by the
 * Settings window (requestLayoutReset, relayed to the main window); see
 * shell/layoutReset.ts. Floating groups are disabled (disableFloatingGroups)
 * for alpha (tear-off is a future phase).
 */

import { useCallback, useEffect, useRef } from 'react';
import type React from 'react';
import { DockviewReact } from 'dockview-react';
import type {
  DockviewApi,
  DockviewReadyEvent,
  IDockviewPanelProps,
  EdgeGroupPosition,
} from 'dockview-react';
import { GameViewPanel }          from './GameViewPanel.js';
import { LogPanel }               from './LogPanel.js';
import { SceneOutlinerPanel }     from './SceneOutlinerPanel.js';
import { PropertyInspectorPanel } from './PropertyInspectorPanel.js';
import { LAYOUT_STORAGE_KEY, LEGACY_LAYOUT_STORAGE_KEYS } from './shell/layoutKey.js';
import { useBridgeState } from '../state/BridgeContext.js';

// -------------------------------------------------------------------------
// Constants
// -------------------------------------------------------------------------

/** Panel ids — shared between the default builder and dynamic reconciliation. */
const PANEL_GAME_VIEW      = 'gameView';
const PANEL_SCENE_OUTLINER = 'sceneOutliner';
const PANEL_INSPECTOR      = 'propertyInspector';
const PANEL_LOG            = 'log';

/** Bottom EdgeGroup (Log drawer) position and options. */
const LOG_EDGE_POSITION: EdgeGroupPosition = 'bottom';
const LOG_EDGE_GROUP_ID = 'logEdgeGroup';
/** Height (px) of the Log drawer when expanded. */
const LOG_EDGE_INITIAL_SIZE = 180;
/** Height (px) of the collapsed Log bar. */
const LOG_EDGE_COLLAPSED_SIZE = 28;

// -------------------------------------------------------------------------
// Panel component map
// -------------------------------------------------------------------------

// Connection / Settings are deliberately absent: P6 moves them to their own
// Tauri windows (SecondaryWindowRoot imports those panels directly), so the
// main-window dockview no longer hosts them.
const PANEL_COMPONENTS: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  [PANEL_GAME_VIEW]:      GameViewPanel,
  [PANEL_LOG]:            LogPanel,
  [PANEL_SCENE_OUTLINER]: SceneOutlinerPanel,
  [PANEL_INSPECTOR]:      PropertyInspectorPanel,
};

// -------------------------------------------------------------------------
// Default layout builder
// -------------------------------------------------------------------------

/**
 * Target right-column width as a fraction of the total dockview width.
 * Game View gets the remainder (~75 %).
 */
const RIGHT_COLUMN_RATIO = 0.25;

/**
 * Fallback right-column width in pixels, used when api.width is 0 at onReady
 * (e.g. the container has not been painted yet). 400 px is a comfortable Scene
 * Outliner width on typical HD+ displays and keeps the ratio close to 25 % on
 * a 1600 px canvas.
 */
const RIGHT_COLUMN_FALLBACK_PX = 400;

/**
 * Build the default layout. Game View is the root (centre/largest, ~75 % of
 * the width); the right column holds the Scene Outliner on top (the Property
 * Inspector is added below it on demand); the Log lives in a collapsed bottom
 * EdgeGroup drawer.
 *
 * The right-column width is set via addPanel's `initialWidth` option, which
 * dockview 6.6.1 passes directly as the `size` argument to the underlying
 * gridview split — this is the canonical way to control the initial split ratio
 * (AddPanelOptions.initialWidth: number, see dockview-core/dist/cjs/dockview/
 * options.d.ts and dockviewComponent.js). The value is also captured in the
 * toJSON snapshot so the persisted layout retains the ratio.
 *
 * Connection and Settings are NOT added here (P6: they open in their own Tauri
 * windows). The Property Inspector is NOT added here either — it is added on
 * demand by the selection-driven reconciliation (reconcileInspector).
 */
function buildDefaultLayout(api: DockviewApi): void {
  // 1. Centre: Game View (first panel — becomes the root, largest area).
  api.addPanel({
    id: PANEL_GAME_VIEW,
    component: PANEL_GAME_VIEW,
    title: 'Game View',
  });

  // 2. Right column top: Scene Outliner (to the right of Game View).
  //    initialWidth sets the new group's width in the gridview split.
  //    api.width is the current rendered width of the dockview container; if it
  //    is 0 (not yet painted) we fall back to RIGHT_COLUMN_FALLBACK_PX.
  //    This is also the anchor the Property Inspector docks below (reconcileInspector).
  const totalWidth = api.width;
  const rightColumnWidth =
    totalWidth > 0
      ? Math.round(totalWidth * RIGHT_COLUMN_RATIO)
      : RIGHT_COLUMN_FALLBACK_PX;
  api.addPanel({
    id: PANEL_SCENE_OUTLINER,
    component: PANEL_SCENE_OUTLINER,
    title: 'Scene Outliner',
    position: {
      direction: 'right',
      referencePanel: PANEL_GAME_VIEW,
    },
    initialWidth: rightColumnWidth,
  });

  // 3. Log: bottom EdgeGroup drawer, collapsed by default.
  ensureLogEdgeGroup(api);
}

// -------------------------------------------------------------------------
// Log EdgeGroup drawer
// -------------------------------------------------------------------------

/**
 * Ensure the Log lives in a collapsed bottom EdgeGroup drawer.
 *
 * dockview 6.6.1 serializes the edge group and its inner Log panel in
 * toJSON, and restores them (including the collapsed/expanded state) in
 * fromJSON. So after a restore the edge group is normally already present.
 * This helper is an idempotent safety net: it early-returns when the edge
 * group exists (via getEdgeGroup), and only creates the collapsed drawer on
 * the first run (default-layout build) or in the unlikely case the restored
 * layout was missing its edge group.
 */
function ensureLogEdgeGroup(api: DockviewApi): void {
  if (api.getEdgeGroup(LOG_EDGE_POSITION) !== undefined) {
    return;
  }
  const groupApi = api.addEdgeGroup(LOG_EDGE_POSITION, {
    id: LOG_EDGE_GROUP_ID,
    collapsed: true,
    collapsedSize: LOG_EDGE_COLLAPSED_SIZE,
    initialSize: LOG_EDGE_INITIAL_SIZE,
  });
  // Add the Log panel into the freshly created edge group.
  api.addPanel({
    id: PANEL_LOG,
    component: PANEL_LOG,
    title: 'Log',
    position: {
      referenceGroup: groupApi.id,
    },
  });
}

/**
 * Toggle the Log drawer between collapsed and expanded (used by the toolbar).
 *
 * The drawer stays in the layout either way: collapsing shrinks it to the
 * LOG_EDGE_COLLAPSED_SIZE bar, expanding grows it to LOG_EDGE_INITIAL_SIZE.
 * (setEdgeGroupVisible would remove the whole drawer, including the bar — a
 * different concept; the toolbar's "Log" button means open/close, i.e.
 * expand/collapse.)
 */
function toggleLog(api: DockviewApi): void {
  const group = api.getEdgeGroup(LOG_EDGE_POSITION);
  if (group === undefined) {
    return;
  }
  if (group.isCollapsed()) {
    group.expand();
  } else {
    group.collapse();
  }
}

// -------------------------------------------------------------------------
// Property Inspector reconciliation (selection-driven add/remove)
// -------------------------------------------------------------------------

/**
 * Reconcile the Property Inspector panel with the current selection.
 *
 *   selectedObjectId set + no Inspector panel → add it below the Scene Outliner.
 *   selectedObjectId unset + Inspector present → remove it.
 *
 * The Inspector docks BELOW the Scene Outliner (P6: it no longer depends on a
 * Connection/Settings group, which was removed). If the Scene Outliner is
 * somehow absent, dockview places the panel in a default location rather than
 * throwing.
 *
 * Idempotent: the existence check (getPanel) prevents double-add, and removal
 * only runs when the panel exists. Safe to call on selection change AND right
 * after restore (so a restored layout matches the live selection).
 */
function reconcileInspector(api: DockviewApi, hasSelection: boolean): void {
  const existing = api.getPanel(PANEL_INSPECTOR);

  if (hasSelection) {
    if (existing !== undefined) {
      return; // already present — do not double-add
    }
    const outlinerPresent = api.getPanel(PANEL_SCENE_OUTLINER) !== undefined;
    api.addPanel({
      id: PANEL_INSPECTOR,
      component: PANEL_INSPECTOR,
      title: 'Property Inspector',
      // Dock below the Scene Outliner; if it is gone, dockview places the panel
      // in a default location rather than throwing.
      ...(outlinerPresent
        ? { position: { direction: 'below', referencePanel: PANEL_SCENE_OUTLINER } }
        : {}),
    });
    return;
  }

  // No selection — remove the Inspector if it is present.
  if (existing !== undefined) {
    api.removePanel(existing);
  }
}

// -------------------------------------------------------------------------
// Persistence helpers
// -------------------------------------------------------------------------

function saveLayout(api: DockviewApi): void {
  try {
    const json = JSON.stringify(api.toJSON());
    localStorage.setItem(LAYOUT_STORAGE_KEY, json);
  } catch {
    // Serialization or storage failure — ignore silently.
  }
}

/**
 * Remove the stale legacy layout keys (v1, v2, v3) once on startup so old
 * garbage does not linger in localStorage. fix/default-layout bumped the
 * active key to v4 because the default panel ratio changed (~75:25 instead of
 * 50:50); a saved v3 layout encodes the old split ratio in its gridview
 * snapshot and must not survive. The array in LEGACY_LAYOUT_STORAGE_KEYS is
 * the single source of truth — add v4 there when a future bump makes v5 active.
 * NOTE: the active key (currently v4) must NOT be added to that legacy array;
 * it is the current persistence target and must be excluded from startup purge
 * so that persistence and cleanup remain disjoint (no risk of purging a key
 * that is still being written).
 * Never re-written. Failures are ignored.
 */
function cleanupLegacyLayout(): void {
  for (const key of LEGACY_LAYOUT_STORAGE_KEYS) {
    try {
      localStorage.removeItem(key);
    } catch {
      // localStorage may be unavailable — ignore silently.
    }
  }
}

/**
 * Try to restore a saved layout (v4 key only).
 * Returns true on success, false if there was no saved layout or it was corrupt.
 * On corruption the storage key is purged so the next load rebuilds the default.
 */
function tryRestoreLayout(api: DockviewApi): boolean {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(LAYOUT_STORAGE_KEY);
  } catch {
    return false;
  }
  if (raw === null) return false;

  try {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
    api.fromJSON(JSON.parse(raw));
    return true;
  } catch {
    // Corrupt JSON — purge the key to prevent repeated failures on reload.
    try {
      localStorage.removeItem(LAYOUT_STORAGE_KEY);
    } catch {
      // Ignore storage errors.
    }
    return false;
  }
}

// -------------------------------------------------------------------------
// AppLayout component
// -------------------------------------------------------------------------

export interface AppLayoutProps {
  /**
   * Receives a callback that toggles the Log drawer once the dockview API is
   * ready (passed up to App.tsx so the toolbar's Log button can drive it).
   * Cleared (undefined) on unmount.
   */
  onLogToggleReady?: (toggle: (() => void) | undefined) => void;
}

export function AppLayout({ onLogToggleReady }: AppLayoutProps = {}): React.JSX.Element {
  // Keep a ref to the DockviewApi so handlers can access it after onReady.
  const apiRef = useRef<DockviewApi | null>(null);

  // Track the latest selection in a ref so the layout-change listener (set up
  // once in onReady) always sees the current value without re-subscribing.
  const { selectedObjectId } = useBridgeState();
  const hasSelectionRef = useRef<boolean>(selectedObjectId !== undefined);
  hasSelectionRef.current = selectedObjectId !== undefined;

  const handleReady = useCallback((event: DockviewReadyEvent): void => {
    const api = event.api;
    apiRef.current = api;

    // One-time cleanup of the stale legacy layout keys (v1, v2, v3).
    cleanupLegacyLayout();

    // Attempt to restore persisted layout; fall back to default.
    const restored = tryRestoreLayout(api);
    if (!restored) {
      buildDefaultLayout(api);
    } else {
      // dockview 6.6.1 restores the edge group (and the saved collapsed/
      // expanded state) via fromJSON. This call is a defensive, idempotent
      // safety net: it early-returns since the edge group is already present,
      // re-creating the collapsed drawer only if the restore was missing it.
      // The user's saved collapsed/expanded state is preserved (correct
      // persistence behaviour).
      ensureLogEdgeGroup(api);
    }

    // Reconcile the Inspector with the live selection (handles a restored
    // layout that included/omitted the Inspector while the selection differs).
    reconcileInspector(api, hasSelectionRef.current);

    // Register the layout persistence listener AFTER the initial build/restore
    // + edge-group + inspector reconciliation, so the construction phase does
    // not trigger redundant saves. Subsequent user-driven changes still save.
    api.onDidLayoutChange(() => {
      saveLayout(api);
    });

    // Expose the Log toggle to the parent (toolbar wiring).
    onLogToggleReady?.(() => {
      const current = apiRef.current;
      if (current !== null) {
        toggleLog(current);
      }
    });
  }, [onLogToggleReady]);

  // Clear the toolbar toggle handle on unmount.
  useEffect(() => {
    return (): void => {
      onLogToggleReady?.(undefined);
    };
  }, [onLogToggleReady]);

  // React to selection changes: add/remove the Inspector panel idempotently.
  useEffect(() => {
    const api = apiRef.current;
    if (api === null) {
      return;
    }
    reconcileInspector(api, selectedObjectId !== undefined);
  }, [selectedObjectId]);

  return (
    <DockviewReact
      components={PANEL_COMPONENTS}
      onReady={handleReady}
      className="dockview-theme-dark"
      disableFloatingGroups={true}
    />
  );
}
