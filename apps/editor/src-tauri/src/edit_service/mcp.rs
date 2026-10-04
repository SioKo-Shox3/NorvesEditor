//! MCP確認を列外で待ち、使い捨てpermitと検査済みの編集だけを共通列へ渡す。

use super::*;
use crate::mcp::{
    authorization::{McpWriteOperation, McpWritePermit},
    reads::McpReadContext,
};

pub(super) struct QueuedMcpPermit {
    pub(super) reads: McpReadContext,
    pub(super) permit: McpWritePermit,
    pub(super) execution: Arc<McpExecution>,
}

/// 表示用要求ごとの送信境界と確認済み件数。記録DTOへ承認IDや認証情報を渡さない。
#[derive(Default)]
pub(crate) struct McpExecution {
    state: AtomicU8,
    completed: std::sync::atomic::AtomicUsize,
    queue_state: StdMutex<Option<Arc<AtomicU8>>>,
    operation: Option<crate::mcp::operations::OperationHandle>,
    request_lease: Option<McpRequestLease>,
}

impl McpExecution {
    /// 編集・まとまり制御のticketを発行していなければ、後続のactor確定はない。
    pub(crate) fn never_queued(&self) -> bool {
        self.queue_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none()
    }

    pub(crate) fn with_operation(
        operation: crate::mcp::operations::OperationHandle,
        request_lease: Option<McpRequestLease>,
    ) -> Self {
        Self {
            operation: Some(operation),
            request_lease,
            ..Self::default()
        }
    }

    pub(crate) fn operation_snapshot(&self) -> Option<crate::dto::McpOperationDto> {
        self.operation
            .as_ref()
            .map(|operation| operation.snapshot())
    }

    pub(super) fn display_group(&self, group: String) {
        if let Some(operation) = &self.operation {
            operation.group(group);
        }
    }

    pub(super) fn control_finished(&self) {
        self.state.store(3, Ordering::Release);
    }

    pub(crate) fn record_result(
        &self,
        success: bool,
        error: Option<&BackendError>,
        actor_finished: bool,
        history: Option<&EditHistorySummaryDto>,
    ) {
        if actor_finished {
            // actorが戻った後なら、Bridge送信境界に達しなかったことを確定できる。
            let _ = self
                .state
                .compare_exchange(0, 4, Ordering::AcqRel, Ordering::Acquire);
        }
        let Some(operation) = &self.operation else {
            return;
        };
        let outcome = self.outcome(success);
        let result = crate::mcp::operations::result_kind(
            outcome,
            error,
            self.request_lease
                .as_ref()
                .is_some_and(|lease| tokio::time::Instant::now() >= lease.request_deadline()),
        );
        // 確認待ちの取消はRequest型でも返るため、説明文から原因を推測しない。
        let result = if outcome == "notApplied"
            && result != "timedOut"
            && self
                .request_lease
                .as_ref()
                .is_some_and(|lease| !lease.is_current() && lease.is_authorization_current())
        {
            "cancelled"
        } else {
            result
        };
        operation.finish(crate::mcp::operations::OperationUpdate {
            result,
            outcome,
            completed: self.completed(),
            pending: history.is_some_and(|history| history.pending),
            retry_allowed: history
                .and_then(|history| history.pending_group.as_ref())
                .is_some_and(|pending| pending.retry_allowed),
            actor_finished,
        });
    }

    pub(super) fn track_queue(&self, queue_state: Arc<AtomicU8>) {
        *self
            .queue_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(queue_state);
    }

    pub(crate) fn not_sent(&self) {
        self.state.store(4, Ordering::Release);
    }

    pub(crate) fn started(&self) {
        self.state.store(1, Ordering::Release);
    }

    pub(crate) fn rejected(&self) {
        self.state.store(2, Ordering::Release);
    }

    pub(crate) fn finished(&self, value: &Value) {
        if value.get("accepted").and_then(Value::as_bool) == Some(true) {
            self.completed.fetch_add(1, Ordering::AcqRel);
            self.state.store(3, Ordering::Release);
        } else {
            self.rejected();
        }
    }

    pub(crate) fn is_unknown(&self) -> bool {
        self.state.load(Ordering::Acquire) == 1
    }

    pub(crate) fn completed(&self) -> usize {
        self.completed.load(Ordering::Acquire)
    }

