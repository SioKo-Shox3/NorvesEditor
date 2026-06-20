/**
 * ConnectionPanel — bridge connection controls.
 *
 * Phase 1 refactor: props drilling removed. State is obtained via
 * useBridgeState() and command callbacks via useBridgeActions().
 *
 * Connection is by PORT (numeric), not ws:// URL — the Rust backend
 * builds the WebSocket URL from the port internally.
 *
 * ConnectionStatus is the UI-level concept from the state store.
 *
 * Role separation (P6): this panel is the connection-setup surface only —
 * connect / disconnect / reconnect. Process and runtime commands (Launch,
 * Stop Process, Play, Pause, Stop, Focus Viewport) live ONLY on the main
 * window's toolbar (ToolbarActions). Do NOT add those here: a single command
 * path avoids a duplicate, drift-prone control surface. The Reconnect disabled
 * condition is kept identical to ToolbarActions (the canonical surface, the
 * main toolbar) so the two surfaces never disagree.
 */

import type React from 'react';
import { useState } from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import { useBridgeState } from '../state/BridgeContext.js';
import { useBridgeActions } from '../hooks/useBridge.js';
import type { ConnectionStatus } from '../state/store.js';

// Status label / CSS map covers ALL ConnectionStatus values (no fall-through).
const STATUS_LABELS: Record<ConnectionStatus, string> = {
  disconnected: 'Disconnected',
  connecting:   'Connecting...',
  connected:    'Connected',
  error:        'Connection error',
};

const STATUS_CSS: Record<ConnectionStatus, string> = {
  disconnected: 'status-badge--disconnected',
  connecting:   'status-badge--warning',
  connected:    'status-badge--connected',
  error:        'status-badge--error',
};

// -------------------------------------------------------------------------
// Default port constant
// -------------------------------------------------------------------------

const DEFAULT_PORT = 9001;

// -------------------------------------------------------------------------
// Component (dockview panel — no props drilling from AppLayout)
// -------------------------------------------------------------------------

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function ConnectionPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state   = useBridgeState();
  const actions = useBridgeActions();

  const status     = state.connection.status;
  const serverName = state.connection.serverName;
  const sessionId  = state.connection.sessionId;

  const [port, setPort] = useState<number>(DEFAULT_PORT);

  const isConnected  = status === 'connected';
  const isConnecting = status === 'connecting';

  // Reconnect: enabled only when status is 'connected' or 'error' — identical to
  // ToolbarActions (the main toolbar, the canonical surface), so the two
  // surfaces never disagree. Disabled while connecting or disconnected.
  const reconnectDisabled = isConnecting || status === 'disconnected';

  // Action handlers — delegate to useBridgeActions() (error mapping lives
  // there, in a single place). Button onClick expects a () => void.
  const handleConnect    = (): void => { void actions.connect(port); };
  const handleDisconnect = (): void => { void actions.disconnect(); };
  const handleReconnect  = (): void => { void actions.reconnect(); };

  function handlePortChange(e: React.ChangeEvent<HTMLInputElement>): void {
    const n = Number(e.target.value);
    if (!Number.isNaN(n) && n > 0 && n <= 65535) {
      setPort(n);
    }
  }

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Connection</span>
      </div>

      <div className="panel__body col">
        {/* Status indicator */}
        <div className="row">
          <span className="label">Status:</span>
          <span className={`status-badge ${STATUS_CSS[status]}`}>
            <span className="status-badge__dot" />
            {STATUS_LABELS[status]}
          </span>
        </div>

        {/* Server info when connected */}
        {isConnected && (serverName !== undefined || sessionId !== undefined) && (
          <div className="col" style={{ gap: 2 }}>
            {serverName !== undefined && (
              <div className="row">
                <span className="label">Server:</span>
                <span style={{ fontSize: 12 }}>{serverName}</span>
              </div>
            )}
            {sessionId !== undefined && (
              <div className="row">
                <span className="label">Session:</span>
                <span style={{ fontSize: 12 }}>{sessionId}</span>
              </div>
            )}
          </div>
        )}

        <div className="divider" />

        {/* Port input */}
        <div className="col" style={{ gap: 4 }}>
          <label className="label" htmlFor="conn-port">
            Bridge port
          </label>
          <input
            id="conn-port"
            className="input"
            type="number"
            min={1}
            max={65535}
            value={port}
            onChange={handlePortChange}
            disabled={isConnected || isConnecting}
            spellCheck={false}
          />
        </div>

        {/* Connect / Disconnect / Reconnect */}
        <div className="row">
          <button
            className="btn btn--primary"
            type="button"
            disabled={isConnected || isConnecting}
            onClick={handleConnect}
          >
            Connect
          </button>
          <button
            className="btn btn--danger"
            type="button"
            disabled={!isConnected}
            onClick={handleDisconnect}
          >
            Disconnect
          </button>
          <button
            className="btn"
            type="button"
            disabled={reconnectDisabled}
            onClick={handleReconnect}
          >
            Reconnect
          </button>
        </div>
      </div>
    </div>
  );
}
