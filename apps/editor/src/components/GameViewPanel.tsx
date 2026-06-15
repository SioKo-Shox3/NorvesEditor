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
 *
 * Workstream K finalization:
 * - Error banner with humanized kind labels + [object Object] guard.
 * - Viewport status badge.
 * - Reconnect button enabled on 'error' status.
 */

import type React from 'react';
import type { EngineState, RuntimeState, ViewportState } from '@norves/bridge-types';
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
  /** Latest viewport state from the engine (Workstream K). */
  viewportState?: ViewportState;
  /** Current error to display (Workstream K). */
  lastError?: { kind?: string; message: string };
  /** Called when the user dismisses the error banner (Workstream K). */
  onDismissError?: () => void;
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

const VIEWPORT_LABELS: Record<ViewportState, string> = {
  focused:   'Focused',
  visible:   'Visible',
  hidden:    'Hidden',
  minimized: 'Minimized',
  unknown:   'Unknown',
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
// Error kind -> human-readable label map
// Covers all BackendError serde tags + frontend fallback codes from useBridge.
// -------------------------------------------------------------------------

const ERROR_KIND_LABELS: Record<string, string> = {
  // Rust serde-tagged BackendError variants
  connect:          'Connection error',
  request:          'Request failed',
  engine:           'Engine error',
  handshake:        'Handshake failed',
  process:          'Process error',
  notConnected:     'Not connected',
  alreadyConnected: 'Already connected',
  // Frontend fallback codes set by useBridge catch blocks
  CONNECT_FAILED:       'Connection error',
  DISCONNECT_FAILED:    'Disconnect failed',
  RECONNECT_FAILED:     'Reconnect failed',
  GET_STATUS_FAILED:    'Status check failed',
  PLAY_FAILED:          'Play failed',
  PAUSE_FAILED:         'Pause failed',
  STOP_FAILED:          'Stop failed',
  FOCUS_FAILED:         'Focus failed',
  LAUNCH_FAILED:        'Launch failed',
  STOP_PROCESS_FAILED:  'Stop process failed',
};

function kindLabel(kind: string | undefined): string {
  if (kind === undefined || kind === '') return 'Error';
  return ERROR_KIND_LABELS[kind] ?? 'Error';
}

/**
 * Returns true when the message field is absent, empty, or the string
 * "[object Object]" that arises when Tauri serializes a non-string Rust value.
 */
function isUselessMessage(msg: string | undefined): boolean {
  return msg === undefined || msg === '' || msg === '[object Object]';
}

// -------------------------------------------------------------------------
// Component
// -------------------------------------------------------------------------

export function GameViewPanel({
  engineState,
  runtimeState,
  connected,
  connectionStatus,
  viewportState,
  lastError,
  onDismissError,
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

  // Reconnect: disabled while connecting or disconnected.
  // 'connected' AND 'error' both enable the button.
  // Treat undefined as disconnected (safe fallback -> disabled).
  const reconnectDisabled =
    connectionStatus === 'connecting' ||
    connectionStatus === 'disconnected' ||
    connectionStatus === undefined;

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Game View</span>
      </div>

      <div className="panel__body col">
        {/* Error banner — shown only when lastError is set */}
        {lastError !== undefined && (
          <div className="error-banner" role="alert">
            <span className="error-banner__kind">
              {kindLabel(lastError.kind)}
            </span>
            {!isUselessMessage(lastError.message) && (
              <span className="error-banner__message">
                {': '}
                {lastError.message}
              </span>
            )}
            <button
              className="error-banner__dismiss"
              type="button"
              aria-label="Dismiss error"
              onClick={onDismissError}
            >
              x
            </button>
          </div>
        )}

        {/* Viewport notice */}
        <div className="placeholder-box">
          <span className="placeholder-box__title">Engine Viewport (External Window)</span>
          <span>
            In alpha, the engine renders in its own window.
            Use &quot;Focus Viewport&quot; to bring it to the foreground.
          </span>
          {/* Viewport status badge */}
          <div className="row" style={{ justifyContent: 'center', marginTop: 4 }}>
            <span className="label">Viewport:</span>
            <span className="status-badge status-badge--disconnected">
              <span className="status-badge__dot" />
              {viewportState !== undefined ? VIEWPORT_LABELS[viewportState] : '--'}
            </span>
          </div>
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
            disabled={reconnectDisabled}
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
