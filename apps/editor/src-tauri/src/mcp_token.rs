//! MCP 認証トークンを生成し、OS の利用者境界で保護して永続化する。

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use subtle::ConstantTimeEq;

/// MCP 専用設定ディレクトリ名。
pub const MCP_DIRECTORY_NAME: &str = "mcp";
/// 保護したトークンを保存するファイル名。
pub const TOKEN_FILE_NAME: &str = "mcp-token.bin";
/// トークンの生バイト数。
pub const TOKEN_SIZE: usize = 32;

const MAX_STORED_SIZE: u64 = 16 * 1024;
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// メモリ上の MCP 認証トークン。デバッグ出力と複製は実装しない。
pub struct McpToken([u8; TOKEN_SIZE]);

impl McpToken {
    fn generate() -> Result<Self, McpTokenError> {
        let mut bytes = [0; TOKEN_SIZE];
        if getrandom::fill(&mut bytes).is_err() {
            bytes.fill(0);
            return Err(McpTokenError::Random);
        }
        Ok(Self(bytes))
    }

    /// Settings に明示表示するときの URL-safe Base64 表現を返す。
    pub fn expose(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// URL-safe Base64 の提示値を、トークン本体と定時間で照合する。
    pub fn matches(&self, presented: &str) -> bool {
        let Ok(mut decoded) = URL_SAFE_NO_PAD.decode(presented) else {
            return false;
        };
        if decoded.len() != TOKEN_SIZE {
            decoded.fill(0);
            return false;
        }
        let matches = bool::from(self.0.as_slice().ct_eq(decoded.as_slice()));
        decoded.fill(0);
        matches
    }
}

impl Drop for McpToken {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// MCP トークンのファイル操作を直列化するストア。
#[derive(Default)]
pub struct McpTokenStore {
    lock: Mutex<()>,
}

impl McpTokenStore {
    /// 保存済みトークンを読み込む。ファイルが無い初回だけ新しいトークンを作る。
    pub fn load_or_create(&self, app_config_dir: &Path) -> Result<McpToken, McpTokenError> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = prepare_token_directory(app_config_dir)?;
        match load_existing(&directory.join(TOKEN_FILE_NAME)) {
            Ok(token) => Ok(token),
            Err(McpTokenError::Missing) => create_token(&directory),
            Err(error) => Err(error),
        }
    }

    /// OS 乱数でトークンを作り、保護した内容を原子的に置き換える。
    pub fn regenerate(&self, app_config_dir: &Path) -> Result<McpToken, McpTokenError> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = prepare_token_directory(app_config_dir)?;
        create_token(&directory)
    }
}

/// トークンの乱数・保護・保存・復号で起きた、安全な表示文だけを持つエラー。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTokenError {
    /// 保存ファイルが無い。初回生成の判定にだけ使う。
    Missing,
    /// ファイル操作または専用ディレクトリの保護に失敗した。
    Io,
    /// 保存内容が壊れているか、想定外の形式。
    InvalidData,
    /// OS の暗号化または復号に失敗した。
    Protection,
    /// OS の乱数取得に失敗した。
    Random,
}

impl std::fmt::Display for McpTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Missing => "MCP トークンが保存されていません",
            Self::Io => "MCP トークンを安全に読み書きできませんでした",
            Self::InvalidData => "MCP トークンの保存内容が壊れているか形式が正しくありません",
            Self::Protection => "MCP トークンを保護または復号できませんでした",
            Self::Random => "安全な乱数を取得できませんでした",
        };
        f.write_str(message)
    }
}

impl std::error::Error for McpTokenError {}

fn create_token(directory: &Path) -> Result<McpToken, McpTokenError> {
    let token = McpToken::generate()?;
    let protected = protect(token.0.as_slice())?;
    write_atomically(
        directory,
        &directory.join(TOKEN_FILE_NAME),
        protected.as_slice(),
    )?;
    Ok(token)
}

fn load_existing(file: &Path) -> Result<McpToken, McpTokenError> {
    let metadata = match fs::symlink_metadata(file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(McpTokenError::Missing)
        }
        Err(_) => return Err(McpTokenError::Io),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(McpTokenError::InvalidData);
    }
    #[cfg(unix)]
    fs::set_permissions(file, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(|_| McpTokenError::Io)?;
    if metadata.len() == 0 || metadata.len() > MAX_STORED_SIZE {
        return Err(McpTokenError::InvalidData);
    }

    let input = File::open(file).map_err(|_| McpTokenError::Io)?;
    let mut stored = SecretBytes::default();
    input
        .take(MAX_STORED_SIZE + 1)
        .read_to_end(&mut stored.0)
        .map_err(|_| McpTokenError::Io)?;
    if stored.0.is_empty() || stored.0.len() as u64 > MAX_STORED_SIZE {
        return Err(McpTokenError::InvalidData);
    }

    let plaintext = unprotect(stored.as_slice())?;
    if plaintext.0.len() != TOKEN_SIZE {
        return Err(McpTokenError::InvalidData);
    }
    let mut token_bytes = [0; TOKEN_SIZE];
    token_bytes.copy_from_slice(plaintext.as_slice());
    Ok(McpToken(token_bytes))
}

