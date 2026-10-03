//! エンジンのログを世代別に有界保持し、時間と連番で読み出す。

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::mem::size_of;
use std::time::{SystemTime, UNIX_EPOCH};

use norves_bridge_core::LogLevel;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// 保持するログレコードの上限。
pub const MAX_LOG_ENTRIES: usize = 1000;
/// ログレコード本体と文字列を合わせた保持バイト数の上限。
pub const MAX_LOG_BYTES: usize = 2 * 1024 * 1024;
/// 1ページに返すログレコードの上限。
pub const MAX_LOG_PAGE_SIZE: usize = 1000;
/// 欠落情報を追跡する世代状態の上限。
pub const MAX_TRACKED_GENERATIONS: usize = MAX_LOG_ENTRIES + 1;
const DEFAULT_LOG_PAGE_SIZE: usize = 100;

/// Bridgeのログ購読状態。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogSubscriptionStatus {
    /// 接続直後で購読を始めていない。
    #[default]
    NotAttempted,
    /// 購読要求を処理中。
    Pending,
    /// 購読に成功した。
    Subscribed,
    /// エンジンが `log.stream` を広告していない。
    Unsupported,
    /// 購読要求または応答の処理に失敗した。
    Failed,
    /// 接続世代の終了により購読要求が取り消された。
    Cancelled,
    /// 成功応答に有効なsubscriptionIdがない。
    IncompatibleAck,
}

/// 保持したエンジンログ。文字列はエンジン由来の非信頼データ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// ログを受信したBridge接続世代。
    pub generation: u64,
    /// 世代内で単調増加する連番。
    pub sequence: u64,
    /// バックエンドが受信したUNIX時刻（ミリ秒）。
    pub received_at_ms: u64,
    /// エンジンが送った重要度。
    pub level: LogLevel,
    /// エンジンが送った本文。
    pub message: String,
    /// 任意のエンジン分類。
    pub category: Option<String>,
    /// 任意のエンジン側時刻。
    pub engine_timestamp: Option<String>,
    /// 保持上限のため本文または任意文字列を途中で切った。
    pub truncated: bool,
    #[serde(skip)]
    stored_bytes: usize,
}

/// 保管したログの世代内連番と切り詰め結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogRecordOutcome {
    /// 世代内で割り当てた連番。
    pub sequence: u64,
    /// 2MiBの個別上限に合わせて文字列を切り詰めた。
    pub truncated: bool,
}

/// 世代と受信時刻、連番によるログ読み出し条件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogQuery {
    /// 指定した接続世代だけを読む。
    pub generation: Option<u64>,
    /// この受信時刻以上を読む（ミリ秒、境界を含む）。
    pub from_time_ms: Option<u64>,
    /// この受信時刻以下を読む（ミリ秒、境界を含む）。
    pub to_time_ms: Option<u64>,
    /// この世代内連番より後を読む（境界を含まない）。
    pub after_sequence: Option<u64>,
    /// この世代内連番以下を読む（境界を含む）。
    pub through_sequence: Option<u64>,
    /// 返却件数。省略時100、最大1000。
    pub limit: Option<usize>,
}

/// 指定世代の保持範囲と欠落状況。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRetention {
    /// 接続世代。
    pub generation: u64,
    /// 現在保持している最古の連番。
    pub first_sequence: Option<u64>,
    /// 現在保持している最新の連番。
    pub last_sequence: Option<u64>,
    /// 保持または容量超過で失われたレコードを含む、最後に観測した連番。
    pub last_observed_sequence: u64,
    /// 現在保持している最古の受信時刻。
    pub first_received_at_ms: Option<u64>,
    /// 現在保持している最新の受信時刻。
    pub last_received_at_ms: Option<u64>,
    /// 容量超過または個別サイズ超過で失われたレコード数。
    pub missing_count: u64,
    /// 連番の欠落または切り詰めがある。
    pub has_gap: bool,
    /// 購読状態。subscriptionId自体は公開しない。
    pub subscription: LogSubscriptionStatus,
}

/// ログのページと、その世代ごとの保持情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSnapshot {
    /// 条件に合うログ。
    pub entries: Vec<LogEntry>,
    /// 条件で選んだ各世代の保持範囲。
    pub retention: Vec<LogRetention>,
    /// 同じ条件に、返却ページより後のログがある。
    pub has_more: bool,
    /// generationを一つに絞った場合の次ページ用連番。
    pub next_after_sequence: Option<u64>,
    /// 指定したafter_sequenceより前のログが保持範囲から欠けている。
    pub missing_before_cursor: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LogBufferError {
    InvalidShape,
    MissingLevel,
    InvalidLevel,
    MissingMessage,
    InvalidMessage,
    InvalidCategory,
    InvalidTimestamp,
    SequenceExhausted,
}

