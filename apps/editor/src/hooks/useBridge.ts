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
  assetReadManifest,
  assetReloadManifest,
  assetResolve,
  workspaceOpen,
  workspaceGet,
  workspaceClose,
  type UnlistenFn,
} from '@norves/bridge-ui';
import type {
  AssetManifestPayload,
  ConnectionStatePayload,
  WorkspacePayload,
} from '@norves/bridge-ui';
import type {
  EngineStatusChangedEvent,
  RuntimeStateChangedEvent,
  LogMessageEvent,
  ErrorReportedEvent,
  EngineProcessExitedEvent,
  ViewportStateChangedEvent,
  SceneTreeChangedEvent,
  ObjectChangedEvent,
  GetStatusResult,
  SceneGetTreeResult,
  SceneCreateObjectResult,
  SceneDeleteObjectResult,
  SceneReparentObjectResult,
  SceneDuplicateObjectResult,
  ObjectSnapshot,
  SchemaSnapshot,
  SetObjectPropertyResult,
  ViewportThumbnail,
} from '@norves/bridge-ui';
import { useBridgeDispatch, useBridgeState } from '../state/BridgeContext.js';
import { assetKeyForEntry, normalizeOldParentId, propertyValuesEqual } from '../state/store.js';

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

        // scene.treeChanged (protocol 0.2) — best-effort live tree update
        subscribeEvent<SceneTreeChangedEvent>(
          BRIDGE_EVENTS.sceneTreeChanged,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'sceneTreeChangedLive', payload });
            }
          },
        ),

        // object.changed (protocol 0.2) — best-effort live object update
        subscribeEvent<ObjectChangedEvent>(
          BRIDGE_EVENTS.objectChanged,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'objectChangedLive', payload });
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
// Thumbnail pull result type (P7: backoff support)
// -------------------------------------------------------------------------

/**
 * Result returned by getViewportThumbnail.
 *   'ok'          — thumbnail loaded; caller may reset failure counter.
 *   'unsupported' — engine does not implement thumbnails; stop polling.
 *   'error'       — any other error; caller may apply exponential back-off.
 */
export type ThumbnailPullResult = 'ok' | 'unsupported' | 'error';

// -------------------------------------------------------------------------
// Action callbacks hook (safe to call from any panel — no subscriptions)
// -------------------------------------------------------------------------

