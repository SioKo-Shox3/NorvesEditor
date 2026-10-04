//! バックエンドの `tracing` の WARN 以上を、stderr と OS のアプリのログディレクトリ(`app_log_dir`)の
//! [`LOG_FILE_NAME`] の両方へ出す。
//!
//! Windows の配布版は `windows_subsystem = "windows"` でコンソールが無く、stderr だけでは警告
//! (Job への割り当て失敗など)が失われる。ファイルは追記で開き、1 件ごとに書き込む(バッファしない)
//! ので、強制終了されてもそれまでの警告は残る。
//!
//! 保持方針: 起動時にファイルが [`MAX_LOG_BYTES`] を超えていれば [`ROTATED_FILE_NAME`] へ移し
//! (前回の移動分は上書き)、新しいファイルから書き始める。残るのは最大 2 世代。WARN 以上だけなので
//! 1 回の起動で上限を大きく超えることは想定せず、実行中の切り替えはしない。

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use tracing_subscriber::fmt::MakeWriter;

/// ログディレクトリ内の現在のログファイル名。
pub const LOG_FILE_NAME: &str = "backend.log";
/// 上限を超えた前回までのログの移動先。
pub const ROTATED_FILE_NAME: &str = "backend.log.1";
/// 起動時にこれを超えていれば 1 世代ずらす(1 MiB)。
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// `dir` を作り、必要なら 1 世代ずらしてから、ログファイルを追記で開く。
pub fn open_log_file(dir: &Path) -> io::Result<File> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(LOG_FILE_NAME);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        std::fs::rename(&path, dir.join(ROTATED_FILE_NAME))?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// stderr と(開けていれば)ログファイルの両方へ書く [`MakeWriter`]。
#[derive(Clone)]
pub struct TeeMakeWriter {
    file: Option<Arc<Mutex<File>>>,
}

impl TeeMakeWriter {
    pub fn new(file: Option<File>) -> Self {
        Self {
            file: file.map(|f| Arc::new(Mutex::new(f))),
        }
    }
}

/// 1 件分の書き込み先。ファイルのロックは 1 件の書き込みの間だけ持つ。
pub struct TeeWriter<'a> {
    file: Option<MutexGuard<'a, File>>,
}

impl Write for TeeWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // どちらかへ書けなくても、もう一方と呼び出し側(ログを出したコード)は止めない。
        let _ = io::stderr().write_all(buf);
        if let Some(file) = self.file.as_mut() {
            let _ = file.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
        }
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for TeeMakeWriter {
    type Writer = TeeWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        TeeWriter {
            // 別スレッドが書き込み中に panic してロックが毒されても、ログは書き続ける。
            file: self
                .file
                .as_ref()
                .map(|f| f.lock().unwrap_or_else(|e| e.into_inner())),
        }
    }
}

/// WARN 以上を `writer` へ出す subscriber を作る。
pub fn subscriber(writer: TeeMakeWriter) -> impl tracing::Subscriber + Send + Sync {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_writer(writer)
        .finish()
}

/// `log_dir` のログファイルと stderr へ出す subscriber を大域に設定し、ログファイルのパスを返す。
/// ログディレクトリが決められない・開けないときは stderr だけへ出し、その旨を警告する。
/// 既に初期化済み(テスト等)なら何もしない。
pub fn init(log_dir: Option<PathBuf>) -> Option<PathBuf> {
    let opened = log_dir.map(|dir| {
        let path = dir.join(LOG_FILE_NAME);
        open_log_file(&dir).map(|file| (path, file))
    });
    let (path, file, open_error) = match opened {
        Some(Ok((path, file))) => (Some(path), Some(file), None),
        Some(Err(e)) => (None, None, Some(e.to_string())),
        None => (
            None,
            None,
            Some("ログディレクトリが決められない".to_owned()),
        ),
    };
    if tracing::subscriber::set_global_default(subscriber(TeeMakeWriter::new(file))).is_err() {
        return None;
    }
    if let Some(error) = open_error {
        tracing::warn!(error = %error, "ログファイルを開けないので警告は stderr にだけ出す");
    }
    path
}

/// テスト用: `dir` のログファイルへ出す subscriber の下で `f` を走らせ、ファイルの中身を返す。
#[cfg(test)]
pub(crate) fn capture_warnings(dir: &Path, f: impl FnOnce()) -> String {
    let file = open_log_file(dir).expect("ログファイルを開けない");
    tracing::subscriber::with_default(subscriber(TeeMakeWriter::new(Some(file))), f);
    std::fs::read_to_string(dir.join(LOG_FILE_NAME)).expect("ログファイルを読めない")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// テストごとに別の空ディレクトリのパスを返す(作りはしない)。
    fn temp_log_dir() -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "norves-backend-log-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn warnings_reach_the_log_file_and_lower_levels_do_not() {
        let dir = temp_log_dir();
        let log = capture_warnings(&dir, || {
            tracing::warn!("警告の本文");
            tracing::error!("エラーの本文");
            tracing::info!("情報の本文");
        });
        assert!(log.contains("警告の本文"), "{log}");
        assert!(log.contains("エラーの本文"), "{log}");
        assert!(!log.contains("情報の本文"), "{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reopening_appends_to_the_existing_log() {
        let dir = temp_log_dir();
        capture_warnings(&dir, || tracing::warn!("1 回目"));
        let log = capture_warnings(&dir, || tracing::warn!("2 回目"));
        assert!(log.contains("1 回目") && log.contains("2 回目"), "{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_log_is_rotated_on_open_keeping_one_generation() {
        let dir = temp_log_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(ROTATED_FILE_NAME), "さらに古い世代").unwrap();
        let old = vec![b'x'; MAX_LOG_BYTES as usize + 1];
        std::fs::write(dir.join(LOG_FILE_NAME), &old).unwrap();

        let log = capture_warnings(&dir, || tracing::warn!("新しい警告"));

        assert!(!log.starts_with('x') && log.contains("新しい警告"), "{log}");
        let rotated = std::fs::read(dir.join(ROTATED_FILE_NAME)).unwrap();
        assert_eq!(rotated, old, "前回のログが 1 世代前へ移っていない");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_at_the_limit_is_not_rotated() {
        let dir = temp_log_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LOG_FILE_NAME), vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        open_log_file(&dir).unwrap();
        assert!(!dir.join(ROTATED_FILE_NAME).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
