import type React from 'react';
import './styles.css';
import { AppLayout }              from './components/AppLayout.js';
import { AppTitleBar }            from './shell/AppTitleBar.js';
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
 * Shell structure: a custom AppTitleBar is stacked above AppLayout in a flex
 * column (.app-shell). The OS title bar is disabled via decorations:false in
 * tauri.conf.json, so AppTitleBar provides minimise / maximise / close / drag.
 * Wiring the title bar here does not add or remove any event subscription —
 * useBridgeSubscriptions() is still called exactly once.
 */
function BridgeRoot(): React.JSX.Element {
  // Register the event subscriptions once at the application root.
  useBridgeSubscriptions();
  return (
    <div className="app-shell">
      <AppTitleBar title="NorvesEditor" />
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
