//! The async dispatcher: a single actor task that drives the pure
//! [`norves_bridge_core::PendingTable`] over a [`Transport`].
//!
//! # Design
//!
//! One `tokio::spawn`ed task owns everything mutable: the moved-in [`Transport`],
//! the [`PendingTable`] of in-flight request waiters, and the [`SeqMonitor`].
//! Nothing is shared behind a `Mutex`; all interaction goes through a bounded
//! command `mpsc`. A [`DispatchHandle`] is the cheap, cloneable front end that
//! sends commands and awaits replies.
//!
//! The task body is a single `tokio::select!` loop multiplexing two sources:
//!
//! * **commands** from the handle ([`Command::SendRequest`], [`Command::Cancel`],
//!   [`Command::Shutdown`]), and
//! * **frames** from `transport.recv()`.
//!
//! A received frame is decoded; a `Response` is matched against the pending
//! table and delivered to its waiter, an `Event` is broadcast, and anything
//! unexpected (unknown id, duplicate, request kind, decode failure) is logged
//! and dropped — never fatal to the task.
//!
//! # Lifecycle / safety
//!
//! * The transport is **moved** into the task, so no borrow crosses the task
//!   boundary.
//! * On shutdown, peer close, or transport error the task drains the pending
//!   table and fails every waiter with [`RequestError::ConnectionClosed`] before
//!   exiting, so no caller hangs.
//! * If the task itself ever stops, the command channel closes; every
//!   [`DispatchHandle::request`] then observes the closed sender and returns
//!   [`RequestError::ConnectionClosed`] instead of hanging.
//! * Request timeouts are enforced on the handle side with
//!   `tokio::time::timeout`; on firing, the handle sends [`Command::Cancel`] so
//!   the task removes the now-orphaned pending entry (no leak). The `Cancel`
//!   carries the request's monotonic generation token, and the task evicts the
//!   entry only when the stored token matches — so a stale `Cancel` from a
//!   timed-out request never evicts a newer request that reused the same `id`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use norves_bridge_core::{
    decode_typed, encode_envelope, CodecError, CorrelationId, Envelope, PendingTable,
    ResponsePayload, SeqMonitor, SeqObservation, ValidatedEnvelope,
};
use tokio::sync::{broadcast, mpsc, oneshot};

/// Capacity of the command channel from each [`DispatchHandle`] to the task.
///
/// Bounded to apply backpressure on callers that flood requests; modest because
/// each command is tiny and the task drains it promptly.
const COMMAND_CHANNEL_CAPACITY: usize = 64;

/// Capacity of the event broadcast channel.
///
/// Bounded per the no-unbounded rule. Chosen generously (events can burst, e.g.
/// log lines) so a momentarily slow subscriber lags rather than loses often;
/// when a subscriber does fall behind, `broadcast` yields `Lagged` and the
/// subscriber resumes from the oldest still-buffered event — the dispatcher
/// never blocks on a slow consumer.
const EVENT_BROADCAST_CAPACITY: usize = 256;

/// Failure of a [`DispatchHandle::request`] call.
///
/// Limited to transport/runtime concerns — connection lost, timed out, or a
/// local decode failure. An engine-level *protocol* error is **not** here: it
/// arrives as `Ok(ResponsePayload::Error(..))` so the caller's domain layer
/// (e.g. the handshake parser) decides how to interpret it.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    /// The dispatcher task is gone (shutdown, peer close, or transport error),
    /// so the request can never be answered.
    #[error("bridge connection closed before a response arrived")]
    ConnectionClosed,

    /// No response arrived within the caller-supplied timeout.
    #[error("request timed out after {0:?}")]
    Timeout(Duration),

    /// Encoding the outgoing request envelope failed before it could be sent.
    #[error("failed to encode request envelope: {0}")]
    Encode(CodecError),
}

