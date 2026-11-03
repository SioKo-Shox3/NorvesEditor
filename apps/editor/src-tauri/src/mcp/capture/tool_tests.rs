//! 撮影呼び出しを打ち切った後も、実行中の撮影が排他を保持することを確かめる。

use super::*;
use crate::bridge_state::{test_edit_facade, BridgeSessionTestControl};
use crate::mcp::{
    authorization::McpWriteSettings,
    log_buffer::LogBuffer,
    reads::McpReadContext,
    thumbnail::McpThumbnailService,
    tool_catalog::{McpToolCatalog, WritePermission},
    McpAuthorization, McpWriteMode,
};
use norves_bridge_core::{
    decode_typed, encode_envelope, CapabilityDescriptor, Envelope, EventName, ResponsePayload,
    ValidatedEnvelope, VersionString,
};
use norves_bridge_editor_client::{
    loopback_pair, DispatchHandle, Dispatcher, LoopbackTransport, Transport,
};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};
use std::sync::Mutex as StdMutex;
use tokio::time::{advance, pause, sleep, timeout};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("時刻を取得する")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "norves-capture-tool-test-{}-{stamp}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("一時ディレクトリを作成する");
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    context: McpReadContext,
    authorization: McpAuthorization,
    session: BridgeSessionTestControl,
    handle: DispatchHandle,
    peer: LoopbackTransport,
    store: CaptureStore,
    _directory: TestDir,
}

