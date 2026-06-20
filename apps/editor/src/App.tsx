import type React from 'react';
import { useCallback, useState } from 'react';
import './styles.css';
import { AppLayout }              from './components/AppLayout.js';
import { AppTitleBar }            from './shell/AppTitleBar.js';
import { Toolbar }                from './components/shell/Toolbar.js';
import { ToolbarActions }         from './components/shell/ToolbarActions.js';
import { BridgeProvider }         from './state/BridgeContext.js';
import { useBridgeSubscriptions } from './hooks/useBridge.js';

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
 * as onToggleLog. Connection/Settings/Reset-Layout toggles stay unset (those
 * are wired in P5/P6).
 *
 * useBridgeSubscriptions() is still called exactly once; ToolbarActions
 * uses useBridgeActions() (invoke-only, no subscriptions).
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

  return (
    <div className="app-shell">
      <AppTitleBar title="NorvesEditor" />
      <Toolbar>
        <ToolbarActions onToggleLog={toggleLog} />
      </Toolbar>
      <div className="app-shell__body">
        <AppLayout onLogToggleReady={handleLogToggleReady} />
      </div>
    </div>
  );
}

function App(): React.JSX.Element {
  return (
    <BridgeProvider>
      <BridgeRoot />
    </BridgeProvider>
  );
}

export default App;
