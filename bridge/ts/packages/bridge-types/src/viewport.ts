// viewport.getThumbnail method result types.
//
// Shapes mirror the positive fixture
// bridge/spec/fixtures/methods/viewport.getThumbnail/positive/response-valid.json
// and the schema methods/viewport.getThumbnail.result.schema.json
// (result = { imageBase64, mimeType, width?, height? }).
//
// Generic protocol: ViewportThumbnail carries no engine-specific semantics;
// mimeType is a free-form token and imageBase64 is an opaque base64 snapshot of
// the engine's framebuffer (a snapshot copy, never a live engine pointer). The
// large-payload limits (PNG, max 640x360, 256 KiB hard cap, pull-style, <= 1 fps)
// are documented in docs/memory-buffer-policy.md. Mirrors the Rust
// ViewportThumbnail in
// bridge/crates/norves-bridge-editor-client/src/viewport.rs.

/**
 * Result of the `viewport.getThumbnail` method: a still thumbnail of the engine's
 * external viewport. A DTO copy of generic values, never a live engine pointer.
 *
 * `imageBase64` and `mimeType` are required; `width`/`height` are optional decoded
 * dimensions.
 */
export interface ViewportThumbnail {
  /** Base64-encoded image bytes (a snapshot copy). */
  imageBase64: string;
  /** MIME type of the encoded image, e.g. "image/png". */
  mimeType: string;
  /** Optional decoded width in pixels. */
  width?: number;
  /** Optional decoded height in pixels. */
  height?: number;
}
