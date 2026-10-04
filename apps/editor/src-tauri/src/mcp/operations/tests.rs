//! 実ファイルの上限、秘密の非記録、通知と初期取得の一致を検査する。

use super::*;
use serde_json::json;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).expect("試験用乱数を作る");
        let name: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = std::env::temp_dir().join(format!("norves-operations-{name}"));
        fs::create_dir(&path).expect("一時ディレクトリを作る");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("試験ディレクトリを消す");
    }
}

fn finish(
    operation: &OperationHandle,
    result: &'static str,
    outcome: &'static str,
    actor_finished: bool,
) {
    operation.finish(OperationUpdate {
        result,
        outcome,
        completed: usize::from(outcome == "applied"),
        pending: false,
        retry_allowed: false,
        actor_finished,
    });
}

#[test]
fn bounded_memory_and_two_file_generations_rotate_during_one_run() {
    let dir = TempDir::new();
    let store = OperationStore::new(Some(dir.0.clone()));
    let first = store.begin("object_set_property", &json!({}));
    let first_id = first.snapshot().request_id;
    finish(&first, "success", "applied", true);
    for _ in 0..6000 {
        finish(
            &store.begin(
                "object_set_property",
                &json!({"params":{"objectId":"node"}}),
            ),
            "success",
            "applied",
            true,
        );
    }
    let snapshot = store.snapshot();
    assert_eq!(snapshot.records.len(), MAX_RECORDS);
    assert_eq!(snapshot.revision, 6001);
    assert!(snapshot.storage_error.is_none());
    assert_eq!(fs::read_dir(&dir.0).expect("ファイル一覧").count(), 2);
    for name in [FILE_NAME, ROTATED_NAME] {
        let path = dir.0.join(name);
        assert!(fs::metadata(&path).expect("保存ファイル").len() <= MAX_BYTES);
        let contents = fs::read_to_string(path).expect("JSONLを開く");
        assert!(!contents.contains(&format!("\"{first_id}\"")));
        for line in contents.lines() {
            let value: Value = serde_json::from_str(line).expect("各行が完全なJSON");
            assert_eq!(value["automaticRetryAllowed"], false);
        }
    }
    let reopened = OperationStore::new(Some(dir.0.clone()));
    assert_ne!(store.snapshot().session_id, reopened.snapshot().session_id);
    let second = reopened.begin("edit_undo", &Value::Null);
    assert_ne!(first_id, second.snapshot().request_id);
    finish(&second, "success", "noChange", true);
    assert!(reopened.snapshot().storage_error.is_none());
}

#[test]
fn arbitrary_strings_bodies_and_secrets_never_reach_file_or_notifications() {
    let dir = TempDir::new();
    let store = OperationStore::new(Some(dir.0.clone()));
    let notifications = Arc::new(Mutex::new(Vec::new()));
    let received = notifications.clone();
    store.set_sink(Arc::new(move |payload| {
        received.lock().unwrap().push(payload)
    }));
    let secret = "Bearer-token-confirmation-owner-secret";
    let args = json!({"token":secret, "confirmationId":secret, "groupId":secret,
        "params":{"objectId":secret, "property":secret, "value":"巨大本文".repeat(200_000)}});
    for tool in ["object_set_property", "edit_begin_group", secret] {
        let operation = store.begin(tool, &args);
        operation.group("edit-1-2".to_owned());
        finish(&operation, "success", "applied", true);
    }
    let huge = store.begin(
        "object_set_property",
        &json!({"params":{"objectId":"巨大対象".repeat(200_000)}}),
    );
    finish(&huge, "rejected", "notApplied", true);
    let file = fs::read_to_string(dir.0.join(FILE_NAME)).expect("保存を開く");
    let received = notifications.lock().unwrap();
    let events = serde_json::to_string(&*received).expect("通知を検査する");
    for output in [&file, &events] {
        for forbidden in [
            secret,
            "巨大本文",
            "巨大対象",
            "confirmationId",
            "\"groupId\"",
            "\"params\"",
            "\"value\"",
        ] {
            assert!(!output.contains(forbidden), "{forbidden}");
        }
    }
    assert!(file.len() < 8192);
    assert_eq!(received.last().unwrap(), &store.snapshot());
    assert!(file.contains("edit-1-2"));
}

