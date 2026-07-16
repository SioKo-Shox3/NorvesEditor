//! `asset.resolve` / `asset.getManifest` result domain types (sans-I/O).
//!
//! Extracts an [`AssetResolveResult`] from the `result` value of an
//! `asset.resolve` response and an [`AssetManifestResult`] from an
//! `asset.getManifest` response. These are drift-guards for Tauri commands:
//! malformed engine results surface as clean backend errors, while commands
//! still return the ORIGINAL wire [`Value`] (no re-modeling round-trip).
//!
//! Generic protocol only: asset entries are serialized DTO copies and never
//! references into engine live memory, loaded manifest storage, or asset bytes.

use serde::Deserialize;
use serde_json::{Map, Value};

/// Outcome of resolving one logical asset path.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetResolveResult {
    pub status: AssetResolveStatus,
    pub source: AssetResolveSource,
    pub normalized_logical_path: String,
    pub requires_explicit_log: Option<bool>,
    pub fallback_action: Option<String>,
    pub failure_kind: Option<String>,
    pub reason: Option<String>,
}

/// Wire `asset.resolve.result.status` values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetResolveStatus {
    SuccessCooked,
    SuccessLoose,
    InvalidRequest,
    InvalidManifest,
    LooseReadFailed,
    CookedPackageReadFailed,
    CookedPackageParseFailed,
    CookedEntryMissing,
    CookedEntryHashMismatch,
}

/// Wire `asset.resolve.result.source` values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetResolveSource {
    None,
    Cooked,
    Loose,
    DebugLooseFallback,
}

/// Snapshot of the engine's loaded asset manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetManifestResult {
    pub version: i64,
    pub entries: Vec<AssetEntry>,
    pub total_count: i64,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

/// Acknowledgement returned by `asset.reloadManifest`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetReloadManifestResult {
    pub accepted: bool,
}

/// One generic manifest entry. A DTO copy, never a live engine pointer.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetEntry {
    pub logical_path: String,
    pub kind: String,
    pub variant: Option<String>,
    pub format: Option<String>,
    pub source_hash: Option<String>,
    pub cooked_package: Option<String>,
    pub entry_name: Option<String>,
    pub entry_type: Option<String>,
    pub cooked_hash: Option<String>,
    pub cooked_version: Option<i64>,
}

/// Failure while extracting an asset result from a wire value.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    /// A required field was missing.
    #[error("missing required field: {0}")]
    MissingField(String),

    /// A field was present but had the wrong type or unsupported enum value.
    #[error("invalid field: {0}")]
    InvalidField(String),

    /// The result payload did not have the expected JSON shape.
    #[error("unexpected payload shape: {0}")]
    UnexpectedShape(String),
}

/// Extracts an [`AssetResolveResult`] from an `asset.resolve` `result` value.
pub fn parse_asset_resolve_result(result: &Value) -> Result<AssetResolveResult, AssetError> {
    let obj = result.as_object().ok_or_else(|| {
        AssetError::UnexpectedShape("asset.resolve result is not an object".to_owned())
    })?;

    let status = parse_status(required_str(obj, "status", "result")?)?;
    let source = parse_source(required_str(obj, "source", "result")?)?;
    let normalized_logical_path = required_str(obj, "normalizedLogicalPath", "result")?.to_owned();
    let requires_explicit_log = optional_bool(obj, "requiresExplicitLog", "result")?;
    let fallback_action = optional_str(obj, "fallbackAction", "result")?.map(str::to_owned);
    let failure_kind = optional_str(obj, "failureKind", "result")?.map(str::to_owned);
    let reason = optional_str(obj, "reason", "result")?.map(str::to_owned);

    Ok(AssetResolveResult {
        status,
        source,
        normalized_logical_path,
        requires_explicit_log,
        fallback_action,
        failure_kind,
        reason,
    })
}

/// Extracts an [`AssetManifestResult`] from an `asset.getManifest` `result` value.
pub fn parse_asset_manifest_result(result: &Value) -> Result<AssetManifestResult, AssetError> {
    let obj = result.as_object().ok_or_else(|| {
        AssetError::UnexpectedShape("asset.getManifest result is not an object".to_owned())
    })?;

    let version = required_integer(obj, "version", "result")?;
    let entries = parse_entries(obj)?;
    let total_count = required_integer(obj, "totalCount", "result")?;
    let page = optional_integer(obj, "page", "result")?;
    let page_size = optional_integer(obj, "pageSize", "result")?;

    Ok(AssetManifestResult {
        version,
        entries,
        total_count,
        page,
        page_size,
    })
}