/// A command sent from a [`DispatchHandle`] to the dispatcher task.
enum Command {
    /// Send a request envelope and route its response to `responder`.
    ///
    /// `envelope` is boxed to keep this enum small: an `Envelope` is large
    /// relative to the other commands, so inlining it would bloat every queued
    /// command.
    SendRequest {
        id: CorrelationId,
        /// Monotonic generation token identifying *this* in-flight request,
        /// independent of the (caller-chosen, reusable) correlation `id`. Used
        /// by [`Command::Cancel`] to avoid evicting a newer request that reuses
        /// the same `id` after a timeout. See [`DispatchHandle::request`].
        token: u64,
        envelope: Box<Envelope>,
        responder: oneshot::Sender<Result<ResponsePayload, RequestError>>,
    },
    /// Drop the pending waiter for `id` (the caller's request timed out).
    ///
    /// Carries the originating request's generation `token`: the task only
    /// drops the entry when the stored token matches, so a stale `Cancel` from a
    /// timed-out request cannot evict a newer request that reused the same `id`.
    Cancel { id: CorrelationId, token: u64 },
    /// Stop the task; it acknowledges on `ack` after failing all pending.
    Shutdown { ack: oneshot::Sender<()> },
}

/// Cheap, cloneable front end to a running dispatcher task.
///
/// Cloning shares the same command channel and event broadcast, so multiple
/// owners can issue requests and subscribe to events concurrently.
#[derive(Clone)]
pub struct DispatchHandle {
    commands: mpsc::Sender<Command>,
    events: broadcast::Sender<Arc<ValidatedEnvelope>>,
    /// Shared, monotonically increasing source of per-request generation tokens.
    ///
    /// Shared across clones so every request issued through any handle gets a
    /// globally unique token. The token disambiguates a timed-out request's
    /// `Cancel` from a later request that reuses the same correlation `id`.
    next_token: Arc<AtomicU64>,
}

impl DispatchHandle {
    /// Sends `request` and awaits the matching response, up to `timeout`.
    ///
    /// `request` must be a request-kind envelope carrying an `id`; that `id` is
    /// the correlation key. Returns the engine's [`ResponsePayload`] (success or
    /// error) on `Ok`, or a [`RequestError`] for a transport/timeout/encode
    /// failure.
    ///
    /// # Correlation id uniqueness
    ///
    /// The caller is responsible for keeping each `id` unique among its own
    /// *in-flight* requests. If two requests with the same `id` are in flight at
    /// once, the dispatcher fails the prior waiter with
    /// [`RequestError::ConnectionClosed`] and keeps only the latest; the engine's
    /// response can correlate to only one of them. Reusing an `id` *after* a
    /// prior request has completed (resolved or timed out) is fine.
    ///
    /// To make the timed-out case safe, each request is tagged with an internal
    /// monotonic generation token. On timeout the handle sends
    /// [`Command::Cancel`] carrying that token, and the task evicts the pending
    /// entry only if its stored token still matches — so a stale `Cancel` from a
    /// timed-out request can never evict a newer request that happens to reuse
    /// the same `id`.
    pub async fn request(
        &self,
        request: ValidatedEnvelope,
        timeout: Duration,
    ) -> Result<ResponsePayload, RequestError> {
        let id = match &request {
            ValidatedEnvelope::Request { id, .. } => id.clone(),
            // Not a request: it has no correlation id to match a response.
            _ => {
                return Err(RequestError::Encode(CodecError::StructuralViolation(
                    "request() requires a request-kind envelope".to_owned(),
                )))
            }
        };

        let envelope = Envelope::from(request);
        // Encode eagerly so an encode failure is reported to this caller rather
        // than logged-and-dropped inside the task.
        if let Err(err) = encode_envelope(&envelope) {
            return Err(RequestError::Encode(err));
        }

        // Allocate this request's generation token. Relaxed ordering suffices:
        // we only need uniqueness/monotonicity of the counter itself, not
        // ordering against other memory.
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);

        let (responder, response_rx) = oneshot::channel();
        let command = Command::SendRequest {
            id: id.clone(),
            token,
            envelope: Box::new(envelope),
            responder,
        };

