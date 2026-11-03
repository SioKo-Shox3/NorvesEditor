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
use norves_bridge_editor_client::{
    parse_capabilities_result, AssetEditClient, AssetEditError, AssetEditRejectReason,
    AssetProperties, DispatchHandle, Dispatcher, WsClientTransport,
};
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_profile_edits_and_saves_asset_properties_from_process() {
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
        "mcp-asset-edit-hello",
        "bridge.hello",
        json!({ "protocolVersions": ["0.2"] }),
    )
    .await;
    assert_eq!(
        hello.get("protocolVersion"),
        Some(&json!("0.2")),
        "合意したプロトコルバージョンが異なります"
    );

    let capabilities = request_result(
        &handle,
        "mcp-asset-edit-capabilities",
        "bridge.getCapabilities",
        json!({}),
    )
    .await;
    let capabilities = parse_capabilities_result(&capabilities).expect("能力一覧を解析できる");
    assert!(
        capabilities
            .capabilities
            .iter()
            .any(|capability| capability.name.as_str() == "asset.edit"),
        "資産編集の能力が広告されていません"
    );
    let client = AssetEditClient::new(handle.clone(), &capabilities, version(), None, RECV_TIMEOUT);

    let logical_path = "effects/mcp-test.vfx.json";
    let rate_path = "emitters/main/rate";
    let label_path = "emitters/main/label";
    let looping_path = "emitters/main/looping";
    let initial = with_timeout("asset.getProperties", client.get_properties(logical_path))
        .await
        .expect("資産のプロパティ一覧を取得できる");
    assert_eq!(initial.logical_path, logical_path, "資産のパスが異なります");
    assert_eq!(initial.properties.len(), 3, "プロパティは3項目です");

    let find_initial = |path: &str| {
        initial
            .properties
            .iter()
            .find(|property| property.path == path)
            .unwrap_or_else(|| panic!("初期プロパティがありません: {path}"))
    };
    for (path, value_type, value) in [
        (rate_path, "number", json!(30)),
        (label_path, "string", json!("spark")),
        (looping_path, "boolean", json!(true)),
    ] {
        let property = find_initial(path);
        assert_eq!(
            property.value_type, value_type,
            "値の型が異なります: {path}"
        );
        assert_eq!(property.value, value, "初期値が異なります: {path}");
    }
    let rate = find_initial(rate_path);
    assert_eq!(rate.unit.as_deref(), Some("1/s"), "rate の単位が異なります");
    assert_eq!(rate.minimum, Some(0.0), "rate の最小値が異なります");
    assert_eq!(rate.maximum, Some(1000.0), "rate の最大値が異なります");

    let expected_values = [
        (rate_path, json!(120)),
        (label_path, json!("ember")),
        (looping_path, json!(false)),
    ];
    for (path, value) in &expected_values {
        let result = with_timeout(
            "asset.setProperty",
            client.set_property(logical_path, path, value.clone()),
        )
        .await
        .unwrap_or_else(|error| panic!("プロパティを変更できません: {path}: {error:?}"));
        assert_eq!(
            result.previous_value,
            find_initial(path).value,
            "変更前の値が異なります: {path}"
        );
    }

    let assert_edited_values = |properties: &AssetProperties| {
        assert_eq!(
            properties.logical_path, logical_path,
            "資産のパスが異なります"
        );
        assert_eq!(properties.properties.len(), 3, "プロパティは3項目です");
        for (path, value) in &expected_values {
            let property = properties
                .properties
                .iter()
                .find(|property| property.path == *path)
                .unwrap_or_else(|| panic!("プロパティがありません: {path}"));
            assert_eq!(&property.value, value, "編集後の値が異なります: {path}");
        }
    };

    for (path, value, expected_reason) in [
        (
            rate_path,
            json!("fast"),
            AssetEditRejectReason::TypeMismatch,
        ),
        (rate_path, json!(5000), AssetEditRejectReason::OutOfRange),
        (
            "emitters/main/missing",
            json!(1),
            AssetEditRejectReason::UnknownProperty,
        ),
    ] {
        match with_timeout(
            "asset.setProperty",
            client.set_property(logical_path, path, value),
        )
        .await
        {
            Err(AssetEditError::Rejected(rejection)) => {
                assert_eq!(
                    rejection.reason, expected_reason,
                    "拒否理由が異なります: {path}"
                );
            }
            result => panic!("プロパティの編集が拒否されませんでした: {path}: {result:?}"),
        }
        let properties = with_timeout("asset.getProperties", client.get_properties(logical_path))
            .await
            .expect("拒否後のプロパティ一覧を取得できる");
        assert_edited_values(&properties);
    }

    let saved = with_timeout("asset.save", client.save(logical_path))
        .await
        .expect("資産を保存できる");
    assert!(saved.saved, "資産の保存が完了していません");
    let properties = with_timeout("asset.getProperties", client.get_properties(logical_path))
        .await
        .expect("保存後のプロパティ一覧を取得できる");
    assert_edited_values(&properties);

    handle.shutdown().await;
}
