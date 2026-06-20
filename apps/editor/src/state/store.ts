/**
 * UI state store for NorvesEditor — bridge/engine/log state.
 *
 * Architecture: React Context + useReducer (no external state libs).
 * The reducer is pure and fully unit-testable.
 *
 * ConnectionStatus is a UI-level concept derived from:
 *   - ConnectionStatePayload.connected (from Tauri command returns / connection-state event)
 *   - In-flight command state (commandPending / commandSettled actions)
 */

import type {
  EngineState,
  RuntimeState,
  LogLevel,
  ViewportState,
  EngineStatusChangedEvent,
  RuntimeStateChangedEvent,
  LogMessageEvent,
  ErrorReportedEvent,
  EngineProcessExitedEvent,
  ViewportStateChangedEvent,
  GetStatusResult,
  SceneNode,
  ObjectSnapshot,
  TypeDescriptor,
} from '@norves/bridge-ui';
import type { ConnectionStatePayload } from '@norves/bridge-ui';

// -------------------------------------------------------------------------
// Derived connection status (UI layer concept)
// -------------------------------------------------------------------------

export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'error';

// -------------------------------------------------------------------------
// Log entry (client-side augmented)
// -------------------------------------------------------------------------

export interface LogEntry {
  /** Monotonic client-side id — used as React key. */
  id: number;
  level: LogLevel;
  message: string;
  category?: string;
  /** ISO-8601 timestamp from the wire, or undefined. */
  timestamp?: string;
}

// -------------------------------------------------------------------------
// Backend error shape (Tauri BackendError serde-tagged)
// -------------------------------------------------------------------------

export interface BackendError {
  kind?: string;
  message: string;
}

// -------------------------------------------------------------------------
// Store state
// -------------------------------------------------------------------------

export interface BridgeState {
  connection: {
    status: ConnectionStatus;
    sessionId?: string;
    serverName?: string;
    endpoint?: string;
    reason?: string;
  };
  engineState?: EngineState;
  runtimeState?: RuntimeState;
  /** Latest viewport state received from the engine. No panel renders this yet (reserved for Workstream K). */
  viewportState?: ViewportState;
  engineName?: string;
  engineVersion?: string;
  title?: string;
  logs: LogEntry[];
  lastError?: BackendError;
  /**
   * ID of the currently selected scene object, or undefined when nothing is selected.
   * Generic string — not tied to any specific engine or mock.
   * Set via objectSelected action; undefined means deselected.
   */
  selectedObjectId?: string;
  /**
   * Root node of the latest scene.getTree snapshot, or undefined before any
   * tree has been fetched (or after disconnect). The wire shape is a single
   * root SceneNode ({ root: SceneNode }); we store the root directly.
   * Generic — carries no engine-specific assumptions.
   */
  sceneTree?: SceneNode;
  /**
   * True when the connected engine answered scene.getTree with
   * METHOD_NOT_SUPPORTED — i.e. it does not implement scene query. This is the
   * engine-agnostic degradation signal the Outliner reads to show an
   * "unsupported" notice instead of an empty scene. Reset on (re)connect.
   */
  sceneUnsupported?: boolean;
  /**
   * Snapshot of the currently selected object's properties (object.getSnapshot),
   * or undefined when nothing is selected / no snapshot has arrived yet. Cleared
   * on deselect, disconnect, and process exit so a stale object never lingers.
   * Generic — carries no engine-specific assumptions.
   */
  objectSnapshot?: ObjectSnapshot;
  /**
   * Generic type descriptors from schema.getSnapshot, fetched once per
   * connection. Used as an auxiliary hint when rendering property valueTypes.
   * Undefined before any schema has been fetched (or after disconnect).
   */
  schemaTypes?: TypeDescriptor[];
  /**
   * True when the connected engine answered object.getSnapshot (or
   * schema.getSnapshot) with METHOD_NOT_SUPPORTED — i.e. it does not implement
   * object/schema query. Engine-agnostic degradation signal the Inspector reads
   * to show an "unsupported" notice. Reset on (re)connect.
   */
  objectUnsupported?: boolean;
}

