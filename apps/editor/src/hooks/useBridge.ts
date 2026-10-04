/**
 * Bridge購読と操作を分けたReact hook。
 *
 * `useBridgeSubscriptions()` はアプリのBridgeProvider内で一度だけ使う。
 * 基本イベントはマウント時に購読し、編集サービスイベントは接続中に購読する。
 * cleanupでは各購読の解除関数を呼ぶため、StrictModeの再実行でも購読が残らない。
 *
 * `useBridgeActions()` はTauri command wrapperを呼ぶ操作callbackを返す。
 * イベント購読は行わないため、各パネルから呼び出しても購読は重複しない。
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
  EditAppliedPayload,
  EditDiscardResult,
  EditHistorySummary,
  UiParentCapture,
  UiPropertyCapture,
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
  AddComponentResult,
  RemoveComponentResult,
  ViewportThumbnail,
} from '@norves/bridge-ui';
import { useBridgeDispatch, useBridgeState } from '../state/BridgeContext.js';
import { assetKeyForEntry, normalizeOldParentId } from '../state/store.js';

// -------------------------------------------------------------------------
// ログ行とsnapshot取得を識別する単調増加番号。
// -------------------------------------------------------------------------

let _logIdCounter = 0;
let _snapshotRequestId = 0;
function nextLogId(): number {
  _logIdCounter += 1;
  return _logIdCounter;
}

function nextSnapshotRequestId(): number {
  _snapshotRequestId += 1;
  return _snapshotRequestId;
}

function isEditHistorySummary(value: unknown): value is EditHistorySummary {
  if (value === null || typeof value !== 'object') {
    return false;
  }
  const summary = value as Partial<EditHistorySummary>;
  const pendingGroup = summary.pendingGroup;
  const validPendingGroup =
    pendingGroup === undefined ||
    pendingGroup === null ||
    (typeof pendingGroup === 'object' &&
      typeof pendingGroup.id === 'string' &&
      typeof pendingGroup.name === 'string' &&
      (pendingGroup.direction === 'undo' || pendingGroup.direction === 'redo') &&
      (pendingGroup.source === 'ui' || pendingGroup.source === 'mcp') &&
      typeof pendingGroup.createdAt === 'number' &&
      typeof pendingGroup.totalCount === 'number' &&
      typeof pendingGroup.completedCount === 'number' &&
      typeof pendingGroup.outcomeUnknown === 'boolean' &&
      typeof pendingGroup.retryAllowed === 'boolean');
  return (
    (summary.generation === null || typeof summary.generation === 'number') &&
    typeof summary.historyRevision === 'number' &&
    typeof summary.appliedRevision === 'number' &&
    typeof summary.canUndo === 'boolean' &&
    typeof summary.canRedo === 'boolean' &&
    typeof summary.pending === 'boolean' &&
    validPendingGroup
  );
}

function isEditDiscardResult(value: unknown): value is EditDiscardResult {
  if (value === null || typeof value !== 'object') {
    return false;
  }
  const result = value as Partial<EditDiscardResult>;
  return (
    typeof result.groupId === 'string' &&
    typeof result.completedCount === 'number' &&
    typeof result.totalCount === 'number' &&
    typeof result.outcomeUnknown === 'boolean' &&
    typeof result.changesRemain === 'boolean'
  );
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

export function extractBackendError(err: unknown): { kind?: string; message: string } {
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

function containsSceneNode(root: SceneGetTreeResult['root'], objectId: string): boolean {
  return (
    root.id === objectId ||
    root.children?.some((child) => containsSceneNode(child, objectId)) === true
  );
}

function requiresTargetRefresh(message: string): boolean {
  return message.includes('対象を再取得してから操作してください');
}

// -------------------------------------------------------------------------
// Event subscriptions hook (mount ONCE at the app root)
// -------------------------------------------------------------------------

/**
 * Tauri Bridgeイベントを購読し、unmount時に解除する。値は返さない。
 * アプリのrootで一度だけ呼び、パネルからは呼ばない。
 */
