// 公開groupIdの所有権、割込み、期限を実際の道具と共通編集列で検査する。

async fn begin_public_group(context: &McpReadContext, auth: &McpAuthorization) -> Value {
    let result = data(
        context
            .call_write_tool(
                "edit_begin_group",
                json!({"name":"名前付き編集"}),
                Some(auth.current_lease()),
            )
            .await,
    );
    assert_eq!(result["outcome"], "applied");
    let secret = result["result"]["groupId"].as_str().expect("秘密IDを返す");
    assert_eq!(secret.len(), 64);
    assert!(secret.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(result["result"]["displayGroupId"], secret);
    result["result"].clone()
}

async fn public_property(
    context: &McpReadContext,
    auth: &McpAuthorization,
    peer: LoopbackTransport,
    group: Option<&Value>,
    object: &str,
) -> LoopbackTransport {
    let params = json!({"objectId":object,"property":"visible","value":true});
    let mut arguments = json!({"params":params});
    if let Some(group) = group {
        arguments["groupId"] = group["groupId"].clone();
    }
    let responder = tokio::spawn(serve_write(
        peer,
        "object.setProperty",
        params,
        json!({"accepted":true,"appliedValue":true}),
    ));
    let result = data(
        context
            .call_write_tool("object_set_property", arguments, Some(auth.current_lease()))
            .await,
    );
    assert_eq!(result["outcome"], "applied", "{result}");
    responder.await.expect("Bridge応答を保持する")
}

async fn assert_public_group_closed(
    context: &McpReadContext,
    auth: &McpAuthorization,
    group: &Value,
) {
    for (name, arguments) in [
        ("edit_end_group", json!({"groupId":group["groupId"]})),
        (
            "object_set_property",
            json!({"params":{"objectId":"node","property":"visible","value":true},"groupId":group["groupId"]}),
        ),
    ] {
        let response = context
            .call_write_tool(name, arguments, Some(auth.current_lease()))
            .await;
        assert_eq!(response.is_error, Some(true));
        let result = data(response);
        assert_eq!(result["outcome"], "notApplied");
        assert!(!result
            .to_string()
            .contains(group["groupId"].as_str().expect("秘密ID")));
    }
}

#[tokio::test]
async fn public_group_survives_request_end_and_ui_undo_redo_operate_on_all_entries() {
    let auth = enabled_auth();
    let (service, _control, handle, mut peer) =
        test_service_with_peer_and_authorization(8, auth.clone());
    let service = Arc::new(service);
    let context = public_context(&service, ALL_CAPABILITIES);
    let lease = auth.current_lease();
    let group = data(
        context
            .call_write_tool(
                "edit_begin_group",
                json!({"name":"3件の編集"}),
                Some(lease.clone()),
            )
            .await,
    )["result"]
        .clone();
    lease.cancel_request();
    for object in ["node", "parent", "scene"] {
        peer = public_property(&context, &auth, peer, Some(&group), object).await;
    }
    let summary = service.history_summary();
    assert!(summary.can_undo, "end前も人がundoできる");
    assert_eq!(
        summary
            .undo_group
            .as_ref()
            .expect("まとまりを表示する")
            .count,
        3
    );
    assert_eq!(
        summary.undo_group.as_ref().unwrap().id,
        group["displayGroupId"]
    );
    assert!(!serde_json::to_string(&summary)
        .unwrap()
        .contains(group["groupId"].as_str().unwrap()));
    for (direction, objects, value) in [
        (HistoryDirection::Undo, ["scene", "parent", "node"], false),
        (HistoryDirection::Redo, ["node", "parent", "scene"], true),
    ] {
        let ticket = queue_history_action(&service, direction);
        for object in objects {
            let (id, method, params) = next_request(&mut peer).await;
            assert_eq!(method, "object.setProperty");
            assert_eq!(
                params.unwrap(),
                json!({"objectId":object,"property":"visible","value":value})
                    .as_object()
                    .unwrap()
                    .clone()
            );
            respond(&mut peer, id, json!({"accepted":true,"appliedValue":value})).await;
        }
        ticket.result().await.expect("1回の操作で3件を処理する");
    }
    assert_public_group_closed(&context, &auth, &group).await;
    service.shutdown().await;
    handle.shutdown().await;
}

#[tokio::test]
async fn public_group_end_and_foreign_or_missing_id_and_ui_close_before_next_edit() {
    for boundary in [
        "end", "foreign", "display", "missing", "ui", "begin", "redo",
    ] {
        let auth = enabled_auth();
        let (service, _control, handle, peer) =
            test_service_with_peer_and_authorization(8, auth.clone());
        let service = Arc::new(service);
        let context = public_context(&service, ALL_CAPABILITIES);
        let group = begin_public_group(&context, &auth).await;
        let mut peer = public_property(&context, &auth, peer, Some(&group), "node").await;
        match boundary {
            "end" => {
                let result = data(
                    context
                        .call_write_tool(
                            "edit_end_group",
                            json!({"groupId":group["groupId"]}),
                            Some(auth.current_lease()),
                        )
                        .await,
                );
                assert_eq!(result["result"]["closed"], true);
            }
            "foreign" | "display" => {
                let id = if boundary == "display" {
                    group["displayGroupId"].clone()
                } else {
                    json!("f".repeat(64))
                };
                let result = data(context.call_write_tool("object_set_property", json!({"params":{"objectId":"node","property":"visible","value":true},"groupId":id}), Some(auth.current_lease())).await);
                assert_eq!(result["outcome"], "notApplied");
            }
            "missing" => peer = public_property(&context, &auth, peer, None, "parent").await,
            "ui" => {
                service
                    .enqueue_recorded_from_ui(
                        EditKind::Edit,
                        HistoryRequest::CreateObject {
                            parent_id: None,
                            kind: None,
                        },
                        |_| async {
                            Ok(QueuedEditResult::plain(
                                json!({"accepted":true,"newId":"human"}),
                            ))
                        },
                    )
                    .expect("人の編集を入れる")
                    .result()
                    .await
                    .expect("人の列を占有しない");
            }
            "begin" => {
                let second = begin_public_group(&context, &auth).await;
                assert_ne!(second["groupId"], group["groupId"]);
                assert_ne!(second["displayGroupId"], group["displayGroupId"]);
                // 古いIDの要求は新しいまとまりへ追加できない。
            }
            "redo" => {
                let result = data(
                    context
                        .call_write_tool("edit_redo", json!({}), Some(auth.current_lease()))
                        .await,
                );
                assert_eq!(result["outcome"], "noChange");
            }
            _ => unreachable!(),
        }
        assert_eq!(
            service.history_summary().undo_group.unwrap().count,
            1,
            "{boundary}"
        );
        assert_public_group_closed(&context, &auth, &group).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(10), peer.recv())
                .await
                .is_err(),
            "失効IDをBridgeへ送らない"
        );
        service.shutdown().await;
        handle.shutdown().await;
    }
}