/// Strictly extracts an [`AssetReloadManifestResult`] from an
/// `asset.reloadManifest` `result` value.
pub fn parse_asset_reload_manifest_result(
    result: &Value,
) -> Result<AssetReloadManifestResult, AssetError> {
    serde_json::from_value(result.clone()).map_err(|err| {
        AssetError::UnexpectedShape(format!("invalid asset.reloadManifest result: {err}"))
    })
}

fn parse_status(value: &str) -> Result<AssetResolveStatus, AssetError> {
    match value {
        "successCooked" => Ok(AssetResolveStatus::SuccessCooked),
        "successLoose" => Ok(AssetResolveStatus::SuccessLoose),
        "invalidRequest" => Ok(AssetResolveStatus::InvalidRequest),
        "invalidManifest" => Ok(AssetResolveStatus::InvalidManifest),
        "looseReadFailed" => Ok(AssetResolveStatus::LooseReadFailed),
        "cookedPackageReadFailed" => Ok(AssetResolveStatus::CookedPackageReadFailed),
        "cookedPackageParseFailed" => Ok(AssetResolveStatus::CookedPackageParseFailed),
        "cookedEntryMissing" => Ok(AssetResolveStatus::CookedEntryMissing),
        "cookedEntryHashMismatch" => Ok(AssetResolveStatus::CookedEntryHashMismatch),
        _ => Err(AssetError::InvalidField("result.status".to_owned())),
    }
}

fn parse_source(value: &str) -> Result<AssetResolveSource, AssetError> {
    match value {
        "none" => Ok(AssetResolveSource::None),
        "cooked" => Ok(AssetResolveSource::Cooked),
        "loose" => Ok(AssetResolveSource::Loose),
        "debugLooseFallback" => Ok(AssetResolveSource::DebugLooseFallback),
        _ => Err(AssetError::InvalidField("result.source".to_owned())),
    }
}

fn parse_entries(obj: &Map<String, Value>) -> Result<Vec<AssetEntry>, AssetError> {
    let value = obj
        .get("entries")
        .ok_or_else(|| AssetError::MissingField("result.entries".to_owned()))?;
    let items = value
        .as_array()
        .ok_or_else(|| AssetError::InvalidField("result.entries".to_owned()))?;

    let mut entries = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        entries.push(parse_entry(item, &format!("result.entries[{index}]"))?);
    }
    Ok(entries)
}

fn parse_entry(value: &Value, path: &str) -> Result<AssetEntry, AssetError> {
    let obj = value
        .as_object()
        .ok_or_else(|| AssetError::InvalidField(format!("{path} is not an object")))?;

    Ok(AssetEntry {
        logical_path: required_str(obj, "logicalPath", path)?.to_owned(),
        kind: required_str(obj, "kind", path)?.to_owned(),
        variant: optional_str(obj, "variant", path)?.map(str::to_owned),
        format: optional_str(obj, "format", path)?.map(str::to_owned),
        source_hash: optional_str(obj, "sourceHash", path)?.map(str::to_owned),
        cooked_package: optional_str(obj, "cookedPackage", path)?.map(str::to_owned),
        entry_name: optional_str(obj, "entryName", path)?.map(str::to_owned),
        entry_type: optional_str(obj, "entryType", path)?.map(str::to_owned),
        cooked_hash: optional_str(obj, "cookedHash", path)?.map(str::to_owned),
        cooked_version: optional_integer(obj, "cookedVersion", path)?,
    })
}

/// Returns the required string at `key`, erroring if absent or not a string.
fn required_str<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a str, AssetError> {
    let value = obj
        .get(key)
        .ok_or_else(|| AssetError::MissingField(format!("{path}.{key}")))?;
    value
        .as_str()
        .ok_or_else(|| AssetError::InvalidField(format!("{path}.{key}")))
}

/// Returns the string at `key` if present; errors if present but not a string.
fn optional_str<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<&'a str>, AssetError> {
    match obj.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| AssetError::InvalidField(format!("{path}.{key}"))),
    }
}

fn optional_bool(
    obj: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<bool>, AssetError> {
    match obj.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| AssetError::InvalidField(format!("{path}.{key}"))),
    }
}

fn required_integer(obj: &Map<String, Value>, key: &str, path: &str) -> Result<i64, AssetError> {
    let value = obj
        .get(key)
        .ok_or_else(|| AssetError::MissingField(format!("{path}.{key}")))?;
    integer_value(value).ok_or_else(|| AssetError::InvalidField(format!("{path}.{key}")))
}