    pub(crate) fn outcome(&self, success: bool) -> &'static str {
        match self.state.load(Ordering::Acquire) {
            1 => "unknown",
            2 if self.completed() == 0 => "rejected",
            2 => "partial",
            3 if success => "applied",
            3 => "partial",
            _ if success => "noChange",
            0 if self
                .queue_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .is_some_and(|state| state.load(Ordering::Acquire) == TICKET_STARTED) =>
            {
                "unknown"
            }
            4 if self.completed() > 0 => "partial",
            _ => "notApplied",
        }
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum McpEditRequest {
    Bridge {
        method: &'static str,
        params: Value,
    },
    GroupedBridge {
        method: &'static str,
        params: Value,
        group_id: String,
    },
    BeginGroup {
        name: String,
    },
    EndGroup {
        group_id: String,
    },
    History(McpHistoryDirection),
}

#[allow(dead_code)]
impl EditService {
    /// 確認・再確認・列待ちを同じ要求期限で処理する。Bridge開始後の寿命はactorが所有する。
    pub(crate) async fn submit_confirmed_mcp(
        &self,
        reads: McpReadContext,
        lease: McpRequestLease,
        request: McpEditRequest,
    ) -> Result<Value, BackendError> {
        self.submit_tracked_mcp(reads, lease, request, Arc::new(McpExecution::default()))
            .await
    }

    pub(crate) async fn submit_tracked_mcp(
        &self,
        reads: McpReadContext,
        lease: McpRequestLease,
        request: McpEditRequest,
        execution: Arc<McpExecution>,
    ) -> Result<Value, BackendError> {
        let mut reads = reads
            .with_authorization(self.authorization.clone())
            .with_history_source(self.confirmation_history_source());
        let mut group_token = None;
        let future = async {
            group_token = match &request {
                McpEditRequest::BeginGroup { name } => {
                    if self.authorization.write_policy_snapshot().settings.mode
                        == crate::mcp::McpWriteMode::ReadOnly
                    {
                        return Err(BackendError::Request {
                            message: "書き込みが許可されていません。".to_owned(),
                        });
                    }
                    return self
                        .submit_group_control(
                            &lease,
                            GroupControl::BeginNamed {
                                name: name.clone(),
                                execution: execution.clone(),
                            },
                        )
                        .await;
                }
                McpEditRequest::EndGroup { group_id } => {
                    return self
                        .submit_group_control(
                            &lease,
                            GroupControl::EndNamed {
                                secret: group_id.clone(),
                                execution: execution.clone(),
                            },
                        )
                        .await;
                }
                McpEditRequest::GroupedBridge {
                    method, group_id, ..
                } => {
                    let keep = matches!(
                        *method,
                        "object.setProperty"
                            | "scene.createObject"
                            | "scene.duplicateObject"
                            | "scene.reparentObject"
                    );
                    self.submit_group_control(
                        &lease,
                        GroupControl::Boundary {
                            secret: Some(group_id.clone()),
                            keep,
                        },
                    )
                    .await?
                    .as_u64()
                }
                McpEditRequest::History(_) => None,
                McpEditRequest::Bridge { .. } => None,
            };
            loop {
                let permit = match &request {
                    McpEditRequest::Bridge { method, params }
                    | McpEditRequest::GroupedBridge { method, params, .. } => {
                        reads.authorize_write(&lease, method, params).await
                    }
                    McpEditRequest::History(direction) => {
                        let Some(action) = self.history_action_for_mcp(*direction) else {
                            // redo対象がない場合も、開いたまとまりは列上で閉じる。
                            let active = self
                                .history
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .has_active_group();
                            if active {
                                self.submit_group_control(
                                    &lease,
                                    GroupControl::Boundary {
                                        secret: None,
                                        keep: false,
                                    },
                                )
                                .await?;
                            }
                            return Ok(Value::Null);
                        };
                        let history = Arc::clone(&self.history);
                        let bridge = self.bridge.clone();
                        let direction = *direction;
                        reads
                            .authorize_history(&lease, action, move || {
                                current_history(&bridge, &history, direction)
                            })
                            .await
                    }
                    McpEditRequest::BeginGroup { .. } | McpEditRequest::EndGroup { .. } => {
                        unreachable!()
                    }
                }
                .map_err(|message| BackendError::Request { message })?;
                let ticket = self.enqueue_confirmed_mcp(
                    reads.clone(),
                    lease.clone(),
                    &request,
                    permit,
                    execution.clone(),
                    group_token,
                )?;
                // ticketの取消CASと同じ状態を見る。列開始とBridge送信の間も未適用とは断言しない。
                execution.track_queue(ticket.queue_state.clone());
                match ticket.outcome_until(lease.request_deadline()).await? {
                    QueueOutcome::Complete(result) => return Ok(result.value),
                    QueueOutcome::Reconfirm => reads = reads.require_reconfirmation(),
                }
            }
        };
        let result = tokio::select! {
            biased;
            result = future => result,
            _ = lease.authorization_cancelled() => Err(BackendError::McpAuthorizationRevoked),
            _ = lease.request_cancelled() => Err(BackendError::EditCancelled),
        };
        // 旧値取得・許可確認で止まった場合も、成功済みの部分だけを閉じる。
        // 要求の通信断そのものからは永続所有者の終了を推定しない。
        if result.is_err() && lease.is_current() {
            if let Some(token) = group_token {
                let _ = self
                    .submit_group_control(&lease, GroupControl::CloseFailed { token })
                    .await;
            }
        }
        result
    }