fn prepare_token_directory(app_config_dir: &Path) -> Result<PathBuf, McpTokenError> {
    fs::create_dir_all(app_config_dir).map_err(|_| McpTokenError::Io)?;
    let directory = app_config_dir.join(MCP_DIRECTORY_NAME);
    prepare_private_directory(&directory)?;
    Ok(directory)
}

#[cfg(unix)]
fn prepare_private_directory(directory: &Path) -> Result<(), McpTokenError> {
    use std::os::unix::fs::DirBuilderExt;

    match fs::symlink_metadata(directory) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(McpTokenError::Io);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(McpTokenError::Io),
            }
            let metadata = fs::symlink_metadata(directory).map_err(|_| McpTokenError::Io)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(McpTokenError::Io);
            }
        }
        Err(_) => return Err(McpTokenError::Io),
    }
    fs::set_permissions(
        directory,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .map_err(|_| McpTokenError::Io)
}

#[cfg(not(unix))]
fn prepare_private_directory(directory: &Path) -> Result<(), McpTokenError> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(McpTokenError::Io)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(directory).map_err(|_| McpTokenError::Io)
        }
        Err(_) => Err(McpTokenError::Io),
    }
}

fn write_atomically(
    directory: &Path,
    destination: &Path,
    contents: &[u8],
) -> Result<(), McpTokenError> {
    let (mut output, mut temporary) = create_temporary_file(directory)?;
    output.write_all(contents).map_err(|_| McpTokenError::Io)?;
    #[cfg(unix)]
    output
        .set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(|_| McpTokenError::Io)?;
    output.sync_all().map_err(|_| McpTokenError::Io)?;
    drop(output);
    fs::rename(&temporary.path, destination).map_err(|_| McpTokenError::Io)?;
    temporary.committed = true;
    sync_directory(directory)?;
    Ok(())
}

fn create_temporary_file(directory: &Path) -> Result<(File, TemporaryFile), McpTokenError> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    for _ in 0..32 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".mcp-token.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&path) {
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
            Err(_) => return Err(McpTokenError::Io),
        }
    }
    Err(McpTokenError::Io)
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
fn sync_directory(directory: &Path) -> Result<(), McpTokenError> {
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| McpTokenError::Io)
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> Result<(), McpTokenError> {
    Ok(())
}

#[cfg(unix)]
fn protect(plaintext: &[u8]) -> Result<SecretBytes, McpTokenError> {
    if plaintext.len() != TOKEN_SIZE {
        return Err(McpTokenError::InvalidData);
    }
    Ok(SecretBytes(plaintext.to_vec()))
}

#[cfg(unix)]
fn unprotect(stored: &[u8]) -> Result<SecretBytes, McpTokenError> {
    if stored.len() != TOKEN_SIZE {
        return Err(McpTokenError::InvalidData);
    }
    Ok(SecretBytes(stored.to_vec()))
}

