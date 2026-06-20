/**
 * SettingsPanel — editor settings and layout controls.
 *
 * P6: Settings is rendered ONLY in its own Tauri window (SecondaryWindowRoot);
 * it is no longer a panel in the main window's dockview. The layout it resets
 * lives in the MAIN window, so the reset button here cannot clear localStorage
 * or reload locally — that would touch the wrong window. Instead it emits a
 * frontend layout-reset request (requestLayoutReset); the main window listens
 * for it and performs the actual clear + reload (see shell/layoutReset.ts).
 * This avoids relying on shared localStorage between windows.
 *
 * IDockviewPanelProps is accepted but containerApi is not used here; the reset
 * works via the cross-window event, not the dockview API.
 */

import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import { requestLayoutReset } from '../shell/layoutReset.js';

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function SettingsPanel(_props: IDockviewPanelProps): React.JSX.Element {
  function handleResetLayout(): void {
    // Fire-and-forget: emit a reset request to the main window. We do NOT touch
    // this window's localStorage or reload it — the main window owns the layout
    // and performs the actual reset on receiving the event. A failed emit is
    // non-fatal (the main toolbar's Reset Layout button is an alternative path).
    void requestLayoutReset().catch((err: unknown) => {
      console.error('[SettingsPanel] Failed to request layout reset:', err);
    });
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

        {/* Layout reset — relays the request to the main window (P6). */}
        <div className="divider" />
        <div className="col" style={{ gap: 4 }}>
          <span className="label">Layout</span>
          <button
            className="btn"
            type="button"
            onClick={handleResetLayout}
            title="Delete the main window's saved layout and restore defaults"
          >
            レイアウトをリセット
          </button>
        </div>
      </div>
    </div>
  );
}
