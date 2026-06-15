/**
 * GameViewPanel — primary control panel for the engine process.
 *
 * P6: wired to real bridge state and hook actions.
 * Types come from @norves/bridge-types (not local placeholders).
 *
 * NOTE: Alpha has NO embedded viewport. The engine renders in its own
 * external window. This panel drives the process and runtime only.
 *
 * Process lifecycle (Launch / Stop-process) wired in Workstream J4.
 * - onLaunch: spawns + connects a new engine process (via launch_engine command).
 * - onStopProcess: terminates the running engine process (via stop_engine command).
 * - The ConnectionPanel's connect(port) path (attach to existing engine) is
 *   separate and unaffected.
 * Reconnect IS wired (bridge-ui action).
 */

import type React from 'react';
import type { EngineState, RuntimeState } from '@norves/bridge-types';
import type { ConnectionStatus } from '../state/store.js';

export interface GameViewPanelProps {
  /** Current engine process state (from bridge state store). */
  engineState?: EngineState;
  /** Current runtime simulation state (from bridge state store). */
  runtimeState?: RuntimeState;
  /** Whether the bridge is connected (gates runtime controls). */
  connected: boolean;
  /** Full connection status — used to derive process-button disabled logic. */
  connectionStatus?: ConnectionStatus;
  /** Reconnect to the bridge (wired to useBridge.reconnect). */
  onReconnect?: () => void;
  /** Runtime simulation handlers (wired to useBridge). */
  onPlay?: () => void;
  onPause?: () => void;
  onStopRuntime?: () => void;
  onFocusViewport?: () => void;
  /** Spawn + connect a new engine process (wired to useBridge.launch). */
  onLaunch?: () => void;
  /** Terminate the running engine process (wired to useBridge.stopProcess). */
  onStopProcess?: () => void;
}

// -------------------------------------------------------------------------
// Label / class maps covering ALL enum values (no silent fall-through)
// -------------------------------------------------------------------------

const ENGINE_LABELS: Record<EngineState, string> = {
  initializing: 'Initializing...',
  ready:        'Ready',
  running:      'Running',
  error:        'Error',
};

const RUNTIME_LABELS: Record<RuntimeState, string> = {
  edit:    'Edit',
  playing: 'Playing',
  paused:  'Paused',
  stopped: 'Stopped',
  unknown: 'Unknown',
};

// CSS modifier for the status badge
function engineChipClass(state: EngineState): string {
  switch (state) {
    case 'running':      return 'status-badge--connected';
    case 'ready':        return 'status-badge--connected';
    case 'initializing': return 'status-badge--warning';
    case 'error':        return 'status-badge--error';
  }
}

function runtimeChipClass(state: RuntimeState): string {
  switch (state) {
    case 'playing': return 'status-badge--connected';
    case 'paused':  return 'status-badge--warning';
    case 'edit':    return 'status-badge--disconnected';
    case 'stopped': return 'status-badge--disconnected';
    case 'unknown': return 'status-badge--disconnected';
  }
}

// -------------------------------------------------------------------------
// Component
// -------------------------------------------------------------------------

export function GameViewPanel({
  engineState,
  runtimeState,
  connected,
  connectionStatus,
  onReconnect,
  onPlay,
  onPause,
  onStopRuntime,
  onFocusViewport,
  onLaunch,
  onStopProcess,
}: GameViewPanelProps): React.JSX.Element {
  // Runtime controls disabled when not connected
  const runtimeDisabled = !connected;

  // Launch is only meaningful when there is no active connection (process not running).
  // Disable while connecting/connected/launching (status 'connecting' or 'connected').
  const launchDisabled =
    connectionStatus === 'connected' || connectionStatus === 'connecting';

  // Stop-process is only meaningful when a process is running (connected).
  const stopProcessDisabled = !connected;

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Game View</span>
      </div>

      <div className="panel__body col">
        {/* Viewport notice */}
        <div className="placeholder-box">
          <span className="placeholder-box__title">Engine Viewport (External Window)</span>
          <span>
            In alpha, the engine renders in its own window.
            Use &quot;Focus Viewport&quot; to bring it to the foreground.
          </span>
        </div>

        {/* Status readout */}
        <div className="divider" />
        <div className="row" style={{ gap: 16 }}>
          <span className="label">Engine:</span>
          {engineState !== undefined ? (
            <StatusChip cssClass={engineChipClass(engineState)} label={ENGINE_LABELS[engineState]} />
          ) : (
            <span className="status-badge status-badge--disconnected">
              <span className="status-badge__dot" />
              --
            </span>
          )}
          <span className="label" style={{ marginLeft: 8 }}>Runtime:</span>
          {runtimeState !== undefined ? (
            <StatusChip cssClass={runtimeChipClass(runtimeState)} label={RUNTIME_LABELS[runtimeState]} />
          ) : (
            <span className="status-badge status-badge--disconnected">
              <span className="status-badge__dot" />
              --
            </span>
          )}
        </div>

        {/* Process controls — Launch/Stop wired (Workstream J4). Reconnect is wired. */}
        <div className="divider" />
        <div className="label">Process</div>
        <div className="row">
          <button
            className="btn btn--primary"
            disabled={launchDisabled}
            onClick={onLaunch}
            title="Spawn and connect a new engine process"
            type="button"
          >
            Launch
          </button>
          <button
            className="btn btn--danger"
            disabled={stopProcessDisabled}
            onClick={onStopProcess}
            title="Terminate the running engine process"
            type="button"
          >
            Stop
          </button>
          <button
            className="btn"
            disabled={!connected}
            onClick={onReconnect}
            type="button"
          >
            Reconnect
          </button>
        </div>

        {/* Runtime controls */}
        <div className="divider" />
        <div className="label">Runtime</div>
        <div className="row">
          <button
            className="btn"
            disabled={runtimeDisabled}
            onClick={onPlay}
            type="button"
          >
            Play
          </button>
          <button
            className="btn"
            disabled={runtimeDisabled}
            onClick={onPause}
            type="button"
          >
            Pause
          </button>
          <button
            className="btn"
            disabled={runtimeDisabled}
            onClick={onStopRuntime}
            type="button"
          >
            Stop
          </button>
          <button
            className="btn"
            disabled={runtimeDisabled}
            onClick={onFocusViewport}
            type="button"
          >
            Focus Viewport
          </button>
        </div>
      </div>
    </div>
  );
}

// -------------------------------------------------------------------------
// Internal sub-component
// -------------------------------------------------------------------------

interface StatusChipProps {
  cssClass: string;
  label: string;
}

function StatusChip({ cssClass, label }: StatusChipProps): React.JSX.Element {
  return (
    <span className={`status-badge ${cssClass}`}>
      <span className="status-badge__dot" />
      {label}
    </span>
  );
}
