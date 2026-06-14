// Wire-transparent enum constants + derived union types.
// Values mirror common.rs exactly — changing any string here is a protocol change.

export const LOG_LEVELS = ['trace', 'debug', 'info', 'warn', 'error'] as const;
export type LogLevel = (typeof LOG_LEVELS)[number];

export const ENGINE_STATES = ['initializing', 'ready', 'running', 'error'] as const;
export type EngineState = (typeof ENGINE_STATES)[number];

export const RUNTIME_STATES = ['edit', 'playing', 'paused', 'stopped', 'unknown'] as const;
export type RuntimeState = (typeof RUNTIME_STATES)[number];

export const VIEWPORT_STATES = ['focused', 'visible', 'hidden', 'minimized', 'unknown'] as const;
export type ViewportState = (typeof VIEWPORT_STATES)[number];

// Origin: "editor-backend" is kebab-case on the wire (see common.rs Origin::EditorBackend).
export const ORIGINS = ['engine', 'editor-backend'] as const;
export type Origin = (typeof ORIGINS)[number];

/** Opaque identifier of a scene object/node. Non-empty string on the wire. */
export type ObjectId = string;

/** Namespaced capability token, e.g. "runtime.control". Non-empty string on the wire. */
export type CapabilityToken = string;

/** Protocol or product version string in MAJOR.MINOR form, e.g. "0.1". */
export type VersionString = string;

/**
 * Capability descriptor as advertised in bridge.hello / bridge.getCapabilities.
 * Mirrors common.rs CapabilityDescriptor (camelCase field names on the wire).
 */
export interface CapabilityDescriptor {
  name: CapabilityToken;
  version?: VersionString;
  description?: string;
}
