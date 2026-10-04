// 実HTTP入口、rmcpのcontext、要求リースの接続を検査する。

struct HttpLifetimeFixture {
    server: RunningServer,
    client: reqwest::Client,
    token: String,
    session: Option<String>,
    version: ProtocolVersion,
    leases: tokio::sync::mpsc::UnboundedReceiver<(McpRequestLease, CancellationToken)>,
    _directory: TestTokenDirectory,
}

impl HttpLifetimeFixture {
    async fn new(version: ProtocolVersion) -> Self {
        let directory = TestTokenDirectory::new();
        let token = directory.create();
        let token_value = token.expose();
        let (leases, receiver) = tokio::sync::mpsc::unbounded_channel();
        let catalog = tool_catalog::McpToolCatalog::default();
        catalog.set_connection(Some(1), &[]);
        let server = RunningServer::start_with_handler(
            Arc::new(McpHttpAuth::new(token)),
            normal_policy(),
            McpServerHandler::CancellationProbe { catalog, leases },
        )
        .await;
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .expect("接続を共有しない試験クライアントを作る");
        let session = if version == ProtocolVersion::V_2025_11_25 {
            Some(initialize_session_for(&client, &server, &token_value, version.clone()).await)
        } else {
            None
        };
        Self {
            server,
            client,
            token: token_value,
            session,
            version,
            leases: receiver,
            _directory: directory,
        }
    }

    fn post(&self, body: Value) -> reqwest::RequestBuilder {
        let mut request = self
            .client
            .post(self.server.url())
            .bearer_auth(&self.token)
            .header(ACCEPT, "application/json, text/event-stream")
            .header(CONTENT_TYPE, "application/json")
            .header("Mcp-Protocol-Version", self.version.as_str())
            .body(body.to_string());
        if let Some(session) = &self.session {
            request = request.header("Mcp-Session-Id", session);
        }
        if let Some(method) = body["method"].as_str() {
            request = request.header("Mcp-Method", method);
        }
        if let Some(name) = body["params"]["name"].as_str() {
            request = request.header("Mcp-Name", name);
        }
        request
    }

    fn tool(&self, id: Value, name: &str) -> reqwest::RequestBuilder {
        let mut body = serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":{}}});
        if self.version == ProtocolVersion::V_2026_07_28 {
            body["params"]["_meta"] = serde_json::json!({
                "io.modelcontextprotocol/protocolVersion":self.version.as_str(),
                "io.modelcontextprotocol/clientCapabilities":{}});
        }
        self.post(body)
    }

    async fn start(
        &mut self,
        id: Value,
        name: &str,
    ) -> (JoinHandle<()>, McpRequestLease, CancellationToken) {
        let request = self.tool(id, name);
        let task = tokio::spawn(async move {
            // 期限切れでTCPを閉じる旧版も、JSONを返す現行版も本文まで保持する。
            if let Ok(response) = request.send().await {
                let _ = response.bytes().await;
            }
        });
        let (lease, ct) = time::timeout(Duration::from_secs(2), self.leases.recv())
            .await
            .expect("SDKへ実要求が到達する")
            .expect("要求リースを受信する");
        (task, lease, ct)
    }

    async fn cancel(&self, id: Value) {
        let response = self
            .post(
                serde_json::json!({"jsonrpc":"2.0","method":"notifications/cancelled",
            "params":{"requestId":id}}),
            )
            .send()
            .await
            .expect("取消通知を送信する");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        response.bytes().await.expect("取消通知のHTTP応答を読む");
    }
}

#[tokio::test]
async fn http_lifetime_cancellation_is_scoped_by_typed_id_and_legacy_session() {
    for version in [ProtocolVersion::V_2025_11_25, ProtocolVersion::V_2026_07_28] {
        let mut fixture = HttpLifetimeFixture::new(version.clone()).await;
        let (first_task, first, first_ct) = fixture
            .start(serde_json::json!(2), "engine_get_status")
            .await;
        let (second_task, second, _) = fixture
            .start(serde_json::json!("2"), "engine_get_status")
            .await;
        let duplicate = fixture
            .tool(serde_json::json!(2), "engine_get_status")
            .send()
            .await
            .expect("重複IDを送る");
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        duplicate.bytes().await.expect("重複拒否を読む");
        fixture.cancel(serde_json::json!(999)).await;
        assert!(first.is_current() && second.is_current());
        if version == ProtocolVersion::V_2025_11_25 {
            let original = fixture.session.take();
            fixture.session = Some(
                initialize_legacy_session(&fixture.client, &fixture.server, &fixture.token).await,
            );
            fixture.cancel(serde_json::json!(2)).await;
            assert!(first.is_current() && second.is_current());
            fixture.session = original;
        }
        fixture.cancel(serde_json::json!(2)).await;
        time::timeout(Duration::from_secs(2), first.request_cancelled())
            .await
            .expect("指定要求だけが取り消される");
        if version == ProtocolVersion::V_2025_11_25 {
            assert!(first_ct.is_cancelled(), "旧版はrmcpのcontext.ctを通る");
        }
        assert!(second.is_current());
        fixture.cancel(serde_json::json!("2")).await;
        time::timeout(Duration::from_secs(2), second.request_cancelled())
            .await
            .expect("文字列IDも区別して取り消す");
        first_task.abort();
        second_task.abort();
        let _ = first_task.await;
        let _ = second_task.await;
        fixture.server.stop().await;
    }
}

