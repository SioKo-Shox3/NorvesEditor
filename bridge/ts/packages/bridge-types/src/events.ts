// Event payload interfaces for the alpha control-plane events.
// Shapes mirror the positive fixtures under bridge/spec/fixtures/events/.
//
// Each interface models the `params` object of the envelope.

import type {
  Origin,
  ObjectId,
  LogLevel,
  EngineState,
  RuntimeState,
  ViewportState,
} from './common.js';
import type { BridgeError } from './envelope.js';
import type { SceneNode } from './scene.js';
import type { PropertyEntry } from './object.js';

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

// ---- scene.treeChanged (protocol 0.2) -----------------------------------

/**
 * Sent by the engine when the scene tree changes. Introduced in protocol
 * version 0.2. Carries snapshot copies (DTO, never live engine pointers) of the
 * affected nodes; `fullRefreshRequired` asks the editor to re-fetch the whole
 * tree via `scene.getTree` instead of applying `changedNodes` incrementally.
 *
 * Shape mirrors the positive fixture
 * bridge/spec/fixtures/events/scene.treeChanged/positive/event-engine-valid.json
 * and events/scene.treeChanged.params.schema.json.
 */
export interface SceneTreeChangedEvent {
  /** Snapshot copies of the nodes that changed; absent when none are carried. */
  changedNodes?: SceneNode[];
  /** When true, re-fetch the whole tree rather than apply changedNodes. */
  fullRefreshRequired?: boolean;
}

// ---- object.changed (protocol 0.2) --------------------------------------

/**
 * Sent by the engine when an object's properties change, e.g. after
 * `object.setProperty` is accepted. Introduced in protocol version 0.2.
 * `properties` is a snapshot copy of the object's property bag (DTO, never a
 * live engine pointer).
 *
 * Shape mirrors the positive fixture
 * bridge/spec/fixtures/events/object.changed/positive/event-engine-valid.json
 * and events/object.changed.params.schema.json.
 */
export interface ObjectChangedEvent {
  /** Object that changed. */
  objectId: ObjectId;
  /** Snapshot copy of the object's current property bag. */
  properties: PropertyEntry[];
  /** Optional current human-readable object name. */
  name?: string;
  /** Optional generic object classification (free-form). */
  kind?: string;
}
