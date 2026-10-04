//! 実mockと両版の実HTTPから、本番と共通の許可・確認・編集列・記録を検証する。

use base64::Engine as _;
use norves_editor_lib::mcp::{acceptance::HeadlessEditor, McpWriteMode};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ClientCapabilities, ClientConfig,
        Implementation, ProtocolVersion, ServerNotification, SubscriptionFilter,
    },
    service::{ClientLifecycleMode, ClientServiceExt, NotificationContext, Peer, RoleClient},
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
    assert_ne!(
        result.is_error,
        Some(true),
        "道具の操作が成功する: {result:?}"
    );
    result.structured_content.expect("構造化結果がある")
}

async fn property(peer: &Peer<RoleClient>, id: &str, name: &str) -> Value {
    let snapshot = data(call(peer, "object_get_snapshot", json!({"objectId":id})).await);
    snapshot["items"]
        .as_array()
        .expect("snapshot項目")
        .iter()
        .find(|item| item["engineData"]["name"] == name)
        .expect("指定したプロパティ")["engineData"]["value"]
        .clone()
}

async fn pages(peer: &Peer<RoleClient>, name: &'static str, mut args: Value) -> Vec<Value> {
    args["pageSize"] = json!(1);
    let mut items = Vec::new();
    let mut total = None;
    loop {
        let page = data(call(peer, name, args.clone()).await);
        assert!(serde_json::to_vec(&page).unwrap().len() <= 256 * 1024);
        let count = page["totalItems"].as_u64().expect("全項目数");
        assert_eq!(
            *total.get_or_insert(count),
            count,
            "snapshot全体の件数は固定"
        );
        let batch = page["items"].as_array().expect("ページ項目");
        assert!(batch.len() <= 1);
        items.extend(batch.iter().cloned());
        assert_eq!(page["nextOffset"], items.len());
        assert!(items.len() <= count as usize);
        if page["truncated"] == false {
            assert!(page["cursor"].is_null());
            assert_eq!(items.len(), count as usize);
            break;
        }
        assert!(!batch.is_empty(), "継続ページが進む");
        args["cursor"] = page["cursor"].clone();
        assert!(args["cursor"].is_string());
    }
    items
}

async fn read_case(peer: &Peer<RoleClient>) {
    let tree = pages(peer, "scene_get_tree", json!({})).await;
    assert_eq!(tree.len(), 4);
    assert_eq!(tree[0]["id"], "n-0");
    let subtree = pages(peer, "scene_get_tree", json!({"rootId":"n-2"})).await;
    assert_eq!(subtree.len(), 2);
    assert_eq!(subtree[0]["id"], "n-2");
    assert!(subtree[0]["parentId"].is_null(), "部分木の根は親を含めない");
    assert_eq!(subtree[1]["parentId"], "n-2");
    assert_eq!(
        pages(
            peer,
            "scene_get_tree_page",
            json!({"rootId":"n-2","maxDepth":0})
        )
        .await
        .len(),
        1
    );
    assert!(
        pages(peer, "object_get_snapshot", json!({"objectId":"n-1"}))
            .await
            .len()
            >= 6
    );
    assert!(!pages(peer, "schema_get_snapshot", json!({}))
        .await
        .is_empty());
    let capabilities = pages(peer, "bridge_get_capabilities", json!({})).await;
    assert!(capabilities
        .iter()
        .any(|item| item["engineData"]["name"] == "asset.read"));
    let assets = pages(peer, "asset_get_manifest", json!({})).await;
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0]["engineData"]["logicalPath"], "textures/hero.png");
    let asset = data(
        call(
            peer,
            "asset_resolve",
            json!({"logicalPath":"textures/hero.png"}),
        )
        .await,
    );
    assert_eq!(asset["items"][0]["engineData"]["status"], "successCooked");
    let missing = data(call(peer, "asset_resolve", json!({"logicalPath":"missing.png"})).await);
    assert_eq!(
        missing["items"][0]["engineData"]["status"],
        "cookedEntryMissing"
    );
    for args in [
        json!({"pageSize":201}),
        json!({"cursor":"unknown"}),
        json!({"rootId":"missing"}),
        json!({"unknown":true}),
    ] {
        assert_eq!(
            call(peer, "scene_get_tree", args).await.is_error,
            Some(true)
        );
    }
    let logs = pages(peer, "logs_get_recent", json!({})).await;
    assert_eq!(logs.len(), 3, "HTTP接続前から本番relayがログを保持する");
    assert!(logs
        .iter()
        .all(|entry| entry["engineData"]["message"] == "Game started"));
    let sequence = logs[0]["engineData"]["sequence"].clone();
    assert_eq!(
        pages(peer, "logs_get_recent", json!({"afterSequence":sequence}))
            .await
            .len(),
        2
    );
    let image = call(peer, "viewport_get_thumbnail", json!({})).await;
    assert_ne!(image.is_error, Some(true));
    let content = serde_json::to_value(image.content).unwrap();
    let png = content
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["type"] == "image")
        .expect("MCP画像content");
    assert_eq!(png["mimeType"], "image/png");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png["data"].as_str().unwrap())
        .expect("PNGのbase64");
    assert!(bytes.len() <= 512 * 1024);
    let decoded =
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).expect("実PNGを復号");
    assert!(decoded.width() <= 512 && decoded.height() <= 512);
    println!("MCP_ACCEPTANCE_OK reads-paging-logs-image");
}

