import type React from 'react';
import { useCallback, useState } from 'react';
import './styles.css';
import { AppLayout }              from './components/AppLayout.js';
import { AppTitleBar }            from './shell/AppTitleBar.js';
import { Toolbar }                from './components/shell/Toolbar.js';
import { ToolbarActions }         from './components/shell/ToolbarActions.js';
import { BridgeProvider }         from './state/BridgeContext.js';
import { useBridgeSubscriptions } from './hooks/useBridge.js';
import { SecondaryWindowRoot }    from './shell/SecondaryWindowRoot.js';
import { openSecondaryWindow }    from './shell/windowManager.js';
import { resolveWindowRoute }     from './shell/windowRoute.js';

/**
 * BridgeRoot — mounts the bridge event subscriptions once inside
 * BridgeProvider so that all Tauri event subscriptions are registered at the
 * app root.
 *
 * Event subscriptions are registered here exactly once via
 * useBridgeSubscriptions(). Panels obtain command callbacks via
 * useBridgeActions(), which does NOT subscribe to events, so no duplicate
 * subscriptions can occur no matter how many panels call it. Panels must NOT
 * call useBridgeSubscriptions().
 *
 * Shell structure (P3): a custom AppTitleBar → Toolbar (with ToolbarActions)
 * → body>AppLayout are stacked in a flex column (.app-shell). The OS title
 * bar is disabled via decorations:false in tauri.conf.json, so AppTitleBar
 * provides minimise / maximise / close / drag.
 *
 * P4: the toolbar's "Log" toggle is wired to the AppLayout's bottom EdgeGroup
 * drawer. AppLayout reports its toggle callback up via onLogToggleReady once
 * the dockview API is ready; we keep it in state and pass it to ToolbarActions
 * as onToggleLog.
 *
 * P5: the Connection / Settings toolbar buttons open those panels in their own
 * Tauri windows via openSecondaryWindow(); the panels also remain inside this
 * window's AppLayout (additive — they are removed from the layout in P6). The
 * Reset-Layout toggle stays unset (wired in P6).
 *
 * useBridgeSubscriptions() is still called exactly once (per window);
 * ToolbarActions uses useBridgeActions() (invoke-only, no subscriptions).
 */
function BridgeRoot(): React.JSX.Element {
  // Register the event subscriptions once at the application root.
  useBridgeSubscriptions();

  // Log-drawer toggle handle, published by AppLayout once dockview is ready.
  const [toggleLog, setToggleLog] = useState<(() => void) | undefined>(undefined);

  // Stable setter so AppLayout's onReady callback identity does not churn.
  const handleLogToggleReady = useCallback(
    (toggle: (() => void) | undefined): void => {
      // Store the function itself; the updater form avoids React treating the
      // function as a state updater.
      setToggleLog(() => toggle);
    },
    [],
  );

  // Open the Connection / Settings panels in their own Tauri windows. These are
  // fire-and-forget: openSecondaryWindow swallows/logs its own failures, so a
  // failed open never rejects into the click handler.
  const handleOpenConnection = useCallback((): void => {
    void openSecondaryWindow('connection');
  }, []);
  const handleOpenSettings = useCallback((): void => {
    void openSecondaryWindow('settings');
  }, []);

  return (
    <div className="app-shell">
      <AppTitleBar title="NorvesEditor" />
      <Toolbar>
        <ToolbarActions
          onToggleLog={toggleLog}
          onOpenConnection={handleOpenConnection}
          onOpenSettings={handleOpenSettings}
        />
      </Toolbar>
      <div className="app-shell__body">
        <AppLayout onLogToggleReady={handleLogToggleReady} />
      </div>
    </div>
  );
}

/**
 * App — query-parameter window router.
 *
 * Each Tauri WebviewWindow boots this same bundle; the `?window=` query
 * parameter selects which root to render (see windowRoute.ts):
 *   - 'main' (or absent) → the full editor shell (BridgeRoot).
 *   - 'connection' / 'settings' → a minimal SecondaryWindowRoot.
 *
 * Every route mounts its own <BridgeProvider> and subscribes exactly once via
 * useBridgeSubscriptions() (inside BridgeRoot / SecondaryWindowRoot). Each
 * window is an independent React tree, so "subscribe once" holds per window;
 * the Rust backend broadcasts events to all windows to keep them in sync.
 */
function App(): React.JSX.Element {
  const route = resolveWindowRoute(window.location.search);

  if (route === 'connection' || route === 'settings') {
    return (
      <BridgeProvider>
        <SecondaryWindowRoot target={route} />
      </BridgeProvider>
    );
  }

  return (
    <BridgeProvider>
      <BridgeRoot />
    </BridgeProvider>
  );
}

export default App;
