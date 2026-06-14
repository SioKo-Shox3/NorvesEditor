/**
 * SettingsPanel — editor settings stub.
 *
 * P4: minimal placeholder. Settings are out of alpha scope; this panel
 * reserves the layout slot. Future phases will expand it.
 */

import type React from "react";

// SettingsPanelProps reserved for future phases (P6+).
export type SettingsPanelProps = Record<string, never>;

export function SettingsPanel(_props: SettingsPanelProps): React.JSX.Element {
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
      </div>
    </div>
  );
}
