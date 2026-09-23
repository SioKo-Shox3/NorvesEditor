//! エンジン設定(今は実行ファイルのパス)の保存・読み込みと、その Tauri コマンド。
//!
//! 設定は OS のアプリ設定ディレクトリ(`app_config_dir`)の [`SETTINGS_FILE_NAME`] に、マシン・ユーザー
//! 単位で置く。ファイルが無い・壊れているときは既定の設定として扱い、エンジンの起動を妨げない。
//! 項目の追加に備えて、欠けたキーは既定値で埋め、知らないキーは保存のときにそのまま書き戻す。
//!
//! 実行ファイルのパスを変えられるのは、Rust 側で開く OS のファイル選択ダイアログ
//! ([`pick_engine_path`])だけ。フロントエンドからパス文字列を受け取るコマンドは置かない。
//! ダイアログは `rfd` を直接使い、webview にはダイアログの権限もコマンドも公開しない。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tokio::sync::oneshot;

use crate::dto::EngineSettingsPayload;
use crate::error::BackendError;
use crate::process;
use crate::process_runtime::{DEFAULT_ENGINE_PATH, ENGINE_PATH_ENV};

/// アプリ設定ディレクトリの中の設定ファイル名。
pub const SETTINGS_FILE_NAME: &str = "engine-settings.json";

/// 設定ファイルの中身。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EngineSettings {
    /// ダイアログで選んだエンジンの実行ファイル。未設定なら `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_path: Option<String>,
    /// このビルドが知らないキー。新しいビルドが足した項目を古いビルドの保存で消さないために持ち回る。
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// 設定ファイルを読む。無い・読めない・壊れているときは既定の設定を返す(壊れているときは警告を出す)。
pub fn load_settings(file: &Path) -> EngineSettings {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return EngineSettings::default(),
        Err(e) => {
            tracing::warn!(path = %file.display(), error = %e, "エンジン設定を読めないので既定の設定を使う");
            return EngineSettings::default();
        }
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        tracing::warn!(path = %file.display(), error = %e, "エンジン設定が壊れているので既定の設定を使う");
        EngineSettings::default()
    })
}

/// 設定ファイルを書く。親ディレクトリが無ければ作る。一時ファイルに書いてから置き換えるので、
/// 書き込みの途中で落ちても前の内容か新しい内容のどちらかが残る。
pub fn save_settings(file: &Path, settings: &EngineSettings) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, file)
}

/// 画面に返す値を組み立てる。`launch_engine` と同じ優先順位(環境変数 > 設定 > 既定値)で解決する。
pub fn build_payload(
    env: Option<&str>,
    settings: &EngineSettings,
    default: &Path,
) -> EngineSettingsPayload {
    let (path, source) =
        process::resolve_engine_path_with_source(env, settings.engine_path.as_deref(), default);
    EngineSettingsPayload {
        effective_path: path.to_string_lossy().into_owned(),
        source,
        saved_path: settings.engine_path.clone(),
    }
}

/// 設定ファイルの読み替え・書き戻しを直列にする。ファイル I/O は同期で、`.await` をまたいで持たない。
#[derive(Default)]
pub struct EngineSettingsState {
    lock: Mutex<()>,
}

impl EngineSettingsState {
    /// 設定を読み、`change` を当てて書き戻す。
    fn update(
        &self,
        file: &Path,
        change: impl FnOnce(&mut EngineSettings),
    ) -> Result<(), BackendError> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut settings = load_settings(file);
        change(&mut settings);
        save_settings(file, &settings).map_err(|e| BackendError::Settings {
            message: format!("エンジン設定を保存できませんでした: {e}"),
        })
    }
}

/// 設定ファイルの場所。
fn settings_file(app: &AppHandle) -> Result<PathBuf, BackendError> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join(SETTINGS_FILE_NAME))
        .map_err(|e| BackendError::Settings {
            message: format!("アプリの設定ディレクトリを決められません: {e}"),
        })
}

/// `launch_engine` が使う保存済みのパス。設定ファイルの場所が決められないときも起動は止めない。
pub fn saved_engine_path(app: &AppHandle) -> Option<String> {
    match settings_file(app) {
        Ok(file) => load_settings(&file).engine_path,
        Err(e) => {
            tracing::warn!(error = %e, "エンジン設定の場所が決められないので設定を使わない");
            None
        }
    }
}

fn current_payload(file: &Path) -> EngineSettingsPayload {
    let env = std::env::var(ENGINE_PATH_ENV).ok();
    build_payload(
        env.as_deref(),
        &load_settings(file),
        Path::new(DEFAULT_ENGINE_PATH),
    )
}

/// OS のファイル選択ダイアログを、呼び出したウィンドウを親にして開く。キャンセルなら `None`。
///
/// macOS はダイアログをメインスレッドで作る必要があるので、生成だけメインスレッドで行い、
/// 結果の待ち合わせはこのタスクで行う(tauri-plugin-dialog と同じ形)。
async fn pick_file(
    app: &AppHandle,
    window: WebviewWindow,
) -> Result<Option<PathBuf>, BackendError> {
    let (tx, rx) = oneshot::channel();
    app.run_on_main_thread(move || {
        let mut dialog = rfd::AsyncFileDialog::new().set_title("エンジンの実行ファイルを選ぶ");
        if cfg!(windows) {
            dialog = dialog
                .add_filter("実行ファイル", &["exe"])
                .add_filter("すべてのファイル", &["*"]);
        }
        let _ = tx.send(dialog.set_parent(&window).pick_file());
    })
    .map_err(|e| BackendError::Settings {
        message: format!("ファイル選択ダイアログを開けませんでした: {e}"),
    })?;
    let pending = rx.await.map_err(|_| BackendError::Settings {
        message: "ファイル選択ダイアログを開けませんでした".to_owned(),
    })?;
    Ok(pending.await.map(|handle| handle.path().to_path_buf()))
}

