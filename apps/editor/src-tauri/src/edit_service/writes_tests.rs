// 公開道具から許可・共通列・Bridge応答・画面の履歴操作までを検査する。
mod writes_tests {
    use super::*;
    use crate::mcp::tool_catalog::WritePermission;

    include!("groups_tests.rs");
    include!("operations_tests.rs");

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
            .with_write_service(service.clone());
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
            let operations = context.operations.clone();
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
            let record = operations.snapshot().records.pop().expect("操作記録がある");
            assert_eq!(record.request_id, result["requestId"]);
            assert_eq!(record.outcome, "applied");
            assert!(record.actor_finished);
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
            let operations = context.operations.clone();
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
            let record = operations.snapshot().records.pop().expect("未送信の記録がある");
            assert_eq!(record.request_id, payload["requestId"]);
            assert_eq!(record.outcome, "notApplied");
            assert!(record.actor_finished, "列へ入らない確定拒否は未確定表示にしない: {reason}");
            assert_eq!(record.result, if reason == "expired" { "timedOut" } else if reason == "cancelled" { "cancelled" } else { "rejected" });
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
                let operations = context.operations.clone();
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
                let record = operations.snapshot().records.pop().expect("失敗結果を記録する");
                assert_eq!(record.request_id, payload["requestId"]);
                assert_eq!(record.outcome, payload["outcome"]);
                if failure == "timeout" { assert_eq!(record.result, "timedOut"); }
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
        assert_eq!(names.len(), 14, "{names:?}");
        for forbidden in [
            "launch_engine",
            "stop_engine",
            "engine_set_settings",
            "asset_reload_manifest",
            "file_read",
            "file_write",
            "object_invoke",
            "runtime_step",
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
        let payload = data(response);
        assert_eq!(payload["outcome"], "rejected");
        assert_eq!(payload["pending"], true);
        assert_eq!(payload["retryAllowed"], true);
        let record = context.operations.snapshot().records.pop().expect("保留記録がある");
        assert_eq!(record.request_id, payload["requestId"]);
        assert_eq!(record.outcome, "rejected");
        assert!(record.pending && record.retry_allowed);
        assert!(record.summary.contains("保留"));
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
    async fn public_group_prequeue_denials_are_terminal() {
        for name in ["edit_begin_group", "edit_end_group"] {
            for reason in ["revoked", "noLease", "disconnected", "stopped", "full"] {
                let auth = enabled_auth();
                let (service, control, handle, mut peer) =
                    test_service_with_peer_and_authorization(1, auth.clone());
                let service = Arc::new(service);
                let context = public_context(&service, ALL_CAPABILITIES);
                let lease = auth.current_lease();
                let mut release = None;
                match reason {
                    "revoked" => { auth.revoke(); }
                    "disconnected" => control.disconnect(),
                    "stopped" => service.shutdown().await,
                    "full" => {
                        let (sender, receiver) = oneshot::channel();
                        let (started, start) = oneshot::channel();
                        let _blocker = service.enqueue_from_ui(EditKind::Edit, move |_| async move {
                            started.send(()).expect("先行操作を開始する");
                            receiver.await.expect("先行操作を解放する");
                            success(Value::Null)
                        }).expect("先行操作を受け付ける");
                        start.await.expect("先行操作が開始する");
                        let _queued = service.enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
                            .expect("待ち列を埋める");
                        release = Some(sender);
                        // ticketを破棄しても、actorが取り出すまでは待ち列の容量を消費する。
                    }
                    _ => {}
                }
                let sequence = service.admission.lock().unwrap().next_sequence;
                let args = if name == "edit_begin_group" {
                    json!({"name":"拒否されるまとまり"})
                } else {
                    json!({"groupId":"owner-secret-must-not-be-recorded"})
                };
                let response = context.call_write_tool(name, args, (reason != "noLease").then_some(lease)).await;
                assert_eq!(response.is_error, Some(true), "{name}/{reason}");
                let payload = data(response);
                assert_eq!(payload["outcome"], "notApplied", "{name}/{reason}");
                assert_eq!(service.admission.lock().unwrap().next_sequence, sequence);
                let record = context.operations.snapshot().records.pop().expect("拒否記録がある");
                assert_eq!(record.request_id, payload["requestId"]);
                assert_eq!(record.result, payload["operationResult"]);
                assert_eq!(record.result, if matches!(reason, "revoked" | "noLease") { "rejected" } else { "failed" });
                assert_eq!(record.outcome, "notApplied");
                assert!(record.actor_finished, "{name}/{reason}");
                assert_eq!(record.completed_count, 0);
                assert!(!record.pending && !record.retry_allowed && !record.automatic_retry_allowed);
                assert!(!serde_json::to_string(&record).unwrap().contains("owner-secret-must-not-be-recorded"));
                if let Some(release) = release { release.send(()).expect("先行操作を解放する"); }
                assert!(tokio::time::timeout(Duration::from_millis(10), peer.recv()).await.is_err());
                service.shutdown().await;
                handle.shutdown().await;
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn queued_group_cancellation_waits_for_actor_and_preserves_terminal_result() {
        for name in ["edit_begin_group", "edit_end_group"] {
            for reason in ["cancelled", "dropped", "revoked", "deadline"] {
                let auth = enabled_auth();
                let (service, _control, handle, mut peer) =
                    test_service_with_peer_and_authorization(8, auth.clone());
                let service = Arc::new(service);
                let context = public_context(&service, ALL_CAPABILITIES);
                let operations = context.operations.clone();
                let (release, released) = oneshot::channel();
                let (started, start) = oneshot::channel();
                let blocker = service.enqueue_from_ui(EditKind::Edit, move |_| async move {
                    started.send(()).expect("先行操作を開始する");
                    released.await.expect("先行操作の終了を待つ");
                    success(Value::Null)
                }).expect("先行操作を列へ入れる");
                start.await.expect("先行操作が開始した");
                let lease = auth.current_lease();
                let caller_lease = lease.clone();
                let args = if name == "edit_begin_group" {
                    json!({"name":"列待ちのまとまり"})
                } else {
                    json!({"groupId":"queued-owner-secret"})
                };
                let task = tokio::spawn(async move {
                    context.call_write_tool(name, args, Some(caller_lease)).await
                });
                wait_for_enqueued(&service).await;
                match reason {
                    "cancelled" => lease.cancel_request(),
                    "dropped" => task.abort(),
                    "revoked" => { auth.revoke(); }
                    "deadline" => tokio::time::advance(Duration::from_secs(126)).await,
                    _ => unreachable!(),
                }
                let response = task.await;
                if reason == "dropped" {
                    assert!(response.expect_err("呼出futureが終了する").is_cancelled());
                } else {
                    let response = response.expect("呼出元が終了する");
                    assert_eq!(response.is_error, Some(true));
                    let payload = data(response);
                    assert_eq!(payload["outcome"], "notApplied");
                    assert_eq!(payload["automaticRetryAllowed"], false);
                }
                let before = operations.snapshot().records.pop().expect("取消記録がある");
                assert!(!before.actor_finished, "列待ち中はactorの確定を待つ: {name}/{reason}");
                assert_eq!(before.outcome, "notApplied");
                assert_eq!(before.result, match reason { "revoked" => "rejected", "deadline" => "timedOut", _ => "cancelled" });
                release.send(()).expect("先行操作を解放する");
                blocker.result().await.expect("先行操作が終わる");
                service.enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
                    .expect("番兵を列へ入れる").result().await.expect("actorが確定する");
                let after = operations.snapshot().records.pop().expect("確定結果がある");
                assert_eq!(after.request_id, before.request_id);
                assert_eq!(after.outcome, "notApplied");
                assert_eq!(after.result, before.result);
                assert!(after.actor_finished);
                assert_eq!(after.completed_count, 0);
                assert!(!after.pending && !after.retry_allowed && !after.automatic_retry_allowed);
                assert!(!service.history.lock().unwrap().has_active_group());
                assert!(!serde_json::to_string(&after).unwrap().contains("queued-owner-secret"));
                assert!(tokio::time::timeout(Duration::from_millis(10), peer.recv()).await.is_err());
                service.shutdown().await;
                handle.shutdown().await;
            }
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
            let catalog = McpToolCatalog::default();
            catalog.set_connection(Some(1), &ALL_CAPABILITIES.iter().map(|name| {
                serde_json::from_value(json!({"name":name})).expect("能力を作る")
            }).collect::<Vec<_>>());
            let context = McpReadContext::new(
                service.bridge.clone(), catalog.clone(),
                Arc::new(StdMutex::new(LogBuffer::default())), McpThumbnailService::default(),
            ).with_write_service(service.clone());
            context.set_write_permission(WritePermission::Enabled);
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
            let operations = crate::mcp::operations::OperationStore::new(Some(directory.clone()));
            let context = context.with_operations(operations.clone());
            let server = McpHttpServer::test_with_reads(
                listener,
                Arc::new(McpHttpAuth::with_authorization(token, auth.clone())),
                context.clone(),
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
            let mut group_id = Value::Null;
            for (id, name, expected) in [(10, "edit_begin_group", "applied"), (11, "edit_end_group", "applied"), (12, "edit_end_group", "notApplied")] {
                let arguments = if name == "edit_begin_group" { json!({"name":"HTTPのまとまり"}) } else { json!({"groupId":group_id}) };
                let mut body = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}});
                if version == "2026-07-28" {
                    body["params"]["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":version,"io.modelcontextprotocol/clientCapabilities":{}});
                }
                // 要求ごとに別のHTTP接続を使う。所有権は返された秘密IDだけで継続する。
                let mut request = reqwest::Client::new().post(&url).bearer_auth(&bearer)
                    .header("Accept", "application/json, text/event-stream")
                    .header("Mcp-Protocol-Version", version)
                    .header("Mcp-Method", "tools/call")
                    .header("Mcp-Name", name).json(&body);
                if let Some(session) = &session {
                    request = request.header("Mcp-Session-Id", session);
                }
                let response = request.send().await.expect("まとまり道具をHTTPで呼ぶ");
                assert!(response.status().is_success());
                let text = response.text().await.expect("まとまり応答を読む");
                let json_text = text.lines().filter_map(|line| line.strip_prefix("data: ")).find(|line| line.trim_start().starts_with('{')).unwrap_or(&text);
                let response: Value = serde_json::from_str(json_text).expect("応答を解析する");
                let payload = &response["result"]["structuredContent"];
                assert_eq!(payload["outcome"], expected, "{version}/{name}");
                let record = operations.snapshot().records.pop().expect("制御の確定記録がある");
                assert_eq!(record.request_id, payload["requestId"]);
                assert_eq!(record.outcome, expected);
                assert_eq!(record.result, if expected == "applied" { "success" } else { "rejected" });
                assert!(record.actor_finished);
                assert!(!record.automatic_retry_allowed);
                if name == "edit_begin_group" {
                    group_id = payload["result"]["groupId"].clone();
                    assert_eq!(group_id.as_str().expect("秘密ID").len(), 64);
                    assert_ne!(payload["result"]["displayGroupId"], group_id);
                } else {
                    assert!(!text.contains(group_id.as_str().expect("秘密ID")));
                }
            }
            let mut display_ids = Vec::new();
            for (id, name, outcome, cancel) in [
                (1, "runtime_play", "applied", false),
                (2, "runtime_pause", "rejected", false),
                (4, "runtime_play", "applied", true),
                (5, "runtime_pause", "rejected", true),
                (3, "runtime_stop", "unknown", false),
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
                if cancel {
                    let before_revision = operations.snapshot().revision;
                    let mut cancellation = client.post(&url).bearer_auth(&bearer)
                        .header("Accept", "application/json, text/event-stream")
                        .header("Mcp-Method", "notifications/cancelled")
                        .header("Mcp-Protocol-Version", version)
                        .json(&json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}));
                    if let Some(session) = &session { cancellation = cancellation.header("Mcp-Session-Id", session); }
                    let response = cancellation.send().await.expect("取消通知が届く");
                    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);
                    response.bytes().await.expect("取消応答を読む");
                    let text = tokio::time::timeout(Duration::from_secs(2), task)
                        .await.expect("取消後にHTTP本文が終了する")
                        .expect("HTTP取消が終了する");
                    let json_text = text.lines()
                        .filter_map(|line| line.strip_prefix("data:"))
                        .map(str::trim)
                        .find(|line| line.starts_with('{'))
                        .or_else(|| text.trim_start().starts_with('{').then_some(text.as_str()));
                    let response_request_id = if version == "2026-07-28" {
                        let response: Value = serde_json::from_str(json_text.expect("現行版は取消結果を返す"))
                            .expect("取消応答を解析する");
                        assert_eq!(response["id"], id);
                        assert_eq!(response["result"]["isError"], true);
                        let payload = &response["result"]["structuredContent"];
                        assert_eq!(payload["outcome"], "unknown");
                        assert_eq!(payload["operationResult"], "unknown");
                        assert_eq!(payload["automaticRetryAllowed"], false);
                        Some(payload["requestId"].as_str().expect("取消結果の表示ID").to_owned())
                    } else {
                        // 旧版のSDKは取消時に要求別SSEを閉じ、道具の結果を返さない。
                        assert!(json_text.is_none(), "旧版の取消はJSON-RPC結果なしで本文が終了する");
                        None
                    };
                    // 旧版はHTTPを先に閉じる場合がある。対象要求の記録まで明示的に同期する。
                    let before = tokio::time::timeout(Duration::from_secs(2), async {
                        loop {
                            let snapshot = operations.snapshot();
                            if snapshot.revision > before_revision {
                                if let Some(record) = snapshot.records.last().filter(|record| record.tool == name && record.outcome == "unknown") {
                                    break record.clone();
                                }
                            }
                            tokio::task::yield_now().await;
                        }
                    }).await.expect("取消された要求の結果不明記録が届く");
                    assert_eq!(before.outcome, "unknown");
                    assert_eq!(before.result, "unknown");
                    if let Some(request_id) = response_request_id {
                        assert_eq!(request_id, before.request_id, "応答と操作記録は同じ表示IDを使う");
                    }
                    assert!(service.history_lock_available());
                    respond(&mut peer, bridge_id, json!({"accepted":outcome == "applied"})).await;
                    service.enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
                        .expect("結果確定を待つ番兵を入れる").result().await.expect("actorが結果を確定する");
                    let after = operations.snapshot().records.pop().expect("確定記録がある");
                    assert_eq!(after.request_id, before.request_id);
                    assert_eq!(after.outcome, outcome);
                    assert_eq!(after.result, if outcome == "applied" { "success" } else { "rejected" });
                    assert!(after.actor_finished);
                    assert!(!after.automatic_retry_allowed);
                    assert!(tokio::time::timeout(Duration::from_millis(10), peer.recv()).await.is_err());
                    println!("MCP_ACCEPTANCE_OK cancel-response-record {version} {outcome}");
                    continue;
                }
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
            let records = operations.snapshot().records;
            for id in &display_ids { assert!(records.iter().any(|record| &record.request_id == id)); }
            let file = std::fs::read_to_string(directory.join("mcp-operations.jsonl")).expect("実HTTP操作の保存を読む");
            assert!(!file.contains(&bearer));
            assert!(!file.contains(group_id.as_str().expect("所有者秘密")));
            assert!(records[0].display_group_id.is_some());
            assert_eq!(records[0].display_group_id, records[1].display_group_id);
            assert!(!service.history_summary().can_undo);
            for (case, reason) in ["invalid", "readOnly", "backendReadOnly", "unavailable"].into_iter().enumerate() {
                context.set_write_permission(if reason == "readOnly" { WritePermission::ReadOnly } else { WritePermission::Enabled });
                auth.set_write_settings(McpWriteSettings {
                    mode: if reason == "backendReadOnly" { McpWriteMode::ReadOnly } else { McpWriteMode::Enabled },
                    scene_root_id: None,
                }).expect("拒否条件を設定する");
                if reason == "unavailable" { catalog.set_connection(None, &[]); }
                for (index, name) in ["edit_begin_group", "edit_end_group"].into_iter().enumerate() {
                    // endは後片付けのため、サービスのReadOnly単独では拒否しない。
                    if reason == "backendReadOnly" && name == "edit_end_group" { continue; }
                    let sequence = service.admission.lock().unwrap().next_sequence;
                    let arguments = if reason == "invalid" {
                        json!({"unexpected":"confirmation-secret-must-not-be-recorded"})
                    } else if name == "edit_begin_group" {
                        json!({"name":"拒否されるまとまり"})
                    } else {
                        json!({"groupId":group_id})
                    };
                    let id = 100 + case * 2 + index;
                    let mut body = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}});
                    if version == "2026-07-28" {
                        body["params"]["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":version,"io.modelcontextprotocol/clientCapabilities":{}});
                    }
                    let mut request = client.post(&url).bearer_auth(&bearer)
                        .header("Accept", "application/json, text/event-stream")
                        .header("Mcp-Protocol-Version", version)
                        .header("Mcp-Method", "tools/call")
                        .header("Mcp-Name", name).json(&body);
                    if let Some(session) = &session { request = request.header("Mcp-Session-Id", session); }
                    let response = request.send().await.expect("拒否される制御をHTTPで呼ぶ");
                    assert!(response.status().is_success());
                    let text = response.text().await.expect("拒否応答を読む");
                    let json_text = text.lines().filter_map(|line| line.strip_prefix("data: ")).find(|line| line.trim_start().starts_with('{')).unwrap_or(&text);
                    let response: Value = serde_json::from_str(json_text).expect("拒否応答を解析する");
                    assert_eq!(response["result"]["isError"], true, "{version}/{name}/{reason}");
                    let payload = &response["result"]["structuredContent"];
                    assert_eq!(payload["outcome"], "notApplied");
                    assert_eq!(payload["operationResult"], "rejected");
                    assert_eq!(payload["automaticRetryAllowed"], false);
                    let detail = payload["detail"].as_str().expect("拒否理由がある");
                    assert!(detail.contains(match reason {
                        "invalid" => "schema", "readOnly" => "読み取り専用",
                        "backendReadOnly" => "許可", "unavailable" => "接続", _ => unreachable!(),
                    }), "{version}/{name}/{reason}: {detail}");
                    assert_eq!(service.admission.lock().unwrap().next_sequence, sequence);
                    let record = operations.snapshot().records.pop().expect("列前の拒否を記録する");
                    assert_eq!(record.request_id, payload["requestId"]);
                    assert_eq!(record.result, "rejected");
                    assert_eq!(record.outcome, "notApplied");
                    assert!(record.actor_finished, "{version}/{name}/{reason}");
                    assert_eq!(record.completed_count, 0);
                    assert!(record.display_group_id.is_none());
                    assert!(!record.pending && !record.retry_allowed && !record.automatic_retry_allowed);
                    let saved = std::fs::read_to_string(directory.join("mcp-operations.jsonl")).expect("列前拒否の保存を読む");
                    let saved_record: Value = serde_json::from_str(saved.lines().last().expect("保存記録がある")).unwrap();
                    assert_eq!(saved_record, serde_json::to_value(&record).unwrap());
                    for secret in [&bearer, group_id.as_str().unwrap(), "confirmation-secret-must-not-be-recorded"] {
                        assert!(!saved.contains(secret));
                    }
                    println!("MCP_GROUP_TERMINAL_OK {version} {name} {reason}");
                }
            }
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
