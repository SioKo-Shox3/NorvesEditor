//! 本文を受け取らない操作記録。最新500件を画面へ渡し、JSONLを1MiB×2世代に制限する。

use std::{
    collections::{hash_map::RandomState, VecDeque},
    fs::{self, OpenOptions},
    hash::BuildHasher,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use crate::{dto::McpOperationDto, dto::McpOperationsDto, error::BackendError};

pub(crate) const MAX_RECORDS: usize = 500;
pub(crate) const MAX_BYTES: u64 = 1024 * 1024;
pub(crate) const FILE_NAME: &str = "mcp-operations.jsonl";
const ROTATED_NAME: &str = "mcp-operations.jsonl.1";
const STORAGE_ERROR: &str =
    "操作記録をファイルへ保存できません。画面内の記録だけを保持しています。";

type EventSink = Arc<dyn Fn(McpOperationsDto) + Send + Sync>;

#[derive(Clone)]
pub(crate) struct OperationStore(Arc<Mutex<StoreState>>);

struct StoreState {
    session_id: String,
    next_id: u64,
    revision: u64,
    records: VecDeque<McpOperationDto>,
    directory: Option<PathBuf>,
    storage_error: Option<String>,
    sink: Option<EventSink>,
    target_hasher: RandomState,
}

impl Default for OperationStore {
    fn default() -> Self {
        Self::new(None)
    }
}

impl OperationStore {
    pub(crate) fn new(directory: Option<PathBuf>) -> Self {
        // 起動識別をOS乱数で分離する。乱数源が使えない場合も時刻・PIDを併記する。
        let mut random = [0_u8; 16];
        let _ = getrandom::fill(&mut random);
        let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let session_id = format!("{}-{}-{nonce}", now(), std::process::id());
        let storage_error = directory
            .as_ref()
            .is_none_or(|dir| {
                fs::create_dir_all(dir)
                    .and_then(|()| {
                        OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(dir.join(FILE_NAME))
                    })
                    .is_err()
            })
            .then(|| STORAGE_ERROR.to_owned());
        Self(Arc::new(Mutex::new(StoreState {
            session_id,
            next_id: 1,
            revision: 0,
            records: VecDeque::new(),
            directory,
            storage_error,
            sink: None,
            target_hasher: RandomState::new(),
        })))
    }

    pub(crate) fn set_sink(&self, sink: EventSink) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sink = Some(sink);
    }

    pub(crate) fn snapshot(&self) -> McpOperationsDto {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    pub(crate) fn begin(&self, tool: &str, arguments: &Value) -> OperationHandle {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tool = known_tool(tool);
        // 対象IDも任意の秘密を含み得る。生文字列を保存せず、起動内で比較できる指紋にする。
        let params = arguments.get("params").unwrap_or(arguments);
        let target = [
            "objectId",
            "parentId",
            "newParentId",
            "rootId",
            "logicalPath",
        ]
        .iter()
        .find_map(|key| {
            params.get(*key).and_then(Value::as_str).map(|value| {
                if value.len() > 512 {
                    format!("{key}: 対象ID省略")
                } else {
                    format!("{key}: #{:016x}", state.target_hasher.hash_one(value))
                }
            })
        })
        .unwrap_or_else(|| match tool {
            "edit_undo" | "edit_redo" => "履歴の先頭まとまり".to_owned(),
            "edit_begin_group" | "edit_end_group" => "編集まとまり".to_owned(),
            _ => "エンジン".to_owned(),
        });
        let request_id = format!("mcp-write-{}-{}", state.session_id, state.next_id);
        state.next_id += 1;
        let record = McpOperationDto {
            request_id,
            session_id: state.session_id.clone(),
            timestamp: now(),
            tool: tool.to_owned(),
            target,
            summary: String::new(),
            result: "cancelled".to_owned(),
            outcome: "notApplied".to_owned(),
            display_group_id: None,
            completed_count: 0,
            pending: false,
            retry_allowed: false,
            automatic_retry_allowed: false,
            actor_finished: false,
        };
        OperationHandle {
            store: self.clone(),
            state: Arc::new(Mutex::new(record)),
        }
    }

    fn publish(&self, record: McpOperationDto) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let saved = state
            .directory
            .as_ref()
            .ok_or_else(|| io::Error::other("保存ディレクトリがありません"))
            .and_then(|dir| append_record(dir, &record));
        state.storage_error = saved.err().map(|_| STORAGE_ERROR.to_owned());
        state
            .records
            .retain(|old| old.request_id != record.request_id);
        state.records.push_back(record);
        while state.records.len() > MAX_RECORDS {
            state.records.pop_front();
        }
        state.revision += 1;
        let payload = state.snapshot();
        let sink = state.sink.clone();
        drop(state);
        if let Some(sink) = sink {
            sink(payload);
        }
    }
}

impl StoreState {
    fn snapshot(&self) -> McpOperationsDto {
        McpOperationsDto {
            session_id: self.session_id.clone(),
            revision: self.revision,
            records: self.records.iter().cloned().collect(),
            storage_error: self.storage_error.clone(),
        }
    }
}

