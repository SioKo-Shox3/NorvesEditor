//! MCP の撮影要求を許可・実行し、検証済み画像と操作結果を返す。

use std::{
    fs::{self, File},
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::SystemTime,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use norves_bridge_editor_client::{CaptureCompleted, CaptureError, CaptureRequest};
use rmcp::model::{CallToolResult, ContentBlock, ResultType};
use serde_json::{json, Value};
use tokio::time::Instant;

use crate::mcp::{
    images::{prepare_capture_image, CaptureImage, MAX_CAPTURE_FILE_BYTES},
    operations::{OperationHandle, OperationUpdate},
    reads::McpReadContext,
    tool_catalog::ValidatedToolInput,
    McpRequestLease,
};

const HISTORY_NOTICE: &str = "撮影は編集履歴に積みません。";

/// 呼び出しが破棄された場合も、送信境界に応じた結果を記録する。
struct CaptureRecord {
    operation: OperationHandle,
    sent: Arc<AtomicBool>,
    finished: bool,
}

impl CaptureRecord {
    fn finish(&mut self, result: &'static str, outcome: &'static str) {
        self.operation.finish(OperationUpdate {
            result,
            outcome,
            completed: usize::from(result == "success"),
            pending: false,
            retry_allowed: false,
            actor_finished: true,
        });
        self.finished = true;
    }

    fn sent_outcome(&self) -> &'static str {
        if self.sent.load(Ordering::SeqCst) {
            "unknown"
        } else {
            "notApplied"
        }
    }

    fn failure(&mut self, failure: CaptureFailure) -> CallToolResult {
        let request_id = self.operation.snapshot().request_id;
        let mut payload = json!({
            "requestId": request_id,
            "outcome": failure.outcome,
            "automaticRetryAllowed": false,
            "undoable": false,
            "historyNotice": HISTORY_NOTICE,
        });
        match failure.detail {
            FailureDetail::Text(detail) => payload["detail"] = json!(detail),
            FailureDetail::Engine { code, message } => {
                payload["engineError"] = json!({
                    "code": bounded(&code, 128),
                    "message": bounded(&message, 4096),
                });
            }
        }
        self.finish(failure.result, failure.outcome);
        let mut response = CallToolResult::structured(payload);
        response.content = vec![ContentBlock::text(failure.message)];
        response.is_error = Some(true);
        response.result_type = Some(ResultType::COMPLETE);
        response
    }
}

impl Drop for CaptureRecord {
    fn drop(&mut self) {
        if !self.finished {
            self.finish("cancelled", self.sent_outcome());
        }
    }
}

enum FailureDetail {
    Text(String),
    Engine { code: String, message: String },
}

struct CaptureFailure {
    outcome: &'static str,
    result: &'static str,
    message: String,
    detail: FailureDetail,
}

impl CaptureFailure {
    fn local(outcome: &'static str, result: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            outcome,
            result,
            detail: FailureDetail::Text(message.clone()),
            message,
        }
    }

    fn interrupted(outcome: &'static str, result: &'static str, detail: &'static str) -> Self {
        let message = if outcome == "unknown" {
            format!("{detail}撮影の結果は不明です。自動再送しません。")
        } else {
            detail.to_owned()
        };
        Self {
            outcome,
            result,
            message,
            detail: FailureDetail::Text(detail.to_owned()),
        }
    }

    fn invalid_image(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            outcome: "applied",
            result: "failed",
            message: format!("撮影は行われましたが、画像を確認できません。{detail}"),
            detail: FailureDetail::Text(detail),
        }
    }
}

