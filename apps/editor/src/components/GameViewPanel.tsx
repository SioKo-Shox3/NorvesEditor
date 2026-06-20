/**
 * GameViewPanel — primary engine view (centre / largest panel).
 *
 * Phase 1 refactor: props drilling removed. State is obtained via
 * useBridgeState() and command callbacks via useBridgeActions().
 *
 * P4: engine/runtime control buttons (Launch / Stop Process / Reconnect /
 * Play / Pause / Stop / Focus Viewport) were moved to the main toolbar in P3
 * (ToolbarActions), so they are removed here to avoid duplicate controls
 * (m1). This panel now focuses on the viewport view: live thumbnail (or the
 * external-window notice when unsupported), the viewport status badge, the
 * engine/runtime status read-out, and the error banner. P7: thumbnail pull
 * uses a self-scheduling setTimeout loop with exponential back-off on errors
 * (useThumbnailAutoRefresh).
 *
 * NOTE: Alpha has NO embedded viewport. The engine renders in its own
 * external window.
 *
 * Workstream K finalization (retained here):
 * - Error banner with humanized kind labels + [object Object] guard.
 * - Viewport status badge.
 */

import type React from 'react';
import { useCallback, useEffect, useMemo, useRef } from 'react';
import type { EngineState, RuntimeState, ViewportState } from '@norves/bridge-types';
import type { IDockviewPanelProps } from 'dockview-react';
import { useBridgeState } from '../state/BridgeContext.js';
import { useBridgeActions } from '../hooks/useBridge.js';
import type { ThumbnailPullResult } from '../hooks/useBridge.js';

// -------------------------------------------------------------------------
// Thumbnail policy constants (docs/memory-buffer-policy.md, Phase 7b).
// pull-style, PNG, max 640x360, <= 1 fps. The auto-refresh interval is a
// floor of 1000 ms so the UI never asks faster than 1 fps.
// -------------------------------------------------------------------------

const THUMBNAIL_MAX_WIDTH = 640;
const THUMBNAIL_MAX_HEIGHT = 360;
/** Auto-refresh cadence: 1 fps cap (>= 1000 ms between pulls). */
const THUMBNAIL_REFRESH_MS = 1000;
/** Exponential back-off ceiling: do not wait longer than 30 s after errors. */
const THUMBNAIL_BACKOFF_CAP_MS = 30_000;

// -------------------------------------------------------------------------
// useThumbnailAutoRefresh — self-scheduling setTimeout loop with exponential
// back-off on consecutive errors. Engine-agnostic: back-off absorbs any
// transient failure without hammering the engine or the WebSocket at 1 fps.
//
// Interface:
//   pull       — the async function that fetches one thumbnail (returns
//                ThumbnailPullResult: 'ok' | 'unsupported' | 'error').
//   connected  — when false the loop does not start (or stops on transition).
//   unsupported— when true the loop does not start (engine lacks the method).
//
// Returns:
//   refreshNow()  — reset failure counter, pull once immediately, restart loop
//                   at the base interval (wired to the manual Refresh button).
//
// Back-off schedule (consecutive errors):
//   0 errors → 1000 ms (BASE)
//   1 error  → 2000 ms
//   2 errors → 4000 ms
//   …        → min(BASE * 2^failures, CAP=30 000 ms)
//
// Reset triggers:
//   - 'ok' result         → counter = 0, next interval = BASE
//   - new effect run      → connected false→true or unsupported changes →
//                           effect tears down + re-mounts → counter = 0
//   - refreshNow()        → counter = 0, immediate pull, BASE interval
// -------------------------------------------------------------------------

