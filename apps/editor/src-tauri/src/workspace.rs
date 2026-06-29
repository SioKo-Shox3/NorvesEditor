//! Workspace root management for the editor shell.
//!
//! This module is deliberately independent from [`crate::bridge_state`]: a
//! workspace is editor-local file-system state, not Bridge transport state.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::State;

use crate::dto::WorkspacePayload;
use crate::error::BackendError;

#[derive(Debug, Clone)]
struct Workspace {
    root_path: String,
    assets_root: String,
    name: String,
}

impl Workspace {
    fn to_payload(&self) -> WorkspacePayload {
        WorkspacePayload {
            root_path: self.root_path.clone(),
            assets_root: self.assets_root.clone(),
            name: self.name.clone(),
        }
    }
}

/// Tauri-managed workspace state.
///
/// The lock protects only a small `Option<Workspace>`. Commands use synchronous
/// `std::fs::metadata` and do not hold the lock while touching the filesystem.
pub struct WorkspaceState {
    inner: Mutex<Option<Workspace>>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        WorkspaceState {
            inner: Mutex::new(None),
        }
    }
}

/// Normalize an editor logical asset path.
///
/// Accepts a path with or without an `Assets/` prefix, collapses `.` segments,
/// rejects absolute / UNC / drive-relative / parent traversal input, and returns
/// a forward-slash logical path with the `Assets/` prefix stripped.
#[cfg_attr(not(test), allow(dead_code))]
pub fn normalize_logical_asset_path(input: &str) -> Result<String, String> {
    if input.is_empty() {
        return Err("empty".to_owned());
    }
    if input.starts_with("//") || input.starts_with("\\\\") {
        return Err("UNC".to_owned());
    }
    if input.starts_with('/') || input.starts_with('\\') {
        return Err("root absolute".to_owned());
    }

    let bytes = input.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        if bytes.len() >= 3 && (bytes[2] == b'/' || bytes[2] == b'\\') {
            return Err("absolute path".to_owned());
        }
        return Err("drive-relative".to_owned());
    }

    let mut segments = Vec::new();
    for segment in input.split(['/', '\\']) {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if segments.is_empty() {
                return Err("escapes root".to_owned());
            }
            return Err("contains ..".to_owned());
        }
        segments.push(segment);
    }

    if segments.first() == Some(&"Assets") {
        segments.remove(0);
    }

    if segments.is_empty() {
        return Err("empty".to_owned());
    }

    Ok(segments.join("/"))
}

fn workspace_payload_for_root(root_path: String) -> Result<WorkspacePayload, BackendError> {
    if root_path.trim().is_empty() {
        return Err(BackendError::Process {
            message: "workspace root path is empty".to_owned(),
        });
    }

    let root = PathBuf::from(&root_path);
    let root_metadata = std::fs::metadata(&root).map_err(|err| BackendError::Process {
        message: format!("workspace root does not exist or cannot be read: {err}"),
    })?;
    if !root_metadata.is_dir() {
        return Err(BackendError::Process {
            message: "workspace root is not a directory".to_owned(),
        });
    }

    let assets_root = root.join("Assets");
    let assets_metadata = std::fs::metadata(&assets_root).map_err(|err| BackendError::Process {
        message: format!("workspace Assets directory does not exist or cannot be read: {err}"),
    })?;
    if !assets_metadata.is_dir() {
        return Err(BackendError::Process {
            message: "workspace Assets path is not a directory".to_owned(),
        });
    }

    let name = workspace_name(&root, &root_path);
    Ok(WorkspacePayload {
        root_path,
        assets_root: assets_root.to_string_lossy().into_owned(),
        name,
    })
}

fn workspace_name(root: &Path, fallback: &str) -> String {
    match root.file_name().and_then(|name| name.to_str()) {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => fallback.to_owned(),
    }
}

fn lock_workspace(
    state: &WorkspaceState,
) -> Result<std::sync::MutexGuard<'_, Option<Workspace>>, BackendError> {
    state.inner.lock().map_err(|_| BackendError::Process {
        message: "workspace state lock is poisoned".to_owned(),
    })
}

/// Open and store a workspace root after verifying `<root>/Assets` exists.
#[tauri::command]
pub fn workspace_open(
    state: State<'_, WorkspaceState>,
    root_path: String,
) -> Result<WorkspacePayload, BackendError> {
    let payload = workspace_payload_for_root(root_path)?;
    let workspace = Workspace {
        root_path: payload.root_path.clone(),
        assets_root: payload.assets_root.clone(),
        name: payload.name.clone(),
    };

    let mut guard = lock_workspace(state.inner())?;
    *guard = Some(workspace);
    Ok(payload)
}

/// Return the current workspace, if one is open.
#[tauri::command]
pub fn workspace_get(
    state: State<'_, WorkspaceState>,
) -> Result<Option<WorkspacePayload>, BackendError> {
    let guard = lock_workspace(state.inner())?;
    Ok(guard.as_ref().map(Workspace::to_payload))
}

/// Close the current workspace.
#[tauri::command]
pub fn workspace_close(state: State<'_, WorkspaceState>) -> Result<(), BackendError> {
    let mut guard = lock_workspace(state.inner())?;
    *guard = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_path_accepts_assets_prefix() {
        assert_eq!(
            normalize_logical_asset_path("Assets/textures/hero.png"),
            Ok("textures/hero.png".to_owned())
        );
    }

    #[test]
    fn logical_path_accepts_plain_relative_path() {
        assert_eq!(
            normalize_logical_asset_path("textures/hero.png"),
            Ok("textures/hero.png".to_owned())
        );
    }

    #[test]
    fn logical_path_collapses_leading_dot_before_assets_prefix() {
        assert_eq!(
            normalize_logical_asset_path("./Assets/a/b.png"),
            Ok("a/b.png".to_owned())
        );
    }

    #[test]
    fn logical_path_collapses_dot_segments() {
        assert_eq!(
            normalize_logical_asset_path("Assets/a/./b.png"),
            Ok("a/b.png".to_owned())
        );
    }

    #[test]
    fn logical_path_rejects_drive_absolute_path() {
        assert_eq!(
            normalize_logical_asset_path("C:/abs/x.png"),
            Err("absolute path".to_owned())
        );
    }

    #[test]
    fn logical_path_rejects_root_absolute_path() {
        assert_eq!(
            normalize_logical_asset_path("/abs/x.png"),
            Err("root absolute".to_owned())
        );
    }

    #[test]
    fn logical_path_rejects_parent_segment_inside_path() {
        assert_eq!(
            normalize_logical_asset_path("a/../b.png"),
            Err("contains ..".to_owned())
        );
    }

    #[test]
    fn logical_path_rejects_parent_segment_escape() {
        assert_eq!(
            normalize_logical_asset_path("../escape.png"),
            Err("escapes root".to_owned())
        );
    }

    #[test]
    fn logical_path_rejects_unc_path() {
        assert_eq!(
            normalize_logical_asset_path("//server/share/x"),
            Err("UNC".to_owned())
        );
    }

    #[test]
    fn logical_path_rejects_drive_relative_path() {
        assert_eq!(
            normalize_logical_asset_path("C:rel.png"),
            Err("drive-relative".to_owned())
        );
    }

    #[test]
    fn logical_path_rejects_empty_path() {
        assert_eq!(normalize_logical_asset_path(""), Err("empty".to_owned()));
    }
}