export interface BridgeActions {
  openWorkspace: (rootPath: string) => Promise<void>;
  getWorkspace: () => Promise<void>;
  closeWorkspace: () => Promise<void>;
  readAssetManifest: (manifestPath: string) => Promise<void>;
  reloadAssetRuntime: () => Promise<void>;
  dismissAssetReloadError: () => void;
  /**
   * Resolve the currently selected asset through asset.resolve and overlay its
   * live health in the store. Late results for no-longer-selected assets are
   * discarded by comparing the request key with the latest selectedAssetKey.
   */
  resolveAsset: (logicalPath: string, kind?: string, variant?: string) => Promise<void>;
  selectAsset: (key: string) => void;
  clearAssetManifest: () => void;
  /** Dismiss (clear) the current asset-manifest error from the store. */
  dismissAssetError: () => void;
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
  createObject: (parentId?: string, kind?: string) => Promise<SceneCreateObjectResult>;
  deleteObject: (objectId: string) => Promise<SceneDeleteObjectResult>;
  reparentObject: (
    objectId: string,
    newParentId?: string,
  ) => Promise<SceneReparentObjectResult>;
  duplicateObject: (
    objectId: string,
    newParentId?: string,
  ) => Promise<SceneDuplicateObjectResult>;
  /**
   * Fetch a single object's property snapshot (object.getSnapshot) for `id` and
   * store it. On METHOD_NOT_SUPPORTED (an engine without object query) this is a
   * graceful degradation (objectSnapshotUnsupported), not a user-facing error;
   * other errors flow through the store like any command.
   */
  getObjectSnapshot: (id: string) => Promise<void>;
  /**
   * Fetch the engine's type-schema descriptors (schema.getSnapshot) and store
   * them. METHOD_NOT_SUPPORTED degrades the same way as getObjectSnapshot.
   */
  getSchemaSnapshot: () => Promise<void>;
  /**
   * Write a single property value on an object (object.setProperty). On an
   * accepted ack the store snapshot is updated with the engine's appliedValue
   * (falling back to the requested value when the engine omits it). Rejects (and
   * reports through the store) on a backend/engine error; resolves with the ack
   * so the caller can surface accepted:false inline. `value` is an arbitrary JSON
   * value (string/number/boolean/null/array/object) — a snapshot copy, never a
   * live engine pointer.
   */
  setObjectProperty: (
    objectId: string,
    property: string,
    value: unknown,
  ) => Promise<SetObjectPropertyResult>;
  /**
   * Fetch a still viewport thumbnail (viewport.getThumbnail, pull-style) and
   * store it. Optional maxWidth/maxHeight cap the size (the engine downscales).
   * On METHOD_NOT_SUPPORTED (an engine without thumbnails) this is a graceful
   * degradation (viewportThumbnailUnsupported), not a user-facing error; other
   * errors flow through the store like any command. Per docs/memory-buffer-policy
   * callers must not poll faster than 1 fps.
   *
   * Returns a ThumbnailPullResult so callers can drive backoff logic:
   *   'ok'          — thumbnail loaded successfully.
   *   'unsupported' — engine does not support thumbnails (stop polling).
   *   'error'       — transient/permanent error (caller may back off).
   */
  getViewportThumbnail: (maxWidth?: number, maxHeight?: number) => Promise<ThumbnailPullResult>;
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
  /**
   * Undo the most recent recorded scene-structure edit (Phase U1; U2 adds
   * setProperty). No-op when the undo stack is empty, the engine is disconnected,
   * scene edit is unsupported, or an undo/redo is already in flight. Issues the
   * inverse scene/object command DIRECTLY (side-effect-free — does not go through
   * the public wrappers and never records history), refreshes the tree, then
   * commits the stack move. For setProperty the inverse re-sets the property to
   * the captured oldValue.
   */
  undo: () => Promise<void>;
  /**
   * Redo the most recently undone scene-structure edit (Phase U1; U2 adds
   * setProperty). No-op under the same guards as undo. Re-issues the FORWARD
   * scene/object command directly and commits the stack move; for create/duplicate
   * the re-created object's new id replaces the stored createdId (id-instability
   * fix). For reparent/setProperty (id-stable) no newId is passed.
   */
  redo: () => Promise<void>;
}

/**
 * Returns the bridge action callbacks. This hook performs NO event
 * subscription, so any number of panels may call it without duplicating
 * subscriptions. Event subscriptions are owned by useBridgeSubscriptions(),
 * mounted once at the application root.
 */