impl fmt::Display for LogBufferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidShape => "log.messageのparamsはオブジェクトではありません",
            Self::MissingLevel => "log.messageにlevelがありません",
            Self::InvalidLevel => "log.messageのlevelが不正です",
            Self::MissingMessage => "log.messageにmessageがありません",
            Self::InvalidMessage => "log.messageのmessageが文字列ではありません",
            Self::InvalidCategory => "log.messageのcategoryが文字列ではありません",
            Self::InvalidTimestamp => "log.messageのtimestampが文字列ではありません",
            Self::SequenceExhausted => "ログ連番の上限に達しました",
        })
    }
}

impl std::error::Error for LogBufferError {}

#[derive(Default)]
struct GenerationState {
    last_observed_sequence: u64,
    missing_count: u64,
    subscription: LogSubscriptionStatus,
}

/// 件数・保持バイト数を制限したエンジンログ保管庫。
#[derive(Default)]
pub struct LogBuffer {
    entries: VecDeque<LogEntry>,
    retained_bytes: usize,
    generations: BTreeMap<u64, GenerationState>,
    generation_order: VecDeque<u64>,
    active_generation: Option<u64>,
    last_generation: Option<u64>,
}

impl LogBuffer {
    /// 新しい接続世代を開始し、古い世代の状態だけを必要に応じて整理する。
    pub fn begin_generation(&mut self, generation: u64) {
        self.active_generation = Some(generation);
        self.last_generation = Some(generation);
        self.ensure_generation(generation);
        self.prune_generation_metadata();
    }

    /// 世代を切断状態にする。最後の世代の購読状態は次の接続まで残す。
    pub fn end_generation(&mut self, generation: u64) {
        if self.active_generation == Some(generation) {
            self.active_generation = None;
            self.last_generation = Some(generation);
            if let Some(state) = self.generations.get_mut(&generation) {
                if state.subscription == LogSubscriptionStatus::Pending {
                    state.subscription = LogSubscriptionStatus::Cancelled;
                }
            }
        }
        self.prune_generation_metadata();
    }

    /// 購読状態を記録する。
    pub fn set_subscription_status(&mut self, generation: u64, status: LogSubscriptionStatus) {
        self.ensure_generation(generation);
        self.generations.entry(generation).or_default().subscription = status;
        if self.active_generation.is_none() {
            self.last_generation = Some(generation);
        }
        self.prune_generation_metadata();
    }

    /// log.messageのparamsを保管する。個別サイズを超えた場合は連番と欠落数だけを残す。
    pub fn record(
        &mut self,
        generation: u64,
        params: &Map<String, Value>,
    ) -> Result<LogRecordOutcome, LogBufferError> {
        self.record_at(generation, params, now_unix_millis())
    }

    /// 受信時刻を指定してlog.messageを保管する。
    pub fn record_at(
        &mut self,
        generation: u64,
        params: &Map<String, Value>,
        received_at_ms: u64,
    ) -> Result<LogRecordOutcome, LogBufferError> {
        let borrowed = BorrowedLog::parse(params)?;
        let max_payload_bytes = MAX_LOG_BYTES.saturating_sub(size_of::<LogEntry>());
        let truncated = borrowed.payload_bytes() > max_payload_bytes;
        let (message, category, engine_timestamp) = borrowed.owned_fields(max_payload_bytes);
        let stored_bytes = size_of::<LogEntry>()
            .saturating_add(message.len())
            .saturating_add(category.as_ref().map_or(0, String::len))
            .saturating_add(engine_timestamp.as_ref().map_or(0, String::len));

        self.ensure_generation(generation);
        let state = self.generations.entry(generation).or_default();
        let sequence = state
            .last_observed_sequence
            .checked_add(1)
            .ok_or(LogBufferError::SequenceExhausted)?;
        state.last_observed_sequence = sequence;
        if self.active_generation.is_none() {
            self.last_generation = Some(generation);
        }

        let entry = LogEntry {
            generation,
            sequence,
            received_at_ms,
            level: borrowed.level,
            message,
            category,
            engine_timestamp,
            truncated,
            stored_bytes,
        };

        while self.entries.len() >= MAX_LOG_ENTRIES
            || self.retained_bytes.saturating_add(stored_bytes) > MAX_LOG_BYTES
        {
            let Some(oldest) = self.entries.pop_front() else {
                break;
            };
            self.retained_bytes = self.retained_bytes.saturating_sub(oldest.stored_bytes);
            if let Some(state) = self.generations.get_mut(&oldest.generation) {
                state.missing_count = state.missing_count.saturating_add(1);
            }
        }

        self.retained_bytes = self.retained_bytes.saturating_add(stored_bytes);
        self.entries.push_back(entry);
        self.prune_generation_metadata();
        Ok(LogRecordOutcome {
            sequence,
            truncated,
        })
    }

