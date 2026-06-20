/**
 * AppLayout — dockview-based layout container for NorvesEditor.
 *
 * P4 layout redesign. New default arrangement:
 *
 *   +-------------------------------------+----------------------+
 *   |                                     |  Scene Outliner      |
 *   |                                     |  (right column top)  |
 *   |          Game View                  +----------------------+
 *   |          (centre / largest)         |  Connection | Settings
 *   |                                     |  [ Inspector ]  (tabs)
 *   |                                     |  (right column bottom)
 *   +-------------------------------------+----------------------+
 *   |  Log (bottom EdgeGroup drawer, collapsed by default)       |
 *   +------------------------------------------------------------+
 *
 *   - Game View is the centre/root panel (largest area).
 *   - Scene Outliner sits at the top of the right column.
 *   - The bottom of the right column is a TAB GROUP holding Connection and
 *     Settings (and, when an object is selected, the Property Inspector).
 *   - Log lives in a bottom EdgeGroup drawer, collapsed by default; the main
 *     toolbar's "Log" toggle (P3) expands/collapses it.
 *
 * Connection/Settings stay in the layout (not moved to their own window) so
 * they remain reachable in P4; moving them to dedicated windows happens in P6.
 *
 * Property Inspector visibility:
 *   - Driven solely by selectedObjectId (engine-agnostic — no mock-specific
 *     names). When an object is selected the Inspector is added as a tab in the
 *     bottom-right group; when deselected it is removed.
 *   - Idempotent: never double-adds (existence checked via getPanel).
 *   - Reconciled after restore so the restored layout matches the current
 *     selection (Inspector shown iff an object is selected).
 *
 * Layout persistence:
 *   - Saved to localStorage under LAYOUT_STORAGE_KEY (v2) on every change.
 *   - Restored on startup (fromJSON). Corrupt JSON purges the key and rebuilds.
 *   - On startup any stale v1 key is removed once (cleanup).
 *
 * Layout reset is exposed by SettingsPanel (shared key import).
 * Floating groups are disabled (disableFloatingGroups) for alpha (tear-off is
 * a future phase).
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
import { ConnectionPanel }        from './ConnectionPanel.js';
import { SettingsPanel }          from './SettingsPanel.js';
import { LogPanel }               from './LogPanel.js';
import { SceneOutlinerPanel }     from './SceneOutlinerPanel.js';
import { PropertyInspectorPanel } from './PropertyInspectorPanel.js';
import { LAYOUT_STORAGE_KEY, LEGACY_LAYOUT_STORAGE_KEY_V1 } from './shell/layoutKey.js';
import { useBridgeState } from '../state/BridgeContext.js';

// -------------------------------------------------------------------------
// Constants
// -------------------------------------------------------------------------

/** Panel ids — shared between the default builder and dynamic reconciliation. */
const PANEL_GAME_VIEW      = 'gameView';
const PANEL_SCENE_OUTLINER = 'sceneOutliner';
const PANEL_CONNECTION     = 'connection';
const PANEL_SETTINGS       = 'settings';
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

const PANEL_COMPONENTS: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  [PANEL_GAME_VIEW]:      GameViewPanel,
  [PANEL_CONNECTION]:     ConnectionPanel,
  [PANEL_SETTINGS]:       SettingsPanel,
  [PANEL_LOG]:            LogPanel,
  [PANEL_SCENE_OUTLINER]: SceneOutlinerPanel,
  [PANEL_INSPECTOR]:      PropertyInspectorPanel,
};

// -------------------------------------------------------------------------
// Default layout builder
// -------------------------------------------------------------------------

/**
 * Build the default layout. Game View is the root (centre/largest); the right
 * column holds Scene Outliner on top and a Connection/Settings tab group below;
 * the Log lives in a collapsed bottom EdgeGroup drawer.
 *
 * The Property Inspector is NOT added here — it is added on demand by the
 * selection-driven reconciliation (reconcileInspector).
 */
function buildDefaultLayout(api: DockviewApi): void {
  // 1. Centre: Game View (first panel — becomes the root, largest area).
  api.addPanel({
    id: PANEL_GAME_VIEW,
    component: PANEL_GAME_VIEW,
    title: 'Game View',
  });

  // 2. Right column top: Scene Outliner (to the right of Game View).
  api.addPanel({
    id: PANEL_SCENE_OUTLINER,
    component: PANEL_SCENE_OUTLINER,
    title: 'Scene Outliner',
    position: {
      direction: 'right',
      referencePanel: PANEL_GAME_VIEW,
    },
  });

  // 3. Right column bottom: Connection (below the outliner) — this becomes the
  //    bottom-right tab group that also hosts Settings and the Inspector.
  api.addPanel({
    id: PANEL_CONNECTION,
    component: PANEL_CONNECTION,
    title: 'Connection',
    position: {
      direction: 'below',
      referencePanel: PANEL_SCENE_OUTLINER,
    },
  });

  // 4. Settings as a tab in the same group as Connection (kept reachable in
  //    P4; moved to its own window in P6).
  api.addPanel({
    id: PANEL_SETTINGS,
    component: PANEL_SETTINGS,
    title: 'Settings',
    position: {
      referenceGroup: connectionGroupId(api) ?? PANEL_CONNECTION,
    },
  });

  // 5. Log: bottom EdgeGroup drawer, collapsed by default.
  ensureLogEdgeGroup(api);
}

/**
 * Return the dockview group id that contains the Connection panel, or undefined
 * if Connection is not present. This is the bottom-right tab group that hosts
 * Connection / Settings / Inspector.
 */
function connectionGroupId(api: DockviewApi): string | undefined {
  return api.getPanel(PANEL_CONNECTION)?.group.id;
}

/**
 * Resolve the bottom-right tab group id, preferring the Connection group, then
 * the Settings group (Connection may have been moved/closed in a future phase).
 * Returns undefined when neither is present.
 */
function bottomRightGroupId(api: DockviewApi): string | undefined {
  return (
    api.getPanel(PANEL_CONNECTION)?.group.id ??
    api.getPanel(PANEL_SETTINGS)?.group.id
  );
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
 *   selectedObjectId set + no Inspector panel → add it (as a tab in the
 *                                                bottom-right group).
 *   selectedObjectId unset + Inspector present → remove it.
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
    const groupId = bottomRightGroupId(api);
    api.addPanel({
      id: PANEL_INSPECTOR,
      component: PANEL_INSPECTOR,
      title: 'Property Inspector',
      // Prefer the bottom-right tab group; if it is gone, dockview places the
      // panel in a default location rather than throwing.
      ...(groupId !== undefined
        ? { position: { referenceGroup: groupId } }
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
 * Remove the stale v1 layout key once on startup so old garbage does not
 * linger in localStorage. Never re-written. Failures are ignored.
 */
function cleanupLegacyLayout(): void {
  try {
    localStorage.removeItem(LEGACY_LAYOUT_STORAGE_KEY_V1);
  } catch {
    // localStorage may be unavailable — ignore silently.
  }
}

/**
 * Try to restore a saved layout (v2 key only).
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

    // One-time cleanup of the old v1 layout key.
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