#[tokio::test]
async fn public_group_wrong_end_cannot_close_the_owner_and_read_only_cannot_begin() {
    let auth = enabled_auth();
    let (service, _control, handle, peer) =
        test_service_with_peer_and_authorization(8, auth.clone());
    let service = Arc::new(service);
    let context = public_context(&service, ALL_CAPABILITIES);
    let group = begin_public_group(&context, &auth).await;
    let response = context
        .call_write_tool(
            "edit_end_group",
            json!({"groupId":group["displayGroupId"]}),
            Some(auth.current_lease()),
        )
        .await;
    assert_eq!(data(response)["outcome"], "notApplied");
    assert!(service.history.lock().unwrap().has_active_group());
    let _peer = public_property(&context, &auth, peer, Some(&group), "node").await;
    auth.set_write_settings(McpWriteSettings {
        mode: McpWriteMode::ReadOnly,
        scene_root_id: None,
    })
    .unwrap();
    // 道具一覧の更新が遅れても、編集サービスの許可を正本とする。
    let response = context
        .call_write_tool(
            "edit_begin_group",
            json!({"name":"許可のない開始"}),
            Some(auth.current_lease()),
        )
        .await;
    assert_eq!(data(response)["outcome"], "notApplied");
    service.shutdown().await;
    handle.shutdown().await;
}

