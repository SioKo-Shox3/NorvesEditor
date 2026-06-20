// schema.getSnapshot method result types.
//
// Shapes mirror the positive fixture
// bridge/spec/fixtures/methods/schema.getSnapshot/positive/response-valid.json and
// the schema methods/schema.getSnapshot.result.schema.json (result = { types })
// whose descriptor shape is the `typeDescriptor` $def (with `propertyDefinition`
// entries) in common.schema.json.
//
// Generic protocol: TypeDescriptor / PropertyDefinition carry no engine-specific
// semantics; typeName/kind/valueType are free-form labels. Mirrors the Rust
// SchemaSnapshot / TypeDescriptor / PropertyDefinition in
// bridge/crates/norves-bridge-editor-client/src/object.rs.

/**
 * Generic definition of one property exposed by a type in schema.getSnapshot.
 * Describes name and value type, not a concrete value.
 */
export interface PropertyDefinition {
  /** Generic property name (non-empty on the wire). */
  name: string;
  /** Free-form generic type label for this property's value. */
  valueType: string;
}

/**
 * Generic description of one object/component type the engine exposes. `typeName`
 * is a free-form generic label; no engine-specific names appear.
 *
 * `typeName` is required; `kind` and `properties` are optional.
 */
export interface TypeDescriptor {
  /** Free-form generic name of the type. */
  typeName: string;
  /** Optional generic classification, e.g. object or component (free-form). */
  kind?: string;
  /** Optional property definitions for this type. */
  properties?: PropertyDefinition[];
}

/**
 * Result of the `schema.getSnapshot` method: the engine's generic type
 * descriptors. A DTO copy, never a live engine pointer.
 *
 * `types` is required and may be empty.
 */
export interface SchemaSnapshot {
  /** Generic type descriptors the engine exposes; may be empty. */
  types: TypeDescriptor[];
}