impl McpReadContext {
    pub(crate) async fn call_capture_tool(
        &self,
        arguments: Value,
        lease: Option<McpRequestLease>,
    ) -> CallToolResult {
        let sent = Arc::new(AtomicBool::new(false));
        let mut record = CaptureRecord {
            operation: self.operations.begin("viewport_capture", &arguments),
            sent: Arc::clone(&sent),
            finished: false,
        };
        let input = match self.catalog.validate_call("viewport_capture", &arguments) {
            Ok(ValidatedToolInput::Custom(input)) => input,
            _ => {
                return record.failure(CaptureFailure::local(
                    "notApplied",
                    "rejected",
                    self.catalog.unavailability_message(),
                ));
            }
        };
        let settle_frames = input_number(&input, "settleFrames", 0);
        let max_width = input_number(&input, "maxWidth", 1920);
        let max_height = input_number(&input, "maxHeight", 1080);
        let params = json!({
            "settleFrames": settle_frames,
            "maxWidth": max_width,
            "maxHeight": max_height,
        });
        let Some(lease) = lease else {
            return record.failure(CaptureFailure::local(
                "notApplied",
                "rejected",
                "MCP要求の認証リースがありません。",
            ));
        };
        let Some(store) = self.capture_store() else {
            return record.failure(CaptureFailure::local(
                "notApplied",
                "failed",
                "撮影用ディレクトリを利用できません。",
            ));
        };
        let Some(_guard) = store.try_begin() else {
            return record.failure(CaptureFailure::local(
                "notApplied",
                "rejected",
                "別の撮影が進行中です。完了してからやり直してください。",
            ));
        };
        let permit = match self.authorize_capture(&lease, &params).await {
            Ok(permit) => permit,
            Err(message) => {
                let result = interruption(&lease)
                    .await
                    .map_or("rejected", |(_, result)| result);
                return record.failure(CaptureFailure::local("notApplied", result, message));
            }
        };

        let prepare_store = store.clone();
        let directory = match tokio::task::spawn_blocking(move || {
            let directory = prepare_store.prepare()?;
            let _ = prepare_store.cleanup(SystemTime::now());
            Ok::<_, std::io::Error>(directory)
        })
        .await
        {
            Ok(Ok(directory)) => directory,
            Ok(Err(error)) => {
                return record.failure(CaptureFailure::local(
                    "notApplied",
                    "failed",
                    format!("撮影用ディレクトリを準備できません: {error}"),
                ));
            }
            Err(_) => {
                return record.failure(CaptureFailure::local(
                    "notApplied",
                    "failed",
                    "撮影用ディレクトリの準備を完了できません。",
                ));
            }
        };
        if let Some((message, result)) = interruption(&lease).await {
            return record.failure(CaptureFailure::local("notApplied", result, message));
        }
        let bridge_lease = match self.bridge().pin() {
            Ok(bridge_lease) if bridge_lease.generation == permit.generation => bridge_lease,
            Ok(_) => {
                return record.failure(CaptureFailure::local(
                    "notApplied",
                    "rejected",
                    "Bridge接続が切り替わったため、撮影を開始できません。",
                ));
            }
            Err(error) => {
                return record.failure(CaptureFailure::local(
                    "notApplied",
                    "failed",
                    error.to_string(),
                ));
            }
        };
        if let Err(message) = self.validate_capture_permit(&permit, &lease, bridge_lease.generation)
        {
            let result = interruption(&lease)
                .await
                .map_or("rejected", |(_, result)| result);
            return record.failure(CaptureFailure::local("notApplied", result, message));
        }
        let timeout = lease
            .request_deadline()
            .saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return record.failure(CaptureFailure::local(
                "notApplied",
                "timedOut",
                "MCP書き込み要求の全体期限を超えました。",
            ));
        }
        let request = CaptureRequest {
            capture_dir: directory.clone(),
            output_name: store.next_output_name(SystemTime::now()),
            settle_frames,
            max_width,
            max_height,
        };
        let captured = tokio::select! {
            biased;
            _ = tokio::time::sleep_until(lease.request_deadline()) => {
                return record.failure(CaptureFailure::interrupted(
                    record.sent_outcome(), "timedOut", "MCP書き込み要求の全体期限を超えました。"
                ));
            }
            _ = lease.request_cancelled() => {
                return record.failure(CaptureFailure::interrupted(
                    record.sent_outcome(), "cancelled", "MCP要求が取り消されました。"
                ));
            }
            _ = lease.authorization_cancelled() => {
                return record.failure(CaptureFailure::interrupted(
                    record.sent_outcome(), "cancelled", "MCP要求の許可が失効しました。"
                ));
            }
            result = self.bridge().capture_deterministic(&bridge_lease, &request, timeout, &sent) => result,
        };
        let completed = match captured {
            Ok(completed) => completed,
            Err(error) => {
                let failure = capture_error(error, record.sent_outcome());
                return record.failure(failure);
            }
        };

        let completed_for_validation = completed.clone();
        let image = match tokio::task::spawn_blocking(move || {
            read_capture_image(&directory, &completed_for_validation)
        })
        .await
        {
            Ok(Ok(image)) => image,
            Ok(Err(message)) => {
                return record.failure(CaptureFailure::invalid_image(message));
            }
            Err(_) => {
                return record.failure(CaptureFailure::invalid_image(
                    "撮影画像の検証を完了できません。",
                ));
            }
        };
        let Some(file_name) = completed.path.file_name().and_then(|name| name.to_str()) else {
            return record.failure(CaptureFailure::invalid_image(
                "撮影ファイル名を確認できません。",
            ));
        };
        let payload = json!({
            "requestId": record.operation.snapshot().request_id,
            "outcome": "applied",
            "captureId": completed.capture_id,
            "width": completed.width,
            "height": completed.height,
            "frame": completed.frame,
            "pixelDigest": {"algorithm": "fnv1a64-rgba8", "value": completed.pixel_digest},
            "fileBytes": completed.file_bytes,
            "fileName": file_name,
            "imageWidth": image.width,
            "imageHeight": image.height,
            "automaticRetryAllowed": false,
            "undoable": false,
            "historyNotice": HISTORY_NOTICE,
        });
        let message = format!(
            "撮影しました。元画像: {}×{}、固定刻み: {}、画素の要約値: {}、返す画像: {}×{}。",
            completed.width,
            completed.height,
            completed.frame,
            completed.pixel_digest,
            image.width,
            image.height
        );
        let mut response = CallToolResult::structured(payload);
        response.content = vec![
            ContentBlock::text(message),
            ContentBlock::image(STANDARD.encode(image.png), "image/png"),
        ];
        response.is_error = Some(false);
        response.result_type = Some(ResultType::COMPLETE);
        record.finish("success", "applied");
        response
    }
}

