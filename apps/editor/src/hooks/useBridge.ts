/**
 * useBridge — subscribes to all Tauri bridge events and exposes action
 * callbacks that invoke Tauri commands through @norves/bridge-ui wrappers.
 *
 * Mount this hook ONCE at the application root (inside <BridgeProvider>).
 * It registers event subscriptions on mount and cleans them up on unmount.
 * Safe under React StrictMode double-invoke: each effect run returns its own
 * cleanup that calls the UnlistenFns from that exact subscription set.
 *
 * NOTE: These actions are the substitute for a real GUI round-trip test.
 * Full end-to-end acceptance requires a running Tauri process + engine
 * (see plan §10 manual acceptance).
 */

import { useEffect, useRef, useCallback } from 'react';
import {
  invokeCommand,
  subscribeEvent,
  BRIDGE_COMMANDS,
  BRIDGE_EVENTS,
  type UnlistenFn,
} from '@norves/bridge-ui';
import type {
  ConnectionStatePayload,
} from '@norves/bridge-ui';
import type {
  EngineStatusChangedEvent,
  RuntimeStateChangedEvent,
  LogMessageEvent,
  ErrorReportedEvent,
  EngineProcessExitedEvent,
  ViewportStateChangedEvent,
  GetStatusResult,
} from '@norves/bridge-ui';
import { useBridgeDispatch } from '../state/BridgeContext.js';

// -------------------------------------------------------------------------
// Monotonic log-entry id (simple counter, avoids Date.now/Math.random churn)
// -------------------------------------------------------------------------

let _logIdCounter = 0;
function nextLogId(): number {
  _logIdCounter += 1;
  return _logIdCounter;
}

// -------------------------------------------------------------------------
// BackendError shape (Tauri returns a serde-tagged Err value on failure)
// -------------------------------------------------------------------------

interface BackendErrorPayload {
  kind?: string;
  message?: string;
  [key: string]: unknown;
}

function extractBackendError(err: unknown): { kind?: string; message: string } {
  if (err !== null && typeof err === 'object') {
    const e = err as BackendErrorPayload;
    return {
      kind: typeof e['kind'] === 'string' ? e['kind'] : undefined,
      message: typeof e['message'] === 'string' ? e['message'] : String(err),
    };
  }
  return { message: String(err) };
}

// -------------------------------------------------------------------------
// Hook
// -------------------------------------------------------------------------

export interface BridgeActions {
  connect: (port: number) => Promise<void>;
  disconnect: () => Promise<void>;
  reconnect: () => Promise<void>;
  getStatus: () => Promise<void>;
  play: () => Promise<void>;
  pause: () => Promise<void>;
  stop: () => Promise<void>;
  focusViewport: () => Promise<void>;
}

