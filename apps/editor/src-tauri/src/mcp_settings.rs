//! MCP の有効状態と待ち受けポートをアプリ設定ディレクトリへ保存する。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use url::Url;

/// MCP 設定ファイル名。
pub const SETTINGS_FILE_NAME: &str = "mcp-settings.json";
/// 既定の MCP loopback ポート。
pub const DEFAULT_PORT: u16 = 49_770;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// MCP サーバーの保存設定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct McpSettings {
    /// MCP サーバーを起動するか。
    pub enabled: bool,
    /// loopback HTTP のポート。
    pub port: u16,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
        }
    }
}

impl McpSettings {
    /// アプリ設定ディレクトリから設定を読み込む。設定が無い場合だけ既定値を返す。
    pub fn load(app_config_dir: &Path) -> Result<Self, McpSettingsError> {
        let file = app_config_dir.join(SETTINGS_FILE_NAME);
        let bytes = match fs::read(file) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(_) => return Err(McpSettingsError::Io),
        };
        let settings: Self =
            serde_json::from_slice(&bytes).map_err(|_| McpSettingsError::Invalid)?;
        settings.validate()?;
        Ok(settings)
    }

    /// 設定を同じディレクトリの一時ファイルから原子的に置き換える。
    pub fn save(&self, app_config_dir: &Path) -> Result<(), McpSettingsError> {
        self.validate()?;
        fs::create_dir_all(app_config_dir).map_err(|_| McpSettingsError::Io)?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| McpSettingsError::Invalid)?;
        write_atomically(&app_config_dir.join(SETTINGS_FILE_NAME), &bytes)
    }

    /// 設定ポートを含む MCP の loopback URL を返す。
    pub fn endpoint(&self) -> Result<Url, McpSettingsError> {
        self.validate()?;
        let mut endpoint =
            Url::parse("http://127.0.0.1:49770/mcp").map_err(|_| McpSettingsError::Endpoint)?;
        endpoint
            .set_port(Some(self.port))
            .map_err(|_| McpSettingsError::Endpoint)?;
        Ok(endpoint)
    }

    fn validate(&self) -> Result<(), McpSettingsError> {
        if self.port == 0 {
            return Err(McpSettingsError::InvalidPort);
        }
        Ok(())
    }
}

/// MCP 設定の読込・検証・保存で起きた、安全な表示文だけを持つエラー。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpSettingsError {
    /// ファイル操作に失敗した。
    Io,
    /// 設定 JSON を解析できない。
    Invalid,
    /// ポート番号が範囲外。
    InvalidPort,
    /// MCP URL を組み立てられない。
    Endpoint,
}

impl std::fmt::Display for McpSettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Io => "MCP の設定を読み書きできませんでした",
            Self::Invalid => "MCP の設定が壊れているか、形式が正しくありません",
            Self::InvalidPort => "MCP のポート番号は1〜65535で指定してください",
            Self::Endpoint => "MCP の接続先 URL を作成できませんでした",
        };
        f.write_str(message)
    }
}

impl std::error::Error for McpSettingsError {}

fn write_atomically(file: &Path, contents: &[u8]) -> Result<(), McpSettingsError> {
    let directory = file.parent().ok_or(McpSettingsError::Io)?;
    let (mut output, mut temporary) = create_temporary_file(directory)?;
    output
        .write_all(contents)
        .map_err(|_| McpSettingsError::Io)?;
    output.sync_all().map_err(|_| McpSettingsError::Io)?;
    drop(output);
    fs::rename(&temporary.path, file).map_err(|_| McpSettingsError::Io)?;
    temporary.committed = true;
    sync_directory(directory)?;
    Ok(())
}

fn create_temporary_file(directory: &Path) -> Result<(File, TemporaryFile), McpSettingsError> {
    for _ in 0..32 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".mcp-settings.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                return Ok((
                    file,
                    TemporaryFile {
                        path,
                        committed: false,
                    },
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(McpSettingsError::Io),
        }
    }
    Err(McpSettingsError::Io)
}

struct TemporaryFile {
    path: PathBuf,
    committed: bool,
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<(), McpSettingsError> {
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| McpSettingsError::Io)
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> Result<(), McpSettingsError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "norves-mcp-settings-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("一時設定ディレクトリを作成する");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn default_is_disabled_on_the_reserved_port() {
        let settings = McpSettings::default();
        assert!(!settings.enabled);
        assert_eq!(settings.port, 49_770);
        assert_eq!(
            settings.endpoint().expect("URL を作成する").as_str(),
            "http://127.0.0.1:49770/mcp"
        );
    }

    #[test]
    fn settings_round_trip_and_atomically_replace_previous_contents() {
        let directory = TestDirectory::new();
        McpSettings::default()
            .save(&directory.0)
            .expect("既定設定を保存する");
        let updated = McpSettings {
            enabled: true,
            port: u16::MAX,
        };
        updated.save(&directory.0).expect("設定を置き換える");
        assert_eq!(McpSettings::load(&directory.0), Ok(updated));
        assert_eq!(
            fs::read_dir(&directory.0)
                .expect("設定ディレクトリを読む")
                .count(),
            1
        );
    }

    #[test]
    fn missing_settings_use_defaults_but_corruption_fails_closed() {
        let directory = TestDirectory::new();
        assert_eq!(McpSettings::load(&directory.0), Ok(McpSettings::default()));
        fs::write(directory.0.join(SETTINGS_FILE_NAME), b"{broken").expect("壊れた設定を書き込む");
        assert_eq!(
            McpSettings::load(&directory.0),
            Err(McpSettingsError::Invalid)
        );
    }

    #[test]
    fn invalid_port_is_rejected_without_replacing_saved_settings() {
        let directory = TestDirectory::new();
        let original = McpSettings::default();
        original.save(&directory.0).expect("既定設定を保存する");
        let invalid = McpSettings {
            enabled: true,
            port: 0,
        };
        assert_eq!(
            invalid.save(&directory.0),
            Err(McpSettingsError::InvalidPort)
        );
        assert_eq!(McpSettings::load(&directory.0), Ok(original));
    }
}
