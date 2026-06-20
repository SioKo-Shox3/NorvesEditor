/**
 * GameViewPanel — primary control panel for the engine process.
 *
 * Phase 1 refactor: props drilling removed. State is obtained via
 * useBridgeState() and command callbacks via useBridgeActions().
 * Rendering logic is unchanged from the original implementation.
 *
 * NOTE: Alpha has NO embedded viewport. The engine renders in its own
 * external window. This panel drives the process and runtime only.
 *
 * Process lifecycle (Launch / Stop-process) wired in Workstream J4.
 * - launch: spawns + connects a new engine process (via launch_engine command).
 * - stopProcess: terminates the running engine process (via stop_engine command).
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
import type { IDockviewPanelProps } from 'dockview-react';
import { useBridgeState } from '../state/BridgeContext.js';
import { useBridgeActions } from '../hooks/useBridge.js';

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
// Covers all BackendError serde tags + frontend fallback codes from useBridgeActions.
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
  // Frontend fallback codes set by useBridgeActions catch blocks
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
// Component (dockview panel — no props drilling from AppLayout)
// -------------------------------------------------------------------------

// IDockviewPanelProps is accepted but not currently used for data.
// It is required by the dockview component map type signature.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function GameViewPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state   = useBridgeState();
  const actions = useBridgeActions();

  const engineState      = state.engineState;
  const runtimeState     = state.runtimeState;
  const viewportState    = state.viewportState;
  const lastError        = state.lastError;
  const connectionStatus = state.connection.status;
  const connected        = connectionStatus === 'connected';

  // -----------------------------------------------------------------------
  // Action handlers — delegate to useBridgeActions() (error mapping lives
  // there, in a single place). Button onClick expects a () => void.
  // -----------------------------------------------------------------------

  const handleDismissError  = (): void => { actions.dismissError(); };
  const handleReconnect     = (): void => { void actions.reconnect(); };
  const handlePlay          = (): void => { void actions.play(); };
  const handlePause         = (): void => { void actions.pause(); };
  const handleStopRuntime   = (): void => { void actions.stop(); };
  const handleFocusViewport = (): void => { void actions.focusViewport(); };
  const handleLaunch        = (): void => { void actions.launch(); };
  const handleStopProcess   = (): void => { void actions.stopProcess(); };

  // -----------------------------------------------------------------------
  // Derived disabled states (identical logic to the original prop-driven impl)
  // -----------------------------------------------------------------------

  const runtimeDisabled = !connected;

  const launchDisabled =
    connectionStatus === 'connected' || connectionStatus === 'connecting';

  const stopProcessDisabled = !connected;

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
              onClick={handleDismissError}
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
            onClick={handleLaunch}
            title="Spawn and connect a new engine process"
            type="button"
          >
            Launch
          </button>
          <button
            className="btn btn--danger"
            disabled={stopProcessDisabled}
            onClick={handleStopProcess}
            title="Terminate the running engine process"
            type="button"
          >
            Stop
          </button>
          <button
            className="btn"
            disabled={reconnectDisabled}
            onClick={handleReconnect}
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
            onClick={handlePlay}
            type="button"
          >
            Play
          </button>
          <button
            className="btn"
            disabled={runtimeDisabled}
            onClick={handlePause}
            type="button"
          >
            Pause
          </button>
          <button
            className="btn"
            disabled={runtimeDisabled}
            onClick={handleStopRuntime}
            type="button"
          >
            Stop
          </button>
          <button
            className="btn"
            disabled={runtimeDisabled}
            onClick={handleFocusViewport}
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
