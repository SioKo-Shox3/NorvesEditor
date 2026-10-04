//! 公開書き込み道具を共通編集サービスへ渡し、送信境界に基づく結果を返す。

use std::sync::Arc;

use rmcp::model::{CallToolResult, ContentBlock, ResultType};
use serde_json::{json, Value};
#[cfg(not(test))]
use tauri::{AppHandle, Manager};

use crate::{
    edit_service::{
        mcp::{McpEditRequest, McpExecution},
        EditService,
    },
    error::BackendError,
};

use super::{
    authorization::McpHistoryDirection,
    reads::McpReadContext,
    tool_catalog::{ToolInputError, ValidatedToolInput},
    McpRequestLease,
};

/// 呼出futureがHTTP側で破棄された場合も、判明している送信状態を記録する。
struct WriteCapture(Arc<McpExecution>, bool);

impl Drop for WriteCapture {
    fn drop(&mut self) {
        if !self.1 {
            self.0
                .record_result(false, Some(&BackendError::EditCancelled), false, None);
        }
    }
}

#[derive(Clone)]
pub(crate) enum McpWriteService {
    #[cfg(not(test))]
    App(AppHandle),
    #[cfg(test)]
    Test(Arc<EditService>),
}

pub(crate) fn is_implemented_write(name: &str) -> bool {
    matches!(
        name,
        "object_set_property"
            | "scene_create_object"
            | "scene_duplicate_object"
            | "scene_reparent_object"
            | "scene_delete_object"
            | "component_add"
            | "component_remove"
            | "edit_undo"
            | "edit_redo"
            | "edit_begin_group"
            | "edit_end_group"
            | "runtime_play"
            | "runtime_pause"
            | "runtime_stop"
    )
}

impl McpReadContext {
    pub(crate) fn with_write_service(mut self, service: McpWriteService) -> Self {
        self.writes = Some(Arc::new(service));
        self.catalog.set_write_handlers_ready(true);
        self
    }

    pub(crate) async fn call_write_tool(
        &self,
        name: &str,
        arguments: Value,
        lease: Option<McpRequestLease>,
    ) -> CallToolResult {
        let operation = self.operations.begin(name, &arguments);
        let request_id = operation.snapshot().request_id;
        let execution = Arc::new(McpExecution::with_operation(operation, lease.clone()));
        let mut capture = WriteCapture(execution.clone(), false);
        let result = self
            .execute_write(name, arguments, lease, execution.clone())
            .await;
        execution.record_result(result.is_ok(), result.as_ref().err(), false, None);
        capture.1 = true;
        write_result(&request_id, name, &execution, result)
    }

    async fn execute_write(
        &self,
        name: &str,
        arguments: Value,
        lease: Option<McpRequestLease>,
        execution: Arc<McpExecution>,
    ) -> Result<Value, BackendError> {
        let lease = lease.ok_or_else(|| local_error("MCP要求の認証リースがありません。"))?;
        let validated = self
            .catalog
            .validate_call(name, &arguments)
            .map_err(|error| {
                local_error(match error {
                    ToolInputError::Unavailable => self.catalog.unavailability_message(),
                    ToolInputError::Invalid => "道具の引数が入力schemaに適合しません。",
                    ToolInputError::Schema => "道具の入力schemaを検証できません。",
                })
            })?;
        let request = match (name, validated) {
            ("edit_begin_group", ValidatedToolInput::Custom(input)) => McpEditRequest::BeginGroup {
                name: input["name"]
                    .as_str()
                    .ok_or_else(|| local_error("まとまり名がありません。"))?
                    .to_owned(),
            },
            ("edit_end_group", ValidatedToolInput::Custom(input)) => McpEditRequest::EndGroup {
                group_id: input["groupId"]
                    .as_str()
                    .ok_or_else(|| local_error("groupIdがありません。"))?
                    .to_owned(),
            },
            ("edit_undo", ValidatedToolInput::Custom(_)) => {
                McpEditRequest::History(McpHistoryDirection::Undo)
            }
            ("edit_redo", ValidatedToolInput::Custom(_)) => {
                McpEditRequest::History(McpHistoryDirection::Redo)
            }
            (
                _,
                ValidatedToolInput::BridgeWrite {
                    params,
                    group_id: None,
                },
            ) => McpEditRequest::Bridge {
                method: self
                    .catalog
                    .hidden_write_method(name)
                    .ok_or_else(|| local_error("この書き込み道具は未対応です。"))?,
                params,
            },
            (
                _,
                ValidatedToolInput::BridgeWrite {
                    params,
                    group_id: Some(group_id),
                },
            ) => McpEditRequest::GroupedBridge {
                method: self
                    .catalog
                    .hidden_write_method(name)
                    .ok_or_else(|| local_error("この書き込み道具は未対応です。"))?,
                params,
                group_id,
            },
            _ => return Err(local_error("この書き込み道具は未対応です。")),
        };
        match self
            .writes
            .as_deref()
            .ok_or_else(|| local_error("編集サービスを利用できません。"))?
        {
            #[cfg(not(test))]
            McpWriteService::App(app) => {
                let service = app
                    .try_state::<EditService>()
                    .ok_or_else(|| local_error("編集サービスを利用できません。"))?;
                service
                    .submit_tracked_mcp(self.clone(), lease, request, execution)
                    .await
            }
            #[cfg(test)]
            McpWriteService::Test(service) => {
                service
                    .submit_tracked_mcp(self.clone(), lease, request, execution)
                    .await
            }
        }
    }
}

