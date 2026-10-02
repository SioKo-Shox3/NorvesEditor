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

use history::{HistoryMarker, HistoryRequest, HistoryState, QueuedEditResult};

const QUEUE_CAPACITY: usize = 64;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

type EditFuture =
    Pin<Box<dyn Future<Output = Result<QueuedEditResult, BackendError>> + Send + 'static>>;
type EditAction = Box<dyn FnOnce(BridgeLease) -> EditFuture + Send + 'static>;

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
    action: EditAction,
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
            action,
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
    }

    #[cfg(test)]
    pub(crate) fn history_lock_available(&self) -> bool {
        self.history.try_lock().is_ok()
    }
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
        let _ = item.result.send(Err(BackendError::NotConnected));
        return false;
    }

    let prepared_history = if let Some(request) = item.history_request.take() {
        let mut state = history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.synchronize(bridge.current_generation());
        match state.prepare(request, item.source, item.lease.generation) {
            Ok(prepared) => Some(prepared),
            Err(error) => {
                let _ = item.result.send(Err(error));
                return false;
            }
        }
    } else {
        None
    };

    let action = (item.action)(item.lease.clone());
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
                break (match result {
                    Ok(value) => {
                        let committed = bridge.with_current_generation(|current| {
                            if current != Some(item.lease.generation) {
                                return false;
                            }
                            let mut state = history
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            state.synchronize(current);
                            if let Some(prepared) = prepared_history {
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
                            true
                        });
                        if committed { Ok(value) } else { Err(BackendError::NotConnected) }
                    }
                    Err(error) => Err(error),
                }, false);
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
    use norves_bridge_editor_client::{loopback_pair, DispatchHandle, Dispatcher};
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
                HistoryRequest::CreateObject { parent_id: None },
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
                HistoryRequest::CreateObject { parent_id: None },
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

    struct DropNotice(Option<oneshot::Sender<()>>);

    impl Drop for DropNotice {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
}
