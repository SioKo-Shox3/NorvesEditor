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
  | { type: 'objectSelected'; id: string | undefined };

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
      };
    }

    case 'viewportStateChanged': {
      return { ...state, viewportState: action.payload.state };
    }

    case 'dismissError': {
      return { ...state, lastError: undefined };
    }

    case 'objectSelected': {
      return { ...state, selectedObjectId: action.id };
    }

    default: {
      // Exhaustiveness guard — TypeScript will catch missing cases at compile time.
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}
