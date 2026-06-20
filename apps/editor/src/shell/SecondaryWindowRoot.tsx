/**
 * SecondaryWindowRoot — minimal shell for a secondary editor window.
 *
 * Each secondary window (Connection, Settings) is its own Tauri WebviewWindow =
 * an independent webview = an independent React tree. App() routes to this
 * component when the `?window=` query parameter is 'connection' or 'settings'.
 *
 * Subscription model: this window subscribes to the bridge events exactly once
 * here, via useBridgeSubscriptions(), inside its own <BridgeProvider> (mounted
 * by App()). State stays in sync because the Rust backend broadcasts events to
 * every window. The main window's once-only subscription invariant is preserved
 * per window — each window owns exactly one subscription set.
 *
 * The shell is intentionally minimal: a custom AppTitleBar (window controls /
 * drag) plus the single routed panel. dockview / AppLayout is NOT rendered here;
 * those belong to the main window only.
 *
 * The panels (ConnectionPanel / SettingsPanel) are dockview panel components and
 * accept IDockviewPanelProps, but here they are rendered standalone. They only
 * read state via the bridge hooks and never use the passed props for data, so a
 * minimal placeholder props object is sufficient.
 */

import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import { AppTitleBar } from './AppTitleBar.js';
import { ConnectionPanel } from '../components/ConnectionPanel.js';
import { SettingsPanel } from '../components/SettingsPanel.js';
import { useBridgeSubscriptions } from '../hooks/useBridge.js';
import type { SecondaryWindowTarget } from './windowManager.js';

export interface SecondaryWindowRootProps {
  /** Which panel this window hosts. */
  target: SecondaryWindowTarget;
}

/** Per-target window title shown in the custom title bar. */
const WINDOW_TITLES: Record<SecondaryWindowTarget, string> = {
  connection: 'Connection',
  settings:   'Settings',
};

/**
 * Placeholder props for the dockview panel components when rendered standalone.
 * ConnectionPanel / SettingsPanel ignore these (they read bridge state via
 * hooks), so an empty cast is sufficient and avoids fabricating a fake
 * DockviewApi. Centralised here so the cast lives in one place.
 */
const STANDALONE_PANEL_PROPS = {} as IDockviewPanelProps;

export function SecondaryWindowRoot({ target }: SecondaryWindowRootProps): React.JSX.Element {
  // Register this window's event subscriptions exactly once (one set per
  // window). The backend broadcasts to every window, so this window stays in
  // sync without any cross-window wiring.
  useBridgeSubscriptions();

  return (
    <div className="app-shell">
      <AppTitleBar title={WINDOW_TITLES[target]} />
      <div className="app-shell__body app-shell__body--secondary">
        {target === 'connection' ? (
          <ConnectionPanel {...STANDALONE_PANEL_PROPS} />
        ) : (
          <SettingsPanel {...STANDALONE_PANEL_PROPS} />
        )}
      </div>
    </div>
  );
}
