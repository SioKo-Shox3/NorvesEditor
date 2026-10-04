//! 画面とMCPの編集を、接続世代に結び付けた有界actorで実行する。

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde_json::Value;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{mpsc, oneshot, watch, Mutex};

use crate::bridge_state::{BridgeFacade, BridgeLease};
use crate::dto::{EditAppliedDto, EditDiscardResultDto, EditHistorySummaryDto, EditSourceDto};
use crate::error::BackendError;
use crate::mcp::{
    authorization::{McpHistoryAction, McpHistoryDirection},
    McpAuthorization, McpRequestLease,
};
use crate::protocol_names::events;

mod groups;
mod history;
pub(crate) mod mcp;

use mcp::QueuedMcpPermit;

use history::{
    HistoryAction, HistoryActionRequest, HistoryMarker, HistoryOperationContext, HistoryState,
};
pub(crate) use history::{
    HistoryCapture, HistoryPrior, HistoryRequest, PriorCapture, QueuedEditResult,
};
pub(crate) use history::{HistoryDirection, HistoryRecord};

const QUEUE_CAPACITY: usize = 64;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
const TICKET_WAITING: u8 = 0;
const TICKET_STARTED: u8 = 1;
const TICKET_CANCELLED: u8 = 2;

type EditFuture =
    Pin<Box<dyn Future<Output = Result<QueuedEditResult, BackendError>> + Send + 'static>>;
type EditAction = Box<dyn FnOnce(BridgeLease) -> EditFuture + Send + 'static>;

enum QueuedAction {
    Edit(EditAction),
    History(HistoryActionRequest),
    Retry,
    Discard,
    Group(GroupControl),
}

enum GroupControl {
    BeginNamed {
        name: String,
        execution: Arc<mcp::McpExecution>,
    },
    EndNamed {
        secret: String,
        execution: Arc<mcp::McpExecution>,
    },
    CloseFailed {
        token: u64,
    },
    Boundary {
        secret: Option<String>,
        keep: bool,
    },
    Begin {
        token: u64,
        generation: u64,
        name: String,
    },
    End {
        token: u64,
        generation: u64,
    },
}

type EditEventSink = Arc<dyn Fn(EditServiceEvent) + Send + Sync + 'static>;

#[derive(Clone)]
enum EditServiceEvent {
    Applied(EditAppliedDto),
    HistoryChanged(EditHistorySummaryDto),
}

/// 捕捉履歴を持たない編集コマンド用の、画面更新イベント情報。
#[derive(Clone)]
pub(crate) struct EditEventInput {
    pub(crate) operation: &'static str,
    pub(crate) object_id: Option<String>,
    pub(crate) property: Option<String>,
    pub(crate) value: Option<Value>,
}

/// MCP確認中に比較する編集履歴の改訂とundo先頭。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EditHistoryConfirmationSnapshot {
    pub(crate) generation: Option<u64>,
    pub(crate) history_revision: u64,
    pub(crate) undo_head_id: Option<u64>,
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

enum QueueOutcome {
    Complete(QueuedEditResult),
    Reconfirm,
}

impl QueueOutcome {
    fn into_value(self) -> Result<Value, BackendError> {
        match self {
            Self::Complete(result) => Ok(result.value),
            Self::Reconfirm => Err(BackendError::Request {
                message: "対象が変わったため、エディタ画面で再確認してください。".to_owned(),
            }),
        }
    }
}

struct QueueItem {
    sequence: u64,
    source: EditSource,
    kind: EditKind,
    lease: BridgeLease,
    action: QueuedAction,
    group_token: Option<u64>,
    history_request: Option<HistoryRequest>,
    event_request: Option<HistoryRequest>,
    event_input: Option<EditEventInput>,
    auth_lease: Option<McpRequestLease>,
    mcp_permit: Option<QueuedMcpPermit>,
    cancelled: oneshot::Receiver<()>,
    queue_state: Arc<AtomicU8>,
    result: oneshot::Sender<Result<QueueOutcome, BackendError>>,
}

struct EnqueueContext {
    source: EditSource,
    kind: EditKind,
    group_token: Option<u64>,
    history_request: Option<HistoryRequest>,
    event_request: Option<HistoryRequest>,
    event_input: Option<EditEventInput>,
    auth_lease: Option<McpRequestLease>,
    mcp_permit: Option<QueuedMcpPermit>,
}

struct Admission {
    accepting: bool,
    next_sequence: u64,
}

/// ticketのdropは開始前だけを取り消す。実行中の要求はサービス停止時に中止する。
#[allow(dead_code)]
pub(crate) struct EditTicket {
    result: oneshot::Receiver<Result<QueueOutcome, BackendError>>,
    cancel: TicketCancellationGuard,
    queue_state: Arc<AtomicU8>,
}

struct TicketCancellationGuard {
    _sender: oneshot::Sender<()>,
    queue_state: Arc<AtomicU8>,
}