        // A closed command channel means the task is gone.
        if self.commands.send(command).await.is_err() {
            return Err(RequestError::ConnectionClosed);
        }

        match tokio::time::timeout(timeout, response_rx).await {
            // Task delivered an outcome (Ok response or a closed/error variant).
            Ok(Ok(outcome)) => outcome,
            // Responder dropped without sending: the task ended (shutdown/close).
            Ok(Err(_recv_err)) => Err(RequestError::ConnectionClosed),
            // Timed out: ask the task to drop the orphaned pending entry so it
            // does not leak, then report the timeout. The token ensures we only
            // drop *our* entry, not a newer request that reused this `id`.
            Err(_elapsed) => {
                // Best effort: if the task is already gone the cleanup is moot.
                let _ = self.commands.send(Command::Cancel { id, token }).await;
                Err(RequestError::Timeout(timeout))
            }
        }
    }

    /// Subscribes to the broadcast of inbound events.
    ///
    /// Safe to call immediately after [`Dispatcher::spawn`] and before any event
    /// arrives: the broadcast sender lives on the handle, so a receiver created
    /// now will see every event the task broadcasts from this point on. A slow
    /// subscriber that falls behind receives [`broadcast::error::RecvError::Lagged`]
    /// and may keep reading.
    pub fn subscribe_events(&self) -> broadcast::Receiver<Arc<ValidatedEnvelope>> {
        self.events.subscribe()
    }

    /// Requests an orderly shutdown and waits for the task to finish failing all
    /// in-flight requests.
    ///
    /// Idempotent in effect: if the task has already stopped, this returns
    /// immediately.
    pub async fn shutdown(&self) {
        let (ack, ack_rx) = oneshot::channel();
        if self.commands.send(Command::Shutdown { ack }).await.is_err() {
            // Task already gone; nothing to wait for.
            return;
        }
        // If the ack sender is dropped (task ended without acking) we still
        // return — the task is no longer running either way.
        let _ = ack_rx.await;
    }
}

/// Owns the spawned dispatcher task; primarily a namespace for [`spawn`].
///
/// [`spawn`] returns only a [`DispatchHandle`]; the task runs detached and is
/// stopped via [`DispatchHandle::shutdown`] or by the peer closing.
///
/// [`spawn`]: Dispatcher::spawn
pub struct Dispatcher;

impl Dispatcher {
    /// Spawns the dispatcher task on the current runtime, taking ownership of
    /// `transport`, and returns a handle to drive it.
    ///
    /// The returned [`DispatchHandle`] is the only way to interact with the
    /// task; the task lives until [`DispatchHandle::shutdown`], a peer close, or
    /// a transport error.
    pub fn spawn<T: Transport>(transport: T) -> DispatchHandle {
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
        let (event_tx, _event_rx) = broadcast::channel(EVENT_BROADCAST_CAPACITY);

        let handle = DispatchHandle {
            commands: command_tx,
            events: event_tx.clone(),
            next_token: Arc::new(AtomicU64::new(0)),
        };

        let task = TaskState {
            transport,
            pending: PendingTable::new(),
            seq: SeqMonitor::new(),
            events: event_tx,
        };
        // The task owns `transport` by value; nothing borrows across the spawn.
        tokio::spawn(task.run(command_rx));

        handle
    }
}

use crate::transport::{Transport, TransportError};

/// All state owned exclusively by the dispatcher task. Never shared.
struct TaskState<T: Transport> {
    transport: T,
    /// In-flight waiters keyed by correlation `id`. The value pairs each waiter
    /// with the request's generation `token` so a stale [`Command::Cancel`] can
    /// be matched against the entry it intended to evict (see the `Cancel` arm
    /// in [`TaskState::run`]).
    pending: PendingTable<(u64, oneshot::Sender<Result<ResponsePayload, RequestError>>)>,
    seq: SeqMonitor,
    events: broadcast::Sender<Arc<ValidatedEnvelope>>,
}

