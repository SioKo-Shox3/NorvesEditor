//! Offline asset manifest reader for the editor shell.
//!
//! This is editor-local file-system state, not Bridge transport state. The
//! command reads a manifest JSON file, validates `version == 1`, and copies the
//! snake_case manifest entries into frontend-facing camelCase DTOs.

use serde::Deserialize;

use crate::dto::{AssetEntryDto, AssetManifestPayload};
use crate::error::BackendError;

const SUPPORTED_MANIFEST_VERSION: u32 = 1;

/// Upper bound for a manifest file we will read fully into memory. A real
/// runtime manifest is a small JSON index (a few hundred KiB at most for
/// thousands of entries); anything far larger is a wrong path or a malformed
/// file, so reject it before allocating rather than reading unbounded bytes.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct AssetManifestFile {
    version: u32,
    assets: Vec<AssetManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct AssetManifestEntry {
    logical_path: String,
    kind: String,
    variant: Option<String>,
    format: Option<String>,
    source_hash: Option<String>,
    cooked_package: Option<String>,
    entry_name: Option<String>,
    entry_type: Option<String>,
    cooked_hash: Option<String>,
    cooked_version: Option<u32>,
}

impl From<AssetManifestEntry> for AssetEntryDto {
    fn from(entry: AssetManifestEntry) -> Self {
        AssetEntryDto {
            logical_path: entry.logical_path,
            kind: entry.kind,
            variant: entry.variant,
            format: entry.format,
            source_hash: entry.source_hash,
            cooked_package: entry.cooked_package,
            entry_name: entry.entry_name,
            entry_type: entry.entry_type,
            cooked_hash: entry.cooked_hash,
            cooked_version: entry.cooked_version,
        }
    }
}

/// Read and parse a snake_case Norves asset manifest from disk.
#[tauri::command]
pub fn asset_read_manifest(manifest_path: String) -> Result<AssetManifestPayload, BackendError> {
    read_manifest_payload(manifest_path)
}