#[cfg(windows)]
fn protect(plaintext: &[u8]) -> Result<SecretBytes, McpTokenError> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(plaintext.len()).map_err(|_| McpTokenError::InvalidData)?,
        pbData: plaintext.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    // 入力バッファは同期呼び出し中に有効で、DPAPI は利用者スコープの既定保護を使う。
    let succeeded = unsafe {
        CryptProtectData(
            &input,
            null(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    let allocation = LocalAllocation(output.pbData);
    if succeeded == 0 {
        return Err(McpTokenError::Protection);
    }
    if allocation.0.is_null() || output.cbData == 0 || output.cbData as u64 > MAX_STORED_SIZE {
        return Err(McpTokenError::Protection);
    }
    // 成功した DPAPI 呼び出しが返した長さのバッファをコピーし、後で LocalFree する。
    let bytes = unsafe { std::slice::from_raw_parts(allocation.0, output.cbData as usize) };
    Ok(SecretBytes(bytes.to_vec()))
}

#[cfg(windows)]
fn unprotect(stored: &[u8]) -> Result<SecretBytes, McpTokenError> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    if stored.is_empty() || stored.len() as u64 > MAX_STORED_SIZE {
        return Err(McpTokenError::InvalidData);
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(stored.len()).map_err(|_| McpTokenError::InvalidData)?,
        pbData: stored.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    // 保存時と同じ利用者資格情報で復号し、UIを出さない。
    let succeeded = unsafe {
        CryptUnprotectData(
            &input,
            null_mut(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    let allocation = LocalAllocation(output.pbData);
    if succeeded == 0 {
        return Err(McpTokenError::Protection);
    }
    if allocation.0.is_null() || output.cbData == 0 || output.cbData as u64 > MAX_STORED_SIZE {
        return Err(McpTokenError::InvalidData);
    }
    // 成功した DPAPI 呼び出しが返した長さのバッファをコピーし、後で LocalFree する。
    let bytes = unsafe { std::slice::from_raw_parts(allocation.0, output.cbData as usize) };
    Ok(SecretBytes(bytes.to_vec()))
}

#[cfg(windows)]
struct LocalAllocation(*mut u8);

#[cfg(windows)]
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            use windows_sys::Win32::Foundation::{LocalFree, HLOCAL};
            // DPAPI が LocalAlloc 系で確保した出力領域を、対応する LocalFree で解放する。
            unsafe {
                LocalFree(self.0.cast::<core::ffi::c_void>() as HLOCAL);
            }
        }
    }
}

#[derive(Default)]
struct SecretBytes(Vec<u8>);

impl SecretBytes {
    fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
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
                "norves-mcp-token-{}-{sequence}",
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
    fn token_is_random_persistent_and_regenerable() {
        let directory = TestDirectory::new();
        let store = McpTokenStore::default();
        let initial = store
            .load_or_create(&directory.0)
            .expect("初回トークンを作る");
        let initial_value = initial.expose();
        assert_eq!(initial_value.len(), 43);
        assert!(initial.matches(&initial_value));
        assert!(!initial.matches("invalid"));

        let loaded = store
            .load_or_create(&directory.0)
            .expect("保存したトークンを読む");
        assert!(loaded.matches(&initial_value));

        let replacement = store.regenerate(&directory.0).expect("トークンを作り直す");
        let replacement_value = replacement.expose();
        assert!(!replacement.matches(&initial_value));
        assert!(replacement.matches(&replacement_value));
        let reloaded = store
            .load_or_create(&directory.0)
            .expect("作り直したトークンを読む");
        assert!(reloaded.matches(&replacement_value));
        assert!(!reloaded.matches(&initial_value));
        assert_eq!(
            fs::read_dir(directory.0.join(MCP_DIRECTORY_NAME))
                .expect("トークンディレクトリを読む")
                .count(),
            1
        );
    }

    #[test]
    fn corrupt_token_fails_closed_until_explicit_regeneration() {
        let directory = TestDirectory::new();
        let token_directory = directory.0.join(MCP_DIRECTORY_NAME);
        fs::create_dir_all(&token_directory).expect("トークンディレクトリを作る");
        fs::write(token_directory.join(TOKEN_FILE_NAME), b"corrupt-marker")
            .expect("破損データを置く");

        let store = McpTokenStore::default();
        let error = match store.load_or_create(&directory.0) {
            Err(error) => error,
            Ok(_) => panic!("破損データを拒否する"),
        };
        assert!(matches!(
            error,
            McpTokenError::InvalidData | McpTokenError::Protection
        ));
        assert!(!error.to_string().contains("corrupt-marker"));

        let recreated = store.regenerate(&directory.0).expect("明示的に作り直す");
        let value = recreated.expose();
        assert!(store
            .load_or_create(&directory.0)
            .expect("作り直したトークンを読む")
            .matches(&value));
    }

    #[test]
    fn failed_atomic_replace_leaves_no_temporary_secret_file() {
        let directory = TestDirectory::new();
        let token_directory = directory.0.join(MCP_DIRECTORY_NAME);
        fs::create_dir_all(&token_directory).expect("トークンディレクトリを作る");
        fs::create_dir(token_directory.join(TOKEN_FILE_NAME)).expect("保存先をディレクトリにする");
        let store = McpTokenStore::default();
        assert_eq!(
            store.regenerate(&directory.0).err(),
            Some(McpTokenError::Io)
        );
        assert_eq!(
            fs::read_dir(token_directory)
                .expect("トークンディレクトリを読む")
                .count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_permissions_are_limited_to_the_token_directory_and_file() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let directory = TestDirectory::new();
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o755))
            .expect("設定ディレクトリ権限を固定する");
        McpTokenStore::default()
            .load_or_create(&directory.0)
            .expect("トークンを保存する");
        let token_directory = directory.0.join(MCP_DIRECTORY_NAME);
        let token_file = token_directory.join(TOKEN_FILE_NAME);
        assert_eq!(
            fs::metadata(&token_directory)
                .expect("専用ディレクトリ情報")
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&token_file)
                .expect("トークンファイル情報")
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&directory.0)
                .expect("設定ディレクトリ情報")
                .mode()
                & 0o777,
            0o755
        );
        assert_eq!(
            fs::metadata(&token_file)
                .expect("トークンファイル情報")
                .len(),
            TOKEN_SIZE as u64
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_file_contains_dpapi_ciphertext_and_round_trips_for_this_user() {
        let directory = TestDirectory::new();
        let store = McpTokenStore::default();
        let token = store
            .load_or_create(&directory.0)
            .expect("DPAPI でトークンを保護する");
        let value = token.expose();
        let bytes = fs::read(directory.0.join(MCP_DIRECTORY_NAME).join(TOKEN_FILE_NAME))
            .expect("保護したトークンを読む");
        assert!(bytes.len() > TOKEN_SIZE);
        assert!(store
            .load_or_create(&directory.0)
            .expect("同じ利用者としてトークンを復号する")
            .matches(&value));
    }
}
