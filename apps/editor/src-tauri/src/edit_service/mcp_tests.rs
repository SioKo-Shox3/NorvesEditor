// 確認と実際のBridge操作を共通列で結び、取消後も結果を取りこぼさないことを検査する。

use super::mcp::McpEditRequest;
use crate::mcp::{
    authorization::McpWriteSettings, log_buffer::LogBuffer, reads::McpReadContext,
    thumbnail::McpThumbnailService, tool_catalog::McpToolCatalog, McpWriteMode,
};
use serde_json::json;

fn confirmed_context(service: &EditService, capabilities: &[&str]) -> McpReadContext {
    let catalog = McpToolCatalog::default();
    catalog.set_connection(
        Some(1),
        &capabilities
            .iter()
            .map(|name| serde_json::from_value(json!({"name":name})).expect("能力を作る"))
            .collect::<Vec<_>>(),
    );
    McpReadContext::new(
        service.bridge.clone(),
        catalog,
        Arc::new(StdMutex::new(LogBuffer::default())),
        McpThumbnailService::default(),
    )
}

async fn confirmation(
    updates: &mut watch::Receiver<Vec<crate::dto::McpConfirmationRequestDto>>,
    previous: Option<&str>,
) -> crate::dto::McpConfirmationRequestDto {
    tokio::time::timeout(
        Duration::from_secs(2),
        updates.wait_for(|requests| {
            requests
                .first()
                .is_some_and(|request| Some(request.id.as_str()) != previous)
        }),
    )
    .await
    .expect("確認が列外へ届く")
    .expect("確認を受信する")[0]
        .clone()
}

async fn respond_tree(peer: &mut LoopbackTransport, tree: Value) {
    let (id, method, _) = next_request(peer).await;
    assert_eq!(method, "scene.getTree");
    respond(peer, id, tree).await;
}

async fn respond_property_review(peer: &mut LoopbackTransport, value: Value) {
    respond_tree(
        peer,
        json!({"root":{"id":"scene","children":[{"id":"node","children":[]}]}}),
    )
    .await;
    let (id, method, params) = next_request(peer).await;
    assert_eq!(method, "object.getSnapshot");
    assert_eq!(params.expect("引数がある")["objectId"], "node");
    respond(peer, id, json!({"objectId":"node", "properties":[{"name":"visible","value":value}], "components":[]})).await;
}

async fn wait_for_enqueued(service: &EditService) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while service.sender.capacity() == service.sender.max_capacity() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("要求が列へ入る");
}

#[tokio::test]
async fn queued_permit_reconfirms_changed_old_value_and_history_without_blocking_ui() {
    queued_reconfirmation_case(McpWriteMode::Confirm).await;
}

#[tokio::test]
async fn queued_enabled_write_reconfirms_changed_old_value_and_history() {
    queued_reconfirmation_case(McpWriteMode::Enabled).await;
}