fn read_manifest_payload(manifest_path: String) -> Result<AssetManifestPayload, BackendError> {
    if manifest_path.trim().is_empty() {
        return Err(BackendError::Process {
            message: "asset manifest path is empty".to_owned(),
        });
    }

    let metadata = std::fs::metadata(&manifest_path).map_err(|err| BackendError::Process {
        message: format!("asset manifest file does not exist or cannot be read: {err}"),
    })?;
    if !metadata.is_file() {
        return Err(BackendError::Process {
            message: "asset manifest path is not a file".to_owned(),
        });
    }
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(BackendError::Process {
            message: format!(
                "asset manifest file is too large ({} bytes; limit {} bytes)",
                metadata.len(),
                MAX_MANIFEST_BYTES
            ),
        });
    }

    let text = std::fs::read_to_string(&manifest_path).map_err(|err| BackendError::Process {
        message: format!("asset manifest file does not exist or cannot be read: {err}"),
    })?;
    let manifest: AssetManifestFile =
        serde_json::from_str(&text).map_err(|err| BackendError::Process {
            message: format!("asset manifest JSON parse failed: {err}"),
        })?;

    if manifest.version != SUPPORTED_MANIFEST_VERSION {
        return Err(BackendError::Process {
            message: format!(
                "unsupported asset manifest version {}; expected {}",
                manifest.version, SUPPORTED_MANIFEST_VERSION
            ),
        });
    }

    Ok(AssetManifestPayload {
        version: manifest.version,
        manifest_path,
        assets: manifest
            .assets
            .into_iter()
            .map(AssetEntryDto::from)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEST_FILE_ID: AtomicUsize = AtomicUsize::new(0);

    fn write_manifest(contents: &str) -> PathBuf {
        let id = NEXT_TEST_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after UNIX_EPOCH")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "norves-editor-asset-manifest-test-{nanos}-{id}.json"
        ));
        std::fs::write(&path, contents).expect("writes test manifest");
        path
    }

    fn read_test_manifest(contents: &str) -> Result<AssetManifestPayload, BackendError> {
        let path = write_manifest(contents);
        let result = read_manifest_payload(path.to_string_lossy().into_owned());
        std::fs::remove_file(path).expect("removes test manifest");
        result
    }

    fn error_message(err: BackendError) -> String {
        match err {
            BackendError::Process { message } => message,
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn rejects_a_directory_path() {
        // A directory exists but is not a readable manifest file; the is_file
        // guard (added alongside the read size limit) must reject it.
        let dir = std::env::temp_dir();
        let err = read_manifest_payload(dir.to_string_lossy().into_owned())
            .expect_err("a directory path must be rejected");
        assert!(error_message(err).contains("not a file"));
    }

    #[test]
    fn reads_valid_manifest_and_copies_snake_case_fields_to_dto() {
        let payload = read_test_manifest(
            r#"{
                "version": 1,
                "assets": [{
                    "logical_path": "textures/hero.png",
                    "kind": "texture",
                    "variant": "default",
                    "format": "png",
                    "source_hash": "source-hash",
                    "cooked_package": "main.ncpkg",
                    "entry_name": "textures/hero",
                    "entry_type": "texture2d",
                    "cooked_hash": "cooked-hash",
                    "cooked_version": 7
                }]
            }"#,
        )
        .expect("valid manifest reads");

        assert_eq!(payload.version, SUPPORTED_MANIFEST_VERSION);
        assert_eq!(payload.assets.len(), 1);
        assert_eq!(
            payload.assets[0],
            AssetEntryDto {
                logical_path: "textures/hero.png".to_owned(),
                kind: "texture".to_owned(),
                variant: Some("default".to_owned()),
                format: Some("png".to_owned()),
                source_hash: Some("source-hash".to_owned()),
                cooked_package: Some("main.ncpkg".to_owned()),
                entry_name: Some("textures/hero".to_owned()),
                entry_type: Some("texture2d".to_owned()),
                cooked_hash: Some("cooked-hash".to_owned()),
                cooked_version: Some(7),
            }
        );
    }

    #[test]
    fn rejects_unsupported_manifest_version() {
        let err = read_test_manifest(
            r#"{
                "version": 2,
                "assets": []
            }"#,
        )
        .expect_err("version 2 is rejected");

        assert_eq!(
            error_message(err),
            "unsupported asset manifest version 2; expected 1"
        );
    }

    #[test]
    fn rejects_invalid_json_with_parse_reason() {
        let err = read_test_manifest(r#"{ "version": 1, "assets": ["#)
            .expect_err("invalid JSON is rejected");

        assert!(
            error_message(err).starts_with("asset manifest JSON parse failed:"),
            "parse error should include a reason"
        );
    }

    #[test]
    fn accepts_missing_cooked_fields_for_loose_assets() {
        let payload = read_test_manifest(
            r#"{
                "version": 1,
                "assets": [{
                    "logical_path": "textures/loose.png",
                    "kind": "texture",
                    "source_hash": "source-hash"
                }]
            }"#,
        )
        .expect("loose-style manifest reads");

        let entry = &payload.assets[0];
        assert_eq!(entry.logical_path, "textures/loose.png");
        assert_eq!(entry.kind, "texture");
        assert_eq!(entry.source_hash.as_deref(), Some("source-hash"));
        assert_eq!(entry.cooked_package, None);
        assert_eq!(entry.entry_name, None);
        assert_eq!(entry.entry_type, None);
        assert_eq!(entry.cooked_hash, None);
        assert_eq!(entry.cooked_version, None);
    }

    #[test]
    fn accepts_empty_assets() {
        let payload = read_test_manifest(
            r#"{
                "version": 1,
                "assets": []
            }"#,
        )
        .expect("empty manifest reads");

        assert_eq!(payload.version, SUPPORTED_MANIFEST_VERSION);
        assert!(payload.assets.is_empty());
    }

    #[test]
    fn missing_file_reports_read_reason() {
        let id = NEXT_TEST_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("norves-editor-missing-asset-manifest-{id}.json"));
        let _ = std::fs::remove_file(&path);
        let err = read_manifest_payload(path.to_string_lossy().into_owned())
            .expect_err("missing file is rejected");

        assert!(
            error_message(err).starts_with("asset manifest file does not exist or cannot be read:"),
            "read error should include a reason"
        );
    }
}
