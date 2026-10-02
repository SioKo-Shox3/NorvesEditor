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

const QUEUE_CAPACITY: usize = 64;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

type EditFuture = Pin<Box<dyn Future<Output = Result<Value, BackendError>> + Send + 'static>>;
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
pub(crate) enum EditSource {
    Ui,
    Mcp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum EditKind {
    Edit,
    Undo,
    Redo,
    RuntimeControl,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) struct HistoryMarker {
    pub(crate) sequence: u64,
    pub(crate) source: EditSource,
    pub(crate) kind: EditKind,
    pub(crate) generation: u64,
}

#[derive(Default)]
struct HistoryState {
    generation: Option<u64>,
    revision: u64,
    entries: Vec<HistoryMarker>,
}

struct QueueItem {
    sequence: u64,
    source: EditSource,
    kind: EditKind,
    lease: BridgeLease,
    action: EditAction,
    cancelled: oneshot::Receiver<()>,
    result: oneshot::Sender<Result<Value, BackendError>>,
}

struct Admission {
    accepting: bool,
    next_sequence: u64,
}

/// ticketのdropは開始前だけを取り消す。実行中の要求はサービス停止時に中止する。
#[allow(dead_code)]
pub(crate) struct EditTicket {
    result: oneshot::Receiver<Result<Value, BackendError>>,
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
        outcome
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
            action: Box::new(|lease| Box::pin(action(lease))),
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
            synchronize_history(&mut history, generation);
            (
                history.generation,
                history.revision,
                history.entries.clone(),
            )
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

fn synchronize_history(history: &mut HistoryState, generation: Option<u64>) {
    if history.generation != generation {
        history.generation = generation;
        history.entries.clear();
        history.revision = history.revision.wrapping_add(1);
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
    synchronize_history(
        &mut history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        generation,
    );

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
                synchronize_history(
                    &mut state,
                    generation,
                );
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
                synchronize_history(
                    &mut state,
                    current,
                );
                if session_generation != Some(item.lease.generation)
                    || current != Some(item.lease.generation)
                {
                    break (Err(BackendError::NotConnected), false);
                }
            }
            result = &mut action => {
                break (match result {
                    Ok(value) => {
                        let marker = HistoryMarker {
                            sequence: item.sequence,
                            source: item.source,
                            kind: item.kind,
                            generation: item.lease.generation,
                        };
                        let committed = bridge.with_current_generation(|current| {
                            if current != Some(item.lease.generation) {
                                return false;
                            }
                            let mut state = history
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            synchronize_history(&mut state, current);
                            state.entries.push(marker);
                            state.revision = state.revision.wrapping_add(1);
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
                success(serde_json::json!("recorded"))
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

    struct DropNotice(Option<oneshot::Sender<()>>);

    impl Drop for DropNotice {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
}