fn input_number(input: &Value, key: &str, default: u32) -> u32 {
    input
        .get(key)
        .and_then(Value::as_f64)
        .map_or(default, |value| value as u32)
}

/// 許可待ちや準備中に生じた取消・期限切れを、待機せずに取り出す。
async fn interruption(lease: &McpRequestLease) -> Option<(&'static str, &'static str)> {
    tokio::select! {
        biased;
        _ = tokio::time::sleep_until(lease.request_deadline()) => {
            Some(("MCP書き込み要求の全体期限を超えました。", "timedOut"))
        }
        _ = lease.request_cancelled() => Some(("MCP要求が取り消されました。", "cancelled")),
        _ = lease.authorization_cancelled() => Some(("MCP要求の許可が失効しました。", "cancelled")),
        _ = std::future::ready(()) => None,
    }
}

fn capture_error(error: CaptureError, sent_outcome: &'static str) -> CaptureFailure {
    match error {
        CaptureError::Rejected { code, message } => CaptureFailure {
            outcome: "rejected",
            result: "rejected",
            message: "エンジンが撮影要求を拒否しました。自動再送しません。".to_owned(),
            detail: FailureDetail::Engine { code, message },
        },
        CaptureError::Failed { code, message } => CaptureFailure {
            outcome: "rejected",
            result: "failed",
            message:
                "エンジンで撮影が失敗しました。固定刻みが進んだ可能性があります。自動再送しません。"
                    .to_owned(),
            detail: FailureDetail::Engine { code, message },
        },
        CaptureError::ConnectionChanged => CaptureFailure::local(
            "unknown",
            "unknown",
            "Bridge接続が切り替わったため撮影の結果は不明です。自動再送しません。",
        ),
        error @ CaptureError::InvalidRequest(_) => {
            CaptureFailure::local("notApplied", "rejected", error.to_string())
        }
        error => {
            let result = if matches!(error, CaptureError::Timeout(_)) {
                "timedOut"
            } else if sent_outcome == "unknown" {
                "unknown"
            } else {
                "failed"
            };
            let message = if sent_outcome == "unknown" {
                "撮影の結果は不明です。自動再送しません。"
            } else {
                "撮影は開始されていません。"
            };
            CaptureFailure {
                outcome: sent_outcome,
                result,
                message: message.to_owned(),
                detail: FailureDetail::Text(error.to_string()),
            }
        }
    }
}

fn bounded(value: &str, limit: usize) -> &str {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn read_capture_image(
    directory: &Path,
    completed: &CaptureCompleted,
) -> Result<CaptureImage, String> {
    let path = fs::canonicalize(&completed.path)
        .map_err(|error| format!("撮影ファイルを正規化できません: {error}"))?;
    if path.parent() != Some(directory) {
        return Err("撮影ファイルが撮影用ディレクトリの直下にありません。".to_owned());
    }
    let file = File::open(path).map_err(|error| format!("撮影ファイルを開けません: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("撮影ファイルを確認できません: {error}"))?;
    if !metadata.is_file()
        || metadata.len() != completed.file_bytes
        || metadata.len() > MAX_CAPTURE_FILE_BYTES as u64
    {
        return Err("撮影ファイルの種類またはサイズが一致しません。".to_owned());
    }
    let mut bytes = Vec::new();
    file.take(MAX_CAPTURE_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("撮影ファイルを読み込めません: {error}"))?;
    if bytes.len() as u64 != completed.file_bytes {
        return Err("撮影ファイルの読み込み中にサイズが変わりました。".to_owned());
    }
    let image = prepare_capture_image(&bytes, completed.width, completed.height)
        .map_err(|error| error.to_string())?;
    if image.pixel_digest != completed.pixel_digest {
        return Err("撮影画像の画素の要約値が一致しません。".to_owned());
    }
    Ok(image)
}
