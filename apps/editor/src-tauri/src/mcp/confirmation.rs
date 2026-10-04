use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    sync::{oneshot, watch},
    time,
};

use crate::{dto::McpConfirmationRequestDto, mcp::McpRequestLease};

pub(crate) const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_PENDING_CONFIRMATIONS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmationResult {
    Approved,
    Rejected,
    TimedOut,
    Cancelled,
    GenerationChanged,
    Unavailable,
}

#[derive(Clone)]
pub(crate) struct McpConfirmationBroker {
    inner: Arc<BrokerInner>,
}

struct BrokerInner {
    requests: Mutex<HashMap<String, PendingConfirmation>>,
    updates: watch::Sender<Vec<McpConfirmationRequestDto>>,
}

struct PendingConfirmation {
    request: McpConfirmationRequestDto,
    decision: oneshot::Sender<bool>,
}

/// 呼び出しfutureの中止・破棄でも登録と画面通知を確実に片付ける。
struct ConfirmationRegistration<'a> {
    broker: &'a McpConfirmationBroker,
    id: String,
}

impl Drop for ConfirmationRegistration<'_> {
    fn drop(&mut self) {
        self.broker.remove(&self.id);
    }
}

impl Default for McpConfirmationBroker {
    fn default() -> Self {
        let (updates, _) = watch::channel(Vec::new());
        Self {
            inner: Arc::new(BrokerInner {
                requests: Mutex::new(HashMap::new()),
                updates,
            }),
        }
    }
}

impl McpConfirmationBroker {
    pub(crate) async fn request<F>(
        &self,
        request: McpConfirmationRequestDto,
        lease: &McpRequestLease,
        generation_changed: F,
    ) -> ConfirmationResult
    where
        F: Future<Output = ()> + Send,
    {
        if !lease.is_current() {
            return ConfirmationResult::Cancelled;
        }
        let Some(deadline) = lease.confirmation_deadline() else {
            return ConfirmationResult::TimedOut;
        };
        let id = match confirmation_id() {
            Some(id) => id,
            None => return ConfirmationResult::Unavailable,
        };
        let mut request = request;
        request.id = id.clone();
        let _registration = ConfirmationRegistration {
            broker: self,
            id: id.clone(),
        };
        let (decision, decision_rx) = oneshot::channel();
        {
            let mut requests = self
                .inner
                .requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if requests.len() >= MAX_PENDING_CONFIRMATIONS {
                return ConfirmationResult::Unavailable;
            }
            requests.insert(id.clone(), PendingConfirmation { request, decision });
            self.publish_locked(&requests);
        }

        tokio::select! {
            biased;
            _ = lease.authorization_cancelled() => ConfirmationResult::Cancelled,
            _ = lease.request_cancelled() => ConfirmationResult::Cancelled,
            _ = time::sleep_until(deadline) => ConfirmationResult::TimedOut,
            _ = generation_changed => ConfirmationResult::GenerationChanged,
            decision = decision_rx => match decision {
                Ok(true) => ConfirmationResult::Approved,
                Ok(false) => ConfirmationResult::Rejected,
                Err(_) => ConfirmationResult::Cancelled,
            },
        }
    }

    pub(crate) fn pending(&self) -> Vec<McpConfirmationRequestDto> {
        self.inner
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .map(|pending| pending.request.clone())
            .collect()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Vec<McpConfirmationRequestDto>> {
        self.inner.updates.subscribe()
    }

    pub(crate) fn approve(&self, id: &str) -> bool {
        self.decide(id, true)
    }

    pub(crate) fn reject(&self, id: &str) -> bool {
        self.decide(id, false)
    }

    pub(crate) fn cancel_all(&self) {
        let pending = {
            let mut requests = self
                .inner
                .requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let pending = std::mem::take(&mut *requests);
            self.publish_locked(&requests);
            pending
        };
        // 送信側の破棄は取消。ユーザーによる明示的な拒否と混同しない。
        drop(pending);
    }

    fn decide(&self, id: &str, approved: bool) -> bool {
        let pending = {
            let mut requests = self
                .inner
                .requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let pending = requests.remove(id);
            if pending.is_some() {
                self.publish_locked(&requests);
            }
            pending
        };
        pending.is_some_and(|pending| pending.decision.send(approved).is_ok())
    }

    fn remove(&self, id: &str) {
        let mut requests = self
            .inner
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if requests.remove(id).is_some() {
            self.publish_locked(&requests);
        }
    }

    fn publish_locked(&self, requests: &HashMap<String, PendingConfirmation>) {
        let mut pending = requests
            .values()
            .map(|entry| entry.request.clone())
            .collect::<Vec<_>>();
        pending.sort_by_key(|request| request.expires_at);
        self.inner.updates.send_replace(pending);
    }
}

fn confirmation_id() -> Option<String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{dto::McpConfirmationRequestDto, mcp::McpAuthorization};
    use serde_json::Value;
    use tokio::time::Instant;

