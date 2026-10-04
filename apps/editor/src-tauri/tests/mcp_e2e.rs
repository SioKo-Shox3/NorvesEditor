//! 実mockと両版の実HTTPから、本番と共通の許可・確認・編集列・記録を検証する。

use norves_editor_lib::mcp::{acceptance::HeadlessEditor, McpWriteMode};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ClientCapabilities, ClientConfig,
        Implementation, ProtocolVersion,
    },
    service::{ClientLifecycleMode, ClientServiceExt, Peer, RoleClient},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
};
use serde_json::{json, Value};
use std::{
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

struct MockEngine(Child);
impl Drop for MockEngine {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct TestDirectory(PathBuf);
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn call(peer: &Peer<RoleClient>, name: &'static str, args: Value) -> CallToolResult {
    let response = peer
        .call_tool_once(
            CallToolRequestParams::new(name)
                .with_arguments(args.as_object().expect("引数はobject").clone()),
        )
        .await;
    assert!(response.is_ok(), "道具のHTTP要求が成功する");
    let CallToolResponse::Complete(result) = response.unwrap() else {
        panic!("道具が完了結果を返す")
    };
    result
}
fn data(result: CallToolResult) -> Value {
    assert_ne!(result.is_error, Some(true), "道具の操作が成功する");
    result.structured_content.expect("構造化結果がある")
}
async fn visible(peer: &Peer<RoleClient>) -> Value {
    let snapshot = data(call(peer, "object_get_snapshot", json!({"objectId":"n-0"})).await);
    snapshot["items"]
        .as_array()
        .expect("snapshot項目がある")
        .iter()
        .find(|item| item["engineData"]["name"] == "visible")
        .expect("visibleがある")["engineData"]["value"]
        .clone()
}

async fn protocol_case(
    editor: Arc<HeadlessEditor>,
    peer: Peer<RoleClient>,
    token: String,
    directory: PathBuf,
) {
    let status = data(call(&peer, "engine_get_status", json!({})).await);
    assert_eq!(status["items"][0]["engineData"]["engineState"], "ready");
    assert_eq!(visible(&peer).await, true);
    let args = json!({"params":{"objectId":"n-0","property":"visible","value":false}});
    editor
        .runtime()
        .set_write_access(McpWriteMode::ReadOnly, None)
        .await
        .expect("読み取り専用");
    let denied = call(&peer, "object_set_property", args.clone()).await;
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(visible(&peer).await, true, "拒否では実mockを変更しない");

    editor
        .runtime()
        .set_write_access(McpWriteMode::Enabled, None)
        .await
        .expect("書き込みを許可");
    let applied = data(call(&peer, "object_set_property", args.clone()).await);
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(visible(&peer).await, false);
    let history = editor.history();
    assert_eq!(history["canUndo"], true);
    assert_eq!(history["undoGroup"]["source"], "mcp");
    editor
        .undo_from_ui()
        .await
        .expect("共通履歴をUI入口からundo");
    assert_eq!(visible(&peer).await, true);

    editor
        .runtime()
        .set_write_access(McpWriteMode::Confirm, None)
        .await
        .expect("都度確認へ変更");
    let before = editor.history();
    let confirmation = async {
        let request = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let mut pending = editor
                    .runtime()
                    .pending_confirmations("main")
                    .await
                    .expect("確認一覧");
                if let Some(request) = pending.pop() {
                    break request;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("確認要求が届く");
        assert_eq!(editor.history(), before, "確認待ちは編集列を変更しない");
        assert_eq!(request.before, Some(json!(true)));
        assert_eq!(request.after, Some(json!(false)));
        assert!(editor
            .runtime()
            .decide_confirmation("settings", &request.id, true)
            .await
            .is_err());
        editor
            .runtime()
            .decide_confirmation("main", &request.id, true)
            .await
            .expect("mainから承認");
        request.id
    };
    let (confirmed, confirmation_id) =
        tokio::join!(call(&peer, "object_set_property", args), confirmation);
    let confirmed = data(confirmed);
    assert_eq!(confirmed["outcome"], "applied");
    assert_eq!(visible(&peer).await, false);
    assert!(editor
        .runtime()
        .pending_confirmations("main")
        .await
        .expect("確認一覧")
        .is_empty());
    let operations = editor.operations();
    assert!(operations["storageError"].is_null());
    let records = operations["records"].as_array().expect("操作記録がある");
    for request in [&applied["requestId"], &confirmed["requestId"]] {
        assert!(records.iter().any(|record| &record["requestId"] == request
            && record["outcome"] == "applied"
            && record["actorFinished"] == true));
    }
    let memory = operations.to_string();
    let disk = std::fs::read_to_string(directory.join("logs/mcp-operations.jsonl"))
        .expect("永続記録を読む");
    for output in [&memory, &disk] {
        assert!(!output.contains(&token), "トークンを記録しない");
        assert!(
            !output.contains(&confirmation_id),
            "確認用秘密IDを記録しない"
        );
    }
    editor
        .undo_from_ui()
        .await
        .expect("次の版へ実mock状態を戻す");
    assert_eq!(visible(&peer).await, true);
}

async fn run_protocol(
    editor: Arc<HeadlessEditor>,
    endpoint: String,
    token: String,
    directory: PathBuf,
    legacy: bool,
) {
    let version = if legacy {
        ProtocolVersion::V_2025_11_25
    } else {
        ProtocolVersion::V_2026_07_28
    };
    let mode = if legacy {
        ClientLifecycleMode::Initialize
    } else {
        ClientLifecycleMode::Discover {
            preferred_versions: vec![version.clone()],
        }
    };
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token.clone()),
    );
    let client = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("mcp-acceptance", "1"),
    )
    .with_protocol_version(version)
    .serve_with_lifecycle(transport, mode)
    .await;
    assert!(client.is_ok(), "指定プロトコル版の接続が成功する");
    let client = client.unwrap();
    let scenario = tokio::spawn(protocol_case(
        editor,
        client.peer().clone(),
        token,
        directory,
    ));
    let result = scenario.await;
    let closed = client.cancel().await;
    assert!(closed.is_ok(), "HTTPクライアントを停止・joinする");
    assert!(result.is_ok(), "受入の条件を満たす");
    println!(
        "MCP_ENTRYPOINT_OK {}",
        if legacy { "Initialize" } else { "Discover" }
    );
}