#[tokio::test]
async fn public_group_capture_failure_closes_successful_prefix_without_writing() {
    let auth = enabled_auth();
    let (service, _control, handle, peer) =
        test_service_with_peer_and_authorization(8, auth.clone());
    let service = Arc::new(service);
    let context = public_context(&service, ALL_CAPABILITIES);
    let group = begin_public_group(&context, &auth).await;
    let mut peer = public_property(&context, &auth, peer, Some(&group), "node").await;
    let reads = context.clone();
    let args = json!({"params":{"objectId":"node","property":"visible","value":true},"groupId":group["groupId"]});
    let lease = auth.current_lease();
    let task = tokio::spawn(async move {
        reads
            .call_write_tool("object_set_property", args, Some(lease))
            .await
    });
    respond_tree(&mut peer, tree()).await;
    let (id, method, _) = next_request(&mut peer).await;
    assert_eq!(method, "object.getSnapshot");
    respond_method_not_supported(&mut peer, id).await;
    assert_eq!(data(task.await.unwrap())["outcome"], "notApplied");
    assert!(!service.history.lock().unwrap().has_active_group());
    assert_eq!(service.history_summary().undo_group.unwrap().count, 1);
    assert_public_group_closed(&context, &auth, &group).await;
    assert!(tokio::time::timeout(Duration::from_millis(10), peer.recv())
        .await
        .is_err());
    service.shutdown().await;
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn public_group_closes_at_128_edits_and_rejects_the_next_id() {
    let auth = enabled_auth();
    let (service, _control, handle, mut peer) =
        test_service_with_peer_and_authorization(8, auth.clone());
    let service = Arc::new(service);
    let context = public_context(&service, ALL_CAPABILITIES);
    let group = begin_public_group(&context, &auth).await;
    for _ in 0..128 {
        peer = public_property(&context, &auth, peer, Some(&group), "node").await;
    }
    assert!(!service.history.lock().unwrap().has_active_group());
    assert_eq!(service.history_summary().undo_group.unwrap().count, 128);
    assert_public_group_closed(&context, &auth, &group).await;
    service.shutdown().await;
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn public_group_idle_and_total_deadlines_close_without_a_new_request() {
    for total in [false, true] {
        let auth = enabled_auth();
        let (service, _control, handle, mut peer) =
            test_service_with_peer_and_authorization(8, auth.clone());
        let service = Arc::new(service);
        let context = public_context(&service, ALL_CAPABILITIES);
        let group = begin_public_group(&context, &auth).await;
        peer = public_property(&context, &auth, peer, Some(&group), "node").await;
        if total {
            for _ in 0..3 {
                tokio::time::advance(Duration::from_secs(240)).await;
                peer = public_property(&context, &auth, peer, Some(&group), "node").await;
            }
        }
        tokio::time::advance(Duration::from_secs(if total { 179 } else { 299 })).await;
        assert!(service.history.lock().unwrap().has_active_group());
        tokio::time::advance(Duration::from_secs(1)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!service.history.lock().unwrap().has_active_group());
        assert_eq!(
            service.history_summary().undo_group.unwrap().count,
            if total { 4 } else { 1 }
        );
        assert_public_group_closed(&context, &auth, &group).await;
        service.shutdown().await;
        handle.shutdown().await;
    }
}

#[tokio::test]
async fn public_group_is_revoked_on_auth_disable_disconnect_and_generation_change() {
    for reason in ["auth", "disable", "disconnect", "generation"] {
        let auth = enabled_auth();
        let (service, control, handle, peer) =
            test_service_with_peer_and_authorization(8, auth.clone());
        let service = Arc::new(service);
        let context = public_context(&service, ALL_CAPABILITIES);
        let group = begin_public_group(&context, &auth).await;
        let _peer = public_property(&context, &auth, peer, Some(&group), "node").await;
        match reason {
            "auth" => {
                auth.revoke();
            }
            "disable" => {
                auth.set_write_settings(McpWriteSettings {
                    mode: McpWriteMode::ReadOnly,
                    scene_root_id: None,
                })
                .unwrap();
            }
            "disconnect" => {
                control.disconnect();
            }
            "generation" => {
                control.set_generation(2, handle.clone());
            }
            _ => unreachable!(),
        }
        service.history_summary();
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(
            !service.history.lock().unwrap().has_active_group(),
            "{reason}"
        );
        assert_public_group_closed(&context, &auth, &group).await;
        service.shutdown().await;
        handle.shutdown().await;
    }
}

#[tokio::test]
async fn public_group_closes_before_nonundoable_operations_even_with_its_id() {
    for (name, method, params, result, confirm) in [
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
            "scene_delete_object",
            "scene.deleteObject",
            json!({"objectId":"node"}),
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
    ] {
        let auth = enabled_auth();
        let (service, _control, handle, peer) =
            test_service_with_peer_and_authorization(8, auth.clone());
        let service = Arc::new(service);
        let context = public_context(&service, ALL_CAPABILITIES);
        let group = begin_public_group(&context, &auth).await;
        let peer = public_property(&context, &auth, peer, Some(&group), "node").await;
        let responder = tokio::spawn(serve_write(peer, method, params.clone(), result));
        let mut updates = auth.confirmations().subscribe();
        let reads = context.clone();
        let args = json!({"params":params,"groupId":group["groupId"]});
        let lease = auth.current_lease();
        let task =
            tokio::spawn(async move { reads.call_write_tool(name, args, Some(lease)).await });
        if confirm {
            let request = confirmation(&mut updates, None).await;
            assert!(!service.history.lock().unwrap().has_active_group());
            auth.confirmations().approve(&request.id);
        }
        assert_eq!(data(task.await.unwrap())["outcome"], "applied");
        let _peer = responder.await.unwrap();
        assert!(!service.history.lock().unwrap().has_active_group());
        assert_eq!(
            service.history_summary().can_undo,
            name != "scene_delete_object"
        );
        assert_public_group_closed(&context, &auth, &group).await;
        service.shutdown().await;
        handle.shutdown().await;
    }
}

#[tokio::test]
async fn public_group_mcp_undo_closes_open_group_and_partial_failure_uses_ne06() {
    let auth = enabled_auth();
    let (service, _control, handle, mut peer) =
        test_service_with_peer_and_authorization(8, auth.clone());
    let service = Arc::new(service);
    let context = public_context(&service, ALL_CAPABILITIES);
    let group = begin_public_group(&context, &auth).await;
    for object in ["node", "parent", "scene"] {
        peer = public_property(&context, &auth, peer, Some(&group), object).await;
    }
    let reads = context.clone();
    let lease = auth.current_lease();
    let task = tokio::spawn(async move {
        reads
            .call_write_tool("edit_undo", json!({}), Some(lease))
            .await
    });
    let responder = tokio::spawn(serve_write(
        peer,
        "object.setProperty",
        json!({"objectId":"scene","property":"visible","value":false}),
        json!({"accepted":true,"appliedValue":false}),
    ));
    peer = responder.await.unwrap();
    let (id, method, params) = next_request(&mut peer).await;
    assert_eq!(method, "object.setProperty");
    assert_eq!(params.unwrap()["objectId"], "parent");
    respond(&mut peer, id, json!({"accepted":false})).await;
    let result = data(task.await.unwrap());
    assert_eq!(result["outcome"], "partial");
    assert_eq!(result["completedCount"], 1);
    let pending = service.history_summary().pending_group.unwrap();
    assert_eq!(pending.id, group["displayGroupId"]);
    assert_eq!(pending.completed_count, 1);
    assert_eq!(pending.total_count, 3);
    assert!(pending.retry_allowed);
    let worker = service.clone();
    let retry = tokio::spawn(async move { worker.retry_pending_from_ui().await });
    for object in ["parent", "node"] {
        let (id, _, params) = next_request(&mut peer).await;
        assert_eq!(
            params.unwrap()["objectId"],
            object,
            "成功済みの操作は繰り返さない"
        );
        respond(&mut peer, id, json!({"accepted":true,"appliedValue":false})).await;
    }
    retry.await.unwrap().expect("保留部分だけを再試行する");
    assert!(!service.history_summary().pending);
    let reads = context.clone();
    let lease = auth.current_lease();
    let redo = tokio::spawn(async move {
        reads
            .call_write_tool("edit_redo", json!({}), Some(lease))
            .await
    });
    peer = serve_write(
        peer,
        "object.setProperty",
        json!({"objectId":"node","property":"visible","value":true}),
        json!({"accepted":true,"appliedValue":true}),
    )
    .await;
    for object in ["parent", "scene"] {
        let (id, _, params) = next_request(&mut peer).await;
        assert_eq!(params.unwrap()["objectId"], object);
        respond(&mut peer, id, json!({"accepted":true,"appliedValue":true})).await;
    }
    assert_eq!(data(redo.await.unwrap())["completedCount"], 3);
    assert_public_group_closed(&context, &auth, &group).await;
    service.shutdown().await;
    handle.shutdown().await;
}

#[tokio::test]
async fn public_group_rejected_forward_edit_closes_and_keeps_successful_prefix() {
    let auth = enabled_auth();
    let (service, _control, handle, peer) =
        test_service_with_peer_and_authorization(8, auth.clone());
    let service = Arc::new(service);
    let context = public_context(&service, ALL_CAPABILITIES);
    let group = begin_public_group(&context, &auth).await;
    let peer = public_property(&context, &auth, peer, Some(&group), "node").await;
    let params = json!({"objectId":"parent","property":"visible","value":true});
    let responder = tokio::spawn(serve_write(
        peer,
        "object.setProperty",
        params.clone(),
        json!({"accepted":false}),
    ));
    let result = data(
        context
            .call_write_tool(
                "object_set_property",
                json!({"params":params,"groupId":group["groupId"]}),
                Some(auth.current_lease()),
            )
            .await,
    );
    assert_eq!(result["outcome"], "rejected");
    let _peer = responder.await.unwrap();
    assert!(!service.history.lock().unwrap().has_active_group());
    assert_eq!(service.history_summary().undo_group.unwrap().count, 1);
    assert_public_group_closed(&context, &auth, &group).await;
    service.shutdown().await;
    handle.shutdown().await;
}

#[tokio::test]
async fn public_group_confirmation_wait_does_not_block_ui_and_cannot_reopen_closed_id() {
    let auth = enabled_auth();
    auth.set_write_settings(McpWriteSettings {
        mode: McpWriteMode::Confirm,
        scene_root_id: None,
    })
    .unwrap();
    let (service, _control, handle, mut peer) =
        test_service_with_peer_and_authorization(8, auth.clone());
    let service = Arc::new(service);
    let context = public_context(&service, ALL_CAPABILITIES);
    let group = begin_public_group(&context, &auth).await;
    let mut updates = auth.confirmations().subscribe();
    let reads = context.clone();
    let lease = auth.current_lease();
    let args = json!({"params":{"objectId":"node","property":"visible","value":true},"groupId":group["groupId"]});
    let mut request = tokio::spawn(async move {
        reads
            .call_write_tool("object_set_property", args, Some(lease))
            .await
    });
    respond_tree(&mut peer, tree()).await;
    let (id, method, _) = next_request(&mut peer).await;
    assert_eq!(method, "object.getSnapshot");
    respond(
        &mut peer,
        id,
        json!({"objectId":"node","properties":[{"name":"visible","value":false}],"components":[]}),
    )
    .await;
    let approval = confirmation(&mut updates, None).await;
    service
        .enqueue_from_ui(EditKind::RuntimeControl, |_| async {
            success(json!({"accepted":true}))
        })
        .unwrap()
        .result()
        .await
        .expect("確認待ち中も人の操作が進む");
    assert!(!service.history.lock().unwrap().has_active_group());
    auth.confirmations().approve(&approval.id);
    // 確認後の再照会中も、人の操作で失効した秘密は復活しない。
    let result = loop {
        tokio::select! {
            result = &mut request => break result.expect("失効した要求が終了する"),
            request = next_request(&mut peer) => {
                let (id, method, _) = request;
                match method.as_str() {
                    "scene.getTree" => respond(&mut peer, id, tree()).await,
                    "object.getSnapshot" => respond(&mut peer, id, json!({"objectId":"node","properties":[{"name":"visible","value":false}],"components":[]})).await,
                    _ => panic!("失効したまとまりから書き込まない: {method}"),
                }
            }
            _ = updates.changed() => {
                // 先頭改訂の変化で再確認へ戻った場合も、列外でのみ承認する。
                let next = updates.borrow_and_update().first().map(|next| next.id.clone());
                if let Some(id) = next { auth.confirmations().approve(&id); }
            }
        }
    };
    assert_eq!(data(result)["outcome"], "notApplied");
    service.shutdown().await;
    handle.shutdown().await;
}