export function useBridge(): BridgeActions {
  const dispatch = useBridgeDispatch();

  // -----------------------------------------------------------------------
  // Event subscriptions (mount / unmount)
  // -----------------------------------------------------------------------

  // Keep a ref so the cleanup closure always sees the current unlisten list
  // even if the component re-renders between subscribe completion and cleanup.
  const unlistenRef = useRef<UnlistenFn[]>([]);

  useEffect(() => {
    // Each call to subscribeEvent is async; collect them all before the
    // effect cleanup might fire. If the component unmounts before setup
    // completes we still clean up via the 'aborted' flag + unlistenRef.
    let aborted = false;
    const fns: UnlistenFn[] = [];

    async function setup(): Promise<void> {
      const subs = await Promise.all([
        // Connection-state (connect / disconnect / reconnect results relayed as event)
        subscribeEvent<ConnectionStatePayload>(
          BRIDGE_EVENTS.connectionState,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'connectionStateChanged', payload });
            }
          },
        ),

        // engine.statusChanged
        subscribeEvent<EngineStatusChangedEvent>(
          BRIDGE_EVENTS.statusChanged,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'engineStatusChanged', payload });
            }
          },
        ),

        // runtime.stateChanged
        subscribeEvent<RuntimeStateChangedEvent>(
          BRIDGE_EVENTS.runtimeStateChanged,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'runtimeStateChanged', payload });
            }
          },
        ),

        // log.message
        subscribeEvent<LogMessageEvent>(
          BRIDGE_EVENTS.logMessage,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'logAppended', payload, id: nextLogId() });
            }
          },
        ),

        // error.reported
        subscribeEvent<ErrorReportedEvent>(
          BRIDGE_EVENTS.errorReported,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'errorReported', payload });
            }
          },
        ),

        // engine.processExited
        subscribeEvent<EngineProcessExitedEvent>(
          BRIDGE_EVENTS.engineProcessExited,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'engineProcessExited', payload });
            }
          },
        ),

        // bridge.connected (informational — update connection state)
        subscribeEvent<ConnectionStatePayload>(
          BRIDGE_EVENTS.bridgeConnected,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'connectionStateChanged', payload });
            }
          },
        ),

        // bridge.disconnected (informational — update connection state)
        subscribeEvent<ConnectionStatePayload>(
          BRIDGE_EVENTS.bridgeDisconnected,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'connectionStateChanged', payload });
            }
          },
        ),

        // viewport.stateChanged — keep latest viewport state in store
        subscribeEvent<ViewportStateChangedEvent>(
          BRIDGE_EVENTS.viewportStateChanged,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'viewportStateChanged', payload });
            }
          },
        ),
      ]);

      if (aborted) {
        // Cleanup fired before setup finished — unlisten everything we set up.
        for (const fn of subs) fn();
        return;
      }

      fns.push(...subs);
      unlistenRef.current = fns;
    }

    setup().catch((err: unknown) => {
      // Non-fatal: log to console but do NOT throw into React tree.
      console.error('[useBridge] Failed to subscribe to events:', err);
    });

    return () => {
      aborted = true;
      for (const fn of unlistenRef.current) fn();
      unlistenRef.current = [];
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // -----------------------------------------------------------------------
  // Action callbacks
  // -----------------------------------------------------------------------

  const connect = useCallback(async (port: number): Promise<void> => {
    dispatch({ type: 'commandPending' });
    try {
      const result = await invokeCommand<ConnectionStatePayload>(
        BRIDGE_COMMANDS.connect,
        { port },
      );
      dispatch({ type: 'connectionStateChanged', payload: result });
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'CONNECT_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const disconnect = useCallback(async (): Promise<void> => {
    try {
      const result = await invokeCommand<ConnectionStatePayload>(
        BRIDGE_COMMANDS.disconnect,
      );
      dispatch({ type: 'connectionStateChanged', payload: result });
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'DISCONNECT_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const reconnect = useCallback(async (): Promise<void> => {
    dispatch({ type: 'commandPending' });
    try {
      const result = await invokeCommand<ConnectionStatePayload>(
        BRIDGE_COMMANDS.reconnect,
      );
      dispatch({ type: 'connectionStateChanged', payload: result });
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'RECONNECT_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const getStatus = useCallback(async (): Promise<void> => {
    try {
      const result = await invokeCommand<GetStatusResult>(
        BRIDGE_COMMANDS.getStatus,
      );
      dispatch({ type: 'statusUpdated', payload: result });
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'GET_STATUS_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const play = useCallback(async (): Promise<void> => {
    try {
      await invokeCommand(BRIDGE_COMMANDS.runtimePlay);
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'PLAY_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const pause = useCallback(async (): Promise<void> => {
    try {
      await invokeCommand(BRIDGE_COMMANDS.runtimePause);
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'PAUSE_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const stop = useCallback(async (): Promise<void> => {
    try {
      await invokeCommand(BRIDGE_COMMANDS.runtimeStop);
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'STOP_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const focusViewport = useCallback(async (): Promise<void> => {
    try {
      await invokeCommand(BRIDGE_COMMANDS.focusViewport);
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'FOCUS_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  return { connect, disconnect, reconnect, getStatus, play, pause, stop, focusViewport };
}