export const INITIAL_STATE: BridgeState = {
  connection: { status: 'disconnected' },
  logs: [],
  selectedObjectId: undefined,
};

// -------------------------------------------------------------------------
// Log cap
// -------------------------------------------------------------------------

const MAX_LOG_ENTRIES = 1000;

// -------------------------------------------------------------------------
// Actions
// -------------------------------------------------------------------------

export type BridgeAction =
  | { type: 'commandPending' }
  | { type: 'commandSettled' }
  | { type: 'connectionStateChanged'; payload: ConnectionStatePayload }
  | { type: 'statusUpdated'; payload: GetStatusResult }
  | { type: 'logAppended'; payload: LogMessageEvent; id: number }
  | { type: 'runtimeStateChanged'; payload: RuntimeStateChangedEvent }
  | { type: 'engineStatusChanged'; payload: EngineStatusChangedEvent }
  | { type: 'errorReported'; payload: ErrorReportedEvent }
  | { type: 'engineProcessExited'; payload: EngineProcessExitedEvent }
  | { type: 'viewportStateChanged'; payload: ViewportStateChangedEvent }
  | { type: 'dismissError' }
  /**
   * Select a scene object by id. Pass undefined to deselect.
   * Engine-agnostic: id is a plain string token, not mock-specific.
   */
  | { type: 'objectSelected'; id: string | undefined }
  /**
   * Store the root of a freshly fetched scene.getTree snapshot.
   * `root` is the single root SceneNode from the wire result { root }.
   */
  | { type: 'sceneTreeLoaded'; root: SceneNode }
  /**
   * Mark scene query as unsupported by the connected engine (scene.getTree
   * answered METHOD_NOT_SUPPORTED). Engine-agnostic degradation signal.
   */
  | { type: 'sceneTreeUnsupported' }
  /**
   * Store a freshly fetched object.getSnapshot for the selected object.
   * Engine-agnostic: a generic property bag, not mock-specific.
   */
  | { type: 'objectSnapshotLoaded'; snapshot: ObjectSnapshot }
  /**
   * Store the type descriptors from a schema.getSnapshot fetch.
   */
  | { type: 'schemaSnapshotLoaded'; types: TypeDescriptor[] }
  /**
   * Mark object/schema query as unsupported by the connected engine
   * (object.getSnapshot or schema.getSnapshot answered METHOD_NOT_SUPPORTED).
   * Engine-agnostic degradation signal.
   */
  | { type: 'objectSnapshotUnsupported' };

// -------------------------------------------------------------------------
// Pure reducer
// -------------------------------------------------------------------------