    /// 保持済みログと、世代別の保持範囲・欠落状況を返す。
    pub fn read(&self, query: &LogQuery) -> LogSnapshot {
        let limit = query
            .limit
            .unwrap_or(DEFAULT_LOG_PAGE_SIZE)
            .clamp(1, MAX_LOG_PAGE_SIZE);
        let mut entries = Vec::with_capacity(limit.min(self.entries.len()));
        let mut has_more = false;

        for entry in self
            .entries
            .iter()
            .filter(|entry| matches_query(entry, query))
        {
            if entries.len() == limit {
                has_more = true;
                break;
            }
            entries.push(entry.clone());
        }

        let generations = self
            .generations
            .keys()
            .copied()
            .filter(|generation| query.generation.is_none_or(|wanted| wanted == *generation));
        let retention = generations
            .filter_map(|generation| {
                self.generations
                    .get(&generation)
                    .map(|state| self.retention_for(generation, state))
            })
            .collect::<Vec<_>>();
        let missing_before_cursor = query.after_sequence.is_some_and(|cursor| {
            retention.iter().any(|range| {
                range
                    .first_sequence
                    .is_some_and(|first| first > cursor.saturating_add(1))
                    || (range.first_sequence.is_none() && range.last_observed_sequence > cursor)
            })
        });
        let next_after_sequence = query.generation.and_then(|_| {
            has_more
                .then(|| entries.last().map(|entry| entry.sequence))
                .flatten()
        });

        LogSnapshot {
            entries,
            retention,
            has_more,
            next_after_sequence,
            missing_before_cursor,
        }
    }

    /// 現在保持している件数を返す。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 保持件数が0かを返す。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// ログレコードとその文字列に割り当てた保持バイト数を返す。
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    fn retention_for(&self, generation: u64, state: &GenerationState) -> LogRetention {
        let first = self
            .entries
            .iter()
            .find(|entry| entry.generation == generation);
        let last = self
            .entries
            .iter()
            .rev()
            .find(|entry| entry.generation == generation);
        let first_sequence = first.map(|entry| entry.sequence);
        let last_sequence = last.map(|entry| entry.sequence);
        let has_gap = state.missing_count > 0
            || first_sequence.is_some_and(|sequence| sequence > 1)
            || last_sequence.is_some_and(|sequence| sequence < state.last_observed_sequence)
            || self
                .entries
                .iter()
                .any(|entry| entry.generation == generation && entry.truncated);

        LogRetention {
            generation,
            first_sequence,
            last_sequence,
            last_observed_sequence: state.last_observed_sequence,
            first_received_at_ms: first.map(|entry| entry.received_at_ms),
            last_received_at_ms: last.map(|entry| entry.received_at_ms),
            missing_count: state.missing_count,
            has_gap,
            subscription: state.subscription,
        }
    }

    fn ensure_generation(&mut self, generation: u64) {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            self.generations.entry(generation)
        {
            entry.insert(GenerationState::default());
            self.generation_order.push_back(generation);
        }
    }

    fn prune_generation_metadata(&mut self) {
        let retained_generations = self
            .entries
            .iter()
            .map(|entry| entry.generation)
            .collect::<std::collections::BTreeSet<_>>();
        while self.generations.len() > MAX_TRACKED_GENERATIONS {
            let removable = self.generation_order.iter().position(|generation| {
                !retained_generations.contains(generation)
                    && Some(*generation) != self.active_generation
                    && Some(*generation) != self.last_generation
            });
            let Some(index) = removable else {
                break;
            };
            if let Some(generation) = self.generation_order.remove(index) {
                self.generations.remove(&generation);
            }
        }
        self.generation_order
            .retain(|generation| self.generations.contains_key(generation));
    }
}

struct BorrowedLog<'a> {
    level: LogLevel,
    message: &'a str,
    category: Option<&'a str>,
    engine_timestamp: Option<&'a str>,
}

