/**
 * ToolbarActions — engine-control button group for the main toolbar.
 *
 * Wires useBridgeActions() (invokes only, no subscriptions) and
 * useBridgeState() (reads connection/runtime status). StatusBadge is
 * rendered inside the toolbar to surface the connection state.
 *
 * Disabled logic mirrors GameViewPanel exactly (see comment per button).
 *
 * View-toggle buttons (Connection, Settings, Log, Reset Layout) are
 * placeholder stubs in P3. They accept optional on* callbacks; when the
 * callback is undefined (App.tsx leaves them unset) the button is disabled
 * with a title explaining it is not yet wired.
 *
 * Engine-agnostic: no mock-specific control is hard-coded.
 * No event subscription added here — useBridgeSubscriptions() still called
 * exactly once at the app root in App.tsx.
 */

import type React from 'react';
import { useBridgeState } from '../../state/BridgeContext.js';
import { useBridgeActions } from '../../hooks/useBridge.js';
import { StatusBadge } from './StatusBadge.js';

// -------------------------------------------------------------------------
// Connection-status label map (same values as ConnectionPanel)
// -------------------------------------------------------------------------

const STATUS_LABELS = {
  disconnected: 'Disconnected',
  connecting:   'Connecting…',
  connected:    'Connected',
  error:        'Error',
} as const;

// -------------------------------------------------------------------------
// Props
// -------------------------------------------------------------------------

export interface ToolbarActionsProps {
  /** Open the Connection window (P5). undefined → button disabled. */
  onOpenConnection?: (() => void) | undefined;
  /** Open the Settings window (P6). undefined → button disabled. */
  onOpenSettings?: (() => void) | undefined;
  /** Expand the Log panel (P4). undefined → button disabled. */
  onToggleLog?: (() => void) | undefined;
  /** Reset the dockview layout (P6). undefined → button disabled. */
  onResetLayout?: (() => void) | undefined;
}

// -------------------------------------------------------------------------
// Toolbar separator helper
// -------------------------------------------------------------------------

function Sep(): React.JSX.Element {
  return <span className="toolbar__sep" aria-hidden="true" />;
}

// -------------------------------------------------------------------------
// Component
// -------------------------------------------------------------------------

export function ToolbarActions({
  onOpenConnection,
  onOpenSettings,
  onToggleLog,
  onResetLayout,
}: ToolbarActionsProps): React.JSX.Element {
  const state   = useBridgeState();
  const actions = useBridgeActions();

  const connectionStatus = state.connection.status;
  const connected        = connectionStatus === 'connected';

  // -----------------------------------------------------------------------
  // Disabled conditions — copied verbatim from GameViewPanel (the "正")
  // -----------------------------------------------------------------------

  /** Launch: disabled while already connected or connecting. */
  const launchDisabled =
    connectionStatus === 'connected' || connectionStatus === 'connecting';

  /** Stop Process: disabled while not connected. */
  const stopProcessDisabled = !connected;

  /**
   * Reconnect: disabled while connecting, disconnected, or undefined.
   * GameViewPanel: connectionStatus === 'connecting' || 'disconnected' || undefined
   * → enabled only when status is 'connected' or 'error'.
   */
  const reconnectDisabled =
    connectionStatus === 'connecting' ||
    connectionStatus === 'disconnected' ||
    connectionStatus === undefined;

  /** Runtime actions: disabled while not connected. */
  const runtimeDisabled = !connected;

  // -----------------------------------------------------------------------
  // Action handlers (same pattern as GameViewPanel)
  // -----------------------------------------------------------------------

  const handleLaunch        = (): void => { void actions.launch(); };
  const handleStopProcess   = (): void => { void actions.stopProcess(); };
  const handleReconnect     = (): void => { void actions.reconnect(); };
  const handlePlay          = (): void => { void actions.play(); };
  const handlePause         = (): void => { void actions.pause(); };
  const handleStopRuntime   = (): void => { void actions.stop(); };
  const handleFocusViewport = (): void => { void actions.focusViewport(); };

  // -----------------------------------------------------------------------
  // View-toggle handlers (P3: disabled placeholders when callback absent)
  // -----------------------------------------------------------------------

  const viewTogglePlaceholder = 'Available in a future phase';

  return (
    <>
      {/* Connection status badge */}
      <StatusBadge
        status={connectionStatus}
        label={STATUS_LABELS[connectionStatus]}
      />

      <Sep />

      {/* Process controls */}
      <button
        className="btn toolbar__btn"
        type="button"
        disabled={launchDisabled}
        onClick={handleLaunch}
        title="Spawn and connect a new engine process"
        aria-label="Launch engine"
      >
        Launch
      </button>
      <button
        className="btn btn--danger toolbar__btn"
        type="button"
        disabled={stopProcessDisabled}
        onClick={handleStopProcess}
        title="Terminate the running engine process"
        aria-label="Stop process"
      >
        Stop Process
      </button>
      <button
        className="btn toolbar__btn"
        type="button"
        disabled={reconnectDisabled}
        onClick={handleReconnect}
        title="Reconnect to the bridge"
        aria-label="Reconnect"
      >
        Reconnect
      </button>

      <Sep />

      {/* Runtime controls */}
      <button
        className="btn toolbar__btn"
        type="button"
        disabled={runtimeDisabled}
        onClick={handlePlay}
        title="Start playback"
        aria-label="Play"
      >
        Play
      </button>
      <button
        className="btn toolbar__btn"
        type="button"
        disabled={runtimeDisabled}
        onClick={handlePause}
        title="Pause playback"
        aria-label="Pause"
      >
        Pause
      </button>
      <button
        className="btn toolbar__btn"
        type="button"
        disabled={runtimeDisabled}
        onClick={handleStopRuntime}
        title="Stop playback"
        aria-label="Stop runtime"
      >
        Stop
      </button>
      <button
        className="btn toolbar__btn"
        type="button"
        disabled={runtimeDisabled}
        onClick={handleFocusViewport}
        title="Bring the engine viewport to the foreground"
        aria-label="Focus Viewport"
      >
        Focus Viewport
      </button>

      <Sep />

      {/* View toggles — P3 stubs (disabled until later phases wire callbacks) */}
      <button
        className="btn toolbar__btn"
        type="button"
        disabled={onOpenConnection === undefined}
        onClick={onOpenConnection}
        title={
          onOpenConnection !== undefined
            ? 'Open Connection window'
            : viewTogglePlaceholder
        }
        aria-label="Open Connection window"
      >
        Connection
      </button>
      <button
        className="btn toolbar__btn"
        type="button"
        disabled={onToggleLog === undefined}
        onClick={onToggleLog}
        title={
          onToggleLog !== undefined
            ? 'Toggle Log panel'
            : viewTogglePlaceholder
        }
        aria-label="Toggle Log"
      >
        Log
      </button>
      <button
        className="btn toolbar__btn"
        type="button"
        disabled={onOpenSettings === undefined}
        onClick={onOpenSettings}
        title={
          onOpenSettings !== undefined
            ? 'Open Settings window'
            : viewTogglePlaceholder
        }
        aria-label="Open Settings window"
      >
        Settings
      </button>
      <button
        className="btn toolbar__btn"
        type="button"
        disabled={onResetLayout === undefined}
        onClick={onResetLayout}
        title={
          onResetLayout !== undefined
            ? 'Reset panel layout'
            : viewTogglePlaceholder
        }
        aria-label="Reset Layout"
      >
        Reset Layout
      </button>
    </>
  );
}
