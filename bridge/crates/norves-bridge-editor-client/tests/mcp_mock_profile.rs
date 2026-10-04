//! MCP の試験プロフィールを実モックエンジンから照会する。

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use norves_bridge_core::{
    CorrelationId, MethodName, ResponsePayload, ValidatedEnvelope, VersionString,
};
use norves_bridge_editor_client::{DispatchHandle, Dispatcher, WsClientTransport};
use serde_json::{json, Value};

const RECV_TIMEOUT: Duration = Duration::from_secs(5);

fn version() -> VersionString {
    VersionString::try_from("0.2".to_owned()).expect("0.2 は有効なプロトコルバージョン")
}

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn mock_engine_path() -> PathBuf {
    let path = match std::env::var("NORVES_MOCK_ENGINE") {
        Ok(path) => PathBuf::from(path),
        Err(std::env::VarError::NotPresent) => {
            eprintln!("mcp_mock_profile は NORVES_MOCK_ENGINE 未設定のためスキップ");
            return PathBuf::new();
        }
        Err(error) => panic!("NORVES_MOCK_ENGINE を読み取れません: {error}"),
    };

    assert!(!path.as_os_str().is_empty(), "NORVES_MOCK_ENGINE が空です");
    assert!(
        path.is_file(),
        "モックエンジン実行ファイルがありません: {}",
        path.display()
    );
    path
}

fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("空きポートを確保する");
    let port = listener
        .local_addr()
        .expect("ローカルアドレスを取得する")
        .port();
    drop(listener);
    port
}

fn spawn_ready_engine(exe: &PathBuf, port: u16) -> ChildGuard {
    let mut child = Command::new(exe)
        .arg("--bridge-port")
        .arg(port.to_string())
        .env("NORVES_MOCK_PROFILE", "mcp-edit")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|error| panic!("モックエンジンを起動できません: {error}"));

    let stdout = child.stdout.take().expect("標準出力を取得する");
    let guard = ChildGuard { child };
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    let ready = line.starts_with("READY");
                    let _ = tx.send(line);
                    if ready {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO);
        match rx.recv_timeout(remaining) {
            Ok(line) if line.starts_with("READY") => return guard,
            Ok(_) => continue,
            Err(error) => panic!("モックエンジンが READY を通知しませんでした: {error}"),
        }
    }
}

async fn connect_with_retry(url: &str) -> WsClientTransport {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut backoff = Duration::from_millis(50);
    loop {
        match WsClientTransport::connect(url).await {
            Ok(transport) => return transport,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "WebSocket 接続に失敗しました: {error:?}"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_millis(500));
            }
        }
    }
}

fn request_envelope(id: &str, method: &str, params: Value) -> ValidatedEnvelope {
    ValidatedEnvelope::Request {
        version: version(),
        id: CorrelationId::try_from(id.to_owned()).expect("要求IDは空でない"),
        method: MethodName::try_from(method.to_owned()).expect("メソッド名は名前空間付き"),
        params: Some(params.as_object().expect("params はオブジェクト").clone()),
        session_id: None,
        seq: None,
    }
}

async fn with_timeout<F, T>(what: &str, future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(RECV_TIMEOUT, future)
        .await
        .unwrap_or_else(|_| panic!("{what} の応答がタイムアウトしました"))
}

async fn request_result(handle: &DispatchHandle, id: &str, method: &str, params: Value) -> Value {
    let response = with_timeout(
        method,
        handle.request(request_envelope(id, method, params), RECV_TIMEOUT),
    )
    .await
    .unwrap_or_else(|error| panic!("{method} 要求に失敗しました: {error:?}"));

    match response {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(error) => panic!("{method} がエラーを返しました: {error:?}"),
    }
}

fn contains_node_id(node: &Value, id: &str) -> bool {
    node.get("id").and_then(Value::as_str) == Some(id)
        || node
            .get("children")
            .and_then(Value::as_array)
            .is_some_and(|children| children.iter().any(|child| contains_node_id(child, id)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_profile_serves_schema_mutable_scene_and_asset_reads_from_process() {
    let exe = mock_engine_path();
    if exe.as_os_str().is_empty() {
        return;
    }

    let port = pick_free_port();
    let _engine = spawn_ready_engine(&exe, port);
    let url = format!("ws://127.0.0.1:{port}");
    let transport = connect_with_retry(&url).await;
    let handle = Dispatcher::spawn(transport);

    let hello = request_result(
        &handle,
        "mcp-profile-hello",
        "bridge.hello",
        json!({ "protocolVersions": ["0.2"] }),
    )
    .await;
    assert_eq!(hello.get("protocolVersion"), Some(&json!("0.2")));

    let capabilities = request_result(
        &handle,
        "mcp-profile-capabilities",
        "bridge.getCapabilities",
        json!({}),
    )
    .await;
    let capability_names = capabilities["capabilities"]
        .as_array()
        .expect("capabilities 配列がある")
        .iter()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(capability_names.contains(&"asset.read"));
    assert!(!capability_names.contains(&"asset.reload"));

    let schema = request_result(
        &handle,
        "mcp-profile-schema",
        "schema.getSnapshot",
        json!({}),
    )
    .await;
    let types = schema["types"].as_array().expect("型一覧がある");
    let type_a = types
        .iter()
        .find(|type_descriptor| type_descriptor["typeName"] == "TypeA")
        .expect("TypeA が照会できる");
    assert!(type_a["properties"]
        .as_array()
        .expect("TypeA のプロパティがある")
        .iter()
        .any(|property| property["name"] == "fieldOfView"));

    let initial_tree = request_result(
        &handle,
        "mcp-profile-tree-before",
        "scene.getTree",
        json!({}),
    )
    .await;
    assert!(contains_node_id(&initial_tree["root"], "n-3"));

    let created = request_result(
        &handle,
        "mcp-profile-create",
        "scene.createObject",
        json!({ "parentId": "n-2", "kind": "object" }),
    )
    .await;
    assert_eq!(created["accepted"], true);
    let new_id = created["newId"]
        .as_str()
        .expect("新しいオブジェクトIDがある");
    let updated_tree = request_result(
        &handle,
        "mcp-profile-tree-after",
        "scene.getTree",
        json!({}),
    )
    .await;
    assert!(contains_node_id(&updated_tree["root"], new_id));

    let manifest = request_result(
        &handle,
        "mcp-profile-manifest",
        "asset.getManifest",
        json!({ "filter": "texture", "page": 0, "pageSize": 50 }),
    )
    .await;
    assert_eq!(manifest["totalCount"], 1);
    assert_eq!(manifest["entries"][0]["logicalPath"], "textures/hero.png");

    let resolved = request_result(
        &handle,
        "mcp-profile-resolve-known",
        "asset.resolve",
        json!({
            "logicalPath": "textures/hero.png",
            "kind": "texture",
            "variant": "default"
        }),
    )
    .await;
    assert_eq!(
        resolved,
        json!({
            "status": "successCooked",
            "source": "cooked",
            "normalizedLogicalPath": "textures/hero.png"
        })
    );

    let unknown = request_result(
        &handle,
        "mcp-profile-resolve-unknown",
        "asset.resolve",
        json!({ "logicalPath": "textures/missing.png" }),
    )
    .await;
    assert_eq!(
        unknown,
        json!({
            "status": "cookedEntryMissing",
            "source": "none",
            "normalizedLogicalPath": "textures/missing.png"
        })
    );

    handle.shutdown().await;
}
