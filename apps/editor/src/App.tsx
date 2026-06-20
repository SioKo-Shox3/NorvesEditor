import type React from 'react';
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
 * Toolbar is placed between TitleBar and the body so engine controls are
 * always visible above the panel layout.
 *
 * View-toggle callbacks (onOpenConnection etc.) are left unset in P3;
 * ToolbarActions renders those buttons as disabled stubs. They will be wired
 * in P4/P5/P6 without touching this function's structure.
 *
 * useBridgeSubscriptions() is still called exactly once; ToolbarActions
 * uses useBridgeActions() (invoke-only, no subscriptions).
 */
function BridgeRoot(): React.JSX.Element {
  // Register the event subscriptions once at the application root.
  useBridgeSubscriptions();
  return (
    <div className="app-shell">
      <AppTitleBar title="NorvesEditor" />
      <Toolbar>
        <ToolbarActions />
      </Toolbar>
      <div className="app-shell__body">
        <AppLayout />
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
