/**
 * AppLayout — dockview-based layout container for NorvesEditor.
 *
 * Phase 1: Replaced the static CSS grid with DockviewReact.
 * Panels: GameView (center), Connection (right), Settings (right),
 *         Log (bottom), SceneOutliner (left), PropertyInspector (right).
 *
 * Layout persistence:
 *   - Saved to localStorage under LAYOUT_KEY on every layout change.
 *   - Restored from localStorage on startup (fromJSON).
 *   - If fromJSON throws (corrupt JSON), the key is purged and the
 *     default layout is rebuilt (prevents perpetual catch-on-reload).
 *
 * Layout reset:
 *   - SettingsPanel exposes a "レイアウトをリセット" button that removes
 *     the localStorage key and reloads the page.
 *
 * Floating groups are disabled (disableFloatingGroups) for alpha.
 */

import { useCallback, useRef } from 'react';
import type React from 'react';
import { DockviewReact } from 'dockview-react';
import type { DockviewApi, DockviewReadyEvent, IDockviewPanelProps } from 'dockview-react';
import { GameViewPanel }          from './GameViewPanel.js';
import { ConnectionPanel }        from './ConnectionPanel.js';
import { SettingsPanel }          from './SettingsPanel.js';
import { LogPanel }               from './LogPanel.js';
import { SceneOutlinerPanel }     from './SceneOutlinerPanel.js';
import { PropertyInspectorPanel } from './PropertyInspectorPanel.js';

// -------------------------------------------------------------------------
// Constants
// -------------------------------------------------------------------------

/** localStorage key for persisting the dockview layout JSON. */
const LAYOUT_KEY = 'norveseditor-layout-v1';

// -------------------------------------------------------------------------
// Panel component map
// -------------------------------------------------------------------------

const PANEL_COMPONENTS: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  gameView:           GameViewPanel,
  connection:         ConnectionPanel,
  settings:           SettingsPanel,
  log:                LogPanel,
  sceneOutliner:      SceneOutlinerPanel,
  propertyInspector:  PropertyInspectorPanel,
};

// -------------------------------------------------------------------------
// Default layout builder
// -------------------------------------------------------------------------

/**
 * Build the default 6-panel layout using addPanel().
 * Visual arrangement:
 *
 *   +------------------+--------------------+------------------+
 *   |                  |                    |                  |
 *   | Scene Outliner   |    Game View       | Connection       |
 *   |                  |    (center/main)   +------------------+
 *   |                  |                    | Property         |
 *   |                  +--------------------+ Inspector        |
 *   |                  |    Log             |                  |
 *   +------------------+--------------------+------------------+
 *                                           | Settings         |
 *                                           +------------------+
 */
function buildDefaultLayout(api: DockviewApi): void {
  // 1. Center: Game View (first panel — becomes the root)
  api.addPanel({
    id: 'gameView',
    component: 'gameView',
    title: 'Game View',
  });

  // 2. Right column: Connection (placed to the right of gameView)
  api.addPanel({
    id: 'connection',
    component: 'connection',
    title: 'Connection',
    position: {
      direction: 'right',
      referencePanel: 'gameView',
    },
  });

  // 3. Right column: Settings (below Connection, same group)
  api.addPanel({
    id: 'settings',
    component: 'settings',
    title: 'Settings',
    position: {
      direction: 'below',
      referencePanel: 'connection',
    },
  });

  // 4. Right column: Property Inspector (below Settings)
  api.addPanel({
    id: 'propertyInspector',
    component: 'propertyInspector',
    title: 'Property Inspector',
    position: {
      direction: 'below',
      referencePanel: 'settings',
    },
  });

  // 5. Bottom: Log (below gameView)
  api.addPanel({
    id: 'log',
    component: 'log',
    title: 'Log',
    position: {
      direction: 'below',
      referencePanel: 'gameView',
    },
  });

  // 6. Left: Scene Outliner (to the left of gameView)
  api.addPanel({
    id: 'sceneOutliner',
    component: 'sceneOutliner',
    title: 'Scene Outliner',
    position: {
      direction: 'left',
      referencePanel: 'gameView',
    },
  });
}

// -------------------------------------------------------------------------
// Persistence helpers
// -------------------------------------------------------------------------

function saveLayout(api: DockviewApi): void {
  try {
    const json = JSON.stringify(api.toJSON());
    localStorage.setItem(LAYOUT_KEY, json);
  } catch {
    // Serialization or storage failure — ignore silently.
  }
}

/**
 * Try to restore a saved layout.
 * Returns true on success, false if there was no saved layout or it was corrupt.
 * On corruption the storage key is purged so the next load rebuilds the default.
 */
function tryRestoreLayout(api: DockviewApi): boolean {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(LAYOUT_KEY);
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
      localStorage.removeItem(LAYOUT_KEY);
    } catch {
      // Ignore storage errors.
    }
    return false;
  }
}

// -------------------------------------------------------------------------
// AppLayout component
// -------------------------------------------------------------------------

export function AppLayout(): React.JSX.Element {
  // Keep a ref to the DockviewApi so the onDidLayoutChange handler can access it.
  const apiRef = useRef<DockviewApi | null>(null);

  const handleReady = useCallback((event: DockviewReadyEvent): void => {
    const api = event.api;
    apiRef.current = api;

    // Register layout persistence listener.
    api.onDidLayoutChange(() => {
      saveLayout(api);
    });

    // Attempt to restore persisted layout; fall back to default.
    const restored = tryRestoreLayout(api);
    if (!restored) {
      buildDefaultLayout(api);
    }
  }, []);

  return (
    <DockviewReact
      components={PANEL_COMPONENTS}
      onReady={handleReady}
      className="dockview-theme-dark"
      disableFloatingGroups={true}
    />
  );
}
