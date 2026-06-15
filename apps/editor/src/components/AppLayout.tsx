/**
 * AppLayout — 4-panel layout for NorvesEditor.
 *
 * P6: reads live state from BridgeContext and wires useBridge actions
 * to each panel. The store Provider and useBridge mount are in App.tsx.
 *
 * Layout (CSS grid):
 *
 *   +---------------------------+------------------+
 *   |                           |                  |
 *   |        Game View          |   Connection     |
 *   |    (primary control)      |                  |
 *   |                           +------------------+
 *   |                           |                  |
 *   +---------------------------+   Settings       |
 *   |                           |                  |
 *   |           Log             |                  |
 *   |      (full width)         |                  |
 *   +---------------------------+------------------+
 */

import type React from 'react';
import { GameViewPanel }    from './GameViewPanel.js';
import { LogPanel }         from './LogPanel.js';
import { ConnectionPanel }  from './ConnectionPanel.js';
import { SettingsPanel }    from './SettingsPanel.js';
import { useBridgeState }   from '../state/BridgeContext.js';
import { useBridge }        from '../hooks/useBridge.js';

const layoutStyle: React.CSSProperties = {
  display: 'grid',
  gridTemplateColumns: '1fr 280px',
  gridTemplateRows:    '1fr 220px',
  gap:    '4px',
  padding: '4px',
  height: '100%',
  width:  '100%',
  overflow: 'hidden',
};

const rightColumnStyle: React.CSSProperties = {
  display: 'grid',
  gridTemplateRows: '1fr 1fr',
  gap: '4px',
  minHeight: 0,
};

export function AppLayout(): React.JSX.Element {
  const state   = useBridgeState();
  const actions = useBridge();

  const connected = state.connection.status === 'connected';

  return (
    <div style={layoutStyle}>
      {/* Top-left: Game View */}
      <div style={{ gridColumn: '1', gridRow: '1', minHeight: 0, display: 'flex', flexDirection: 'column' }}>
        <GameViewPanel
          engineState={state.engineState}
          runtimeState={state.runtimeState}
          connected={connected}
          connectionStatus={state.connection.status}
          onReconnect={() => { void actions.reconnect(); }}
          onPlay={() => { void actions.play(); }}
          onPause={() => { void actions.pause(); }}
          onStopRuntime={() => { void actions.stop(); }}
          onFocusViewport={() => { void actions.focusViewport(); }}
          onLaunch={() => { void actions.launch(); }}
          onStopProcess={() => { void actions.stopProcess(); }}
        />
      </div>

      {/* Right sidebar: Connection + Settings stacked */}
      <div style={{ gridColumn: '2', gridRow: '1 / span 2', minHeight: 0, ...rightColumnStyle }}>
        <ConnectionPanel
          status={state.connection.status}
          serverName={state.connection.serverName}
          sessionId={state.connection.sessionId}
          onConnect={(port) => { void actions.connect(port); }}
          onDisconnect={() => { void actions.disconnect(); }}
          onReconnect={() => { void actions.reconnect(); }}
        />
        <SettingsPanel />
      </div>

      {/* Bottom: Log */}
      <div style={{ gridColumn: '1', gridRow: '2', minHeight: 0, display: 'flex', flexDirection: 'column' }}>
        <LogPanel entries={state.logs} />
      </div>
    </div>
  );
}