async fn confirmed_call(
    editor: &HeadlessEditor,
    peer: &Peer<RoleClient>,
    name: &'static str,
    args: Value,
    approve: bool,
) -> CallToolResult {
    let confirmation = async {
        let request = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(request) = editor
                    .runtime()
                    .pending_confirmations("main")
                    .await
                    .unwrap()
                    .pop()
                {
                    break request;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("必須確認を受信する");
        assert_eq!(
            request.tool_name,
            if name == "scene_delete_object" {
                "scene.deleteObject"
            } else {
                "edit_undo_redo"
            }
        );
        editor
            .runtime()
            .decide_confirmation("main", &request.id, approve)
            .await
            .expect("確認へ回答");
    };
    let (result, ()) = tokio::join!(call(peer, name, args), confirmation);
    result
}

async fn group_and_scope_case(editor: &HeadlessEditor, peer: &Peer<RoleClient>) {
    editor
        .runtime()
        .set_write_access(McpWriteMode::Enabled, Some("n-2".to_owned()))
        .await
        .unwrap();
    let denied = call(
        peer,
        "object_set_property",
        json!({"params":{"objectId":"n-0","property":"visible","value":false}}),
    )
    .await;
    assert_eq!(denied.is_error, Some(true));
    let denied = denied.structured_content.unwrap();
    assert_eq!(denied["outcome"], "notApplied");
    assert_eq!(denied["operationResult"], "rejected");
    assert_eq!(visible(peer).await, true);
    let in_scope = data(
        call(
            peer,
            "object_set_property",
            json!({"params":{"objectId":"n-3","property":"enabled","value":true}}),
        )
        .await,
    );
    assert_eq!(in_scope["outcome"], "applied");
    editor.undo_from_ui().await.unwrap();
    assert_eq!(property(peer, "n-3", "enabled").await, false);
    let denied_move = call(
        peer,
        "scene_reparent_object",
        json!({"params":{"objectId":"n-3","newParentId":"n-0"}}),
    )
    .await;
    assert_eq!(denied_move.is_error, Some(true));
    editor
        .runtime()
        .set_write_access(McpWriteMode::Enabled, None)
        .await
        .unwrap();

    let begin = data(call(peer, "edit_begin_group", json!({"name":"作成と値の変更"})).await);
    let group = begin["result"]["groupId"].clone();
    assert_eq!(group.as_str().unwrap().len(), 64);
    let created = data(
        call(
            peer,
            "scene_create_object",
            json!({"groupId":group,"params":{"parentId":"n-0"}}),
        )
        .await,
    );
    let parent = created["result"]["newId"]
        .as_str()
        .expect("作成ID")
        .to_owned();
    // 作成した親の下へ既存プロパティを持つノードを複製し、その新IDを編集する。
    let duplicated = data(
        call(
            peer,
            "scene_duplicate_object",
            json!({"groupId":group,"params":{"objectId":"n-1","newParentId":parent}}),
        )
        .await,
    );
    let id = duplicated["result"]["newId"]
        .as_str()
        .expect("複製の作成ID")
        .to_owned();
    let result = data(
        call(
            peer,
            "object_set_property",
            json!({"groupId":group,"params":{"objectId":id,"property":"fieldOfView","value":75}}),
        )
        .await,
    );
    assert_eq!(result["outcome"], "applied");
    data(call(peer, "edit_end_group", json!({"groupId":group})).await);
    assert_eq!(editor.history()["undoGroup"]["name"], "作成と値の変更");
    assert_eq!(
        editor.history()["undoGroup"]["count"],
        3,
        "親作成・複製・値設定を同じまとまりに記録する"
    );
    assert_eq!(property(peer, &id, "fieldOfView").await, 75);
    let undone = data(confirmed_call(editor, peer, "edit_undo", json!({}), true).await);
    assert_eq!(undone["completedCount"], 3);
    assert!(!pages(peer, "scene_get_tree", json!({}))
        .await
        .iter()
        .any(|node| node["id"] == id));
    let redone = call(peer, "edit_redo", json!({})).await;
    assert_ne!(
        redone.is_error,
        Some(true),
        "作成・複製・値設定のまとまりをMCPからredoできる: {:?}",
        redone.structured_content
    );
    data(redone);
    let nodes = pages(peer, "scene_get_tree", json!({})).await;
    let new_id = nodes
        .iter()
        .filter(|node| node["name"] == "NodeA Copy")
        .find_map(|node| node["id"].as_str().filter(|id| id.starts_with("mcp-node-")))
        .expect("再作成ID");
    assert_ne!(new_id, id);
    let recreated_copy = nodes.iter().find(|node| node["id"] == new_id).unwrap();
    assert_ne!(recreated_copy["parentId"], parent, "複製先の親も再採番する");
    assert!(nodes
        .iter()
        .any(|node| node["id"] == recreated_copy["parentId"] && node["parentId"] == "n-0"));
    assert_eq!(
        property(peer, new_id, "fieldOfView").await,
        75,
        "redoは新しいIDへ値を適用する"
    );
    editor
        .undo_from_ui()
        .await
        .expect("UIの一回undoも再採番後のIDを使う");
    assert_eq!(pages(peer, "scene_get_tree", json!({})).await.len(), 4);
    assert_eq!(
        call(peer, "edit_end_group", json!({"groupId":group}))
            .await
            .is_error,
        Some(true)
    );
    assert!(!editor
        .operations()
        .to_string()
        .contains(group.as_str().unwrap()));
    println!("MCP_ACCEPTANCE_OK scope-group-remap-ui-undo");

    let created = data(
        call(
            peer,
            "scene_create_object",
            json!({"params":{"parentId":"n-0"}}),
        )
        .await,
    );
    let id = created["result"]["newId"].clone();
    let rejected = confirmed_call(
        editor,
        peer,
        "scene_delete_object",
        json!({"params":{"objectId":id}}),
        false,
    )
    .await;
    assert_eq!(rejected.is_error, Some(true));
    assert_eq!(pages(peer, "scene_get_tree", json!({})).await.len(), 5);
    let deleted = data(
        confirmed_call(
            editor,
            peer,
            "scene_delete_object",
            json!({"params":{"objectId":id}}),
            true,
        )
        .await,
    );
    assert_eq!(deleted["undoable"], false);
    assert_eq!(editor.history()["canUndo"], false);
    assert_eq!(editor.history()["canRedo"], false);
    assert_eq!(pages(peer, "scene_get_tree", json!({})).await.len(), 4);
    println!("MCP_ACCEPTANCE_OK mandatory-confirmation-delete-history");
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
    bridge_port: u16,
) {
    read_case(&peer).await;
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
    let denied = denied.structured_content.expect("読み取り専用の拒否結果");
    assert_eq!(denied["outcome"], "notApplied");
    assert_eq!(denied["operationResult"], "rejected");
    assert!(denied["detail"].as_str().unwrap().contains("読み取り専用"));
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
    group_and_scope_case(&editor, &peer).await;
    let cursor = data(call(&peer, "scene_get_tree", json!({"pageSize":1})).await)["cursor"].clone();
    let group = data(call(&peer, "edit_begin_group", json!({"name":"切断時に閉じる"})).await)
        ["result"]["groupId"]
        .clone();
    editor.disconnect().await;
    assert!(peer.list_tools(None).await.unwrap().tools.is_empty());
    editor.connect(bridge_port).await.expect("実mockへ再接続");
    assert_eq!(
        call(&peer, "scene_get_tree", json!({"cursor":cursor}))
            .await
            .is_error,
        Some(true)
    );
    assert_eq!(
        call(&peer, "edit_end_group", json!({"groupId":group}))
            .await
            .is_error,
        Some(true)
    );
    assert_eq!(editor.history()["canUndo"], false);
    assert_eq!(visible(&peer).await, true);
    println!("MCP_ACCEPTANCE_OK reconnect-cursor-group");
}

#[derive(Clone)]
struct ObservedClient {
    config: ClientConfig,
    changed: Arc<tokio::sync::Notify>,
}

impl rmcp::ClientHandler for ObservedClient {
    async fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) {
        self.changed.notify_one();
    }
    fn get_info(&self) -> ClientConfig {
        self.config.clone()
    }
}

