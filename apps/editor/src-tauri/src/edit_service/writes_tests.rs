// 公開道具から許可・共通列・Bridge応答・画面の履歴操作までを検査する。
mod writes_tests {
    use super::*;
    use crate::mcp::{tool_catalog::WritePermission, writes::McpWriteService};

    const ALL_CAPABILITIES: &[&str] = &[
        "scene.query",
        "scene.edit",
        "object.query",
        "object.edit",
        "component.edit",
        "runtime.control",
    ];

    fn public_context(service: &Arc<EditService>, capabilities: &[&str]) -> McpReadContext {
        let context = confirmed_context(service, capabilities)
            .with_write_service(McpWriteService::Test(service.clone()));
        context.set_write_permission(WritePermission::Enabled);
        context
    }

    fn enabled_auth() -> McpAuthorization {
        let auth = McpAuthorization::default();
        auth.set_write_settings(McpWriteSettings {
            mode: McpWriteMode::Enabled,
            scene_root_id: None,
        })
        .expect("書き込みを許可する");
        auth
    }

    fn tree() -> Value {
        json!({"root":{"id":"scene","children":[{"id":"node","children":[]},{"id":"parent","children":[]}]}})
    }

    async fn serve_write(
        mut peer: LoopbackTransport,
        method: &'static str,
        expected: Value,
        value: Value,
    ) -> LoopbackTransport {
        loop {
            let (id, received, params) = next_request(&mut peer).await;
            match received.as_str() {
                "scene.getTree" => respond(&mut peer, id, tree()).await,
                "object.getSnapshot" => {
                    let object = params.expect("snapshot引数がある")["objectId"].clone();
                    let components = if object == "node" {
                        json!([{"objectId":"component","kind":"Camera"}])
                    } else {
                        json!([])
                    };
                    respond(&mut peer, id, json!({"objectId":object,"properties":[{"name":"visible","value":false}],"components":components})).await;
                }
                _ => {
                    assert_eq!(received, method);
                    assert_eq!(Value::Object(params.expect("操作引数がある")), expected);
                    respond(&mut peer, id, value).await;
                    return peer;
                }
            }
        }
    }

    fn data(response: rmcp::model::CallToolResult) -> Value {
        response
            .structured_content
            .expect("構造化した操作結果がある")
    }