impl Fixture {
    fn new() -> Self {
        let directory = TestDir::new();
        let store = CaptureStore::new(directory.0.join("captures"));
        let (transport, peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(transport);
        let (bridge, session) = test_edit_facade(1, handle.clone());
        let catalog = McpToolCatalog::default();
        let capability: CapabilityDescriptor = serde_json::from_value(json!({
            "name": "viewport.capture"
        }))
        .expect("撮影能力を作成する");
        catalog.set_connection(Some(1), &[capability]);
        catalog.set_write_handlers_ready(true);
        let authorization = McpAuthorization::default();
        authorization
            .set_write_settings(McpWriteSettings {
                mode: McpWriteMode::Enabled,
                scene_root_id: None,
            })
            .expect("撮影を許可する");
        let context = McpReadContext::new(
            bridge,
            catalog,
            Arc::new(StdMutex::new(LogBuffer::default())),
            McpThumbnailService::default(),
        )
        .with_capture_store(store.clone())
        .with_authorization(authorization.clone());
        context.set_write_permission(WritePermission::Enabled);
        Self {
            context,
            authorization,
            session,
            handle,
            peer,
            store,
            _directory: directory,
        }
    }

    /// 道具から要求を送り、受け付け応答の後に呼び出しを取り消す。
    async fn send_and_cancel(&mut self, capture_id: &str) {
        let lease = self.authorization.current_lease();
        let cancel = lease.clone();
        let context = self.context.clone();
        let pending =
            tokio::spawn(async move { context.call_capture_tool(json!({}), Some(lease)).await });
        let frame = timeout(Duration::from_secs(2), self.peer.recv())
            .await
            .expect("撮影要求が届く")
            .expect("撮影要求を受信する")
            .expect("接続が開いている");
        let ValidatedEnvelope::Request { id, method, .. } =
            decode_typed(&frame).expect("撮影要求を復号する")
        else {
            panic!("撮影要求が必要です");
        };
        assert_eq!(method.as_str(), "viewport.captureDeterministic");
        let response: Envelope = ValidatedEnvelope::Response {
            version: VersionString::try_from("0.2".to_owned()).expect("有効なプロトコル版を使う"),
            id,
            payload: ResponsePayload::Result(json!({"captureId": capture_id})),
            session_id: None,
            seq: None,
        }
        .into();
        self.peer
            .send(encode_envelope(&response).expect("受け付け応答を符号化する"))
            .await
            .expect("撮影の受け付け応答を返す");
        cancel.cancel_request();
        let response = timeout(Duration::from_secs(2), pending)
            .await
            .expect("取り消した呼び出しが終了する")
            .expect("呼び出しタスクを回収する");
        assert_eq!(payload(&response)["outcome"], "unknown");
        assert_eq!(payload(&response)["automaticRetryAllowed"], false);
        assert!(self.store.try_begin().is_none(), "撮影中の排他権を保持する");
    }

    async fn complete_failed(&mut self, capture_id: &str) {
        let event: Envelope = ValidatedEnvelope::Event {
            version: VersionString::try_from("0.2".to_owned()).expect("有効なプロトコル版を使う"),
            event: EventName::try_from("viewport.captureCompleted".to_owned())
                .expect("有効な完了イベント名を使う"),
            params: Some(
                json!({
                    "captureId": capture_id,
                    "status": "failed",
                    "failure": {"code": "RENDER_FAILED", "message": "描画に失敗しました。"}
                })
                .as_object()
                .expect("完了イベントの引数を作る")
                .clone(),
            ),
            session_id: None,
            seq: None,
        }
        .into();
        self.peer
            .send(encode_envelope(&event).expect("完了イベントを符号化する"))
            .await
            .expect("撮影の失敗完了イベントを返す");
    }

    async fn shutdown(&self) {
        self.session.shutdown_captures().await;
        self.handle.shutdown().await;
    }
}

fn payload(response: &CallToolResult) -> &Value {
    response
        .structured_content
        .as_ref()
        .expect("撮影結果の構造化データがある")
}

async fn wait_until_released(store: &CaptureStore) {
    timeout(Duration::from_secs(2), async {
        loop {
            if store.try_begin().is_some() {
                break;
            }
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("撮影の排他権が解放される");
}

#[tokio::test]
async fn cancelled_capture_rejects_second_capture_without_sending() {
    let mut fixture = Fixture::new();
    fixture.send_and_cancel("capture-1").await;
    let response = fixture
        .context
        .call_capture_tool(json!({}), Some(fixture.authorization.current_lease()))
        .await;
    assert_eq!(payload(&response)["outcome"], "notApplied");
    assert_eq!(
        payload(&response)["detail"],
        "別の撮影が進行中です。完了してからやり直してください。"
    );
    let operations = fixture.context.operations.snapshot();
    let request_id = payload(&response)["requestId"]
        .as_str()
        .expect("要求IDがある");
    assert_eq!(
        operations
            .records
            .iter()
            .find(|operation| operation.request_id == request_id)
            .expect("拒否した操作の記録がある")
            .result,
        "rejected"
    );
    assert!(timeout(Duration::from_millis(30), fixture.peer.recv())
        .await
        .is_err());
    fixture.shutdown().await;
}

#[tokio::test]
async fn completion_after_cancellation_allows_next_capture() {
    let mut fixture = Fixture::new();
    fixture.send_and_cancel("capture-1").await;
    fixture.complete_failed("capture-1").await;
    wait_until_released(&fixture.store).await;
    fixture.send_and_cancel("capture-2").await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn generation_change_after_cancellation_releases_exclusion() {
    let mut fixture = Fixture::new();
    fixture.send_and_cancel("capture-1").await;
    assert!(
        fixture.store.try_begin().is_none(),
        "世代が変わるまで撮影の排他権を保持する"
    );
    fixture.session.set_generation(2, fixture.handle.clone());
    wait_until_released(&fixture.store).await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn exclusion_limit_after_cancellation_releases_exclusion() {
    let mut fixture = Fixture::new();
    fixture.send_and_cancel("capture-1").await;
    // ファイル準備と送信は実時間で済ませ、完了待ちだけを仮想時刻で進める。
    pause();
    advance(Duration::from_secs(126)).await;
    assert!(
        fixture.store.try_begin().is_none(),
        "MCP の全体期限を過ぎても撮影の排他権を保持する"
    );
    advance(CAPTURE_EXCLUSION_LIMIT - Duration::from_secs(126) + Duration::from_secs(1)).await;
    wait_until_released(&fixture.store).await;
    fixture.shutdown().await;
}