fn optional_integer(
    obj: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<i64>, AssetError> {
    match obj.get(key) {
        None => Ok(None),
        Some(value) => integer_value(value)
            .map(Some)
            .ok_or_else(|| AssetError::InvalidField(format!("{path}.{key}"))),
    }
}

fn integer_value(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_asset_resolve_result_extracts_success() {
        let value = serde_json::json!({
            "status": "successCooked",
            "source": "cooked",
            "normalizedLogicalPath": "textures/hero.png",
            "requiresExplicitLog": true,
            "fallbackAction": "none",
            "failureKind": "none",
            "reason": "ok"
        });
        let result = parse_asset_resolve_result(&value).expect("parses");
        assert_eq!(result.status, AssetResolveStatus::SuccessCooked);
        assert_eq!(result.source, AssetResolveSource::Cooked);
        assert_eq!(result.normalized_logical_path, "textures/hero.png");
        assert_eq!(result.requires_explicit_log, Some(true));
        assert_eq!(result.fallback_action.as_deref(), Some("none"));
        assert_eq!(result.failure_kind.as_deref(), Some("none"));
        assert_eq!(result.reason.as_deref(), Some("ok"));
    }

    #[test]
    fn parse_asset_resolve_result_rejects_unknown_status() {
        let value = serde_json::json!({
            "status": "successArchived",
            "source": "cooked",
            "normalizedLogicalPath": "textures/hero.png"
        });
        assert!(matches!(
            parse_asset_resolve_result(&value),
            Err(AssetError::InvalidField(f)) if f == "result.status"
        ));
    }

    #[test]
    fn parse_asset_resolve_result_missing_normalized_path_errors() {
        let value = serde_json::json!({ "status": "invalidRequest", "source": "none" });
        assert!(matches!(
            parse_asset_resolve_result(&value),
            Err(AssetError::MissingField(f)) if f == "result.normalizedLogicalPath"
        ));
    }

    #[test]
    fn parse_asset_manifest_result_extracts_entries() {
        let value = serde_json::json!({
            "version": 1,
            "entries": [{
                "logicalPath": "textures/hero.png",
                "kind": "texture",
                "variant": "default",
                "format": "png",
                "sourceHash": "source-hash",
                "cookedPackage": "packs/textures.ncp",
                "entryName": "textures/hero.png",
                "entryType": "texture",
                "cookedHash": "cooked-hash",
                "cookedVersion": 1
            }],
            "totalCount": 1,
            "page": 0,
            "pageSize": 50
        });
        let result = parse_asset_manifest_result(&value).expect("parses");
        assert_eq!(result.version, 1);
        assert_eq!(result.total_count, 1);
        assert_eq!(result.page, Some(0));
        assert_eq!(result.page_size, Some(50));
        assert_eq!(result.entries.len(), 1);
        let entry = &result.entries[0];
        assert_eq!(entry.logical_path, "textures/hero.png");
        assert_eq!(entry.kind, "texture");
        assert_eq!(entry.cooked_version, Some(1));
    }

    #[test]
    fn parse_asset_manifest_result_empty_entries_ok() {
        let value = serde_json::json!({ "version": 1, "entries": [], "totalCount": 0 });
        let result = parse_asset_manifest_result(&value).expect("parses");
        assert!(result.entries.is_empty());
        assert_eq!(result.total_count, 0);
        assert_eq!(result.page, None);
        assert_eq!(result.page_size, None);
    }

    #[test]
    fn parse_asset_manifest_result_entry_missing_kind_errors() {
        let value = serde_json::json!({
            "version": 1,
            "entries": [{ "logicalPath": "textures/hero.png" }],
            "totalCount": 1
        });
        assert!(matches!(
            parse_asset_manifest_result(&value),
            Err(AssetError::MissingField(f)) if f == "result.entries[0].kind"
        ));
    }

    #[test]
    fn parse_asset_manifest_result_non_array_entries_errors() {
        let value = serde_json::json!({ "version": 1, "entries": {}, "totalCount": 0 });
        assert!(matches!(
            parse_asset_manifest_result(&value),
            Err(AssetError::InvalidField(f)) if f == "result.entries"
        ));
    }

    #[test]
    fn asset_reload_accepts_boolean_results() {
        for accepted in [true, false] {
            let value = serde_json::json!({ "accepted": accepted });
            let result = parse_asset_reload_manifest_result(&value).expect("parses");
            assert_eq!(result, AssetReloadManifestResult { accepted });
        }
    }

    #[test]
    fn asset_reload_rejects_missing_non_boolean_and_extra_fields() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({ "accepted": "true" }),
            serde_json::json!({ "accepted": true, "extra": 1 }),
        ] {
            assert!(
                parse_asset_reload_manifest_result(&value).is_err(),
                "unexpectedly accepted {value}"
            );
        }
    }
}