    pub(crate) async fn poll_pending<F: Future>(mut future: std::pin::Pin<&mut F>) {
        std::future::poll_fn(|context| {
            assert!(future.as_mut().poll(context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
    }

    pub(crate) fn request() -> McpConfirmationRequestDto {
        McpConfirmationRequestDto {
            id: String::new(),
            tool_name: "scene_delete_object".to_owned(),
            method: "scene.deleteObject".to_owned(),
            target_ids: vec!["node-1".to_owned()],
            target_count: 1,
            before: Some(Value::String("before".to_owned())),
            after: None,
            source: None,
            undo_available: false,
            clears_history: true,
            history_generation: None,
            history_revision: 0,
            undo_head_id: None,
            expires_at: 0,
        }
    }

    #[tokio::test]
    async fn approval_is_one_time_and_request_specific() {
        let broker = McpConfirmationBroker::default();
        let auth = McpAuthorization::default();
        let first_lease = auth.current_lease();
        let first_broker = broker.clone();
        let first = tokio::spawn(async move {
            first_broker
                .request(request(), &first_lease, std::future::pending())
                .await
        });
        let mut updates = broker.subscribe();
        updates.changed().await.expect("確認を通知する");
        let id = updates.borrow()[0].id.clone();
        assert!(broker.approve(&id));
        assert!(!broker.approve(&id));
        assert_eq!(
            first.await.expect("確認taskが終了する"),
            ConfirmationResult::Approved
        );
        assert!(updates.borrow_and_update().is_empty());

        let second_lease = auth.current_lease();
        let second_broker = broker.clone();
        let second = tokio::spawn(async move {
            second_broker
                .request(request(), &second_lease, std::future::pending())
                .await
        });
        updates.changed().await.expect("新しい確認を通知する");
        let second_id = updates.borrow()[0].id.clone();
        assert_ne!(id, second_id);
        assert!(!broker.approve(&id));
        assert!(broker.reject(&second_id));
        assert_eq!(
            second.await.expect("確認taskが終了する"),
            ConfirmationResult::Rejected
        );
    }

    #[tokio::test]
    async fn request_disconnect_and_authorization_revision_remove_pending_confirmation() {
        let broker = McpConfirmationBroker::default();
        let auth = McpAuthorization::default();
        let lease = auth.current_lease();
        let pending_broker = broker.clone();
        let pending = tokio::spawn(async move {
            pending_broker
                .request(request(), &lease, std::future::pending())
                .await
        });
        let mut updates = broker.subscribe();
        updates.changed().await.expect("確認を通知する");
        auth.revoke();
        assert_eq!(
            pending.await.expect("確認taskが終了する"),
            ConfirmationResult::Cancelled
        );
        assert!(broker.pending().is_empty());
        assert!(updates.changed().await.is_ok());
        assert!(updates.borrow().is_empty());
    }

    #[tokio::test]
    async fn broker_shutdown_cancels_all_pending_confirmations() {
        let broker = McpConfirmationBroker::default();
        let auth = McpAuthorization::default();
        let lease = auth.current_lease();
        let pending_broker = broker.clone();
        let pending = tokio::spawn(async move {
            pending_broker
                .request(request(), &lease, std::future::pending())
                .await
        });
        let mut updates = broker.subscribe();
        updates.changed().await.expect("確認を通知する");
        broker.cancel_all();
        assert_eq!(
            pending.await.expect("確認taskが終了する"),
            ConfirmationResult::Cancelled
        );
        assert!(broker.pending().is_empty());
    }

    #[tokio::test]
    async fn request_cancellation_and_confirmation_deadline_remove_pending_confirmation() {
        let broker = McpConfirmationBroker::default();
        let auth = McpAuthorization::default();
        let lease = auth.current_lease();
        let pending_broker = broker.clone();
        let pending_lease = lease.clone();
        let pending = tokio::spawn(async move {
            pending_broker
                .request(request(), &pending_lease, std::future::pending())
                .await
        });
        let mut updates = broker.subscribe();
        updates.changed().await.expect("確認を通知する");
        lease.cancel_request();
        assert_eq!(
            pending.await.expect("確認taskが終了する"),
            ConfirmationResult::Cancelled
        );
        assert!(broker.pending().is_empty());

        let mut timed_lease = auth.current_lease();
        timed_lease.set_request_deadline(Instant::now() + Duration::from_millis(10));
        let timed_broker = broker.clone();
        let timed = tokio::spawn(async move {
            timed_broker
                .request(request(), &timed_lease, std::future::pending())
                .await
        });
        updates.changed().await.expect("期限付き確認を通知する");
        assert_eq!(
            timed.await.expect("期限付き確認taskが終了する"),
            ConfirmationResult::TimedOut
        );
        assert!(broker.pending().is_empty());
    }

    #[tokio::test]
    async fn dropping_a_request_future_immediately_removes_registration_and_notifies() {
        let broker = McpConfirmationBroker::default();
        let auth = McpAuthorization::default();
        let lease = auth.current_lease();
        let mut updates = broker.subscribe();
        let mut pending = Box::pin(broker.request(request(), &lease, std::future::pending()));
        poll_pending(pending.as_mut()).await;
        let id = updates.borrow_and_update()[0].id.clone();

        drop(pending);
        assert!(broker.pending().is_empty());
        assert!(updates.has_changed().expect("取り下げ通知を受信する"));
        assert!(updates.borrow().is_empty());
        assert!(!broker.approve(&id));
        assert!(!broker.reject(&id));
    }

    #[tokio::test]
    async fn aborting_a_request_task_reclaims_its_registration() {
        let broker = McpConfirmationBroker::default();
        let auth = McpAuthorization::default();
        let lease = auth.current_lease();
        let mut updates = broker.subscribe();
        let task_broker = broker.clone();
        let task = tokio::spawn(async move {
            task_broker
                .request(request(), &lease, std::future::pending())
                .await
        });
        updates.changed().await.expect("確認を通知する");
        task.abort();
        assert!(task.await.expect_err("taskを中止する").is_cancelled());
        assert!(broker.pending().is_empty());
        assert!(updates.borrow().is_empty());
    }

    #[tokio::test]
    async fn sixteen_pending_requests_are_isolated_and_reclaimed_slots_can_be_reused() {
        let broker = McpConfirmationBroker::default();
        let auth = McpAuthorization::default();
        let leases: Vec<_> = (0..17).map(|_| auth.current_lease()).collect();
        let mut pending = Vec::new();
        for lease in &leases[..16] {
            let mut future = Box::pin(broker.request(request(), lease, std::future::pending()));
            poll_pending(future.as_mut()).await;
            pending.push(future);
        }
        let ids: std::collections::HashSet<_> =
            broker.pending().into_iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), 16);
        assert_eq!(
            broker
                .request(request(), &leases[16], std::future::pending())
                .await,
            ConfirmationResult::Unavailable
        );

        drop(pending.pop());
        assert_eq!(broker.pending().len(), 15);
        let mut replacement =
            Box::pin(broker.request(request(), &leases[16], std::future::pending()));
        poll_pending(replacement.as_mut()).await;
        let new_id = broker
            .pending()
            .into_iter()
            .find(|r| !ids.contains(&r.id))
            .expect("新しい要求固有IDを発行する")
            .id;
        assert_eq!(broker.pending().len(), 16);
        assert!(broker.approve(&new_id));
        assert_eq!(replacement.await, ConfirmationResult::Approved);
        assert!(!broker.approve(&new_id));
        assert_eq!(broker.pending().len(), 15);
        broker.cancel_all();
        for future in pending {
            assert_eq!(future.await, ConfirmationResult::Cancelled);
        }
        assert!(broker.pending().is_empty());
    }

    #[tokio::test]
    async fn cancellation_wins_over_approval_and_generation_change_removes_registration() {
        let broker = McpConfirmationBroker::default();
        let auth = McpAuthorization::default();
        let lease = auth.current_lease();
        let mut future = Box::pin(broker.request(request(), &lease, std::future::pending()));
        poll_pending(future.as_mut()).await;
        let id = broker.pending()[0].id.clone();
        assert!(broker.approve(&id));
        lease.cancel_request();
        assert_eq!(future.await, ConfirmationResult::Cancelled);
        assert!(broker.pending().is_empty());

        let lease = auth.current_lease();
        let (changed, changed_rx) = oneshot::channel();
        let mut future = Box::pin(broker.request(request(), &lease, async {
            let _ = changed_rx.await;
        }));
        poll_pending(future.as_mut()).await;
        let id = broker.pending()[0].id.clone();
        changed.send(()).expect("世代変更を送る");
        assert_eq!(future.await, ConfirmationResult::GenerationChanged);
        assert!(broker.pending().is_empty());
        assert!(!broker.approve(&id));
    }

    #[tokio::test]
    async fn confirmation_deadline_is_120_seconds_or_the_remaining_request_time() {
        let auth = McpAuthorization::default();
        let mut lease = auth.current_lease();
        let start = Instant::now();
        lease.set_request_deadline(start + Duration::from_secs(125));
        let deadline = lease.confirmation_deadline().expect("確認期限を計算する");
        assert!(deadline >= start + Duration::from_secs(120));
        assert!(deadline <= Instant::now() + Duration::from_secs(120));
        let shorter = Instant::now() + Duration::from_secs(2);
        lease.set_request_deadline(shorter);
        assert_eq!(lease.confirmation_deadline(), Some(shorter));
        lease.set_request_deadline(Instant::now());
        let broker = McpConfirmationBroker::default();
        assert_eq!(
            broker
                .request(request(), &lease, std::future::pending())
                .await,
            ConfirmationResult::TimedOut
        );
        assert!(broker.pending().is_empty());
    }
}