export function bridgeReducer(state: BridgeState, action: BridgeAction): BridgeState {
  switch (action.type) {
    case 'commandPending': {
      // Only move to 'connecting' if currently disconnected;
      // avoid overwriting 'error' or 'connected' for non-connect commands.
      if (state.connection.status === 'disconnected') {
        return {
          ...state,
          connection: { ...state.connection, status: 'connecting' },
        };
      }
      return state;
    }

    case 'commandSettled': {
      // commandSettled is used to roll back optimistic 'connecting' status
      // when a command returns synchronously (connection-state event takes over).
      return state;
    }

    case 'connectionStateChanged': {
      const p = action.payload;
      const status: ConnectionStatus = p.connected ? 'connected' : 'disconnected';
      return {
        ...state,
        connection: {
          status,
          sessionId: p.sessionId,
          serverName: p.serverName,
          endpoint: p.endpoint,
          reason: p.reason,
        },
        // Clear lastError on successful connection
        lastError: p.connected ? undefined : state.lastError,
        // Drop the scene snapshot + selection on disconnect so the Outliner
        // degrades to its disconnected empty state and stale ids cannot linger.
        // A fresh connection also clears any prior "unsupported" marker so the
        // next engine is re-probed.
        sceneTree: p.connected ? state.sceneTree : undefined,
        selectedObjectId: p.connected ? state.selectedObjectId : undefined,
        sceneUnsupported: p.connected ? false : state.sceneUnsupported,
        // Inspector data is per-object / per-engine: a fresh connection re-probes
        // both, and a disconnect drops the stale snapshot + schema + verdict.
        objectSnapshot: p.connected ? state.objectSnapshot : undefined,
        schemaTypes: p.connected ? state.schemaTypes : undefined,
        objectUnsupported: p.connected ? false : state.objectUnsupported,
      };
    }

    case 'statusUpdated': {
      const p = action.payload;
      return {
        ...state,
        engineState: p.engineState,
        runtimeState: p.runtimeState,
        engineName: p.engineName,
        engineVersion: p.engineVersion,
        title: p.title,
      };
    }

    case 'logAppended': {
      const p = action.payload;
      const entry: LogEntry = {
        id: action.id,
        level: p.level,
        message: p.message,
        category: p.category,
        timestamp: p.timestamp,
      };
      const logs = state.logs.length >= MAX_LOG_ENTRIES
        ? [...state.logs.slice(state.logs.length - MAX_LOG_ENTRIES + 1), entry]
        : [...state.logs, entry];
      return { ...state, logs };
    }

    case 'runtimeStateChanged': {
      return { ...state, runtimeState: action.payload.state };
    }

    case 'engineStatusChanged': {
      const p = action.payload;
      return {
        ...state,
        engineState: p.engineState,
        runtimeState: p.runtimeState ?? state.runtimeState,
        title: p.title ?? state.title,
      };
    }

    case 'errorReported': {
      const err = action.payload.error;
      return {
        ...state,
        lastError: { kind: err.code, message: err.message },
        connection: { ...state.connection, status: 'error' },
      };
    }

    case 'engineProcessExited': {
      return {
        ...state,
        engineState: undefined,
        runtimeState: undefined,
        connection: { ...state.connection, status: 'disconnected' },
        // The engine is gone: its scene snapshot + selection are no longer valid.
        sceneTree: undefined,
        selectedObjectId: undefined,
        sceneUnsupported: undefined,
        // The Inspector data is likewise invalid once the engine dies.
        objectSnapshot: undefined,
        schemaTypes: undefined,
        objectUnsupported: undefined,
      };
    }

    case 'viewportStateChanged': {
      return { ...state, viewportState: action.payload.state };
    }

    case 'dismissError': {
      return { ...state, lastError: undefined };
    }

    case 'objectSelected': {
      // Changing (or clearing) the selection invalidates the previously fetched
      // object snapshot. The schema (engine-wide) is left intact. The fetch
      // effect in the Inspector loads the new selection's snapshot; until it
      // arrives the panel shows a loading state rather than the old object.
      if (action.id === state.selectedObjectId) {
        // Re-selecting the same id is a no-op for the snapshot.
        return { ...state, selectedObjectId: action.id };
      }
      return { ...state, selectedObjectId: action.id, objectSnapshot: undefined };
    }

    case 'sceneTreeLoaded': {
      // A successful tree clears any prior "unsupported" marker.
      return { ...state, sceneTree: action.root, sceneUnsupported: false };
    }

    case 'sceneTreeUnsupported': {
      // No tree to show; record the engine's degradation for the Outliner.
      return { ...state, sceneTree: undefined, sceneUnsupported: true };
    }

    case 'objectSnapshotLoaded': {
      // A successful snapshot clears any prior "unsupported" marker.
      return { ...state, objectSnapshot: action.snapshot, objectUnsupported: false };
    }

    case 'schemaSnapshotLoaded': {
      return { ...state, schemaTypes: action.types };
    }

    case 'objectSnapshotUnsupported': {
      // No snapshot to show; record the engine's degradation for the Inspector.
      return { ...state, objectSnapshot: undefined, objectUnsupported: true };
    }

    default: {
      // Exhaustiveness guard — TypeScript will catch missing cases at compile time.
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}
