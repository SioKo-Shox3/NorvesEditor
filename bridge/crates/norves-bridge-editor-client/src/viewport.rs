//! `viewport.getThumbnail` result domain type (sans-I/O).
//!
//! Extracts a [`ViewportThumbnail`] from the `result` value of a
//! `viewport.getThumbnail` response. Wire shape follows
//! `viewport.getThumbnail.result.schema.json` (`{ imageBase64, mimeType, width?,
//! height? }`).
//!
//! This is the drift-guard used by the `viewport_get_thumbnail` Tauri command: it
//! validates the wire shape so a malformed engine result surfaces as a clean
//! backend error rather than being forwarded blindly, while the command still
//! returns the ORIGINAL wire `Value` (no re-modeling round-trip). The type here is
//! generic — `mimeType` carries no engine-specific semantics, and `imageBase64` is
//! a snapshot copy of the engine's framebuffer (never a live engine pointer). The
//! large-payload limits (PNG, max 640x360, 256 KiB hard cap, pull-style, <= 1 fps)
//! are documented in `docs/memory-buffer-policy.md`.

use serde_json::{Map, Value};

/// A serialized still thumbnail extracted from a `viewport.getThumbnail` result.
///
/// A DTO copy, never a live engine pointer. `image_base64` is the base64-encoded
/// snapshot of the engine's framebuffer; `mime_type` identifies the encoding.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportThumbnail {
    /// Base64-encoded image bytes (non-empty on the wire). A snapshot copy.
    pub image_base64: String,
    /// MIME type of the encoded image, e.g. `image/png` (non-empty on the wire).
    pub mime_type: String,
    /// Optional decoded width in pixels.
    pub width: Option<u32>,
    /// Optional decoded height in pixels.
    pub height: Option<u32>,
}

/// Failure while extracting a [`ViewportThumbnail`] from a result value.
#[derive(Debug, thiserror::Error)]
pub enum ViewportError {
    /// A required field was missing.
    #[error("missing required field: {0}")]
    MissingField(String),

    /// A field was present but had the wrong type.
    #[error("invalid field: {0}")]
    InvalidField(String),

    /// The result payload did not have the expected JSON shape.
    #[error("unexpected payload shape: {0}")]
    UnexpectedShape(String),
}

/// Extracts a [`ViewportThumbnail`] from a `viewport.getThumbnail` `result` value.
///
/// Required: `imageBase64` (a non-empty string), `mimeType` (a non-empty string).
/// Optional: `width` / `height` (non-negative integers). A malformed result is
/// rejected at the boundary so the thumbnail path surfaces a clean backend error
/// rather than forwarding a non-conforming result.
pub fn parse_thumbnail_result(result: &Value) -> Result<ViewportThumbnail, ViewportError> {
    let obj = result.as_object().ok_or_else(|| {
        ViewportError::UnexpectedShape("thumbnail result is not an object".to_owned())
    })?;

    let image_base64 = required_non_empty_str(obj, "imageBase64", "result")?.to_owned();
    let mime_type = required_non_empty_str(obj, "mimeType", "result")?.to_owned();
    let width = optional_u32(obj, "width", "result")?;
    let height = optional_u32(obj, "height", "result")?;

    Ok(ViewportThumbnail {
        image_base64,
        mime_type,
        width,
        height,
    })
}

/// Returns the required non-empty string at `key`, erroring if absent, not a
/// string, or empty.
fn required_non_empty_str<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a str, ViewportError> {
    let value = obj
        .get(key)
        .ok_or_else(|| ViewportError::MissingField(format!("{path}.{key}")))?;
    let text = value
        .as_str()
        .ok_or_else(|| ViewportError::InvalidField(format!("{path}.{key}")))?;
    if text.is_empty() {
        return Err(ViewportError::InvalidField(format!("{path}.{key}")));
    }
    Ok(text)
}

/// Returns the optional `u32` at `key` if present; errors if present but not a
/// non-negative integer that fits in `u32`.
fn optional_u32(
    obj: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<u32>, ViewportError> {
    match obj.get(key) {
        None => Ok(None),
        Some(value) => {
            let n = value
                .as_u64()
                .ok_or_else(|| ViewportError::InvalidField(format!("{path}.{key}")))?;
            let n = u32::try_from(n)
                .map_err(|_| ViewportError::InvalidField(format!("{path}.{key}")))?;
            Ok(Some(n))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_thumbnail_result_extracts_all_fields() {
        // Mirrors fixtures/methods/viewport.getThumbnail/positive/response-valid.json result.
        let value = serde_json::json!({
            "imageBase64": "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR42mNwaDgARAwQCgAoDgYBqzvMVQAAAABJRU5ErkJggg==",
            "mimeType": "image/png",
            "width": 2,
            "height": 2
        });
        let thumb = parse_thumbnail_result(&value).expect("parses");
        assert!(thumb.image_base64.starts_with("iVBOR"));
        assert_eq!(thumb.mime_type, "image/png");
        assert_eq!(thumb.width, Some(2));
        assert_eq!(thumb.height, Some(2));
    }

    #[test]
    fn parse_thumbnail_result_minimal_without_dimensions_ok() {
        let value = serde_json::json!({ "imageBase64": "AAAA", "mimeType": "image/png" });
        let thumb = parse_thumbnail_result(&value).expect("parses");
        assert_eq!(thumb.image_base64, "AAAA");
        assert_eq!(thumb.mime_type, "image/png");
        assert_eq!(thumb.width, None);
        assert_eq!(thumb.height, None);
    }

    #[test]
    fn parse_thumbnail_result_missing_image_errors() {
        let value = serde_json::json!({ "mimeType": "image/png" });
        assert!(matches!(
            parse_thumbnail_result(&value),
            Err(ViewportError::MissingField(f)) if f == "result.imageBase64"
        ));
    }

    #[test]
    fn parse_thumbnail_result_missing_mime_errors() {
        let value = serde_json::json!({ "imageBase64": "AAAA" });
        assert!(matches!(
            parse_thumbnail_result(&value),
            Err(ViewportError::MissingField(f)) if f == "result.mimeType"
        ));
    }

    #[test]
    fn parse_thumbnail_result_empty_image_errors() {
        let value = serde_json::json!({ "imageBase64": "", "mimeType": "image/png" });
        assert!(matches!(
            parse_thumbnail_result(&value),
            Err(ViewportError::InvalidField(f)) if f == "result.imageBase64"
        ));
    }

    #[test]
    fn parse_thumbnail_result_non_string_mime_errors() {
        let value = serde_json::json!({ "imageBase64": "AAAA", "mimeType": 7 });
        assert!(matches!(
            parse_thumbnail_result(&value),
            Err(ViewportError::InvalidField(f)) if f == "result.mimeType"
        ));
    }

    #[test]
    fn parse_thumbnail_result_non_integer_width_errors() {
        let value =
            serde_json::json!({ "imageBase64": "AAAA", "mimeType": "image/png", "width": -1 });
        assert!(matches!(
            parse_thumbnail_result(&value),
            Err(ViewportError::InvalidField(f)) if f == "result.width"
        ));
    }

    #[test]
    fn parse_thumbnail_result_non_object_result_errors() {
        let value = serde_json::json!([]);
        assert!(matches!(
            parse_thumbnail_result(&value),
            Err(ViewportError::UnexpectedShape(_))
        ));
    }
}
