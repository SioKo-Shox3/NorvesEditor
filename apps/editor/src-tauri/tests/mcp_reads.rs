//! 実mockの読み取りBridgeメソッドとlog.subscribe保管経路を検証する試験。

use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use norves_bridge_core::{
    CorrelationId, MethodName, ResponsePayload, ValidatedEnvelope, VersionString,
};
use norves_bridge_editor_client::{connect_with_retry, parse_capabilities_result, RetryConfig};
use norves_editor_lib::mcp::log_buffer::{
    record_relay_log, subscribe_log_stream, LogBuffer, LogQuery, LogSubscriptionStatus,
    RelayLogRecord,
};
use serde_json::Value;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

struct MockEngine(Child);

impl Drop for MockEngine {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(id: &str, method: &str, params: Value) -> ValidatedEnvelope {
    let params = params
        .as_object()
        .cloned()
        .expect("要求paramsはオブジェクト");
    ValidatedEnvelope::Request {
        version: VersionString::try_from("0.2".to_owned()).expect("有効なプロトコル版を使う"),
        id: CorrelationId::try_from(id.to_owned()).expect("要求IDを作る"),
        method: MethodName::try_from(method.to_owned()).expect("メソッド名を作る"),
        params: Some(params),
        session_id: None,
        seq: None,
    }
}

async fn result(
    handle: &norves_bridge_editor_client::DispatchHandle,
    id: &str,
    method: &str,
    params: Value,
) -> Value {
    match handle
        .request(request(id, method, params), Duration::from_secs(5))
        .await
        .expect("Bridge要求が成功")
    {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(error) => panic!("{method}が拒否されました: {error:?}"),
    }
}

#[tokio::test]
async fn real_mock_subscription_burst_is_retained_without_a_ui_or_mcp_client() {
    let Some(engine_path) = std::env::var_os("NORVES_ENGINE_PATH").map(PathBuf::from) else {
        // libtestの出力捕捉を通さず、通常のcargo testにも未実行の理由を残す。
        writeln!(
            std::io::stderr().lock(),
            "[SKIP] mcp_reads: NORVES_ENGINE_PATHが設定されていません"
        )
        .expect("実mock試験を実行しない理由を表示する");
        return;
    };
    assert!(
        engine_path.is_file(),
        "NORVES_ENGINE_PATHが実行ファイルではありません: {engine_path:?}"
    );

    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("空きポートを確保");
    let port = listener.local_addr().expect("空きポートを読む").port();
    drop(listener);

    let engine = Command::new(&engine_path)
        .arg("--bridge-port")
        .arg(port.to_string())
        .env("NORVES_MOCK_PROFILE", "mcp-edit")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("試験プロフィールのmock engineを起動");
    let _engine = MockEngine(engine);

    let endpoint = format!("ws://127.0.0.1:{port}");
    let handle = connect_with_retry(&endpoint, &RetryConfig::default())
        .await
        .expect("mock engineへ接続");
    let mut events = handle.subscribe_events();
    let hello = serde_json::json!({
        "role": "editor",
        "clientName": "NorvesEditor",
        "protocolVersions": ["0.2", "0.1"]
    });
    result(&handle, "mcp-log-hello", "bridge.hello", hello).await;
    let capabilities = result(
        &handle,
        "mcp-log-capabilities",
        "bridge.getCapabilities",
        serde_json::json!({}),
    )
    .await;
    let capabilities = parse_capabilities_result(&capabilities).expect("capability応答を解析");
    assert!(
        capabilities
            .capabilities
            .iter()
            .any(|capability| capability.name.as_str() == "log.stream"),
        "mock profileがlog.streamを広告する"
    );

    let status = result(
        &handle,
        "mcp-read-status",
        "engine.getStatus",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status["engineState"], "ready");
    assert_eq!(status["engineName"], "MockEngine");

    let tree = result(
        &handle,
        "mcp-read-tree",
        "scene.getTree",
        serde_json::json!({"rootId":"missing-root","maxDepth":0}),
    )
    .await;
    assert_eq!(tree["root"]["id"], "n-0", "mockはtree範囲指定を無視する");
    assert_eq!(tree["root"]["children"].as_array().unwrap().len(), 2);

    let object_snapshot = result(
        &handle,
        "mcp-read-object",
        "object.getSnapshot",
        serde_json::json!({"objectId":"n-1"}),
    )
    .await;
    assert_eq!(object_snapshot["objectId"], "n-1");
    assert!(object_snapshot["properties"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    let schema = result(
        &handle,
        "mcp-read-schema",
        "schema.getSnapshot",
        serde_json::json!({}),
    )
    .await;
    assert!(schema["types"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    let manifest = result(
        &handle,
        "mcp-read-manifest",
        "asset.getManifest",
        serde_json::json!({"page":0,"pageSize":50}),
    )
    .await;
    assert_eq!(manifest["entries"][0]["logicalPath"], "textures/hero.png");
    assert_eq!(manifest["totalCount"], 1);

    let asset = result(
        &handle,
        "mcp-read-asset",
        "asset.resolve",
        serde_json::json!({"logicalPath":"textures/hero.png"}),
    )
    .await;
    assert_eq!(asset["status"], "successCooked");
    assert_eq!(asset["source"], "cooked");

    let generation = 41;
    let mut buffer = LogBuffer::default();
    buffer.begin_generation(generation);
    let log_buffer = std::sync::Mutex::new(buffer);
    let subscription = subscribe_log_stream(
        &log_buffer,
        &handle,
        request("mcp-log-subscribe", "log.subscribe", serde_json::json!({})),
        &capabilities.capabilities,
        generation,
        &CancellationToken::new(),
    )
    .await
    .expect("backendのlog.subscribeが完了");
    assert_eq!(subscription.status, LogSubscriptionStatus::Subscribed);
    let subscription_id = subscription
        .subscription_id
        .expect("実mockのackにsubscriptionIdがある");

    for _ in 0..3 {
        let envelope = timeout(Duration::from_secs(5), async {
            loop {
                match events.recv().await {
                    Ok(envelope) => {
                        if matches!(&*envelope, ValidatedEnvelope::Event { event, .. } if event.as_str() == "log.message") {
                            break envelope;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        panic!("mockのログイベントを{skipped}件取りこぼしました")
                    }
                    Err(error) => panic!("Bridgeイベント受信に失敗しました: {error}"),
                }
            }
        })
        .await
        .expect("subscribe後のlog.messageを受信");
        let ValidatedEnvelope::Event {
            params: Some(params),
            ..
        } = &*envelope
        else {
            panic!("log.messageのparamsがありません")
        };
        assert!(matches!(
            record_relay_log(
                &mut log_buffer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                generation,
                params
            )
            .expect("受信ログを保持"),
            RelayLogRecord::Retained(_)
        ));
    }

    let snapshot = log_buffer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .read(&LogQuery {
            generation: Some(generation),
            ..LogQuery::default()
        });
    assert_eq!(snapshot.entries.len(), 3);
    assert!(snapshot
        .entries
        .iter()
        .all(|entry| entry.message == "Game started"));
    assert_eq!(
        snapshot
            .entries
            .iter()
            .map(|entry| entry.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(snapshot.retention[0].first_sequence, Some(1));
    assert_eq!(snapshot.retention[0].last_sequence, Some(3));

    let unsubscribe = serde_json::json!({ "subscriptionId": subscription_id });
    let unsubscribed = result(
        &handle,
        "mcp-log-unsubscribe",
        "log.unsubscribe",
        unsubscribe,
    )
    .await;
    assert_eq!(unsubscribed.get("ok"), Some(&Value::Bool(true)));
    handle.shutdown().await;
}
