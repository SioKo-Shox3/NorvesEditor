import type React from 'react';
import './styles.css';
import { AppLayout }              from './components/AppLayout.js';
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
 */
function BridgeRoot(): React.JSX.Element {
  // Register the event subscriptions once at the application root.
  useBridgeSubscriptions();
  return <AppLayout />;
}

function App(): React.JSX.Element {
  return (
    <BridgeProvider>
      <BridgeRoot />
    </BridgeProvider>
  );
}

export default App;
