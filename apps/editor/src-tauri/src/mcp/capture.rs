//! MCP の撮影用ディレクトリの命名、排他、保存期間を管理する。

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::sync::{Mutex, OwnedMutexGuard};

pub(crate) const CAPTURE_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub(crate) const MAX_RETAINED_CAPTURES: usize = 200;

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct CaptureStore {
    inner: Arc<CaptureStoreInner>,
}

struct CaptureStoreInner {
    directory: PathBuf,
    next_sequence: AtomicU64,
    busy: Arc<Mutex<()>>,
}

struct CaptureFile {
    path: PathBuf,
    name: String,
    modified: SystemTime,
}

#[allow(dead_code)]
impl CaptureStore {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self {
            inner: Arc::new(CaptureStoreInner {
                directory,
                next_sequence: AtomicU64::new(1),
                busy: Arc::new(Mutex::new(())),
            }),
        }
    }

    /// 設定された撮影用ディレクトリを、正規化する前の形で返す。
    pub(crate) fn directory(&self) -> &Path {
        &self.inner.directory
    }

    /// 撮影用ディレクトリを作り、正規化した絶対パスを返す。
    pub(crate) fn prepare(&self) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.inner.directory)?;
        fs::canonicalize(&self.inner.directory)
    }

    /// 撮影用ディレクトリを片付け、削除できたファイル数を返す。
    pub(crate) fn cleanup(&self, now: SystemTime) -> io::Result<usize> {
        let directory = self.prepare()?;
        let mut captures = Vec::new();
        for entry in fs::read_dir(directory)?.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str().filter(|name| is_capture_file_name(name)) else {
                continue;
            };
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.file_type().is_file() {
                continue;
            }
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            captures.push(CaptureFile {
                path,
                name: name.to_owned(),
                modified,
            });
        }

        captures.sort_unstable_by(|a, b| {
            a.modified
                .cmp(&b.modified)
                .then_with(|| a.name.cmp(&b.name))
        });

        let mut removed = 0;
        let mut retained = Vec::new();
        for capture in captures {
            let age = now.duration_since(capture.modified).unwrap_or_default();
            if age > CAPTURE_RETENTION && fs::remove_file(&capture.path).is_ok() {
                removed += 1;
            } else {
                retained.push(capture);
            }
        }

        let excess = retained.len().saturating_sub(MAX_RETAINED_CAPTURES);
        let mut excess_removed = 0;
        for capture in retained {
            if excess_removed == excess {
                break;
            }
            if fs::remove_file(capture.path).is_ok() {
                removed += 1;
                excess_removed += 1;
            }
        }
        Ok(removed)
    }

    /// UTC の日時と連番から撮影ファイルの出力名を作る。
    pub(crate) fn next_output_name(&self, now: SystemTime) -> String {
        let seconds = match now.duration_since(UNIX_EPOCH) {
            Ok(duration) => i128::from(duration.as_secs()),
            Err(error) => {
                let duration = error.duration();
                -i128::from(duration.as_secs()) - i128::from(duration.subsec_nanos() != 0)
            }
        };
        // 年を必ず 4 桁に収め、命名規則から外れる時刻を端点に丸める。
        let seconds = seconds.clamp(-62_167_219_200, 253_402_300_799) as i64;
        let days = seconds.div_euclid(86_400);
        let second_of_day = seconds.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let hour = second_of_day / 3_600;
        let minute = second_of_day % 3_600 / 60;
        let second = second_of_day % 60;
        let sequence = self.inner.next_sequence.fetch_add(1, Ordering::Relaxed);
        format!("cap-{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z-{sequence}")
    }

    /// 撮影が進行中でなければ排他権を取得する。
    pub(crate) fn try_begin(&self) -> Option<OwnedMutexGuard<()>> {
        Arc::clone(&self.inner.busy).try_lock_owned().ok()
    }
}

fn is_capture_file_name(name: &str) -> bool {
    let Some(stem) = name
        .strip_suffix(".png.tmp")
        .or_else(|| name.strip_suffix(".png"))
    else {
        return false;
    };
    let bytes = stem.as_bytes();
    if !(22..=41).contains(&bytes.len()) {
        return false;
    }
    bytes.starts_with(b"cap-")
        && bytes[4..12].iter().all(u8::is_ascii_digit)
        && bytes[12] == b'T'
        && bytes[13..19].iter().all(u8::is_ascii_digit)
        && &bytes[19..21] == b"Z-"
        && bytes[21..].iter().all(u8::is_ascii_digit)
}