impl<'a> BorrowedLog<'a> {
    fn parse(params: &'a Map<String, Value>) -> Result<Self, LogBufferError> {
        let level_value = params.get("level").ok_or(LogBufferError::MissingLevel)?;
        let level = LogLevel::deserialize(level_value).map_err(|_| LogBufferError::InvalidLevel)?;
        let message = params
            .get("message")
            .ok_or(LogBufferError::MissingMessage)?
            .as_str()
            .ok_or(LogBufferError::InvalidMessage)?;
        let category =
            optional_string(params, "category").map_err(|_| LogBufferError::InvalidCategory)?;
        let engine_timestamp =
            optional_string(params, "timestamp").map_err(|_| LogBufferError::InvalidTimestamp)?;

        Ok(Self {
            level,
            message,
            category,
            engine_timestamp,
        })
    }

    fn payload_bytes(&self) -> usize {
        self.message
            .len()
            .saturating_add(self.category.map_or(0, str::len))
            .saturating_add(self.engine_timestamp.map_or(0, str::len))
    }

    fn owned_fields(&self, limit: usize) -> (String, Option<String>, Option<String>) {
        let mut remaining = limit;
        let message = bounded_copy(self.message, &mut remaining);
        let category = self
            .category
            .map(|value| bounded_copy(value, &mut remaining));
        let engine_timestamp = self
            .engine_timestamp
            .map(|value| bounded_copy(value, &mut remaining));
        (message, category, engine_timestamp)
    }
}

fn bounded_copy(value: &str, remaining: &mut usize) -> String {
    let mut end = value.len().min(*remaining);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    *remaining -= end;
    value[..end].to_owned()
}

fn optional_string<'a>(params: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, ()> {
    match params.get(key) {
        None => Ok(None),
        Some(value) => value.as_str().map(Some).ok_or(()),
    }
}

fn matches_query(entry: &LogEntry, query: &LogQuery) -> bool {
    query
        .generation
        .is_none_or(|generation| generation == entry.generation)
        && query
            .from_time_ms
            .is_none_or(|from| entry.received_at_ms >= from)
        && query.to_time_ms.is_none_or(|to| entry.received_at_ms <= to)
        && query
            .after_sequence
            .is_none_or(|after| entry.sequence > after)
        && query
            .through_sequence
            .is_none_or(|through| entry.sequence <= through)
}

