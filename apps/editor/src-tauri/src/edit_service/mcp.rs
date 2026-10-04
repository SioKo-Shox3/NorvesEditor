//! MCP確認を列外で待ち、使い捨てpermitと検査済みの編集だけを共通列へ渡す。

use super::*;
use crate::mcp::{
    authorization::{McpWriteOperation, McpWritePermit},
    reads::McpReadContext,
};

pub(super) struct QueuedMcpPermit {
    pub(super) reads: McpReadContext,
    pub(super) permit: McpWritePermit,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum McpEditRequest {
    Bridge { method: &'static str, params: Value },
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
        let mut reads = reads
            .with_authorization(self.authorization.clone())
            .with_history_source(self.confirmation_history_source());
        let future = async {
            loop {
                let permit = match &request {
                    McpEditRequest::Bridge { method, params } => {
                        reads.authorize_write(&lease, method, params).await
                    }
                    McpEditRequest::History(direction) => {
                        let Some(action) = self.history_action_for_mcp(*direction) else {
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
                }
                .map_err(|message| BackendError::Request { message })?;
                let ticket =
                    self.enqueue_confirmed_mcp(reads.clone(), lease.clone(), &request, permit)?;
                match ticket.outcome_until(lease.request_deadline()).await? {
                    QueueOutcome::Complete(result) => return Ok(result.value),
                    QueueOutcome::Reconfirm => reads = reads.require_reconfirmation(),
                }
            }
        };
        tokio::select! {
            biased;
            _ = lease.authorization_cancelled() => Err(BackendError::McpAuthorizationRevoked),
            _ = lease.request_cancelled() => Err(BackendError::EditCancelled),
            result = future => result,
        }
    }

    fn enqueue_confirmed_mcp(
        &self,
        reads: McpReadContext,
        lease: McpRequestLease,
        request: &McpEditRequest,
        permit: McpWritePermit,
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
            McpEditRequest::Bridge { method, params } => {
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
                (
                    kind,
                    QueuedAction::Edit(Box::new(move |bridge_lease| {
                        Box::pin(async move {
                            // この検査から送信までawaitを挟まず、送信後のfutureは取消で破棄しない。
                            if !auth.is_current() {
                                return Err(BackendError::McpAuthorizationRevoked);
                            }
                            let value = bridge
                                .send_with_lease(&bridge_lease, method, Some(params))
                                .await?;
                            validate_write_result(method, &value)?;
                            Ok(QueuedEditResult { value, prior })
                        })
                    })),
                )
            }
        };
        self.enqueue_action_with_context(
            EnqueueContext {
                source: EditSource::Mcp,
                kind,
                group_token: None,
                event_request: history_request.clone(),
                history_request,
                event_input,
                auth_lease: Some(lease),
                mcp_permit: Some(QueuedMcpPermit { reads, permit }),
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