/// Unix 時代からの日数をグレゴリオ暦の年月日に変換する。
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = shifted_month + if shifted_month < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("時刻を取得")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "norves-mcp-capture-test-{}-{stamp}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("一時ディレクトリを作成");
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn create_file(path: &Path, modified: SystemTime) {
        File::create(path)
            .expect("試験ファイルを作成")
            .set_modified(modified)
            .expect("試験ファイルの更新時刻を設定");
    }

    #[test]
    fn cleanup_applies_age_and_count_limits() {
        let dir = TestDir::new();
        let store = CaptureStore::new(dir.0.join("captures"));
        let capture_dir = store.prepare().expect("撮影用ディレクトリを準備");
        let now = SystemTime::now();
        let expired = [
            "cap-20261103T223633Z-1001.png",
            "cap-20261103T223633Z-1002.png.tmp",
            "cap-20261103T223633Z-1003.png",
        ];
        for name in expired {
            create_file(
                &capture_dir.join(name),
                now - Duration::from_secs(8 * 86_400),
            );
        }
        for sequence in 1..=201 {
            let modified = if sequence <= 2 {
                now - CAPTURE_RETENTION
            } else {
                now - Duration::from_secs(202 - sequence)
            };
            create_file(
                &capture_dir.join(format!("cap-20261103T223633Z-{sequence}.png")),
                modified,
            );
        }

        assert_eq!(store.cleanup(now).expect("撮影ファイルを片付け"), 4);
        for name in expired {
            assert!(!capture_dir.join(name).exists());
        }
        assert!(!capture_dir.join("cap-20261103T223633Z-1.png").exists());
        assert!(capture_dir.join("cap-20261103T223633Z-2.png").exists());
        assert_eq!(fs::read_dir(capture_dir).expect("一覧を取得").count(), 200);
    }

    #[test]
    fn cleanup_preserves_other_entries_and_names_are_shared() {
        let dir = TestDir::new();
        let store = CaptureStore::new(dir.0.join("captures"));
        let clone = store.clone();
        assert_eq!(store.directory(), dir.0.join("captures"));
        let capture_dir = store.prepare().expect("撮影用ディレクトリを準備");
        let now = SystemTime::now();
        let old = now - Duration::from_secs(8 * 86_400);
        let other_png = capture_dir.join("other.png");
        let other_old = capture_dir.join("cap-invalid.txt");
        let nested = capture_dir.join("cap-20261103T223633Z-3.png");
        create_file(&other_png, now);
        create_file(&other_old, old);
        fs::create_dir(&nested).expect("対象外のディレクトリを作成");
        let nested_file = nested.join("cap-20261103T223633Z-4.png");
        create_file(&nested_file, old);

        let outside = dir.0.join("outside.png");
        let link = capture_dir.join("cap-20261103T223633Z-5.png");
        create_file(&outside, old);
        #[cfg(unix)]
        let symlink_created = std::os::unix::fs::symlink(&outside, &link).is_ok();
        #[cfg(windows)]
        let symlink_created = std::os::windows::fs::symlink_file(&outside, &link).is_ok();
        #[cfg(not(any(unix, windows)))]
        let symlink_created = false;
        if !symlink_created {
            eprintln!("シンボリックリンクを作成できないため、その確認を省略");
        }

        assert_eq!(store.cleanup(now).expect("撮影ファイルを片付け"), 0);
        assert!(other_png.is_file());
        assert!(other_old.is_file());
        assert!(nested.is_dir());
        assert!(nested_file.is_file());
        assert!(outside.is_file());
        if symlink_created {
            assert!(fs::symlink_metadata(&link)
                .expect("シンボリックリンクの情報を取得")
                .file_type()
                .is_symlink());
        }

        let name = store.next_output_name(UNIX_EPOCH + Duration::from_secs(1_793_745_393));
        let next = clone.next_output_name(UNIX_EPOCH + Duration::from_secs(951_827_696));
        assert_eq!(name, "cap-20261103T223633Z-1");
        assert_eq!(next, "cap-20000229T123456Z-2");
        assert!(name.starts_with("cap-") && name.len() <= 64);
        assert!(is_capture_file_name(&format!("{name}.png")));
        assert!(is_capture_file_name(&format!("{next}.png.tmp")));
        assert_ne!(name, next);
        assert_eq!(
            store.next_output_name(UNIX_EPOCH - Duration::from_secs(1)),
            "cap-19691231T235959Z-3"
        );
        for invalid in [
            "cap-20261103T223633Z-.png",
            "cap-20261103T223633Z-123456789012345678901.png",
            "cap-20261103T223633Z-1.png.tmp.extra",
            "cap-20261103T223633Z-１.png",
        ] {
            assert!(!is_capture_file_name(invalid));
        }

        let guard = store.try_begin().expect("撮影の排他権を取得");
        assert!(clone.try_begin().is_none());
        drop(guard);
        assert!(clone.try_begin().is_some());
    }
}