impl<T: Transport> TaskState<T> {
    /// The single select loop. Runs until shutdown, peer close, or transport
    /// error; on every exit path it fails all pending waiters.
    async fn run(mut self, mut commands: mpsc::Receiver<Command>) {
        loop {
            tokio::select! {
                command = commands.recv() => {
                    match command {
                        Some(Command::SendRequest { id, token, envelope, responder }) => {
                            self.on_send_request(id, token, envelope, responder).await;
                        }
                        Some(Command::Cancel { id, token }) => {
                            self.on_cancel(id, token);
                        }
                        Some(Command::Shutdown { ack }) => {
                            tracing::debug!("dispatcher: shutdown requested");
                            self.fail_all_pending();
                            let _ = ack.send(());
                            return;
                        }
                        None => {
                            // Every handle dropped: no more commands can arrive.
                            tracing::debug!("dispatcher: all handles dropped, stopping");
                            self.fail_all_pending();
                            return;
                        }
                    }
                }
                frame = self.transport.recv() => {
                    match frame {
                        Ok(Some(text)) => self.on_frame(text),
                        Ok(None) => {
                            tracing::debug!("dispatcher: peer closed connection");
                            self.fail_all_pending();
                            return;
                        }
                        Err(err) => {
                            self.on_transport_error(err);
                            self.fail_all_pending();
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Registers the waiter and writes the request frame to the transport.
    async fn on_send_request(
        &mut self,
        id: CorrelationId,
        token: u64,
        envelope: Box<Envelope>,
        responder: oneshot::Sender<Result<ResponsePayload, RequestError>>,
    ) {
        // Encoding was already validated on the handle side, but re-encode here
        // to obtain the frame without an unwrap; on the (unreachable) failure
        // path fail the waiter rather than panic.
        let frame = match encode_envelope(&envelope) {
            Ok(frame) => frame,
            Err(err) => {
                let _ = responder.send(Err(RequestError::Encode(err)));
                return;
            }
        };

        // Insert before sending so a fast response can never race ahead of the
        // waiter. A duplicate id replaces the prior waiter; fail the old one.
        // The stored token lets a later `Cancel` target exactly this entry.
        if let Some((_prev_token, previous)) = self.pending.insert(id.clone(), (token, responder)) {
            tracing::warn!(id = %id.as_str(), "dispatcher: request id reused, failing prior waiter");
            let _ = previous.send(Err(RequestError::ConnectionClosed));
        }

        if let Err(err) = self.transport.send(frame).await {
            // The send failed: the waiter we just inserted will never be
            // answered by the peer, so fail it now and surface the cause. Only
            // our own entry (matching token) should be failed here.
            self.on_transport_error(err);
            if let Some((entry_token, waiter)) = self.pending.take(&id) {
                if entry_token == token {
                    let _ = waiter.send(Err(RequestError::ConnectionClosed));
                } else {
                    // A newer request already replaced ours under this id; put
                    // it back untouched.
                    self.pending.insert(id, (entry_token, waiter));
                }
            }
        }
    }

    /// Handles a timed-out request's [`Command::Cancel`].
    ///
    /// Evicts the pending entry only when its stored generation `token` matches
    /// the one carried by the `Cancel`. If a newer request has since reused the
    /// same `id` (different token), the entry is re-inserted unchanged so the
    /// stale `Cancel` does not evict it. The take/compare/re-insert runs within
    /// this single task, so no other path can interleave between the steps.
    fn on_cancel(&mut self, id: CorrelationId, token: u64) {
        if let Some((entry_token, waiter)) = self.pending.take(&id) {
            if entry_token == token {
                // Our orphaned waiter: dropping `waiter` here cancels it.
                drop(waiter);
            } else {
                // A newer in-flight request owns this id now; restore it.
                self.pending.insert(id, (entry_token, waiter));
            }
        }
    }

    /// Decodes one inbound frame and routes it. Never fatal: a decode failure or
    /// an unexpected kind is logged and dropped.
    fn on_frame(&mut self, text: String) {
        let envelope = match decode_typed(&text) {
            Ok(envelope) => envelope,
            Err(err) => {
                // Bad JSON or a structural violation: log and keep the task alive.
                tracing::warn!(error = %err, "dispatcher: dropping undecodable frame");
                return;
            }
        };

        match envelope {
            ValidatedEnvelope::Response { id, payload, .. } => match self.pending.take(&id) {
                Some((_token, waiter)) => {
                    // Receiver may be gone if the caller already timed out; that
                    // is fine — the send simply fails and we move on. The token
                    // is irrelevant here: a response for this id resolves
                    // whichever request currently holds the entry.
                    let _ = waiter.send(Ok(payload));
                }
                None => {
                    tracing::warn!(
                        id = %id.as_str(),
                        "dispatcher: response for unknown/duplicate id, dropping"
                    );
                }
            },
            event @ ValidatedEnvelope::Event { .. } => self.on_event(event),
            ValidatedEnvelope::Request { id, .. } => {
                // The editor client does not serve requests; ignore.
                tracing::warn!(
                    id = %id.as_str(),
                    "dispatcher: unexpected inbound request, dropping"
                );
            }
            // `ValidatedEnvelope` is `#[non_exhaustive]`; a future kind is
            // unknown to this client and is dropped rather than fatal.
            other => {
                tracing::warn!(?other, "dispatcher: unknown envelope kind, dropping");
            }
        }
    }

    /// Observes the event's `seq` (advisory) and broadcasts it.
    ///
    /// The sole caller passes the `Event` variant from the `Event` arm of
    /// [`TaskState::on_frame`]; any other variant carries no `seq` and is
    /// observed as `None`. Read by reference via `if let` (the owned `event`
    /// outlives the read), so there is no unreachable match arm.
    fn on_event(&mut self, event: ValidatedEnvelope) {
        let seq = if let ValidatedEnvelope::Event { seq, .. } = &event {
            *seq
        } else {
            None
        };
        if let SeqObservation::Regressed { last, got } = self.seq.observe(seq) {
            // Advisory only: per the seq policy this never drops the connection.
            tracing::warn!(last, got, "dispatcher: event seq regressed (advisory)");
        }

        // A send error means there are zero live subscribers, which is normal;
        // do not treat it as an error.
        let _ = self.events.send(Arc::new(event));
    }

    /// Logs a transport error in one place.
    fn on_transport_error(&self, err: TransportError) {
        match err {
            TransportError::Closed => {
                tracing::debug!("dispatcher: transport reported closed");
            }
            TransportError::Io(msg) => {
                tracing::warn!(error = %msg, "dispatcher: transport I/O error");
            }
        }
    }

    /// Fails every outstanding waiter with [`RequestError::ConnectionClosed`].
    /// Called on every task-exit path so no caller hangs.
    fn fail_all_pending(&mut self) {
        for (id, (_token, waiter)) in self.pending.drain_all() {
            tracing::debug!(id = %id.as_str(), "dispatcher: failing pending request on shutdown");
            let _ = waiter.send(Err(RequestError::ConnectionClosed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{loopback_pair, LoopbackTransport};
    use norves_bridge_core::{Envelope, MethodName, VersionString};

    /// Builds a request-kind [`ValidatedEnvelope`] with the given id.
    fn request_envelope(id: &str) -> ValidatedEnvelope {
        ValidatedEnvelope::Request {
            version: VersionString::try_from("0.1".to_owned()).expect("valid version"),
            id: CorrelationId::try_from(id.to_owned()).expect("valid id"),
            method: MethodName::try_from("engine.getStatus".to_owned()).expect("valid method"),
            params: None,
            session_id: None,
            seq: None,
        }
    }

    /// Builds a `result` response frame (wire JSON) for the given id.
    fn response_result_frame(id: &str, result: serde_json::Value) -> String {
        let env: Envelope = ValidatedEnvelope::Response {
            version: VersionString::try_from("0.1".to_owned()).expect("valid version"),
            id: CorrelationId::try_from(id.to_owned()).expect("valid id"),
            payload: ResponsePayload::Result(result),
            session_id: None,
            seq: None,
        }
        .into();
        encode_envelope(&env).expect("response encodes")
    }

    /// Builds an event frame (wire JSON) with the given event name and seq.
    fn event_frame(event: &str, seq: Option<u64>) -> String {
        use norves_bridge_core::EventName;
        let env: Envelope = ValidatedEnvelope::Event {
            version: VersionString::try_from("0.1".to_owned()).expect("valid version"),
            event: EventName::try_from(event.to_owned()).expect("valid event name"),
            params: None,
            session_id: None,
            seq,
        }
        .into();
        encode_envelope(&env).expect("event encodes")
    }

    /// Reads the next frame the dispatcher sent out on the peer transport.
    async fn next_outbound(peer: &mut LoopbackTransport) -> String {
        peer.recv()
            .await
            .expect("peer recv ok")
            .expect("a frame was sent")
    }

    #[tokio::test]
    async fn request_response_correlation() {
        let (client, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(client);

        let request = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request(request_envelope("req-1"), Duration::from_secs(5))
                    .await
            }
        });

        // The dispatcher should have written our request out to the peer.
        let _outbound = next_outbound(&mut peer).await;
        // Reply with a matching response.
        peer.send(response_result_frame(
            "req-1",
            serde_json::json!({ "ok": true }),
        ))
        .await
        .expect("peer send ok");

        let outcome = request.await.expect("task joins").expect("request ok");
        match outcome {
            ResponsePayload::Result(value) => {
                assert_eq!(value, serde_json::json!({ "ok": true }));
            }
            ResponsePayload::Error(err) => panic!("expected result, got error {err:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_id_response_is_dropped_without_affecting_others() {
        let (client, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(client);

        let request = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request(request_envelope("req-1"), Duration::from_secs(5))
                    .await
            }
        });

        let _outbound = next_outbound(&mut peer).await;
        // Stray response for an id nobody is waiting on: must be ignored.
        peer.send(response_result_frame(
            "req-999",
            serde_json::json!({ "stray": true }),
        ))
        .await
        .expect("peer send ok");
        // The real response still resolves the request.
        peer.send(response_result_frame(
            "req-1",
            serde_json::json!({ "ok": true }),
        ))
        .await
        .expect("peer send ok");

        let outcome = request.await.expect("task joins").expect("request ok");
        assert!(matches!(outcome, ResponsePayload::Result(_)));
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_fires_and_leaves_no_leak() {
        let (client, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(client);

        // First request: never answered, must time out deterministically.
        let first = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request(request_envelope("req-1"), Duration::from_secs(1))
                    .await
            }
        });
        // Drain the outbound frame so the dispatcher has registered the waiter.
        let _outbound = next_outbound(&mut peer).await;

        // Advance virtual time past the timeout.
        tokio::time::advance(Duration::from_secs(2)).await;

        let first_err = first.await.expect("task joins").expect_err("must time out");
        assert!(matches!(first_err, RequestError::Timeout(_)));

        // Yield so the Cancel command is processed before the next request.
        tokio::task::yield_now().await;

        // A subsequent request must still work (no leaked/poisoned state).
        let second = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request(request_envelope("req-2"), Duration::from_secs(5))
                    .await
            }
        });
        let _outbound2 = next_outbound(&mut peer).await;
        peer.send(response_result_frame(
            "req-2",
            serde_json::json!({ "ok": true }),
        ))
        .await
        .expect("peer send ok");
        let outcome = second.await.expect("task joins").expect("request ok");
        assert!(matches!(outcome, ResponsePayload::Result(_)));
    }

    /// Regression: a request that times out must not let its stale `Cancel`
    /// evict a *later* request that reuses the same correlation `id`.
    ///
    /// Reproduces the bug-report sequence on a single id ("req-X"):
    /// 1. A(id=X) is issued and times out, so the handle sends
    ///    `Cancel{X, token_A}`.
    /// 2. A's (now orphaned) response arrives — A's receiver is gone, so it is a
    ///    silent take-and-drop.
    /// 3. B(id=X) is issued, reusing the same id with a *new* token.
    /// 4. B must resolve normally; the stale `Cancel{X, token_A}` must never
    ///    evict B's waiter (token_B).
    ///
    /// The single FIFO command channel already serializes a `Cancel` ahead of a
    /// later `SendRequest`, so the literal interleaving is hard to force; the
    /// generation token is the explicit guard that keeps the take/compare in
    /// [`TaskState::on_cancel`] from evicting a mismatched (newer) entry. This
    /// test pins the end-to-end same-id-reuse-after-timeout behavior so a
    /// regression in that guard surfaces.
    #[tokio::test(start_paused = true)]
    async fn timed_out_cancel_does_not_evict_reused_id() {
        let (client, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(client);

        // A: id = "req-X", short timeout that we will fire deterministically.
        let first = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request(request_envelope("req-X"), Duration::from_secs(1))
                    .await
            }
        });
        // Drain A's outbound so its waiter (token_A) is registered.
        let _outbound_a = next_outbound(&mut peer).await;

        // Fire A's timeout: the handle now sends Cancel{X, token_A}.
        tokio::time::advance(Duration::from_secs(2)).await;
        let first_err = first.await.expect("task joins").expect_err("must time out");
        assert!(matches!(first_err, RequestError::Timeout(_)));

        // A's late response for the same id: A's receiver is dropped, so this is
        // a silent take-and-drop. Mirrors the bug-report "X 応答到着" step.
        peer.send(response_result_frame(
            "req-X",
            serde_json::json!({ "late": "for-A" }),
        ))
        .await
        .expect("peer send ok");

        // Let the dispatcher drain the queued Cancel{X, token_A} and the stray
        // response before B is registered.
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }

        // B: reuse the same id "req-X" with a fresh token.
        let second = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request(request_envelope("req-X"), Duration::from_secs(5))
                    .await
            }
        });
        let _outbound_b = next_outbound(&mut peer).await;

        // Give any straggling stale Cancel a chance to (wrongly) evict B before
        // we answer it; the token guard must prevent that.
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }

        // Answer B for the reused id. B must resolve with its own result, proving
        // the stale Cancel{X, token_A} did not evict B's waiter (token_B).
        peer.send(response_result_frame(
            "req-X",
            serde_json::json!({ "ok": "for-B" }),
        ))
        .await
        .expect("peer send ok");

        let outcome = second.await.expect("task joins").expect("request ok");
        match outcome {
            ResponsePayload::Result(value) => {
                assert_eq!(value, serde_json::json!({ "ok": "for-B" }));
            }
            ResponsePayload::Error(err) => panic!("expected B's result, got error {err:?}"),
        }
    }

    #[tokio::test]
    async fn shutdown_fails_in_flight_with_connection_closed() {
        let (client, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(client);

        let request = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request(request_envelope("req-1"), Duration::from_secs(30))
                    .await
            }
        });
        let _outbound = next_outbound(&mut peer).await;

        handle.shutdown().await;

        let err = request
            .await
            .expect("task joins")
            .expect_err("must fail on shutdown");
        assert!(matches!(err, RequestError::ConnectionClosed));
    }

    #[tokio::test]
    async fn peer_close_fails_in_flight_with_connection_closed() {
        let (client, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(client);

        let request = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request(request_envelope("req-1"), Duration::from_secs(30))
                    .await
            }
        });
        // Ensure the request is registered before closing the peer.
        let _outbound = next_outbound(&mut peer).await;

        // Drop the peer: its sender closing makes the client transport recv None.
        drop(peer);

        let err = request
            .await
            .expect("task joins")
            .expect_err("must fail on peer close");
        assert!(matches!(err, RequestError::ConnectionClosed));
    }

    #[tokio::test]
    async fn broadcast_event_delivery() {
        let (client, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(client);
        let mut events = handle.subscribe_events();

        peer.send(event_frame("log.message", Some(1)))
            .await
            .expect("peer send ok");

        let event = events.recv().await.expect("event received");
        match &*event {
            ValidatedEnvelope::Event { event, seq, .. } => {
                assert_eq!(event.as_str(), "log.message");
                assert_eq!(*seq, Some(1));
            }
            other => panic!("expected event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn lagged_subscriber_can_continue() {
        // Capacity-1 broadcast is not configurable per-test, so instead we
        // overflow the configured capacity and assert Lagged then continued
        // delivery. We send EVENT_BROADCAST_CAPACITY + a few extra events while
        // the subscriber has not yet read, forcing it to lag.
        let (client, mut peer) = loopback_pair(EVENT_BROADCAST_CAPACITY + 16);
        let handle = Dispatcher::spawn(client);
        let mut events = handle.subscribe_events();

        let overflow = EVENT_BROADCAST_CAPACITY + 8;
        for i in 0..overflow {
            peer.send(event_frame("log.message", Some(i as u64)))
                .await
                .expect("peer send ok");
        }

        // Give the dispatcher time to forward all events into the broadcast.
        for _ in 0..(overflow * 4) {
            tokio::task::yield_now().await;
        }

        // The subscriber lagged; it must observe Lagged then keep reading.
        let mut saw_lagged = false;
        let mut saw_event_after = false;
        loop {
            match events.try_recv() {
                Ok(_event) => {
                    if saw_lagged {
                        saw_event_after = true;
                    }
                }
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    saw_lagged = true;
                }
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }
        assert!(saw_lagged, "expected the subscriber to lag");
        assert!(
            saw_event_after,
            "subscriber must keep receiving after Lagged"
        );
    }

    #[tokio::test]
    async fn decode_failure_keeps_task_alive() {
        let (client, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(client);

        // 1. Broken JSON.
        peer.send("{ this is not json".to_owned())
            .await
            .expect("peer send ok");
        // 2. Well-formed JSON but an invalid envelope (event carrying an id).
        peer.send(
            r#"{
                "bridge": "norves.editor.bridge",
                "version": "0.1",
                "kind": "event",
                "id": "req-x",
                "event": "log.message"
            }"#
            .to_owned(),
        )
        .await
        .expect("peer send ok");

        // The task must still serve a subsequent valid request.
        let request = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request(request_envelope("req-1"), Duration::from_secs(5))
                    .await
            }
        });
        let _outbound = next_outbound(&mut peer).await;
        peer.send(response_result_frame(
            "req-1",
            serde_json::json!({ "ok": true }),
        ))
        .await
        .expect("peer send ok");

        let outcome = request.await.expect("task joins").expect("request ok");
        assert!(matches!(outcome, ResponsePayload::Result(_)));
    }

    /// Exercises a request/response round trip on a real multi-threaded
    /// scheduler. Where the other tests use the single-threaded current-thread
    /// runtime, this proves the dispatcher task and its futures are genuinely
    /// `Send + 'static` and behave under a 2-worker runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn request_response_round_trip_multi_thread() {
        let (client, mut peer) = loopback_pair(8);
        let handle = Dispatcher::spawn(client);

        let request = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request(request_envelope("req-1"), Duration::from_secs(5))
                    .await
            }
        });

        // The dispatcher forwards the request to the peer; reply to it.
        let _outbound = next_outbound(&mut peer).await;
        peer.send(response_result_frame(
            "req-1",
            serde_json::json!({ "ok": true }),
        ))
        .await
        .expect("peer send ok");

        let outcome = request.await.expect("task joins").expect("request ok");
        match outcome {
            ResponsePayload::Result(value) => {
                assert_eq!(value, serde_json::json!({ "ok": true }));
            }
            ResponsePayload::Error(err) => panic!("expected result, got error {err:?}"),
        }
    }
}
