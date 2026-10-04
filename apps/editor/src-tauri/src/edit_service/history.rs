//! 編集要求の結果から取り消し記録を作り、古い画面捕捉を安全に補正する。

use std::collections::HashMap;
use std::future::Future;

use serde_json::Value;
use tokio::time::Instant;

use super::groups::{GroupSecret, IDLE_TIMEOUT, MAX_EDITS, TOTAL_TIMEOUT};
use super::{EditKind, EditSource};
use crate::bridge_state::BridgeLease;
use crate::dto::{
    EditDiscardResultDto, EditGroupSummaryDto, EditHistorySummaryDto, EditPendingGroupDto,
    EditSourceDto,
};
use crate::error::BackendError;

const MAX_CORRECTION_ENTRIES: usize = 512;
const MAX_CORRECTION_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HistoryMarker {
    pub(crate) sequence: u64,
    pub(crate) source: EditSource,
    pub(crate) kind: EditKind,
    pub(crate) generation: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HistoryRecord {
    Create {
        created_id: String,
        parent_id: Option<String>,
        kind: Option<String>,
    },
    Duplicate {
        source_object_id: String,
        created_id: String,
        parent_id: Option<String>,
    },
    Reparent {
        object_id: String,
        old_parent_id: Option<String>,
        new_parent_id: Option<String>,
    },
    SetProperty {
        object_id: String,
        property: String,
        old_value: Value,
        new_value: Value,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GroupKey {
    pub(super) generation: u64,
    pub(super) sequence: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct GroupMetadata {
    pub(super) key: GroupKey,
    pub(super) name: String,
    pub(super) source: EditSource,
    pub(super) created_at: u64,
    pub(super) count: usize,
    pub(super) named: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct HistoryEntry {
    marker: HistoryMarker,
    record: HistoryRecord,
    group: GroupMetadata,
}

#[derive(Clone, Debug, PartialEq)]
struct ActiveGroup {
    token: u64,
    secret: Option<GroupSecret>,
    started_at: Instant,
    last_edit_at: Instant,
    metadata: GroupMetadata,
    entries: Vec<HistoryEntry>,
}

#[derive(Clone, Debug, PartialEq)]
struct PendingGroupAction {
    direction: HistoryDirection,
    metadata: GroupMetadata,
    completed: usize,
    outcome_unknown: bool,
    retry_allowed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HistoryCapture<T> {
    pub(crate) generation: u64,
    pub(crate) revision: u64,
    /// `None` は未取得、`Some(Value::Null)` は取得済みのJSON null。
    pub(crate) value: Option<T>,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) enum PriorCapture<T> {
    Ui(HistoryCapture<T>),
    InAction,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) enum HistoryRequest {
    CreateObject {
        parent_id: Option<String>,
        kind: Option<String>,
    },
    DuplicateObject {
        source_object_id: String,
        parent_id: Option<String>,
    },
    ReparentObject {
        object_id: String,
        new_parent_id: Option<String>,
        old_parent: PriorCapture<Option<String>>,
    },
    SetProperty {
        object_id: String,
        property: String,
        requested_value: Value,
        old_value: PriorCapture<Value>,
    },
    /// 成功した削除は履歴と補正情報を空にする。削除自体は記録しない。
    DeleteObject { object_id: String },
}

impl HistoryRequest {
    pub(super) fn is_scene_structure_edit(&self) -> bool {
        matches!(
            self,
            Self::CreateObject { .. }
                | Self::DuplicateObject { .. }
                | Self::ReparentObject { .. }
                | Self::DeleteObject { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryDirection {
    Undo,
    Redo,
}

#[derive(Clone, Copy)]
pub(super) struct HistoryOperationContext {
    pub(super) source: EditSource,
    pub(super) kind: EditKind,
    pub(super) sequence: u64,
    pub(super) generation: u64,
    pub(super) group_token: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HistoryActionRequest {
    pub(crate) direction: HistoryDirection,
    pub(crate) expected_head_id: Option<u64>,
    pub(crate) expected_revision: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HistoryAction {
    pub(crate) direction: HistoryDirection,
    pub(super) entry_id: u64,
    pub(crate) record: HistoryRecord,
    pub(super) group: GroupMetadata,
    pub(super) steps: Vec<HistoryActionStep>,
    pub(super) grouped: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct HistoryActionStep {
    pub(super) entry_id: u64,
    pub(super) record: HistoryRecord,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) enum HistoryPrior {
    Property(Value),
    /// `Some(None)` を親なし(シーン直下)として扱う。
    Parent(Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QueuedEditResult {
    pub(crate) value: Value,
    pub(crate) prior: Option<HistoryPrior>,
}

impl QueuedEditResult {
    pub(crate) fn plain(value: Value) -> Self {
        Self { value, prior: None }
    }

    #[cfg(test)]
    pub(crate) fn with_prior(value: Value, prior: HistoryPrior) -> Self {
        Self {
            value,
            prior: Some(prior),
        }
    }
}

/// MCPの旧値照会が終わるまで書き込みを呼べない形で、1件の列内操作を組み立てる。
#[allow(dead_code)]
pub(crate) async fn apply_mcp_edit_with_prior<R, RFut, W, WFut>(
    lease: BridgeLease,
    read_prior: R,
    write: W,
) -> Result<QueuedEditResult, BackendError>
where
    R: FnOnce(BridgeLease) -> RFut,
    RFut: Future<Output = Result<Option<HistoryPrior>, BackendError>>,
    W: FnOnce(BridgeLease, HistoryPrior) -> WFut,
    WFut: Future<Output = Result<Value, BackendError>>,
{
    let prior = read_prior(lease.clone())
        .await?
        .ok_or_else(|| BackendError::Request {
            message: "編集前の値を取得できません。対象を再取得してから操作してください。"
                .to_owned(),
        })?;
    let value = write(lease, prior.clone()).await?;
    Ok(QueuedEditResult {
        value,
        prior: Some(prior),
    })
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum CorrectionKey {
    Property { object_id: String, property: String },
    Parent { object_id: String },
}

impl CorrectionKey {
    fn byte_len(&self) -> usize {
        match self {
            CorrectionKey::Property {
                object_id,
                property,
            } => object_id.len().saturating_add(property.len()),
            CorrectionKey::Parent { object_id } => object_id.len(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum CorrectionValue {
    Property(Value),
    Parent(Option<String>),
}

impl CorrectionValue {
    fn byte_len(&self) -> Option<usize> {
        match self {
            CorrectionValue::Property(value) => serde_json::to_vec(value).ok().map(|v| v.len()),
            CorrectionValue::Parent(value) => serde_json::to_vec(value).ok().map(|v| v.len()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct CorrectionEntry {
    revision: u64,
    value: CorrectionValue,
    byte_len: usize,
}

#[derive(Default)]
struct CorrectionCache {
    entries: HashMap<CorrectionKey, CorrectionEntry>,
    bytes: usize,
    lost_through: u64,
}

impl CorrectionCache {
    fn get(&self, key: &CorrectionKey) -> Option<&CorrectionEntry> {
        self.entries.get(key)
    }

    fn insert(&mut self, key: CorrectionKey, revision: u64, value: CorrectionValue) {
        if let Some(previous) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(previous.byte_len);
        }

        let Some(byte_len) = key
            .byte_len()
            .checked_add(value.byte_len().unwrap_or(usize::MAX))
        else {
            self.mark_lost_through(revision);
            return;
        };
        if byte_len > MAX_CORRECTION_BYTES {
            self.mark_lost_through(revision);
            return;
        }

        while self.entries.len() >= MAX_CORRECTION_ENTRIES
            || self.bytes.saturating_add(byte_len) > MAX_CORRECTION_BYTES
        {
            let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.revision)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(oldest) = self.entries.remove(&oldest_key) {
                self.bytes = self.bytes.saturating_sub(oldest.byte_len);
                self.mark_lost_through(oldest.revision);
            }
        }

        self.bytes = self.bytes.saturating_add(byte_len);
        self.entries.insert(
            key,
            CorrectionEntry {
                revision,
                value,
                byte_len,
            },
        );
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.lost_through = 0;
    }

    fn clear_and_mark_lost_through(&mut self, revision: u64) {
        self.entries.clear();
        self.bytes = 0;
        self.lost_through = revision;
    }

    fn remove_object(&mut self, object_id: &str, revision: u64) {
        let keys = self
            .entries
            .keys()
            .filter(|key| match key {
                CorrectionKey::Property {
                    object_id: key_object_id,
                    ..
                }
                | CorrectionKey::Parent {
                    object_id: key_object_id,
                } => key_object_id == object_id,
            })
            .cloned()
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return;
        }
        for key in keys {
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.byte_len);
            }
        }
        self.mark_lost_through(revision);
    }

    fn mark_lost_through(&mut self, revision: u64) {
        self.lost_through = self.lost_through.max(revision);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum PreparedHistoryRequest {
    CreateObject {
        parent_id: Option<String>,
        kind: Option<String>,
    },
    DuplicateObject {
        source_object_id: String,
        parent_id: Option<String>,
    },
    ReparentObject {
        object_id: String,
        new_parent_id: Option<String>,
        old_parent_id: Option<Option<String>>,
    },
    SetProperty {
        object_id: String,
        property: String,
        requested_value: Value,
        old_value: Option<Value>,
    },
    DeleteObject {
        object_id: String,
    },
}

#[derive(Default)]
pub(super) struct HistoryState {
    generation: Option<u64>,
    revision: u64,
    applied_revision: u64,
    markers: Vec<HistoryMarker>,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    corrections: CorrectionCache,
    edit_unsupported: bool,
    pending: bool,
    pending_group: Option<PendingGroupAction>,
    active_group: Option<ActiveGroup>,
}

impl HistoryState {
    pub(super) fn synchronize(&mut self, generation: Option<u64>) {
        if self.generation != generation {
            self.generation = generation;
            self.undo.clear();
            self.redo.clear();
            self.corrections.clear();
            self.edit_unsupported = false;
            self.applied_revision = 0;
            self.markers.clear();
            self.pending = false;
            self.pending_group = None;
            self.active_group = None;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn summary(&self) -> EditHistorySummaryDto {
        let undo_group = self.group_summary(HistoryDirection::Undo);
        let redo_group = self.group_summary(HistoryDirection::Redo);
        let (undo_head_id, undo_revision) = self.history_cursor(HistoryDirection::Undo);
        let (redo_head_id, redo_revision) = self.history_cursor(HistoryDirection::Redo);
        EditHistorySummaryDto {
            generation: self.generation,
            history_revision: self.revision,
            applied_revision: self.applied_revision,
            can_undo: undo_head_id.is_some() && !self.edit_unsupported && !self.pending,
            can_redo: redo_head_id.is_some() && !self.edit_unsupported && !self.pending,
            undo_head_id,
            undo_revision,
            undo_group,
            redo_head_id,
            redo_revision,
            redo_group,
            pending: self.pending,
            pending_group: self.pending_summary(),
        }
    }

    fn group_summary(&self, direction: HistoryDirection) -> Option<EditGroupSummaryDto> {
        let stack = self.visible_stack(direction);
        let entry = stack.last()?;
        Some(EditGroupSummaryDto {
            id: group_id(entry.group.key.generation, entry.group.key.sequence),
            name: entry.group.name.clone(),
            source: match entry.group.source {
                EditSource::Ui => EditSourceDto::Ui,
                EditSource::Mcp => EditSourceDto::Mcp,
            },
            count: entry.group.count,
            created_at: entry.group.created_at,
        })
    }

    fn pending_summary(&self) -> Option<EditPendingGroupDto> {
        let pending = self.pending_group.as_ref()?;
        Some(EditPendingGroupDto {
            id: group_id(
                pending.metadata.key.generation,
                pending.metadata.key.sequence,
            ),
            name: pending.metadata.name.clone(),
            direction: match pending.direction {
                HistoryDirection::Undo => "undo".to_owned(),
                HistoryDirection::Redo => "redo".to_owned(),
            },
            source: match pending.metadata.source {
                EditSource::Ui => EditSourceDto::Ui,
                EditSource::Mcp => EditSourceDto::Mcp,
            },
            created_at: pending.metadata.created_at,
            total_count: pending.metadata.count,
            completed_count: pending.completed,
            outcome_unknown: pending.outcome_unknown,
            retry_allowed: pending.retry_allowed,
        })
    }

    pub(super) fn snapshot(&self) -> (Option<u64>, u64, Vec<HistoryMarker>) {
        (self.generation, self.revision, self.markers.clone())
    }

    pub(super) fn applied_revision(&self) -> u64 {
        self.applied_revision
    }

    pub(super) fn history_cursor(&self, direction: HistoryDirection) -> (Option<u64>, u64) {
        let stack = self.visible_stack(direction);
        (
            stack.last().map(|entry| entry.marker.sequence),
            self.revision,
        )
    }

    fn visible_stack(&self, direction: HistoryDirection) -> &[HistoryEntry] {
        match direction {
            HistoryDirection::Undo => self
                .active_group
                .as_ref()
                .filter(|group| !group.entries.is_empty())
                .map_or(&self.undo, |group| &group.entries),
            HistoryDirection::Redo => &self.redo,
        }
    }

    pub(super) fn prepare_action(&self, request: HistoryActionRequest) -> Option<HistoryAction> {
        let expected_head_id = request.expected_head_id?;
        if self.edit_unsupported || self.pending || request.expected_revision != self.revision {
            return None;
        }
        let stack = self.visible_stack(request.direction);
        let entry = stack.last()?;
        if entry.marker.sequence != expected_head_id {
            return None;
        }
        let steps = stack
            .iter()
            .rev()
            .take_while(|candidate| candidate.group.key == entry.group.key)
            .map(|candidate| HistoryActionStep {
                entry_id: candidate.marker.sequence,
                record: candidate.record.clone(),
            })
            .collect::<Vec<_>>();
        let first = steps.first()?;
        Some(HistoryAction {
            direction: request.direction,
            entry_id: first.entry_id,
            record: first.record.clone(),
            grouped: entry.group.named,
            group: entry.group.clone(),
            steps,
        })
    }

    pub(super) fn finish_action_success(
        &mut self,
        action: HistoryAction,
        source: EditSource,
        kind: EditKind,
        sequence: u64,
        generation: u64,
        result: Value,
    ) -> Result<bool, BackendError> {
        if result.get("accepted").and_then(Value::as_bool) != Some(true) {
            self.discard_action(&action);
            return Err(BackendError::Request {
                message:
                    "取り消し・やり直しをエンジンが受け付けませんでした。対象の履歴を破棄しました。"
                        .to_owned(),
            });
        }

        let updated_record = match updated_history_record(&action.record, action.direction, &result)
        {
            Ok(record) => record,
            Err(error) => {
                self.discard_action(&action);
                return Err(error);
            }
        };

        let Some(mut entry) = self.pop_action_entry(&action) else {
            return Ok(false);
        };
        self.applied_revision = self.applied_revision.wrapping_add(1);
        self.update_action_corrections(&action.record, &updated_record, action.direction, &result);
        entry.record = updated_record;
        self.push_marker(source, kind, sequence, generation);
        match action.direction {
            HistoryDirection::Undo => self.redo.push(entry),
            HistoryDirection::Redo => self.undo.push(entry),
        }
        Ok(true)
    }

    pub(super) fn begin_group(
        &mut self,
        token: u64,
        name: String,
        source: EditSource,
        generation: u64,
    ) -> Result<(), BackendError> {
        let name = name.trim().to_owned();
        if name.is_empty() || name.len() > 256 {
            return Err(BackendError::Request {
                message: "まとまり名は1〜256バイトで指定してください。".to_owned(),
            });
        }
        self.close_active_group();
        self.active_group = Some(ActiveGroup {
            token,
            secret: None,
            started_at: Instant::now(),
            last_edit_at: Instant::now(),
            metadata: GroupMetadata {
                key: GroupKey {
                    generation,
                    sequence: token,
                },
                name,
                source,
                created_at: unix_millis(),
                count: 0,
                named: true,
            },
            entries: Vec::new(),
        });
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    pub(super) fn end_group(&mut self, token: u64) -> Result<bool, BackendError> {
        self.expire_group();
        if self.active_group.as_ref().map(|group| group.token) != Some(token) {
            return Err(BackendError::Request {
                message: "まとまりIDが現在のまとまりと一致しません。".to_owned(),
            });
        }
        Ok(self.close_active_group())
    }

    pub(super) fn close_group_unless(&mut self, token: Option<u64>) -> bool {
        if self.active_group.as_ref().map(|group| group.token) != token {
            self.close_active_group()
        } else {
            false
        }
    }

    pub(super) fn begin_named_group(
        &mut self,
        token: u64,
        name: String,
        generation: u64,
    ) -> Result<Value, BackendError> {
        let secret = GroupSecret::generate()?;
        self.begin_group(token, name, EditSource::Mcp, generation)?;
        let value = serde_json::json!({
            "groupId": secret.expose(),
            "displayGroupId": group_id(generation, token),
            "maxEdits": MAX_EDITS,
            "idleTimeoutSeconds": IDLE_TIMEOUT.as_secs(),
            "totalTimeoutSeconds": TOTAL_TIMEOUT.as_secs(),
        });
        if let Some(group) = self.active_group.as_mut() {
            group.secret = Some(secret);
        }
        Ok(value)
    }

    /// 列上でだけ秘密を照合する。未知IDも先行まとまりを閉じ、失効済みIDは復活させない。
    pub(super) fn group_boundary(
        &mut self,
        secret: Option<&str>,
        keep: bool,
    ) -> Result<Option<u64>, BackendError> {
        self.expire_group();
        let token = secret.and_then(|candidate| {
            self.active_group
                .as_ref()
                .filter(|group| {
                    group
                        .secret
                        .as_ref()
                        .is_some_and(|secret| secret.matches(candidate))
                })
                .map(|group| group.token)
        });
        self.close_group_unless(token.filter(|_| keep));
        if secret.is_some() && token.is_none() {
            return Err(BackendError::Request {
                message: "groupIdが無効または失効しています。新しいまとまりを開始してください。"
                    .to_owned(),
            });
        }
        Ok(token.filter(|_| keep))
    }

    pub(super) fn end_named_group(&mut self, secret: &str) -> Result<(), BackendError> {
        self.expire_group();
        // endは編集の割込みではないので、誤ったIDで他のまとまりを閉じない。
        if !self
            .active_group
            .as_ref()
            .and_then(|group| group.secret.as_ref())
            .is_some_and(|current| current.matches(secret))
        {
            return Err(BackendError::Request {
                message: "groupIdが無効または失効しています。".to_owned(),
            });
        }
        self.close_active_group();
        Ok(())
    }

    pub(super) fn group_deadline(&self) -> Option<Instant> {
        self.active_group
            .as_ref()
            .map(|group| (group.started_at + TOTAL_TIMEOUT).min(group.last_edit_at + IDLE_TIMEOUT))
    }

    pub(super) fn expire_group(&mut self) -> bool {
        if self
            .group_deadline()
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.close_active_group()
        } else {
            false
        }
    }

    pub(super) fn has_group(&self, token: u64, generation: u64) -> bool {
        self.active_group.as_ref().is_some_and(|group| {
            group.token == token && group.metadata.key.generation == generation
        })
    }

    pub(super) fn has_active_group(&self) -> bool {
        self.active_group.is_some()
    }

    /// 認証改訂で MCP 所有の未完了まとまりを閉じ、古いまとまりIDを失効させる。
    pub(super) fn revoke_mcp_group(&mut self) -> bool {
        if self
            .active_group
            .as_ref()
            .is_some_and(|group| group.metadata.source == EditSource::Mcp)
        {
            self.close_active_group()
        } else {
            false
        }
    }

    fn close_active_group(&mut self) -> bool {
        let Some(mut group) = self.active_group.take() else {
            return false;
        };
        if group.entries.is_empty() {
            self.revision = self.revision.wrapping_add(1);
            return true;
        }
        group.metadata.count = group.entries.len();
        for entry in &mut group.entries {
            entry.group = group.metadata.clone();
        }
        self.undo.extend(group.entries);
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub(super) fn prepare_retry(&self) -> Option<HistoryAction> {
        let pending = self.pending_group.as_ref()?;
        if !pending.retry_allowed || pending.outcome_unknown {
            return None;
        }
        let stack = match pending.direction {
            HistoryDirection::Undo => &self.undo,
            HistoryDirection::Redo => &self.redo,
        };
        let steps = stack
            .iter()
            .rev()
            .take_while(|entry| entry.group.key == pending.metadata.key)
            .map(|entry| HistoryActionStep {
                entry_id: entry.marker.sequence,
                record: entry.record.clone(),
            })
            .collect::<Vec<_>>();
        let first = steps.first()?;
        Some(HistoryAction {
            direction: pending.direction,
            entry_id: first.entry_id,
            record: first.record.clone(),
            group: pending.metadata.clone(),
            steps,
            grouped: true,
        })
    }

    pub(super) fn refresh_action_step(
        &self,
        direction: HistoryDirection,
        group_key: &GroupKey,
        entry_id: u64,
    ) -> Option<HistoryActionStep> {
        let stack = match direction {
            HistoryDirection::Undo => &self.undo,
            HistoryDirection::Redo => &self.redo,
        };
        stack
            .iter()
            .find(|entry| entry.marker.sequence == entry_id && &entry.group.key == group_key)
            .map(|entry| HistoryActionStep {
                entry_id,
                record: entry.record.clone(),
            })
    }

    pub(super) fn start_pending_group(
        &mut self,
        action: &HistoryAction,
        completed: usize,
        outcome_unknown: bool,
        retry_allowed: bool,
    ) {
        self.pending = true;
        self.pending_group = Some(PendingGroupAction {
            direction: action.direction,
            metadata: action.group.clone(),
            completed,
            outcome_unknown,
            retry_allowed,
        });
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn finish_group_step(
        &mut self,
        action: &HistoryAction,
        step: &HistoryActionStep,
        context: HistoryOperationContext,
        result: &Value,
    ) -> Result<bool, BackendError> {
        if result.get("accepted").and_then(Value::as_bool) != Some(true) {
            return Ok(false);
        }
        let updated_record = updated_history_record(&step.record, action.direction, result)?;
        let Some(mut entry) =
            self.pop_action_entry_id(action.direction, step.entry_id, &action.group.key)
        else {
            return Ok(false);
        };
        self.applied_revision = self.applied_revision.wrapping_add(1);
        self.update_action_corrections(&step.record, &updated_record, action.direction, result);
        if let Some((old_id, new_id)) = changed_created_id(&step.record, &updated_record) {
            self.remap_group_ids(&action.group.key, &old_id, &new_id);
        }
        entry.record = updated_record;
        self.push_marker(
            context.source,
            context.kind,
            context.sequence,
            context.generation,
        );
        match action.direction {
            HistoryDirection::Undo => self.redo.push(entry),
            HistoryDirection::Redo => self.undo.push(entry),
        }
        let pending_remaining = if let Some(pending) = self.pending_group.as_mut() {
            if pending.metadata.key == action.group.key {
                pending.completed = pending.completed.saturating_add(1);
                Some((pending.direction, pending.metadata.key.clone()))
            } else {
                None
            }
        } else {
            None
        };
        if let Some((direction, key)) = pending_remaining {
            if !self.has_group_on_stack(direction, &key) {
                self.pending = false;
                self.pending_group = None;
            }
        }
        Ok(true)
    }

    pub(super) fn finish_group_failure(
        &mut self,
        action: &HistoryAction,
        completed: usize,
        error: &BackendError,
    ) {
        let retry_allowed = matches!(error, BackendError::Engine { .. });
        self.start_pending_group(action, completed, !retry_allowed, retry_allowed);
    }

    /// Bridgeが結果を確定できないMCP編集を、再送不可の保留として表示する。
    pub(super) fn mark_mcp_outcome_unknown(&mut self, sequence: u64, generation: u64) {
        self.pending = true;
        self.pending_group = Some(PendingGroupAction {
            direction: HistoryDirection::Undo,
            metadata: GroupMetadata {
                key: GroupKey {
                    generation,
                    sequence,
                },
                name: "MCP編集の結果確認".to_owned(),
                source: EditSource::Mcp,
                created_at: unix_millis(),
                count: 1,
                named: false,
            },
            completed: 0,
            outcome_unknown: true,
            retry_allowed: false,
        });
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn clear_completed_pending_group(&mut self, key: &GroupKey) {
        if self
            .pending_group
            .as_ref()
            .is_some_and(|pending| &pending.metadata.key == key)
            && !self
                .pending_group
                .as_ref()
                .is_some_and(|pending| self.has_group_on_stack(pending.direction, key))
        {
            self.pending = false;
            self.pending_group = None;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn discard_pending_group(&mut self) -> Result<EditDiscardResultDto, BackendError> {
        let Some(pending) = self.pending_group.take() else {
            return Err(BackendError::Request {
                message: "破棄できる保留中のまとまりはありません。".to_owned(),
            });
        };
        self.undo
            .retain(|entry| entry.group.key != pending.metadata.key);
        self.redo
            .retain(|entry| entry.group.key != pending.metadata.key);
        self.pending = false;
        self.revision = self.revision.wrapping_add(1);
        Ok(EditDiscardResultDto {
            group_id: group_id(
                pending.metadata.key.generation,
                pending.metadata.key.sequence,
            ),
            completed_count: pending.completed,
            total_count: pending.metadata.count,
            outcome_unknown: pending.outcome_unknown,
            changes_remain: pending.completed > 0 || pending.outcome_unknown,
        })
    }

    pub(super) fn finish_action_failure(&mut self, action: &HistoryAction) {
        if self.pop_action_entry(action).is_some() {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn mark_edit_unsupported(&mut self) {
        if !self.edit_unsupported {
            self.edit_unsupported = true;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn is_pending(&self) -> bool {
        self.pending
    }

    pub(super) fn clear_for_shutdown(&mut self) {
        self.generation = None;
        self.applied_revision = 0;
        self.markers.clear();
        self.undo.clear();
        self.redo.clear();
        self.corrections.clear();
        self.edit_unsupported = false;
        self.pending = false;
        self.pending_group = None;
        self.active_group = None;
        self.revision = self.revision.wrapping_add(1);
    }

    fn pop_action_entry(&mut self, action: &HistoryAction) -> Option<HistoryEntry> {
        self.pop_action_entry_id(action.direction, action.entry_id, &action.group.key)
    }

    fn pop_action_entry_id(
        &mut self,
        direction: HistoryDirection,
        entry_id: u64,
        group_key: &GroupKey,
    ) -> Option<HistoryEntry> {
        let stack = match direction {
            HistoryDirection::Undo => &mut self.undo,
            HistoryDirection::Redo => &mut self.redo,
        };
        let entry = stack.last()?;
        if entry.marker.sequence != entry_id || &entry.group.key != group_key {
            return None;
        }
        stack.pop()
    }

    fn has_group_on_stack(&self, direction: HistoryDirection, key: &GroupKey) -> bool {
        let stack = match direction {
            HistoryDirection::Undo => &self.undo,
            HistoryDirection::Redo => &self.redo,
        };
        stack.iter().any(|entry| &entry.group.key == key)
    }

    fn remap_group_ids(&mut self, key: &GroupKey, old_id: &str, new_id: &str) {
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            if &entry.group.key == key {
                replace_record_ids(&mut entry.record, old_id, new_id);
            }
        }
        if let Some(group) = self.active_group.as_mut() {
            if &group.metadata.key == key {
                for entry in &mut group.entries {
                    replace_record_ids(&mut entry.record, old_id, new_id);
                }
            }
        }
    }

    fn discard_action(&mut self, action: &HistoryAction) {
        if self.pop_action_entry(action).is_some() {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn update_action_corrections(
        &mut self,
        old_record: &HistoryRecord,
        new_record: &HistoryRecord,
        direction: HistoryDirection,
        result: &Value,
    ) {
        let revision = self.applied_revision;
        match (old_record, new_record, direction) {
            (
                HistoryRecord::Create { created_id, .. },
                HistoryRecord::Create { .. },
                HistoryDirection::Undo,
            ) => self.corrections.remove_object(created_id, revision),
            (
                HistoryRecord::Create { created_id, .. },
                HistoryRecord::Create {
                    created_id: new_id,
                    parent_id,
                    ..
                },
                HistoryDirection::Redo,
            ) => {
                self.corrections.remove_object(created_id, revision);
                self.corrections.insert(
                    CorrectionKey::Parent {
                        object_id: new_id.clone(),
                    },
                    revision,
                    CorrectionValue::Parent(parent_id.clone()),
                );
            }
            (
                HistoryRecord::Duplicate { created_id, .. },
                HistoryRecord::Duplicate { .. },
                HistoryDirection::Undo,
            ) => self.corrections.remove_object(created_id, revision),
            (
                HistoryRecord::Duplicate { created_id, .. },
                HistoryRecord::Duplicate {
                    created_id: new_id,
                    parent_id,
                    ..
                },
                HistoryDirection::Redo,
            ) => {
                self.corrections.remove_object(created_id, revision);
                if let Some(parent_id) = parent_id {
                    self.corrections.insert(
                        CorrectionKey::Parent {
                            object_id: new_id.clone(),
                        },
                        revision,
                        CorrectionValue::Parent(Some(parent_id.clone())),
                    );
                }
            }
            (
                HistoryRecord::Reparent {
                    object_id,
                    old_parent_id,
                    new_parent_id,
                },
                HistoryRecord::Reparent { .. },
                direction,
            ) => {
                let parent_id = match direction {
                    HistoryDirection::Undo => old_parent_id,
                    HistoryDirection::Redo => new_parent_id,
                };
                self.corrections.insert(
                    CorrectionKey::Parent {
                        object_id: object_id.clone(),
                    },
                    revision,
                    CorrectionValue::Parent(parent_id.clone()),
                );
            }
            (
                HistoryRecord::SetProperty {
                    object_id,
                    property,
                    old_value,
                    new_value,
                },
                HistoryRecord::SetProperty { .. },
                direction,
            ) => {
                let recorded_value = match direction {
                    HistoryDirection::Undo => old_value,
                    HistoryDirection::Redo => new_value,
                };
                let value = result.get("appliedValue").unwrap_or(recorded_value);
                self.corrections.insert(
                    CorrectionKey::Property {
                        object_id: object_id.clone(),
                        property: property.clone(),
                    },
                    revision,
                    CorrectionValue::Property(value.clone()),
                );
            }
            _ => {}
        }
    }

    pub(super) fn prepare(
        &self,
        request: HistoryRequest,
        source: EditSource,
        generation: u64,
    ) -> Result<PreparedHistoryRequest, BackendError> {
        match request {
            HistoryRequest::CreateObject { parent_id, kind } => {
                Ok(PreparedHistoryRequest::CreateObject { parent_id, kind })
            }
            HistoryRequest::DuplicateObject {
                source_object_id,
                parent_id,
            } => Ok(PreparedHistoryRequest::DuplicateObject {
                source_object_id,
                parent_id,
            }),
            HistoryRequest::ReparentObject {
                object_id,
                new_parent_id,
                old_parent,
            } => {
                let old_parent_id = match old_parent {
                    PriorCapture::Ui(capture) => self.resolve_capture(
                        &CorrectionKey::Parent {
                            object_id: object_id.clone(),
                        },
                        capture,
                        source,
                        generation,
                        |value| match value {
                            CorrectionValue::Parent(parent_id) => Some(parent_id.clone()),
                            CorrectionValue::Property(_) => None,
                        },
                    )?,
                    PriorCapture::InAction if source == EditSource::Mcp => None,
                    PriorCapture::InAction => return Err(refresh_required()),
                };
                Ok(PreparedHistoryRequest::ReparentObject {
                    object_id,
                    new_parent_id,
                    old_parent_id,
                })
            }
            HistoryRequest::SetProperty {
                object_id,
                property,
                requested_value,
                old_value,
            } => {
                let old_value = match old_value {
                    PriorCapture::Ui(capture) => self.resolve_capture(
                        &CorrectionKey::Property {
                            object_id: object_id.clone(),
                            property: property.clone(),
                        },
                        capture,
                        source,
                        generation,
                        |value| match value {
                            CorrectionValue::Property(value) => Some(value.clone()),
                            CorrectionValue::Parent(_) => None,
                        },
                    )?,
                    PriorCapture::InAction if source == EditSource::Mcp => None,
                    PriorCapture::InAction => return Err(refresh_required()),
                };
                Ok(PreparedHistoryRequest::SetProperty {
                    object_id,
                    property,
                    requested_value,
                    old_value,
                })
            }
            HistoryRequest::DeleteObject { object_id } => {
                Ok(PreparedHistoryRequest::DeleteObject { object_id })
            }
        }
    }

    fn resolve_capture<T: Clone>(
        &self,
        key: &CorrectionKey,
        capture: HistoryCapture<T>,
        source: EditSource,
        generation: u64,
        correction: impl FnOnce(&CorrectionValue) -> Option<T>,
    ) -> Result<Option<T>, BackendError> {
        if source != EditSource::Ui
            || capture.generation != generation
            || self.generation != Some(generation)
            || capture.revision > self.applied_revision
        {
            return Err(refresh_required());
        }

        let cached = self.corrections.get(key);
        if let Some(entry) = cached.filter(|entry| entry.revision > capture.revision) {
            return correction(&entry.value)
                .map(Some)
                .ok_or_else(refresh_required);
        }

        if capture.value.is_some()
            && capture.revision < self.corrections.lost_through
            && cached.is_none()
        {
            return Err(refresh_required());
        }
        Ok(capture.value)
    }

    #[cfg(test)]
    pub(super) fn record_success(
        &mut self,
        prepared: PreparedHistoryRequest,
        source: EditSource,
        kind: EditKind,
        sequence: u64,
        generation: u64,
        result: QueuedEditResult,
    ) {
        self.record_success_in_group(
            prepared,
            HistoryOperationContext {
                source,
                kind,
                sequence,
                generation,
                group_token: None,
            },
            result,
        );
    }

    pub(super) fn record_success_in_group(
        &mut self,
        prepared: PreparedHistoryRequest,
        context: HistoryOperationContext,
        result: QueuedEditResult,
    ) {
        if result.value.get("accepted").and_then(Value::as_bool) != Some(true) {
            return;
        }

        if let PreparedHistoryRequest::DeleteObject { object_id: _ } = prepared {
            self.applied_revision = self.applied_revision.wrapping_add(1);
            self.corrections
                .clear_and_mark_lost_through(self.applied_revision);
            self.undo.clear();
            self.redo.clear();
            self.push_marker(
                context.source,
                context.kind,
                context.sequence,
                context.generation,
            );
            return;
        }

        self.applied_revision = self.applied_revision.wrapping_add(1);
        self.push_marker(
            context.source,
            context.kind,
            context.sequence,
            context.generation,
        );
        let revision = self.applied_revision;
        let record = match prepared {
            PreparedHistoryRequest::CreateObject { parent_id, kind } => result
                .value
                .get("newId")
                .and_then(Value::as_str)
                .filter(|created_id| !created_id.is_empty())
                .map(|created_id| {
                    self.corrections.insert(
                        CorrectionKey::Parent {
                            object_id: created_id.to_owned(),
                        },
                        revision,
                        CorrectionValue::Parent(parent_id.clone()),
                    );
                    HistoryRecord::Create {
                        created_id: created_id.to_owned(),
                        parent_id,
                        kind,
                    }
                }),
            PreparedHistoryRequest::DuplicateObject {
                source_object_id,
                parent_id,
            } => result
                .value
                .get("newId")
                .and_then(Value::as_str)
                .filter(|created_id| !created_id.is_empty())
                .map(|created_id| {
                    if let Some(parent_id) = &parent_id {
                        self.corrections.insert(
                            CorrectionKey::Parent {
                                object_id: created_id.to_owned(),
                            },
                            revision,
                            CorrectionValue::Parent(Some(parent_id.clone())),
                        );
                    }
                    HistoryRecord::Duplicate {
                        source_object_id,
                        created_id: created_id.to_owned(),
                        parent_id,
                    }
                }),
            PreparedHistoryRequest::ReparentObject {
                object_id,
                new_parent_id,
                old_parent_id,
            } => {
                self.corrections.insert(
                    CorrectionKey::Parent {
                        object_id: object_id.clone(),
                    },
                    revision,
                    CorrectionValue::Parent(new_parent_id.clone()),
                );
                let old_parent_id = old_parent_id.or(match result.prior {
                    Some(HistoryPrior::Parent(parent_id)) => Some(parent_id),
                    _ => None,
                });
                old_parent_id
                    .filter(|old_parent_id| old_parent_id != &new_parent_id)
                    .map(|old_parent_id| HistoryRecord::Reparent {
                        object_id,
                        old_parent_id,
                        new_parent_id,
                    })
            }
            PreparedHistoryRequest::SetProperty {
                object_id,
                property,
                requested_value,
                old_value,
            } => {
                let new_value = result
                    .value
                    .get("appliedValue")
                    .cloned()
                    .unwrap_or(requested_value);
                self.corrections.insert(
                    CorrectionKey::Property {
                        object_id: object_id.clone(),
                        property: property.clone(),
                    },
                    revision,
                    CorrectionValue::Property(new_value.clone()),
                );
                let old_value = old_value.or(match result.prior {
                    Some(HistoryPrior::Property(value)) => Some(value),
                    _ => None,
                });
                old_value
                    .filter(|old_value| !property_values_equal(old_value, &new_value))
                    .map(|old_value| HistoryRecord::SetProperty {
                        object_id,
                        property,
                        old_value,
                        new_value,
                    })
            }
            PreparedHistoryRequest::DeleteObject { .. } => None,
        };

        if let Some(record) = record {
            let marker = self
                .markers
                .last()
                .expect("accepted マーカーを記録済み")
                .clone();
            let active_match = self.active_group.as_ref().filter(|group| {
                Some(group.token) == context.group_token
                    && group.metadata.key.generation == context.generation
                    && group.metadata.source == context.source
            });
            if let Some(group) = active_match {
                let mut metadata = group.metadata.clone();
                metadata.count = group.entries.len().saturating_add(1);
                let entry = HistoryEntry {
                    marker,
                    record,
                    group: metadata,
                };
                if let Some(group) = self.active_group.as_mut() {
                    group.entries.push(entry);
                    group.last_edit_at = Instant::now();
                }
                if self
                    .active_group
                    .as_ref()
                    .is_some_and(|group| group.entries.len() == 1)
                {
                    self.redo.clear();
                }
                if self
                    .active_group
                    .as_ref()
                    .is_some_and(|group| group.entries.len() >= MAX_EDITS)
                {
                    self.close_active_group();
                }
                self.expire_group();
            } else {
                let metadata = GroupMetadata {
                    key: GroupKey {
                        generation: context.generation,
                        sequence: context.sequence,
                    },
                    name: record.summary_name().to_owned(),
                    source: context.source,
                    created_at: unix_millis(),
                    count: 1,
                    named: false,
                };
                self.undo.push(HistoryEntry {
                    marker,
                    record,
                    group: metadata,
                });
                self.redo.clear();
            }
        }
    }

    fn push_marker(&mut self, source: EditSource, kind: EditKind, sequence: u64, generation: u64) {
        self.markers.push(HistoryMarker {
            sequence,
            source,
            kind,
            generation,
        });
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn record_untracked_success(
        &mut self,
        source: EditSource,
        kind: EditKind,
        sequence: u64,
        generation: u64,
    ) {
        self.applied_revision = self.applied_revision.wrapping_add(1);
        self.push_marker(source, kind, sequence, generation);
    }

    #[cfg(test)]
    pub(super) fn set_pending_for_test(&mut self, pending: bool) {
        self.pending = pending;
        self.pending_group = None;
        self.revision = self.revision.wrapping_add(1);
    }

    #[cfg(test)]
    pub(super) fn undo_records(&self) -> Vec<HistoryRecord> {
        self.undo.iter().map(|entry| entry.record.clone()).collect()
    }

    #[cfg(test)]
    pub(super) fn redo_len(&self) -> usize {
        self.redo.len()
    }

    #[cfg(test)]
    pub(super) fn seed_undo_record(&mut self, sequence: u64, record: HistoryRecord) {
        self.generation.get_or_insert(1);
        let generation = self.generation.unwrap_or_default();
        let group = test_group_metadata(generation, sequence, &record);
        self.undo.push(HistoryEntry {
            marker: HistoryMarker {
                sequence,
                source: EditSource::Ui,
                kind: EditKind::Edit,
                generation,
            },
            record,
            group,
        });
        self.revision = self.revision.wrapping_add(1);
    }

    #[cfg(test)]
    pub(super) fn seed_undo_group_for_test(
        &mut self,
        first_sequence: u64,
        records: Vec<HistoryRecord>,
    ) {
        let Some(first) = records.first() else {
            return;
        };
        self.generation.get_or_insert(1);
        let generation = self.generation.unwrap_or_default();
        let mut group = test_group_metadata(generation, first_sequence, first);
        group.count = records.len();
        group.named = true;
        let key = group.key.clone();
        for (offset, record) in records.into_iter().enumerate() {
            let sequence = first_sequence.wrapping_add(offset as u64);
            self.undo.push(HistoryEntry {
                marker: HistoryMarker {
                    sequence,
                    source: EditSource::Ui,
                    kind: EditKind::Edit,
                    generation,
                },
                record,
                group: GroupMetadata {
                    key: key.clone(),
                    ..group.clone()
                },
            });
        }
        self.revision = self.revision.wrapping_add(1);
    }

    #[cfg(test)]
    pub(super) fn seed_redo_record(&mut self, sequence: u64, record: HistoryRecord) {
        self.generation.get_or_insert(1);
        let generation = self.generation.unwrap_or_default();
        let group = test_group_metadata(generation, sequence, &record);
        self.redo.push(HistoryEntry {
            marker: HistoryMarker {
                sequence,
                source: EditSource::Ui,
                kind: EditKind::Edit,
                generation,
            },
            record,
            group,
        });
        self.revision = self.revision.wrapping_add(1);
    }

    #[cfg(test)]
    pub(super) fn undo_len(&self) -> usize {
        self.undo.len()
    }

    #[cfg(test)]
    pub(super) fn mark_edit_unsupported_for_test(&mut self) {
        self.edit_unsupported = true;
    }

    #[cfg(test)]
    pub(super) fn seed_redo(&mut self) {
        self.seed_redo_record(
            0,
            HistoryRecord::Create {
                created_id: "redo-entry".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
    }

    #[cfg(test)]
    pub(super) fn correction_stats(&self) -> (usize, usize) {
        (self.corrections.entries.len(), self.corrections.bytes)
    }
}

pub(super) fn group_id(generation: u64, sequence: u64) -> String {
    format!("edit-{generation}-{sequence}")
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
fn test_group_metadata(generation: u64, sequence: u64, record: &HistoryRecord) -> GroupMetadata {
    GroupMetadata {
        key: GroupKey {
            generation,
            sequence,
        },
        name: record.summary_name().to_owned(),
        source: EditSource::Ui,
        created_at: unix_millis(),
        count: 1,
        named: false,
    }
}

fn updated_history_record(
    record: &HistoryRecord,
    direction: HistoryDirection,
    result: &Value,
) -> Result<HistoryRecord, BackendError> {
    match (record, direction) {
        (
            HistoryRecord::Create {
                parent_id, kind, ..
            },
            HistoryDirection::Redo,
        ) => {
            let Some(created_id) = non_empty_new_id(result) else {
                return Err(malformed_history_result("newId"));
            };
            Ok(HistoryRecord::Create {
                created_id,
                parent_id: parent_id.clone(),
                kind: kind.clone(),
            })
        }
        (
            HistoryRecord::Duplicate {
                source_object_id,
                parent_id,
                ..
            },
            HistoryDirection::Redo,
        ) => {
            let Some(created_id) = non_empty_new_id(result) else {
                return Err(malformed_history_result("newId"));
            };
            Ok(HistoryRecord::Duplicate {
                source_object_id: source_object_id.clone(),
                created_id,
                parent_id: parent_id.clone(),
            })
        }
        _ => Ok(record.clone()),
    }
}

fn changed_created_id(old: &HistoryRecord, new: &HistoryRecord) -> Option<(String, String)> {
    match (old, new) {
        (
            HistoryRecord::Create {
                created_id: old_id, ..
            },
            HistoryRecord::Create {
                created_id: new_id, ..
            },
        )
        | (
            HistoryRecord::Duplicate {
                created_id: old_id, ..
            },
            HistoryRecord::Duplicate {
                created_id: new_id, ..
            },
        ) if old_id != new_id => Some((old_id.clone(), new_id.clone())),
        _ => None,
    }
}

fn replace_record_ids(record: &mut HistoryRecord, old_id: &str, new_id: &str) {
    let replace = |value: &mut String| {
        if value == old_id {
            *value = new_id.to_owned();
        }
    };
    let replace_option = |value: &mut Option<String>| {
        if value.as_deref() == Some(old_id) {
            *value = Some(new_id.to_owned());
        }
    };
    match record {
        HistoryRecord::Create {
            created_id,
            parent_id,
            ..
        } => {
            replace(created_id);
            replace_option(parent_id);
        }
        HistoryRecord::Duplicate {
            source_object_id,
            created_id,
            parent_id,
        } => {
            replace(source_object_id);
            replace(created_id);
            replace_option(parent_id);
        }
        HistoryRecord::Reparent {
            object_id,
            old_parent_id,
            new_parent_id,
        } => {
            replace(object_id);
            replace_option(old_parent_id);
            replace_option(new_parent_id);
        }
        HistoryRecord::SetProperty { object_id, .. } => replace(object_id),
    }
}

impl HistoryRecord {
    fn summary_name(&self) -> &'static str {
        match self {
            Self::Create { .. } => "オブジェクトを作成",
            Self::Duplicate { .. } => "オブジェクトを複製",
            Self::Reparent { .. } => "親を変更",
            Self::SetProperty { .. } => "プロパティを変更",
        }
    }
}

fn refresh_required() -> BackendError {
    BackendError::Request {
        message: "編集前の表示が古く、履歴の旧値を安全に補正できません。対象を再取得してから操作してください。"
            .to_owned(),
    }
}

fn non_empty_new_id(value: &Value) -> Option<String> {
    value
        .get("newId")
        .and_then(Value::as_str)
        .filter(|created_id| !created_id.is_empty())
        .map(str::to_owned)
}

fn malformed_history_result(field: &str) -> BackendError {
    BackendError::Request {
        message: format!("やり直し結果の {field} がありません。対象の履歴を破棄しました。"),
    }
}

fn property_values_equal(left: &Value, right: &Value) -> bool {
    js_stringify(left) == js_stringify(right)
}

fn js_stringify(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => js_number_string(value),
        Value::String(value) => serde_json::to_string(value).expect("文字列値を直列化できる"),
        Value::Array(values) => {
            let items = values.iter().map(js_stringify).collect::<Vec<_>>();
            format!("[{}]", items.join(","))
        }
        Value::Object(values) => {
            let mut integer_keys = Vec::new();
            let mut string_keys = Vec::new();
            for (key, value) in values {
                if let Some(index) = javascript_array_index(key) {
                    integer_keys.push((index, key, value));
                } else {
                    string_keys.push((key, value));
                }
            }
            integer_keys.sort_by_key(|(index, _, _)| *index);
            let fields = integer_keys
                .into_iter()
                .map(|(_, key, value)| (key, value))
                .chain(string_keys)
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("オブジェクトのキーを直列化できる"),
                        js_stringify(value)
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", fields.join(","))
        }
    }
}

fn javascript_array_index(key: &str) -> Option<u32> {
    let index = key.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == key).then_some(index)
}

fn js_number_string(number: &serde_json::Number) -> String {
    let value = number
        .as_f64()
        .expect("serde_json の数値は有限で f64 で表現できる");
    if value == 0.0 {
        return "0".to_owned();
    }

    let raw = value.to_string();
    let (negative, unsigned) = raw
        .strip_prefix('-')
        .map_or((false, raw.as_str()), |unsigned| (true, unsigned));
    let (coefficient, exponent) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, 0_i32), |(coefficient, exponent)| {
            (coefficient, exponent.parse::<i32>().unwrap_or_default())
        });
    let decimal_index = coefficient.find('.').unwrap_or(coefficient.len()) as i32 + exponent;
    let mut digits = coefficient
        .chars()
        .filter(|character| *character != '.')
        .collect::<String>();
    let mut decimal_index = decimal_index;
    while digits.starts_with('0') && digits.len() > 1 {
        digits.remove(0);
        decimal_index -= 1;
    }
    while digits.ends_with('0') && digits.len() > 1 {
        digits.pop();
    }

    let sign = if negative { "-" } else { "" };
    if decimal_index > -6 && decimal_index <= 21 {
        let body = if decimal_index <= 0 {
            format!("0.{}{}", "0".repeat((-decimal_index) as usize), digits)
        } else if decimal_index as usize >= digits.len() {
            format!(
                "{digits}{}",
                "0".repeat(decimal_index as usize - digits.len())
            )
        } else {
            let decimal_index = decimal_index as usize;
            format!("{}.{}", &digits[..decimal_index], &digits[decimal_index..])
        };
        format!("{sign}{body}")
    } else {
        let exponent = decimal_index - 1;
        let mantissa = if digits.len() == 1 {
            digits
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        let exponent_sign = if exponent >= 0 { "+" } else { "" };
        format!("{sign}{mantissa}e{exponent_sign}{exponent}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepted(value: Value) -> QueuedEditResult {
        QueuedEditResult::plain(value)
    }

    fn apply(
        state: &mut HistoryState,
        request: HistoryRequest,
        source: EditSource,
        generation: u64,
        sequence: u64,
        result: QueuedEditResult,
    ) -> Result<(), BackendError> {
        let prepared = state.prepare(request, source, generation)?;
        state.record_success(
            prepared,
            source,
            EditKind::Edit,
            sequence,
            generation,
            result,
        );
        Ok(())
    }

    fn capture<T>(generation: u64, revision: u64, value: Option<T>) -> HistoryCapture<T> {
        HistoryCapture {
            generation,
            revision,
            value,
        }
    }

    #[test]
    fn records_only_accepted_edits_and_uses_new_ids_and_applied_values() {
        let mut state = HistoryState::default();
        state.synchronize(Some(7));
        state.seed_redo();

        apply(
            &mut state,
            HistoryRequest::CreateObject {
                parent_id: None,
                kind: None,
            },
            EditSource::Ui,
            7,
            1,
            accepted(serde_json::json!({"accepted": false, "newId": "ignored"})),
        )
        .expect("capture is not needed for create");
        apply(
            &mut state,
            HistoryRequest::DuplicateObject {
                source_object_id: "source".to_owned(),
                parent_id: None,
            },
            EditSource::Mcp,
            7,
            2,
            accepted(serde_json::json!({"accepted": false, "newId": "ignored"})),
        )
        .expect("rejected duplicate does not record");
        apply(
            &mut state,
            HistoryRequest::ReparentObject {
                object_id: "child".to_owned(),
                new_parent_id: Some("next".to_owned()),
                old_parent: PriorCapture::Ui(capture(7, 0, Some(None))),
            },
            EditSource::Ui,
            7,
            3,
            accepted(serde_json::json!({"accepted": false})),
        )
        .expect("rejected reparent does not record");
        apply(
            &mut state,
            HistoryRequest::SetProperty {
                object_id: "object".to_owned(),
                property: "strength".to_owned(),
                requested_value: serde_json::json!(2),
                old_value: PriorCapture::Ui(capture(7, 0, Some(serde_json::json!(1)))),
            },
            EditSource::Ui,
            7,
            4,
            accepted(serde_json::json!({"accepted": false, "appliedValue": 2})),
        )
        .expect("rejected property edit does not record");
        assert!(state.undo_records().is_empty());
        assert_eq!(state.redo_len(), 1);

        apply(
            &mut state,
            HistoryRequest::CreateObject {
                parent_id: None,
                kind: None,
            },
            EditSource::Ui,
            7,
            5,
            accepted(serde_json::json!({"accepted": true, "newId": "created"})),
        )
        .expect("create is accepted");

        apply(
            &mut state,
            HistoryRequest::DuplicateObject {
                source_object_id: "source".to_owned(),
                parent_id: Some("parent".to_owned()),
            },
            EditSource::Mcp,
            7,
            6,
            accepted(serde_json::json!({"accepted": true, "newId": "copy"})),
        )
        .expect("duplicate is accepted");

        let current = state.applied_revision();
        apply(
            &mut state,
            HistoryRequest::ReparentObject {
                object_id: "child".to_owned(),
                new_parent_id: Some("new-parent".to_owned()),
                old_parent: PriorCapture::Ui(capture(
                    7,
                    current,
                    Some(Some("old-parent".to_owned())),
                )),
            },
            EditSource::Ui,
            7,
            7,
            accepted(serde_json::json!({"accepted": true})),
        )
        .expect("parent capture is current");

        let current = state.applied_revision();
        apply(
            &mut state,
            HistoryRequest::SetProperty {
                object_id: "object".to_owned(),
                property: "strength".to_owned(),
                requested_value: serde_json::json!(2),
                old_value: PriorCapture::Ui(capture(7, current, Some(serde_json::json!(1)))),
            },
            EditSource::Ui,
            7,
            8,
            accepted(serde_json::json!({"accepted": true, "appliedValue": 3})),
        )
        .expect("property capture is current");

        assert_eq!(
            state.undo_records(),
            [
                HistoryRecord::Create {
                    created_id: "created".to_owned(),
                    parent_id: None,
                    kind: None,
                },
                HistoryRecord::Duplicate {
                    source_object_id: "source".to_owned(),
                    created_id: "copy".to_owned(),
                    parent_id: Some("parent".to_owned()),
                },
                HistoryRecord::Reparent {
                    object_id: "child".to_owned(),
                    old_parent_id: Some("old-parent".to_owned()),
                    new_parent_id: Some("new-parent".to_owned()),
                },
                HistoryRecord::SetProperty {
                    object_id: "object".to_owned(),
                    property: "strength".to_owned(),
                    old_value: serde_json::json!(1),
                    new_value: serde_json::json!(3),
                },
            ]
        );
    }

    #[test]
    fn accepted_create_and_duplicate_without_new_ids_are_not_recorded() {
        let mut state = HistoryState::default();
        state.synchronize(Some(3));
        state.seed_redo();

        apply(
            &mut state,
            HistoryRequest::CreateObject {
                parent_id: None,
                kind: None,
            },
            EditSource::Ui,
            3,
            1,
            accepted(serde_json::json!({"accepted": true})),
        )
        .expect("create is accepted");
        apply(
            &mut state,
            HistoryRequest::DuplicateObject {
                source_object_id: "source".to_owned(),
                parent_id: None,
            },
            EditSource::Mcp,
            3,
            2,
            accepted(serde_json::json!({"accepted": true})),
        )
        .expect("duplicate is accepted");

        assert!(state.undo_records().is_empty());
        assert_eq!(state.redo_len(), 1);
    }

    #[test]
    fn skips_equal_and_missing_old_values_without_clearing_redo() {
        let mut state = HistoryState::default();
        state.synchronize(Some(1));
        state.seed_redo();

        apply(
            &mut state,
            HistoryRequest::SetProperty {
                object_id: "object".to_owned(),
                property: "value".to_owned(),
                requested_value: serde_json::json!(1),
                old_value: PriorCapture::Ui(capture(1, 0, Some(serde_json::json!(1.0)))),
            },
            EditSource::Ui,
            1,
            1,
            accepted(serde_json::json!({"accepted": true, "appliedValue": 1})),
        )
        .expect("current capture is valid");
        assert!(state.undo_records().is_empty());
        assert_eq!(state.redo_len(), 1);

        let revision = state.applied_revision();
        apply(
            &mut state,
            HistoryRequest::SetProperty {
                object_id: "other".to_owned(),
                property: "value".to_owned(),
                requested_value: serde_json::Value::Null,
                old_value: PriorCapture::Ui(capture(1, revision, None)),
            },
            EditSource::Ui,
            1,
            2,
            accepted(serde_json::json!({"accepted": true, "appliedValue": null})),
        )
        .expect("missing old value does not block the edit");
        assert!(state.undo_records().is_empty());
        assert_eq!(state.redo_len(), 1);

        let revision = state.applied_revision();
        apply(
            &mut state,
            HistoryRequest::SetProperty {
                object_id: "object".to_owned(),
                property: "value".to_owned(),
                requested_value: serde_json::json!(2),
                old_value: PriorCapture::Ui(capture(1, revision, Some(serde_json::json!(1)))),
            },
            EditSource::Ui,
            1,
            3,
            accepted(serde_json::json!({"accepted": false})),
        )
        .expect("capture is current");
        assert!(state.undo_records().is_empty());
        assert_eq!(state.redo_len(), 1);
    }

    #[test]
    fn new_record_invalidates_redo_for_every_history_kind() {
        let requests = [
            HistoryRequest::CreateObject {
                parent_id: None,
                kind: None,
            },
            HistoryRequest::DuplicateObject {
                source_object_id: "source".to_owned(),
                parent_id: None,
            },
            HistoryRequest::ReparentObject {
                object_id: "child".to_owned(),
                new_parent_id: Some("next".to_owned()),
                old_parent: PriorCapture::Ui(capture(2, 0, Some(None))),
            },
            HistoryRequest::SetProperty {
                object_id: "object".to_owned(),
                property: "value".to_owned(),
                requested_value: serde_json::json!(2),
                old_value: PriorCapture::Ui(capture(2, 0, Some(serde_json::json!(1)))),
            },
        ];

        for (index, request) in requests.into_iter().enumerate() {
            let mut state = HistoryState::default();
            state.synchronize(Some(2));
            state.seed_redo();
            let result = match index {
                0 => serde_json::json!({"accepted": true, "newId": "created"}),
                1 => serde_json::json!({"accepted": true, "newId": "copy"}),
                2 => serde_json::json!({"accepted": true}),
                _ => serde_json::json!({"accepted": true, "appliedValue": 2}),
            };
            apply(
                &mut state,
                request,
                EditSource::Ui,
                2,
                index as u64 + 1,
                accepted(result),
            )
            .expect("capture is current");
            assert_eq!(state.undo_records().len(), 1);
            assert_eq!(state.redo_len(), 0);
            if index == 2 {
                assert_eq!(
                    state.undo_records(),
                    [HistoryRecord::Reparent {
                        object_id: "child".to_owned(),
                        old_parent_id: None,
                        new_parent_id: Some("next".to_owned()),
                    }]
                );
            }
        }
    }

    #[test]
    fn records_known_null_prior_and_skips_unavailable_prior() {
        let mut state = HistoryState::default();
        state.synchronize(Some(4));
        apply(
            &mut state,
            HistoryRequest::SetProperty {
                object_id: "object".to_owned(),
                property: "known-null".to_owned(),
                requested_value: serde_json::json!("new"),
                old_value: PriorCapture::Ui(capture(4, 0, Some(Value::Null))),
            },
            EditSource::Ui,
            4,
            1,
            accepted(serde_json::json!({"accepted": true, "appliedValue": "new"})),
        )
        .expect("null was captured");
        apply(
            &mut state,
            HistoryRequest::SetProperty {
                object_id: "object".to_owned(),
                property: "missing".to_owned(),
                requested_value: serde_json::json!("new"),
                old_value: PriorCapture::Ui(capture(4, 1, None)),
            },
            EditSource::Ui,
            4,
            2,
            accepted(serde_json::json!({"accepted": true, "appliedValue": "new"})),
        )
        .expect("missing old value is accepted without recording");

        assert_eq!(
            state.undo_records(),
            [HistoryRecord::SetProperty {
                object_id: "object".to_owned(),
                property: "known-null".to_owned(),
                old_value: Value::Null,
                new_value: serde_json::json!("new"),
            }]
        );
    }

    #[test]
    fn json_stringify_compatibility_keeps_key_order_and_javascript_number_rules() {
        let first: Value =
            serde_json::from_str(r#"{"b":1,"2":"two","10":"ten","01":"leading","a":2}"#)
                .expect("first object parses");
        let same_order: Value =
            serde_json::from_str(r#"{"2":"two","10":"ten","b":1,"01":"leading","a":2}"#)
                .expect("second object parses");
        assert!(property_values_equal(&first, &same_order));

        let other_order: Value =
            serde_json::from_str(r#"{"a":2,"b":1,"2":"two","10":"ten","01":"leading"}"#)
                .expect("third object parses");
        assert!(!property_values_equal(&first, &other_order));
        assert_eq!(
            js_stringify(&first),
            r#"{"2":"two","10":"ten","b":1,"01":"leading","a":2}"#
        );

        let one_integer: Value = serde_json::from_str("1").expect("integer parses");
        let one_float: Value = serde_json::from_str("1.0").expect("float parses");
        let negative_zero: Value = serde_json::from_str("-0.0").expect("negative zero parses");
        let zero: Value = serde_json::from_str("0").expect("zero parses");
        let tiny_exp: Value = serde_json::from_str("1e-6").expect("exponent parses");
        let tiny_fixed: Value = serde_json::from_str("0.000001").expect("fixed parses");
        let large_exp: Value = serde_json::from_str("1e20").expect("large exponent parses");
        let large_fixed: Value =
            serde_json::from_str("100000000000000000000").expect("large integer parses");
        assert!(property_values_equal(&one_integer, &one_float));
        assert!(property_values_equal(&negative_zero, &zero));
        assert!(property_values_equal(&tiny_exp, &tiny_fixed));
        assert!(property_values_equal(&large_exp, &large_fixed));
        assert!(property_values_equal(&Value::Null, &Value::Null));
        assert!(!property_values_equal(
            &Value::Null,
            &Value::String("null".to_owned())
        ));
        assert_eq!(js_stringify(&negative_zero), "0");
        assert_eq!(js_stringify(&tiny_exp), "0.000001");
        assert_eq!(js_stringify(&large_exp), "100000000000000000000");
        assert_eq!(js_stringify(&serde_json::json!(1e-7)), "1e-7");
        assert_eq!(js_stringify(&serde_json::json!(1e21)), "1e+21");
    }

    #[test]
    fn missing_and_json_null_old_values_are_distinct() {
        let missing = capture::<Value>(1, 0, None);
        let known_null = capture(1, 0, Some(Value::Null));
        assert_ne!(missing, known_null);
    }

    #[test]
    fn undo_redo_correction_cache_uses_engine_applied_value() {
        let mut state = HistoryState::default();
        state.synchronize(Some(4));
        state.seed_undo_record(
            20,
            HistoryRecord::SetProperty {
                object_id: "object".to_owned(),
                property: "value".to_owned(),
                old_value: serde_json::json!(1),
                new_value: serde_json::json!(2),
            },
        );

        let (head_id, revision) = state.history_cursor(HistoryDirection::Undo);
        let undo = state
            .prepare_action(HistoryActionRequest {
                direction: HistoryDirection::Undo,
                expected_head_id: head_id,
                expected_revision: revision,
            })
            .expect("undoの先頭が一致する");
        assert!(state
            .finish_action_success(
                undo,
                EditSource::Ui,
                EditKind::Undo,
                21,
                4,
                serde_json::json!({"accepted": true, "appliedValue": 10}),
            )
            .expect("undoを適用できる"));
        assert_eq!(
            state.corrections.entries[&CorrectionKey::Property {
                object_id: "object".to_owned(),
                property: "value".to_owned(),
            }]
                .value,
            CorrectionValue::Property(serde_json::json!(10))
        );

        let (head_id, revision) = state.history_cursor(HistoryDirection::Redo);
        let redo = state
            .prepare_action(HistoryActionRequest {
                direction: HistoryDirection::Redo,
                expected_head_id: head_id,
                expected_revision: revision,
            })
            .expect("redoの先頭が一致する");
        assert!(state
            .finish_action_success(
                redo,
                EditSource::Ui,
                EditKind::Redo,
                22,
                4,
                serde_json::json!({"accepted": true, "appliedValue": 20}),
            )
            .expect("redoを適用できる"));
        assert_eq!(
            state.corrections.entries[&CorrectionKey::Property {
                object_id: "object".to_owned(),
                property: "value".to_owned(),
            }]
                .value,
            CorrectionValue::Property(serde_json::json!(20))
        );
    }

    #[test]
    fn correction_cache_is_bounded_by_entry_count_and_total_bytes() {
        let mut cache = CorrectionCache::default();
        for index in 0..=MAX_CORRECTION_ENTRIES {
            cache.insert(
                CorrectionKey::Property {
                    object_id: format!("object-{index}"),
                    property: "value".to_owned(),
                },
                index as u64 + 1,
                CorrectionValue::Property(Value::from(index as u64)),
            );
        }
        assert_eq!(cache.entries.len(), MAX_CORRECTION_ENTRIES);
        assert!(cache.bytes <= MAX_CORRECTION_BYTES);
        assert_eq!(cache.lost_through, 1);

        cache.clear();
        assert_eq!(cache.lost_through, 0);
        let first_key = CorrectionKey::Property {
            object_id: "first".to_owned(),
            property: "value".to_owned(),
        };
        let second_key = CorrectionKey::Property {
            object_id: "second".to_owned(),
            property: "value".to_owned(),
        };
        let value_size = MAX_CORRECTION_BYTES / 2 + 128;
        cache.insert(
            first_key.clone(),
            1,
            CorrectionValue::Property(Value::String("a".repeat(value_size))),
        );
        cache.insert(
            second_key.clone(),
            2,
            CorrectionValue::Property(Value::String("b".repeat(value_size))),
        );
        assert!(cache.get(&first_key).is_none());
        assert!(cache.get(&second_key).is_some());
        assert!(cache.bytes <= MAX_CORRECTION_BYTES);
        assert_eq!(cache.lost_through, 1);

        cache.clear();
        cache.insert(
            CorrectionKey::Property {
                object_id: "large".to_owned(),
                property: "value".to_owned(),
            },
            3,
            CorrectionValue::Property(Value::String("x".repeat(MAX_CORRECTION_BYTES))),
        );
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
        assert_eq!(cache.lost_through, 3);
    }

    #[test]
    fn accepted_delete_and_generation_change_release_correction_information() {
        let mut state = HistoryState::default();
        state.synchronize(Some(1));
        let revision = state.applied_revision();
        apply(
            &mut state,
            HistoryRequest::SetProperty {
                object_id: "object".to_owned(),
                property: "value".to_owned(),
                requested_value: serde_json::json!(2),
                old_value: PriorCapture::Ui(capture(1, revision, Some(serde_json::json!(1)))),
            },
            EditSource::Ui,
            1,
            1,
            accepted(serde_json::json!({"accepted": true})),
        )
        .expect("property edit is accepted");
        assert_eq!(state.correction_stats().0, 1);

        apply(
            &mut state,
            HistoryRequest::DeleteObject {
                object_id: "object".to_owned(),
            },
            EditSource::Ui,
            1,
            2,
            accepted(serde_json::json!({"accepted": true})),
        )
        .expect("delete does not need a capture");
        assert_eq!(state.correction_stats(), (0, 0));
        assert_eq!(state.corrections.lost_through, state.applied_revision());
        assert!(state.undo_records().is_empty());

        let revision = state.applied_revision();
        let stale_after_delete = HistoryRequest::SetProperty {
            object_id: "other-object".to_owned(),
            property: "value".to_owned(),
            requested_value: serde_json::json!(3),
            old_value: PriorCapture::Ui(capture(1, revision - 1, Some(serde_json::json!(2)))),
        };
        assert!(matches!(
            state.prepare(stale_after_delete, EditSource::Ui, 1),
            Err(BackendError::Request { message }) if message.contains("再取得")
        ));

        apply(
            &mut state,
            HistoryRequest::SetProperty {
                object_id: "object".to_owned(),
                property: "value".to_owned(),
                requested_value: serde_json::json!(3),
                old_value: PriorCapture::Ui(capture(1, revision, Some(serde_json::json!(2)))),
            },
            EditSource::Ui,
            1,
            3,
            accepted(serde_json::json!({"accepted": true})),
        )
        .expect("current capture is valid");
        assert_eq!(state.correction_stats().0, 1);
        state.synchronize(Some(2));
        assert_eq!(state.correction_stats(), (0, 0));
        assert_eq!(state.corrections.lost_through, 0);
        assert!(state.undo_records().is_empty());
        let stale_capture = HistoryRequest::SetProperty {
            object_id: "object".to_owned(),
            property: "value".to_owned(),
            requested_value: serde_json::json!(4),
            old_value: PriorCapture::Ui(capture(1, 1, Some(serde_json::json!(3)))),
        };
        assert!(matches!(
            state.prepare(stale_capture, EditSource::Ui, 2),
            Err(BackendError::Request { message }) if message.contains("再取得")
        ));
    }
}
