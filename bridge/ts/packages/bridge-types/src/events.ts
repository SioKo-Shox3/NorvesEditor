// Event payload interfaces for the alpha control-plane events.
// Shapes mirror the positive fixtures under bridge/spec/fixtures/events/.
//
// Each interface models the `params` object of the envelope.

import type {
  Origin,
  LogLevel,
  EngineState,
  RuntimeState,
  ViewportState,
} from './common.js';
import type { BridgeError } from './envelope.js';

// ---- log.message --------------------------------------------------------

export interface LogMessageEvent {
  level: LogLevel;
  category?: string;
  message: string;
  /** ISO 8601 datetime string. */
  timestamp?: string;
}

// ---- runtime.stateChanged -----------------------------------------------

export interface RuntimeStateChangedEvent {
  state: RuntimeState;
  /** Previous runtime state — absent when no prior state is known. */
  previous?: RuntimeState;
}

// ---- engine.statusChanged -----------------------------------------------

export interface EngineStatusChangedEvent {
  engineState: EngineState;
  /** Previous engine state — absent when no prior state is known. */
  previous?: EngineState;
  runtimeState?: RuntimeState;
  title?: string;
}

// ---- bridge.connected ---------------------------------------------------

export interface BridgeConnectedEvent {
  endpoint: string;
  origin?: Origin;
}

// ---- bridge.disconnected ------------------------------------------------

export interface BridgeDisconnectedEvent {
  reason: string;
  code?: string;
  willReconnect?: boolean;
  origin?: Origin;
}

// ---- error.reported -----------------------------------------------------

/**
 * Reuses BridgeError from envelope.ts — structurally identical to the schema's
 * envelope $defs/error, which error.reported.$ref points to.
 */
export type { BridgeError as ReportedError };

export interface ErrorReportedEvent {
  error: BridgeError;
  origin?: Origin;
}

// ---- engine.processExited -----------------------------------------------

export interface EngineProcessExitedEvent {
  exitCode: number;
  /** Optional termination signal name on platforms that report one. */
  signal?: string;
  origin?: Origin;
}

// ---- viewport.stateChanged ----------------------------------------------

export interface ViewportStateChangedEvent {
  state: ViewportState;
  /** Previous viewport state — absent when no prior state is known. */
  previous?: ViewportState;
}
