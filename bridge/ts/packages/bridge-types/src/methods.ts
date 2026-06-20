// Method param/result interfaces for the alpha control-plane methods.
// Shapes mirror the positive fixtures under bridge/spec/fixtures/methods/.
//
// scene.getTree lives in ./scene.ts (Phase 3). Still omitted here:
// object.getSnapshot, object.setProperty, schema.getSnapshot.

import type {
  CapabilityDescriptor,
  VersionString,
  EngineState,
  RuntimeState,
  LogLevel,
} from './common.js';

// ---- bridge.hello -------------------------------------------------------

export interface HelloParams {
  role: 'editor';
  clientName: string;
  /** Optional client product version. */
  clientVersion?: string;
  /** Ordered list of protocol versions the client speaks, e.g. ["0.1"]. */
  protocolVersions: VersionString[];
  /** Capability tokens the client wishes to use. */
  capabilities?: string[];
}

export interface HelloServerInfo {
  name: string;
  version?: string;
  engine?: string;
}

export interface HelloResult {
  sessionId: string;
  protocolVersion: VersionString;
  server: HelloServerInfo;
  capabilities?: CapabilityDescriptor[];
}

// ---- bridge.getCapabilities ---------------------------------------------

/** bridge.getCapabilities has no params (empty object on the wire). */
export type GetCapabilitiesParams = Record<string, never>;

export interface GetCapabilitiesResult {
  capabilities: CapabilityDescriptor[];
}

// ---- engine.getStatus ---------------------------------------------------

/** engine.getStatus has no params (empty object on the wire). */
export type GetStatusParams = Record<string, never>;

export interface GetStatusResult {
  engineState: EngineState;
  runtimeState: RuntimeState;
  engineName?: string;
  engineVersion?: string;
  title?: string;
}

// ---- engine.launchInfo --------------------------------------------------

/** engine.launchInfo has no params (empty object on the wire). */
export type LaunchInfoParams = Record<string, never>;

export interface LaunchInfoResult {
  pid: number;
  title: string;
  endpoint?: string;
  executable?: string;
  argv?: string[];
  /** ISO 8601 datetime string. */
  startedAt?: string;
}

// ---- log.subscribe ------------------------------------------------------

export interface LogFilter {
  minLevel?: LogLevel;
  categories?: string[];
}

export interface LogSubscribeParams {
  filter?: LogFilter;
}

export interface EffectiveFilter {
  minLevel?: LogLevel;
  categories?: string[];
}

export interface LogSubscribeResult {
  subscriptionId: string;
  effectiveFilter?: EffectiveFilter;
}

// ---- log.unsubscribe ----------------------------------------------------

export interface LogUnsubscribeParams {
  subscriptionId: string;
}

export interface LogUnsubscribeResult {
  ok: boolean;
}

// ---- runtime.play -------------------------------------------------------

/** runtime.play has no params (empty object on the wire). */
export type PlayParams = Record<string, never>;

export interface PlayResult {
  accepted: boolean;
  requestedState?: RuntimeState;
}

// ---- runtime.pause ------------------------------------------------------

/** runtime.pause has no params (empty object on the wire). */
export type PauseParams = Record<string, never>;

export interface PauseResult {
  accepted: boolean;
  requestedState?: RuntimeState;
}

// ---- runtime.stop -------------------------------------------------------

/** runtime.stop has no params (empty object on the wire). */
export type StopParams = Record<string, never>;

export interface StopResult {
  accepted: boolean;
  requestedState?: RuntimeState;
}

// ---- runtime.focusViewport ----------------------------------------------

/** runtime.focusViewport has no params (empty object on the wire). */
export type FocusViewportParams = Record<string, never>;

export interface FocusViewportResult {
  focused: boolean;
}
