//! MCP の試験プロフィールを実モックエンジンから照会する。

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use norves_bridge_core::{
    CorrelationId, MethodName, ResponsePayload, ValidatedEnvelope, VersionString,
};
use norves_bridge_editor_client::{
    capture_deterministic, CaptureError, CaptureRequest, CaptureTimeoutPhase, DispatchHandle,
    Dispatcher, Transport, TransportError, WsClientTransport,
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
    spawn_ready_engine_with_env(exe, port, &[])
}

fn spawn_ready_engine_with_env(exe: &PathBuf, port: u16, env: &[(&str, &str)]) -> ChildGuard {
    let mut child = Command::new(exe)
        .arg("--bridge-port")
        .arg(port.to_string())
        .env("NORVES_MOCK_PROFILE", "mcp-edit")
        .envs(env.iter().copied())
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

struct CaptureTestDir(PathBuf);

impl CaptureTestDir {
    fn new() -> Self {
        static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("現在時刻を取得する")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "norves-mcp-capture-{}-{stamp}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("一時ディレクトリを作成する");
        Self(path)
    }
}

impl Drop for CaptureTestDir {
    fn drop(&mut self) {
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.expect("撮影用の一時ディレクトリを削除する");
        }
    }
}

// 実プロセスへの送信を観測し、送信前の拒否を応答の種類とは独立に検証する。
struct CountingTransport {
    inner: WsClientTransport,
    sent: Arc<AtomicUsize>,
}

impl Transport for CountingTransport {
    async fn send(&mut self, frame: String) -> Result<(), TransportError> {
        self.sent.fetch_add(1, Ordering::SeqCst);
        self.inner.send(frame).await
    }