#[tokio::test]
async fn http_lifetime_sdk_context_cancels_only_its_lease() {
    for version in [ProtocolVersion::V_2025_11_25, ProtocolVersion::V_2026_07_28] {
        let mut fixture = HttpLifetimeFixture::new(version).await;
        let (first_task, first, sdk_ct) = fixture
            .start(serde_json::json!(2), "engine_get_status")
            .await;
        let (second_task, second, _) = fixture
            .start(serde_json::json!(3), "engine_get_status")
            .await;
        sdk_ct.cancel();
        time::timeout(Duration::from_secs(2), first.request_cancelled())
            .await
            .expect("SDKからリースへ取消が届く");
        assert!(second.is_current());
        first_task.abort();
        second_task.abort();
        let _ = first_task.await;
        let _ = second_task.await;
        time::timeout(Duration::from_secs(2), second.request_cancelled())
            .await
            .expect("もう一方はHTTP破棄で取り消される");
        fixture.server.stop().await;
    }
}

#[tokio::test]
async fn http_lifetime_cancellation_is_admitted_when_all_ordinary_slots_are_full() {
    for version in [ProtocolVersion::V_2025_11_25, ProtocolVersion::V_2026_07_28] {
        let mut fixture = HttpLifetimeFixture::new(version).await;
        let mut requests = Vec::new();
        for id in 0..MAX_ORDINARY_REQUESTS {
            requests.push(
                fixture
                    .start(serde_json::json!(id + 2), "runtime_play")
                    .await,
            );
        }
        assert_eq!(fixture.server.state.limits.ordinary.available_permits(), 0);
        fixture.cancel(serde_json::json!(2)).await;
        time::timeout(Duration::from_secs(2), requests[0].1.request_cancelled())
            .await
            .expect("満杯でも取消通知を配送する");
        assert!(requests[1..].iter().all(|(_, lease, _)| lease.is_current()));
        for (task, _, _) in requests {
            task.abort();
            let _ = task.await;
        }
        fixture.server.stop().await;
    }
}

#[tokio::test]
async fn http_lifetime_both_protocols_enforce_30_and_125_seconds_with_paused_time() {
    for version in [ProtocolVersion::V_2025_11_25, ProtocolVersion::V_2026_07_28] {
        for (name, seconds) in [("engine_get_status", 30), ("runtime_play", 125)] {
            let mut fixture = HttpLifetimeFixture::new(version.clone()).await;
            let (task, lease, _) = fixture.start(serde_json::json!(2), name).await;
            // 実ソケットの待機中に仮想時計が自動で進むことを防ぎ、境界を手動で跨ぐ。
            let clock_guard = tokio::spawn(async {
                loop {
                    tokio::task::yield_now().await;
                }
            });
            time::pause();
            let remaining = lease.request_deadline() - Instant::now();
            assert!(remaining <= Duration::from_secs(seconds));
            assert!(remaining > Duration::from_secs(seconds - 1));
            time::advance(remaining - Duration::from_secs(1)).await;
            tokio::task::yield_now().await;
            assert!(lease.is_current());
            assert!(!task.is_finished());
            time::advance(Duration::from_secs(1)).await;
            assert!(!lease.is_current());
            // Tokioのタイマーはミリ秒へ切り上がるため、その1 tick以内に取消が届く。
            time::advance(Duration::from_millis(1)).await;
            for _ in 0..100 {
                if lease.request_cancellation.is_cancelled() {
                    break;
                }
                tokio::task::yield_now().await;
            }
            assert!(lease.request_cancellation.is_cancelled());
            clock_guard.abort();
            time::resume();
            task.abort();
            let _ = task.await;
            fixture.server.stop().await;
        }
    }
}