#[test]
fn storage_failure_is_visible_and_recovers_without_losing_memory() {
    let dir = TempDir::new();
    let blocked = dir.0.join("not-a-directory");
    fs::write(&blocked, b"occupied").unwrap();
    let store = OperationStore::new(Some(blocked.clone()));
    assert!(store.snapshot().storage_error.is_some());
    finish(
        &store.begin("runtime_play", &Value::Null),
        "failed",
        "notApplied",
        true,
    );
    assert_eq!(store.snapshot().records.len(), 1);
    assert!(store.snapshot().storage_error.is_some());
    fs::remove_file(&blocked).unwrap();
    fs::create_dir(&blocked).unwrap();
    finish(
        &store.begin("runtime_stop", &Value::Null),
        "success",
        "applied",
        true,
    );
    assert!(store.snapshot().storage_error.is_none());
    assert!(fs::read_to_string(blocked.join(FILE_NAME))
        .unwrap()
        .contains("runtime_stop"));
}

#[test]
fn late_actor_result_replaces_unknown_and_cannot_be_overwritten_by_http() {
    let dir = TempDir::new();
    let store = OperationStore::new(Some(dir.0.clone()));
    let operation = store.begin("object_set_property", &Value::Null);
    finish(&operation, "cancelled", "unknown", false);
    assert!(store.snapshot().records[0].summary.contains("自動再送せず"));
    finish(&operation, "success", "applied", true);
    finish(&operation, "cancelled", "unknown", false);
    let snapshot = store.snapshot();
    assert_eq!(snapshot.records.len(), 1);
    assert_eq!(snapshot.records[0].outcome, "applied");
    assert_eq!(snapshot.revision, 2);
    assert_eq!(
        fs::read_to_string(dir.0.join(FILE_NAME))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn all_result_classes_persist_with_required_fields() {
    let dir = TempDir::new();
    let store = OperationStore::new(Some(dir.0.clone()));
    for (result, outcome) in [
        ("success", "applied"),
        ("rejected", "rejected"),
        ("failed", "notApplied"),
        ("partial", "partial"),
        ("timedOut", "unknown"),
    ] {
        finish(
            &store.begin("edit_undo", &Value::Null),
            result,
            outcome,
            true,
        );
    }
    let file = fs::read_to_string(dir.0.join(FILE_NAME)).unwrap();
    assert_eq!(file.lines().count(), 5);
    for line in file.lines() {
        let record: Value = serde_json::from_str(line).unwrap();
        for field in [
            "requestId",
            "sessionId",
            "timestamp",
            "tool",
            "target",
            "summary",
            "result",
            "displayGroupId",
            "outcome",
        ] {
            assert!(record.get(field).is_some(), "{field}");
        }
        assert!(record["timestamp"].as_u64().unwrap() > 0);
    }
}

#[tokio::test(start_paused = true)]
async fn dropped_read_is_recorded_as_cancelled_or_timed_out_without_its_body() {
    for timed_out in [false, true] {
        let dir = TempDir::new();
        let store = OperationStore::new(Some(dir.0.clone()));
        let lease = super::super::McpAuthorization::default().current_lease();
        let capture = ReadCapture {
            operation: store.begin("viewport_get_thumbnail", &json!({})),
            lease: Some(lease.clone()),
            finished: false,
        };
        if timed_out {
            tokio::time::advance(std::time::Duration::from_secs(125)).await;
        } else {
            lease.cancel_request();
        }
        drop(capture);
        let record = store.snapshot().records.pop().expect("破棄時に記録する");
        assert_eq!(
            record.result,
            if timed_out { "timedOut" } else { "cancelled" }
        );
        assert_eq!(record.outcome, "readFailed");
        assert!(!record.automatic_retry_allowed);
    }
}
