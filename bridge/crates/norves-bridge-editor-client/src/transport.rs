//! The async byte/frame boundary the [`crate::Dispatcher`] sits on top of.
//!
//! A [`Transport`] carries exactly one wire envelope per frame, as the raw JSON
//! text (`String`). It is deliberately ignorant of the Bridge protocol: it does
//! no `serde` work and never touches a [`norves_bridge_core::Envelope`]. All
//! encode/decode lives in the dispatcher, so the transport stays a thin pipe
//! that a future WebSocket implementation can satisfy without knowing the
//! schema.
//!
//! The trait uses `-> impl Future + Send` rather than `async fn` in trait so the
//! returned futures are nameable as `Send` and the dispatcher task is provably
//! `Send + 'static` without an `async_trait` allocation on every call.

use tokio::sync::mpsc;

/// A bidirectional, frame-oriented async transport. One frame == one wire
/// envelope, carried as its JSON text.
///
/// `recv` returns `Ok(None)` for a graceful peer close (EOF) and `Err` for an
/// abnormal failure; the dispatcher treats both as terminal but distinguishes
/// them in logs.
///
/// The transport is owned outright by the dispatcher task (moved in at
/// `spawn`), so `&mut self` here never needs to cross a task boundary as a
/// borrow.
///
/// Fallback: if a `Box<dyn Transport>` is ever required (e.g. selecting a
/// transport at runtime), migrate this to `async_trait` or a hand-written
/// `BoxFuture`-returning version — `impl Future` in trait cannot be made into a
/// trait object.
pub trait Transport: Send + 'static {
    /// Sends one frame (a complete wire envelope as JSON text).
    fn send(
        &mut self,
        frame: String,
    ) -> impl std::future::Future<Output = Result<(), TransportError>> + Send;

    /// Receives the next frame. `Ok(None)` == the peer closed the connection
    /// cleanly; `Err` == an abnormal transport failure.
    fn recv(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Option<String>, TransportError>> + Send;
}

/// A transport-level failure, kept intentionally small.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The transport is closed (the peer endpoint is gone) and can carry no
    /// more frames.
    #[error("transport closed")]
    Closed,

    /// An underlying I/O failure, described by its message.
    #[error("transport I/O error: {0}")]
    Io(String),
}

/// Default channel capacity for an in-memory [`LoopbackTransport`] pair. Small
/// on purpose: tests want to observe backpressure without large buffers.
pub const LOOPBACK_DEFAULT_CAPACITY: usize = 16;

/// An in-memory [`Transport`] that pipes frames to a paired endpoint over a
/// bounded `mpsc` channel.
///
/// This is a pure bidirectional pipe with **no** protocol behaviour — it never
/// generates or interprets envelopes. It exists so the dispatcher can be driven
/// and verified in tests (and reused by later phases). Create a connected pair
/// with [`loopback_pair`].
#[derive(Debug)]
pub struct LoopbackTransport {
    /// Frames we send go out here, to the peer's `incoming`.
    outgoing: mpsc::Sender<String>,
    /// Frames the peer sent us arrive here.
    incoming: mpsc::Receiver<String>,
}

impl LoopbackTransport {
    /// Builds one endpoint from its send/receive halves. Internal helper for
    /// [`loopback_pair`].
    fn new(outgoing: mpsc::Sender<String>, incoming: mpsc::Receiver<String>) -> Self {
        LoopbackTransport { outgoing, incoming }
    }
}

impl Transport for LoopbackTransport {
    // The trait declares `-> impl Future + Send`; an `async fn` in the impl
    // satisfies that and reads more clearly than a hand-rolled `async move`
    // block.
    async fn send(&mut self, frame: String) -> Result<(), TransportError> {
        // A send error means the peer's receiver was dropped: peer is gone.
        self.outgoing
            .send(frame)
            .await
            .map_err(|_| TransportError::Closed)
    }

    async fn recv(&mut self) -> Result<Option<String>, TransportError> {
        // `None` from the channel == every peer sender was dropped == clean
        // close, mapped to `Ok(None)`.
        Ok(self.incoming.recv().await)
    }
}

/// Creates two connected [`LoopbackTransport`] endpoints whose `send`/`recv`
/// are cross-wired: a frame sent on one arrives on the other's `recv`.
///
/// `capacity` is the bounded buffer for each direction (must be >= 1; a
/// `capacity` of 0 is not supported by `tokio::sync::mpsc` and will panic in
/// the channel constructor).
pub fn loopback_pair(capacity: usize) -> (LoopbackTransport, LoopbackTransport) {
    let (a_tx, a_rx) = mpsc::channel(capacity);
    let (b_tx, b_rx) = mpsc::channel(capacity);
    // a sends into b's incoming; b sends into a's incoming.
    let a = LoopbackTransport::new(a_tx, b_rx);
    let b = LoopbackTransport::new(b_tx, a_rx);
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn loopback_round_trips_a_frame() {
        let (mut a, mut b) = loopback_pair(4);
        a.send("hello".to_owned()).await.expect("send ok");
        let got = b.recv().await.expect("recv ok");
        assert_eq!(got, Some("hello".to_owned()));
    }

    #[tokio::test]
    async fn recv_returns_none_after_peer_dropped() {
        let (a, mut b) = loopback_pair(4);
        drop(a);
        // With the peer's sender gone, recv drains to a clean close.
        let got = b.recv().await.expect("recv ok");
        assert_eq!(got, None);
    }

    #[tokio::test]
    async fn send_errors_closed_after_peer_dropped() {
        let (mut a, b) = loopback_pair(4);
        drop(b);
        let err = a.send("x".to_owned()).await.expect_err("send must fail");
        assert!(matches!(err, TransportError::Closed));
    }
}