    async fn submit_group_control(
        &self,
        lease: &McpRequestLease,
        control: GroupControl,
    ) -> Result<Value, BackendError> {
        if !lease.is_current() {
            return Err(BackendError::McpAuthorizationRevoked);
        }
        let execution = match &control {
            GroupControl::BeginNamed { execution, .. }
            | GroupControl::EndNamed { execution, .. } => Some(execution.clone()),
            _ => None,
        };
        let (ticket, _, _) =
            self.enqueue_control_with_auth(EditSource::Mcp, Some(lease.clone()), |_, _| {
                QueuedAction::Group(control)
            })?;
        // 発行に成功した制御ticketだけを追跡し、取消後もactorの確定を待つ。
        if let Some(execution) = execution {
            execution.track_queue(ticket.queue_state.clone());
        }
        ticket
            .outcome_until(lease.request_deadline())
            .await?
            .into_value()
    }

    #[allow(clippy::too_many_arguments)]
    fn enqueue_confirmed_mcp(
        &self,
        reads: McpReadContext,
        lease: McpRequestLease,
        request: &McpEditRequest,
        permit: McpWritePermit,
        execution: Arc<McpExecution>,
        group_token: Option<u64>,
    ) -> Result<EditTicket, BackendError> {
        let mut history_request = None;
        let mut event_input = None;
        let (kind, action) = match request {
            McpEditRequest::History(direction) => {
                let McpWriteOperation::History(action) = &permit.operation else {
                    return Err(BackendError::McpAuthorizationRevoked);
                };
                let direction: HistoryDirection = (*direction).into();
                (
                    if direction == HistoryDirection::Undo {
                        EditKind::Undo
                    } else {
                        EditKind::Redo
                    },
                    QueuedAction::History(HistoryActionRequest {
                        direction,
                        expected_head_id: Some(action.head_id),
                        expected_revision: action.revision,
                    }),
                )
            }
            McpEditRequest::Bridge { method, params }
            | McpEditRequest::GroupedBridge { method, params, .. } => {
                let prior = permit.captured_prior();
                history_request = match &permit.operation {
                    McpWriteOperation::Property { object_id } => {
                        if prior.is_none() {
                            return Err(BackendError::Request {
                                message: "編集前の値を取得できません。対象を再取得してください。"
                                    .to_owned(),
                            });
                        }
                        Some(HistoryRequest::SetProperty {
                            object_id: object_id.clone(),
                            property: required_string(params, "property")?,
                            requested_value: params.get("value").cloned().ok_or_else(|| {
                                BackendError::Request {
                                    message: "設定する値がありません。".to_owned(),
                                }
                            })?,
                            old_value: PriorCapture::InAction,
                        })
                    }
                    McpWriteOperation::Create { parent_id } => Some(HistoryRequest::CreateObject {
                        parent_id: parent_id.clone(),
                        kind: params
                            .get("kind")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    }),
                    McpWriteOperation::Duplicate {
                        object_id,
                        new_parent_id,
                    } => Some(HistoryRequest::DuplicateObject {
                        source_object_id: object_id.clone(),
                        parent_id: new_parent_id.clone(),
                    }),
                    McpWriteOperation::Reparent {
                        object_id,
                        new_parent_id,
                    } => Some(HistoryRequest::ReparentObject {
                        object_id: object_id.clone(),
                        new_parent_id: new_parent_id.clone(),
                        old_parent: PriorCapture::InAction,
                    }),
                    McpWriteOperation::Delete { object_id } => Some(HistoryRequest::DeleteObject {
                        object_id: object_id.clone(),
                    }),
                    McpWriteOperation::ComponentAdd { object_id } => {
                        event_input = Some(EditEventInput {
                            operation: "componentAdd",
                            object_id: Some(object_id.clone()),
                            property: None,
                            value: None,
                        });
                        None
                    }
                    McpWriteOperation::ComponentRemove { component_id } => {
                        event_input = Some(EditEventInput {
                            operation: "componentRemove",
                            object_id: Some(component_id.clone()),
                            property: None,
                            value: None,
                        });
                        None
                    }
                    McpWriteOperation::RuntimeControl => None,
                    McpWriteOperation::History(_) => {
                        return Err(BackendError::McpAuthorizationRevoked)
                    }
                };
                let kind = if permit.operation == McpWriteOperation::RuntimeControl {
                    EditKind::RuntimeControl
                } else {
                    EditKind::Edit
                };
                let bridge = self.bridge.clone();
                let method = *method;
                let params = params
                    .as_object()
                    .cloned()
                    .ok_or_else(|| BackendError::Request {
                        message: "編集引数がobjectではありません。".to_owned(),
                    })?;
                let auth = lease.clone();
                let trace = execution.clone();
                (
                    kind,
                    QueuedAction::Edit(Box::new(move |bridge_lease| {
                        Box::pin(async move {
                            // この検査から送信までawaitを挟まず、送信後のfutureは取消で破棄しない。
                            if !auth.is_current() {
                                trace.not_sent();
                                return Err(BackendError::McpAuthorizationRevoked);
                            }
                            let value = bridge
                                .send_tracked_mcp(&bridge_lease, method, Some(params), Some(&trace))
                                .await?;
                            validate_write_result(method, &value)?;
                            trace.finished(&value);
                            Ok(QueuedEditResult { value, prior })
                        })
                    })),
                )
            }
            McpEditRequest::BeginGroup { .. } | McpEditRequest::EndGroup { .. } => unreachable!(),
        };
        self.enqueue_action_with_context(
            EnqueueContext {
                source: EditSource::Mcp,
                kind,
                group_token,
                event_request: history_request.clone(),
                history_request,
                event_input,
                auth_lease: Some(lease),
                mcp_permit: Some(QueuedMcpPermit {
                    reads,
                    permit,
                    execution,
                }),
            },
            action,
        )
    }
}

fn required_string(params: &Value, key: &str) -> Result<String, BackendError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| BackendError::Request {
            message: format!("編集引数 {key} がありません。"),
        })
}

