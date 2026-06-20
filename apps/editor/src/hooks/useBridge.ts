/**
 * Bridge hooks — split into subscriptions and actions.
 *
 * `useBridgeSubscriptions()` registers all Tauri bridge event subscriptions.
 * Mount it ONCE at the application root (inside <BridgeProvider>). It registers
 * subscriptions on mount and cleans them up on unmount. Safe under React
 * StrictMode double-invoke: each effect run returns its own cleanup that calls
 * the UnlistenFns from that exact subscription set.
 *
 * `useBridgeActions()` returns the action callbacks that invoke Tauri commands
 * through @norves/bridge-ui wrappers. It performs NO event subscription, so it
 * is safe to call from any number of panels without duplicating subscriptions.
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
  SceneGetTreeResult,
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
//
// This is the SINGLE source of truth for backend-error extraction. Panels
// must NOT re-implement it; they obtain actions via useBridgeActions().
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

/**
 * Returns true when `err` is an engine protocol error whose stable `code` is
 * METHOD_NOT_SUPPORTED. The Rust BackendError::Engine variant serializes as
 * { kind: "engine", code, message }, so the engine code lives in `code`
 * (extractBackendError only surfaces `kind`/`message`). Engine-agnostic: any
 * engine that does not implement an optional method answers this way.
 */
function isMethodNotSupported(err: unknown): boolean {
  if (err !== null && typeof err === 'object') {
    const e = err as BackendErrorPayload;
    return e['kind'] === 'engine' && e['code'] === 'METHOD_NOT_SUPPORTED';
  }
  return false;
}

// -------------------------------------------------------------------------
// Event subscriptions hook (mount ONCE at the app root)
// -------------------------------------------------------------------------

/**
 * Registers all Tauri bridge event subscriptions and tears them down on
 * unmount. Returns nothing. Mount this exactly once at the application root;
 * panels must NOT call this hook (that would duplicate subscriptions).
 */
export function useBridgeSubscriptions(): void {
  const dispatch = useBridgeDispatch();

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
      console.error('[useBridgeSubscriptions] Failed to subscribe to events:', err);
    });

    return () => {
      aborted = true;
      for (const fn of unlistenRef.current) fn();
      unlistenRef.current = [];
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}

// -------------------------------------------------------------------------
// Action callbacks hook (safe to call from any panel — no subscriptions)
// -------------------------------------------------------------------------

export interface BridgeActions {
  connect: (port: number) => Promise<void>;
  disconnect: () => Promise<void>;
  reconnect: () => Promise<void>;
  getStatus: () => Promise<void>;
  /**
   * Fetch the engine's scene tree (scene.getTree) and store its root.
   * On an engine error (e.g. METHOD_NOT_SUPPORTED for an engine without scene
   * query) the error is reported through the store like any other command.
   */
  getSceneTree: () => Promise<void>;
  play: () => Promise<void>;
  pause: () => Promise<void>;
  stop: () => Promise<void>;
  focusViewport: () => Promise<void>;
  /** Spawn a new engine process and connect to it (Workstream J). */
  launch: () => Promise<void>;
  /** Terminate the running engine process (Workstream J). */
  stopProcess: () => Promise<void>;
  /** Dismiss (clear) the current lastError from the store. */
  dismissError: () => void;
  /**
   * Select a scene object by id. Pass undefined to deselect.
   * Engine-agnostic: id is a plain string token, not mock-specific.
   */
  selectObject: (id: string | undefined) => void;
}

/**
 * Returns the bridge action callbacks. This hook performs NO event
 * subscription, so any number of panels may call it without duplicating
 * subscriptions. Event subscriptions are owned by useBridgeSubscriptions(),
 * mounted once at the application root.
 */
export function useBridgeActions(): BridgeActions {
  const dispatch = useBridgeDispatch();

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

  const getSceneTree = useCallback(async (): Promise<void> => {
    try {
      const result = await invokeCommand<SceneGetTreeResult>(
        BRIDGE_COMMANDS.sceneGetTree,
      );
      dispatch({ type: 'sceneTreeLoaded', root: result.root });
    } catch (err: unknown) {
      // An engine without scene query answers METHOD_NOT_SUPPORTED. Treat that
      // as a graceful degradation (engine-agnostic), not a user-facing error.
      if (isMethodNotSupported(err)) {
        dispatch({ type: 'sceneTreeUnsupported' });
        return;
      }
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'SCENE_GET_TREE_FAILED', message },
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

  const launch = useCallback(async (): Promise<void> => {
    dispatch({ type: 'commandPending' });
    try {
      const result = await invokeCommand<ConnectionStatePayload>(
        BRIDGE_COMMANDS.launchEngine,
      );
      dispatch({ type: 'connectionStateChanged', payload: result });
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'LAUNCH_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const stopProcess = useCallback(async (): Promise<void> => {
    try {
      await invokeCommand(BRIDGE_COMMANDS.stopEngine);
      // The backend will emit a disconnected connectionState event which updates
      // the store. No optimistic dispatch needed; the event subscription handles it.
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'STOP_PROCESS_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const dismissError = useCallback((): void => {
    dispatch({ type: 'dismissError' });
  }, [dispatch]);

  const selectObject = useCallback((id: string | undefined): void => {
    dispatch({ type: 'objectSelected', id });
  }, [dispatch]);

  return { connect, disconnect, reconnect, getStatus, getSceneTree, play, pause, stop, focusViewport, launch, stopProcess, dismissError, selectObject };
}