function useThumbnailAutoRefresh(
  connected: boolean,
  unsupported: boolean,
  pull: (maxWidth: number, maxHeight: number) => Promise<ThumbnailPullResult>,
): { refreshNow: () => void } {
  // Mutable state kept in refs (not useState) to avoid triggering re-renders.
  const failuresRef  = useRef<number>(0);
  const timeoutRef   = useRef<ReturnType<typeof setTimeout> | null>(null);
  // cancelledRef lets async pull continuations know the effect was torn down.
  const cancelledRef = useRef<boolean>(false);

  // refreshNowRef holds a stable function reference so it can be returned from
  // the hook without becoming a useEffect dependency itself.
  const refreshNowRef = useRef<() => void>(() => { /* initialised below */ });

  useEffect(() => {
    if (!connected || unsupported) {
      return;
    }

    cancelledRef.current = false;
    failuresRef.current  = 0;

    function clearPending(): void {
      if (timeoutRef.current !== null) {
        clearTimeout(timeoutRef.current);
        timeoutRef.current = null;
      }
    }

    function scheduleNext(delayMs: number): void {
      clearPending();
      timeoutRef.current = setTimeout((): void => { void tick(); }, delayMs);
    }

    async function tick(): Promise<void> {
      if (cancelledRef.current) return;

      const result = await pull(THUMBNAIL_MAX_WIDTH, THUMBNAIL_MAX_HEIGHT);

      if (cancelledRef.current) return;

      if (result === 'unsupported') {
        // viewportThumbnailUnsupported flag already dispatched inside pull().
        // Do not reschedule — stop the loop.
        return;
      }

      if (result === 'ok') {
        failuresRef.current = 0;
        scheduleNext(THUMBNAIL_REFRESH_MS);
      } else {
        // 'error': apply exponential back-off, capped at THUMBNAIL_BACKOFF_CAP_MS.
        failuresRef.current += 1;
        const delay = Math.min(
          THUMBNAIL_REFRESH_MS * Math.pow(2, failuresRef.current - 1),
          THUMBNAIL_BACKOFF_CAP_MS,
        );
        scheduleNext(delay);
      }
    }

    // Wire refreshNow: reset counter, pull immediately, loop restarts via tick().
    refreshNowRef.current = (): void => {
      clearPending();
      failuresRef.current = 0;
      void tick();
    };

    // Initial immediate pull on connect/mount.
    void tick();

    return (): void => {
      cancelledRef.current = true;
      clearPending();
      // Reset refreshNow to a no-op while the loop is torn down.
      refreshNowRef.current = (): void => { /* loop not running */ };
    };
  }, [connected, unsupported, pull]);

  const refreshNow = useCallback((): void => {
    refreshNowRef.current();
  }, []);

  return { refreshNow };
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
  VIEWPORT_GET_THUMBNAIL_FAILED: 'Thumbnail fetch failed',
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

  const thumbnail            = state.viewportThumbnail;
  const thumbnailUnsupported = state.viewportThumbnailUnsupported === true;

  const getViewportThumbnail = actions.getViewportThumbnail;

  // -----------------------------------------------------------------------
  // Action handlers — delegate to useBridgeActions() (error mapping lives
  // there, in a single place). Engine/runtime controls now live in the main
  // toolbar (ToolbarActions, P3); this panel keeps only the controls tied to
  // its own view: dismiss-error and refresh-thumbnail.
  // -----------------------------------------------------------------------

  const handleDismissError  = (): void => { actions.dismissError(); };

  // pull — stable callback referencing getViewportThumbnail.
  // Passed to useThumbnailAutoRefresh as the async pull function.
  const pull = useCallback(
    (maxWidth: number, maxHeight: number) => getViewportThumbnail(maxWidth, maxHeight),
    [getViewportThumbnail],
  );

  // -----------------------------------------------------------------------
  // Low-frequency auto-refresh with exponential back-off (<= 1 fps,
  // docs/memory-buffer-policy.md). Engine-agnostic: back-off absorbs
  // transient errors without hammering the engine.
  // -----------------------------------------------------------------------
  const { refreshNow } = useThumbnailAutoRefresh(connected, thumbnailUnsupported, pull);

  const handleRefreshThumbnail = (): void => { refreshNow(); };

  // Build the data: URL once per thumbnail change so an unrelated re-render
  // does not churn the <img src> (which would re-decode the image).
  const thumbnailSrc = useMemo<string | undefined>(() => {
    if (thumbnail === undefined) {
      return undefined;
    }
    return `data:${thumbnail.mimeType};base64,${thumbnail.imageBase64}`;
  }, [thumbnail]);

  // Show the live thumbnail only while connected and the engine supports it.
  // Otherwise fall back to the external-window notice (engine-agnostic).
  const showThumbnail = connected && !thumbnailUnsupported && thumbnailSrc !== undefined;

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

        {/* Viewport area — live thumbnail (pull-style, <= 1 fps) when the engine
            supports viewport.getThumbnail; otherwise the external-window notice.
            The viewport status badge (Phase 7a) is shown in both cases. */}
        <div className="placeholder-box">
          {showThumbnail ? (
            <>
              <span className="placeholder-box__title">Engine Viewport (Thumbnail)</span>
              <img
                className="viewport-thumbnail"
                src={thumbnailSrc}
                alt="Engine viewport thumbnail"
                style={{ maxWidth: '100%', height: 'auto', imageRendering: 'auto' }}
              />
            </>
          ) : (
            <>
              <span className="placeholder-box__title">Engine Viewport (External Window)</span>
              <span>
                In alpha, the engine renders in its own window.
                Use &quot;Focus Viewport&quot; to bring it to the foreground.
              </span>
            </>
          )}
          {/* Viewport status badge (Phase 7a) — always shown */}
          <div className="row" style={{ justifyContent: 'center', marginTop: 4 }}>
            <span className="label">Viewport:</span>
            <span className="status-badge status-badge--disconnected">
              <span className="status-badge__dot" />
              {viewportState !== undefined ? VIEWPORT_LABELS[viewportState] : '--'}
            </span>
          </div>
          {/* Manual thumbnail refresh (pull). Disabled while disconnected or
              when the engine reported the method unsupported. */}
          <div className="row" style={{ justifyContent: 'center', marginTop: 4 }}>
            <button
              className="btn"
              disabled={!connected || thumbnailUnsupported}
              onClick={handleRefreshThumbnail}
              title="Fetch a still thumbnail of the engine viewport"
              type="button"
            >
              Refresh Thumbnail
            </button>
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
        {/* Engine/runtime/process controls live in the main toolbar
            (ToolbarActions, P3) — not duplicated here (m1). */}
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
