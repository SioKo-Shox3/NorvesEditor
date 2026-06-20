import type React from 'react';
import { useCallback, useEffect, useState } from 'react';
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
import {
  clearSavedLayoutAndReload,
  subscribeLayoutReset,
} from './shell/layoutReset.js';

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
 * P6: Connection / Settings are no longer panels in this window's AppLayout —
 * the toolbar buttons open them in their own Tauri windows via
 * openSecondaryWindow(). The Reset-Layout toggle is wired here:
 *   - the toolbar button clears the saved layout and reloads this window
 *     directly (clearSavedLayoutAndReload), and
 *   - this window also listens for a cross-window reset request emitted by the
 *     Settings window (subscribeLayoutReset) and performs the same clear +
 *     reload, since the layout (and its localStorage entry) lives here.
 * The listener is registered once and unlistened on unmount.
 *
 * useBridgeSubscriptions() is still called exactly once (per window);
 * ToolbarActions uses useBridgeActions() (invoke-only, no subscriptions).
 */
function BridgeRoot(): React.JSX.Element {
  // Register the event subscriptions once at the application root.
  useBridgeSubscriptions();

  // Listen (in the main window only) for layout-reset requests emitted by the
  // Settings window, and clear + reload here. The async listen registration is
  // guarded so a request that arrives, or an unmount that fires, before the
  // listener resolves never leaks: we unlisten immediately if cleanup already
  // ran (same pattern as AppTitleBar.onResized).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    subscribeLayoutReset(() => {
      clearSavedLayoutAndReload();
    })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch((err: unknown) => {
        // Non-fatal: the toolbar's own Reset Layout button still works.
        console.error('[BridgeRoot] Failed to subscribe to layout-reset:', err);
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

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

  // Reset the layout: clear this (main) window's saved layout and reload so
  // AppLayout rebuilds the default. Local — no cross-window hop needed.
  const handleResetLayout = useCallback((): void => {
    clearSavedLayoutAndReload();
  }, []);

  return (
    <div className="app-shell">
      <AppTitleBar title="NorvesEditor" />
      <Toolbar>
        <ToolbarActions
          onToggleLog={toggleLog}
          onOpenConnection={handleOpenConnection}
          onOpenSettings={handleOpenSettings}
          onResetLayout={handleResetLayout}
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