async fn run_protocol(
    editor: Arc<HeadlessEditor>,
    endpoint: String,
    token: String,
    directory: PathBuf,
    legacy: bool,
    bridge_port: u16,
) {
    editor
        .runtime()
        .set_write_access(McpWriteMode::ReadOnly, None)
        .await
        .unwrap();
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
    let config = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("mcp-acceptance", "1"),
    )
    .with_protocol_version(version);
    let changed = Arc::new(tokio::sync::Notify::new());
    let client = ObservedClient {
        config,
        changed: changed.clone(),
    }
    .serve_with_lifecycle(transport, mode)
    .await;
    assert!(client.is_ok(), "指定プロトコル版の接続が成功する");
    let client = client.unwrap();
    let tools = client.peer().list_tools(None).await.unwrap().tools;
    assert!(tools.iter().any(|tool| tool.name == "scene_get_tree"));
    assert!(!tools.iter().any(|tool| tool.name == "object_set_property"));
    assert!(!tools
        .iter()
        .any(|tool| tool.name == "runtime_step" || tool.name == "engine_launch"));
    let mut subscription = if legacy {
        None
    } else {
        Some(
            client
                .peer()
                .listen(SubscriptionFilter::builder().tools_list_changed().build())
                .await
                .expect("現行listenを開く"),
        )
    };
    editor
        .runtime()
        .set_write_access(McpWriteMode::Enabled, None)
        .await
        .unwrap();
    if let Some(subscription) = subscription.as_mut() {
        let notification = tokio::time::timeout(Duration::from_secs(5), subscription.next())
            .await
            .expect("現行通知の受信期限")
            .expect("現行通知")
            .expect("購読の継続");
        assert!(matches!(
            notification,
            ServerNotification::ToolListChangedNotification(_)
        ));
        println!("MCP_ACCEPTANCE_OK current-listen-notification");
    } else {
        tokio::time::timeout(Duration::from_secs(5), changed.notified())
            .await
            .expect("旧版peer通知の実受信");
        println!("MCP_ACCEPTANCE_OK legacy-peer-notification");
    }
    assert!(client
        .peer()
        .list_tools(None)
        .await
        .unwrap()
        .tools
        .iter()
        .any(|tool| tool.name == "object_set_property"));
    let scenario = tokio::spawn(protocol_case(
        editor,
        client.peer().clone(),
        token,
        directory,
        bridge_port,
    ));
    let result = scenario.await;
    drop(subscription);
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
        assert!(!active.runtime().settings().await.enabled, "既定はHTTP無効");
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
        let mut token = active
            .runtime()
            .token()
            .await
            .expect("認証トークンを得る")
            .token;
        security_case(&endpoint, &token).await;
        for legacy in [false, true] {
            run_protocol(
                active.clone(),
                endpoint.clone(),
                token.clone(),
                workdir.clone(),
                legacy,
                bridge_port,
            )
            .await;
            active
                .runtime()
                .regenerate_token()
                .await
                .expect("トークンを再生成");
            let client = reqwest::Client::new();
            assert_eq!(
                client
                    .post(&endpoint)
                    .bearer_auth(&token)
                    .json(&json!({}))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                reqwest::StatusCode::UNAUTHORIZED
            );
            token = active.runtime().token().await.unwrap().token;
            security_case(&endpoint, &token).await;
            println!("MCP_ACCEPTANCE_OK token-regeneration");
        }
        active
            .runtime()
            .set_settings(false, http_port)
            .await
            .unwrap();
        assert!(tokio::net::TcpStream::connect(("127.0.0.1", http_port))
            .await
            .is_err());
        println!("MCP_ACCEPTANCE_OK disabled");
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

async fn security_case(endpoint: &str, token: &str) {
    let client = reqwest::Client::new();
    for bearer in [None, Some("invalid")] {
        let mut request = client.post(endpoint).json(&json!({}));
        if let Some(bearer) = bearer {
            request = request.bearer_auth(bearer);
        }
        assert_eq!(
            request.send().await.unwrap().status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
    }
    for (name, value) in [
        ("Origin", "null"),
        ("Origin", "https://example.invalid"),
        ("Host", "example.invalid"),
        ("Host", "127.0.0.1:1"),
    ] {
        assert_eq!(
            client
                .post(endpoint)
                .bearer_auth(token)
                .header(name, value)
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::FORBIDDEN
        );
    }
    let response = client.post(endpoint).bearer_auth(token)
        .header("Accept", "application/json, text/event-stream")
        .header("Mcp-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/list")
        .json(&json!({"jsonrpc":"2.0","id":"security","method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}))
        .send().await.unwrap();
    assert!(
        response.status().is_success(),
        "Originなしの認証済みCLIを受け付ける"
    );
    response.bytes().await.unwrap();
    println!("MCP_ACCEPTANCE_OK authentication-origin-host");
}
