//! 決定的な撮影の要求と完了イベントの検証。

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use norves_bridge_core::{
    CorrelationId, MethodName, ResponsePayload, ValidatedEnvelope, VersionString,
};
use serde_json::{json, Map, Value};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{timeout_at, Instant};

use crate::dispatcher::{DispatchHandle, RequestError};

// 撮影はプロトコル 0.2 の面に属する。
const PROTOCOL_VERSION: &str = "0.2";
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const CAPTURE_METHOD: &str = "viewport.captureDeterministic";
const COMPLETED_EVENT: &str = "viewport.captureCompleted";

/// 撮影先と撮影条件を指定する要求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    /// エディタが用意した撮影用ディレクトリの絶対パス。
    pub capture_dir: PathBuf,
    /// 拡張子を含まない出力ファイル名。
    pub output_name: String,
    /// 描画前に進める固定刻みの数。
    pub settle_frames: u32,
    /// 出力画像の最大幅。
    pub max_width: u32,
    /// 出力画像の最大高さ。
    pub max_height: u32,
}

/// 正常に完了した撮影のファイルと画像情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureCompleted {
    /// 受け付け時に割り当てられた撮影 ID。
    pub capture_id: String,
    /// 検証済みの出力ファイルのパス。
    pub path: PathBuf,
    /// 出力画像の幅。
    pub width: u32,
    /// 出力画像の高さ。
    pub height: u32,
    /// 撮影エポック内の固定刻み。
    pub frame: u64,
    /// RGBA8 画素の FNV-1a 64 ビット要約値。
    pub pixel_digest: String,
    /// 出力ファイルのバイト数。
    pub file_bytes: u64,
}

/// 時間切れになった撮影の段階。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTimeoutPhase {
    /// 要求の受け付けを待っている段階。
    Accept,
    /// 撮影の完了を待っている段階。
    Completion,
}

impl fmt::Display for CaptureTimeoutPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accept => f.write_str("受け付け"),
            Self::Completion => f.write_str("完了"),
        }
    }
}

/// 撮影要求または完了の検証で発生したエラー。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    /// 送信前の要求検査に失敗した。
    #[error("撮影要求が不正です: {0}")]
    InvalidRequest(String),
    /// エンジンが撮影要求を拒否した。
    #[error("撮影要求が拒否されました ({code}): {message}")]
    Rejected {
        /// エンジンが返したエラーコード。
        code: String,
        /// エンジンが返した説明。
        message: String,
    },
    /// 受け付け後に撮影が失敗した。
    #[error("撮影に失敗しました ({code}): {message}")]
    Failed {
        /// エンジンが返したエラーコード。
        code: String,
        /// エンジンが返した説明。
        message: String,
    },
    /// 出力ファイルの場所またはサイズが検証に通らなかった。
    #[error("撮影ファイルを検証できません: {0}")]
    OutsideCaptureDir(String),
    /// 完了イベントの撮影 ID が期待値と異なる。
    #[error("撮影 ID が一致しません: 期待値 {expected}、受信値 {actual}")]
    CaptureIdMismatch {
        /// 期待した撮影 ID。
        expected: String,
        /// 受信した撮影 ID。
        actual: String,
    },
    /// 指定された段階で時間切れになった。
    #[error("撮影の{0}が時間切れになりました")]
    Timeout(CaptureTimeoutPhase),
    /// 接続が閉じたか、接続の世代が変わった。
    #[error("撮影中に Bridge 接続が変わりました")]
    ConnectionChanged,
    /// イベントの取りこぼしが発生した。
    #[error("撮影中にイベントを {0} 件取りこぼしました")]
    EventsLagged(u64),
    /// 受信した応答またはイベントの形式が不正だった。
    #[error("撮影の通信形式が不正です: {0}")]
    Protocol(String),
}

