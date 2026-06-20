/**
 * SettingsPanel — editor settings and layout controls.
 *
 * Phase 1 refactor: added layout reset button that:
 *   1. Removes the persisted layout from localStorage (key: norveseditor-layout-v1).
 *   2. Reloads the page so the default layout is reconstructed.
 *
 * IDockviewPanelProps is accepted but containerApi is not used here;
 * the reset works via localStorage purge + page reload which is sufficient.
 */

import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';

/** localStorage key used for dockview layout persistence (Phase 1). */
const LAYOUT_STORAGE_KEY = 'norveseditor-layout-v1';

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function SettingsPanel(_props: IDockviewPanelProps): React.JSX.Element {
  function handleResetLayout(): void {
    try {
      localStorage.removeItem(LAYOUT_STORAGE_KEY);
    } catch {
      // localStorage may be unavailable in some environments — ignore silently.
    }
    // Reload the page to trigger AppLayout's onReady with the default layout.
    window.location.reload();
  }

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Settings</span>
      </div>

      <div className="panel__body col">
        <div className="placeholder-box" style={{ flex: 1 }}>
          <span className="placeholder-box__title">Settings</span>
          <span>Editor settings are not yet available in this build.</span>
          <span style={{ fontSize: 11 }}>Coming in a future release.</span>
        </div>

        {/* Layout reset — Phase 1 deliverable (c) */}
        <div className="divider" />
        <div className="col" style={{ gap: 4 }}>
          <span className="label">Layout</span>
          <button
            className="btn"
            type="button"
            onClick={handleResetLayout}
            title="Delete saved layout and restore defaults"
          >
            レイアウトをリセット
          </button>
        </div>
      </div>
    </div>
  );
}
