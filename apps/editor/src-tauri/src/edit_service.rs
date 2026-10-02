//! 画面とMCPの編集を、接続世代に結び付けた有界actorで実行する。

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde_json::Value;
use tauri::async_runtime::JoinHandle;
use tokio::sync::{mpsc, oneshot, watch, Mutex};

use crate::bridge_state::{BridgeFacade, BridgeLease};
use crate::error::BackendError;

mod history;

use history::{
    HistoryAction, HistoryActionRequest, HistoryDirection, HistoryMarker, HistoryRequest,
    HistoryState, QueuedEditResult,
};

const QUEUE_CAPACITY: usize = 64;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

type EditFuture =
    Pin<Box<dyn Future<Output = Result<QueuedEditResult, BackendError>> + Send + 'static>>;
type EditAction = Box<dyn FnOnce(BridgeLease) -> EditFuture + Send + 'static>;

enum QueuedAction {
    Edit(EditAction),
    History(HistoryActionRequest),
}

async fn join_after_grace(mut join: JoinHandle<()>) {
    if tokio::time::timeout(SHUTDOWN_GRACE, &mut join)
        .await
        .is_err()
    {
        join.abort();
        let _ = join.await;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum EditKind {
    Edit,
    Undo,
    Redo,
    RuntimeControl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum EditSource {
    Ui,
    Mcp,
}

struct QueueItem {
    sequence: u64,
    source: EditSource,
    kind: EditKind,
    lease: BridgeLease,
    action: QueuedAction,
    history_request: Option<HistoryRequest>,
    cancelled: oneshot::Receiver<()>,
    result: oneshot::Sender<Result<QueuedEditResult, BackendError>>,
}

struct Admission {
    accepting: bool,
    next_sequence: u64,
}

/// ticketのdropは開始前だけを取り消す。実行中の要求はサービス停止時に中止する。
#[allow(dead_code)]
pub(crate) struct EditTicket {
    result: oneshot::Receiver<Result<QueuedEditResult, BackendError>>,
    cancel: oneshot::Sender<()>,
}

#[allow(dead_code)]
impl EditTicket {
    pub(crate) async fn result(self) -> Result<Value, BackendError> {
        let EditTicket { result, cancel } = self;
        let outcome = result
            .await
            .unwrap_or(Err(BackendError::EditServiceStopping));
        drop(cancel);
        outcome.map(|result| result.value)
    }
}

/// UIとMCPで共有する編集受付。既存UIコマンドの切り替えは後続タスクで行う。
#[allow(dead_code)]
pub(crate) struct EditService {
    bridge: BridgeFacade,
    sender: mpsc::Sender<QueueItem>,
    admission: StdMutex<Admission>,
    history: Arc<StdMutex<HistoryState>>,
    shutdown_tx: watch::Sender<bool>,
    join: Mutex<Option<JoinHandle<()>>>,
}

#[allow(dead_code)]
impl EditService {
    pub(crate) fn new(bridge: BridgeFacade) -> Self {
        Self::with_capacity(bridge, QUEUE_CAPACITY)
    }

    fn with_capacity(bridge: BridgeFacade, capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let history = Arc::new(StdMutex::new(HistoryState::default()));
        let join = tauri::async_runtime::spawn(run_actor(
            bridge.clone(),
            receiver,
            bridge.subscribe(),
            shutdown_rx,
            Arc::clone(&history),
        ));
        Self {
            bridge,
            sender,
            admission: StdMutex::new(Admission {
                accepting: true,
                next_sequence: 0,
            }),
            history,
            shutdown_tx,
            join: Mutex::new(Some(join)),
        }
    }

    pub(crate) fn enqueue_from_ui<F, Fut>(
        &self,
        kind: EditKind,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, BackendError>> + Send + 'static,
    {
        self.enqueue(EditSource::Ui, kind, action)
    }

    pub(crate) fn enqueue_from_mcp<F, Fut>(
        &self,
        kind: EditKind,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, BackendError>> + Send + 'static,
    {
        self.enqueue(EditSource::Mcp, kind, action)
    }

    /// 記録候補をUIから列へ入れ、捕捉改訂が古ければBridge操作の前に拒否する。
    pub(crate) fn enqueue_recorded_from_ui<F, Fut>(
        &self,
        kind: EditKind,
        request: HistoryRequest,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<QueuedEditResult, BackendError>> + Send + 'static,
    {
        self.enqueue_recorded(EditSource::Ui, kind, request, action)
    }

    /// MCPの旧値照会と書き込みを同じ列の1操作にする。
    pub(crate) fn enqueue_recorded_from_mcp<F, Fut>(
        &self,
        kind: EditKind,
        request: HistoryRequest,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<QueuedEditResult, BackendError>> + Send + 'static,
    {
        self.enqueue_recorded(EditSource::Mcp, kind, request, action)
    }

    pub(crate) async fn submit_from_ui<F, Fut>(
        &self,
        kind: EditKind,
        action: F,
    ) -> Result<Value, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, BackendError>> + Send + 'static,
    {
        self.enqueue_from_ui(kind, action)?.result().await
    }

    pub(crate) async fn submit_from_mcp<F, Fut>(
        &self,
        kind: EditKind,
        action: F,
    ) -> Result<Value, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, BackendError>> + Send + 'static,
    {
        self.enqueue_from_mcp(kind, action)?.result().await
    }

    pub(crate) fn enqueue_undo_from_ui(
        &self,
        expected_head_id: Option<u64>,
        expected_revision: u64,
    ) -> Result<EditTicket, BackendError> {
        self.enqueue_history_action(
            EditSource::Ui,
            HistoryDirection::Undo,
            expected_head_id,
            expected_revision,
        )
    }

    pub(crate) fn enqueue_redo_from_ui(
        &self,
        expected_head_id: Option<u64>,
        expected_revision: u64,
    ) -> Result<EditTicket, BackendError> {
        self.enqueue_history_action(
            EditSource::Ui,
            HistoryDirection::Redo,
            expected_head_id,
            expected_revision,
        )
    }

    fn enqueue_history_action(
        &self,
        source: EditSource,
        direction: HistoryDirection,
        expected_head_id: Option<u64>,
        expected_revision: u64,
    ) -> Result<EditTicket, BackendError> {
        let mut admission = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !admission.accepting {
            return Err(BackendError::EditServiceStopping);
        }

        let lease = match self.bridge.pin() {
            Ok(lease) => lease,
            Err(BackendError::NotConnected) => {
                let mut history = self
                    .history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                history.synchronize(self.bridge.current_generation());
                return Ok(completed_noop_ticket());
            }
            Err(error) => return Err(error),
        };
        let request = HistoryActionRequest {
            direction,
            expected_head_id,
            expected_revision,
        };
        let can_run = {
            let mut history = self
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            history.synchronize(Some(lease.generation));
            history.prepare_action(request).is_some()
        };
        if !can_run {
            return Ok(completed_noop_ticket());
        }

        let (cancel, cancelled) = oneshot::channel();
        let (result, result_rx) = oneshot::channel();
        let item = QueueItem {
            sequence: admission.next_sequence,
            source,
            kind: match direction {
                HistoryDirection::Undo => EditKind::Undo,
                HistoryDirection::Redo => EditKind::Redo,
            },
            lease,
            action: QueuedAction::History(request),
            history_request: None,
            cancelled,
            result,
        };
        match self.sender.try_send(item) {
            Ok(()) => {
                admission.next_sequence = admission.next_sequence.wrapping_add(1);
                Ok(EditTicket {
                    result: result_rx,
                    cancel,
                })
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(BackendError::EditQueueFull),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(BackendError::EditServiceStopping),
        }
    }

    fn enqueue<F, Fut>(
        &self,
        source: EditSource,
        kind: EditKind,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, BackendError>> + Send + 'static,
    {
        self.enqueue_inner(
            source,
            kind,
            None,
            Box::new(|lease| {
                Box::pin(async move { action(lease).await.map(QueuedEditResult::plain) })
            }),
        )
    }

    fn enqueue_recorded<F, Fut>(
        &self,
        source: EditSource,
        kind: EditKind,
        request: HistoryRequest,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<QueuedEditResult, BackendError>> + Send + 'static,
    {
        self.enqueue_inner(
            source,
            kind,
            Some(request),
            Box::new(|lease| Box::pin(action(lease))),
        )
    }

    fn enqueue_inner(
        &self,
        source: EditSource,
        kind: EditKind,
        history_request: Option<HistoryRequest>,
        action: EditAction,
    ) -> Result<EditTicket, BackendError> {
        let mut admission = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !admission.accepting {
            return Err(BackendError::EditServiceStopping);
        }
        let lease = self.bridge.pin()?;
        let (cancel, cancelled) = oneshot::channel();
        let (result, result_rx) = oneshot::channel();
        let item = QueueItem {
            sequence: admission.next_sequence,
            source,
            kind,
            lease,
            action: QueuedAction::Edit(action),
            history_request,
            cancelled,
            result,
        };
        match self.sender.try_send(item) {
            Ok(()) => {
                admission.next_sequence = admission.next_sequence.wrapping_add(1);
                Ok(EditTicket {
                    result: result_rx,
                    cancel,
                })
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(BackendError::EditQueueFull),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(BackendError::EditServiceStopping),
        }
    }

    pub(crate) fn history_snapshot(&self) -> (Option<u64>, u64, Vec<HistoryMarker>) {
        self.bridge.with_current_generation(|generation| {
            let mut history = self
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            history.synchronize(generation);
            history.snapshot()
        })
    }

    pub(crate) fn history_cursor(&self, direction: HistoryDirection) -> (Option<u64>, u64) {
        self.bridge.with_current_generation(|generation| {
            let mut history = self
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            history.synchronize(generation);
            history.history_cursor(direction)
        })
    }

    /// 画面スナップショットに付ける、現在接続の適用改訂を返す。
    #[allow(dead_code)]
    pub(crate) fn applied_history_revision(&self) -> (Option<u64>, u64) {
        self.bridge.with_current_generation(|generation| {
            let mut history = self
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            history.synchronize(generation);
            (generation, history.applied_revision())
        })
    }

    /// 受付を閉じ、2秒以内に終わらないactorを中止してjoinする。
    pub(crate) async fn shutdown(&self) {
        {
            let mut admission = self
                .admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            admission.accepting = false;
            self.shutdown_tx.send_replace(true);
        }

        let mut join_guard = self.join.lock().await;
        if let Some(join) = join_guard.take() {
            join_after_grace(join).await;
        }
        self.history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear_for_shutdown();
    }

    #[cfg(test)]
    pub(crate) fn history_lock_available(&self) -> bool {
        self.history.try_lock().is_ok()
    }
}

fn completed_noop_ticket() -> EditTicket {
    let (result_tx, result) = oneshot::channel();
    let (cancel, cancelled) = oneshot::channel();
    let _ = result_tx.send(Ok(QueuedEditResult::plain(Value::Null)));
    drop(cancelled);
    EditTicket { result, cancel }
}

async fn run_history_action(
    bridge: BridgeFacade,
    lease: BridgeLease,
    action: HistoryAction,
) -> Result<QueuedEditResult, BackendError> {
    let mut params = serde_json::Map::new();
    let (method, result_kind) = match (&action.record, action.direction) {
        (history::HistoryRecord::Create { created_id, .. }, HistoryDirection::Undo)
        | (history::HistoryRecord::Duplicate { created_id, .. }, HistoryDirection::Undo) => {
            params.insert("objectId".to_owned(), Value::String(created_id.clone()));
            ("scene.deleteObject", HistoryResultKind::Delete)
        }
        (
            history::HistoryRecord::Create {
                parent_id, kind, ..
            },
            HistoryDirection::Redo,
        ) => {
            insert_optional_string(&mut params, "parentId", parent_id.as_deref());
            insert_optional_string(&mut params, "kind", kind.as_deref());
            ("scene.createObject", HistoryResultKind::Create)
        }
        (
            history::HistoryRecord::Duplicate {
                source_object_id,
                parent_id,
                ..
            },
            HistoryDirection::Redo,
        ) => {
            params.insert(
                "objectId".to_owned(),
                Value::String(source_object_id.clone()),
            );
            insert_optional_string(&mut params, "newParentId", parent_id.as_deref());
            ("scene.duplicateObject", HistoryResultKind::Duplicate)
        }
        (
            history::HistoryRecord::Reparent {
                object_id,
                old_parent_id,
                ..
            },
            HistoryDirection::Undo,
        ) => {
            params.insert("objectId".to_owned(), Value::String(object_id.clone()));
            insert_optional_string(&mut params, "newParentId", old_parent_id.as_deref());
            ("scene.reparentObject", HistoryResultKind::Reparent)
        }
        (
            history::HistoryRecord::Reparent {
                object_id,
                new_parent_id,
                ..
            },
            HistoryDirection::Redo,
        ) => {
            params.insert("objectId".to_owned(), Value::String(object_id.clone()));
            insert_optional_string(&mut params, "newParentId", new_parent_id.as_deref());
            ("scene.reparentObject", HistoryResultKind::Reparent)
        }
        (
            history::HistoryRecord::SetProperty {
                object_id,
                property,
                old_value,
                ..
            },
            HistoryDirection::Undo,
        ) => {
            params.insert("objectId".to_owned(), Value::String(object_id.clone()));
            params.insert("property".to_owned(), Value::String(property.clone()));
            params.insert("value".to_owned(), old_value.clone());
            ("object.setProperty", HistoryResultKind::Property)
        }
        (
            history::HistoryRecord::SetProperty {
                object_id,
                property,
                new_value,
                ..
            },
            HistoryDirection::Redo,
        ) => {
            params.insert("objectId".to_owned(), Value::String(object_id.clone()));
            params.insert("property".to_owned(), Value::String(property.clone()));
            params.insert("value".to_owned(), new_value.clone());
            ("object.setProperty", HistoryResultKind::Property)
        }
    };

    let value = bridge.send_with_lease(&lease, method, Some(params)).await?;
    validate_history_result(method, result_kind, &value)?;
    Ok(QueuedEditResult::plain(value))
}

#[derive(Clone, Copy)]
enum HistoryResultKind {
    Create,
    Duplicate,
    Delete,
    Reparent,
    Property,
}

fn insert_optional_string(
    params: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        params.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn validate_history_result(
    method: &str,
    kind: HistoryResultKind,
    value: &Value,
) -> Result<(), BackendError> {
    let result = match kind {
        HistoryResultKind::Create => norves_bridge_editor_client::parse_create_object_result(value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        HistoryResultKind::Duplicate => {
            norves_bridge_editor_client::parse_duplicate_object_result(value)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        HistoryResultKind::Delete => norves_bridge_editor_client::parse_delete_object_result(value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        HistoryResultKind::Reparent => {
            norves_bridge_editor_client::parse_reparent_object_result(value)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        HistoryResultKind::Property => {
            norves_bridge_editor_client::parse_set_property_result(value)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    };
    result.map_err(|error| BackendError::Request {
        message: format!("{method} の応答形式が不正です: {error}"),
    })
}

async fn run_actor(
    bridge: BridgeFacade,
    mut receiver: mpsc::Receiver<QueueItem>,
    mut sessions: watch::Receiver<Option<BridgeLease>>,
    mut shutdown: watch::Receiver<bool>,
    history: Arc<StdMutex<HistoryState>>,
) {
    let generation = bridge.current_generation();
    history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .synchronize(generation);

    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow_and_update() {
                    break;
                }
            }
            changed = sessions.changed() => {
                if changed.is_err() {
                    break;
                }
                let generation = bridge.current_generation();
                let mut state = history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.synchronize(generation);
            }
            item = receiver.recv() => {
                let Some(item) = item else { break; };
                if process_item(&bridge, &mut sessions, &mut shutdown, &history, item).await {
                    break;
                }
            }
        }
    }

    receiver.close();
    while let Ok(item) = receiver.try_recv() {
        let _ = item.result.send(Err(BackendError::EditServiceStopping));
    }
}

async fn process_item(
    bridge: &BridgeFacade,
    sessions: &mut watch::Receiver<Option<BridgeLease>>,
    shutdown: &mut watch::Receiver<bool>,
    history: &Arc<StdMutex<HistoryState>>,
    mut item: QueueItem,
) -> bool {
    if matches!(
        item.cancelled.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    ) {
        let _ = item.result.send(Err(BackendError::EditCancelled));
        return false;
    }
    if !bridge.is_current(item.lease.generation) {
        let outcome = if matches!(item.action, QueuedAction::History(_)) {
            Ok(QueuedEditResult::plain(Value::Null))
        } else {
            Err(BackendError::NotConnected)
        };
        let _ = item.result.send(outcome);
        return false;
    }

    let mut prepared_history = None;
    let prepared_action = {
        let mut state = history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.synchronize(bridge.current_generation());
        if let Some(request) = item.history_request.take() {
            match state.prepare(request, item.source, item.lease.generation) {
                Ok(prepared) => prepared_history = Some(prepared),
                Err(error) => {
                    let _ = item.result.send(Err(error));
                    return false;
                }
            }
        }
        if let QueuedAction::History(request) = &item.action {
            let Some(action) = state.prepare_action(*request) else {
                let _ = item.result.send(Ok(QueuedEditResult::plain(Value::Null)));
                return false;
            };
            Some(action)
        } else {
            None
        }
    };

    let action = match item.action {
        QueuedAction::Edit(action) => action(item.lease.clone()),
        QueuedAction::History(_) => {
            let Some(plan) = prepared_action.clone() else {
                let _ = item.result.send(Ok(QueuedEditResult::plain(Value::Null)));
                return false;
            };
            Box::pin(run_history_action(bridge.clone(), item.lease.clone(), plan))
        }
    };
    tokio::pin!(action);
    let (outcome, stopping) = loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow_and_update() {
                    break (Err(BackendError::EditServiceStopping), true);
                }
            }
            changed = sessions.changed() => {
                if changed.is_err() {
                    break (Err(BackendError::NotConnected), false);
                }
                let session_generation = sessions
                    .borrow_and_update()
                    .as_ref()
                    .map(|lease| lease.generation);
                let current = bridge.current_generation();
                let mut state = history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.synchronize(current);
                if session_generation != Some(item.lease.generation)
                    || current != Some(item.lease.generation)
                {
                    break (Err(BackendError::NotConnected), false);
                }
            }
            result = &mut action => {
                let outcome = match result {
                    Ok(value) => bridge.with_current_generation(|current| {
                        let mut state = history
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        state.synchronize(current);
                        if current != Some(item.lease.generation) {
                            return Err(BackendError::NotConnected);
                        }
                        if let Some(plan) = prepared_action.clone() {
                            match state.finish_action_success(
                                plan,
                                item.source,
                                item.kind,
                                item.sequence,
                                item.lease.generation,
                                value.value.clone(),
                            ) {
                                Ok(true) => Ok(value),
                                Ok(false) => Ok(QueuedEditResult::plain(Value::Null)),
                                Err(error) => Err(error),
                            }
                        } else {
                            if let Some(prepared) = prepared_history.take() {
                                state.record_success(
                                    prepared,
                                    item.source,
                                    item.kind,
                                    item.sequence,
                                    item.lease.generation,
                                    value.clone(),
                                );
                            } else if item.kind == EditKind::Edit
                                && value.value.get("accepted").and_then(Value::as_bool)
                                    == Some(true)
                            {
                                state.record_untracked_success(
                                    item.source,
                                    item.kind,
                                    item.sequence,
                                    item.lease.generation,
                                );
                            }
                            Ok(value)
                        }
                    }),
                    Err(error) => {
                        bridge.with_current_generation(|current| {
                            let mut state = history
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            state.synchronize(current);
                            if current == Some(item.lease.generation) {
                                if let Some(plan) = prepared_action.as_ref() {
                                    state.finish_action_failure(plan, &error);
                                } else if matches!(
                                    error,
                                    BackendError::Engine { ref code, .. }
                                        if code == "METHOD_NOT_SUPPORTED"
                                ) {
                                    state.mark_edit_unsupported();
                                }
                            }
                        });
                        Err(error)
                    }
                };
                break (outcome, false);
            }
        }
    };
    let _ = item.result.send(outcome);
    stopping
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge_state::{test_edit_facade, BridgeSessionTestControl};
    use norves_bridge_core::{
        decode_typed, encode_envelope, BridgeError, CorrelationId, Envelope, ErrorCode,
        ResponsePayload, ValidatedEnvelope, VersionString,
    };
    use norves_bridge_editor_client::{
        loopback_pair, DispatchHandle, Dispatcher, LoopbackTransport, Transport,
    };
    use std::sync::Mutex as TestMutex;
    use tokio::sync::{oneshot, Notify};

    fn test_service(capacity: usize) -> (EditService, BridgeSessionTestControl, DispatchHandle) {
        let (transport, _peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(transport);
        let (bridge, control) = test_edit_facade(1, handle.clone());
        (
            EditService::with_capacity(bridge, capacity),
            control,
            handle,
        )
    }

    fn test_service_with_peer(
        capacity: usize,
    ) -> (
        EditService,
        BridgeSessionTestControl,
        DispatchHandle,
        LoopbackTransport,
    ) {
        let (transport, peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(transport);
        let (bridge, control) = test_edit_facade(1, handle.clone());
        (
            EditService::with_capacity(bridge, capacity),
            control,
            handle,
            peer,
        )
    }

    fn response_frame(id: CorrelationId, payload: ResponsePayload) -> String {
        let envelope: Envelope = ValidatedEnvelope::Response {
            version: VersionString::try_from("0.2".to_owned()).expect("version is valid"),
            id,
            payload,
            session_id: None,
            seq: None,
        }
        .into();
        encode_envelope(&envelope).expect("response envelope encodes")
    }

    async fn next_request(
        peer: &mut LoopbackTransport,
    ) -> (
        CorrelationId,
        String,
        Option<serde_json::Map<String, Value>>,
    ) {
        let frame = tokio::time::timeout(Duration::from_secs(2), peer.recv())
            .await
            .expect("timed out waiting for history request")
            .expect("peer receive succeeds")
            .expect("history request was sent");
        match decode_typed(&frame).expect("history request decodes") {
            ValidatedEnvelope::Request {
                id, method, params, ..
            } => (id, method.as_str().to_owned(), params),
            other => panic!("expected a request, received {other:?}"),
        }
    }

    async fn respond(peer: &mut LoopbackTransport, id: CorrelationId, result: Value) {
        peer.send(response_frame(id, ResponsePayload::Result(result)))
            .await
            .expect("history result sends");
    }

    fn seed_history(
        service: &EditService,
        direction: HistoryDirection,
        sequence: u64,
        record: history::HistoryRecord,
    ) {
        let mut state = service
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.synchronize(Some(1));
        match direction {
            HistoryDirection::Undo => state.seed_undo_record(sequence, record),
            HistoryDirection::Redo => state.seed_redo_record(sequence, record),
        }
    }

    fn queue_history_action(service: &EditService, direction: HistoryDirection) -> EditTicket {
        let (head_id, revision) = service.history_cursor(direction);
        match direction {
            HistoryDirection::Undo => service.enqueue_undo_from_ui(head_id, revision),
            HistoryDirection::Redo => service.enqueue_redo_from_ui(head_id, revision),
        }
        .expect("history action is accepted")
    }

    fn success(value: Value) -> Result<Value, BackendError> {
        Ok(value)
    }

    #[tokio::test]
    async fn ui_and_mcp_share_one_acceptance_order_and_undo_blocks_later_edits() {
        let (service, _control, handle) = test_service(4);
        let order = Arc::new(TestMutex::new(Vec::new()));
        let (undo_started, undo_started_rx) = oneshot::channel();
        let (finish_undo, finish_undo_rx) = oneshot::channel();

        let first_order = Arc::clone(&order);
        let undo = service
            .enqueue_from_ui(EditKind::Undo, move |_| async move {
                first_order.lock().unwrap().push("undo-start");
                let _ = undo_started.send(());
                let _ = finish_undo_rx.await;
                first_order.lock().unwrap().push("undo-end");
                success(serde_json::json!("undo"))
            })
            .expect("UI undo is accepted");
        undo_started_rx.await.expect("undo started");

        let next_order = Arc::clone(&order);
        let edit = service
            .enqueue_from_mcp(EditKind::Edit, move |_| async move {
                next_order.lock().unwrap().push("mcp-edit");
                success(serde_json::json!("edit"))
            })
            .expect("MCP edit is queued");

        assert_eq!(*order.lock().unwrap(), ["undo-start"]);
        finish_undo.send(()).expect("undo gate is open");
        assert_eq!(undo.result().await.expect("undo succeeds"), "undo");
        assert_eq!(edit.result().await.expect("edit succeeds"), "edit");
        assert_eq!(
            *order.lock().unwrap(),
            ["undo-start", "undo-end", "mcp-edit"]
        );

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn full_queue_is_rejected_and_dropped_ticket_cancels_pending_edit() {
        let (service, _control, handle) = test_service(2);
        let started = Arc::new(Notify::new());
        let order = Arc::new(TestMutex::new(Vec::new()));
        let first_started = Arc::clone(&started);
        let first_order = Arc::clone(&order);
        let (finish, finish_rx) = oneshot::channel();
        let first = service
            .enqueue_from_ui(EditKind::Undo, move |_| async move {
                first_order.lock().unwrap().push("undo");
                first_started.notify_one();
                let _ = finish_rx.await;
                success(Value::Null)
            })
            .expect("first request is accepted");
        started.notified().await;

        let cancelled_order = Arc::clone(&order);
        let cancelled = service
            .enqueue_from_mcp(EditKind::Edit, move |_| async move {
                cancelled_order.lock().unwrap().push("cancelled");
                success(Value::Null)
            })
            .expect("one request fits in the waiting queue");
        drop(cancelled);

        let sentinel_order = Arc::clone(&order);
        let sentinel = service
            .enqueue_from_mcp(EditKind::Edit, move |_| async move {
                sentinel_order.lock().unwrap().push("sentinel");
                success(Value::Null)
            })
            .expect("sentinel fits behind the cancelled request");
        assert!(matches!(
            service.enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) }),
            Err(BackendError::EditQueueFull)
        ));

        finish.send(()).expect("undo gate is open");
        first.result().await.expect("undo succeeds");
        sentinel.result().await.expect("sentinel succeeds");
        assert_eq!(*order.lock().unwrap(), ["undo", "sentinel"]);

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn generation_change_clears_history_and_discards_old_response() {
        let (service, control, handle) = test_service(4);
        let recorded = service
            .enqueue_from_mcp(EditKind::Edit, |_| async {
                success(serde_json::json!({"accepted": true}))
            })
            .expect("generation one edit is accepted");
        recorded
            .result()
            .await
            .expect("generation one edit succeeds");
        let (generation, previous_revision, entries) = service.history_snapshot();
        assert_eq!(generation, Some(1));
        assert_eq!(entries.len(), 1);

        let (started, started_rx) = oneshot::channel();
        let (finish, finish_rx) = oneshot::channel();
        let response_control = control.clone();
        let (new_transport, _peer) = loopback_pair(8);
        let new_handle = Dispatcher::spawn(new_transport);
        let response_handle = new_handle.clone();
        let ticket = service
            .enqueue_from_mcp(EditKind::Edit, move |_| async move {
                let _ = started.send(());
                let _ = finish_rx.await;
                response_control.set_generation(2, response_handle);
                success(serde_json::json!("stale response"))
            })
            .expect("request is accepted");
        started_rx.await.expect("bridge operation started");
        let pending_action_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pending_action_probe = Arc::clone(&pending_action_ran);
        let pending = service
            .enqueue_from_ui(EditKind::Edit, move |_| async move {
                pending_action_probe.store(true, std::sync::atomic::Ordering::Release);
                success(Value::Null)
            })
            .expect("old-generation request is queued");

        finish.send(()).expect("old response is released");
        assert!(matches!(
            ticket.result().await,
            Err(BackendError::NotConnected)
        ));
        assert!(matches!(
            pending.result().await,
            Err(BackendError::NotConnected)
        ));
        assert!(!pending_action_ran.load(std::sync::atomic::Ordering::Acquire));
        let (generation, revision, entries) = service.history_snapshot();
        assert_eq!(generation, Some(2));
        assert!(entries.is_empty());
        assert!(revision > previous_revision);

        service.shutdown().await;
        handle.shutdown().await;
        new_handle.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_cancels_in_flight_work_and_rejects_waiting_requests() {
        let (service, _control, handle) = test_service(2);
        let (started, started_rx) = oneshot::channel();
        let (drop_notice, drop_notice_rx) = oneshot::channel();
        let ticket = service
            .enqueue_from_ui(EditKind::Edit, move |_| async move {
                let _notice = DropNotice(Some(drop_notice));
                let _ = started.send(());
                std::future::pending::<Result<Value, BackendError>>().await
            })
            .expect("request is accepted");
        started_rx.await.expect("operation started");
        let pending = service
            .enqueue_from_mcp(EditKind::Edit, |_| async { success(Value::Null) })
            .expect("pending request is accepted before shutdown");

        let began = tokio::time::Instant::now();
        tokio::time::timeout(Duration::from_secs(1), service.shutdown())
            .await
            .expect("cooperative shutdown finishes before the abort grace");
        let elapsed = began.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "shutdown took {elapsed:?}"
        );
        tokio::time::timeout(Duration::from_secs(1), drop_notice_rx)
            .await
            .expect("operation was cancelled")
            .expect("drop was observed");
        assert!(matches!(
            ticket.result().await,
            Err(BackendError::EditServiceStopping)
        ));
        assert!(matches!(
            pending.result().await,
            Err(BackendError::EditServiceStopping)
        ));
        assert!(matches!(
            service.enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) }),
            Err(BackendError::EditServiceStopping)
        ));

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn unresponsive_owned_task_is_aborted_and_joined_after_two_second_grace() {
        let (started, started_rx) = oneshot::channel();
        let (drop_notice, drop_notice_rx) = oneshot::channel();
        let join = tauri::async_runtime::spawn(async move {
            let _notice = DropNotice(Some(drop_notice));
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        started_rx.await.expect("owned task started");
        let began = tokio::time::Instant::now();
        join_after_grace(join).await;
        let elapsed = began.elapsed();
        assert!(
            elapsed >= SHUTDOWN_GRACE,
            "shutdown ended after {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(4),
            "shutdown took {elapsed:?}"
        );
        tokio::time::timeout(Duration::from_secs(1), drop_notice_rx)
            .await
            .expect("unresponsive task was aborted")
            .expect("task drop was observed");
    }

    #[tokio::test]
    async fn history_lock_remains_available_during_bridge_operation() {
        let (service, _control, handle) = test_service(2);
        let (started, started_rx) = oneshot::channel();
        let (finish, finish_rx) = oneshot::channel();
        let ticket = service
            .enqueue_from_ui(EditKind::Edit, move |_| async move {
                let _ = started.send(());
                let _ = finish_rx.await;
                success(Value::Null)
            })
            .expect("request is accepted");
        started_rx.await.expect("operation started");

        assert!(service.history_lock_available());
        finish.send(()).expect("operation gate is open");
        ticket.result().await.expect("operation succeeds");
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn stale_ui_capture_uses_latest_service_value() {
        let (service, _control, handle) = test_service(4);
        let mcp = service
            .enqueue_recorded_from_mcp(
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "object".to_owned(),
                    property: "color".to_owned(),
                    requested_value: serde_json::json!("B"),
                    old_value: history::PriorCapture::InAction,
                },
                |_| async {
                    Ok(history::QueuedEditResult::with_prior(
                        serde_json::json!({"accepted": true, "appliedValue": "B"}),
                        history::HistoryPrior::Property(serde_json::json!("A")),
                    ))
                },
            )
            .expect("MCP edit is queued");
        mcp.result().await.expect("MCP edit is accepted");
        assert_eq!(service.applied_history_revision(), (Some(1), 1));

        let ui = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "object".to_owned(),
                    property: "color".to_owned(),
                    requested_value: serde_json::json!("C"),
                    old_value: history::PriorCapture::Ui(history::HistoryCapture {
                        generation: 1,
                        revision: 0,
                        value: Some(serde_json::json!("A")),
                    }),
                },
                |_| async {
                    success(serde_json::json!({
                        "accepted": true,
                        "appliedValue": "C"
                    }))
                    .map(history::QueuedEditResult::plain)
                },
            )
            .expect("stale UI edit with a correction is queued");
        ui.result().await.expect("UI edit is accepted");

        let records = service
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .undo_records();
        assert_eq!(
            records.last(),
            Some(&history::HistoryRecord::SetProperty {
                object_id: "object".to_owned(),
                property: "color".to_owned(),
                old_value: serde_json::json!("B"),
                new_value: serde_json::json!("C"),
            })
        );

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn stale_ui_capture_uses_latest_service_parent() {
        let (service, _control, handle) = test_service(4);
        let mcp = service
            .enqueue_recorded_from_mcp(
                EditKind::Edit,
                HistoryRequest::ReparentObject {
                    object_id: "child".to_owned(),
                    new_parent_id: Some("parent-b".to_owned()),
                    old_parent: history::PriorCapture::InAction,
                },
                |_| async {
                    Ok(history::QueuedEditResult::with_prior(
                        serde_json::json!({"accepted": true}),
                        history::HistoryPrior::Parent(Some("parent-a".to_owned())),
                    ))
                },
            )
            .expect("MCP reparent is queued");
        mcp.result().await.expect("MCP reparent is accepted");

        let ui = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::ReparentObject {
                    object_id: "child".to_owned(),
                    new_parent_id: Some("parent-c".to_owned()),
                    old_parent: history::PriorCapture::Ui(history::HistoryCapture {
                        generation: 1,
                        revision: 0,
                        value: Some(Some("parent-a".to_owned())),
                    }),
                },
                |_| async {
                    success(serde_json::json!({"accepted": true}))
                        .map(history::QueuedEditResult::plain)
                },
            )
            .expect("stale UI reparent with a correction is queued");
        ui.result().await.expect("UI reparent is accepted");

        let records = service
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .undo_records();
        assert_eq!(
            records.last(),
            Some(&history::HistoryRecord::Reparent {
                object_id: "child".to_owned(),
                old_parent_id: Some("parent-b".to_owned()),
                new_parent_id: Some("parent-c".to_owned()),
            })
        );

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn live_updates_do_not_replace_the_ui_captured_old_value() {
        let (service, _control, handle) = test_service(4);
        let captured_old_value = serde_json::json!("A");
        let later_live_value = serde_json::json!("Live");
        assert_ne!(captured_old_value, later_live_value);
        let ui = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "object".to_owned(),
                    property: "color".to_owned(),
                    requested_value: serde_json::json!("C"),
                    old_value: history::PriorCapture::Ui(history::HistoryCapture {
                        generation: 1,
                        revision: 0,
                        value: Some(captured_old_value),
                    }),
                },
                |_| async {
                    success(serde_json::json!({
                        "accepted": true,
                        "appliedValue": "C"
                    }))
                    .map(history::QueuedEditResult::plain)
                },
            )
            .expect("UI edit is queued");
        ui.result().await.expect("UI edit is accepted");

        let records = service
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .undo_records();
        assert_eq!(
            records,
            [history::HistoryRecord::SetProperty {
                object_id: "object".to_owned(),
                property: "color".to_owned(),
                old_value: serde_json::json!("A"),
                new_value: serde_json::json!("C"),
            }]
        );

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn unlost_stale_capture_uses_its_ui_value_after_an_unrelated_edit() {
        let (service, _control, handle) = test_service(4);
        let created = service
            .enqueue_recorded_from_mcp(
                EditKind::Edit,
                HistoryRequest::CreateObject {
                    parent_id: None,
                    kind: None,
                },
                |_| async {
                    Ok(history::QueuedEditResult::plain(serde_json::json!({
                        "accepted": true,
                        "newId": "created"
                    })))
                },
            )
            .expect("create is queued");
        created.result().await.expect("create is accepted");
        let edit_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let edit_probe = Arc::clone(&edit_ran);
        let stale = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "other-object".to_owned(),
                    property: "color".to_owned(),
                    requested_value: serde_json::json!("C"),
                    old_value: history::PriorCapture::Ui(history::HistoryCapture {
                        generation: 1,
                        revision: 0,
                        value: Some(serde_json::json!("A")),
                    }),
                },
                move |_| async move {
                    edit_probe.store(true, std::sync::atomic::Ordering::Release);
                    Ok(history::QueuedEditResult::plain(serde_json::json!({
                        "accepted": true,
                        "appliedValue": "C"
                    })))
                },
            )
            .expect("stale edit enters the queue");
        stale.result().await.expect("unlost capture is accepted");
        assert!(edit_ran.load(std::sync::atomic::Ordering::Acquire));
        let records = service
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .undo_records();
        assert_eq!(
            records,
            [
                history::HistoryRecord::Create {
                    created_id: "created".to_owned(),
                    parent_id: None,
                    kind: None,
                },
                history::HistoryRecord::SetProperty {
                    object_id: "other-object".to_owned(),
                    property: "color".to_owned(),
                    old_value: serde_json::json!("A"),
                    new_value: serde_json::json!("C"),
                },
            ]
        );

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn stale_capture_without_old_value_writes_without_recording() {
        let (service, _control, handle) = test_service(4);
        let created = service
            .enqueue_recorded_from_mcp(
                EditKind::Edit,
                HistoryRequest::CreateObject {
                    parent_id: None,
                    kind: None,
                },
                |_| async {
                    Ok(history::QueuedEditResult::plain(serde_json::json!({
                        "accepted": true,
                        "newId": "created"
                    })))
                },
            )
            .expect("create is queued");
        created.result().await.expect("create is accepted");
        let deleted = service
            .enqueue_recorded_from_mcp(
                EditKind::Edit,
                HistoryRequest::DeleteObject {
                    object_id: "created".to_owned(),
                },
                |_| async {
                    Ok(history::QueuedEditResult::plain(serde_json::json!({
                        "accepted": true
                    })))
                },
            )
            .expect("delete is queued");
        deleted.result().await.expect("delete is accepted");

        let edit_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let edit_probe = Arc::clone(&edit_ran);
        let stale = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "other-object".to_owned(),
                    property: "color".to_owned(),
                    requested_value: serde_json::json!("C"),
                    old_value: history::PriorCapture::Ui(history::HistoryCapture {
                        generation: 1,
                        revision: 0,
                        value: None,
                    }),
                },
                move |_| async move {
                    edit_probe.store(true, std::sync::atomic::Ordering::Release);
                    Ok(history::QueuedEditResult::plain(serde_json::json!({
                        "accepted": true,
                        "appliedValue": "C"
                    })))
                },
            )
            .expect("stale edit enters the queue");
        stale
            .result()
            .await
            .expect("missing old value does not block the edit");
        assert!(edit_ran.load(std::sync::atomic::Ordering::Acquire));
        let records = service
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .undo_records();
        assert!(records.is_empty());

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn lost_stale_correction_requests_reload_before_running_the_edit() {
        let (service, _control, handle) = test_service(4);
        for index in 0..513 {
            let object_id = format!("object-{index}");
            let edit = service
                .enqueue_recorded_from_mcp(
                    EditKind::Edit,
                    HistoryRequest::SetProperty {
                        object_id,
                        property: "value".to_owned(),
                        requested_value: serde_json::json!(index),
                        old_value: history::PriorCapture::InAction,
                    },
                    move |_| async move {
                        Ok(history::QueuedEditResult::with_prior(
                            serde_json::json!({
                                "accepted": true,
                                "appliedValue": index
                            }),
                            history::HistoryPrior::Property(serde_json::json!("old")),
                        ))
                    },
                )
                .expect("MCP edit is queued");
            edit.result().await.expect("MCP edit is accepted");
        }

        let edit_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let edit_probe = Arc::clone(&edit_ran);
        let stale = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "object-0".to_owned(),
                    property: "value".to_owned(),
                    requested_value: serde_json::json!("new"),
                    old_value: history::PriorCapture::Ui(history::HistoryCapture {
                        generation: 1,
                        revision: 0,
                        value: Some(serde_json::json!("old")),
                    }),
                },
                move |_| async move {
                    edit_probe.store(true, std::sync::atomic::Ordering::Release);
                    Ok(history::QueuedEditResult::plain(serde_json::json!({
                        "accepted": true
                    })))
                },
            )
            .expect("stale edit enters the queue");
        assert!(matches!(
            stale.result().await,
            Err(BackendError::Request { message }) if message.contains("再取得")
        ));
        assert!(!edit_ran.load(std::sync::atomic::Ordering::Acquire));

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn mcp_snapshot_and_write_stay_in_one_queue_item_and_read_failure_writes_nothing() {
        let (service, _control, handle) = test_service(4);
        let order = Arc::new(TestMutex::new(Vec::new()));
        let action_order = Arc::clone(&order);
        let read_order = Arc::clone(&action_order);
        let write_order = Arc::clone(&action_order);
        let (snapshot_started, snapshot_started_rx) = oneshot::channel();
        let (finish_snapshot, finish_snapshot_rx) = oneshot::channel();
        let mcp = service
            .enqueue_recorded_from_mcp(
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "object".to_owned(),
                    property: "color".to_owned(),
                    requested_value: serde_json::json!("B"),
                    old_value: history::PriorCapture::InAction,
                },
                move |lease| async move {
                    history::apply_mcp_edit_with_prior(
                        lease,
                        move |_| async move {
                            read_order.lock().unwrap().push("snapshot");
                            let _ = snapshot_started.send(());
                            let _ = finish_snapshot_rx.await;
                            Ok(Some(history::HistoryPrior::Property(serde_json::json!(
                                "A"
                            ))))
                        },
                        move |_, _prior| async move {
                            write_order.lock().unwrap().push("set");
                            Ok(serde_json::json!({"accepted": true, "appliedValue": "B"}))
                        },
                    )
                    .await
                },
            )
            .expect("MCP write is queued");
        snapshot_started_rx.await.expect("snapshot starts");
        let ui_order = Arc::clone(&order);
        let ui = service
            .enqueue_from_ui(EditKind::Edit, move |_| async move {
                ui_order.lock().unwrap().push("ui");
                success(Value::Null)
            })
            .expect("UI edit is queued behind the MCP operation");
        assert_eq!(*order.lock().unwrap(), ["snapshot"]);

        finish_snapshot.send(()).expect("snapshot is released");
        mcp.result().await.expect("MCP write succeeds");
        ui.result().await.expect("queued UI action succeeds");
        assert_eq!(*order.lock().unwrap(), ["snapshot", "set", "ui"]);

        let failed_order = Arc::new(TestMutex::new(Vec::new()));
        let read_order = Arc::clone(&failed_order);
        let write_order = Arc::clone(&failed_order);
        let failed = service
            .enqueue_recorded_from_mcp(
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "object".to_owned(),
                    property: "other".to_owned(),
                    requested_value: serde_json::json!(2),
                    old_value: history::PriorCapture::InAction,
                },
                move |lease| async move {
                    history::apply_mcp_edit_with_prior(
                        lease,
                        move |_| async move {
                            read_order.lock().unwrap().push("snapshot");
                            Err(BackendError::Request {
                                message: "旧値の照会に失敗しました".to_owned(),
                            })
                        },
                        move |_, _prior| async move {
                            write_order.lock().unwrap().push("set");
                            Ok(serde_json::json!({"accepted": true}))
                        },
                    )
                    .await
                },
            )
            .expect("failed MCP read is queued");
        assert!(failed.result().await.is_err());
        assert_eq!(*failed_order.lock().unwrap(), ["snapshot"]);
        let records = service
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .undo_records();
        assert_eq!(records.len(), 1);

        let missing_order = Arc::new(TestMutex::new(Vec::new()));
        let read_order = Arc::clone(&missing_order);
        let write_order = Arc::clone(&missing_order);
        let missing = service
            .enqueue_recorded_from_mcp(
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "object".to_owned(),
                    property: "missing".to_owned(),
                    requested_value: serde_json::json!(2),
                    old_value: history::PriorCapture::InAction,
                },
                move |lease| async move {
                    history::apply_mcp_edit_with_prior(
                        lease,
                        move |_| async move {
                            read_order.lock().unwrap().push("snapshot");
                            Ok(None)
                        },
                        move |_, _prior| async move {
                            write_order.lock().unwrap().push("set");
                            Ok(serde_json::json!({"accepted": true}))
                        },
                    )
                    .await
                },
            )
            .expect("missing MCP old value is queued");
        assert!(missing.result().await.is_err());
        assert_eq!(*missing_order.lock().unwrap(), ["snapshot"]);

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn undo_uses_internal_inverse_for_four_record_kinds() {
        let scenarios = [
            (
                history::HistoryRecord::Create {
                    created_id: "created".to_owned(),
                    parent_id: None,
                    kind: Some("light".to_owned()),
                },
                "scene.deleteObject",
                serde_json::json!({"objectId": "created"}),
                serde_json::json!({"accepted": true}),
            ),
            (
                history::HistoryRecord::Duplicate {
                    source_object_id: "source".to_owned(),
                    created_id: "copy".to_owned(),
                    parent_id: Some("parent".to_owned()),
                },
                "scene.deleteObject",
                serde_json::json!({"objectId": "copy"}),
                serde_json::json!({"accepted": true}),
            ),
            (
                history::HistoryRecord::Reparent {
                    object_id: "child".to_owned(),
                    old_parent_id: None,
                    new_parent_id: Some("parent".to_owned()),
                },
                "scene.reparentObject",
                serde_json::json!({"objectId": "child"}),
                serde_json::json!({"accepted": true}),
            ),
            (
                history::HistoryRecord::SetProperty {
                    object_id: "object".to_owned(),
                    property: "strength".to_owned(),
                    old_value: serde_json::json!(1),
                    new_value: serde_json::json!(2),
                },
                "object.setProperty",
                serde_json::json!({
                    "objectId": "object",
                    "property": "strength",
                    "value": 1
                }),
                serde_json::json!({"accepted": true, "appliedValue": 1}),
            ),
        ];

        for (index, (record, expected_method, expected_params, response)) in
            scenarios.into_iter().enumerate()
        {
            let (service, _control, handle, mut peer) = test_service_with_peer(4);
            seed_history(&service, HistoryDirection::Undo, 20, record);
            seed_history(
                &service,
                HistoryDirection::Redo,
                30,
                history::HistoryRecord::SetProperty {
                    object_id: "kept-redo".to_owned(),
                    property: "flag".to_owned(),
                    old_value: Value::Null,
                    new_value: serde_json::json!(true),
                },
            );

            let ticket = queue_history_action(&service, HistoryDirection::Undo);
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, expected_method, "case {index}");
            assert_eq!(serde_json::to_value(params).unwrap(), expected_params);
            respond(&mut peer, id, response).await;
            assert!(ticket.result().await.is_ok());

            {
                let state = service
                    .history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                assert_eq!(state.undo_len(), 0, "case {index}");
                assert_eq!(state.redo_len(), 2, "internal inverse cleared redo");
            }
            service.shutdown().await;
            handle.shutdown().await;
        }
    }

    #[tokio::test]
    async fn redo_replaces_created_ids_and_next_undo_uses_the_new_id() {
        let scenarios = [
            (
                history::HistoryRecord::Create {
                    created_id: "old-create-id".to_owned(),
                    parent_id: Some("parent".to_owned()),
                    kind: Some("light".to_owned()),
                },
                "scene.createObject",
                serde_json::json!({"parentId": "parent", "kind": "light"}),
            ),
            (
                history::HistoryRecord::Duplicate {
                    source_object_id: "source".to_owned(),
                    created_id: "old-copy-id".to_owned(),
                    parent_id: Some("target".to_owned()),
                },
                "scene.duplicateObject",
                serde_json::json!({"objectId": "source", "newParentId": "target"}),
            ),
        ];

        for (index, (record, expected_method, expected_params)) in scenarios.into_iter().enumerate()
        {
            let (service, _control, handle, mut peer) = test_service_with_peer(4);
            seed_history(&service, HistoryDirection::Redo, 40, record);
            let redo = queue_history_action(&service, HistoryDirection::Redo);
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, expected_method, "case {index}");
            assert_eq!(serde_json::to_value(params).unwrap(), expected_params);
            respond(
                &mut peer,
                id,
                serde_json::json!({"accepted": true, "newId": "fresh-id"}),
            )
            .await;
            assert!(redo.result().await.is_ok());

            let undo_record = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .undo_records()
                .pop()
                .expect("redo moved the updated record to undo");
            match undo_record {
                history::HistoryRecord::Create { created_id, .. }
                | history::HistoryRecord::Duplicate { created_id, .. } => {
                    assert_eq!(created_id, "fresh-id");
                }
                other => panic!("unexpected record after redo: {other:?}"),
            }

            let undo = queue_history_action(&service, HistoryDirection::Undo);
            let (undo_id, undo_method, undo_params) = next_request(&mut peer).await;
            assert_eq!(undo_method, "scene.deleteObject");
            assert_eq!(
                serde_json::to_value(undo_params).unwrap(),
                serde_json::json!({"objectId": "fresh-id"})
            );
            respond(&mut peer, undo_id, serde_json::json!({"accepted": true})).await;
            assert!(undo.result().await.is_ok());
            service.shutdown().await;
            handle.shutdown().await;
        }
    }

    #[tokio::test]
    async fn redo_reuses_stable_reparent_and_property_targets() {
        let scenarios = [
            (
                history::HistoryRecord::Reparent {
                    object_id: "child".to_owned(),
                    old_parent_id: None,
                    new_parent_id: Some("next-parent".to_owned()),
                },
                "scene.reparentObject",
                serde_json::json!({"objectId": "child", "newParentId": "next-parent"}),
                serde_json::json!({"accepted": true}),
            ),
            (
                history::HistoryRecord::SetProperty {
                    object_id: "object".to_owned(),
                    property: "visible".to_owned(),
                    old_value: Value::Null,
                    new_value: serde_json::json!(true),
                },
                "object.setProperty",
                serde_json::json!({"objectId": "object", "property": "visible", "value": true}),
                serde_json::json!({"accepted": true, "appliedValue": true}),
            ),
        ];

        for (record, expected_method, expected_params, response) in scenarios {
            let (service, _control, handle, mut peer) = test_service_with_peer(4);
            seed_history(&service, HistoryDirection::Redo, 41, record);
            let redo = queue_history_action(&service, HistoryDirection::Redo);
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, expected_method);
            assert_eq!(serde_json::to_value(params).unwrap(), expected_params);
            respond(&mut peer, id, response).await;
            assert!(redo.result().await.is_ok());
            assert_eq!(service.history_cursor(HistoryDirection::Undo).0, Some(41));
            service.shutdown().await;
            handle.shutdown().await;
        }
    }

    #[tokio::test]
    async fn single_undo_redo_failure_drops_only_the_attempted_entry_and_reports_error() {
        let (service, _control, handle, mut peer) = test_service_with_peer(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            50,
            history::HistoryRecord::SetProperty {
                object_id: "older".to_owned(),
                property: "value".to_owned(),
                old_value: serde_json::json!(1),
                new_value: serde_json::json!(2),
            },
        );
        seed_history(
            &service,
            HistoryDirection::Undo,
            51,
            history::HistoryRecord::Create {
                created_id: "rejected".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        let undo = queue_history_action(&service, HistoryDirection::Undo);
        let (id, method, _) = next_request(&mut peer).await;
        assert_eq!(method, "scene.deleteObject");
        respond(&mut peer, id, serde_json::json!({"accepted": false})).await;
        assert!(matches!(
            undo.result().await,
            Err(BackendError::Request { .. })
        ));
        {
            let state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert_eq!(state.undo_len(), 1);
            assert_eq!(
                state.undo_records()[0],
                history::HistoryRecord::SetProperty {
                    object_id: "older".to_owned(),
                    property: "value".to_owned(),
                    old_value: serde_json::json!(1),
                    new_value: serde_json::json!(2),
                }
            );
        }

        seed_history(
            &service,
            HistoryDirection::Redo,
            60,
            history::HistoryRecord::Reparent {
                object_id: "retained".to_owned(),
                old_parent_id: None,
                new_parent_id: Some("parent".to_owned()),
            },
        );
        seed_history(
            &service,
            HistoryDirection::Redo,
            61,
            history::HistoryRecord::SetProperty {
                object_id: "unsupported".to_owned(),
                property: "value".to_owned(),
                old_value: serde_json::json!(1),
                new_value: serde_json::json!(2),
            },
        );
        let redo = queue_history_action(&service, HistoryDirection::Redo);
        let (id, method, _) = next_request(&mut peer).await;
        assert_eq!(method, "object.setProperty");
        let error = BridgeError {
            code: ErrorCode::method_not_supported(),
            message: "unsupported".to_owned(),
            data: None,
        };
        peer.send(response_frame(id, ResponsePayload::Error(error)))
            .await
            .expect("error response sends");
        assert!(matches!(
            redo.result().await,
            Err(BackendError::Engine { .. })
        ));
        assert_eq!(service.history_cursor(HistoryDirection::Redo).0, Some(60));
        let unsupported = queue_history_action(&service, HistoryDirection::Redo);
        assert_eq!(
            unsupported.result().await.expect("unsupported is a no-op"),
            Value::Null
        );

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn public_delete_clears_both_histories_only_after_acceptance() {
        let (service, _control, handle) = test_service(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            70,
            history::HistoryRecord::Create {
                created_id: "undo-entry".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        seed_history(
            &service,
            HistoryDirection::Redo,
            71,
            history::HistoryRecord::Create {
                created_id: "redo-entry".to_owned(),
                parent_id: None,
                kind: None,
            },
        );

        let rejected = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::DeleteObject {
                    object_id: "object".to_owned(),
                },
                |_| async {
                    Ok(QueuedEditResult::plain(
                        serde_json::json!({"accepted": false}),
                    ))
                },
            )
            .expect("delete request is accepted into the queue");
        assert_eq!(
            rejected
                .result()
                .await
                .expect("engine rejection is returned")["accepted"],
            false
        );
        {
            let state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert_eq!(state.undo_len(), 1);
            assert_eq!(state.redo_len(), 1);
        }

        let accepted = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::DeleteObject {
                    object_id: "object".to_owned(),
                },
                |_| async {
                    Ok(QueuedEditResult::plain(
                        serde_json::json!({"accepted": true}),
                    ))
                },
            )
            .expect("delete request is accepted into the queue");
        accepted.result().await.expect("delete is accepted");
        {
            let state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert_eq!(state.undo_len(), 0);
            assert_eq!(state.redo_len(), 0);
        }
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn stale_empty_disconnected_and_unsupported_history_requests_are_noops() {
        let (service, _control, handle, mut peer) = test_service_with_peer(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            80,
            history::HistoryRecord::Create {
                created_id: "entry".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        let (head_id, revision) = service.history_cursor(HistoryDirection::Undo);
        let stale_id = service
            .enqueue_undo_from_ui(head_id.map(|id| id.wrapping_add(1)), revision)
            .expect("stale head is a no-op");
        assert_eq!(
            stale_id.result().await.expect("stale head is ignored"),
            Value::Null
        );
        let stale_revision = service
            .enqueue_undo_from_ui(head_id, revision.wrapping_add(1))
            .expect("stale revision is a no-op");
        assert_eq!(
            stale_revision
                .result()
                .await
                .expect("stale revision is ignored"),
            Value::Null
        );
        assert_eq!(service.history_cursor(HistoryDirection::Undo).0, head_id);

        let empty = service
            .enqueue_redo_from_ui(None, revision)
            .expect("empty redo is a no-op");
        assert_eq!(
            empty.result().await.expect("empty redo is ignored"),
            Value::Null
        );

        {
            let mut state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.mark_edit_unsupported_for_test();
        }
        let unsupported = service
            .enqueue_undo_from_ui(head_id, revision)
            .expect("unsupported history is a no-op");
        assert_eq!(
            unsupported.result().await.expect("unsupported is ignored"),
            Value::Null
        );
        assert!(tokio::time::timeout(Duration::from_millis(30), peer.recv())
            .await
            .is_err());
        service.shutdown().await;
        handle.shutdown().await;

        let (disconnected_service, disconnected_control, disconnected_handle, mut peer) =
            test_service_with_peer(4);
        seed_history(
            &disconnected_service,
            HistoryDirection::Undo,
            81,
            history::HistoryRecord::Create {
                created_id: "disconnected-entry".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        let (stale_head, stale_revision) =
            disconnected_service.history_cursor(HistoryDirection::Undo);
        disconnected_control.disconnect();
        let disconnected = disconnected_service
            .enqueue_undo_from_ui(stale_head, stale_revision)
            .expect("disconnected undo is a no-op");
        assert_eq!(
            disconnected.result().await.expect("disconnect is ignored"),
            Value::Null
        );
        assert!(disconnected_service
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .undo_records()
            .is_empty());
        assert!(tokio::time::timeout(Duration::from_millis(30), peer.recv())
            .await
            .is_err());
        disconnected_service.shutdown().await;
        disconnected_handle.shutdown().await;
    }

    #[tokio::test]
    async fn duplicate_undo_and_redo_requests_apply_only_the_expected_head_once() {
        let (service, _control, handle, mut peer) = test_service_with_peer(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            90,
            history::HistoryRecord::Create {
                created_id: "older".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        seed_history(
            &service,
            HistoryDirection::Undo,
            91,
            history::HistoryRecord::Create {
                created_id: "newer".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        let (head_id, revision) = service.history_cursor(HistoryDirection::Undo);
        let first = service
            .enqueue_undo_from_ui(head_id, revision)
            .expect("first undo is queued");
        let repeated = service
            .enqueue_undo_from_ui(head_id, revision)
            .expect("repeated undo is queued for cursor validation");
        let (id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.deleteObject");
        assert_eq!(
            serde_json::to_value(params).unwrap(),
            serde_json::json!({"objectId": "newer"})
        );
        respond(&mut peer, id, serde_json::json!({"accepted": true})).await;
        assert!(first.result().await.is_ok());
        assert_eq!(
            repeated.result().await.expect("repeat is ignored"),
            Value::Null
        );

        seed_history(
            &service,
            HistoryDirection::Redo,
            92,
            history::HistoryRecord::Create {
                created_id: "older-redo".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        seed_history(
            &service,
            HistoryDirection::Redo,
            93,
            history::HistoryRecord::Create {
                created_id: "newer-redo".to_owned(),
                parent_id: Some("parent".to_owned()),
                kind: None,
            },
        );
        let (redo_head, redo_revision) = service.history_cursor(HistoryDirection::Redo);
        let first_redo = service
            .enqueue_redo_from_ui(redo_head, redo_revision)
            .expect("first redo is queued");
        let repeated_redo = service
            .enqueue_redo_from_ui(redo_head, redo_revision)
            .expect("repeated redo is queued for cursor validation");
        let (redo_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.createObject");
        assert_eq!(
            serde_json::to_value(params).unwrap(),
            serde_json::json!({"parentId": "parent"})
        );
        respond(
            &mut peer,
            redo_id,
            serde_json::json!({"accepted": true, "newId": "redo-fresh"}),
        )
        .await;
        assert!(first_redo.result().await.is_ok());
        assert_eq!(
            repeated_redo.result().await.expect("repeat is ignored"),
            Value::Null
        );
        assert!(tokio::time::timeout(Duration::from_millis(30), peer.recv())
            .await
            .is_err());

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn history_clears_on_disconnect_and_exit_but_survives_workspace_close() {
        let (service, control, handle) = test_service(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            100,
            history::HistoryRecord::Create {
                created_id: "undo".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        seed_history(
            &service,
            HistoryDirection::Redo,
            101,
            history::HistoryRecord::Create {
                created_id: "redo".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        {
            let mut state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.synchronize(Some(1));
            assert_eq!(state.undo_len(), 1);
            assert_eq!(state.redo_len(), 1);
        }
        control.disconnect();
        let snapshot = service.history_snapshot();
        assert_eq!(snapshot.0, None);
        {
            let state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert_eq!(state.undo_len(), 0);
            assert_eq!(state.redo_len(), 0);
        }
        service.shutdown().await;
        handle.shutdown().await;

        let (service, _control, handle) = test_service(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            102,
            history::HistoryRecord::Create {
                created_id: "workspace-undo".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        seed_history(
            &service,
            HistoryDirection::Redo,
            103,
            history::HistoryRecord::Create {
                created_id: "workspace-redo".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        {
            let state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert_eq!(state.undo_len(), 1);
            assert_eq!(state.redo_len(), 1);
        }
        {
            let mut state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // workspace_closeはBridge世代を変えないため履歴を保持する。
            state.synchronize(Some(1));
            assert_eq!(state.undo_len(), 1);
            assert_eq!(state.redo_len(), 1);
        }
        service.shutdown().await;
        handle.shutdown().await;

        let (service, _control, handle) = test_service(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            104,
            history::HistoryRecord::Create {
                created_id: "exit-undo".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        seed_history(
            &service,
            HistoryDirection::Redo,
            105,
            history::HistoryRecord::Create {
                created_id: "exit-redo".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        service.shutdown().await;
        {
            let state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert_eq!(state.undo_len(), 0);
            assert_eq!(state.redo_len(), 0);
        }
        handle.shutdown().await;
    }

    struct DropNotice(Option<oneshot::Sender<()>>);

    impl Drop for DropNotice {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
}