impl CaptureRequest {
    /// 要求を検査し、撮影用ディレクトリの正規化済みパスを返す。
    pub fn validate(&self) -> Result<PathBuf, CaptureError> {
        if !self.capture_dir.is_absolute() {
            return Err(CaptureError::InvalidRequest(
                "撮影用ディレクトリには絶対パスが必要です".to_owned(),
            ));
        }
        let dir = self.capture_dir.to_str().ok_or_else(|| {
            CaptureError::InvalidRequest("撮影用ディレクトリを UTF-8 に変換できません".to_owned())
        })?;
        if dir.is_empty() || dir.chars().count() > 1024 {
            return Err(CaptureError::InvalidRequest(
                "撮影用ディレクトリの長さが範囲外です".to_owned(),
            ));
        }
        let mut name = self.output_name.bytes();
        if self.output_name.len() > 64
            || !matches!(name.next(), Some(c) if c.is_ascii_alphanumeric())
            || !name.all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(CaptureError::InvalidRequest(
                "出力名は英数字で始まる英数字・ハイフン・下線の 64 文字以内にしてください"
                    .to_owned(),
            ));
        }
        if self.settle_frames > 600
            || !(1..=1920).contains(&self.max_width)
            || !(1..=1080).contains(&self.max_height)
        {
            return Err(CaptureError::InvalidRequest(
                "固定刻みまたは画像寸法が範囲外です".to_owned(),
            ));
        }
        let metadata = std::fs::metadata(&self.capture_dir).map_err(|_| {
            CaptureError::InvalidRequest("撮影用ディレクトリを確認できません".to_owned())
        })?;
        if !metadata.is_dir() {
            return Err(CaptureError::InvalidRequest(
                "撮影先がディレクトリではありません".to_owned(),
            ));
        }
        std::fs::canonicalize(&self.capture_dir).map_err(|_| {
            CaptureError::InvalidRequest("撮影用ディレクトリを正規化できません".to_owned())
        })
    }

    fn params(&self) -> Result<Map<String, Value>, CaptureError> {
        let capture_dir = self.capture_dir.to_str().ok_or_else(|| {
            CaptureError::InvalidRequest("撮影用ディレクトリを UTF-8 に変換できません".to_owned())
        })?;
        let mut params = Map::new();
        params.insert("captureDir".to_owned(), json!(capture_dir));
        params.insert("outputName".to_owned(), json!(self.output_name));
        params.insert("settleFrames".to_owned(), json!(self.settle_frames));
        params.insert("maxWidth".to_owned(), json!(self.max_width));
        params.insert("maxHeight".to_owned(), json!(self.max_height));
        Ok(params)
    }
}

fn protocol(message: impl Into<String>) -> CaptureError {
    CaptureError::Protocol(message.into())
}

fn exact_keys(object: &Map<String, Value>, keys: &[&str], name: &str) -> Result<(), CaptureError> {
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(protocol(format!("{name} の欄が仕様と一致しません")));
    }
    Ok(())
}

fn required_string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, CaptureError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| protocol(format!("{key} が文字列ではありません")))
}

fn required_u64(object: &Map<String, Value>, key: &str) -> Result<u64, CaptureError> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| protocol(format!("{key} が負でない整数ではありません")))
}

fn valid_capture_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'))
}

/// 撮影の受け付け応答から撮影 ID を取り出す。
pub fn parse_capture_accepted(result: &Value) -> Result<String, CaptureError> {
    let object = result
        .as_object()
        .ok_or_else(|| protocol("受け付け結果がオブジェクトではありません"))?;
    exact_keys(object, &["captureId"], "受け付け結果")?;
    let capture_id = required_string(object, "captureId")?;
    if !valid_capture_id(capture_id) {
        return Err(protocol("captureId の形式が不正です"));
    }
    Ok(capture_id.to_owned())
}

