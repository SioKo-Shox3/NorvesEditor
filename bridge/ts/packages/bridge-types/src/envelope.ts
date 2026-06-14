// Bridge wire envelope types.
// Mirrors envelope.rs Envelope struct and Kind enum exactly.
// Field names follow the JSON wire format (camelCase where Rust uses rename).

export const KINDS = ['request', 'response', 'event'] as const;
export type Kind = (typeof KINDS)[number];

/**
 * Wire error object carried in a response envelope's error field.
 * Mirrors error.rs BridgeError (additionalProperties: false).
 */
export interface BridgeError {
  /** Screaming-snake-case error code, e.g. "METHOD_NOT_SUPPORTED". */
  code: string;
  /** Human-readable error message. */
  message: string;
  /** Optional structured, error-code-specific detail. */
  data?: Record<string, unknown>;
}

/**
 * Canonical Bridge wire envelope.
 * Mirrors envelope.rs Envelope with serde field names (camelCase: sessionId).
 * Optional fields are present/absent per-kind as enforced by Envelope::validate.
 */
export interface Envelope {
  /** Protocol marker — always "norves.editor.bridge". */
  bridge: 'norves.editor.bridge';
  /** Protocol version string, MAJOR.MINOR. */
  version: string;
  /** Envelope discriminator. */
  kind: Kind;
  /** Request/response correlation id. Present on request and response. */
  id?: string;
  /** Method name on a request. */
  method?: string;
  /** Event name on an event envelope. */
  event?: string;
  /** Method or event payload. */
  params?: Record<string, unknown>;
  /** Success payload on a response. Mutually exclusive with error. */
  result?: unknown;
  /** Error payload on a response. Mutually exclusive with result. */
  error?: BridgeError;
  /** Optional session id assigned during the handshake. Wire name: sessionId. */
  sessionId?: string;
  /** Optional monotonically increasing per-connection sequence number. */
  seq?: number;
}
