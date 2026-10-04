// 確認・再照会・範囲走査が同じ要求期限と取消を使うことを製品経路で検査する。

#[tokio::test(start_paused = true)]
async fn request_lifetime_cancellation_cleans_only_its_confirmation() {
    let (transport, _peer) = loopback_pair(8);
    let handle = Dispatcher::spawn(transport);
    let auth = McpAuthorization::default();
    auth.set_write_settings(crate::mcp::authorization::McpWriteSettings {
        mode: crate::mcp::McpWriteMode::Confirm,
        scene_root_id: None,
    })
    .expect("都度確認にする");
    let (context, _session) = test_context_with_session(
        1,
        handle.clone(),
        &["runtime.control"],
        Arc::new(StdMutex::new(LogBuffer::default())),
    );
    let context = context.with_authorization(auth.clone());
    let first = auth.current_lease();
    let second = auth.current_lease();
    let params = json!({});
    let mut first_request = Box::pin(context.authorize_write(&first, "runtime.play", &params));
    let mut second_request = Box::pin(context.authorize_write(&second, "runtime.play", &params));
    crate::mcp::confirmation::tests::poll_pending(first_request.as_mut()).await;
    let first_id = auth.confirmations().pending()[0].id.clone();
    crate::mcp::confirmation::tests::poll_pending(second_request.as_mut()).await;
    assert_eq!(auth.confirmations().pending().len(), 2);
    first.cancel_request();
    assert!(first_request
        .await
        .expect_err("要求取消が確認へ届く")
        .contains("取り消"));
    assert!(!auth.confirmations().approve(&first_id));
    assert_eq!(auth.confirmations().pending().len(), 1);
    assert!(second.is_current());
    second.cancel_request();
    assert!(second_request.await.is_err());
    assert!(auth.confirmations().pending().is_empty());
    handle.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn request_lifetime_scope_requery_and_reconfirmation_share_125_seconds() {
    for stage in ["scope", "requery", "reconfirmation"] {
        let (transport, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(transport);
        let auth = McpAuthorization::default();
        auth.set_write_settings(crate::mcp::authorization::McpWriteSettings {
            mode: crate::mcp::McpWriteMode::Enabled,
            scene_root_id: None,
        })
        .expect("削除だけ確認を要求する");
        let (context, _session) = test_context_with_session(
            1,
            handle.clone(),
            &["scene.query", "scene.edit"],
            Arc::new(StdMutex::new(LogBuffer::default())),
        );
        let context = context.with_authorization(auth.clone());
        let lease = auth.current_lease();
        let started = tokio::time::Instant::now();
        let request_lease = lease.clone();
        if stage == "scope" {
            tokio::time::advance(Duration::from_secs(122)).await;
        }
        let request = tokio::spawn(async move {
            context
                .authorize_write(
                    &request_lease,
                    "scene.deleteObject",
                    &json!({"objectId":"node"}),
                )
                .await
        });
        let (id, method, _) = next_request(&mut peer).await;
        assert_eq!(method, "scene.getTree");
        let mut updates = auth.confirmations().subscribe();
        if stage != "scope" {
            tokio::time::advance(Duration::from_secs(4)).await;
            peer.send(response_frame(
                id,
                json!({"root":{"id":"scene","children":[{"id":"node","children":[]}]}}),
            ))
            .await
            .expect("最初の範囲走査を返す");
            let first = next_confirmation(&mut updates, None).await;
            tokio::time::advance(Duration::from_secs(119)).await;
            assert!(auth.confirmations().approve(&first.id));
            let (id, method, _) = next_request(&mut peer).await;
            assert_eq!(method, "scene.getTree");
            if stage == "reconfirmation" {
                tokio::time::advance(Duration::from_secs(1)).await;
                peer.send(response_frame(id, json!({"root":{"id":"scene","children":[{"id":"node","children":[{"id":"child","children":[]}]}]}})))
                    .await.expect("削除範囲の変化を返す");
                let second = next_confirmation(&mut updates, Some(&first.id)).await;
                assert_eq!(second.target_count, 2);
                assert_eq!(
                    lease.confirmation_deadline(),
                    Some(started + Duration::from_secs(125))
                );
            }
        }
        assert!(!request.is_finished());
        let remaining = lease.request_deadline() - tokio::time::Instant::now();
        tokio::time::advance(remaining).await;
        let result = request
            .await
            .expect("要求期限で処理が終了する")
            .expect_err("期限延長を許さない");
        assert!(result.contains("全体期限"), "{stage}: {result}");
        assert_eq!(
            tokio::time::Instant::now(),
            started + Duration::from_secs(125)
        );
        assert!(auth.confirmations().pending().is_empty());
        handle.shutdown().await;
    }
}