/// 撮影完了の形式と出力ファイルの場所・サイズを検証する。
pub fn validate_capture_completed(
    expected_capture_id: &str,
    canonical_capture_dir: &Path,
    output_name: &str,
    params: &Value,
) -> Result<CaptureCompleted, CaptureError> {
    let object = params
        .as_object()
        .ok_or_else(|| protocol("完了イベントの params がオブジェクトではありません"))?;
    let capture_id = required_string(object, "captureId")?;
    if capture_id != expected_capture_id {
        return Err(CaptureError::CaptureIdMismatch {
            expected: expected_capture_id.to_owned(),
            actual: capture_id.to_owned(),
        });
    }
    if !valid_capture_id(capture_id) {
        return Err(protocol("captureId の形式が不正です"));
    }

    match required_string(object, "status")? {
        "failed" => {
            exact_keys(object, &["captureId", "status", "failure"], "失敗イベント")?;
            let failure = object
                .get("failure")
                .and_then(Value::as_object)
                .ok_or_else(|| protocol("failure がオブジェクトではありません"))?;
            exact_keys(failure, &["code", "message"], "failure")?;
            let code = required_string(failure, "code")?;
            let mut code_bytes = code.bytes();
            if !matches!(code_bytes.next(), Some(c) if c.is_ascii_uppercase())
                || !code_bytes.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
            {
                return Err(protocol("failure.code の形式が不正です"));
            }
            let message = required_string(failure, "message")?;
            if message.is_empty() {
                return Err(protocol("failure.message が空です"));
            }
            Err(CaptureError::Failed {
                code: code.to_owned(),
                message: message.to_owned(),
            })
        }
        "succeeded" => {
            exact_keys(
                object,
                &[
                    "captureId",
                    "status",
                    "path",
                    "width",
                    "height",
                    "frame",
                    "pixelDigest",
                    "fileBytes",
                ],
                "成功イベント",
            )?;
            let path_text = required_string(object, "path")?;
            if path_text.is_empty() || path_text.chars().count() > 2048 {
                return Err(protocol("path の長さが範囲外です"));
            }
            let width = required_u64(object, "width")?;
            let height = required_u64(object, "height")?;
            if !(1..=1920).contains(&width) || !(1..=1080).contains(&height) {
                return Err(protocol("画像寸法が範囲外です"));
            }
            let frame = required_u64(object, "frame")?;
            let digest = object
                .get("pixelDigest")
                .and_then(Value::as_object)
                .ok_or_else(|| protocol("pixelDigest がオブジェクトではありません"))?;
            exact_keys(digest, &["algorithm", "value"], "pixelDigest")?;
            if required_string(digest, "algorithm")? != "fnv1a64-rgba8" {
                return Err(protocol("pixelDigest.algorithm が不正です"));
            }
            let pixel_digest = required_string(digest, "value")?;
            if pixel_digest.len() != 16
                || !pixel_digest
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(protocol("pixelDigest.value の形式が不正です"));
            }
            let file_bytes = required_u64(object, "fileBytes")?;
            if file_bytes == 0 {
                return Err(protocol("fileBytes が 0 です"));
            }
            if file_bytes > MAX_FILE_BYTES {
                return Err(CaptureError::OutsideCaptureDir(
                    "撮影ファイルが 32 MiB を超えています".to_owned(),
                ));
            }
            let path = Path::new(path_text);
            // components は途中の `.` を省くため、元の文字列も検査する。
            if !path.is_absolute()
                || path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
                || path_text
                    .split(std::path::is_separator)
                    .any(|part| part == "." || part == "..")
            {
                return Err(CaptureError::OutsideCaptureDir(
                    "撮影ファイルのパスが不正です".to_owned(),
                ));
            }
            if path.file_name() != Some(std::ffi::OsStr::new(&format!("{output_name}.png"))) {
                return Err(CaptureError::OutsideCaptureDir(
                    "撮影ファイル名が要求と一致しません".to_owned(),
                ));
            }
            let canonical_path = std::fs::canonicalize(path).map_err(|_| {
                CaptureError::OutsideCaptureDir("撮影ファイルを正規化できません".to_owned())
            })?;
            if canonical_path.parent() != Some(canonical_capture_dir) {
                return Err(CaptureError::OutsideCaptureDir(
                    "撮影ファイルが撮影用ディレクトリの直下にありません".to_owned(),
                ));
            }
            let metadata = std::fs::metadata(&canonical_path).map_err(|_| {
                CaptureError::OutsideCaptureDir("撮影ファイルを確認できません".to_owned())
            })?;
            if !metadata.is_file()
                || metadata.len() != file_bytes
                || metadata.len() > MAX_FILE_BYTES
            {
                return Err(CaptureError::OutsideCaptureDir(
                    "撮影ファイルの種類またはサイズが不正です".to_owned(),
                ));
            }
            Ok(CaptureCompleted {
                capture_id: capture_id.to_owned(),
                path: path.to_owned(),
                width: width as u32,
                height: height as u32,
                frame,
                pixel_digest: pixel_digest.to_owned(),
                file_bytes,
            })
        }
        _ => Err(protocol("status が不正です")),
    }
}