#[cfg(windows)]
fn assert_no_webview_loaded() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
    }
    for name in ["WebView2Loader.dll", "EmbeddedBrowserWebView.dll"] {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // 呼び出しの間、終端付きUTF-16文字列を保持する。参照だけでロードはしない。
        assert!(
            unsafe { GetModuleHandleW(name.as_ptr()) }.is_null(),
            "WebView2をロードしない"
        );
    }
}

#[tokio::test]
async fn real_http_and_mock_share_production_services() {
    let path = PathBuf::from(
        std::env::var_os("NORVES_ENGINE_PATH").expect("受入にはNORVES_ENGINE_PATHが必須"),
    );
    assert!(path.is_file(), "実mockのパスが有効なファイルである");
    let bridge_listener = TcpListener::bind(("127.0.0.1", 0)).expect("Bridgeポートを確保");
    let http_listener = TcpListener::bind(("127.0.0.1", 0)).expect("HTTPポートを確保");
    let bridge_port = bridge_listener
        .local_addr()
        .expect("Bridgeポートを読む")
        .port();
    let http_port = http_listener.local_addr().expect("HTTPポートを読む").port();
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).expect("一時領域の識別子を生成");
    let directory = TestDirectory(
        std::env::temp_dir().join(format!("norves-mcp-e2e-{:x}", u128::from_le_bytes(nonce))),
    );
    std::fs::create_dir(&directory.0).expect("一時領域を作る");
    drop(bridge_listener);
    let engine = Command::new(path)
        .args(["--bridge-port", &bridge_port.to_string()])
        .env("NORVES_MOCK_PROFILE", "mcp-edit")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("実mockを起動");
    let mut engine = MockEngine(engine);
    let editor = Arc::new(HeadlessEditor::new(directory.0.clone()));
    let active = editor.clone();
    let workdir = directory.0.clone();
    let result = tokio::spawn(async move {
        active
            .connect(bridge_port)
            .await
            .expect("本番Bridge接続入口で実mockへ接続");
        drop(http_listener);
        let settings = active
            .runtime()
            .set_settings(true, http_port)
            .await
            .expect("HTTP起動設定を適用");
        let endpoint = settings.endpoint.expect("HTTPが起動済み");
        let token = active
            .runtime()
            .token()
            .await
            .expect("認証トークンを得る")
            .token;
        for legacy in [false, true] {
            run_protocol(
                active.clone(),
                endpoint.clone(),
                token.clone(),
                workdir.clone(),
                legacy,
            )
            .await;
        }
    })
    .await;
    editor.shutdown().await;
    assert!(
        tokio::net::TcpListener::bind(("127.0.0.1", http_port))
            .await
            .is_ok(),
        "HTTP停止後にポートが返る"
    );
    assert!(
        engine.0.try_wait().expect("mock終了状態を確認").is_none(),
        "受入中にmockが異常終了していない"
    );
    engine.0.kill().expect("mockを停止");
    engine.0.wait().expect("mock終了をjoin");
    #[cfg(windows)]
    assert_no_webview_loaded();
    assert!(result.is_ok(), "実HTTP/実mockの受入が成功する");
    println!("MCP_ENTRYPOINT_OK shutdown");
}