impl Drop for TicketCancellationGuard {
    fn drop(&mut self) {
        let _ = self.queue_state.compare_exchange(
            TICKET_WAITING,
            TICKET_CANCELLED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EditGroupHandle {
    token: u64,
    generation: u64,
    auth_revision: u64,
}

#[allow(dead_code)]
impl EditTicket {
    pub(crate) async fn result(self) -> Result<Value, BackendError> {
        let EditTicket {
            result,
            cancel,
            queue_state: _,
        } = self;
        let outcome = result
            .await
            .unwrap_or(Err(BackendError::EditServiceStopping));
        drop(cancel);
        outcome.and_then(QueueOutcome::into_value)
    }

    pub(crate) async fn result_until(
        self,
        deadline: tokio::time::Instant,
    ) -> Result<Value, BackendError> {
        self.outcome_until(deadline)
            .await
            .and_then(QueueOutcome::into_value)
    }

    async fn outcome_until(
        self,
        deadline: tokio::time::Instant,
    ) -> Result<QueueOutcome, BackendError> {
        let EditTicket {
            result,
            cancel,
            queue_state,
        } = self;
        let outcome = tokio::time::timeout_at(deadline, result).await;
        if outcome.is_err() {
            let _ = queue_state.compare_exchange(
                TICKET_WAITING,
                TICKET_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        drop(cancel);
        match outcome {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => Err(BackendError::EditServiceStopping),
            Err(_) if queue_state.load(Ordering::Acquire) == TICKET_STARTED => {
                Err(BackendError::Request {
                    message: "MCP要求の全体期限を超えました。開始済み操作のBridge結果は不明のため、自動再送せず状態を確認してください。"
                        .to_owned(),
                })
            }
            Err(_) => Err(BackendError::Request {
                message: "MCP要求の全体期限を超えました。Bridge開始前の操作は編集列から取り消しました。"
                    .to_owned(),
            }),
        }
    }
}

/// UIとMCPで共有し、接続世代ごとの編集列と履歴を所有する。
#[allow(dead_code)]
pub(crate) struct EditService {
    bridge: BridgeFacade,
    sender: mpsc::Sender<QueueItem>,
    admission: StdMutex<Admission>,
    history: Arc<StdMutex<HistoryState>>,
    authorization: McpAuthorization,
    shutdown_tx: watch::Sender<bool>,
    join: Mutex<Option<JoinHandle<()>>>,
}

#[allow(dead_code)]
impl EditService {
    pub(crate) fn new(bridge: BridgeFacade) -> Self {
        Self::with_capacity(bridge, QUEUE_CAPACITY)
    }

    pub(crate) fn new_with_app(bridge: BridgeFacade, app: AppHandle) -> Self {
        Self::new_with_app_and_authorization(bridge, app, McpAuthorization::default())
    }

    pub(crate) fn new_with_app_and_authorization(
        bridge: BridgeFacade,
        app: AppHandle,
        authorization: McpAuthorization,
    ) -> Self {
        Self::with_capacity_and_sink_and_authorization(
            bridge,
            QUEUE_CAPACITY,
            Some(tauri_event_sink(app)),
            authorization,
        )
    }

    fn with_capacity(bridge: BridgeFacade, capacity: usize) -> Self {
        Self::with_capacity_and_sink_and_authorization(
            bridge,
            capacity,
            None,
            McpAuthorization::default(),
        )
    }

    fn with_capacity_and_sink(
        bridge: BridgeFacade,
        capacity: usize,
        event_sink: Option<EditEventSink>,
    ) -> Self {
        Self::with_capacity_and_sink_and_authorization(
            bridge,
            capacity,
            event_sink,
            McpAuthorization::default(),
        )
    }

    fn with_capacity_and_sink_and_authorization(
        bridge: BridgeFacade,
        capacity: usize,
        event_sink: Option<EditEventSink>,
        authorization: McpAuthorization,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let auth_changes = authorization.subscribe_revision();
        let history = Arc::new(StdMutex::new(HistoryState::default()));
        let actor = run_actor(
            bridge.clone(),
            receiver,
            bridge.subscribe(),
            shutdown_rx,
            auth_changes,
            Arc::clone(&history),
            event_sink.clone(),
            authorization.clone(),
        );
        #[cfg(not(test))]
        let join = tauri::async_runtime::spawn(actor);
        // 要求とactorの時計を揃え、仮想時計で全体期限とBridge期限を検査する。
        #[cfg(test)]
        let join = JoinHandle::Tokio(tokio::spawn(actor));
        Self {
            bridge,
            sender,
            admission: StdMutex::new(Admission {
                accepting: true,
                next_sequence: 0,
            }),
            history,
            authorization,
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

    pub(crate) fn enqueue_with_event_from_ui<F, Fut>(
        &self,
        kind: EditKind,
        event_input: EditEventInput,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, BackendError>> + Send + 'static,
    {
        self.enqueue_inner(
            EditSource::Ui,
            kind,
            None,
            None,
            Some(event_input),
            Box::new(|lease| {
                Box::pin(async move { action(lease).await.map(QueuedEditResult::plain) })
            }),
        )
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

    /// HTTP で捕捉した認証リースを維持して MCP 編集を列へ投入する。
    pub(crate) fn enqueue_from_mcp_authorized<F, Fut>(
        &self,
        auth_lease: McpRequestLease,
        kind: EditKind,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, BackendError>> + Send + 'static,
    {
        self.enqueue_with_context(
            EnqueueContext {
                source: EditSource::Mcp,
                kind,
                group_token: None,
                history_request: None,
                event_request: None,
                event_input: None,
                auth_lease: Some(auth_lease),
                mcp_permit: None,
            },
            Box::new(|lease| {
                Box::pin(async move { action(lease).await.map(QueuedEditResult::plain) })
            }),
        )
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

    /// HTTP で捕捉した認証リースを維持して旧値照会と書込みを同じ列へ投入する。
    pub(crate) fn enqueue_recorded_from_mcp_authorized<F, Fut>(
        &self,
        auth_lease: McpRequestLease,
        kind: EditKind,
        request: HistoryRequest,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<QueuedEditResult, BackendError>> + Send + 'static,
    {
        self.enqueue_with_context(
            EnqueueContext {
                source: EditSource::Mcp,
                kind,
                group_token: None,
                history_request: Some(request.clone()),
                event_request: Some(request),
                event_input: None,
                auth_lease: Some(auth_lease),
                mcp_permit: None,
            },
            Box::new(|lease| Box::pin(action(lease))),
        )
    }

    /// MCPの名前付きまとまりを列へ投入し、後続の明示的な編集に使うハンドルを返す。
    pub(crate) async fn begin_group_from_mcp(
        &self,
        name: String,
    ) -> Result<EditGroupHandle, BackendError> {
        self.begin_group_from_mcp_authorized(name, self.authorization.current_lease())
            .await
    }

    /// 認証済み MCP 要求の改訂に結び付けて名前付きまとまりを開始する。
    pub(crate) async fn begin_group_from_mcp_authorized(
        &self,
        name: String,
        auth_lease: McpRequestLease,
    ) -> Result<EditGroupHandle, BackendError> {
        if !auth_lease.is_current() {
            return Err(BackendError::McpAuthorizationRevoked);
        }
        let (ticket, token, generation) = self.enqueue_control_with_auth(
            EditSource::Mcp,
            Some(auth_lease.clone()),
            |sequence, generation| {
                QueuedAction::Group(GroupControl::Begin {
                    token: sequence,
                    generation,
                    name,
                })
            },
        )?;
        ticket.result().await?;
        Ok(EditGroupHandle {
            token,
            generation,
            auth_revision: auth_lease.revision(),
        })
    }

    pub(crate) async fn end_group_from_mcp(
        &self,
        group: EditGroupHandle,
    ) -> Result<Value, BackendError> {
        self.end_group_from_mcp_authorized(group, self.authorization.current_lease())
            .await
    }

    /// 認証改訂が開始時と一致する場合だけ、名前付きまとまりを閉じる。
    pub(crate) async fn end_group_from_mcp_authorized(
        &self,
        group: EditGroupHandle,
        auth_lease: McpRequestLease,
    ) -> Result<Value, BackendError> {
        if !auth_lease.is_current() || group.auth_revision != auth_lease.revision() {
            return Err(BackendError::McpAuthorizationRevoked);
        }
        if self.bridge.current_generation() != Some(group.generation) {
            return Err(BackendError::NotConnected);
        }
        let (ticket, _, _) =
            self.enqueue_control_with_auth(EditSource::Mcp, Some(auth_lease), |_, _| {
                QueuedAction::Group(GroupControl::End {
                    token: group.token,
                    generation: group.generation,
                })
            })?;
        ticket.result().await
    }

    pub(crate) fn enqueue_recorded_in_group_from_mcp<F, Fut>(
        &self,
        group: EditGroupHandle,
        kind: EditKind,
        request: HistoryRequest,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<QueuedEditResult, BackendError>> + Send + 'static,
    {
        self.enqueue_recorded_in_group_from_mcp_authorized(
            group,
            self.authorization.current_lease(),
            kind,
            request,
            action,
        )
    }

    /// 要求リースと開始時の認証改訂が一致するまとまり編集だけを列へ入れる。
    pub(crate) fn enqueue_recorded_in_group_from_mcp_authorized<F, Fut>(
        &self,
        group: EditGroupHandle,
        auth_lease: McpRequestLease,
        kind: EditKind,
        request: HistoryRequest,
        action: F,
    ) -> Result<EditTicket, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<QueuedEditResult, BackendError>> + Send + 'static,
    {
        if !auth_lease.is_current() || group.auth_revision != auth_lease.revision() {
            return Err(BackendError::McpAuthorizationRevoked);
        }
        let generation = self.bridge.current_generation().unwrap_or_default();
        if generation != group.generation {
            return Err(BackendError::NotConnected);
        }
        self.enqueue_with_context(
            EnqueueContext {
                source: EditSource::Mcp,
                kind,
                group_token: Some(group.token),
                history_request: Some(request.clone()),
                event_request: Some(request),
                event_input: None,
                auth_lease: Some(auth_lease),
                mcp_permit: None,
            },
            Box::new(|lease| Box::pin(action(lease))),
        )
    }

    pub(crate) async fn retry_pending_from_ui(&self) -> Result<Value, BackendError> {
        let (ticket, _, _) = self.enqueue_control(EditSource::Ui, |_, _| QueuedAction::Retry)?;
        ticket.result().await
    }

    pub(crate) async fn discard_pending_from_ui(
        &self,
    ) -> Result<crate::dto::EditDiscardResultDto, BackendError> {
        let (ticket, _, _) = self.enqueue_control(EditSource::Ui, |_, _| QueuedAction::Discard)?;
        let result = ticket.result().await?;
        serde_json::from_value(result).map_err(|error| BackendError::Request {
            message: format!("破棄結果の形式が不正です: {error}"),
        })
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

    /// 認証済み MCP 要求を、actor の実行直前に同じ改訂であることを確かめて実行する。
    pub(crate) async fn submit_from_mcp_authorized<F, Fut>(
        &self,
        auth_lease: McpRequestLease,
        kind: EditKind,
        action: F,
    ) -> Result<Value, BackendError>
    where
        F: FnOnce(BridgeLease) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, BackendError>> + Send + 'static,
    {
        let deadline = auth_lease.request_deadline();
        self.enqueue_from_mcp_authorized(auth_lease, kind, action)?
            .result_until(deadline)
            .await
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
                self.synchronize_history_if_disconnected();
                return Ok(completed_noop_ticket());
            }
            Err(error) => return Err(error),
        };
        let request = HistoryActionRequest {
            direction,
            expected_head_id,
            expected_revision,
        };
        let can_run = self.can_prepare_history_action(lease.generation, request);
        let pending = self
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_pending();
        if !can_run && !pending {
            return Ok(completed_noop_ticket());
        }

        let (cancel, cancelled) = oneshot::channel();
        let (result, result_rx) = oneshot::channel();
        let queue_state = Arc::new(AtomicU8::new(TICKET_WAITING));
        let item = QueueItem {
            sequence: admission.next_sequence,
            source,
            kind: match direction {
                HistoryDirection::Undo => EditKind::Undo,
                HistoryDirection::Redo => EditKind::Redo,
            },
            lease,
            action: QueuedAction::History(request),
            group_token: None,
            history_request: None,
            event_request: None,
            event_input: None,
            auth_lease: (source == EditSource::Mcp).then(|| self.authorization.current_lease()),
            mcp_permit: None,
            cancelled,
            queue_state: Arc::clone(&queue_state),
            result,
        };
        match self.sender.try_send(item) {
            Ok(()) => {
                admission.next_sequence = admission.next_sequence.wrapping_add(1);
                Ok(EditTicket {
                    result: result_rx,
                    cancel: TicketCancellationGuard {
                        _sender: cancel,
                        queue_state: Arc::clone(&queue_state),
                    },
                    queue_state,
                })
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(BackendError::EditQueueFull),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(BackendError::EditServiceStopping),
        }
    }

    fn synchronize_history_if_disconnected(&self) {
        self.bridge.with_current_generation(|generation| {
            if generation.is_none() {
                self.history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .synchronize(None);
            }
        });
    }

    fn can_prepare_history_action(
        &self,
        expected_generation: u64,
        request: HistoryActionRequest,
    ) -> bool {
        self.bridge.with_current_generation(|generation| {
            if generation != Some(expected_generation) {
                return false;
            }
            let mut history = self
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            history.synchronize(generation);
            history.has_active_group() || history.prepare_action(request).is_some()
        })
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
            None,
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
            Some(request.clone()),
            Some(request),
            None,
            Box::new(|lease| Box::pin(action(lease))),
        )
    }

    fn enqueue_inner(
        &self,
        source: EditSource,
        kind: EditKind,
        history_request: Option<HistoryRequest>,
        event_request: Option<HistoryRequest>,
        event_input: Option<EditEventInput>,
        action: EditAction,
    ) -> Result<EditTicket, BackendError> {
        self.enqueue_with_context(
            EnqueueContext {
                source,
                kind,
                group_token: None,
                history_request,
                event_request,
                event_input,
                auth_lease: None,
                mcp_permit: None,
            },
            action,
        )
    }

    fn enqueue_with_context(
        &self,
        context: EnqueueContext,
        action: EditAction,
    ) -> Result<EditTicket, BackendError> {
        self.enqueue_action_with_context(context, QueuedAction::Edit(action))
    }

    fn enqueue_action_with_context(
        &self,
        context: EnqueueContext,
        action: QueuedAction,
    ) -> Result<EditTicket, BackendError> {
        let EnqueueContext {
            source,
            kind,
            group_token,
            history_request,
            event_request,
            event_input,
            mut auth_lease,
            mcp_permit,
        } = context;
        if source == EditSource::Mcp && auth_lease.is_none() {
            auth_lease = Some(self.authorization.current_lease());
        }
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
        let queue_state = Arc::new(AtomicU8::new(TICKET_WAITING));
        let item = QueueItem {
            sequence: admission.next_sequence,
            source,
            kind,
            lease,
            action,
            group_token,
            history_request,
            event_request,
            event_input,
            auth_lease,
            mcp_permit,
            cancelled,
            queue_state: Arc::clone(&queue_state),
            result,
        };
        match self.sender.try_send(item) {
            Ok(()) => {
                admission.next_sequence = admission.next_sequence.wrapping_add(1);
                Ok(EditTicket {
                    result: result_rx,
                    cancel: TicketCancellationGuard {
                        _sender: cancel,
                        queue_state: Arc::clone(&queue_state),
                    },
                    queue_state,
                })
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(BackendError::EditQueueFull),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(BackendError::EditServiceStopping),
        }
    }

    fn enqueue_control<F>(
        &self,
        source: EditSource,
        build: F,
    ) -> Result<(EditTicket, u64, u64), BackendError>
    where
        F: FnOnce(u64, u64) -> QueuedAction,
    {
        let auth_lease = (source == EditSource::Mcp).then(|| self.authorization.current_lease());
        self.enqueue_control_with_auth(source, auth_lease, build)
    }

    fn enqueue_control_with_auth<F>(
        &self,
        source: EditSource,
        auth_lease: Option<McpRequestLease>,
        build: F,
    ) -> Result<(EditTicket, u64, u64), BackendError>
    where
        F: FnOnce(u64, u64) -> QueuedAction,
    {
        let mut admission = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !admission.accepting {
            return Err(BackendError::EditServiceStopping);
        }
        let lease = self.bridge.pin()?;
        let sequence = admission.next_sequence;
        let generation = lease.generation;
        let (cancel, cancelled) = oneshot::channel();
        let (result, result_rx) = oneshot::channel();
        let queue_state = Arc::new(AtomicU8::new(TICKET_WAITING));
        let item = QueueItem {
            sequence,
            source,
            kind: EditKind::Edit,
            lease,
            action: build(sequence, generation),
            group_token: None,
            history_request: None,
            event_request: None,
            event_input: None,
            auth_lease,
            mcp_permit: None,
            cancelled,
            queue_state: Arc::clone(&queue_state),
            result,
        };
        match self.sender.try_send(item) {
            Ok(()) => {
                admission.next_sequence = admission.next_sequence.wrapping_add(1);
                Ok((
                    EditTicket {
                        result: result_rx,
                        cancel: TicketCancellationGuard {
                            _sender: cancel,
                            queue_state: Arc::clone(&queue_state),
                        },
                        queue_state,
                    },
                    sequence,
                    generation,
                ))
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

    pub(crate) fn history_summary(&self) -> EditHistorySummaryDto {
        self.bridge.with_current_generation(|generation| {
            let mut history = self
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            history.synchronize(generation);
            history.summary()
        })
    }

    /// 確認待ちの前後で履歴影響を比較する読み取り窓口を作る。
    pub(crate) fn confirmation_history_source(
        &self,
    ) -> Arc<dyn Fn() -> EditHistoryConfirmationSnapshot + Send + Sync> {
        let bridge = self.bridge.clone();
        let history = Arc::clone(&self.history);
        Arc::new(move || {
            let generation = bridge.current_generation();
            let mut state = history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.synchronize(generation);
            let summary = state.summary();
            EditHistoryConfirmationSnapshot {
                generation: summary.generation,
                history_revision: summary.history_revision,
                undo_head_id: summary.undo_head_id,
            }
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

    /// MCPに見える先頭まとまり全体を、対象ID・履歴改訂と一緒に複製する。
    pub(crate) fn history_action_for_mcp(
        &self,
        direction: McpHistoryDirection,
    ) -> Option<McpHistoryAction> {
        self.bridge.with_current_generation(|generation| {
            let mut history = self
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            history.synchronize(generation);
            let direction: HistoryDirection = direction.into();
            let (head_id, revision) = history.history_cursor(direction);
            let action = history.prepare_action(HistoryActionRequest {
                direction,
                expected_head_id: head_id,
                expected_revision: revision,
            })?;
            Some(McpHistoryAction {
                direction: match direction {
                    HistoryDirection::Undo => McpHistoryDirection::Undo,
                    HistoryDirection::Redo => McpHistoryDirection::Redo,
                },
                head_id: action.entry_id,
                revision,
                group_name: action.group.name.clone(),
                source: action.group.source,
                records: action.steps.into_iter().map(|step| step.record).collect(),
            })
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
    let queue_state = Arc::new(AtomicU8::new(TICKET_WAITING));
    let _ = result_tx.send(Ok(QueueOutcome::Complete(QueuedEditResult::plain(
        Value::Null,
    ))));
    drop(cancelled);
    EditTicket {
        result,
        cancel: TicketCancellationGuard {
            _sender: cancel,
            queue_state: Arc::clone(&queue_state),
        },
        queue_state,
    }
}

async fn run_history_action(
    bridge: BridgeFacade,
    lease: BridgeLease,
    action: HistoryAction,
    execution: Option<Arc<mcp::McpExecution>>,
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

    let value = bridge
        .send_tracked_mcp(&lease, method, Some(params), execution.as_deref())
        .await?;
    validate_history_result(method, result_kind, &value)?;
    if let Some(execution) = execution {
        execution.finished(&value);
    }
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

#[allow(clippy::too_many_arguments)]
async fn run_actor(
    bridge: BridgeFacade,
    mut receiver: mpsc::Receiver<QueueItem>,
    mut sessions: watch::Receiver<Option<BridgeLease>>,
    mut shutdown: watch::Receiver<bool>,
    mut auth_changes: watch::Receiver<u64>,
    history: Arc<StdMutex<HistoryState>>,
    event_sink: Option<EditEventSink>,
    authorization: McpAuthorization,
) {
    let mut observed_generation = bridge.current_generation();
    let generation = observed_generation;
    history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .synchronize(generation);

    loop {
        let group_deadline = history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .group_deadline();
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
                if generation != observed_generation {
                    observed_generation = generation;
                    authorization.revoke();
                }
                let mut state = history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let before = state.summary();
                state.synchronize(generation);
                let after = state.summary();
                drop(state);
                if before != after {
                    emit_service_event(
                        event_sink.as_ref(),
                        EditServiceEvent::HistoryChanged(after),
                    );
                }
            }
            changed = auth_changes.changed() => {
                if changed.is_err() {
                    break;
                }
                let mut state = history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let before = state.summary();
                state.revoke_mcp_group();
                let after = state.summary();
                drop(state);
                if before != after {
                    emit_service_event(
                        event_sink.as_ref(),
                        EditServiceEvent::HistoryChanged(after),
                    );
                }
            }
            _ = groups::wait_deadline(group_deadline) => {
                let mut state = history.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if state.expire_group() {
                    let summary = state.summary();
                    drop(state);
                    emit_service_event(event_sink.as_ref(), EditServiceEvent::HistoryChanged(summary));
                }
            }
            item = receiver.recv() => {
                let Some(item) = item else { break; };
                if process_item(
                    &bridge,
                    &mut sessions,
                    &mut shutdown,
                    &mut auth_changes,
                    &history,
                    event_sink.as_ref(),
                    item,
                ).await {
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

// HTTP応答の受信者が消えても、actorが確定した結果を同じ要求IDへ残す。
// 一時的な結果チャネルは同じactor内だけで完結し、別taskや無界の待ち列を作らない。
struct McpActorCapture {
    execution: Arc<mcp::McpExecution>,
    history: Arc<StdMutex<HistoryState>>,
    finished: bool,
}

impl Drop for McpActorCapture {
    fn drop(&mut self) {
        if !self.finished {
            let history = self
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .summary();
            self.execution.record_result(
                false,
                Some(&BackendError::EditServiceStopping),
                true,
                Some(&history),
            );
        }
    }
}

async fn process_item(
    bridge: &BridgeFacade,
    sessions: &mut watch::Receiver<Option<BridgeLease>>,
    shutdown: &mut watch::Receiver<bool>,
    auth_changes: &mut watch::Receiver<u64>,
    history: &Arc<StdMutex<HistoryState>>,
    event_sink: Option<&EditEventSink>,
    mut item: QueueItem,
) -> bool {
    let execution = item
        .mcp_permit
        .as_ref()
        .map(|permit| permit.execution.clone())
        .or_else(|| match &item.action {
            QueuedAction::Group(
                GroupControl::BeginNamed { execution, .. }
                | GroupControl::EndNamed { execution, .. },
            ) => Some(execution.clone()),
            _ => None,
        });
    let Some(execution) = execution else {
        return process_item_inner(
            bridge,
            sessions,
            shutdown,
            auth_changes,
            history,
            event_sink,
            item,
        )
        .await;
    };
    let mut capture = McpActorCapture {
        execution: execution.clone(),
        history: history.clone(),
        finished: false,
    };
    let (sender, mut receiver) = oneshot::channel();
    let reply = std::mem::replace(&mut item.result, sender);
    let stopping = process_item_inner(
        bridge,
        sessions,
        shutdown,
        auth_changes,
        history,
        event_sink,
        item,
    )
    .await;
    let result = receiver
        .try_recv()
        .unwrap_or(Err(BackendError::EditServiceStopping));
    if !matches!(result, Ok(QueueOutcome::Reconfirm)) {
        let summary = history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .summary();
        execution.record_result(result.is_ok(), result.as_ref().err(), true, Some(&summary));
    }
    capture.finished = true;
    let _ = reply.send(result);
    stopping
}

async fn process_item_inner(
    bridge: &BridgeFacade,
    sessions: &mut watch::Receiver<Option<BridgeLease>>,
    shutdown: &mut watch::Receiver<bool>,
    auth_changes: &mut watch::Receiver<u64>,
    history: &Arc<StdMutex<HistoryState>>,
    event_sink: Option<&EditEventSink>,
    mut item: QueueItem,
) -> bool {
    let mcp_execution = item
        .mcp_permit
        .as_ref()
        .map(|permit| permit.execution.clone());
    if item
        .auth_lease
        .as_ref()
        .is_some_and(|lease| !lease.is_current())
    {
        let _ = item.result.send(Err(BackendError::McpAuthorizationRevoked));
        return false;
    }
    if matches!(
        item.cancelled.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    ) {
        let _ = item.result.send(Err(BackendError::EditCancelled));
        return false;
    }
    if matches!(item.action, QueuedAction::Group(_) | QueuedAction::Discard) {
        return process_control_item(bridge, history, event_sink, item);
    }
    if !bridge.is_current(item.lease.generation) {
        let outcome =
            if item.source == EditSource::Ui && matches!(item.action, QueuedAction::History(_)) {
                Ok(QueuedEditResult::plain(Value::Null))
            } else {
                Err(BackendError::NotConnected)
            };
        let _ = item.result.send(outcome.map(QueueOutcome::Complete));
        return false;
    }

    if let Some(QueuedMcpPermit { reads, permit, .. }) = item.mcp_permit.take() {
        let Some(auth) = item.auth_lease.as_ref() else {
            let _ = item.result.send(Err(BackendError::McpAuthorizationRevoked));
            return false;
        };
        let current_history = match &permit.operation {
            crate::mcp::authorization::McpWriteOperation::History(action) => {
                mcp::current_history(bridge, history, action.direction)
            }
            _ => None,
        };
        let validated = tokio::select! {
            biased;
            _ = shutdown.changed() => {
                let _ = item.result.send(Err(BackendError::EditServiceStopping));
                return true;
            },
            _ = auth.authorization_cancelled() => Err("MCP要求の許可が失効しました。".to_owned()),
            _ = auth.request_cancelled() => Err("MCP要求が取り消されました。".to_owned()),
            _ = &mut item.cancelled => Err("MCP要求が取り消されました。".to_owned()),
            _ = tokio::time::sleep_until(auth.request_deadline()) => Err("MCP要求の全体期限を超えました。".to_owned()),
            result = reads.revalidate_queued_permit(auth, permit, item.lease.generation, current_history) => result,
        };
        match validated {
            Ok(true) => {}
            Ok(false) => {
                let _ = item.result.send(Ok(QueueOutcome::Reconfirm));
                return false;
            }
            Err(message) => {
                let _ = item.result.send(Err(BackendError::Request { message }));
                return false;
            }
        }
    }

    let unsupported_scene_edit = item
        .history_request
        .as_ref()
        .is_some_and(HistoryRequest::is_scene_structure_edit);
    let mut prepared_history = None;
    let event_request = item.event_request.clone();
    let event_input = item.event_input.clone();
    let event_source = item.source;
    let event_kind = item.kind;
    let event_sequence = item.sequence;
    let event_generation = item.lease.generation;
    let retry_action = matches!(item.action, QueuedAction::Retry);
    let (prepared_action, history_before) = {
        let current_generation = bridge.current_generation();
        let mut state = history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.synchronize(current_generation);
        let history_before = state.summary();
        if state.is_pending() && !retry_action {
            let _ = item.result.send(Err(BackendError::Request {
                message: "編集履歴に部分失敗が残っています。保留中は編集・取り消し・実行制御を受け付けません。"
                    .to_owned(),
            }));
            return false;
        }
        if !retry_action {
            // 画面が捕捉した同じ先頭だけを、閉鎖による改訂の変化に追従させる。
            let current_cursor = match &item.action {
                QueuedAction::History(request) => {
                    state.history_cursor(request.direction)
                        == (request.expected_head_id, request.expected_revision)
                }
                _ => false,
            };
            state.expire_group();
            state.close_group_unless(item.group_token);
            if current_cursor {
                if let QueuedAction::History(request) = &mut item.action {
                    (request.expected_head_id, request.expected_revision) =
                        state.history_cursor(request.direction);
                }
            }
            if let Some(token) = item.group_token {
                if !state.has_group(token, item.lease.generation) {
                    let after = state.summary();
                    drop(state);
                    if after != history_before {
                        emit_service_event(event_sink, EditServiceEvent::HistoryChanged(after));
                    }
                    let _ = item.result.send(Err(BackendError::Request {
                        message: "まとまりIDが現在の接続世代または所有中のまとまりと一致しません。"
                            .to_owned(),
                    }));
                    return false;
                }
            }
        }
        if let Some(request) = item.history_request.take() {
            match state.prepare(request, item.source, item.lease.generation) {
                Ok(prepared) => prepared_history = Some(prepared),
                Err(error) => {
                    let after = state.summary();
                    drop(state);
                    if after != history_before {
                        emit_service_event(event_sink, EditServiceEvent::HistoryChanged(after));
                    }
                    let _ = item.result.send(Err(error));
                    return false;
                }
            }
        }
        let prepared_action = match &item.action {
            QueuedAction::History(request) => {
                let Some(action) = state.prepare_action(*request) else {
                    let after = state.summary();
                    drop(state);
                    if after != history_before {
                        emit_service_event(event_sink, EditServiceEvent::HistoryChanged(after));
                    }
                    let _ = item
                        .result
                        .send(Ok(QueueOutcome::Complete(QueuedEditResult::plain(
                            Value::Null,
                        ))));
                    return false;
                };
                Some(action)
            }
            QueuedAction::Retry => {
                let Some(action) = state.prepare_retry() else {
                    let _ = item.result.send(Err(BackendError::Request {
                        message:
                            "保留中のまとまりは再試行できません。状態を確認して破棄してください。"
                                .to_owned(),
                    }));
                    return false;
                };
                Some(action)
            }
            _ => None,
        };
        (prepared_action, history_before)
    };

    if let Some(trace) = &mcp_execution {
        if let Some(plan) = &prepared_action {
            trace.display_group(history::group_id(
                plan.group.key.generation,
                plan.group.key.sequence,
            ));
        } else if prepared_history.is_some() || item.group_token.is_some() {
            trace.display_group(history::group_id(
                item.lease.generation,
                item.group_token.unwrap_or(item.sequence),
            ));
        }
    }
    let grouped_action = prepared_action
        .as_ref()
        .is_some_and(|action| action.grouped || retry_action || item.source == EditSource::Mcp);
    let group_events = Arc::new(StdMutex::new(Vec::new()));
    if item
        .auth_lease
        .as_ref()
        .is_some_and(|lease| !lease.is_current())
    {
        let _ = item.result.send(Err(BackendError::McpAuthorizationRevoked));
        return false;
    }
    if item
        .queue_state
        .compare_exchange(
            TICKET_WAITING,
            TICKET_STARTED,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        let _ = item.result.send(Err(BackendError::EditCancelled));
        return false;
    }
    let action: EditFuture = match item.action {
        QueuedAction::Edit(action) => action(item.lease.clone()),
        QueuedAction::History(_) | QueuedAction::Retry => {
            let Some(plan) = prepared_action.clone() else {
                let _ = item
                    .result
                    .send(Ok(QueueOutcome::Complete(QueuedEditResult::plain(
                        Value::Null,
                    ))));
                return false;
            };
            if grouped_action {
                Box::pin(run_group_history_action(
                    plan,
                    GroupActionContext {
                        bridge: bridge.clone(),
                        lease: item.lease.clone(),
                        history: Arc::clone(history),
                        source: item.source,
                        sequence: item.sequence,
                        events: Arc::clone(&group_events),
                        retrying: retry_action,
                        auth_lease: item.auth_lease.clone(),
                        execution: mcp_execution.clone(),
                    },
                )) as EditFuture
            } else {
                Box::pin(run_history_action(
                    bridge.clone(),
                    item.lease.clone(),
                    plan,
                    None,
                )) as EditFuture
            }
        }
        QueuedAction::Group(_) | QueuedAction::Discard => {
            unreachable!("制御要求は列の先頭で処理する")
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
            changed = auth_changes.changed() => {
                if changed.is_err() {
                    break (Err(BackendError::McpAuthorizationRevoked), false);
                }
                let mut state = history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let before = state.summary();
                state.revoke_mcp_group();
                let after = state.summary();
                drop(state);
                if before != after {
                    emit_service_event(
                        event_sink,
                        EditServiceEvent::HistoryChanged(after),
                    );
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
                        if grouped_action {
                            Ok(value)
                        } else if let Some(plan) = prepared_action.clone() {
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
                                state.record_success_in_group(
                                    prepared,
                                    HistoryOperationContext {
                                        source: item.source,
                                        kind: item.kind,
                                        sequence: item.sequence,
                                        generation: item.lease.generation,
                                        group_token: item.group_token,
                                    },
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
                            if item.group_token.is_some() && value.value.get("accepted").and_then(Value::as_bool) != Some(true) {
                                state.close_group_unless(None);
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
                                if item.group_token.is_some() {
                                    state.close_group_unless(None);
                                }
                                if let Some(plan) = prepared_action.as_ref().filter(|_| !grouped_action) {
                                    state.finish_action_failure(plan);
                                } else if matches!(
                                    error,
                                    BackendError::Engine { ref code, .. }
                                        if code == "METHOD_NOT_SUPPORTED"
                                ) && unsupported_scene_edit
                                {
                                    state.mark_edit_unsupported();
                                }
                                if item.kind != EditKind::RuntimeControl && !grouped_action && mcp_execution.as_ref().is_some_and(|trace| trace.is_unknown()) {
                                    state.mark_mcp_outcome_unknown(item.sequence, item.lease.generation);
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

    let history_after = history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .summary();
    for payload in group_events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain(..)
    {
        emit_service_event(event_sink, EditServiceEvent::Applied(payload));
    }
    if !grouped_action {
        if let (Some(sink), Ok(result)) = (event_sink, outcome.as_ref()) {
            if history_after.applied_revision != history_before.applied_revision
                && result.value.get("accepted").and_then(Value::as_bool) == Some(true)
                && event_kind != EditKind::RuntimeControl
            {
                if let Some(details) = edit_event_details(
                    event_request.as_ref(),
                    event_input.as_ref(),
                    prepared_action.as_ref(),
                    &result.value,
                ) {
                    let group_sequence = item
                        .group_token
                        .or_else(|| {
                            prepared_action
                                .as_ref()
                                .map(|action| action.group.key.sequence)
                        })
                        .unwrap_or(event_sequence);
                    emit_service_event(
                        Some(sink),
                        EditServiceEvent::Applied(EditAppliedDto {
                            operation: details.operation,
                            object_id: details.object_id,
                            property: details.property,
                            value: details.value,
                            new_id: details.new_id,
                            source: source_dto(event_source),
                            group_id: history::group_id(event_generation, group_sequence),
                            generation: event_generation,
                            sequence: event_sequence,
                            history_revision: history_after.history_revision,
                            applied_revision: history_after.applied_revision,
                        }),
                    );
                }
            }
        }
    }
    if history_after != history_before {
        emit_service_event(event_sink, EditServiceEvent::HistoryChanged(history_after));
    }
    let _ = item.result.send(outcome.map(QueueOutcome::Complete));
    stopping
}

fn process_control_item(
    bridge: &BridgeFacade,
    history: &Arc<StdMutex<HistoryState>>,
    event_sink: Option<&EditEventSink>,
    item: QueueItem,
) -> bool {
    if item
        .auth_lease
        .as_ref()
        .is_some_and(|lease| !lease.is_current())
    {
        let _ = item.result.send(Err(BackendError::McpAuthorizationRevoked));
        return false;
    }
    let before = history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .summary();
    let outcome = bridge.with_current_generation(|generation| {
        let mut state = history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.synchronize(generation);
        if generation != Some(item.lease.generation) {
            return Err(BackendError::NotConnected);
        }
        if item
            .auth_lease
            .as_ref()
            .is_some_and(|lease| !lease.is_current())
        {
            return Err(BackendError::McpAuthorizationRevoked);
        }
        if state.is_pending() && !matches!(item.action, QueuedAction::Discard) {
            return Err(BackendError::Request {
                message: "編集履歴に部分失敗が残っています。先に再試行または破棄してください。"
                    .to_owned(),
            });
        }
        state.expire_group();
        match item.action {
            QueuedAction::Group(GroupControl::BeginNamed { name, execution }) => {
                let value = state.begin_named_group(item.sequence, name, item.lease.generation)?;
                execution.display_group(history::group_id(item.lease.generation, item.sequence));
                execution.control_finished();
                Ok(QueuedEditResult::plain(value))
            }
            QueuedAction::Group(GroupControl::EndNamed { secret, execution }) => {
                let group = state.active_display_group_id();
                state.end_named_group(&secret)?;
                if let Some(group) = group {
                    execution.display_group(group);
                }
                execution.control_finished();
                Ok(QueuedEditResult::plain(serde_json::json!({"closed":true})))
            }
            QueuedAction::Group(GroupControl::Boundary { secret, keep }) => {
                let token = state.group_boundary(secret.as_deref(), keep)?;
                Ok(QueuedEditResult::plain(serde_json::json!(token)))
            }
            QueuedAction::Group(GroupControl::CloseFailed { token }) => {
                if state.has_group(token, item.lease.generation) {
                    state.close_group_unless(None);
                }
                Ok(QueuedEditResult::plain(Value::Null))
            }
            QueuedAction::Group(GroupControl::Begin {
                token,
                generation: expected_generation,
                name,
            }) if expected_generation == item.lease.generation => {
                state.begin_group(token, name, item.source, item.lease.generation)?;
                Ok(QueuedEditResult::plain(Value::Null))
            }
            QueuedAction::Group(GroupControl::End {
                token,
                generation: expected_generation,
            }) if expected_generation == item.lease.generation => {
                state.end_group(token)?;
                Ok(QueuedEditResult::plain(Value::Null))
            }
            QueuedAction::Discard => {
                let discarded = state.discard_pending_group()?;
                let value =
                    serde_json::to_value(discarded).map_err(|error| BackendError::Request {
                        message: format!("破棄結果を直列化できません: {error}"),
                    })?;
                Ok(QueuedEditResult::plain(value))
            }
            _ => Err(BackendError::Request {
                message: "編集制御要求の形式が不正です。".to_owned(),
            }),
        }
    });
    let after = history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .summary();
    if after != before {
        emit_service_event(event_sink, EditServiceEvent::HistoryChanged(after));
    }
    let _ = item.result.send(outcome.map(QueueOutcome::Complete));
    false
}

struct GroupActionContext {
    bridge: BridgeFacade,
    lease: BridgeLease,
    history: Arc<StdMutex<HistoryState>>,
    source: EditSource,
    sequence: u64,
    events: Arc<StdMutex<Vec<EditAppliedDto>>>,
    retrying: bool,
    auth_lease: Option<McpRequestLease>,
    execution: Option<Arc<mcp::McpExecution>>,
}

async fn run_group_history_action(
    action: HistoryAction,
    context: GroupActionContext,
) -> Result<QueuedEditResult, BackendError> {
    let GroupActionContext {
        bridge,
        lease,
        history,
        source,
        sequence,
        events,
        retrying,
        auth_lease,
        execution,
    } = context;
    let mut completed = if retrying {
        history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .summary()
            .pending_group
            .map_or(0, |pending| pending.completed_count)
    } else {
        0
    };
    let total = action.group.count;
    for queued_step in &action.steps {
        if auth_lease.as_ref().is_some_and(|lease| !lease.is_current()) {
            if completed == 0 {
                return Err(BackendError::EditCancelled);
            }
            history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .start_pending_group(&action, completed, false, false);
            return Err(BackendError::Request {
                message: format!("MCP要求が取り消されました。{completed}件の適用結果を確認済みです。残りの操作は開始していません。"),
            });
        }
        if !bridge.is_current(lease.generation) {
            return Err(BackendError::NotConnected);
        }
        let step = history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .refresh_action_step(action.direction, &action.group.key, queued_step.entry_id)
            .ok_or(BackendError::NotConnected)?;
        let step_action = HistoryAction {
            direction: action.direction,
            entry_id: step.entry_id,
            record: step.record.clone(),
            group: action.group.clone(),
            steps: vec![step.clone()],
            grouped: true,
        };
        let result = run_history_action(
            bridge.clone(),
            lease.clone(),
            step_action.clone(),
            execution.clone(),
        )
        .await;
        let value = match result {
            Ok(value) if value.value.get("accepted").and_then(Value::as_bool) == Some(true) => {
                value
            }
            Ok(_) => {
                let current_generation = bridge.current_generation();
                let mut state = history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.synchronize(current_generation);
                if current_generation != Some(lease.generation) {
                    return Err(BackendError::NotConnected);
                }
                state.start_pending_group(&action, completed, false, true);
                return Err(BackendError::Request {
                    message: "まとまり内の編集をエンジンが受け付けませんでした。未処理の操作は再試行できます。"
                        .to_owned(),
                });
            }
            Err(error) => {
                let current_generation = bridge.current_generation();
                let mut state = history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.synchronize(current_generation);
                if current_generation == Some(lease.generation) {
                    state.finish_group_failure(&action, completed, &error);
                }
                return Err(error);
            }
        };
        let current_generation = bridge.current_generation();
        let mut state = history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.synchronize(current_generation);
        if current_generation != Some(lease.generation) {
            return Err(BackendError::NotConnected);
        }
        match state.finish_group_step(
            &action,
            &step,
            HistoryOperationContext {
                source,
                kind: match action.direction {
                    HistoryDirection::Undo => EditKind::Undo,
                    HistoryDirection::Redo => EditKind::Redo,
                },
                sequence,
                generation: lease.generation,
                group_token: None,
            },
            &value.value,
        ) {
            Ok(true) => {}
            Ok(false) => {
                state.start_pending_group(&action, completed, true, false);
                return Err(BackendError::Request {
                    message: "まとまり内の編集結果を履歴へ反映できませんでした。結果確認まで再送しません。"
                        .to_owned(),
                });
            }
            Err(error) => {
                state.finish_group_failure(&action, completed, &error);
                return Err(error);
            }
        }
        completed = completed.saturating_add(1);
        if let Some(details) = edit_event_details(None, None, Some(&step_action), &value.value) {
            let summary = state.summary();
            events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(EditAppliedDto {
                    operation: details.operation,
                    object_id: details.object_id,
                    property: details.property,
                    value: details.value,
                    new_id: details.new_id,
                    source: source_dto(source),
                    group_id: history::group_id(
                        action.group.key.generation,
                        action.group.key.sequence,
                    ),
                    generation: lease.generation,
                    sequence,
                    history_revision: summary.history_revision,
                    applied_revision: summary.applied_revision,
                });
        }
    }
    if retrying {
        let mut state = history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.clear_completed_pending_group(&action.group.key);
    }
    Ok(QueuedEditResult::plain(serde_json::json!({
        "accepted": true,
        "groupId": history::group_id(action.group.key.generation, action.group.key.sequence),
        "completedCount": completed.min(total),
        "totalCount": total,
    })))
}

struct EditEventDetails {
    operation: String,
    object_id: Option<String>,
    property: Option<String>,
    value: Option<Value>,
    new_id: Option<String>,
}

fn source_dto(source: EditSource) -> EditSourceDto {
    match source {
        EditSource::Ui => EditSourceDto::Ui,
        EditSource::Mcp => EditSourceDto::Mcp,
    }
}

fn edit_event_details(
    request: Option<&HistoryRequest>,
    input: Option<&EditEventInput>,
    action: Option<&HistoryAction>,
    result: &Value,
) -> Option<EditEventDetails> {
    if let Some(action) = action {
        let (operation, object_id, property, value, new_id) =
            match (&action.record, action.direction) {
                (history::HistoryRecord::Create { created_id, .. }, HistoryDirection::Undo) => {
                    ("undo", Some(created_id.clone()), None, None, None)
                }
                (history::HistoryRecord::Create { .. }, HistoryDirection::Redo) => (
                    "redo",
                    None,
                    None,
                    None,
                    result
                        .get("newId")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                ),
                (history::HistoryRecord::Duplicate { created_id, .. }, HistoryDirection::Undo) => {
                    ("undo", Some(created_id.clone()), None, None, None)
                }
                (
                    history::HistoryRecord::Duplicate {
                        source_object_id, ..
                    },
                    HistoryDirection::Redo,
                ) => (
                    "redo",
                    Some(source_object_id.clone()),
                    None,
                    None,
                    result
                        .get("newId")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                ),
                (
                    history::HistoryRecord::Reparent {
                        object_id,
                        old_parent_id,
                        ..
                    },
                    HistoryDirection::Undo,
                ) => (
                    "undo",
                    Some(object_id.clone()),
                    Some("parentId".to_owned()),
                    Some(optional_string_value(old_parent_id.as_deref())),
                    None,
                ),
                (
                    history::HistoryRecord::Reparent {
                        object_id,
                        new_parent_id,
                        ..
                    },
                    HistoryDirection::Redo,
                ) => (
                    "redo",
                    Some(object_id.clone()),
                    Some("parentId".to_owned()),
                    Some(optional_string_value(new_parent_id.as_deref())),
                    None,
                ),
                (
                    history::HistoryRecord::SetProperty {
                        object_id,
                        property,
                        old_value,
                        ..
                    },
                    HistoryDirection::Undo,
                ) => (
                    "undo",
                    Some(object_id.clone()),
                    Some(property.clone()),
                    Some(
                        result
                            .get("appliedValue")
                            .cloned()
                            .unwrap_or_else(|| old_value.clone()),
                    ),
                    None,
                ),
                (
                    history::HistoryRecord::SetProperty {
                        object_id,
                        property,
                        new_value,
                        ..
                    },
                    HistoryDirection::Redo,
                ) => (
                    "redo",
                    Some(object_id.clone()),
                    Some(property.clone()),
                    Some(
                        result
                            .get("appliedValue")
                            .cloned()
                            .unwrap_or_else(|| new_value.clone()),
                    ),
                    None,
                ),
            };
        return Some(EditEventDetails {
            operation: operation.to_owned(),
            object_id,
            property,
            value,
            new_id,
        });
    }

    if let Some(request) = request {
        return Some(match request {
            HistoryRequest::CreateObject { .. } => EditEventDetails {
                operation: "createObject".to_owned(),
                object_id: None,
                property: None,
                value: None,
                new_id: result
                    .get("newId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            },
            HistoryRequest::DuplicateObject {
                source_object_id, ..
            } => EditEventDetails {
                operation: "duplicateObject".to_owned(),
                object_id: Some(source_object_id.clone()),
                property: None,
                value: None,
                new_id: result
                    .get("newId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            },
            HistoryRequest::DeleteObject { object_id } => EditEventDetails {
                operation: "deleteObject".to_owned(),
                object_id: Some(object_id.clone()),
                property: None,
                value: None,
                new_id: None,
            },
            HistoryRequest::ReparentObject {
                object_id,
                new_parent_id,
                ..
            } => EditEventDetails {
                operation: "reparentObject".to_owned(),
                object_id: Some(object_id.clone()),
                property: Some("parentId".to_owned()),
                value: Some(optional_string_value(new_parent_id.as_deref())),
                new_id: None,
            },
            HistoryRequest::SetProperty {
                object_id,
                property,
                requested_value,
                ..
            } => EditEventDetails {
                operation: "setProperty".to_owned(),
                object_id: Some(object_id.clone()),
                property: Some(property.clone()),
                value: Some(
                    result
                        .get("appliedValue")
                        .cloned()
                        .unwrap_or_else(|| requested_value.clone()),
                ),
                new_id: None,
            },
        });
    }

    input.map(|input| EditEventDetails {
        operation: input.operation.to_owned(),
        object_id: input.object_id.clone(),
        property: input.property.clone(),
        value: input.value.clone(),
        new_id: if input.operation == "componentAdd" {
            result
                .get("componentId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        } else {
            None
        },
    })
}

fn optional_string_value(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |value| Value::String(value.to_owned()))
}

fn emit_service_event(event_sink: Option<&EditEventSink>, event: EditServiceEvent) {
    if let Some(event_sink) = event_sink {
        event_sink(event);
    }
}

fn tauri_event_sink(app: AppHandle) -> EditEventSink {
    Arc::new(move |event| {
        let result = match event {
            EditServiceEvent::Applied(payload) => app.emit(events::EDIT_APPLIED, payload),
            EditServiceEvent::HistoryChanged(payload) => {
                app.emit(events::EDIT_HISTORY_CHANGED, payload)
            }
        };
        if let Err(error) = result {
            tracing::warn!(error = %error, "編集サービスのイベントを送信できませんでした");
        }
    })
}

/// 保留中のまとまりで未処理と確定した操作を再試行する。
#[tauri::command]
pub async fn edit_retry(edit_service: State<'_, EditService>) -> Result<Value, BackendError> {
    edit_service.retry_pending_from_ui().await
}

/// 保留中のまとまりを履歴から破棄し、適用済み変更が残るか返す。
#[tauri::command]
pub async fn edit_discard(
    edit_service: State<'_, EditService>,
) -> Result<EditDiscardResultDto, BackendError> {
    edit_service.discard_pending_from_ui().await
}

#[cfg(test)]
mod tests {
    include!("edit_service/mcp_tests.rs");
    include!("edit_service/writes_tests.rs");
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

    fn test_service_with_peer_and_authorization(
        capacity: usize,
        authorization: McpAuthorization,
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
            EditService::with_capacity_and_sink_and_authorization(
                bridge,
                capacity,
                None,
                authorization,
            ),
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

    fn enqueue_grouped_property(
        service: &EditService,
        group: EditGroupHandle,
        object_id: &str,
        property: &str,
        old_value: Value,
        new_value: Value,
    ) -> EditTicket {
        let request = HistoryRequest::SetProperty {
            object_id: object_id.to_owned(),
            property: property.to_owned(),
            requested_value: new_value,
            old_value: PriorCapture::InAction,
        };
        service
            .enqueue_recorded_in_group_from_mcp(
                group,
                EditKind::Edit,
                request,
                move |_| async move {
                    Ok(QueuedEditResult::with_prior(
                        serde_json::json!({ "accepted": true }),
                        history::HistoryPrior::Property(old_value),
                    ))
                },
            )
            .expect("まとまり内の編集を列へ入れる")
    }

    fn enqueue_grouped_create(
        service: &EditService,
        group: EditGroupHandle,
        parent_id: Option<String>,
        kind: Option<String>,
        new_id: String,
    ) -> EditTicket {
        let request = HistoryRequest::CreateObject {
            parent_id: parent_id.clone(),
            kind: kind.clone(),
        };
        service
            .enqueue_recorded_in_group_from_mcp(
                group,
                EditKind::Edit,
                request,
                move |_| async move {
                    Ok(QueuedEditResult::plain(serde_json::json!({
                        "accepted": true,
                        "newId": new_id,
                    })))
                },
            )
            .expect("まとまり内の作成を列へ入れる")
    }

    fn enqueue_grouped_duplicate(
        service: &EditService,
        group: EditGroupHandle,
        source_object_id: String,
        parent_id: Option<String>,
        new_id: String,
    ) -> EditTicket {
        let request = HistoryRequest::DuplicateObject {
            source_object_id,
            parent_id,
        };
        service
            .enqueue_recorded_in_group_from_mcp(
                group,
                EditKind::Edit,
                request,
                move |_| async move {
                    Ok(QueuedEditResult::plain(serde_json::json!({
                        "accepted": true,
                        "newId": new_id,
                    })))
                },
            )
            .expect("まとまり内の複製を列へ入れる")
    }

    fn enqueue_grouped_reparent(
        service: &EditService,
        group: EditGroupHandle,
        object_id: String,
        new_parent_id: Option<String>,
        old_parent_id: Option<String>,
    ) -> EditTicket {
        let request = HistoryRequest::ReparentObject {
            object_id,
            new_parent_id,
            old_parent: PriorCapture::InAction,
        };
        service
            .enqueue_recorded_in_group_from_mcp(
                group,
                EditKind::Edit,
                request,
                move |_| async move {
                    Ok(QueuedEditResult::with_prior(
                        serde_json::json!({ "accepted": true }),
                        history::HistoryPrior::Parent(old_parent_id),
                    ))
                },
            )
            .expect("まとまり内の親変更を列へ入れる")
    }

    fn success(value: Value) -> Result<Value, BackendError> {
        Ok(value)
    }

    #[tokio::test]
    async fn named_group_undo_runs_reverse_and_redo_runs_forward_once() {
        let (service, _control, handle, mut peer) = test_service_with_peer(8);
        let group = service
            .begin_group_from_mcp("色をまとめて変更".to_owned())
            .await
            .expect("名前付きまとまりが始まる");
        for (object_id, old, new) in [
            ("first", "white", "red"),
            ("second", "white", "green"),
            ("third", "white", "blue"),
        ] {
            enqueue_grouped_property(
                &service,
                group,
                object_id,
                "color",
                serde_json::json!(old),
                serde_json::json!(new),
            )
            .result()
            .await
            .expect("まとまり内の編集が適用される");
        }
        service
            .end_group_from_mcp(group)
            .await
            .expect("まとまりが閉じる");

        let summary = service.history_summary();
        let display = summary.undo_group.expect("まとまりの要約がある");
        assert_eq!(display.name, "色をまとめて変更");
        assert_eq!(display.source, EditSourceDto::Mcp);
        assert_eq!(display.count, 3);
        assert!(display.created_at > 0);

        let undo = queue_history_action(&service, HistoryDirection::Undo);
        for expected_object in ["third", "second", "first"] {
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, "object.setProperty");
            assert_eq!(
                params
                    .as_ref()
                    .and_then(|params| params.get("objectId"))
                    .and_then(Value::as_str),
                Some(expected_object)
            );
            respond(
                &mut peer,
                id,
                serde_json::json!({ "accepted": true, "appliedValue": "white" }),
            )
            .await;
        }
        assert!(undo.result().await.is_ok());
        let summary = service.history_summary();
        assert!(!summary.pending);
        assert!(summary.can_redo);
        assert_eq!(
            summary.redo_group.as_ref().map(|group| group.count),
            Some(3)
        );
        assert!(tokio::time::timeout(Duration::from_millis(30), peer.recv())
            .await
            .is_err());

        let redo = queue_history_action(&service, HistoryDirection::Redo);
        for expected_object in ["first", "second", "third"] {
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, "object.setProperty");
            assert_eq!(
                params
                    .as_ref()
                    .and_then(|params| params.get("objectId"))
                    .and_then(Value::as_str),
                Some(expected_object)
            );
            respond(
                &mut peer,
                id,
                serde_json::json!({ "accepted": true, "appliedValue": "new" }),
            )
            .await;
        }
        assert!(redo.result().await.is_ok());
        assert!(!service.history_summary().pending);
        assert!(tokio::time::timeout(Duration::from_millis(30), peer.recv())
            .await
            .is_err());
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn redo_remaps_created_ids_across_children_and_resumes_after_known_rejection() {
        let (service, _control, handle, mut peer) = test_service_with_peer(8);
        let group = service
            .begin_group_from_mcp("ノードを作成して設定".to_owned())
            .await
            .expect("まとまりが始まる");
        enqueue_grouped_create(
            &service,
            group,
            None,
            Some("Node".to_owned()),
            "old-parent".to_owned(),
        )
        .result()
        .await
        .expect("親ノードを作成する");
        enqueue_grouped_property(
            &service,
            group,
            "old-parent",
            "enabled",
            serde_json::json!(false),
            serde_json::json!("old-parent"),
        )
        .result()
        .await
        .expect("作成したノードの値を設定する");
        enqueue_grouped_create(
            &service,
            group,
            Some("old-parent".to_owned()),
            Some("Child".to_owned()),
            "old-child".to_owned(),
        )
        .result()
        .await
        .expect("子ノードを作成する");
        service
            .end_group_from_mcp(group)
            .await
            .expect("まとまりを閉じる");

        let undo = queue_history_action(&service, HistoryDirection::Undo);
        for (expected_method, expected_id) in [
            ("scene.deleteObject", "old-child"),
            ("object.setProperty", "old-parent"),
            ("scene.deleteObject", "old-parent"),
        ] {
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, expected_method);
            assert_eq!(
                params
                    .as_ref()
                    .and_then(|params| params.get("objectId"))
                    .and_then(Value::as_str),
                Some(expected_id)
            );
            respond(
                &mut peer,
                id,
                serde_json::json!({ "accepted": true, "appliedValue": false }),
            )
            .await;
        }
        assert!(undo.result().await.is_ok());

        let redo = queue_history_action(&service, HistoryDirection::Redo);
        let (create_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.createObject");
        assert_eq!(
            params
                .as_ref()
                .map(|params| params.get("kind").and_then(Value::as_str)),
            Some(Some("Node"))
        );
        respond(
            &mut peer,
            create_id,
            serde_json::json!({ "accepted": true, "newId": "fresh-parent" }),
        )
        .await;

        let (property_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "object.setProperty");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("objectId"))
                .and_then(Value::as_str),
            Some("fresh-parent")
        );
        assert_eq!(
            params.as_ref().and_then(|params| params.get("value")),
            Some(&serde_json::json!("old-parent"))
        );
        respond_method_not_supported(&mut peer, property_id).await;
        assert!(matches!(
            redo.result().await,
            Err(BackendError::Engine { .. })
        ));

        let pending = service.history_summary();
        let pending_group = pending.pending_group.expect("途中失敗が保留される");
        assert_eq!(pending_group.completed_count, 1);
        assert_eq!(pending_group.total_count, 3);
        assert!(pending_group.retry_allowed);
        assert!(!pending_group.outcome_unknown);

        let retry = service.retry_pending_from_ui();
        tokio::pin!(retry);
        let (retry_property_id, method, params) = tokio::select! {
            result = &mut retry => panic!("再試行前に完了した: {result:?}"),
            request = next_request(&mut peer) => request,
        };
        assert_eq!(method, "object.setProperty");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("objectId"))
                .and_then(Value::as_str),
            Some("fresh-parent")
        );
        respond(
            &mut peer,
            retry_property_id,
            serde_json::json!({ "accepted": true, "appliedValue": "old-parent" }),
        )
        .await;
        let (child_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.createObject");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("parentId"))
                .and_then(Value::as_str),
            Some("fresh-parent")
        );
        respond(
            &mut peer,
            child_id,
            serde_json::json!({ "accepted": true, "newId": "fresh-child" }),
        )
        .await;
        assert!(retry.await.is_ok());
        assert!(!service.history_summary().pending);

        let next_undo = queue_history_action(&service, HistoryDirection::Undo);
        for (expected_method, expected_id) in [
            ("scene.deleteObject", "fresh-child"),
            ("object.setProperty", "fresh-parent"),
            ("scene.deleteObject", "fresh-parent"),
        ] {
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, expected_method);
            assert_eq!(
                params
                    .as_ref()
                    .and_then(|params| params.get("objectId"))
                    .and_then(Value::as_str),
                Some(expected_id)
            );
            respond(
                &mut peer,
                id,
                serde_json::json!({ "accepted": true, "appliedValue": false }),
            )
            .await;
        }
        assert!(next_undo.result().await.is_ok());
        assert!(tokio::time::timeout(Duration::from_millis(30), peer.recv())
            .await
            .is_err());
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn duplicate_redo_remaps_reparent_target_and_the_next_undo_target() {
        let (service, _control, handle, mut peer) = test_service_with_peer(6);
        let group = service
            .begin_group_from_mcp("複製ノードを移動".to_owned())
            .await
            .expect("まとまりが始まる");
        enqueue_grouped_duplicate(
            &service,
            group,
            "source".to_owned(),
            None,
            "old-copy".to_owned(),
        )
        .result()
        .await
        .expect("複製が適用される");
        enqueue_grouped_reparent(
            &service,
            group,
            "old-copy".to_owned(),
            Some("destination".to_owned()),
            Some("original-parent".to_owned()),
        )
        .result()
        .await
        .expect("複製を移動する");
        service
            .end_group_from_mcp(group)
            .await
            .expect("まとまりを閉じる");

        let undo = queue_history_action(&service, HistoryDirection::Undo);
        for (expected_method, expected_object, expected_parent) in [
            ("scene.reparentObject", "old-copy", "original-parent"),
            ("scene.deleteObject", "old-copy", ""),
        ] {
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, expected_method);
            assert_eq!(
                params
                    .as_ref()
                    .and_then(|params| params.get("objectId"))
                    .and_then(Value::as_str),
                Some(expected_object)
            );
            if !expected_parent.is_empty() {
                assert_eq!(
                    params
                        .as_ref()
                        .and_then(|params| params.get("newParentId"))
                        .and_then(Value::as_str),
                    Some(expected_parent)
                );
            }
            respond(&mut peer, id, serde_json::json!({ "accepted": true })).await;
        }
        assert!(undo.result().await.is_ok());

        let redo = queue_history_action(&service, HistoryDirection::Redo);
        let (duplicate_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.duplicateObject");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("objectId"))
                .and_then(Value::as_str),
            Some("source")
        );
        respond(
            &mut peer,
            duplicate_id,
            serde_json::json!({ "accepted": true, "newId": "fresh-copy" }),
        )
        .await;
        let (reparent_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.reparentObject");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("objectId"))
                .and_then(Value::as_str),
            Some("fresh-copy")
        );
        respond(
            &mut peer,
            reparent_id,
            serde_json::json!({ "accepted": true }),
        )
        .await;
        assert!(redo.result().await.is_ok());

        let next_undo = queue_history_action(&service, HistoryDirection::Undo);
        let (id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.reparentObject");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("objectId"))
                .and_then(Value::as_str),
            Some("fresh-copy")
        );
        respond(&mut peer, id, serde_json::json!({ "accepted": true })).await;
        let (id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.deleteObject");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("objectId"))
                .and_then(Value::as_str),
            Some("fresh-copy")
        );
        respond(&mut peer, id, serde_json::json!({ "accepted": true })).await;
        assert!(next_undo.result().await.is_ok());
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn unknown_group_result_is_not_retried_and_discard_reports_remaining_changes() {
        let (service, _control, handle, mut peer) = test_service_with_peer(6);
        let group = service
            .begin_group_from_mcp("結果不明のまとまり".to_owned())
            .await
            .expect("まとまりが始まる");
        for object_id in ["first", "second"] {
            enqueue_grouped_property(
                &service,
                group,
                object_id,
                "visible",
                serde_json::json!(false),
                serde_json::json!(true),
            )
            .result()
            .await
            .expect("まとまりの編集が適用される");
        }
        service
            .end_group_from_mcp(group)
            .await
            .expect("まとまりを閉じる");

        let undo = queue_history_action(&service, HistoryDirection::Undo);
        let (first_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "object.setProperty");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("objectId"))
                .and_then(Value::as_str),
            Some("second")
        );
        respond(
            &mut peer,
            first_id,
            serde_json::json!({ "accepted": true, "appliedValue": false }),
        )
        .await;
        let (_unknown_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "object.setProperty");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("objectId"))
                .and_then(Value::as_str),
            Some("first")
        );
        drop(peer);
        assert!(matches!(
            undo.result().await,
            Err(BackendError::Request { .. })
        ));

        let pending = service
            .history_summary()
            .pending_group
            .expect("結果不明を保持する");
        assert_eq!(pending.completed_count, 1);
        assert_eq!(pending.total_count, 2);
        assert!(pending.outcome_unknown);
        assert!(!pending.retry_allowed);
        assert!(service.retry_pending_from_ui().await.is_err());
        let discarded = service
            .discard_pending_from_ui()
            .await
            .expect("破棄結果が返る");
        assert_eq!(discarded.group_id, pending.id);
        assert_eq!(discarded.completed_count, 1);
        assert_eq!(discarded.total_count, 2);
        assert!(discarded.outcome_unknown);
        assert!(discarded.changes_remain);
        let summary = service.history_summary();
        assert!(!summary.pending);
        assert!(!summary.can_undo);
        assert!(!summary.can_redo);
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn timed_out_group_result_is_held_until_reconnect_without_retrying() {
        let (service, control, handle, mut peer) = test_service_with_peer(6);
        let group = service
            .begin_group_from_mcp("timeout後に再送しないまとまり".to_owned())
            .await
            .expect("まとまりが始まる");
        for object_id in ["first", "second"] {
            enqueue_grouped_property(
                &service,
                group,
                object_id,
                "visible",
                serde_json::json!(false),
                serde_json::json!(true),
            )
            .result()
            .await
            .expect("まとまり内の編集が適用される");
        }
        service
            .end_group_from_mcp(group)
            .await
            .expect("まとまりを閉じる");

        let undo = queue_history_action(&service, HistoryDirection::Undo);
        let (applied_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "object.setProperty");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("objectId"))
                .and_then(Value::as_str),
            Some("second")
        );
        respond(
            &mut peer,
            applied_id,
            serde_json::json!({ "accepted": true, "appliedValue": false }),
        )
        .await;

        let (_unknown_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "object.setProperty");
        assert_eq!(
            params
                .as_ref()
                .and_then(|params| params.get("objectId"))
                .and_then(Value::as_str),
            Some("first")
        );
        assert!(matches!(
            undo.result().await,
            Err(BackendError::Request { .. })
        ));

        let pending = service
            .history_summary()
            .pending_group
            .expect("timeoutの結果不明を要約する");
        assert_eq!(pending.completed_count, 1);
        assert_eq!(pending.total_count, 2);
        assert!(pending.outcome_unknown);
        assert!(!pending.retry_allowed);
        assert!(service.retry_pending_from_ui().await.is_err());
        assert!(tokio::time::timeout(Duration::from_millis(30), peer.recv())
            .await
            .is_err());

        let (new_transport, _new_peer) = loopback_pair(4);
        let new_handle = Dispatcher::spawn(new_transport);
        control.set_generation(2, new_handle.clone());
        let summary = service.history_summary();
        assert_eq!(summary.generation, Some(2));
        assert!(!summary.pending);
        assert!(summary.undo_group.is_none());
        assert!(summary.redo_group.is_none());
        service.shutdown().await;
        handle.shutdown().await;
        new_handle.shutdown().await;
    }

    #[tokio::test]
    async fn generation_change_clears_pending_group_and_rejects_old_group_handle() {
        let (service, control, handle, mut peer) = test_service_with_peer(6);
        let group = service
            .begin_group_from_mcp("世代をまたがないまとまり".to_owned())
            .await
            .expect("まとまりが始まる");
        for object_id in ["first", "second"] {
            enqueue_grouped_property(
                &service,
                group,
                object_id,
                "visible",
                serde_json::json!(false),
                serde_json::json!(true),
            )
            .result()
            .await
            .expect("まとまりの編集が適用される");
        }
        service
            .end_group_from_mcp(group)
            .await
            .expect("まとまりを閉じる");
        let undo = queue_history_action(&service, HistoryDirection::Undo);
        let (id, method, _) = next_request(&mut peer).await;
        assert_eq!(method, "object.setProperty");
        respond_method_not_supported(&mut peer, id).await;
        assert!(matches!(
            undo.result().await,
            Err(BackendError::Engine { .. })
        ));
        assert!(service.history_summary().pending);

        control.set_generation(2, handle.clone());
        let summary = service.history_summary();
        assert_eq!(summary.generation, Some(2));
        assert!(!summary.pending);
        assert!(summary.undo_group.is_none());
        assert!(summary.redo_group.is_none());
        assert!(service.end_group_from_mcp(group).await.is_err());
        let stale_write = service.enqueue_recorded_in_group_from_mcp(
            group,
            EditKind::Edit,
            HistoryRequest::SetProperty {
                object_id: "first".to_owned(),
                property: "visible".to_owned(),
                requested_value: serde_json::json!(true),
                old_value: PriorCapture::InAction,
            },
            |_| async {
                Ok(QueuedEditResult::plain(
                    serde_json::json!({ "accepted": true }),
                ))
            },
        );
        assert!(matches!(
            stale_write,
            Err(BackendError::NotConnected | BackendError::McpAuthorizationRevoked)
        ));
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn forward_group_failure_keeps_only_the_successful_prefix() {
        let (service, _control, handle, mut peer) = test_service_with_peer(6);
        let group = service
            .begin_group_from_mcp("前半だけ適用".to_owned())
            .await
            .expect("まとまりが始まる");
        for object_id in ["first", "second"] {
            enqueue_grouped_property(
                &service,
                group,
                object_id,
                "enabled",
                serde_json::json!(false),
                serde_json::json!(true),
            )
            .result()
            .await
            .expect("成功分を記録する");
        }
        let rejected = service
            .enqueue_recorded_in_group_from_mcp(
                group,
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "third".to_owned(),
                    property: "enabled".to_owned(),
                    requested_value: serde_json::json!(true),
                    old_value: PriorCapture::InAction,
                },
                |_| async {
                    Err(BackendError::Engine {
                        code: "REJECTED".to_owned(),
                        message: "未適用".to_owned(),
                    })
                },
            )
            .expect("まとまり内の3件目を列へ入れる");
        assert!(matches!(
            rejected.result().await,
            Err(BackendError::Engine { .. })
        ));
        service
            .end_group_from_mcp(group)
            .await
            .expect_err("部分失敗で既に閉じたまとまりは拒否する");
        let summary = service.history_summary();
        assert_eq!(
            summary.undo_group.as_ref().map(|group| group.count),
            Some(2)
        );
        assert!(!summary.pending);

        let undo = queue_history_action(&service, HistoryDirection::Undo);
        for expected_object in ["second", "first"] {
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, "object.setProperty");
            assert_eq!(
                params
                    .as_ref()
                    .and_then(|params| params.get("objectId"))
                    .and_then(Value::as_str),
                Some(expected_object)
            );
            respond(
                &mut peer,
                id,
                serde_json::json!({ "accepted": true, "appliedValue": false }),
            )
            .await;
        }
        assert!(undo.result().await.is_ok());
        assert!(tokio::time::timeout(Duration::from_millis(30), peer.recv())
            .await
            .is_err());
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn history_summary_exposes_the_undo_head_and_display_group() {
        let (service, _control, _handle) = test_service(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            17,
            history::HistoryRecord::SetProperty {
                object_id: "object-1".to_owned(),
                property: "color".to_owned(),
                old_value: serde_json::json!("blue"),
                new_value: serde_json::json!("red"),
            },
        );

        let summary = service.history_summary();
        assert!(summary.can_undo);
        assert!(!summary.can_redo);
        assert_eq!(summary.undo_head_id, Some(17));
        assert_eq!(summary.undo_revision, summary.history_revision);
        let group = summary.undo_group.expect("先頭まとまりを表示する");
        assert_eq!(group.id, "edit-1-17");
        assert_eq!(group.name, "プロパティを変更");
        assert_eq!(group.source, EditSourceDto::Ui);
        assert_eq!(group.count, 1);
        assert!(group.created_at > 0);
        assert_eq!(summary.pending_group, None);
        service.shutdown().await;
    }

    #[tokio::test]
    async fn mcp_history_snapshot_contains_every_entry_in_the_ui_head_group() {
        let (service, _control, _handle) = test_service(4);
        {
            let mut history = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            history.synchronize(Some(1));
            history.seed_undo_group_for_test(
                500,
                vec![
                    HistoryRecord::SetProperty {
                        object_id: "inside-scope".to_owned(),
                        property: "visible".to_owned(),
                        old_value: Value::Bool(false),
                        new_value: Value::Bool(true),
                    },
                    HistoryRecord::SetProperty {
                        object_id: "outside-scope".to_owned(),
                        property: "visible".to_owned(),
                        old_value: Value::Bool(false),
                        new_value: Value::Bool(true),
                    },
                ],
            );
        }

        let action = service
            .history_action_for_mcp(McpHistoryDirection::Undo)
            .expect("人の先頭まとまりを捕捉する");
        assert_eq!(action.head_id, 501);
        assert_eq!(action.records.len(), 2);
        assert!(action.records.iter().any(|record| matches!(
            record,
            HistoryRecord::SetProperty { object_id, .. } if object_id == "inside-scope"
        )));
        assert!(action.records.iter().any(|record| matches!(
            record,
            HistoryRecord::SetProperty { object_id, .. } if object_id == "outside-scope"
        )));
        service.shutdown().await;
    }

    #[tokio::test]
    async fn pending_partial_failure_rejects_edits_undo_redo_and_runtime_controls() {
        let (service, _control, _handle) = test_service(8);
        {
            let mut state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.synchronize(Some(1));
            state.set_pending_for_test(true);
        }
        let invoked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut tickets = Vec::new();
        let invoked_action = Arc::clone(&invoked);
        tickets.push(
            service
                .enqueue_from_ui(EditKind::Edit, move |_| async move {
                    invoked_action.store(true, std::sync::atomic::Ordering::Release);
                    success(serde_json::json!({ "accepted": true }))
                })
                .expect("通常編集の受付が列に入る"),
        );
        for operation in ["play", "pause", "stop"] {
            let invoked_action = Arc::clone(&invoked);
            tickets.push(
                service
                    .enqueue_from_ui(EditKind::RuntimeControl, move |_| async move {
                        let _operation = operation;
                        invoked_action.store(true, std::sync::atomic::Ordering::Release);
                        success(serde_json::json!({ "accepted": true }))
                    })
                    .expect("実行制御の受付が列に入る"),
            );
        }
        let (_, revision) = service.history_cursor(HistoryDirection::Undo);
        tickets.push(
            service
                .enqueue_undo_from_ui(None, revision)
                .expect("取り消し要求を列に入れる"),
        );
        tickets.push(
            service
                .enqueue_redo_from_ui(None, revision)
                .expect("やり直し要求を列に入れる"),
        );

        for ticket in tickets {
            assert!(matches!(
                ticket.result().await,
                Err(BackendError::Request { message }) if message.contains("部分失敗")
            ));
        }
        assert!(!invoked.load(std::sync::atomic::Ordering::Acquire));
        let summary = service.history_summary();
        assert!(summary.pending);
        assert!(!summary.can_undo);
        assert!(!summary.can_redo);
        service.shutdown().await;
    }

    #[test]
    fn applied_property_event_uses_engine_value_and_ui_source_fields() {
        let request = HistoryRequest::SetProperty {
            object_id: "object-1".to_owned(),
            property: "color".to_owned(),
            requested_value: serde_json::json!("red"),
            old_value: PriorCapture::Ui(HistoryCapture {
                generation: 1,
                revision: 3,
                value: Some(serde_json::json!("blue")),
            }),
        };
        let details = edit_event_details(
            Some(&request),
            None,
            None,
            &serde_json::json!({ "accepted": true, "appliedValue": null }),
        )
        .expect("適用イベント情報が作られる");
        assert_eq!(details.operation, "setProperty");
        assert_eq!(details.object_id.as_deref(), Some("object-1"));
        assert_eq!(details.property.as_deref(), Some("color"));
        assert_eq!(details.value, Some(Value::Null));
        assert_eq!(source_dto(EditSource::Ui), EditSourceDto::Ui);
    }

    #[tokio::test]
    async fn accepted_ui_edit_emits_applied_details_and_history_summary() {
        let (transport, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(transport);
        let (bridge, _control) = test_edit_facade(1, handle.clone());
        let facade = bridge.clone();
        let emitted = Arc::new(TestMutex::new(Vec::new()));
        let event_log = Arc::clone(&emitted);
        let sink: EditEventSink = Arc::new(move |event| {
            event_log.lock().unwrap().push(event);
        });
        let service = EditService::with_capacity_and_sink(bridge, 8, Some(sink));
        let mut params = serde_json::Map::new();
        params.insert("kind".to_owned(), Value::String("Node".to_owned()));
        let ticket = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::CreateObject {
                    parent_id: None,
                    kind: Some("Node".to_owned()),
                },
                move |lease| async move {
                    let result = facade
                        .send_with_lease(&lease, "scene.createObject", Some(params))
                        .await?;
                    Ok(QueuedEditResult::plain(result))
                },
            )
            .expect("画面の編集要求が受け付けられる");
        let (request_id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.createObject");
        assert_eq!(
            params.and_then(|params| params.get("kind").cloned()),
            Some(Value::String("Node".to_owned()))
        );
        respond(
            &mut peer,
            request_id,
            serde_json::json!({ "accepted": true, "newId": "node-1" }),
        )
        .await;
        assert_eq!(
            ticket.result().await.expect("編集結果が返る"),
            serde_json::json!({ "accepted": true, "newId": "node-1" })
        );

        {
            let events = emitted.lock().unwrap();
            assert_eq!(events.len(), 2);
            match &events[0] {
                EditServiceEvent::Applied(payload) => {
                    assert_eq!(payload.operation, "createObject");
                    assert_eq!(payload.new_id.as_deref(), Some("node-1"));
                    assert_eq!(payload.source, EditSourceDto::Ui);
                    assert_eq!(payload.group_id, "edit-1-0");
                    assert_eq!(payload.generation, 1);
                    assert_eq!(payload.sequence, 0);
                    assert_eq!(payload.applied_revision, 1);
                    assert_eq!(payload.history_revision, 2);
                }
                EditServiceEvent::HistoryChanged(_) => panic!("適用イベントが先に発行される"),
            }
            match &events[1] {
                EditServiceEvent::HistoryChanged(summary) => {
                    assert!(summary.can_undo);
                    assert_eq!(summary.undo_head_id, Some(0));
                    assert_eq!(
                        summary.undo_group.as_ref().map(|group| group.count),
                        Some(1)
                    );
                }
                EditServiceEvent::Applied(_) => panic!("履歴要約イベントが後に発行される"),
            }
        }
        service.shutdown().await;
    }

    async fn respond_method_not_supported(peer: &mut LoopbackTransport, id: CorrelationId) {
        let error = BridgeError {
            code: ErrorCode::method_not_supported(),
            message: "unsupported".to_owned(),
            data: None,
        };
        peer.send(response_frame(id, ResponsePayload::Error(error)))
            .await
            .expect("エンジンの未対応応答を送信できる");
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
    async fn confirmation_wait_keeps_the_ui_queue_and_shutdown_available() {
        use crate::mcp::{
            authorization::McpWriteSettings, log_buffer::LogBuffer, reads::McpReadContext,
            thumbnail::McpThumbnailService, tool_catalog::McpToolCatalog, McpWriteMode,
        };

        let authorization = McpAuthorization::default();
        authorization
            .set_write_settings(McpWriteSettings {
                mode: McpWriteMode::Confirm,
                scene_root_id: None,
            })
            .expect("都度確認に切り替える");
        let (service, _control, handle, _peer) =
            test_service_with_peer_and_authorization(1, authorization.clone());
        let catalog = McpToolCatalog::default();
        catalog.set_connection(
            Some(1),
            &[
                serde_json::from_value(serde_json::json!({"name":"runtime.control"}))
                    .expect("実行制御の能力を作る"),
            ],
        );
        let context = McpReadContext::new(
            service.bridge.clone(),
            catalog,
            Arc::new(StdMutex::new(LogBuffer::default())),
            McpThumbnailService::default(),
        )
        .with_authorization(authorization.clone())
        .with_history_source(service.confirmation_history_source());
        let lease = authorization.current_lease();
        let broker = authorization.confirmations();
        let mut updates = broker.subscribe();
        let pending = tokio::spawn(async move {
            context
                .authorize_write(&lease, "runtime.play", &serde_json::json!({}))
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), updates.changed())
            .await
            .expect("確認待ちへ到達する")
            .expect("確認を通知する");
        let id = updates.borrow()[0].id.clone();
        assert!(service.history_lock_available());
        let edit = service
            .enqueue_from_ui(EditKind::Edit, |_| async {
                success(serde_json::json!({"accepted":true}))
            })
            .expect("確認待ち中もUI編集を受け付ける");
        tokio::time::timeout(Duration::from_secs(1), edit.result())
            .await
            .expect("承認せずにUI編集が完了する")
            .expect("UI編集が成功する");
        assert_eq!(service.history_summary().applied_revision, 1);
        assert_eq!(broker.pending()[0].id, id);
        tokio::time::timeout(Duration::from_secs(1), service.shutdown())
            .await
            .expect("確認待ち中も編集actorが終了する");
        authorization.revoke();
        assert!(pending.await.expect("確認要求が終了する").is_err());
        assert!(broker.pending().is_empty());
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn mcp_request_deadline_cancels_an_edit_before_the_actor_starts_it() {
        let authorization = McpAuthorization::default();
        let (service, _control, handle, mut peer) =
            test_service_with_peer_and_authorization(4, authorization.clone());
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let blocker = service
            .enqueue_from_ui(EditKind::Edit, move |_| async move {
                let _ = started.send(());
                let _ = release_rx.await;
                success(Value::Null)
            })
            .expect("先行要求を受け付ける");
        started_rx.await.expect("先行要求が開始する");

        let mut auth_lease = authorization.current_lease();
        auth_lease.set_request_deadline(tokio::time::Instant::now() + Duration::from_millis(40));
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let action_ran = Arc::clone(&ran);
        let result = service
            .submit_from_mcp_authorized(auth_lease, EditKind::Edit, move |_| async move {
                action_ran.store(true, Ordering::Release);
                success(Value::Null)
            })
            .await
            .expect_err("列待ちが全体期限を消費する");
        assert!(matches!(result, BackendError::Request { ref message }
            if message.contains("Bridge開始前") && message.contains("取り消しました")));

        release.send(()).expect("先行要求を解放する");
        blocker.result().await.expect("先行要求が完了する");
        assert!(!ran.load(Ordering::Acquire));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), peer.recv())
                .await
                .is_err()
        );
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn confirmation_and_queue_wait_share_the_same_125_second_request_deadline() {
        use crate::mcp::{
            authorization::McpWriteSettings, confirmation::tests::poll_pending,
            log_buffer::LogBuffer, reads::McpReadContext, thumbnail::McpThumbnailService,
            tool_catalog::McpToolCatalog, McpWriteMode,
        };
        let authorization = McpAuthorization::default();
        authorization
            .set_write_settings(McpWriteSettings {
                mode: McpWriteMode::Confirm,
                scene_root_id: None,
            })
            .expect("都度確認にする");
        let (service, _control, handle, _peer) =
            test_service_with_peer_and_authorization(4, authorization.clone());
        let catalog = McpToolCatalog::default();
        catalog.set_connection(
            Some(1),
            &[
                serde_json::from_value(serde_json::json!({"name":"runtime.control"}))
                    .expect("能力を作る"),
            ],
        );
        let context = McpReadContext::new(
            service.bridge.clone(),
            catalog,
            Arc::new(StdMutex::new(LogBuffer::default())),
            McpThumbnailService::default(),
        )
        .with_authorization(authorization.clone());
        let lease = authorization.current_lease();
        let started = tokio::time::Instant::now();
        let params = serde_json::json!({});
        let mut permission = Box::pin(context.authorize_write(&lease, "runtime.play", &params));
        poll_pending(permission.as_mut()).await;
        let id = authorization.confirmations().pending()[0].id.clone();
        tokio::time::advance(Duration::from_secs(119)).await;
        assert!(authorization.confirmations().approve(&id));
        permission.await.expect("119秒で承認される");

        let (release, release_rx) = oneshot::channel();
        let (started_tx, started_rx) = oneshot::channel();
        let blocker = service
            .enqueue_from_ui(EditKind::Edit, move |_| async move {
                let _ = started_tx.send(());
                let _ = release_rx.await;
                success(Value::Null)
            })
            .expect("UI操作を先に実行する");
        started_rx.await.expect("UI操作が開始する");
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let action_ran = ran.clone();
        let mut submit = Box::pin(service.submit_from_mcp_authorized(
            lease,
            EditKind::RuntimeControl,
            move |_| async move {
                action_ran.store(true, Ordering::Release);
                success(Value::Null)
            },
        ));
        poll_pending(submit.as_mut()).await;
        tokio::time::advance(Duration::from_secs(5)).await;
        poll_pending(submit.as_mut()).await;
        tokio::time::advance(Duration::from_secs(1)).await;
        let result = submit
            .await
            .expect_err("確認後の列待ちも同じ全体期限を使う");
        assert!(
            matches!(result, BackendError::Request { ref message } if message.contains("Bridge開始前"))
        );
        assert_eq!(
            tokio::time::Instant::now(),
            started + Duration::from_secs(125)
        );
        release.send(()).expect("UI操作を完了させる");
        blocker.result().await.expect("UI操作が完了する");
        service
            .enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
            .expect("列を進める")
            .result()
            .await
            .expect("後続UI操作が完了する");
        assert!(!ran.load(Ordering::Acquire));
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn started_mcp_edit_waits_for_bridge_result_before_advancing_the_queue() {
        let authorization = McpAuthorization::default();
        let (service, _control, handle, _peer) =
            test_service_with_peer_and_authorization(4, authorization.clone());
        let mut auth_lease = authorization.current_lease();
        auth_lease.set_request_deadline(tokio::time::Instant::now() + Duration::from_millis(60));
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let mut submit = Box::pin(service.submit_from_mcp_authorized(
            auth_lease,
            EditKind::Edit,
            move |_| async move {
                let _ = started.send(());
                let _ = release_rx.await;
                success(serde_json::json!({"accepted":true}))
            },
        ));
        tokio::select! {
            result = &mut submit => panic!("期限前に操作が終わった: {result:?}"),
            started = started_rx => started.expect("最初の操作が開始する"),
        }

        let (next_started, mut next_started_rx) = oneshot::channel();
        let next = service
            .enqueue_from_ui(EditKind::Edit, move |_| async move {
                let _ = next_started.send(());
                success(Value::Null)
            })
            .expect("次の操作を待ち列に入れる");
        let timeout_error = tokio::time::timeout(Duration::from_secs(1), &mut submit)
            .await
            .expect("要求期限で結果を返す")
            .expect_err("開始済み操作の期限切れを結果不明として返す");
        assert!(
            matches!(timeout_error, BackendError::Request { ref message }
            if message.contains("結果は不明") && message.contains("自動再送せず"))
        );
        assert!(matches!(
            next_started_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        release.send(()).expect("Bridge結果を返す");
        next.result()
            .await
            .expect("Bridge結果確認後に次の操作が進む");
        assert!(next_started_rx.try_recv().is_ok());
        assert_eq!(service.history_summary().applied_revision, 1);
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
    async fn actor_rejects_an_authenticated_request_queued_before_revocation() {
        let authorization = McpAuthorization::default();
        let (service, _control, handle, _peer) =
            test_service_with_peer_and_authorization(4, authorization.clone());
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let blocker = service
            .enqueue_from_ui(EditKind::Edit, move |_| async move {
                let _ = started.send(());
                let _ = release_rx.await;
                success(Value::Null)
            })
            .expect("先行要求を列へ入れる");
        started_rx.await.expect("先行要求がactorへ入る");

        let auth_lease = authorization.current_lease();
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ran_action = Arc::clone(&ran);
        let queued = service
            .enqueue_from_mcp_authorized(auth_lease.clone(), EditKind::Edit, move |_| async move {
                ran_action.store(true, std::sync::atomic::Ordering::Release);
                success(Value::Null)
            })
            .expect("MCP要求を認証改訂付きで列へ入れる");

        authorization.revoke();
        release.send(()).expect("先行要求を解放する");
        blocker.result().await.expect("先行要求が完了する");
        assert!(matches!(
            queued.result().await,
            Err(BackendError::McpAuthorizationRevoked)
        ));
        assert!(!ran.load(std::sync::atomic::Ordering::Acquire));
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn bridge_reconnect_revokes_a_previous_generation_write_permit() {
        let authorization = McpAuthorization::default();
        authorization
            .set_write_settings(crate::mcp::authorization::McpWriteSettings {
                mode: crate::mcp::McpWriteMode::Enabled,
                scene_root_id: None,
            })
            .expect("書き込み可にする");
        let (service, control, handle, _peer) =
            test_service_with_peer_and_authorization(4, authorization.clone());
        let lease = authorization.current_lease();
        let policy = authorization.write_policy_snapshot();
        let operation = crate::mcp::authorization::McpWriteOperation::Property {
            object_id: "node".to_owned(),
        };
        let permit = authorization
            .issue_write_permit(&lease, &policy, 1, operation.clone())
            .expect("旧接続のpermitを作る");
        tokio::time::sleep(Duration::from_millis(20)).await;

        control.set_generation(2, handle.clone());
        tokio::time::timeout(Duration::from_secs(1), async {
            while lease.is_current() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("再接続で旧要求を失効させる");
        assert_eq!(
            authorization.validate_write_permit(&permit, &lease, 1, &operation),
            Err(crate::mcp::authorization::ScopeError::AuthorizationRevoked)
        );

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn actor_preserves_an_in_flight_mcp_result_when_its_lease_is_revoked() {
        let authorization = McpAuthorization::default();
        let (service, _control, handle, _peer) =
            test_service_with_peer_and_authorization(4, authorization.clone());
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let ticket = service
            .enqueue_from_mcp_authorized(
                authorization.current_lease(),
                EditKind::Edit,
                move |_| async move {
                    let _ = started.send(());
                    release_rx.await.expect("取消後もBridge結果を待つ");
                    success(serde_json::json!({"accepted":true}))
                },
            )
            .expect("MCP要求を列へ入れる");
        started_rx.await.expect("Bridge操作が開始する");
        authorization.revoke();
        let next = service
            .enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
            .expect("後続UI操作を列へ入れる");
        assert_eq!(service.history_summary().applied_revision, 0);
        release.send(()).expect("失効後のBridge結果を返す");
        assert_eq!(
            ticket.result().await.expect("適用結果を保全する")["accepted"],
            true
        );
        next.result().await.expect("結果確認後にUI操作が進む");
        assert_eq!(service.history_summary().applied_revision, 1);
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn revoking_authorization_closes_an_open_mcp_group_and_rejects_its_id() {
        let authorization = McpAuthorization::default();
        let (service, _control, handle, _peer) =
            test_service_with_peer_and_authorization(4, authorization.clone());
        let auth_lease = authorization.current_lease();
        let group = service
            .begin_group_from_mcp_authorized("失効するまとまり".to_owned(), auth_lease.clone())
            .await
            .expect("MCPまとまりを開く");
        assert!(service
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .has_active_group());

        authorization.revoke();
        let current_lease = authorization.current_lease();
        assert!(matches!(
            service
                .end_group_from_mcp_authorized(group, current_lease)
                .await,
            Err(BackendError::McpAuthorizationRevoked)
        ));
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let active = service
                    .history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .has_active_group();
                if !active {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("失効通知が開いたまとまりを閉じる");
        assert!(!auth_lease.is_current());
        service.shutdown().await;
        handle.shutdown().await;
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
        respond_method_not_supported(&mut peer, id).await;
        assert!(matches!(
            redo.result().await,
            Err(BackendError::Engine { .. })
        ));
        assert_eq!(service.history_cursor(HistoryDirection::Redo).0, Some(60));
        let next_redo = queue_history_action(&service, HistoryDirection::Redo);
        let (id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.reparentObject");
        assert_eq!(
            serde_json::to_value(params).unwrap(),
            serde_json::json!({"objectId": "retained", "newParentId": "parent"})
        );
        respond(&mut peer, id, serde_json::json!({"accepted": true})).await;
        assert!(next_redo.result().await.is_ok());
        assert_eq!(service.history_cursor(HistoryDirection::Redo).0, None);

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn unsupported_structure_edit_makes_undo_a_noop() {
        let (service, _control, handle, mut peer) = test_service_with_peer(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            62,
            history::HistoryRecord::Create {
                created_id: "created".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        let bridge = service.bridge.clone();
        let edit = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::CreateObject {
                    parent_id: None,
                    kind: None,
                },
                move |lease| async move {
                    bridge
                        .send_with_lease(&lease, "scene.createObject", None)
                        .await
                        .map(QueuedEditResult::plain)
                },
            )
            .expect("構造編集を列へ入れられる");
        let (id, method, _) = next_request(&mut peer).await;
        assert_eq!(method, "scene.createObject");
        respond_method_not_supported(&mut peer, id).await;
        assert!(matches!(
            edit.result().await,
            Err(BackendError::Engine { .. })
        ));

        let undo = queue_history_action(&service, HistoryDirection::Undo);
        assert_eq!(
            undo.result().await.expect("未対応後のundoは無操作"),
            Value::Null
        );
        assert!(tokio::time::timeout(Duration::from_millis(30), peer.recv())
            .await
            .is_err());
        assert_eq!(service.history_cursor(HistoryDirection::Undo).0, Some(62));

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn unsupported_property_edit_keeps_undo_available() {
        let (service, _control, handle, mut peer) = test_service_with_peer(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            63,
            history::HistoryRecord::Create {
                created_id: "created".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        let bridge = service.bridge.clone();
        let revision = service.applied_history_revision().1;
        let edit = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::SetProperty {
                    object_id: "object".to_owned(),
                    property: "value".to_owned(),
                    requested_value: serde_json::json!(2),
                    old_value: history::PriorCapture::Ui(history::HistoryCapture {
                        generation: 1,
                        revision,
                        value: Some(serde_json::json!(1)),
                    }),
                },
                move |lease| async move {
                    let mut params = serde_json::Map::new();
                    params.insert("objectId".to_owned(), serde_json::json!("object"));
                    params.insert("property".to_owned(), serde_json::json!("value"));
                    params.insert("value".to_owned(), serde_json::json!(2));
                    bridge
                        .send_with_lease(&lease, "object.setProperty", Some(params))
                        .await
                        .map(QueuedEditResult::plain)
                },
            )
            .expect("値編集を列へ入れられる");
        let (id, method, _) = next_request(&mut peer).await;
        assert_eq!(method, "object.setProperty");
        respond_method_not_supported(&mut peer, id).await;
        assert!(matches!(
            edit.result().await,
            Err(BackendError::Engine { .. })
        ));

        let undo = queue_history_action(&service, HistoryDirection::Undo);
        let (id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.deleteObject");
        assert_eq!(
            serde_json::to_value(params).unwrap(),
            serde_json::json!({"objectId": "created"})
        );
        respond(&mut peer, id, serde_json::json!({"accepted": true})).await;
        assert!(undo.result().await.is_ok());

        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn unsupported_runtime_control_keeps_undo_available() {
        let (service, _control, handle, mut peer) = test_service_with_peer(4);
        seed_history(
            &service,
            HistoryDirection::Undo,
            64,
            history::HistoryRecord::Create {
                created_id: "created".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        let bridge = service.bridge.clone();
        let control = service
            .enqueue_from_ui(EditKind::RuntimeControl, move |lease| async move {
                bridge
                    .send_with_lease(&lease, "runtime.play", Some(serde_json::Map::new()))
                    .await
            })
            .expect("実行制御を列へ入れられる");
        let (id, method, _) = next_request(&mut peer).await;
        assert_eq!(method, "runtime.play");
        respond_method_not_supported(&mut peer, id).await;
        assert!(matches!(
            control.result().await,
            Err(BackendError::Engine { .. })
        ));

        let undo = queue_history_action(&service, HistoryDirection::Undo);
        let (id, method, _) = next_request(&mut peer).await;
        assert_eq!(method, "scene.deleteObject");
        respond(&mut peer, id, serde_json::json!({"accepted": true})).await;
        assert!(undo.result().await.is_ok());

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
    async fn stale_history_admission_does_not_clear_the_new_generation() {
        let (service, control, handle, _peer) = test_service_with_peer(4);
        control.set_generation(2, handle.clone());
        {
            let mut state = service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.synchronize(Some(2));
            state.seed_undo_record(
                82,
                history::HistoryRecord::Create {
                    created_id: "new-generation".to_owned(),
                    parent_id: None,
                    kind: None,
                },
            );
        }
        let (head_id, revision) = service.history_cursor(HistoryDirection::Undo);
        let request = HistoryActionRequest {
            direction: HistoryDirection::Undo,
            expected_head_id: head_id,
            expected_revision: revision,
        };

        assert!(!service.can_prepare_history_action(1, request));
        assert_eq!(service.history_snapshot().0, Some(2));
        assert_eq!(service.history_cursor(HistoryDirection::Undo).0, Some(82));
        assert_eq!(
            service
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .undo_records(),
            vec![history::HistoryRecord::Create {
                created_id: "new-generation".to_owned(),
                parent_id: None,
                kind: None,
            }]
        );

        service.shutdown().await;
        handle.shutdown().await;
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
    async fn engine_exit_bridge_disconnect_clears_both_histories() {
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
        // エンジン終了後にBridgeが切断されるときと同じ世代変更を流す。
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
    }

    #[tokio::test]
    async fn history_survives_workspace_close_without_bridge_generation_change() {
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
            // workspace_close は Bridge 世代を変えないため履歴を保持する。
            state.synchronize(Some(1));
            assert_eq!(state.undo_len(), 1);
            assert_eq!(state.redo_len(), 1);
        }
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn history_clears_on_editor_service_shutdown() {
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