/// 撮影を要求し、同じ接続世代の完了イベントを期限まで待つ。
///
/// 呼び出し側は同じ接続で撮影要求が同時に複数動かないようにする。
pub async fn capture_deterministic(
    handle: &DispatchHandle,
    request_id: &str,
    request: &CaptureRequest,
    generation: u64,
    current_generation: &AtomicU64,
    timeout: Duration,
) -> Result<CaptureCompleted, CaptureError> {
    let request_for_validation = request.clone();
    let canonical_capture_dir =
        tokio::task::spawn_blocking(move || request_for_validation.validate())
            .await
            .map_err(|_| protocol("撮影要求の検査を完了できません"))??;
    if current_generation.load(Ordering::SeqCst) != generation {
        return Err(CaptureError::ConnectionChanged);
    }
    let envelope = ValidatedEnvelope::Request {
        version: VersionString::try_from(PROTOCOL_VERSION.to_owned())
            .map_err(|_| protocol("Bridge の版が不正です"))?,
        id: CorrelationId::try_from(request_id.to_owned())
            .map_err(|_| CaptureError::InvalidRequest("要求 ID が不正です".to_owned()))?,
        method: MethodName::try_from(CAPTURE_METHOD.to_owned())
            .map_err(|_| protocol("撮影メソッド名が不正です"))?,
        params: Some(request.params()?),
        session_id: None,
        seq: None,
    };
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| CaptureError::InvalidRequest("制限時間が大きすぎます".to_owned()))?;
    let mut events = handle.subscribe_events();
    if current_generation.load(Ordering::SeqCst) != generation {
        return Err(CaptureError::ConnectionChanged);
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(CaptureError::Timeout(CaptureTimeoutPhase::Accept));
    }
    let response = handle.request(envelope, remaining).await;
    if current_generation.load(Ordering::SeqCst) != generation {
        return Err(CaptureError::ConnectionChanged);
    }
    let response = response.map_err(|error| match error {
        RequestError::Timeout(_) => CaptureError::Timeout(CaptureTimeoutPhase::Accept),
        RequestError::ConnectionClosed => CaptureError::ConnectionChanged,
        RequestError::Encode(_) => protocol("撮影要求を符号化できません"),
    })?;
    if Instant::now() >= deadline {
        return Err(CaptureError::Timeout(CaptureTimeoutPhase::Accept));
    }
    let capture_id = match response {
        ResponsePayload::Result(result) => parse_capture_accepted(&result)?,
        ResponsePayload::Error(error) => {
            return Err(CaptureError::Rejected {
                code: error.code.as_str().to_owned(),
                message: error.message,
            })
        }
    };

    loop {
        if Instant::now() >= deadline {
            return Err(if current_generation.load(Ordering::SeqCst) != generation {
                CaptureError::ConnectionChanged
            } else {
                CaptureError::Timeout(CaptureTimeoutPhase::Completion)
            });
        }
        let event = match timeout_at(deadline, events.recv()).await {
            Ok(Ok(event)) => event,
            Ok(Err(RecvError::Lagged(count))) => return Err(CaptureError::EventsLagged(count)),
            Ok(Err(RecvError::Closed)) => return Err(CaptureError::ConnectionChanged),
            Err(_) => {
                return Err(if current_generation.load(Ordering::SeqCst) != generation {
                    CaptureError::ConnectionChanged
                } else {
                    CaptureError::Timeout(CaptureTimeoutPhase::Completion)
                });
            }
        };
        if Instant::now() >= deadline {
            return Err(if current_generation.load(Ordering::SeqCst) != generation {
                CaptureError::ConnectionChanged
            } else {
                CaptureError::Timeout(CaptureTimeoutPhase::Completion)
            });
        }
        let ValidatedEnvelope::Event { event, params, .. } = event.as_ref() else {
            continue;
        };
        if event.as_str() != COMPLETED_EVENT {
            continue;
        }
        let params = params
            .as_ref()
            .ok_or_else(|| protocol("完了イベントに params がありません"))?;
        let actual_id = required_string(params, "captureId")?;
        if actual_id != capture_id {
            continue;
        }
        if current_generation.load(Ordering::SeqCst) != generation {
            return Err(CaptureError::ConnectionChanged);
        }
        let output_name = request.output_name.clone();
        let params = Value::Object(params.clone());
        let completed = tokio::task::spawn_blocking(move || {
            validate_capture_completed(&capture_id, &canonical_capture_dir, &output_name, &params)
        })
        .await
        .map_err(|_| protocol("撮影完了の検査を完了できません"))?;
        if current_generation.load(Ordering::SeqCst) != generation {
            return Err(CaptureError::ConnectionChanged);
        }
        let completed = completed?;
        if completed.width > request.max_width
            || completed.height > request.max_height
            || completed.frame != u64::from(request.settle_frames)
        {
            return Err(protocol("撮影結果の寸法または固定刻みが要求と一致しません"));
        }
        return Ok(completed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("時刻を取得")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "norves-capture-test-{}-{stamp}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).expect("一時ディレクトリを作成");
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn succeeded(path: &Path) -> Value {
        json!({
            "captureId": "capture-1",
            "status": "succeeded",
            "path": path.to_str().expect("パスを UTF-8 に変換"),
            "width": 1,
            "height": 1,
            "frame": 0,
            "pixelDigest": {"algorithm": "fnv1a64-rgba8", "value": "0123456789abcdef"},
            "fileBytes": 4
        })
    }

    #[test]
    fn capture_rejects_other_id() {
        let dir = TestDir::new();
        let event = succeeded(&dir.0.join("capture.png"));
        assert!(matches!(
            validate_capture_completed("capture-2", &dir.0, "capture", &event),
            Err(CaptureError::CaptureIdMismatch { .. })
        ));
    }

    #[test]
    fn capture_rejects_invalid_file_locations_and_size() {
        let dir = TestDir::new();
        let capture_dir = dir.0.join("capture");
        let outside = dir.0.join("outside");
        std::fs::create_dir(&capture_dir).expect("撮影ディレクトリを作成");
        std::fs::create_dir(&outside).expect("外側ディレクトリを作成");
        std::fs::write(capture_dir.join("capture.png"), b"test").expect("撮影ファイルを作成");
        std::fs::write(capture_dir.join("other.png"), b"test").expect("別名ファイルを作成");
        std::fs::write(outside.join("capture.png"), b"test").expect("外側ファイルを作成");
        let canonical = std::fs::canonicalize(&capture_dir).expect("ディレクトリを正規化");
        let cases = [
            succeeded(&outside.join("capture.png")),
            succeeded(&capture_dir.join("..").join("capture").join("capture.png")),
            succeeded(&capture_dir.join("other.png")),
            {
                let mut event = succeeded(&capture_dir.join("capture.png"));
                event["fileBytes"] = json!(3);
                event
            },
        ];
        for event in cases {
            assert!(matches!(
                validate_capture_completed("capture-1", &canonical, "capture", &event),
                Err(CaptureError::OutsideCaptureDir(_))
            ));
        }
    }

    #[test]
    fn capture_reads_failed_completion() {
        let event = json!({
            "captureId": "capture-1",
            "status": "failed",
            "failure": {"code": "RENDER_FAILED", "message": "描画に失敗しました"}
        });
        assert_eq!(
            validate_capture_completed("capture-1", Path::new(""), "capture", &event),
            Err(CaptureError::Failed {
                code: "RENDER_FAILED".to_owned(),
                message: "描画に失敗しました".to_owned(),
            })
        );
    }

    #[test]
    fn capture_rejects_invalid_requests() {
        let dir = TestDir::new();
        let valid = CaptureRequest {
            capture_dir: dir.0.clone(),
            output_name: "capture".to_owned(),
            settle_frames: 0,
            max_width: 1920,
            max_height: 1080,
        };
        let cases = [
            CaptureRequest {
                capture_dir: PathBuf::from("relative"),
                ..valid.clone()
            },
            CaptureRequest {
                output_name: "a/b".to_owned(),
                ..valid.clone()
            },
            CaptureRequest {
                output_name: "a..b".to_owned(),
                ..valid.clone()
            },
            CaptureRequest {
                settle_frames: 601,
                ..valid.clone()
            },
            CaptureRequest {
                max_width: 1921,
                ..valid.clone()
            },
            CaptureRequest {
                max_height: 1081,
                ..valid
            },
        ];
        for request in cases {
            assert!(matches!(
                request.validate(),
                Err(CaptureError::InvalidRequest(_))
            ));
        }
    }
}