fn append_record(directory: &Path, record: &McpOperationDto) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(record)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::other("記録が保存上限を超えています"));
    }
    let path = directory.join(FILE_NAME);
    let size = match fs::metadata(&path) {
        Ok(meta) => meta.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    if size + bytes.len() as u64 > MAX_BYTES {
        let previous = directory.join(ROTATED_NAME);
        match fs::remove_file(&previous) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        // 外部で上限を超えたファイルも、保存世代の上限を守る。
        if size > MAX_BYTES {
            fs::remove_file(&path)?;
        } else {
            fs::rename(&path, previous)?;
        }
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(&bytes)
}

#[derive(Clone)]
pub(crate) struct OperationHandle {
    store: OperationStore,
    state: Arc<Mutex<McpOperationDto>>,
}

impl OperationHandle {
    pub(crate) fn snapshot(&self) -> McpOperationDto {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn group(&self, display_group_id: String) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .display_group_id = Some(display_group_id);
    }

    pub(crate) fn finish(&self, update: OperationUpdate) {
        let mut record = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // HTTPの取消応答が後着しても、actorが確定した結果を上書きしない。
        if record.actor_finished {
            return;
        }
        record.timestamp = now();
        record.result = update.result.to_owned();
        record.outcome = update.outcome.to_owned();
        record.completed_count = update.completed;
        record.pending = update.pending;
        record.retry_allowed = update.retry_allowed;
        record.actor_finished = update.actor_finished;
        record.summary = summary(update.outcome, update.pending).to_owned();
        // 同一要求の更新順を保存・通知まで保つ。このロックをBridge I/Oへ持ち出さない。
        self.store.publish(record.clone());
    }
}

pub(crate) struct OperationUpdate {
    pub(crate) result: &'static str,
    pub(crate) outcome: &'static str,
    pub(crate) completed: usize,
    pub(crate) pending: bool,
    pub(crate) retry_allowed: bool,
    pub(crate) actor_finished: bool,
}

pub(crate) fn result_kind(
    outcome: &str,
    error: Option<&BackendError>,
    timed_out: bool,
) -> &'static str {
    if timed_out
        || matches!(error, Some(BackendError::Request { message }) if message.contains("期限") || message.contains("timed out"))
    {
        "timedOut"
    } else {
        match outcome {
            "applied" | "noChange" => "success",
            "partial" => "partial",
            "rejected" => "rejected",
            "unknown" => "unknown",
            _ if matches!(error, Some(BackendError::EditCancelled)) => "cancelled",
            _ if matches!(
                error,
                Some(BackendError::Request { .. } | BackendError::McpAuthorizationRevoked)
            ) =>
            {
                "rejected"
            }
            _ => "failed",
        }
    }
}

pub(crate) fn summary(outcome: &str, pending: bool) -> &'static str {
    match (outcome, pending) {
        ("unknown", _) => "操作結果は不明です。自動再送せず、要求IDとエディタの状態を確認してください。",
        ("rejected", true) => "操作は拒否され、編集履歴を保留しています。自動再送せず、画面で再試行または破棄を選んでください。",
        (_, true) => "編集履歴を保留しています。自動再送せず、画面で適用済みの件数と保留状態を確認してください。",
        ("partial", _) => "一部の適用を確認済みです。自動再送せず、エディタの状態を確認してください。",
        ("rejected", _) => "エンジンが操作を拒否しました。変更は適用されていません。",
        ("notApplied", _) => "操作は未送信です。理由を確認してください。",
        ("readFailed", _) => "読み取りが完了しませんでした。",
        ("noChange", _) => "操作対象の履歴がないため、変更はありません。",
        _ => "操作が完了しました。",
    }
}

fn known_tool(name: &str) -> &'static str {
    match name {
        "object_set_property" => "object_set_property",
        "scene_create_object" => "scene_create_object",
        "scene_duplicate_object" => "scene_duplicate_object",
        "scene_reparent_object" => "scene_reparent_object",
        "scene_delete_object" => "scene_delete_object",
        "component_add" => "component_add",
        "component_remove" => "component_remove",
        "edit_undo" => "edit_undo",
        "edit_redo" => "edit_redo",
        "edit_begin_group" => "edit_begin_group",
        "edit_end_group" => "edit_end_group",
        "runtime_play" => "runtime_play",
        "runtime_pause" => "runtime_pause",
        "runtime_stop" => "runtime_stop",
        "engine_get_status" => "engine_get_status",
        "bridge_get_capabilities" => "bridge_get_capabilities",
        "scene_get_tree_page" => "scene_get_tree_page",
        "object_get_snapshot" => "object_get_snapshot",
        "schema_get_snapshot" => "schema_get_snapshot",
        "asset_get_manifest" => "asset_get_manifest",
        "asset_resolve" => "asset_resolve",
        "logs_get_recent" => "logs_get_recent",
        "viewport_get_thumbnail" => "viewport_get_thumbnail",
        _ => "unknown_tool",
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests;

/// 読み取りもSDKのfuture破棄・期限を含めて記録する。応答本文には触れない。
pub(super) struct ReadCapture {
    pub(super) operation: OperationHandle,
    pub(super) lease: Option<super::McpRequestLease>,
    pub(super) finished: bool,
}

impl ReadCapture {
    pub(super) fn finish(&mut self, result: &'static str) {
        let timed_out = self
            .lease
            .as_ref()
            .is_some_and(|lease| tokio::time::Instant::now() >= lease.request_deadline());
        self.operation.finish(OperationUpdate {
            result: if timed_out { "timedOut" } else { result },
            outcome: if result == "success" {
                "readCompleted"
            } else {
                "readFailed"
            },
            completed: 0,
            pending: false,
            retry_allowed: false,
            actor_finished: true,
        });
        self.finished = true;
    }
}

impl Drop for ReadCapture {
    fn drop(&mut self) {
        if !self.finished {
            self.finish("cancelled");
        }
    }
}