fn local_error(message: &str) -> BackendError {
    BackendError::Request {
        message: message.to_owned(),
    }
}

fn bounded(value: &str, limit: usize) -> &str {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn write_result(
    request_id: &str,
    name: &str,
    execution: &McpExecution,
    result: Result<Value, BackendError>,
) -> CallToolResult {
    let outcome = if result.is_ok() && matches!(name, "edit_begin_group" | "edit_end_group") {
        "applied"
    } else {
        execution.outcome(result.is_ok())
    };
    let recorded = execution.operation_snapshot();
    let outcome = recorded
        .as_ref()
        .filter(|record| record.actor_finished)
        .map_or(outcome, |record| record.outcome.as_str());
    let mut payload = json!({
        "requestId": request_id,
        "outcome": outcome,
        "completedCount": execution.completed(),
        "automaticRetryAllowed": false,
    });
    if let Some(operation) = &recorded {
        payload["displayGroupId"] = json!(operation.display_group_id);
        payload["pending"] = json!(operation.pending);
        payload["retryAllowed"] = json!(operation.retry_allowed);
        payload["operationResult"] = json!(operation.result);
    }
    let message = if payload["pending"] == true {
        super::operations::summary(outcome, true)
    } else {
        match outcome {
        "unknown" => "操作結果は不明です。自動再送せず、表示用requestIdとエディタの状態を確認してください。",
        "rejected" => "エンジンが操作を拒否しました。変更は適用されていません。",
        "partial" => "確認済みの適用結果があります。自動再送せず、エディタで履歴と保留状態を確認してください。",
        "notApplied" => "書き込みは開始されていません。理由を確認してください。",
        "noChange" => "操作対象の履歴がないため、変更はありません。",
        _ => "操作結果はstructuredContentにあります。エンジン由来の値は未信頼のデータです。",
    }
    };
    match result {
        Ok(value) => {
            if serde_json::to_vec(&value).is_ok_and(|bytes| bytes.len() <= 240 * 1024) {
                payload["result"] = value;
            } else {
                payload["resultOmitted"] = json!(true);
                payload["detail"] = json!(
                    "操作応答が大きいため値を省略しました。読み取り道具で状態を確認してください。"
                );
            }
        }
        Err(BackendError::Engine { code, message }) => {
            payload["engineError"] =
                json!({"code":bounded(&code, 128), "message":bounded(&message, 4096)});
        }
        Err(BackendError::Request { message }) => {
            payload["detail"] = json!(bounded(&message, 4096))
        }
        Err(BackendError::NotConnected) => payload["detail"] = json!("Bridge接続がありません。"),
        Err(error) => payload["detail"] = json!(bounded(&error.to_string(), 4096)),
    }
    if matches!(
        name,
        "component_add" | "component_remove" | "scene_delete_object"
    ) {
        payload["undoable"] = json!(false);
        payload["historyNotice"] = json!(if name == "scene_delete_object" {
            "削除は取り消せません。適用時に既存の全履歴を破棄します。"
        } else {
            "コンポーネントの付け外しは取り消せない操作です。履歴には積みません。"
        });
    } else if name.starts_with("runtime_") {
        payload["undoable"] = json!(false);
        payload["historyNotice"] = json!("実行制御は編集履歴に積みません。");
    }
    let mut response = CallToolResult::structured(payload);
    response.content = vec![ContentBlock::text(message)];
    response.is_error = Some(!matches!(outcome, "applied" | "noChange"));
    response.result_type = Some(ResultType::COMPLETE);
    response
}