/// `get_engine_settings`: 有効なパス・その出所・保存済みのパスを返す。
#[tauri::command]
pub async fn get_engine_settings(app: AppHandle) -> Result<EngineSettingsPayload, BackendError> {
    Ok(current_payload(&settings_file(&app)?))
}

/// `pick_engine_path`: ダイアログで選んだファイルを確かめてから保存する。キャンセルなら何も変えない。
/// どちらの場合も、その時点の設定を返す。
#[tauri::command]
pub async fn pick_engine_path(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, EngineSettingsState>,
) -> Result<EngineSettingsPayload, BackendError> {
    let file = settings_file(&app)?;
    if let Some(picked) = pick_file(&app, window).await? {
        process::validate_engine_path(&picked)?;
        let picked = picked
            .into_os_string()
            .into_string()
            .map_err(|_| BackendError::Settings {
                message: "選んだファイルのパスに扱えない文字が含まれています".to_owned(),
            })?;
        state.update(&file, |settings| settings.engine_path = Some(picked))?;
    }
    Ok(current_payload(&file))
}

/// `clear_engine_path`: 保存済みのパスを消し、その後の設定を返す。
#[tauri::command]
pub async fn clear_engine_path(
    app: AppHandle,
    state: State<'_, EngineSettingsState>,
) -> Result<EngineSettingsPayload, BackendError> {
    let file = settings_file(&app)?;
    state.update(&file, |settings| settings.engine_path = None)?;
    Ok(current_payload(&file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::EnginePathSource;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// テストごとに別の空ディレクトリを作り、その中の設定ファイルのパスを返す。
    fn temp_settings_file() -> (PathBuf, PathBuf) {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "norves-engine-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("nested").join(SETTINGS_FILE_NAME);
        (dir, file)
    }

    fn write_raw(file: &Path, contents: &str) {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, contents).unwrap();
    }

    #[test]
    fn missing_file_loads_as_default() {
        let (dir, file) = temp_settings_file();
        assert_eq!(load_settings(&file), EngineSettings::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_file_loads_as_default() {
        let (dir, file) = temp_settings_file();
        for contents in ["{not json", "", "null", "[1]", "{\"enginePath\": 42}"] {
            write_raw(&file, contents);
            assert_eq!(
                load_settings(&file),
                EngineSettings::default(),
                "{contents:?} は既定の設定として読まれる"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_and_unknown_keys_are_accepted() {
        let (dir, file) = temp_settings_file();
        write_raw(&file, r#"{"futureKey": {"a": 1}}"#);
        let settings = load_settings(&file);
        assert_eq!(settings.engine_path, None);
        assert_eq!(settings.extra["futureKey"], serde_json::json!({"a": 1}));

        write_raw(
            &file,
            r#"{"enginePath": "C:/engine.exe", "futureKey": true}"#,
        );
        assert_eq!(
            load_settings(&file).engine_path.as_deref(),
            Some("C:/engine.exe")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn save_creates_directory_and_round_trips() {
        let (dir, file) = temp_settings_file();
        let settings = EngineSettings {
            engine_path: Some("C:/engines/norves.exe".to_owned()),
            ..EngineSettings::default()
        };
        save_settings(&file, &settings).expect("保存できる");
        assert_eq!(load_settings(&file), settings);
        assert!(
            !file.with_extension("json.tmp").exists(),
            "一時ファイルが残らない"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn update_keeps_unknown_keys_and_replaces_corrupt_file() {
        let (dir, file) = temp_settings_file();
        let state = EngineSettingsState::default();

        write_raw(&file, r#"{"enginePath": "old.exe", "futureKey": [1, 2]}"#);
        state
            .update(&file, |s| s.engine_path = Some("new.exe".to_owned()))
            .expect("保存できる");
        let saved = load_settings(&file);
        assert_eq!(saved.engine_path.as_deref(), Some("new.exe"));
        assert_eq!(saved.extra["futureKey"], serde_json::json!([1, 2]));

        state
            .update(&file, |s| s.engine_path = None)
            .expect("消せる");
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(raw, serde_json::json!({"futureKey": [1, 2]}));

        write_raw(&file, "{broken");
        state
            .update(&file, |s| s.engine_path = Some("after.exe".to_owned()))
            .expect("壊れたファイルを上書きできる");
        assert_eq!(
            load_settings(&file).engine_path.as_deref(),
            Some("after.exe")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn payload_prefers_env_over_settings_over_default() {
        let default = Path::new("default_engine");
        let saved = EngineSettings {
            engine_path: Some("saved.exe".to_owned()),
            ..EngineSettings::default()
        };

        let p = build_payload(Some("env.exe"), &saved, default);
        assert_eq!(p.effective_path, "env.exe");
        assert_eq!(p.source, EnginePathSource::Env);
        assert_eq!(
            p.saved_path.as_deref(),
            Some("saved.exe"),
            "上書きされていても保存値は返す"
        );

        let p = build_payload(Some("  "), &saved, default);
        assert_eq!(p.effective_path, "saved.exe");
        assert_eq!(p.source, EnginePathSource::Settings);

        let p = build_payload(None, &EngineSettings::default(), default);
        assert_eq!(p.effective_path, "default_engine");
        assert_eq!(p.source, EnginePathSource::Default);
        assert_eq!(p.saved_path, None);
    }

    #[test]
    fn payload_serializes_with_camel_case_source() {
        let p = build_payload(None, &EngineSettings::default(), Path::new("d"));
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            serde_json::json!({"effectivePath": "d", "source": "default", "savedPath": null})
        );
    }
}
