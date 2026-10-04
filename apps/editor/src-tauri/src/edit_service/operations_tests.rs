// 操作記録がHTTP終了後の列内結果と一致することを検査する。

#[tokio::test]
async fn cancelled_queued_public_write_stays_not_sent_when_actor_finishes() {
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
    let task = tokio::spawn(async move {
        context.call_write_tool("runtime_play", json!({"params":{}}), Some(caller_lease)).await
    });
    wait_for_enqueued(&service).await;
    lease.cancel_request();
    let response = data(task.await.expect("呼出元が取消で終了する"));
    assert_eq!(response["outcome"], "notApplied");
    assert_eq!(response["operationResult"], "cancelled");
    assert!(!operations.snapshot().records[0].actor_finished);
    release.send(()).expect("先行操作を解放する");
    blocker.result().await.expect("先行操作が終わる");
    service.enqueue_from_ui(EditKind::Edit, |_| async { success(Value::Null) })
        .expect("番兵を列へ入れる").result().await.expect("取消項目の処理が終わる");
    let record = operations.snapshot().records.pop().expect("確定結果がある");
    assert_eq!(record.request_id, response["requestId"]);
    assert_eq!(record.outcome, "notApplied");
    assert_eq!(record.result, "cancelled");
    assert!(record.actor_finished);
    assert!(!record.pending);
    assert!(!service.history_summary().pending);
    assert!(tokio::time::timeout(Duration::from_millis(10), peer.recv()).await.is_err());
    service.shutdown().await;
    handle.shutdown().await;
}