export function useBridgeActions(): BridgeActions {
  const dispatch = useBridgeDispatch();
  const state = useBridgeState();
  const selectedAssetKeyRef = useRef(state.selectedAssetKey);
  selectedAssetKeyRef.current = state.selectedAssetKey;
  // Connection generation guard: a live asset.resolve started on one connection
  // must not apply its result (health or capability) to a different connection
  // after a disconnect/reconnect, even if the same asset stays selected.
  const connectionSessionIdRef = useRef(state.connection.sessionId);
  connectionSessionIdRef.current = state.connection.sessionId;
  // Latest-state ref (B1): a mutable ref that always points at the freshest
  // BridgeState. Updated on every render so callbacks can read the current
  // sceneTree / undoStack / redoStack SYNCHRONOUSLY without a stale closure and
  // without racing a live scene.treeChanged event that updates the reducer.
  const stateRef = useRef(state);
  stateRef.current = state;
  // In-flight guards for undo/redo — mirror SceneOutlinerPanel.refreshInFlightRef.
  // A second undo/redo click while one is issuing is a no-op (avoids double-pop).
  const undoInFlightRef = useRef(false);
  const redoInFlightRef = useRef(false);

  const openWorkspace = useCallback(async (rootPath: string): Promise<void> => {
    try {
      const result = await workspaceOpen(rootPath);
      dispatch({ type: 'workspaceOpened', payload: result });
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({ type: 'workspaceError', payload: { error: { kind, message } } });
    }
  }, [dispatch]);

  const getWorkspace = useCallback(async (): Promise<void> => {
    try {
      const result: WorkspacePayload | null = await workspaceGet();
      if (result === null || result === undefined) {
        dispatch({ type: 'workspaceClosed' });
      } else {
        dispatch({ type: 'workspaceOpened', payload: result });
      }
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({ type: 'workspaceError', payload: { error: { kind, message } } });
    }
  }, [dispatch]);

  const closeWorkspace = useCallback(async (): Promise<void> => {
    try {
      await workspaceClose();
      dispatch({ type: 'workspaceClosed' });
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({ type: 'workspaceError', payload: { error: { kind, message } } });
    }
  }, [dispatch]);

  const readAssetManifest = useCallback(async (manifestPath: string): Promise<void> => {
    try {
      const result: AssetManifestPayload = await assetReadManifest(manifestPath);
      dispatch({ type: 'assetManifestLoaded', payload: result });
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({ type: 'assetManifestError', payload: { error: { kind, message } } });
    }
  }, [dispatch]);

  const reloadAssetRuntime = useCallback(async (): Promise<void> => {
    const currentState = stateRef.current;
    const startSessionId = currentState.connection.sessionId;
    if (
      currentState.connection.status !== 'connected' ||
      startSessionId === undefined ||
      startSessionId.length === 0 ||
      currentState.connection.capabilityNames?.has('asset.reload') !== true ||
      currentState.assetReloadUnsupported
    ) {
      return;
    }

    try {
      const result = await assetReloadManifest();
      if (connectionSessionIdRef.current !== startSessionId) {
        return;
      }
      if (result.accepted) {
        dispatch({ type: 'assetReloadSucceeded' });
      } else {
        dispatch({
          type: 'assetReloadFailed',
          payload: {
            error: {
              kind: 'asset',
              message: 'Engine rejected runtime asset manifest reload.',
            },
          },
        });
      }
    } catch (err: unknown) {
      if (connectionSessionIdRef.current !== startSessionId) {
        return;
      }
      if (isMethodNotSupported(err)) {
        dispatch({ type: 'assetReloadUnsupported' });
        return;
      }
      const { kind, message } = extractBackendError(err);
      dispatch({ type: 'assetReloadFailed', payload: { error: { kind, message } } });
    }
  }, [dispatch]);

  const dismissAssetReloadError = useCallback((): void => {
    dispatch({ type: 'assetReloadErrorDismissed' });
  }, [dispatch]);

  const resolveAsset = useCallback(
    async (logicalPath: string, kind?: string, variant?: string): Promise<void> => {
      const key = assetKeyForEntry({ logicalPath, variant });
      const startSessionId = connectionSessionIdRef.current;
      // True only if the Bridge connection generation is unchanged since this
      // probe started (a reconnect changes sessionId).
      const sameConnection = (): boolean =>
        connectionSessionIdRef.current === startSessionId;
      try {
        const result = await assetResolve(logicalPath, kind, variant);
        // Discard if the selection OR the connection changed while in flight.
        if (selectedAssetKeyRef.current !== key || !sameConnection()) {
          return;
        }
        dispatch({ type: 'assetResolveLoaded', key, result });
      } catch (err: unknown) {
        // METHOD_NOT_SUPPORTED is a connection-wide capability verdict, valid
        // even if the selection changed — but only for THIS connection
        // generation, so drop it if a reconnect happened mid-flight.
        if (isMethodNotSupported(err)) {
          if (sameConnection()) {
            dispatch({ type: 'assetResolveUnsupported' });
          }
          return;
        }
        if (selectedAssetKeyRef.current !== key || !sameConnection()) {
          return;
        }
        const { kind: errorKind, message } = extractBackendError(err);
        // A single asset's live probe failed: record it per-key (shows "未確定"
        // on that row) WITHOUT raising the manifest-level assetError banner.
        dispatch({
          type: 'assetResolveError',
          key,
          payload: { error: { kind: errorKind, message } },
        });
      }
    },
    [dispatch],
  );

  const selectAsset = useCallback((key: string): void => {
    dispatch({ type: 'assetSelected', key });
  }, [dispatch]);

  const clearAssetManifest = useCallback((): void => {
    dispatch({ type: 'assetManifestCleared' });
  }, [dispatch]);

  const dismissAssetError = useCallback((): void => {
    dispatch({ type: 'assetErrorDismissed' });
  }, [dispatch]);

  const connect = useCallback(async (port: number): Promise<void> => {
    dispatch({ type: 'commandPending' });
    try {
      await invokeCommand(BRIDGE_COMMANDS.connect, { port });
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
      await invokeCommand(BRIDGE_COMMANDS.disconnect);
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
      await invokeCommand(BRIDGE_COMMANDS.reconnect);
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

  const createObject = useCallback(
    async (parentId?: string, kind?: string): Promise<SceneCreateObjectResult> => {
      try {
        const args: { parentId?: string; kind?: string } = {};
        if (parentId !== undefined) {
          args.parentId = parentId;
        }
        if (kind !== undefined) {
          args.kind = kind;
        }
        const result = await invokeCommand<SceneCreateObjectResult>(
          BRIDGE_COMMANDS.sceneCreateObject,
          args,
        );
        if (result.accepted) {
          await getSceneTree();
          if (result.newId !== undefined) {
            dispatch({ type: 'objectSelected', id: result.newId });
            // Record an undoable create keyed by the engine-assigned newId. Only
            // recorded when accepted AND an id came back (undo needs the id to
            // delete). parentId/kind are the args we passed so redo re-creates
            // under the same parent.
            dispatch({
              type: 'recordSceneEdit',
              command: {
                kind: 'create',
                createdId: result.newId,
                parentId,
                objectKind: kind,
              },
            });
          }
        }
        return result;
      } catch (err: unknown) {
        if (isMethodNotSupported(err)) {
          dispatch({ type: 'sceneEditUnsupported' });
          return { accepted: false };
        }
        const { kind: errorKind, message } = extractBackendError(err);
        dispatch({
          type: 'errorReported',
          payload: {
            error: { code: errorKind ?? 'SCENE_CREATE_OBJECT_FAILED', message },
          },
        });
        throw err;
      }
    },
    [dispatch, getSceneTree],
  );

  const deleteObject = useCallback(
    async (objectId: string): Promise<SceneDeleteObjectResult> => {
      try {
        const result = await invokeCommand<SceneDeleteObjectResult>(
          BRIDGE_COMMANDS.sceneDeleteObject,
          { objectId },
        );
        if (result.accepted) {
          dispatch({ type: 'sceneObjectDeleted', accepted: true });
          // Delete is NOT undoable in U1: any recorded history becomes
          // unreconstructable (a deleted subtree cannot be re-created), so clear
          // both stacks.
          dispatch({ type: 'sceneEditHistoryCleared' });
          await getSceneTree();
        }
        return result;
      } catch (err: unknown) {
        if (isMethodNotSupported(err)) {
          dispatch({ type: 'sceneEditUnsupported' });
          return { accepted: false };
        }
        const { kind: errorKind, message } = extractBackendError(err);
        dispatch({
          type: 'errorReported',
          payload: {
            error: { code: errorKind ?? 'SCENE_DELETE_OBJECT_FAILED', message },
          },
        });
        throw err;
      }
    },
    [dispatch, getSceneTree],
  );

  const reparentObject = useCallback(
    async (objectId: string, newParentId?: string): Promise<SceneReparentObjectResult> => {
      // B1/B2: capture the object's CURRENT parent SYNCHRONOUSLY, before issuing
      // the command, from the freshest tree (stateRef) — never from the reducer
      // later, which is racy against live scene.treeChanged events. A direct
      // child of the scene root normalizes to undefined (the engine's
      // nullptr=root path), so undo does not pass the root node's id as a parent.
      const freshTree = stateRef.current.sceneTree;
      const oldParentId =
        freshTree !== undefined
          ? normalizeOldParentId(freshTree, objectId)
          : undefined;
      try {
        const args: { objectId: string; newParentId?: string } = { objectId };
        if (newParentId !== undefined) {
          args.newParentId = newParentId;
        }
        const result = await invokeCommand<SceneReparentObjectResult>(
          BRIDGE_COMMANDS.sceneReparentObject,
          args,
        );
        if (result.accepted) {
          await getSceneTree();
          // Record an undoable reparent with the VALUE captured above. Undo
          // reissues reparentObject(objectId, oldParentId); redo reissues
          // reparentObject(objectId, newParentId). The id is stable across both.
          dispatch({
            type: 'recordSceneEdit',
            command: { kind: 'reparent', objectId, oldParentId, newParentId },
          });
        }
        return result;
      } catch (err: unknown) {
        if (isMethodNotSupported(err)) {
          dispatch({ type: 'sceneEditUnsupported' });
          return { accepted: false };
        }
        const { kind: errorKind, message } = extractBackendError(err);
        dispatch({
          type: 'errorReported',
          payload: {
            error: { code: errorKind ?? 'SCENE_REPARENT_OBJECT_FAILED', message },
          },
        });
        throw err;
      }
    },
    [dispatch, getSceneTree],
  );

  const duplicateObject = useCallback(
    async (objectId: string, newParentId?: string): Promise<SceneDuplicateObjectResult> => {
      try {
        const args: { objectId: string; newParentId?: string } = { objectId };
        if (newParentId !== undefined) {
          args.newParentId = newParentId;
        }
        const result = await invokeCommand<SceneDuplicateObjectResult>(
          BRIDGE_COMMANDS.sceneDuplicateObject,
          args,
        );
        if (result.accepted) {
          await getSceneTree();
          if (result.newId !== undefined) {
            dispatch({ type: 'objectSelected', id: result.newId });
            // Record an undoable duplicate keyed by the engine-assigned newId.
            // sourceId is the original object (so redo can re-duplicate it);
            // parentId is the requested newParentId.
            dispatch({
              type: 'recordSceneEdit',
              command: {
                kind: 'duplicate',
                createdId: result.newId,
                sourceId: objectId,
                parentId: newParentId,
              },
            });
          }
        }
        return result;
      } catch (err: unknown) {
        if (isMethodNotSupported(err)) {
          dispatch({ type: 'sceneEditUnsupported' });
          return { accepted: false };
        }
        const { kind: errorKind, message } = extractBackendError(err);
        dispatch({
          type: 'errorReported',
          payload: {
            error: { code: errorKind ?? 'SCENE_DUPLICATE_OBJECT_FAILED', message },
          },
        });
        throw err;
      }
    },
    [dispatch, getSceneTree],
  );
  const getObjectSnapshot = useCallback(async (id: string): Promise<void> => {
    try {
      const result = await invokeCommand<ObjectSnapshot>(
        BRIDGE_COMMANDS.objectGetSnapshot,
        { objectId: id },
      );
      dispatch({ type: 'objectSnapshotLoaded', snapshot: result });
    } catch (err: unknown) {
      // An engine without object query answers METHOD_NOT_SUPPORTED. Treat that
      // as a graceful degradation (engine-agnostic), not a user-facing error.
      if (isMethodNotSupported(err)) {
        dispatch({ type: 'objectSnapshotUnsupported' });
        return;
      }
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'OBJECT_GET_SNAPSHOT_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const getSchemaSnapshot = useCallback(async (): Promise<void> => {
    try {
      const result = await invokeCommand<SchemaSnapshot>(
        BRIDGE_COMMANDS.schemaGetSnapshot,
      );
      dispatch({ type: 'schemaSnapshotLoaded', types: result.types });
    } catch (err: unknown) {
      if (isMethodNotSupported(err)) {
        dispatch({ type: 'objectSnapshotUnsupported' });
        return;
      }
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'SCHEMA_GET_SNAPSHOT_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const setObjectProperty = useCallback(
    async (
      objectId: string,
      property: string,
      value: unknown,
    ): Promise<SetObjectPropertyResult> => {
      // U2: capture the property's CURRENT value SYNCHRONOUSLY, before issuing the
      // command, from the freshest snapshot (stateRef) — never from the reducer
      // later, which is racy against live object.changed events that overwrite
      // objectSnapshot.properties wholesale. Mirrors reparent's B1 oldParent
      // capture. Only used to record an undoable edit; the write itself proceeds
      // regardless of whether an old value is available.
      const priorSnapshot = stateRef.current.objectSnapshot;
      const oldEntry =
        priorSnapshot !== undefined && priorSnapshot.objectId === objectId
          ? priorSnapshot.properties.find((e) => e.name === property)
          : undefined;
      try {
        const result = await invokeCommand<SetObjectPropertyResult>(
          BRIDGE_COMMANDS.objectSetProperty,
          { objectId, property, value },
        );
        if (result.accepted) {
          // Reflect what the engine actually stored. When the engine omits
          // appliedValue, fall back to the value we requested so the snapshot
          // still updates. The cast is safe: the requested value was a valid
          // PropertyValue (the editor only sends JSON-parseable values).
          const applied =
            result.appliedValue !== undefined
              ? result.appliedValue
              : (value as SetObjectPropertyResult['appliedValue']);
          const appliedValue = applied ?? null;
          dispatch({
            type: 'objectPropertyApplied',
            objectId,
            property,
            appliedValue,
          });
          // Record an undoable property edit (U2) only when the old value was
          // known AND the applied value actually differs (skip no-op writes so
          // undo/redo history stays meaningful). newValue is the ENGINE-echoed
          // value, matching what objectPropertyApplied stored above.
          if (
            oldEntry !== undefined &&
            !propertyValuesEqual(oldEntry.value, appliedValue)
          ) {
            dispatch({
              type: 'recordSceneEdit',
              command: {
                kind: 'setProperty',
                objectId,
                property,
                oldValue: oldEntry.value,
                newValue: appliedValue,
              },
            });
          }
        }
        return result;
      } catch (err: unknown) {
        const { kind, message } = extractBackendError(err);
        dispatch({
          type: 'errorReported',
          payload: {
            error: { code: kind ?? 'OBJECT_SET_PROPERTY_FAILED', message },
          },
        });
        throw err;
      }
    },
    [dispatch],
  );

  const getViewportThumbnail = useCallback(
    async (maxWidth?: number, maxHeight?: number): Promise<ThumbnailPullResult> => {
      try {
        const result = await invokeCommand<ViewportThumbnail>(
          BRIDGE_COMMANDS.viewportGetThumbnail,
          { maxWidth, maxHeight },
        );
        dispatch({ type: 'viewportThumbnailLoaded', thumbnail: result });
        return 'ok';
      } catch (err: unknown) {
        // An engine without thumbnails answers METHOD_NOT_SUPPORTED. Treat that
        // as a graceful degradation (engine-agnostic), not a user-facing error:
        // the GameView falls back to the external-window notice.
        if (isMethodNotSupported(err)) {
          dispatch({ type: 'viewportThumbnailUnsupported' });
          return 'unsupported';
        }
        const { kind, message } = extractBackendError(err);
        dispatch({
          type: 'errorReported',
          payload: {
            error: { code: kind ?? 'VIEWPORT_GET_THUMBNAIL_FAILED', message },
          },
        });
        return 'error';
      }
    },
    [dispatch],
  );

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
      await invokeCommand(BRIDGE_COMMANDS.launchEngine);
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

  // -----------------------------------------------------------------------
  // Undo / Redo (Phase U1)
  //
  // S6: undo/redo issue scene commands to compute an inverse (or re-apply a
  // forward op), but they must be SIDE-EFFECT-FREE with respect to the history:
  // they call invokeCommand(...) DIRECTLY (never the public createObject/
  // deleteObject/... wrappers, which would dispatch recordSceneEdit /
  // sceneEditHistoryCleared) and dispatch ONLY the stack-commit action
  // (undoCommitted/redoCommitted) on success, or undoFailed/redoFailed on
  // rejection. This is what keeps an undo-of-create from clearing the redo stack.
  //
  // LIMITATION (U1): id-sharing chains are out of scope. If a re-created object
  // gets a new id and a later undo/redo targets a now-stale id, the engine
  // answers accepted:false; we drop that entry and surface lastError rather than
  // trying to rewrite dependent history. See docs/scene-structure-editing plan.
  // -----------------------------------------------------------------------

  const undo = useCallback(async (): Promise<void> => {
    const current = stateRef.current;
    const top = current.undoStack[current.undoStack.length - 1];
    // No-op guards: empty stack / not connected / edit unsupported / in flight.
    if (
      top === undefined ||
      current.connection.status !== 'connected' ||
      current.sceneEditUnsupported === true ||
      undoInFlightRef.current
    ) {
      return;
    }
    undoInFlightRef.current = true;
    try {
      let accepted = false;
      if (top.kind === 'create' || top.kind === 'duplicate') {
        // Inverse of a create/duplicate is a delete of the created id. Issue the
        // raw command directly (S6) — NOT the public deleteObject wrapper, which
        // would clear both stacks via sceneEditHistoryCleared.
        const result = await invokeCommand<SceneDeleteObjectResult>(
          BRIDGE_COMMANDS.sceneDeleteObject,
          { objectId: top.createdId },
        );
        accepted = result.accepted;
      } else if (top.kind === 'reparent') {
        // Inverse of a reparent is a reparent back to the captured oldParentId
        // (undefined => omit newParentId => the engine's nullptr=root path).
        const args: { objectId: string; newParentId?: string } = {
          objectId: top.objectId,
        };
        if (top.oldParentId !== undefined) {
          args.newParentId = top.oldParentId;
        }
        const result = await invokeCommand<SceneReparentObjectResult>(
          BRIDGE_COMMANDS.sceneReparentObject,
          args,
        );
        accepted = result.accepted;
      } else {
        // top.kind === 'setProperty' (U2; TS narrows here). Inverse is a re-set of
        // the property to the captured oldValue. Issue the raw command directly
        // (S6); on accept, reflect the applied value in the snapshot so the
        // Inspector shows what the engine stored.
        const result = await invokeCommand<SetObjectPropertyResult>(
          BRIDGE_COMMANDS.objectSetProperty,
          { objectId: top.objectId, property: top.property, value: top.oldValue },
        );
        accepted = result.accepted;
        if (accepted) {
          dispatch({
            type: 'objectPropertyApplied',
            objectId: top.objectId,
            property: top.property,
            appliedValue: result.appliedValue ?? top.oldValue,
          });
        }
      }
      if (accepted) {
        // Unlike the forward setProperty edit (which does not refresh the tree),
        // undo/redo here call getSceneTree() via this shared U1 tail; this is
        // harmless and consistent with how create/reparent undo already behaves.
        await getSceneTree();
        dispatch({ type: 'undoCommitted' });
      } else {
        dispatch({
          type: 'undoFailed',
          message: 'Undo was rejected by the engine.',
        });
      }
    } catch (err: unknown) {
      const { message } = extractBackendError(err);
      dispatch({ type: 'undoFailed', message });
    } finally {
      undoInFlightRef.current = false;
    }
  }, [dispatch, getSceneTree]);

  const redo = useCallback(async (): Promise<void> => {
    const current = stateRef.current;
    const top = current.redoStack[current.redoStack.length - 1];
    if (
      top === undefined ||
      current.connection.status !== 'connected' ||
      current.sceneEditUnsupported === true ||
      redoInFlightRef.current
    ) {
      return;
    }
    redoInFlightRef.current = true;
    try {
      if (top.kind === 'create') {
        // Re-issue the forward create directly (S6). The re-created object gets a
        // NEW id; pass it to redoCommitted so a subsequent undo deletes it.
        const args: { parentId?: string; kind?: string } = {};
        if (top.parentId !== undefined) {
          args.parentId = top.parentId;
        }
        if (top.objectKind !== undefined) {
          args.kind = top.objectKind;
        }
        const result = await invokeCommand<SceneCreateObjectResult>(
          BRIDGE_COMMANDS.sceneCreateObject,
          args,
        );
        if (result.accepted) {
          await getSceneTree();
          dispatch({ type: 'redoCommitted', newId: result.newId });
        } else {
          dispatch({
            type: 'redoFailed',
            message: 'Redo was rejected by the engine.',
          });
        }
      } else if (top.kind === 'duplicate') {
        const args: { objectId: string; newParentId?: string } = {
          objectId: top.sourceId,
        };
        if (top.parentId !== undefined) {
          args.newParentId = top.parentId;
        }
        const result = await invokeCommand<SceneDuplicateObjectResult>(
          BRIDGE_COMMANDS.sceneDuplicateObject,
          args,
        );
        if (result.accepted) {
          await getSceneTree();
          dispatch({ type: 'redoCommitted', newId: result.newId });
        } else {
          dispatch({
            type: 'redoFailed',
            message: 'Redo was rejected by the engine.',
          });
        }
      } else if (top.kind === 'reparent') {
        // reparent: re-apply the forward move. The id is stable, so no newId.
        const args: { objectId: string; newParentId?: string } = {
          objectId: top.objectId,
        };
        if (top.newParentId !== undefined) {
          args.newParentId = top.newParentId;
        }
        const result = await invokeCommand<SceneReparentObjectResult>(
          BRIDGE_COMMANDS.sceneReparentObject,
          args,
        );
        if (result.accepted) {
          await getSceneTree();
          dispatch({ type: 'redoCommitted' });
        } else {
          dispatch({
            type: 'redoFailed',
            message: 'Redo was rejected by the engine.',
          });
        }
      } else {
        // top.kind === 'setProperty' (U2; TS narrows here). Re-apply the forward
        // edit: re-set the property to the captured newValue. The id is stable, so
        // no newId is passed to redoCommitted (the entry is pushed back unchanged).
        const result = await invokeCommand<SetObjectPropertyResult>(
          BRIDGE_COMMANDS.objectSetProperty,
          { objectId: top.objectId, property: top.property, value: top.newValue },
        );
        if (result.accepted) {
          dispatch({
            type: 'objectPropertyApplied',
            objectId: top.objectId,
            property: top.property,
            appliedValue: result.appliedValue ?? top.newValue,
          });
          // Shared U1 tail: refresh the tree (harmless for setProperty) then
          // commit the stack move with NO newId (id-stable).
          await getSceneTree();
          dispatch({ type: 'redoCommitted' });
        } else {
          dispatch({
            type: 'redoFailed',
            message: 'Redo was rejected by the engine.',
          });
        }
      }
    } catch (err: unknown) {
      const { message } = extractBackendError(err);
      dispatch({ type: 'redoFailed', message });
    } finally {
      redoInFlightRef.current = false;
    }
  }, [dispatch, getSceneTree]);

  return {
    openWorkspace,
    getWorkspace,
    closeWorkspace,
    readAssetManifest,
    reloadAssetRuntime,
    dismissAssetReloadError,
    resolveAsset,
    selectAsset,
    clearAssetManifest,
    dismissAssetError,
    connect,
    disconnect,
    reconnect,
    getStatus,
    getSceneTree,
    createObject,
    deleteObject,
    reparentObject,
    duplicateObject,
    getObjectSnapshot,
    getSchemaSnapshot,
    setObjectProperty,
    getViewportThumbnail,
    play,
    pause,
    stop,
    focusViewport,
    launch,
    stopProcess,
    dismissError,
    selectObject,
    undo,
    redo,
  };
}