async fn queued_reconfirmation_case(mode: McpWriteMode) {
    let auth = McpAuthorization::default();
    auth.set_write_settings(McpWriteSettings {
        mode,
        scene_root_id: None,
    })
    .expect("都度確認にする");
    let (service, _control, handle, mut peer) =
        test_service_with_peer_and_authorization(8, auth.clone());
    let service = Arc::new(service);
    let context = confirmed_context(&service, &["object.edit", "object.query", "scene.query"]);
    let (release, released) = oneshot::channel();
    let (started, start) = oneshot::channel();
    let blocker = service
        .enqueue_from_ui(EditKind::Edit, move |_| async move {
            started.send(()).expect("先行UI編集が開始する");
            released.await.expect("先行UI編集を解放する");
            success(json!({"accepted":true}))
        })
        .expect("先行UI編集を列へ入れる");
    start.await.expect("先行UI編集を待つ");
    let lease = auth.current_lease();
    let worker = service.clone();
    let mut updates = auth.confirmations().subscribe();
    let task = tokio::spawn(async move {
        worker
            .submit_confirmed_mcp(
                context,
                lease,
                McpEditRequest::Bridge {
                    method: "object.setProperty",
                    params: json!({"objectId":"node", "property":"visible", "value":null}),
                },
            )
            .await
    });
    respond_property_review(&mut peer, json!(false)).await;
    let first = if mode == McpWriteMode::Confirm {
        let first = confirmation(&mut updates, None).await;
        assert_eq!(first.before, Some(json!(false)));
        assert!(auth.confirmations().approve(&first.id));
        respond_property_review(&mut peer, json!(false)).await;
        Some(first)
    } else {
        assert!(auth.confirmations().pending().is_empty(), "Enabledの初回は確認不要");
        None
    };
    wait_for_enqueued(&service).await;
    release
        .send(())
        .expect("承認後の列待ち中にUI編集を適用する");
    blocker.result().await.expect("UI編集が完了する");
    // 承認後の列内検査では書き込まず、変化を型付き再確認として返す。
    respond_property_review(&mut peer, json!(true)).await;
    respond_property_review(&mut peer, json!(true)).await;
    let second = confirmation(&mut updates, first.as_ref().map(|first| first.id.as_str())).await;
    assert_eq!(second.before, Some(json!(true)));
    if let Some(first) = first {
        assert_ne!(second.history_revision, first.history_revision);
        assert!(!auth.confirmations().approve(&first.id));
    }
    service
        .enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
        .expect("再確認中もUIを受け付ける")
        .result()
        .await
        .expect("再確認中もUIが完了する");
    assert!(service.history_lock_available());
    assert!(auth.confirmations().approve(&second.id));
    respond_property_review(&mut peer, json!(true)).await;
    respond_property_review(&mut peer, json!(true)).await;
    let (id, method, params) = next_request(&mut peer).await;
    assert_eq!(method, "object.setProperty");
    assert_eq!(params.expect("書き込み引数がある")["value"], Value::Null);
    respond(&mut peer, id, json!({"accepted":true, "appliedValue":null})).await;
    assert_eq!(
        task.await
            .expect("要求が終了する")
            .expect("書き込みが成功する")["accepted"],
        true
    );
    let action = service
        .history_action_for_mcp(McpHistoryDirection::Undo)
        .expect("履歴がある");
    assert!(
        matches!(&action.records[0], HistoryRecord::SetProperty { old_value, new_value, .. }
        if old_value == &json!(true) && new_value == &Value::Null)
    );
    assert!(auth.confirmations().pending().is_empty());
    service.shutdown().await;
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn confirmed_queue_cancellation_before_start_sends_no_bridge_request() {
    for reason in ["http", "authorization", "deadline", "generation"] {
        let auth = McpAuthorization::default();
        auth.set_write_settings(McpWriteSettings {
            mode: McpWriteMode::Enabled,
            scene_root_id: None,
        })
        .expect("書き込みを許可する");
        let (service, control, handle, mut peer) =
            test_service_with_peer_and_authorization(8, auth.clone());
        let service = Arc::new(service);
        let context = confirmed_context(&service, &["runtime.control"]);
        let (release, released) = oneshot::channel();
        let (started, start) = oneshot::channel();
        let blocker = service
            .enqueue_from_ui(EditKind::Edit, move |_| async move {
                let _ = started.send(());
                let _ = released.await;
                success(Value::Null)
            })
            .expect("先行操作を列へ入れる");
        start.await.expect("先行操作が開始する");
        let lease = auth.current_lease();
        let task_lease = lease.clone();
        let worker = service.clone();
        let task = tokio::spawn(async move {
            worker
                .submit_confirmed_mcp(
                    context,
                    task_lease,
                    McpEditRequest::Bridge {
                        method: "runtime.play",
                        params: json!({}),
                    },
                )
                .await
        });
        wait_for_enqueued(&service).await;
        match reason {
            "http" => lease.cancel_request(),
            "authorization" => {
                auth.revoke();
            }
            "deadline" => tokio::time::advance(Duration::from_secs(125)).await,
            "generation" => control.disconnect(),
            _ => unreachable!(),
        }
        let _ = release.send(());
        let _ = blocker.result().await;
        assert!(task.await.expect("取消要求が終了する").is_err(), "{reason}");
        assert!(
            tokio::time::timeout(Duration::from_millis(20), peer.recv())
                .await
                .is_err(),
            "{reason}"
        );
        service.shutdown().await;
        handle.shutdown().await;
    }
}

#[tokio::test(start_paused = true)]
async fn started_confirmed_write_preserves_applied_rejected_and_unknown_results_after_cancellation()
{
    for reason in ["http", "authorization", "deadline"] {
        for result in ["applied", "rejected", "unknown"] {
            let auth = McpAuthorization::default();
            auth.set_write_settings(McpWriteSettings {
                mode: McpWriteMode::Enabled,
                scene_root_id: None,
            })
            .expect("書き込みを許可する");
            let (service, _control, handle, mut peer) =
                test_service_with_peer_and_authorization(8, auth.clone());
            let service = Arc::new(service);
            let context = confirmed_context(&service, &["scene.edit", "scene.query"]);
            let lease = auth.current_lease();
            if reason == "deadline" {
                tokio::time::advance(Duration::from_secs(124)).await;
            }
            let operations = context.operations.clone();
            let operation = operations.begin("scene_create_object", &json!({}));
            let execution = Arc::new(mcp::McpExecution::with_operation(operation, Some(lease.clone())));
            let task_lease = lease.clone();
            let worker = service.clone();
            let task = tokio::spawn(async move {
                worker
                    .submit_tracked_mcp(
                        context,
                        task_lease,
                        McpEditRequest::Bridge {
                            method: "scene.createObject",
                            params: json!({"kind":"object"}),
                        },
                        execution,
                    )
                    .await
            });
            let tree = json!({"root":{"id":"scene","children":[]}});
            respond_tree(&mut peer, tree.clone()).await;
            respond_tree(&mut peer, tree).await;
            let (id, method, _) = next_request(&mut peer).await;
            assert_eq!(method, "scene.createObject");
            assert!(service.history_lock_available());
            match reason {
                "http" => lease.cancel_request(),
                "authorization" => {
                    auth.revoke();
                }
                "deadline" => tokio::time::advance(Duration::from_secs(1)).await,
                _ => unreachable!(),
            }
            assert!(task.await.expect("HTTP側は取消で終了する").is_err());
            let (sentinel, mut sentinel_rx) = oneshot::channel();
            let next = service
                .enqueue_from_ui(EditKind::Edit, move |_| async move {
                    let _ = sentinel.send(());
                    success(Value::Null)
                })
                .expect("後続UI操作を列へ入れる");
            tokio::task::yield_now().await;
            assert!(matches!(
                sentinel_rx.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            match result {
                "applied" => {
                    respond(&mut peer, id, json!({"accepted":true,"newId":"created"})).await
                }
                "rejected" => respond(&mut peer, id, json!({"accepted":false})).await,
                "unknown" => tokio::time::advance(Duration::from_secs(6)).await,
                _ => unreachable!(),
            }
            let outcome = next.result().await;
            let record = operations.snapshot().records.pop().expect("取消後もactorが結果を記録する");
            assert_eq!(record.outcome, result);
            assert!(record.actor_finished);
            assert_eq!(record.completed_count, usize::from(result == "applied"));
            let summary = service.history_summary();
            assert_eq!(record.pending, summary.pending);
            if result == "unknown" {
                assert!(outcome.is_err());
                let pending = summary.pending_group.expect("結果不明を履歴へ保留する");
                assert!(pending.outcome_unknown);
                assert!(!pending.retry_allowed);
                assert_eq!(pending.completed_count, 0);
            } else {
                outcome.expect("Bridge結果の確定後にUIが進む");
                assert!(!summary.pending);
                assert_eq!(summary.can_undo, result == "applied");
                assert_eq!(summary.applied_revision, u64::from(result == "applied"));
            }
            service.shutdown().await;
            handle.shutdown().await;
        }
    }
}

#[tokio::test(start_paused = true)]
async fn cancelled_mcp_undo_keeps_first_bridge_result_and_never_starts_second_delete() {
    for reason in ["http", "authorization", "deadline"] {
        for unknown in [false, true] {
            let auth = McpAuthorization::default();
            auth.set_write_settings(McpWriteSettings {
                mode: McpWriteMode::Enabled,
                scene_root_id: None,
            })
            .expect("書き込みを許可する");
            let (service, _control, handle, mut peer) =
                test_service_with_peer_and_authorization(8, auth.clone());
            let group = service
                .begin_group_from_mcp("2個のノード".to_owned())
                .await
                .expect("まとまりを始める");
            for id in ["first", "second"] {
                enqueue_grouped_create(&service, group, None, None, id.to_owned())
                    .result()
                    .await
                    .expect("作成履歴を積む");
            }
            service
                .end_group_from_mcp(group)
                .await
                .expect("まとまりを閉じる");
            let service = Arc::new(service);
            let context = confirmed_context(&service, &["scene.edit", "scene.query"]);
            let lease = auth.current_lease();
            let operations = context.operations.clone();
            let operation = operations.begin("edit_undo", &json!({}));
            let execution = Arc::new(mcp::McpExecution::with_operation(operation, Some(lease.clone())));
            let task_lease = lease.clone();
            let worker = service.clone();
            let mut updates = auth.confirmations().subscribe();
            let task = tokio::spawn(async move {
                worker
                    .submit_tracked_mcp(
                        context,
                        task_lease,
                        McpEditRequest::History(McpHistoryDirection::Undo),
                        execution,
                    )
                    .await
            });
            let tree = json!({"root":{"id":"scene","children":[{"id":"first","children":[]},{"id":"second","children":[]}]}});
            respond_tree(&mut peer, tree.clone()).await;
            let confirm = confirmation(&mut updates, None).await;
            assert_eq!(confirm.method, "edit.history");
            assert!(auth.confirmations().approve(&confirm.id));
            if reason == "deadline" {
                tokio::time::advance(Duration::from_secs(124)).await;
            }
            respond_tree(&mut peer, tree.clone()).await;
            respond_tree(&mut peer, tree).await;
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, "scene.deleteObject");
            assert_eq!(params.expect("削除対象がある")["objectId"], "second");
            match reason {
                "http" => lease.cancel_request(),
                "authorization" => {
                    auth.revoke();
                }
                "deadline" => tokio::time::advance(Duration::from_secs(1)).await,
                _ => unreachable!(),
            }
            assert!(task.await.expect("要求が終了する").is_err());
            if unknown {
                tokio::time::advance(Duration::from_secs(6)).await;
            } else {
                respond(&mut peer, id, json!({"accepted":true})).await;
            }
            let sentinel = service
                .enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
                .expect("後続を入れる");
            assert!(sentinel.result().await.is_err());
            let summary = service.history_summary();
            let record = operations.snapshot().records.pop().expect("部分成功を記録する");
            assert_eq!(record.outcome, if unknown { "unknown" } else { "partial" });
            assert_eq!(record.completed_count, usize::from(!unknown));
            assert!(record.pending && !record.retry_allowed);
            assert!(record.display_group_id.is_some());
            let pending = summary.pending_group.expect("未処理の残りを保留する");
            assert_eq!(pending.completed_count, usize::from(!unknown));
            assert_eq!(pending.total_count, 2);
            assert_eq!(pending.outcome_unknown, unknown);
            assert!(!pending.retry_allowed);
            assert!(tokio::time::timeout(Duration::from_millis(20), peer.recv())
                .await
                .is_err());
            service.shutdown().await;
            handle.shutdown().await;
        }
    }
}

#[tokio::test]
async fn queued_delete_and_undo_reconfirm_the_changed_history_impact_and_head() {
    for undo in [false, true] {
        let auth = McpAuthorization::default();
        auth.set_write_settings(McpWriteSettings {
            mode: McpWriteMode::Enabled,
            scene_root_id: None,
        })
        .expect("書き込みを許可する");
        let (service, _control, handle, mut peer) =
            test_service_with_peer_and_authorization(8, auth.clone());
        seed_history(
            &service,
            HistoryDirection::Undo,
            90,
            HistoryRecord::Create {
                created_id: "first".to_owned(),
                parent_id: None,
                kind: None,
            },
        );
        let service = Arc::new(service);
        let context = confirmed_context(&service, &["scene.edit", "scene.query"]);
        let (release, released) = oneshot::channel();
        let (started, start) = oneshot::channel();
        let blocker = service
            .enqueue_recorded_from_ui(
                EditKind::Edit,
                HistoryRequest::CreateObject {
                    parent_id: None,
                    kind: None,
                },
                move |_| async move {
                    let _ = started.send(());
                    released.await.expect("UIの作成を解放する");
                    Ok(QueuedEditResult::plain(
                        json!({"accepted":true,"newId":"second"}),
                    ))
                },
            )
            .expect("作成を列へ入れる");
        start.await.expect("先行UI操作が開始する");
        let worker = service.clone();
        let lease = auth.current_lease();
        let request = if undo {
            McpEditRequest::History(McpHistoryDirection::Undo)
        } else {
            McpEditRequest::Bridge {
                method: "scene.deleteObject",
                params: json!({"objectId":"first"}),
            }
        };
        let mut updates = auth.confirmations().subscribe();
        let task =
            tokio::spawn(async move { worker.submit_confirmed_mcp(context, lease, request).await });
        let tree = json!({"root":{"id":"scene","children":[{"id":"first","children":[]},{"id":"second","children":[]}]}});
        respond_tree(&mut peer, tree.clone()).await;
        let first = confirmation(&mut updates, None).await;
        assert_eq!(first.clears_history, !undo);
        assert!(auth.confirmations().approve(&first.id));
        respond_tree(&mut peer, tree.clone()).await;
        wait_for_enqueued(&service).await;
        release.send(()).expect("列待ち中に履歴先頭を変える");
        blocker.result().await.expect("新しい履歴を記録する");
        if !undo {
            respond_tree(&mut peer, tree.clone()).await;
        }
        respond_tree(&mut peer, tree.clone()).await;
        let second = confirmation(&mut updates, Some(&first.id)).await;
        assert_ne!(first.history_revision, second.history_revision);
        assert_ne!(first.undo_head_id, second.undo_head_id);
        assert_eq!(second.target_ids, [if undo { "second" } else { "first" }]);
        assert!(!auth.confirmations().approve(&first.id));
        assert!(auth.confirmations().approve(&second.id));
        respond_tree(&mut peer, tree.clone()).await;
        respond_tree(&mut peer, tree).await;
        let (id, method, params) = next_request(&mut peer).await;
        assert_eq!(method, "scene.deleteObject");
        assert_eq!(
            params.expect("削除対象がある")["objectId"],
            if undo { "second" } else { "first" }
        );
        respond(&mut peer, id, json!({"accepted":true})).await;
        task.await
            .expect("要求が終了する")
            .expect("新しい影響を承認して実行する");
        let summary = service.history_summary();
        assert_eq!(summary.can_redo, undo);
        assert_eq!(summary.can_undo, undo);
        service.shutdown().await;
        handle.shutdown().await;
    }
}

#[tokio::test(start_paused = true)]
async fn queued_revalidation_uses_the_remaining_request_deadline_and_cannot_write_after_it() {
    let auth = McpAuthorization::default();
    auth.set_write_settings(McpWriteSettings {
        mode: McpWriteMode::Enabled,
        scene_root_id: None,
    })
    .expect("書き込みを許可する");
    let (service, _control, handle, mut peer) =
        test_service_with_peer_and_authorization(8, auth.clone());
    let service = Arc::new(service);
    let context = confirmed_context(&service, &["scene.edit", "scene.query"]);
    let lease = auth.current_lease();
    tokio::time::advance(Duration::from_secs(124)).await;
    let worker = service.clone();
    let task = tokio::spawn(async move {
        worker
            .submit_confirmed_mcp(
                context,
                lease,
                McpEditRequest::Bridge {
                    method: "scene.createObject",
                    params: json!({}),
                },
            )
            .await
    });
    respond_tree(&mut peer, json!({"root":{"id":"scene","children":[]}})).await;
    let (_id, method, _) = next_request(&mut peer).await;
    assert_eq!(method, "scene.getTree");
    assert!(service.history_lock_available());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(task.await.expect("全体期限で終わる").is_err());
    service
        .enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
        .expect("UI要求を入れる")
        .result()
        .await
        .expect("再照会の期限切れ後も列が進む");
    assert!(!service.history_summary().pending);
    assert!(tokio::time::timeout(Duration::from_millis(20), peer.recv())
        .await
        .is_err());
    service.shutdown().await;
    handle.shutdown().await;
}

#[tokio::test]
async fn confirmed_submission_rejects_read_only_delete_remove_and_internal_undo_delete() {
    let auth = McpAuthorization::default();
    let (service, _control, handle, mut peer) =
        test_service_with_peer_and_authorization(8, auth.clone());
    seed_history(
        &service,
        HistoryDirection::Undo,
        1,
        HistoryRecord::Create {
            created_id: "node".to_owned(),
            parent_id: None,
            kind: None,
        },
    );
    let context = confirmed_context(
        &service,
        &[
            "scene.edit",
            "scene.query",
            "object.query",
            "component.edit",
        ],
    );
    for request in [
        McpEditRequest::Bridge {
            method: "scene.deleteObject",
            params: json!({"objectId":"node"}),
        },
        McpEditRequest::Bridge {
            method: "component.remove",
            params: json!({"objectId":"component"}),
        },
        McpEditRequest::History(McpHistoryDirection::Undo),
    ] {
        let error = service
            .submit_confirmed_mcp(context.clone(), auth.current_lease(), request)
            .await
            .expect_err("read-onlyを拒否する");
        assert!(error.to_string().contains("読み取り専用"));
        assert!(auth.confirmations().pending().is_empty());
    }
    assert!(tokio::time::timeout(Duration::from_millis(20), peer.recv())
        .await
        .is_err());
    service.shutdown().await;
    handle.shutdown().await;
}
