//! HTTP要求IDとSDKの取消を、その要求のリースだけへ結び付ける。

use super::*;
use rmcp::model::{ClientJsonRpcMessage, ClientNotification, ClientRequest, GetMeta, RequestId};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct RequestKey {
    session: Option<String>,
    id: RequestId,
}

fn namespace(headers: &HeaderMap, request: &ClientRequest) -> Option<String> {
    // rmcpのis_legacy_requestと同じく、Initializeは旧版、要求metaはヘッダーより優先する。
    let meta = request.get_meta();
    let current = !matches!(request, ClientRequest::InitializeRequest(_))
        && (meta
            .missing_required_keys(&ProtocolVersion::V_2026_07_28)
            .is_empty()
            || meta.protocol_version().map_or_else(
                || is_current_protocol(headers),
                |version| !version.has_initialize(),
            ));
    if current {
        None
    } else {
        Some(
            headers
                .get("Mcp-Session-Id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_owned(),
        )
    }
}

fn is_current_protocol(headers: &HeaderMap) -> bool {
    headers
        .get("Mcp-Protocol-Version")
        .and_then(|value| value.to_str().ok())
        == Some(ProtocolVersion::V_2026_07_28.as_str())
}

pub(super) fn request_key(headers: &HeaderMap, body: &[u8]) -> Option<RequestKey> {
    let ClientJsonRpcMessage::Request(request) = serde_json::from_slice(body).ok()? else {
        return None;
    };
    Some(RequestKey {
        session: namespace(headers, &request.request),
        id: request.id,
    })
}

pub(super) fn cancellation_key(headers: &HeaderMap, body: &[u8]) -> Option<RequestKey> {
    let ClientJsonRpcMessage::Notification(notification) = serde_json::from_slice(body).ok()?
    else {
        return None;
    };
    let ClientNotification::CancelledNotification(cancelled) = notification.notification else {
        return None;
    };
    Some(RequestKey {
        session: if is_current_protocol(headers) {
            None
        } else {
            Some(
                headers
                    .get("Mcp-Session-Id")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_owned(),
            )
        },
        id: cancelled.params.request_id?,
    })
}

impl RequestKey {
    pub(super) fn is_stateless(&self) -> bool {
        self.session.is_none()
    }
}

/// SDKがfutureを破棄してもリースを取り消す。監視taskを要求の外へ残さない。
pub(super) async fn run<T>(
    context: &rmcp::service::RequestContext<rmcp::service::RoleServer>,
    future: impl Future<Output = Result<T, rmcp::ErrorData>>,
) -> Result<T, rmcp::ErrorData> {
    let Some(lease) = request_authorization_from_context(context) else {
        return future.await;
    };
    let guard = lease.request_cancellation.clone().drop_guard();
    let message = tokio::select! {
        biased;
        _ = context.ct.cancelled() => "MCP要求がSDKから取り消されました。",
        _ = lease.request_cancelled() => "MCP要求が取り消されました。",
        _ = lease.authorization_cancelled() => "MCP要求の許可が失効しました。",
        _ = time::sleep_until(lease.request_deadline()) => "MCP要求の全体期限を超えました。",
        result = future => {
            guard.disarm();
            return result;
        },
    };
    Err(rmcp::ErrorData::internal_error(message, None))
}