    async fn recv(&mut self) -> Result<Option<String>, TransportError> {
        self.inner.recv().await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_profile_capture_repeats_identical_pixels_and_png_bytes() {
    let exe = mock_engine_path();
    if exe.as_os_str().is_empty() {
        return;
    }

    let dir = CaptureTestDir::new();
    let port = pick_free_port();
    let _engine = spawn_ready_engine(&exe, port);
    let transport = connect_with_retry(&format!("ws://127.0.0.1:{port}")).await;
    let handle = Dispatcher::spawn(transport);
    request_result(
        &handle,
        "capture-repeat-hello",
        "bridge.hello",
        json!({ "protocolVersions": ["0.2"] }),
    )
    .await;
    let capabilities = request_result(
        &handle,
        "capture-repeat-capabilities",
        "bridge.getCapabilities",
        json!({}),
    )
    .await;
    assert!(capabilities["capabilities"]
        .as_array()
        .expect("capabilities 配列がある")
        .iter()
        .any(|entry| entry["name"] == "viewport.capture"));

    let generation = AtomicU64::new(1);
    let request = CaptureRequest {
        capture_dir: dir.0.clone(),
        output_name: "repeat".to_owned(),
        settle_frames: 17,
        max_width: 80,
        max_height: 24,
    };
    let mut captures = Vec::new();
    for id in ["capture-repeat-first", "capture-repeat-second"] {
        let completed = capture_deterministic(&handle, id, &request, 1, &generation, RECV_TIMEOUT)
            .await
            .expect("撮影が完了する");
        let bytes = std::fs::read(&completed.path).expect("完了後に PNG を読む");
        assert!(!bytes.is_empty());
        assert_eq!(completed.file_bytes, bytes.len() as u64);
        assert_eq!(completed.width, request.max_width.min(64));
        assert_eq!(completed.height, request.max_height.min(36));
        assert_eq!(completed.frame, u64::from(request.settle_frames));
        captures.push((completed, bytes));
    }
    assert_eq!(captures[0].0.path, captures[1].0.path);
    assert_eq!(captures[0].0.pixel_digest, captures[1].0.pixel_digest);
    assert_eq!(captures[0].1, captures[1].1);
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_profile_capture_rejects_locations_outside_capture_dir() {
    let exe = mock_engine_path();
    if exe.as_os_str().is_empty() {
        return;
    }

    let dir = CaptureTestDir::new();
    let capture_dir = dir.0.join("captures");
    std::fs::create_dir(&capture_dir).expect("撮影用ディレクトリを作成する");
    let escaped = dir.0.join("escape.png");
    let port = pick_free_port();
    let _engine = spawn_ready_engine(&exe, port);
    let transport = connect_with_retry(&format!("ws://127.0.0.1:{port}")).await;
    let sent = Arc::new(AtomicUsize::new(0));
    let handle = Dispatcher::spawn(CountingTransport {
        inner: transport,
        sent: Arc::clone(&sent),
    });
    request_result(
        &handle,
        "capture-location-hello",
        "bridge.hello",
        json!({ "protocolVersions": ["0.2"] }),
    )
    .await;
    let generation = AtomicU64::new(1);
    let request = CaptureRequest {
        capture_dir: capture_dir.clone(),
        output_name: "../escape".to_owned(),
        settle_frames: 2,
        max_width: 64,
        max_height: 36,
    };
    let before = sent.load(Ordering::SeqCst);
    let result = capture_deterministic(
        &handle,
        "capture-location-client",
        &request,
        1,
        &generation,
        RECV_TIMEOUT,
    )
    .await;
    assert!(
        matches!(result, Err(CaptureError::InvalidRequest(_))),
        "{result:?}"
    );
    // 後続の要求の応答まで待ち、送信キューに撮影要求が残っていないことも確かめる。
    request_result(
        &handle,
        "capture-location-barrier",
        "bridge.getCapabilities",
        json!({}),
    )
    .await;
    assert_eq!(sent.load(Ordering::SeqCst), before + 1);
    assert!(!escaped.exists());
    assert_eq!(
        std::fs::read_dir(&capture_dir)
            .expect("撮影先を読む")
            .count(),
        0
    );

    let response = with_timeout(
        "不正な出力名の生の撮影要求",
        handle.request(
            request_envelope(
                "capture-location-raw",
                "viewport.captureDeterministic",
                json!({
                    "captureDir": capture_dir.to_str().expect("撮影先を UTF-8 に変換する"),
                    "outputName": "../escape",
                    "settleFrames": 2,
                    "maxWidth": 64,
                    "maxHeight": 36
                }),
            ),
            RECV_TIMEOUT,
        ),
    )
    .await
    .expect("生の撮影要求の応答が届く");
    match response {
        ResponsePayload::Error(error) => assert_eq!(error.code.as_str(), "ENGINE_INVALID_PARAMS"),
        ResponsePayload::Result(result) => panic!("不正な出力名が受け付けられました: {result:?}"),
    }
    assert!(!escaped.exists());
    assert_eq!(
        std::fs::read_dir(&capture_dir)
            .expect("撮影先を読む")
            .count(),
        0
    );

    let relative_request = CaptureRequest {
        capture_dir: PathBuf::from("relative-captures"),
        output_name: "relative".to_owned(),
        ..request
    };
    let before = sent.load(Ordering::SeqCst);
    let result = capture_deterministic(
        &handle,
        "capture-location-relative",
        &relative_request,
        1,
        &generation,
        RECV_TIMEOUT,
    )
    .await;
    assert!(
        matches!(result, Err(CaptureError::InvalidRequest(_))),
        "{result:?}"
    );
    request_result(
        &handle,
        "capture-relative-barrier",
        "bridge.getCapabilities",
        json!({}),
    )
    .await;
    assert_eq!(sent.load(Ordering::SeqCst), before + 1);
    assert!(!escaped.exists());
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_profile_capture_stall_times_out_during_completion() {
    let exe = mock_engine_path();
    if exe.as_os_str().is_empty() {
        return;
    }

    let dir = CaptureTestDir::new();
    let port = pick_free_port();
    let _engine = spawn_ready_engine_with_env(&exe, port, &[("NORVES_MOCK_CAPTURE_STALL", "1")]);
    let transport = connect_with_retry(&format!("ws://127.0.0.1:{port}")).await;
    let handle = Dispatcher::spawn(transport);
    request_result(
        &handle,
        "capture-stall-hello",
        "bridge.hello",
        json!({ "protocolVersions": ["0.2"] }),
    )
    .await;
    let generation = AtomicU64::new(1);
    let request = CaptureRequest {
        capture_dir: dir.0.clone(),
        output_name: "stalled".to_owned(),
        settle_frames: 3,
        max_width: 64,
        max_height: 36,
    };
    let result = capture_deterministic(
        &handle,
        "capture-stall",
        &request,
        1,
        &generation,
        Duration::from_millis(500),
    )
    .await;
    assert_eq!(
        result,
        Err(CaptureError::Timeout(CaptureTimeoutPhase::Completion))
    );
    assert_eq!(std::fs::read_dir(&dir.0).expect("撮影先を読む").count(), 0);
    handle.shutdown().await;
}