export function useBridgeSubscriptions(): void {
  const dispatch = useBridgeDispatch();
  const state = useBridgeState();
  const stateRef = useRef(state);
  stateRef.current = state;

  // 購読設定中に再描画されても、cleanup が解除関数を参照できるようにする。
  const unlistenRef = useRef<UnlistenFn[]>([]);
  const historyRequestSerialRef = useRef(0);
  const connectionEpochRef = useRef(0);
  const observedGenerationRef = useRef(state.editServiceGeneration);
  const observedRevisionRef = useRef(state.editAppliedRevision);
  const observedHistoryRevisionRef = useRef(state.editHistorySummary?.historyRevision);
  const observedSequenceRef = useRef(state.editSequence);

  useEffect(() => {
    let aborted = false;
    const fns: UnlistenFn[] = [];
    let refreshHistory: (expectedSessionId?: string) => Promise<void> = async () => {};
    let subscribeServiceEvents: () => Promise<void> = async () => {};
    let desiredConnected = stateRef.current.connection.status === 'connected';
    let desiredSessionId = stateRef.current.connection.sessionId;
    let serviceEventsReady = false;
    let serviceEventsPendingSync = false;
    let serviceSubscriptionPending = false;
    let serviceSubscriptionToken = 0;
    let serviceUnlistenFns: UnlistenFn[] = [];

    const resetObservedEditPosition = (): void => {
      observedGenerationRef.current = undefined;
      observedRevisionRef.current = undefined;
      observedHistoryRevisionRef.current = undefined;
      observedSequenceRef.current = undefined;
    };

    const receiveHistory = (summary: EditHistorySummary, fromQuery = false): void => {
      if (aborted || !desiredConnected) {
        return;
      }
      if (!serviceEventsReady && !fromQuery) {
        serviceEventsPendingSync = true;
        return;
      }
      const incomingGeneration = summary.generation ?? undefined;
      const generation = observedGenerationRef.current;
      if (
        generation !== undefined &&
        (incomingGeneration === undefined || incomingGeneration < generation)
      ) {
        return;
      }
      const sameGeneration = generation === incomingGeneration;
      if (
        sameGeneration &&
        observedRevisionRef.current !== undefined &&
        summary.appliedRevision < observedRevisionRef.current
      ) {
        return;
      }
      if (
        sameGeneration &&
        observedHistoryRevisionRef.current !== undefined &&
        summary.historyRevision < observedHistoryRevisionRef.current
      ) {
        return;
      }
      const gap =
        sameGeneration &&
        summary.appliedRevision > (observedRevisionRef.current ?? 0) + 1;
      const needsAnotherSync = gap || (fromQuery && serviceEventsPendingSync);
      if (!sameGeneration) {
        observedSequenceRef.current = undefined;
      }
      observedGenerationRef.current = incomingGeneration;
      observedRevisionRef.current = summary.appliedRevision;
      observedHistoryRevisionRef.current = summary.historyRevision;
      serviceEventsReady = true;
      serviceEventsPendingSync = false;
      dispatch({ type: 'editHistorySummaryReceived', summary });
      if (needsAnotherSync) {
        void refreshHistory();
      }
    };

    refreshHistory = async (expectedSessionId?: string): Promise<void> => {
      const requestSerial = ++historyRequestSerialRef.current;
      const connectionEpoch = connectionEpochRef.current;
      const sessionId = expectedSessionId ?? stateRef.current.connection.sessionId;
      try {
        const summary = await invokeCommand<EditHistorySummary>(BRIDGE_COMMANDS.editGetHistory);
        if (
          aborted ||
          requestSerial !== historyRequestSerialRef.current ||
          connectionEpoch !== connectionEpochRef.current ||
          (sessionId !== undefined &&
            stateRef.current.connection.sessionId !== undefined &&
            stateRef.current.connection.sessionId !== sessionId) ||
          !isEditHistorySummary(summary)
        ) {
          return;
        }
        receiveHistory(summary, true);
      } catch {
        // 履歴要約の取得失敗は、接続状態や既存表示を壊さず次の同期機会を待つ。
      }
    };

    const receiveConnection = (payload: ConnectionStatePayload): void => {
      if (aborted) {
        return;
      }
      const nextSessionId = payload.connected ? payload.sessionId : undefined;
      if (
        !payload.connected ||
        desiredConnected !== payload.connected ||
        desiredSessionId !== nextSessionId
      ) {
        resetObservedEditPosition();
        serviceEventsReady = false;
        serviceEventsPendingSync = false;
      }
      connectionEpochRef.current += 1;
      desiredConnected = payload.connected;
      desiredSessionId = nextSessionId;
      dispatch({ type: 'connectionStateChanged', payload });
      if (payload.connected) {
        if (serviceUnlistenFns.length > 0) {
          void refreshHistory(payload.sessionId);
        } else {
          void subscribeServiceEvents();
        }
      } else {
        serviceSubscriptionToken += 1;
        serviceSubscriptionPending = false;
        for (const fn of serviceUnlistenFns) fn();
        const removed = new Set(serviceUnlistenFns);
        serviceUnlistenFns = [];
        unlistenRef.current = unlistenRef.current.filter((fn) => !removed.has(fn));
      }
    };

    const receiveApplied = (payload: EditAppliedPayload): void => {
      if (aborted || !desiredConnected) {
        return;
      }
      if (!serviceEventsReady) {
        serviceEventsPendingSync = true;
        return;
      }
      const generation = observedGenerationRef.current;
      if (generation !== undefined && payload.generation < generation) {
        return;
      }
      const generationChanged = generation !== undefined && payload.generation !== generation;
      const sameGeneration = generation === payload.generation;
      const previousRevision = sameGeneration ? observedRevisionRef.current : undefined;
      if (
        sameGeneration &&
        previousRevision !== undefined &&
        payload.appliedRevision <= previousRevision
      ) {
        return;
      }
      if (
        sameGeneration &&
        observedSequenceRef.current !== undefined &&
        payload.sequence <= observedSequenceRef.current
      ) {
        return;
      }
      if (
        sameGeneration &&
        observedHistoryRevisionRef.current !== undefined &&
        payload.historyRevision < observedHistoryRevisionRef.current
      ) {
        void refreshHistory();
        return;
      }
      const gap =
        generationChanged ||
        (sameGeneration && previousRevision !== undefined && payload.appliedRevision > previousRevision + 1) ||
        (!sameGeneration && generation === undefined && payload.appliedRevision > 1);
      if (!sameGeneration) {
        observedHistoryRevisionRef.current = undefined;
      }
      observedGenerationRef.current = payload.generation;
      observedRevisionRef.current = payload.appliedRevision;
      observedHistoryRevisionRef.current = payload.historyRevision;
      observedSequenceRef.current = payload.sequence;
      dispatch({ type: 'editApplied', payload });
      if (gap || generationChanged) {
        void refreshHistory();
      }
    };

    subscribeServiceEvents = async (): Promise<void> => {
      if (!desiredConnected || serviceSubscriptionPending || serviceUnlistenFns.length > 0) {
        return;
      }
      serviceSubscriptionPending = true;
      const token = ++serviceSubscriptionToken;
      try {
        const subs = await Promise.all([
          subscribeEvent<EditAppliedPayload>(BRIDGE_EVENTS.editApplied, receiveApplied),
          subscribeEvent<EditHistorySummary>(BRIDGE_EVENTS.editHistoryChanged, receiveHistory),
        ]);
        if (aborted || token !== serviceSubscriptionToken || !desiredConnected) {
          for (const fn of subs) fn();
          return;
        }
        serviceUnlistenFns = subs;
        unlistenRef.current = [...unlistenRef.current, ...subs];
        serviceSubscriptionPending = false;
        void refreshHistory(desiredSessionId);
      } catch (err: unknown) {
        if (token === serviceSubscriptionToken) {
          serviceSubscriptionPending = false;
        }
        console.error('[useBridgeSubscriptions] 編集サービス購読に失敗しました:', err);
      }
    };

    async function setup(): Promise<void> {
      const subs = await Promise.all([
        subscribeEvent<ConnectionStatePayload>(
          BRIDGE_EVENTS.connectionState,
          receiveConnection,
        ),

        subscribeEvent<EngineStatusChangedEvent>(
          BRIDGE_EVENTS.statusChanged,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'engineStatusChanged', payload });
            }
          },
        ),

        subscribeEvent<RuntimeStateChangedEvent>(
          BRIDGE_EVENTS.runtimeStateChanged,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'runtimeStateChanged', payload });
            }
          },
        ),

        subscribeEvent<LogMessageEvent>(
          BRIDGE_EVENTS.logMessage,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'logAppended', payload, id: nextLogId() });
            }
          },
        ),

        subscribeEvent<ErrorReportedEvent>(
          BRIDGE_EVENTS.errorReported,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'errorReported', payload });
            }
          },
        ),

        subscribeEvent<EngineProcessExitedEvent>(
          BRIDGE_EVENTS.engineProcessExited,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'engineProcessExited', payload });
            }
          },
        ),

        subscribeEvent<ConnectionStatePayload>(
          BRIDGE_EVENTS.bridgeConnected,
          receiveConnection,
        ),

        subscribeEvent<ConnectionStatePayload>(
          BRIDGE_EVENTS.bridgeDisconnected,
          receiveConnection,
        ),

        subscribeEvent<ViewportStateChangedEvent>(
          BRIDGE_EVENTS.viewportStateChanged,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'viewportStateChanged', payload });
            }
          },
        ),

        subscribeEvent<SceneTreeChangedEvent>(
          BRIDGE_EVENTS.sceneTreeChanged,
          (payload) => {
            if (!aborted) {
              dispatch({ type: 'sceneTreeChangedLive', payload });
            }
          },
        ),

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
        for (const fn of subs) fn();
        return;
      }

      fns.push(...subs);
      unlistenRef.current = [...fns, ...serviceUnlistenFns];
      if (desiredConnected) {
        void subscribeServiceEvents();
      }
    }

    setup().catch((err: unknown) => {
      console.error('[useBridgeSubscriptions] Failed to subscribe to events:', err);
    });

    return () => {
      aborted = true;
      serviceSubscriptionToken += 1;
      historyRequestSerialRef.current += 1;
      connectionEpochRef.current += 1;
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
   * Fetch the property snapshot of one component (object.getSnapshot on the
   * component's opaque id, taken from the entity snapshot's `components`) and
   * store it apart from the entity snapshot, so drilling into a component does
   * not drop the list it was chosen from.
   */
  getComponentSnapshot: (id: string) => Promise<void>;
  /**
   * Attach a component of `kind` to the object `objectId` (component.add) and,
   * on an accepted call, re-read that object's snapshot so the component list
   * reflects the engine's own view. Resolves with whether the engine accepted.
   * A no-op (resolves false) when the engine does not advertise `component.edit`.
   */
  addComponent: (objectId: string, kind: string) => Promise<boolean>;
  /**
   * Detach the component `componentId` (component.remove) and re-read the owning
   * object's snapshot on acceptance. `ownerObjectId` is the object to re-read —
   * the editor never parses the component id to find it. Resolves with whether
   * the engine accepted.
   */
  removeComponent: (componentId: string, ownerObjectId: string) => Promise<boolean>;
  /**
   * Select a component of the currently selected object, or clear the selection
   * with undefined to go back to the object's own properties.
   */
  selectComponent: (id: string | undefined) => void;
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
  /** 履歴要約の先頭ID・改訂を指定して、編集サービスへ取り消しを依頼する。 */
  undo: () => Promise<void>;
  /** 履歴要約の先頭ID・改訂を指定して、編集サービスへやり直しを依頼する。 */
  redo: () => Promise<void>;
  /** 保留中のまとまりの未処理部分だけを再試行する。 */
  retryPendingEdit?: () => Promise<void>;
  /** 保留中のまとまりを破棄し、その結果を画面へ残す。 */
  discardPendingEdit?: () => Promise<void>;
  /** 編集サービスから最新の履歴要約を取得する。 */
  refreshEditHistory?: () => Promise<void>;
  /** 破棄結果の通知を閉じる。 */
  dismissEditDiscardResult?: () => void;
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
  // 最新状態のref。callbackが最新のsceneTreeやsnapshotを同期的に読み、
  // liveイベントとの競合前に画面の捕捉値を作れるようにする。
  const stateRef = useRef(state);
  stateRef.current = state;
  // undo/redo実行中の二重送信を防ぐ。
  const undoInFlightRef = useRef(false);
  const redoInFlightRef = useRef(false);
  const pendingActionInFlightRef = useRef(false);

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

  const refreshEditHistory = useCallback(async (): Promise<void> => {
    const requestGeneration = stateRef.current.connection.generation;
    try {
      const summary = await invokeCommand<EditHistorySummary>(BRIDGE_COMMANDS.editGetHistory);
      const current = stateRef.current;
      if (
        isEditHistorySummary(summary) &&
        current.connection.status === 'connected' &&
        current.connection.generation === requestGeneration
      ) {
        dispatch({ type: 'editHistorySummaryReceived', summary });
      }
    } catch {
      // 履歴要約の取得失敗では、現在の表示状態を変更しない。
    }
  }, [dispatch]);

  const retryPendingEdit = useCallback(async (): Promise<void> => {
    const current = stateRef.current;
    const pendingGroup = current.editHistorySummary?.pendingGroup;
    if (
      pendingActionInFlightRef.current ||
      current.connection.status !== 'connected' ||
      current.editHistorySummary?.pending !== true ||
      pendingGroup === undefined ||
      pendingGroup === null ||
      pendingGroup.outcomeUnknown ||
      !pendingGroup.retryAllowed
    ) {
      return;
    }
    pendingActionInFlightRef.current = true;
    try {
      await invokeCommand(BRIDGE_COMMANDS.editRetry);
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: { error: { code: kind ?? 'EDIT_RETRY_FAILED', message } },
      });
    } finally {
      await refreshEditHistory();
      pendingActionInFlightRef.current = false;
    }
  }, [dispatch, refreshEditHistory]);

  const discardPendingEdit = useCallback(async (): Promise<void> => {
    const current = stateRef.current;
    if (
      pendingActionInFlightRef.current ||
      current.connection.status !== 'connected' ||
      current.editHistorySummary?.pending !== true
    ) {
      return;
    }
    pendingActionInFlightRef.current = true;
    try {
      const result = await invokeCommand<EditDiscardResult>(BRIDGE_COMMANDS.editDiscard);
      if (isEditDiscardResult(result)) {
        dispatch({ type: 'editDiscardResultReceived', result });
      } else {
        dispatch({
          type: 'errorReported',
          payload: {
            error: {
              code: 'EDIT_DISCARD_INVALID_RESULT',
              message: '破棄結果を読み取れませんでした。履歴の状態を確認してください。',
            },
          },
        });
      }
    } catch (err: unknown) {
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: { error: { code: kind ?? 'EDIT_DISCARD_FAILED', message } },
      });
    } finally {
      await refreshEditHistory();
      pendingActionInFlightRef.current = false;
    }
  }, [dispatch, refreshEditHistory]);

  const dismissEditDiscardResult = useCallback((): void => {
    dispatch({ type: 'editDiscardResultDismissed' });
  }, [dispatch]);

  const getSceneTree = useCallback(async (): Promise<void> => {
    const requestId = nextSnapshotRequestId();
    const requestState = stateRef.current;
    const request = {
      requestId,
      connectionGeneration: requestState.connection.generation,
      editServiceGeneration: requestState.editServiceGeneration,
      appliedRevision: requestState.editAppliedRevision ?? 0,
    };
    dispatch({ type: 'sceneTreeFetchStarted', requestId });
    try {
      const result = await invokeCommand<SceneGetTreeResult>(
        BRIDGE_COMMANDS.sceneGetTree,
      );
      dispatch({ type: 'sceneTreeLoaded', root: result.root, ...request });
    } catch (err: unknown) {
      if (
        stateRef.current.sceneTreeRequestId !== undefined &&
        stateRef.current.sceneTreeRequestId !== requestId
      ) {
        return;
      }
      // scene.getTree 未対応はエンジン差として扱い、画面エラーにはしない。
      if (isMethodNotSupported(err)) {
        dispatch({ type: 'sceneTreeUnsupported', ...request });
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
      if (stateRef.current.editHistorySummary?.pending === true) {
        return { accepted: false };
      }
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
      if (stateRef.current.editHistorySummary?.pending === true) {
        return { accepted: false };
      }
      try {
        const result = await invokeCommand<SceneDeleteObjectResult>(
          BRIDGE_COMMANDS.sceneDeleteObject,
          { objectId },
        );
        if (result.accepted) {
          dispatch({ type: 'sceneObjectDeleted', accepted: true });
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
      if (stateRef.current.editHistorySummary?.pending === true) {
        return { accepted: false };
      }
      // 親IDと、そのツリーが反映している世代・改訂を呼び出し前に捕捉する。
      const captureState = stateRef.current;
      const freshTree = captureState.sceneTree;
      const targetIsInTree = freshTree !== undefined && containsSceneNode(freshTree, objectId);
      const oldParentId =
        freshTree !== undefined && targetIsInTree
          ? normalizeOldParentId(freshTree, objectId)
          : undefined;
      const capture: UiParentCapture | undefined =
        targetIsInTree &&
        captureState.sceneTreeAppliedGeneration !== undefined &&
        captureState.sceneTreeAppliedRevision !== undefined
          ? {
              generation: captureState.sceneTreeAppliedGeneration,
              revision: captureState.sceneTreeAppliedRevision,
              parentId: oldParentId ?? null,
            }
          : undefined;
      try {
        const args: {
          objectId: string;
          newParentId?: string;
          capture?: UiParentCapture;
        } = { objectId };
        if (newParentId !== undefined) {
          args.newParentId = newParentId;
        }
        if (capture !== undefined) {
          args.capture = capture;
        }
        const result = await invokeCommand<SceneReparentObjectResult>(
          BRIDGE_COMMANDS.sceneReparentObject,
          args,
        );
        if (result.accepted) {
          await getSceneTree();
        }
        return result;
      } catch (err: unknown) {
        const { kind: errorKind, message } = extractBackendError(err);
        if (requiresTargetRefresh(message)) {
          await getSceneTree();
          dispatch({ type: 'editRefreshRequired', message });
          throw err;
        }
        if (isMethodNotSupported(err)) {
          dispatch({ type: 'sceneEditUnsupported' });
          return { accepted: false };
        }
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
      if (stateRef.current.editHistorySummary?.pending === true) {
        return { accepted: false };
      }
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
    const requestId = nextSnapshotRequestId();
    const requestState = stateRef.current;
    const request = {
      requestId,
      connectionGeneration: requestState.connection.generation,
      editServiceGeneration: requestState.editServiceGeneration,
      appliedRevision: requestState.editAppliedRevision ?? 0,
    };
    dispatch({ type: 'objectSnapshotFetchStarted', requestId, objectId: id });
    try {
      const result = await invokeCommand<ObjectSnapshot>(
        BRIDGE_COMMANDS.objectGetSnapshot,
        { objectId: id },
      );
      dispatch({ type: 'objectSnapshotLoaded', snapshot: result, ...request });
    } catch (err: unknown) {
      if (
        stateRef.current.objectSnapshotRequestId !== undefined &&
        stateRef.current.objectSnapshotRequestId !== requestId
      ) {
        return;
      }
      // object.getSnapshot 未対応はエンジン差として扱い、画面エラーにはしない。
      if (isMethodNotSupported(err)) {
        dispatch({ type: 'objectSnapshotUnsupported', objectId: id, ...request });
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

  const getComponentSnapshot = useCallback(async (id: string): Promise<void> => {
    const requestId = nextSnapshotRequestId();
    const requestState = stateRef.current;
    const request = {
      requestId,
      connectionGeneration: requestState.connection.generation,
      editServiceGeneration: requestState.editServiceGeneration,
      appliedRevision: requestState.editAppliedRevision ?? 0,
    };
    dispatch({ type: 'componentSnapshotFetchStarted', requestId, componentId: id });
    try {
      const result = await invokeCommand<ObjectSnapshot>(
        BRIDGE_COMMANDS.objectGetSnapshot,
        { objectId: id },
      );
      dispatch({ type: 'componentSnapshotLoaded', snapshot: result, ...request });
    } catch (err: unknown) {
      if (
        stateRef.current.componentSnapshotRequestId !== undefined &&
        stateRef.current.componentSnapshotRequestId !== requestId
      ) {
        return;
      }
      // 同じsnapshot取得を選択中オブジェクトにも使うため、ここでの失敗は画面へ通知する。
      const { kind, message } = extractBackendError(err);
      dispatch({
        type: 'errorReported',
        payload: {
          error: { code: kind ?? 'OBJECT_GET_SNAPSHOT_FAILED', message },
        },
      });
    }
  }, [dispatch]);

  const selectComponent = useCallback((id: string | undefined): void => {
    dispatch({ type: 'componentSelected', id });
  }, [dispatch]);

  // Component structure edits live behind the `component.edit` capability, which
  // is per-connection: an engine that does not advertise it gets no calls at all
  // (rather than a METHOD_NOT_SUPPORTED round trip), and the session id captured
  // before the call guards a result that lands after a reconnect.
  const editComponents = useCallback(
    async (
      run: () => Promise<{ accepted: boolean }>,
      ownerObjectId: string,
      errorCode: string,
    ): Promise<boolean> => {
      const currentState = stateRef.current;
      const startSessionId = currentState.connection.sessionId;
      if (
        currentState.editHistorySummary?.pending === true ||
        currentState.connection.status !== 'connected' ||
        startSessionId === undefined ||
        currentState.connection.capabilityNames?.has('component.edit') !== true
      ) {
        return false;
      }
      try {
        const result = await run();
        if (connectionSessionIdRef.current !== startSessionId) {
          return false;
        }
        if (result.accepted) {
          // The engine is the authority on what the object now holds; re-read it
          // instead of patching the list from the ack.
          await getObjectSnapshot(ownerObjectId);
        }
        return result.accepted;
      } catch (err: unknown) {
        if (connectionSessionIdRef.current !== startSessionId) {
          return false;
        }
        const { kind, message } = extractBackendError(err);
        dispatch({
          type: 'errorReported',
          payload: { error: { code: kind ?? errorCode, message } },
        });
        return false;
      }
    },
    [dispatch, getObjectSnapshot],
  );

  const addComponent = useCallback(
    async (objectId: string, kind: string): Promise<boolean> =>
      editComponents(
        () =>
          invokeCommand<AddComponentResult>(BRIDGE_COMMANDS.componentAdd, { objectId, kind }),
        objectId,
        'COMPONENT_ADD_FAILED',
      ),
    [editComponents],
  );

  const removeComponent = useCallback(
    async (componentId: string, ownerObjectId: string): Promise<boolean> => {
      const accepted = await editComponents(
        () =>
          invokeCommand<RemoveComponentResult>(BRIDGE_COMMANDS.componentRemove, {
            objectId: componentId,
          }),
        ownerObjectId,
        'COMPONENT_REMOVE_FAILED',
      );
      // Clearing the selection here would also unselect a DIFFERENT component
      // when this one is detached. The re-read that editComponents performs on
      // acceptance already drops a selection whose component is gone from the
      // object's new snapshot, so nothing is needed here.
      return accepted;
    },
    [dispatch, editComponents],
  );

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
      if (stateRef.current.editHistorySummary?.pending === true) {
        return { accepted: false };
      }
      // 旧値とsnapshotの世代・改訂をTauri呼び出し前に捕捉する。
      const captureState = stateRef.current;
      const priorSnapshot =
        captureState.objectSnapshot?.objectId === objectId
          ? captureState.objectSnapshot
          : captureState.componentSnapshot?.objectId === objectId
            ? captureState.componentSnapshot
            : undefined;
      const snapshotGeneration =
        priorSnapshot === captureState.objectSnapshot
          ? captureState.objectSnapshotAppliedGeneration
          : captureState.componentSnapshotAppliedGeneration;
      const snapshotRevision =
        priorSnapshot === captureState.objectSnapshot
          ? captureState.objectSnapshotAppliedRevision
          : captureState.componentSnapshotAppliedRevision;
      const oldEntry =
        priorSnapshot !== undefined
          ? priorSnapshot.properties.find((entry) => entry.name === property)
          : undefined;
      const capture: UiPropertyCapture | undefined =
        oldEntry !== undefined &&
        snapshotGeneration !== undefined &&
        snapshotRevision !== undefined
          ? {
              generation: snapshotGeneration,
              revision: snapshotRevision,
              value: oldEntry.value,
            }
          : undefined;
      try {
        const args: {
          objectId: string;
          property: string;
          value: unknown;
          capture?: UiPropertyCapture;
        } = { objectId, property, value };
        if (capture !== undefined) {
          args.capture = capture;
        }
        const result = await invokeCommand<SetObjectPropertyResult>(
          BRIDGE_COMMANDS.objectSetProperty,
          args,
        );
        if (result.accepted) {
          // エンジンの適用値を表示へ反映する。応答に適用値が無ければ、
          // 画面から送った値を使う。
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
        }
        return result;
      } catch (err: unknown) {
        const { kind, message } = extractBackendError(err);
        if (requiresTargetRefresh(message)) {
          if (captureState.selectedComponentId === objectId) {
            await getComponentSnapshot(objectId);
          } else {
            await getObjectSnapshot(objectId);
          }
          dispatch({ type: 'editRefreshRequired', message });
          throw err;
        }
        dispatch({
          type: 'errorReported',
          payload: {
            error: { code: kind ?? 'OBJECT_SET_PROPERTY_FAILED', message },
          },
        });
        throw err;
      }
    },
    [dispatch, getComponentSnapshot, getObjectSnapshot],
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
    if (stateRef.current.editHistorySummary?.pending === true) return;
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
    if (stateRef.current.editHistorySummary?.pending === true) return;
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
    if (stateRef.current.editHistorySummary?.pending === true) return;
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
  // 取り消しとやり直しの正本は編集サービス。画面は履歴要約にある
  // 先頭ID・改訂を指定してTauri commandを呼び、イベントで表示を更新する。
  // -----------------------------------------------------------------------

  const undo = useCallback(async (): Promise<void> => {
    const current = stateRef.current;
    const history = current.editHistorySummary;
    if (
      history === undefined ||
      !history.canUndo ||
      history.undoHeadId === null ||
      history.pending ||
      current.connection.status !== 'connected' ||
      current.sceneEditUnsupported === true ||
      undoInFlightRef.current
    ) {
      return;
    }
    undoInFlightRef.current = true;
    try {
      await invokeCommand(BRIDGE_COMMANDS.editUndo, {
        expectedHeadId: history.undoHeadId,
        expectedRevision: history.undoRevision,
      });
    } catch (err: unknown) {
      dispatch({ type: 'undoFailed', message: extractBackendError(err).message });
    } finally {
      await refreshEditHistory();
      undoInFlightRef.current = false;
    }
  }, [dispatch, refreshEditHistory]);

  const redo = useCallback(async (): Promise<void> => {
    const current = stateRef.current;
    const history = current.editHistorySummary;
    if (
      history === undefined ||
      !history.canRedo ||
      history.redoHeadId === null ||
      history.pending ||
      current.connection.status !== 'connected' ||
      current.sceneEditUnsupported === true ||
      redoInFlightRef.current
    ) {
      return;
    }
    redoInFlightRef.current = true;
    try {
      await invokeCommand(BRIDGE_COMMANDS.editRedo, {
        expectedHeadId: history.redoHeadId,
        expectedRevision: history.redoRevision,
      });
    } catch (err: unknown) {
      dispatch({ type: 'redoFailed', message: extractBackendError(err).message });
    } finally {
      await refreshEditHistory();
      redoInFlightRef.current = false;
    }
  }, [dispatch, refreshEditHistory]);

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
    getComponentSnapshot,
    selectComponent,
    addComponent,
    removeComponent,
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
    retryPendingEdit,
    discardPendingEdit,
    refreshEditHistory,
    dismissEditDiscardResult,
  };
}