fn now_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(message: impl Into<String>) -> Map<String, Value> {
        serde_json::json!({
            "level": "info",
            "message": message.into(),
            "category": "Engine"
        })
        .as_object()
        .expect("JSONオブジェクト")
        .clone()
    }

    #[test]
    fn count_limit_evicts_the_oldest_record_and_reports_the_missing_range() {
        let mut buffer = LogBuffer::default();
        buffer.begin_generation(4);
        for sequence in 1..=MAX_LOG_ENTRIES + 1 {
            buffer
                .record_at(4, &params(format!("log-{sequence}")), sequence as u64)
                .expect("ログを記録");
        }

        let snapshot = buffer.read(&LogQuery {
            generation: Some(4),
            after_sequence: Some(0),
            limit: Some(MAX_LOG_PAGE_SIZE),
            ..LogQuery::default()
        });
        assert_eq!(buffer.len(), MAX_LOG_ENTRIES);
        assert_eq!(
            snapshot.entries.first().map(|entry| entry.sequence),
            Some(2)
        );
        assert_eq!(
            snapshot.entries.last().map(|entry| entry.sequence),
            Some(1001)
        );
        assert!(snapshot.missing_before_cursor);
        assert_eq!(snapshot.retention[0].missing_count, 1);
        assert_eq!(snapshot.retention[0].last_observed_sequence, 1001);
    }

    #[test]
    fn byte_limit_evicts_old_entries_and_truncates_a_single_oversized_log() {
        let mut buffer = LogBuffer::default();
        buffer.begin_generation(8);
        let half = "x".repeat(MAX_LOG_BYTES / 2);
        buffer
            .record_at(8, &params(half.clone()), 10)
            .expect("1つ目を記録");
        buffer.record_at(8, &params(half), 20).expect("2つ目を記録");
        assert_eq!(buffer.len(), 1);
        assert!(buffer.retained_bytes() <= MAX_LOG_BYTES);
        assert_eq!(buffer.read(&LogQuery::default()).entries[0].sequence, 2);

        let oversized = "y".repeat(MAX_LOG_BYTES + 1);
        let outcome = buffer
            .record_at(8, &params(oversized), 30)
            .expect("大きなログを切り詰めて保持");
        assert_eq!(outcome.sequence, 3);
        assert!(outcome.truncated);
        let snapshot = buffer.read(&LogQuery {
            generation: Some(8),
            ..LogQuery::default()
        });
        assert!(buffer.retained_bytes() <= MAX_LOG_BYTES);
        assert_eq!(snapshot.retention[0].last_observed_sequence, 3);
        assert_eq!(snapshot.retention[0].missing_count, 2);
        assert_eq!(snapshot.entries.len(), 1);
        assert!(snapshot.entries[0].truncated);
    }

    #[test]
    fn generation_time_and_sequence_filters_return_retention_and_subscription_state() {
        let mut buffer = LogBuffer::default();
        buffer.begin_generation(10);
        buffer.set_subscription_status(10, LogSubscriptionStatus::Subscribed);
        buffer
            .record_at(10, &params("first"), 100)
            .expect("1つ目を記録");
        buffer
            .record_at(10, &params("second"), 200)
            .expect("2つ目を記録");
        buffer.begin_generation(11);
        buffer.set_subscription_status(11, LogSubscriptionStatus::IncompatibleAck);
        buffer
            .record_at(11, &params("other generation"), 150)
            .expect("別世代を記録");

        let snapshot = buffer.read(&LogQuery {
            generation: Some(10),
            from_time_ms: Some(150),
            to_time_ms: Some(250),
            after_sequence: Some(1),
            through_sequence: Some(2),
            limit: Some(1),
        });
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(snapshot.entries[0].message, "second");
        assert_eq!(snapshot.entries[0].sequence, 2);
        assert_eq!(snapshot.entries[0].generation, 10);
        assert_eq!(snapshot.retention[0].first_sequence, Some(1));
        assert_eq!(snapshot.retention[0].last_sequence, Some(2));
        assert_eq!(
            snapshot.retention[0].subscription,
            LogSubscriptionStatus::Subscribed
        );

        let incompatible = buffer.read(&LogQuery {
            generation: Some(11),
            ..LogQuery::default()
        });
        assert_eq!(
            incompatible.retention[0].subscription,
            LogSubscriptionStatus::IncompatibleAck
        );
    }

    #[test]
    fn malformed_fields_are_rejected_and_unbounded_fields_are_truncated() {
        let mut buffer = LogBuffer::default();
        buffer.begin_generation(15);
        assert_eq!(
            buffer.record_at(
                15,
                &serde_json::json!({"message":"x"})
                    .as_object()
                    .unwrap()
                    .clone(),
                1
            ),
            Err(LogBufferError::MissingLevel)
        );
        let oversized_category = serde_json::json!({
            "level": "info",
            "message": "short",
            "category": "x".repeat(MAX_LOG_BYTES)
        });
        let outcome = buffer
            .record_at(15, oversized_category.as_object().unwrap(), 2)
            .expect("大きなcategoryを切り詰めて保持");
        assert_eq!(outcome.sequence, 1);
        assert!(outcome.truncated);
        assert_eq!(buffer.len(), 1);
        assert!(buffer.retained_bytes() <= MAX_LOG_BYTES);
        let snapshot = buffer.read(&LogQuery {
            generation: Some(15),
            ..LogQuery::default()
        });
        assert!(snapshot.retention[0].has_gap);
    }

    #[test]
    fn fully_evicted_generation_keeps_its_missing_range() {
        let mut buffer = LogBuffer::default();
        buffer.begin_generation(20);
        buffer
            .record_at(20, &params("old generation"), 10)
            .expect("旧世代のログを記録");
        buffer.begin_generation(21);
        for sequence in 1..=MAX_LOG_ENTRIES {
            buffer
                .record_at(21, &params(format!("new-{sequence}")), 10 + sequence as u64)
                .expect("新世代のログを記録");
        }

        let snapshot = buffer.read(&LogQuery {
            generation: Some(20),
            ..LogQuery::default()
        });
        assert!(snapshot.entries.is_empty());
        assert_eq!(snapshot.retention.len(), 1);
        assert_eq!(snapshot.retention[0].first_sequence, None);
        assert_eq!(snapshot.retention[0].last_sequence, None);
        assert_eq!(snapshot.retention[0].last_observed_sequence, 1);
        assert_eq!(snapshot.retention[0].missing_count, 1);
        assert!(snapshot.retention[0].has_gap);
    }

    #[test]
    fn generation_metadata_has_a_fixed_upper_bound() {
        let mut buffer = LogBuffer::default();
        for generation in 1..=MAX_TRACKED_GENERATIONS as u64 + 2 {
            buffer.begin_generation(generation);
            buffer.end_generation(generation);
        }

        assert_eq!(buffer.generations.len(), MAX_TRACKED_GENERATIONS);
        assert_eq!(buffer.generation_order.len(), MAX_TRACKED_GENERATIONS);
        assert!(buffer
            .read(&LogQuery {
                generation: Some(1),
                ..LogQuery::default()
            })
            .retention
            .is_empty());
    }
}
