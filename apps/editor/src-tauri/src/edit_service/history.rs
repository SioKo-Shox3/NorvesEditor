//! 編集要求の結果から取り消し記録を作り、古い画面捕捉を安全に補正する。

use std::collections::HashMap;
use std::future::Future;

use serde_json::Value;

use super::{EditKind, EditSource};
use crate::bridge_state::BridgeLease;
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
    },
    Duplicate {
        source_object_id: String,
        created_id: String,
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

#[derive(Clone, Debug, PartialEq)]
struct HistoryEntry {
    marker: HistoryMarker,
    record: HistoryRecord,
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
    DeleteObject {
        object_id: String,
    },
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
            return;
        };
        if byte_len > MAX_CORRECTION_BYTES {
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
        self.entries = HashMap::new();
        self.bytes = 0;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum PreparedHistoryRequest {
    CreateObject {
        parent_id: Option<String>,
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
}

impl HistoryState {
    pub(super) fn synchronize(&mut self, generation: Option<u64>) {
        if self.generation != generation {
            self.generation = generation;
            self.undo.clear();
            self.redo.clear();
            self.corrections.clear();
            self.applied_revision = 0;
            self.markers.clear();
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn snapshot(&self) -> (Option<u64>, u64, Vec<HistoryMarker>) {
        (self.generation, self.revision, self.markers.clone())
    }

    pub(super) fn applied_revision(&self) -> u64 {
        self.applied_revision
    }

    pub(super) fn prepare(
        &self,
        request: HistoryRequest,
        source: EditSource,
        generation: u64,
    ) -> Result<PreparedHistoryRequest, BackendError> {
        match request {
            HistoryRequest::CreateObject { parent_id } => {
                Ok(PreparedHistoryRequest::CreateObject { parent_id })
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

        if capture.revision < self.applied_revision && cached.is_none() {
            return Err(refresh_required());
        }
        Ok(capture.value)
    }

    pub(super) fn record_success(
        &mut self,
        prepared: PreparedHistoryRequest,
        source: EditSource,
        kind: EditKind,
        sequence: u64,
        generation: u64,
        result: QueuedEditResult,
    ) {
        if result.value.get("accepted").and_then(Value::as_bool) != Some(true) {
            return;
        }

        if let PreparedHistoryRequest::DeleteObject { object_id: _ } = prepared {
            self.applied_revision = self.applied_revision.wrapping_add(1);
            self.corrections.clear();
            self.undo.clear();
            self.redo.clear();
            self.push_marker(source, kind, sequence, generation);
            return;
        }

        self.applied_revision = self.applied_revision.wrapping_add(1);
        self.push_marker(source, kind, sequence, generation);
        let revision = self.applied_revision;
        let record = match prepared {
            PreparedHistoryRequest::CreateObject { parent_id } => result
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
                        CorrectionValue::Parent(parent_id),
                    );
                    HistoryRecord::Create {
                        created_id: created_id.to_owned(),
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
                    if let Some(parent_id) = parent_id {
                        self.corrections.insert(
                            CorrectionKey::Parent {
                                object_id: created_id.to_owned(),
                            },
                            revision,
                            CorrectionValue::Parent(Some(parent_id)),
                        );
                    }
                    HistoryRecord::Duplicate {
                        source_object_id,
                        created_id: created_id.to_owned(),
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
            self.undo.push(HistoryEntry {
                marker: self
                    .markers
                    .last()
                    .expect("accepted marker was recorded")
                    .clone(),
                record,
            });
            self.redo.clear();
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
        self.push_marker(source, kind, sequence, generation);
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
    pub(super) fn seed_redo(&mut self) {
        self.redo.push(HistoryEntry {
            marker: HistoryMarker {
                sequence: 0,
                source: EditSource::Ui,
                kind: EditKind::Edit,
                generation: self.generation.unwrap_or_default(),
            },
            record: HistoryRecord::Create {
                created_id: "redo-entry".to_owned(),
            },
        });
    }

    #[cfg(test)]
    pub(super) fn correction_stats(&self) -> (usize, usize) {
        (self.corrections.entries.len(), self.corrections.bytes)
    }
}

fn refresh_required() -> BackendError {
    BackendError::Request {
        message: "編集前の表示が古く、履歴の旧値を安全に補正できません。対象を再取得してから操作してください。"
            .to_owned(),
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
        Value::String(value) => serde_json::to_string(value).expect("string values serialize"),
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
                        serde_json::to_string(key).expect("object keys serialize"),
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
        .expect("serde_json numbers are finite and representable as f64");
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
            HistoryRequest::CreateObject { parent_id: None },
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
            HistoryRequest::CreateObject { parent_id: None },
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
                },
                HistoryRecord::Duplicate {
                    source_object_id: "source".to_owned(),
                    created_id: "copy".to_owned(),
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
            HistoryRequest::CreateObject { parent_id: None },
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
            HistoryRequest::CreateObject { parent_id: None },
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
    fn correction_cache_is_bounded_by_entry_count_and_total_bytes() {
        let mut cache = CorrectionCache::default();
        for index in 0..=MAX_CORRECTION_ENTRIES {
            cache.insert(
                CorrectionKey::Property {
                    object_id: format!("object-{index}"),
                    property: "value".to_owned(),
                },
                index as u64,
                CorrectionValue::Property(Value::from(index as u64)),
            );
        }
        assert_eq!(cache.entries.len(), MAX_CORRECTION_ENTRIES);
        assert!(cache.bytes <= MAX_CORRECTION_BYTES);

        cache.clear();
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

        cache.clear();
        cache.insert(
            CorrectionKey::Property {
                object_id: "large".to_owned(),
                property: "value".to_owned(),
            },
            1,
            CorrectionValue::Property(Value::String("x".repeat(MAX_CORRECTION_BYTES))),
        );
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
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
        assert!(state.undo_records().is_empty());

        let revision = state.applied_revision();
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