fn validate_write_result(method: &str, value: &Value) -> Result<(), BackendError> {
    use norves_bridge_editor_client as client;
    let result = match method {
        "scene.createObject" => client::parse_create_object_result(value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "scene.duplicateObject" => client::parse_duplicate_object_result(value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "scene.deleteObject" => client::parse_delete_object_result(value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "scene.reparentObject" => client::parse_reparent_object_result(value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "object.setProperty" => client::parse_set_property_result(value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "component.add" => client::parse_component_add_result(value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "component.remove" => client::parse_component_remove_result(value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        _ => {
            return if value.get("accepted").and_then(Value::as_bool).is_some() {
                Ok(())
            } else {
                Err(BackendError::Request {
                    message: "実行制御の応答形式が不正です。".to_owned(),
                })
            }
        }
    };
    result.map_err(|error| BackendError::Request {
        message: format!("{method}の応答形式が不正です: {error}"),
    })
}

pub(super) fn current_history(
    bridge: &BridgeFacade,
    history: &StdMutex<HistoryState>,
    direction: McpHistoryDirection,
) -> Option<McpHistoryAction> {
    bridge.with_current_generation(|generation| {
        let mut state = history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.synchronize(generation);
        let (head_id, revision) = state.history_cursor(direction.into());
        let action = state.prepare_action(HistoryActionRequest {
            direction: direction.into(),
            expected_head_id: head_id,
            expected_revision: revision,
        })?;
        Some(McpHistoryAction {
            direction,
            head_id: action.entry_id,
            revision,
            group_name: action.group.name,
            source: action.group.source,
            records: action.steps.into_iter().map(|step| step.record).collect(),
        })
    })
}
