// object.getSnapshot method result types.
//
// Shapes mirror the positive fixture
// bridge/spec/fixtures/methods/object.getSnapshot/positive/response-valid.json and
// the schema methods/object.getSnapshot.result.schema.json
// (result = { objectId, name?, kind?, properties }) whose property-entry shape is
// the `propertyEntry` $def (value = `propertyValue`) in common.schema.json.
//
// Generic protocol: ObjectSnapshot / PropertyEntry carry no engine-specific
// semantics; objectId/name/kind/valueType are free-form tokens and property
// values are arbitrary serialized JSON (never a live engine pointer). Mirrors the
// Rust ObjectSnapshot / PropertyEntry in
// bridge/crates/norves-bridge-editor-client/src/object.rs.

import type { ObjectId } from './common.js';

/**
 * Generic serialized property value: a plain JSON value (snapshot copy, never a
 * reference into engine live memory). Mirrors the `propertyValue` $def in
 * common.schema.json: string | number | boolean | null | array | object.
 *
 * Structured values (array / object) use generic containers without
 * engine-specific type names, so nested elements are themselves `PropertyValue`.
 */
export type PropertyValue =
  | string
  | number
  | boolean
  | null
  | PropertyValue[]
  | { [key: string]: PropertyValue };

/**
 * One generic name->value entry in an object property snapshot. A serialized DTO
 * copy, never a live engine pointer.
 *
 * `name` and `value` are required; `valueType` is an optional free-form label
 * sharing the same vocabulary as schema.getSnapshot's valueType.
 */
export interface PropertyEntry {
  /** Generic property name (non-empty on the wire). */
  name: string;
  /** Snapshot copy of the property value. */
  value: PropertyValue;
  /** Optional free-form type label (not enumerated). */
  valueType?: string;
}

/**
 * Result of the `object.getSnapshot` method: one object's serialized property
 * snapshot. A DTO copy of generic values, never a live engine pointer.
 *
 * `objectId` and `properties` are required (`properties` may be empty); `name`
 * and `kind` are optional.
 */
export interface ObjectSnapshot {
  /** Object this snapshot describes. */
  objectId: ObjectId;
  /** Optional human-readable object name. */
  name?: string;
  /** Optional generic object classification (free-form, not an engine type name). */
  kind?: string;
  /** Serialized property entries for this object; may be empty. */
  properties: PropertyEntry[];
}

/**
 * Result of the `object.setProperty` method: the engine's acknowledgement of a
 * property write. Mirrors the schema
 * methods/object.setProperty.result.schema.json (result = { accepted,
 * appliedValue? }) and the Rust SetPropertyAck in
 * bridge/crates/norves-bridge-editor-client/src/object.rs.
 *
 * `accepted` reports whether the engine applied the change. `appliedValue`, when
 * present, is a snapshot copy of the value the engine actually stored (which may
 * be normalized from the requested value) — a plain JSON value, never a live
 * engine pointer. Absent `appliedValue` means the engine did not echo a value.
 */
export interface SetObjectPropertyResult {
  /** Whether the engine accepted and applied the property change. */
  accepted: boolean;
  /** Optional snapshot copy of the value actually applied. */
  appliedValue?: PropertyValue;
}
