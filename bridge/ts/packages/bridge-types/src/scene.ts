// scene.getTree method result types.
//
// Shapes mirror the positive fixture
// bridge/spec/fixtures/methods/scene.getTree/positive/response-valid.json and
// the schema methods/scene.getTree.result.schema.json (result = { root }) whose
// node shape is the recursive `sceneNode` $def in common.schema.json.
//
// Generic protocol: SceneNode carries no engine-specific semantics; id/name/kind
// are free-form tokens. Mirrors the Rust SceneTree / SceneNode in
// bridge/crates/norves-bridge-editor-client/src/scene.rs.

import type { ObjectId } from './common.js';

/**
 * Recursive node in a scene-tree snapshot. A serialized DTO copy, never a live
 * engine pointer.
 *
 * `id` is the only required field. `children` absent or empty means a leaf.
 */
export interface SceneNode {
  /** Opaque identifier of this node (non-empty on the wire). */
  id: ObjectId;
  /** Optional human-readable node name. */
  name?: string;
  /** Optional generic node classification (free-form, not an engine type name). */
  kind?: string;
  /** Optional child nodes; absent or empty means a leaf. */
  children?: SceneNode[];
}

/**
 * Result of the `scene.getTree` method: a single `root` node.
 *
 * This is exactly the wire shape `{ root: SceneNode }` — not an array.
 */
export interface SceneGetTreeResult {
  /** Root node of the snapshotted (sub)tree. */
  root: SceneNode;
}