    #[tokio::test]
    async fn public_write_tools_cover_edits_history_and_runtime() {
        let cases = [
            (
                "object_set_property",
                "object.setProperty",
                json!({"objectId":"node","property":"visible","value":true}),
                json!({"accepted":true,"appliedValue":true}),
                false,
            ),
            (
                "scene_create_object",
                "scene.createObject",
                json!({"parentId":"scene","kind":"object"}),
                json!({"accepted":true,"newId":"created"}),
                false,
            ),
            (
                "scene_duplicate_object",
                "scene.duplicateObject",
                json!({"objectId":"node","newParentId":"scene"}),
                json!({"accepted":true,"newId":"copy"}),
                false,
            ),
            (
                "scene_reparent_object",
                "scene.reparentObject",
                json!({"objectId":"node","newParentId":"parent"}),
                json!({"accepted":true}),
                false,
            ),
            (
                "scene_delete_object",
                "scene.deleteObject",
                json!({"objectId":"node"}),
                json!({"accepted":true}),
                true,
            ),
            (
                "component_add",
                "component.add",
                json!({"objectId":"node","kind":"Camera"}),
                json!({"accepted":true,"componentId":"new-component"}),
                false,
            ),
            (
                "component_remove",
                "component.remove",
                json!({"objectId":"component"}),
                json!({"accepted":true}),
                true,
            ),
            (
                "runtime_play",
                "runtime.play",
                json!({}),
                json!({"accepted":true}),
                false,
            ),
            (
                "runtime_pause",
                "runtime.pause",
                json!({}),
                json!({"accepted":true}),
                false,
            ),
            (
                "runtime_stop",
                "runtime.stop",
                json!({}),
                json!({"accepted":true}),
                false,
            ),
            (
                "edit_undo",
                "object.setProperty",
                json!({"objectId":"node","property":"visible","value":false}),
                json!({"accepted":true,"appliedValue":false}),
                false,
            ),
            (
                "edit_redo",
                "object.setProperty",
                json!({"objectId":"node","property":"visible","value":true}),
                json!({"accepted":true,"appliedValue":true}),
                false,
            ),
        ];
        for (name, method, params, result, requires_confirmation) in cases {
            let auth = enabled_auth();
            let (service, _control, handle, peer) =
                test_service_with_peer_and_authorization(8, auth.clone());
            let service = Arc::new(service);
            if name.starts_with("edit_") {
                seed_history(
                    &service,
                    if name == "edit_undo" {
                        HistoryDirection::Undo
                    } else {
                        HistoryDirection::Redo
                    },
                    70,
                    HistoryRecord::SetProperty {
                        object_id: "node".to_owned(),
                        property: "visible".to_owned(),
                        old_value: json!(false),
                        new_value: json!(true),
                    },
                );
            }
            let context = public_context(&service, ALL_CAPABILITIES);
            assert!(context.get_tool(name).is_some(), "{name}");
            let lease = auth.current_lease();
            let mut updates = auth.confirmations().subscribe();
            let arguments = if name.starts_with("edit_") {
                json!({})
            } else {
                json!({"params":params})
            };
            let responder = tokio::spawn(serve_write(peer, method, params, result));
            let request =
                tokio::spawn(
                    async move { context.call_write_tool(name, arguments, Some(lease)).await },
                );
            if requires_confirmation {
                let pending = confirmation(&mut updates, None).await;
                assert!(!request.is_finished());
                assert!(auth.confirmations().approve(&pending.id));
            }
            let response = request.await.expect("公開道具が終了する");
            assert_eq!(response.is_error, Some(false), "{name}: {response:?}");
            let mut peer = responder.await.expect("Bridge応答を返す");
            assert_eq!(response.is_error, Some(false), "{name}: {response:?}");
            let result = data(response);
            assert_eq!(result["outcome"], "applied", "{name}: {result}");
            eprintln!("公開道具 {name}: applied");
            assert_eq!(result["completedCount"], 1);
            assert_eq!(result["automaticRetryAllowed"], false);
            assert!(result["requestId"]
                .as_str()
                .expect("表示ID")
                .starts_with("mcp-write-"));
            let summary = service.history_summary();
            if name.starts_with("runtime_")
                || name.starts_with("component_")
                || name == "scene_delete_object"
            {
                assert!(!summary.can_undo, "{name}");
                assert!(!summary.pending);
                assert_eq!(result["undoable"], false);
                assert!(result["historyNotice"].is_string());
            } else if !name.starts_with("edit_") {
                let group = summary.undo_group.expect("取り消せる履歴がある");
                assert_eq!(group.source, EditSourceDto::Mcp);
                assert_eq!(group.count, 1);
            }
            if name == "object_set_property" {
                let ticket = queue_history_action(&service, HistoryDirection::Undo);
                let (id, method, params) = next_request(&mut peer).await;
                assert_eq!(method, "object.setProperty");
                assert_eq!(params.expect("旧値を送る")["value"], false);
                respond(&mut peer, id, json!({"accepted":true,"appliedValue":false})).await;
                ticket.result().await.expect("Ctrl+Zと同じUI入口で戻せる");
                assert!(service.history_summary().can_redo);
            }
            service.shutdown().await;
            handle.shutdown().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn public_write_denials_and_unfinished_confirmation_never_send() {
        for reason in [
            "readOnly",
            "capability",
            "rejected",
            "expired",
            "cancelled",
            "groupId",
            "scope",
            "invalid",
        ] {
            let auth = enabled_auth();
            if matches!(reason, "rejected" | "expired" | "cancelled") {
                auth.set_write_settings(McpWriteSettings {
                    mode: McpWriteMode::Confirm,
                    scene_root_id: None,
                })
                .expect("確認する");
            }
            if reason == "scope" {
                auth.set_write_settings(McpWriteSettings {
                    mode: McpWriteMode::Enabled,
                    scene_root_id: Some("node".to_owned()),
                })
                .expect("部分木に制限する");
            }
            let (service, _control, handle, mut peer) =
                test_service_with_peer_and_authorization(8, auth.clone());
            let service = Arc::new(service);
            let context = public_context(
                &service,
                if reason == "capability" {
                    &[]
                } else {
                    ALL_CAPABILITIES
                },
            );
            if reason == "readOnly" {
                auth.set_write_settings(McpWriteSettings::default())
                    .expect("読み取り専用にする");
                context.set_write_permission(WritePermission::ReadOnly);
            }
            let lease = auth.current_lease();
            let request_lease = lease.clone();
            let mut updates = auth.confirmations().subscribe();
            let mut args = json!({"params":{}});
            if reason == "groupId" {
                args["groupId"] = json!("untrusted-group");
            }
            if reason == "invalid" {
                args["params"]["unexpected"] = json!(true);
            }
            let request = tokio::spawn(async move {
                context
                    .call_write_tool("runtime_play", args, Some(request_lease))
                    .await
            });
            if reason == "scope" {
                respond_tree(&mut peer, tree()).await;
            }
            if matches!(reason, "rejected" | "expired" | "cancelled") {
                let pending = confirmation(&mut updates, None).await;
                assert!(!request.is_finished());
                assert!(tokio::time::timeout(Duration::from_millis(1), peer.recv())
                    .await
                    .is_err());
                service
                    .enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
                    .expect("確認待ち中のUI編集を受け付ける")
                    .result()
                    .await
                    .expect("UIは待たされない");
                match reason {
                    "rejected" => {
                        assert!(auth.confirmations().reject(&pending.id));
                    }
                    "expired" => tokio::time::advance(Duration::from_secs(120)).await,
                    _ => lease.cancel_request(),
                }
            }
            let response = request.await.expect("拒否が返る");
            assert_eq!(response.is_error, Some(true));
            let payload = data(response);
            assert_eq!(payload["outcome"], "notApplied", "{reason}");
            let detail = payload["detail"].as_str().expect("拒否理由がある");
            let expected = match reason {
                "readOnly" => "読み取り専用",
                "capability" => "能力",
                "rejected" => "拒否",
                "expired" => "期限",
                "cancelled" => "取り消",
                "groupId" => "groupId",
                "scope" => "部分木の外",
                "invalid" => "schema",
                _ => unreachable!(),
            };
            assert!(detail.contains(expected), "{reason}: {payload}");
            eprintln!("未適用 {reason}: {detail}");
            assert!(tokio::time::timeout(Duration::from_millis(1), peer.recv())
                .await
                .is_err());
            assert!(!service.history_summary().pending);
            assert!(!service.history_summary().can_undo);
            service.shutdown().await;
            handle.shutdown().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn public_write_results_distinguish_rejection_from_unknown_without_resend() {
        for name in ["runtime_play", "scene_create_object"] {
            for failure in [
                "rejected",
                "engineError",
                "timeout",
                "disconnect",
                "malformed",
            ] {
                let auth = enabled_auth();
                let (service, _control, handle, mut peer) =
                    test_service_with_peer_and_authorization(8, auth.clone());
                let service = Arc::new(service);
                let context = public_context(&service, ALL_CAPABILITIES);
                let lease = auth.current_lease();
                let task = tokio::spawn(async move {
                    context
                        .call_write_tool(name, json!({"params":{}}), Some(lease))
                        .await
                });
                if name == "scene_create_object" {
                    respond_tree(&mut peer, tree()).await;
                    respond_tree(&mut peer, tree()).await;
                }
                let (id, method, _) = next_request(&mut peer).await;
                assert_eq!(
                    method,
                    if name == "runtime_play" {
                        "runtime.play"
                    } else {
                        "scene.createObject"
                    }
                );
                match failure {
                    "rejected" => respond(&mut peer, id, json!({"accepted":false})).await,
                    "engineError" => respond_method_not_supported(&mut peer, id).await,
                    "timeout" => tokio::time::advance(Duration::from_secs(6)).await,
                    "disconnect" => handle.shutdown().await,
                    "malformed" => respond(&mut peer, id, json!({"unexpected":true})).await,
                    _ => unreachable!(),
                }
                let response = task.await.expect("結果が返る");
                assert_eq!(response.is_error, Some(true));
                let payload = data(response);
                let rejected = matches!(failure, "rejected" | "engineError");
                assert_eq!(
                    payload["outcome"],
                    if rejected { "rejected" } else { "unknown" },
                    "{name}/{failure}: {payload}"
                );
                assert_eq!(payload["automaticRetryAllowed"], false);
                assert!(payload["requestId"].is_string());
                let summary = service.history_summary();
                assert!(!summary.can_undo);
                assert_eq!(summary.pending, !rejected && name != "runtime_play");
                if let Some(pending) = summary.pending_group {
                    assert!(!pending.retry_allowed);
                }
                if failure != "disconnect" {
                    assert!(tokio::time::timeout(Duration::from_millis(1), peer.recv())
                        .await
                        .is_err());
                }
                service.shutdown().await;
                handle.shutdown().await;
            }
        }
    }

    #[tokio::test]
    async fn public_writes_wait_behind_ui_and_publish_only_allowed_tools() {
        let auth = enabled_auth();
        let (service, _control, handle, mut peer) =
            test_service_with_peer_and_authorization(8, auth.clone());
        let service = Arc::new(service);
        let context = public_context(&service, ALL_CAPABILITIES);
        let names: Vec<_> = context
            .list_tools()
            .into_iter()
            .filter(|tool| tool.annotations.as_ref().and_then(|a| a.read_only_hint) == Some(false))
            .map(|tool| tool.name.to_string())
            .collect();
        assert_eq!(names.len(), 12, "{names:?}");
        for forbidden in [
            "launch_engine",
            "stop_engine",
            "engine_set_settings",
            "asset_reload_manifest",
            "file_read",
            "file_write",
            "object_invoke",
            "runtime_step",
            "edit_begin_group",
            "edit_end_group",
            "mcp_approve_confirmation",
        ] {
            assert!(context.get_tool(forbidden).is_none(), "{forbidden}");
        }
        let (release, released) = oneshot::channel();
        let (started, start) = oneshot::channel();
        let blocker = service
            .enqueue_from_ui(EditKind::Edit, move |_| async move {
                let _ = started.send(());
                let _ = released.await;
                success(Value::Null)
            })
            .expect("先行UI編集を入れる");
        start.await.expect("UI編集が開始する");
        let lease = auth.current_lease();
        let task = tokio::spawn(async move {
            context
                .call_write_tool("runtime_stop", json!({"params":{}}), Some(lease))
                .await
        });
        wait_for_enqueued(&service).await;
        assert!(tokio::time::timeout(Duration::from_millis(10), peer.recv())
            .await
            .is_err());
        release.send(()).expect("UIを解放する");
        blocker.result().await.expect("UIが終了する");
        let (id, method, _) = next_request(&mut peer).await;
        assert_eq!(method, "runtime.stop");
        respond(&mut peer, id, json!({"accepted":true})).await;
        assert_eq!(
            data(task.await.expect("道具が終了する"))["outcome"],
            "applied"
        );
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn rejected_single_mcp_undo_is_pending_until_ui_discards_it() {
        let auth = enabled_auth();
        let (service, _control, handle, peer) =
            test_service_with_peer_and_authorization(8, auth.clone());
        let service = Arc::new(service);
        seed_history(
            &service,
            HistoryDirection::Undo,
            12,
            HistoryRecord::SetProperty {
                object_id: "node".to_owned(),
                property: "visible".to_owned(),
                old_value: json!(false),
                new_value: json!(true),
            },
        );
        let context = public_context(&service, ALL_CAPABILITIES);
        let responder = tokio::spawn(serve_write(
            peer,
            "object.setProperty",
            json!({"objectId":"node","property":"visible","value":false}),
            json!({"accepted":false}),
        ));
        let response = context
            .call_write_tool("edit_undo", json!({}), Some(auth.current_lease()))
            .await;
        let _peer = responder.await.expect("拒否応答が完了する");
        assert_eq!(data(response)["outcome"], "rejected");
        let pending = service
            .history_summary()
            .pending_group
            .expect("拒否を保留表示する");
        assert_eq!(pending.completed_count, 0);
        assert!(pending.retry_allowed);
        assert!(!pending.outcome_unknown);
        assert!(service
            .enqueue_from_ui(EditKind::RuntimeControl, |_| async { success(Value::Null) })
            .expect("列へ入る")
            .result()
            .await
            .is_err());
        service
            .discard_pending_from_ui()
            .await
            .expect("画面で保留を破棄する");
        assert!(!service.history_summary().pending);
        service
            .enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
            .expect("UI編集が再開する")
            .result()
            .await
            .expect("UIが進む");
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[test]
    fn cancellation_at_queue_start_never_claims_no_write() {
        for started in [false, true] {
            let execution = mcp::McpExecution::default();
            let state = Arc::new(AtomicU8::new(TICKET_WAITING));
            execution.track_queue(state.clone());
            let (sender, _receiver) = oneshot::channel();
            let guard = TicketCancellationGuard {
                _sender: sender,
                queue_state: state.clone(),
            };
            if started {
                state
                    .compare_exchange(
                        TICKET_WAITING,
                        TICKET_STARTED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .expect("列を開始する");
            }
            drop(guard);
            // まだBridge futureをpollしていなくても、開始済みticketの取消は未適用を保証しない。
            assert_eq!(
                execution.outcome(false),
                if started { "unknown" } else { "notApplied" }
            );
            execution.not_sent();
            assert_eq!(execution.outcome(false), "notApplied");
        }
    }

    #[tokio::test]
    async fn runtime_with_explicit_scene_root_preserves_existing_history() {
        let auth = enabled_auth();
        auth.set_write_settings(McpWriteSettings {
            mode: McpWriteMode::Enabled,
            scene_root_id: Some("scene".to_owned()),
        })
        .expect("実ルートを許可する");
        let (service, _control, handle, mut peer) =
            test_service_with_peer_and_authorization(8, auth.clone());
        let service = Arc::new(service);
        seed_history(
            &service,
            HistoryDirection::Undo,
            12,
            HistoryRecord::SetProperty {
                object_id: "node".to_owned(),
                property: "visible".to_owned(),
                old_value: json!(false),
                new_value: json!(true),
            },
        );
        let before = service.history_summary();
        for name in ["runtime_play", "runtime_pause", "runtime_stop"] {
            let context = public_context(&service, ALL_CAPABILITIES);
            let lease = auth.current_lease();
            let task = tokio::spawn(async move {
                context
                    .call_write_tool(name, json!({"params":{}}), Some(lease))
                    .await
            });
            respond_tree(&mut peer, tree()).await;
            respond_tree(&mut peer, tree()).await;
            let (id, method, _) = next_request(&mut peer).await;
            assert_eq!(method, name.replace('_', "."));
            respond(&mut peer, id, json!({"accepted":true})).await;
            assert_eq!(
                data(task.await.expect("実行制御が終了する"))["outcome"],
                "applied"
            );
            assert_eq!(service.history_summary(), before);
        }
        service.shutdown().await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn both_http_versions_dispatch_writes_and_return_structured_outcomes() {
        use crate::mcp::{McpHttpAuth, McpHttpServer};
        use crate::mcp_token::McpTokenStore;
        use tokio_util::sync::CancellationToken;
        for version in ["2026-07-28", "2025-11-25"] {
            let auth = enabled_auth();
            let (service, _control, handle, mut peer) =
                test_service_with_peer_and_authorization(8, auth.clone());
            let service = Arc::new(service);
            let context = public_context(&service, ALL_CAPABILITIES);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("試験HTTPをbindする");
            let port = listener.local_addr().expect("アドレスを得る").port();
            let directory = std::env::temp_dir().join(format!(
                "norves-mcp-write-test-{}-{port}",
                std::process::id()
            ));
            let token = McpTokenStore::default()
                .load_or_create(&directory)
                .expect("試験トークンを作る");
            let bearer = token.expose();
            let server = McpHttpServer::test_with_reads(
                listener,
                Arc::new(McpHttpAuth::with_authorization(token, auth)),
                context,
            );
            let shutdown = CancellationToken::new();
            let server_task = tokio::spawn(server.serve(shutdown.clone()));
            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{port}/mcp");
            let mut session = None;
            if version == "2025-11-25" {
                let response = client.post(&url).bearer_auth(&bearer)
                    .header("Accept", "application/json, text/event-stream")
                    .json(&json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"write-test","version":"1"}}}))
                    .send().await.expect("旧版を初期化する");
                assert!(response.status().is_success());
                session = Some(
                    response
                        .headers()
                        .get("Mcp-Session-Id")
                        .expect("sessionがある")
                        .to_str()
                        .expect("文字列")
                        .to_owned(),
                );
                response.bytes().await.expect("初期化応答を読む");
            }
            let mut display_ids = Vec::new();
            for (id, name, outcome) in [
                (1, "runtime_play", "applied"),
                (2, "runtime_pause", "rejected"),
                (3, "runtime_stop", "unknown"),
            ] {
                let mut body = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":{"params":{}}}});
                if version == "2026-07-28" {
                    body["params"]["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":version,"io.modelcontextprotocol/clientCapabilities":{}});
                }
                let mut request = client
                    .post(&url)
                    .bearer_auth(&bearer)
                    .header("Accept", "application/json, text/event-stream")
                    .header("Mcp-Protocol-Version", version)
                    .header("Mcp-Method", "tools/call")
                    .header("Mcp-Name", name)
                    .json(&body);
                if let Some(session) = &session {
                    request = request.header("Mcp-Session-Id", session);
                }
                let task = tokio::spawn(async move {
                    let response = request.send().await.expect("HTTP道具を呼ぶ");
                    assert!(response.status().is_success());
                    response.text().await.expect("本文を読む")
                });
                let (bridge_id, method, _) = next_request(&mut peer).await;
                assert_eq!(method, name.replace('_', "."));
                if outcome == "unknown" {
                    handle.shutdown().await;
                } else {
                    respond(
                        &mut peer,
                        bridge_id,
                        json!({"accepted":outcome == "applied"}),
                    )
                    .await;
                }
                let text = task.await.expect("HTTP応答が終了する");
                let json_text = text
                    .lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                    .find(|line| line.trim_start().starts_with('{'))
                    .unwrap_or(&text);
                let response: Value = serde_json::from_str(json_text).unwrap_or_else(|error| {
                    panic!("{version}/{name}: JSON応答を読めない: {error}: {text}")
                });
                let payload = &response["result"]["structuredContent"];
                assert_eq!(payload["outcome"], outcome, "{version}: {text}");
                assert_eq!(payload["automaticRetryAllowed"], false);
                assert_eq!(response["result"]["isError"], outcome != "applied");
                display_ids.push(payload["requestId"].as_str().expect("表示ID").to_owned());
            }
            assert!(display_ids.windows(2).all(|pair| pair[0] != pair[1]));
            assert!(!service.history_summary().can_undo);
            shutdown.cancel();
            server_task
                .await
                .expect("HTTPをjoinする")
                .expect("HTTPが終了する");
            service.shutdown().await;
            handle.shutdown().await;
            std::fs::remove_dir_all(&directory).expect("試験専用トークンを消す");
        }
    }
}
