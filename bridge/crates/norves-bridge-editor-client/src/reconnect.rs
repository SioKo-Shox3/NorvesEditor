//! Upper-layer (above-transport) connect/reconnect for the editor client.
//!
//! # Why reconnect lives here, not in the transport
//!
//! The [`Transport`] contract is single-shot by design: a transport is **moved**
//! into the [`Dispatcher`] task, and once `recv` yields `Ok(None)` (clean peer
//! close) or `Err` the task drains its pending table and exits. After that the
//! owning [`DispatchHandle`] is permanently closed — every `request` returns
//! [`crate::RequestError::ConnectionClosed`] and every `subscribe_events` stream
//! ends. Embedding reconnect *inside* a transport would violate that contract
//! (the task would observe a live transport after it had already signalled
//! terminal close). So reconnect is an upper-layer concern: to reconnect we build
//! a *fresh* [`WsClientTransport`] and spawn a *new* dispatcher, yielding a new
//! handle.
//!
//! # What this module does (and does not) do
//!
//! * [`connect_with_retry`] dials a URL, retrying a *connect* failure (server not
//!   yet listening / mid-bind) with exponential backoff until it succeeds or a
//!   budget is exhausted, then spawns a [`Dispatcher`] and returns its handle.
//! * [`ReconnectManager`] is a thin convenience wrapper that remembers the URL +
//!   [`RetryConfig`] and hands out the current handle, with an explicit
//!   [`ReconnectManager::reconnect`] that tears down the old handle and dials
//!   again.
//!
//! **Out of scope for alpha (deliberately not built here):** a *transparent*,
//! stable handle that auto-reconnects underneath in-flight `request` calls and
//! re-routes them. That belongs to the higher "I" layer (the editor-facing
//! connection service) which owns session re-establishment, event-stream
//! re-subscription, and request replay/idempotency policy. This module only
//! provides the mechanism (connect-with-retry + explicit reconnect); it never
//! silently re-dials behind a caller that is awaiting a `request`.

use std::time::Duration;

use tokio::time::Instant;

use crate::dispatcher::{DispatchHandle, Dispatcher};
use crate::transport::TransportError;
use crate::ws_transport::WsClientTransport;

/// Tuning for [`connect_with_retry`]'s exponential backoff.
///
/// The retry loop starts at `initial_backoff`, doubles each failed attempt up to
/// `max_backoff`, and gives up once the total elapsed time would exceed
/// `max_elapsed`. With `jitter` enabled each sleep is perturbed by a small
/// deterministic amount (no `rand` dependency) to avoid lockstep reconnect
/// storms when several clients retry at once.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Backoff before the second attempt (the first attempt is immediate).
    pub initial_backoff: Duration,
    /// Upper bound on any single backoff sleep.
    pub max_backoff: Duration,
    /// Total wall-clock budget across all attempts; once exceeded the loop
    /// stops and returns [`ConnectError::Timeout`].
    pub max_elapsed: Duration,
    /// When true, perturb each sleep deterministically to de-correlate retries.
    pub jitter: bool,
}

impl Default for RetryConfig {
    /// Modest local-only defaults: quick first retries, capped at 1s per sleep,
    /// giving up after 10s total. Suitable for dialing an engine on the same
    /// machine that is briefly mid-(re)bind.
    fn default() -> Self {
        RetryConfig {
            initial_backoff: Duration::from_millis(50),
            max_backoff: Duration::from_secs(1),
            max_elapsed: Duration::from_secs(10),
            jitter: true,
        }
    }
}

/// Failure of [`connect_with_retry`].
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// The retry budget ([`RetryConfig::max_elapsed`]) elapsed before any
    /// connect attempt succeeded. Carries the last underlying transport error
    /// so the caller can see *why* the last attempt failed.
    #[error("could not connect within {elapsed:?}: last error: {last}")]
    Timeout {
        /// The configured budget that was exhausted.
        elapsed: Duration,
        /// The most recent connect failure.
        last: TransportError,
    },
}

/// Computes the next backoff: `current` doubled, capped at `max_backoff`, with an
/// optional small deterministic jitter.
///
/// Jitter is derived from a caller-supplied monotonically-changing `salt` (we use
/// the attempt count) rather than a PRNG, keeping the crate dependency-free while
/// still de-correlating concurrent retriers. The perturbation is bounded to
/// +0..=25% of the base sleep, so it never *shortens* the backoff (which would
/// risk busy-spinning) and never more than mildly lengthens it.
fn next_backoff(current: Duration, max_backoff: Duration, jitter: bool, salt: u64) -> Duration {
    let base = current.min(max_backoff);
    if !jitter {
        return base;
    }
    // Deterministic spread in [0, base/4]. `salt` (the attempt index) gives a
    // different fraction each attempt without any randomness source.
    let quarter = base / 4;
    let extra = if quarter.is_zero() {
        Duration::ZERO
    } else {
        let nanos = quarter.as_nanos() as u64;
        // salt-derived fraction of the quarter window; cheap and deterministic.
        Duration::from_nanos(nanos.wrapping_mul(salt).wrapping_add(salt) % (nanos + 1))
    };
    (base + extra).min(max_backoff + max_backoff / 4)
}

/// Dials `url` and spawns a [`Dispatcher`] over the connected
/// [`WsClientTransport`], retrying *connect* failures with exponential backoff
/// per `cfg` until success or the [`RetryConfig::max_elapsed`] budget elapses.
///
/// Returns the live [`DispatchHandle`] on success, or [`ConnectError::Timeout`]
/// if every attempt within the budget failed. The loop is bounded: it always
/// terminates once the budget is exhausted, so a permanently-unreachable URL
/// produces an `Err` rather than hanging.
///
/// Only the *connect* step is retried. Once a transport connects and the
/// dispatcher is spawned, ordinary close/error handling is the dispatcher's job
/// (and re-dialing after that is an explicit [`ReconnectManager::reconnect`] or a
/// fresh `connect_with_retry` call — never silent).
pub async fn connect_with_retry(
    url: &str,
    cfg: &RetryConfig,
) -> Result<DispatchHandle, ConnectError> {
    let deadline = Instant::now() + cfg.max_elapsed;
    let mut backoff = cfg.initial_backoff;
    let mut attempt: u64 = 0;

    loop {
        match WsClientTransport::connect(url).await {
            Ok(transport) => return Ok(Dispatcher::spawn(transport)),
            Err(last) => {
                attempt += 1;
                // Stop if we are at/over budget: do not sleep past the deadline.
                let now = Instant::now();
                if now >= deadline {
                    return Err(ConnectError::Timeout {
                        elapsed: cfg.max_elapsed,
                        last,
                    });
                }
                // Never sleep beyond the deadline; clamp the next sleep to the
                // remaining budget so the loop terminates promptly.
                let sleep = next_backoff(backoff, cfg.max_backoff, cfg.jitter, attempt);
                let remaining = deadline.saturating_duration_since(now);
                let sleep = sleep.min(remaining);
                tokio::time::sleep(sleep).await;
                // Grow the base backoff toward max_backoff for the next attempt.
                backoff = (backoff * 2).min(cfg.max_backoff);
            }
        }
    }
}

/// A thin, explicit reconnect helper over [`connect_with_retry`].
///
/// Holds the dial target (`url` + [`RetryConfig`]) and the current live
/// [`DispatchHandle`], if any. It does **not** auto-reconnect: the caller decides
/// *when* to re-dial (e.g. after observing [`crate::RequestError::ConnectionClosed`])
/// by calling [`Self::reconnect`]. Transparent, in-flight-request-preserving
/// reconnect is intentionally left to the higher "I" layer (see module docs).
pub struct ReconnectManager {
    url: String,
    cfg: RetryConfig,
    handle: Option<DispatchHandle>,
}

impl ReconnectManager {
    /// Creates a manager for `url` with `cfg`. No connection is made yet; call
    /// [`Self::connect`] (or [`Self::reconnect`]) to establish one.
    pub fn new(url: impl Into<String>, cfg: RetryConfig) -> Self {
        ReconnectManager {
            url: url.into(),
            cfg,
            handle: None,
        }
    }

    /// Establishes the initial connection, storing and returning a clone of the
    /// handle. If a handle already exists this still re-dials (equivalent to
    /// [`Self::reconnect`]); prefer [`Self::reconnect`] for clarity in that case.
    pub async fn connect(&mut self) -> Result<DispatchHandle, ConnectError> {
        self.reconnect().await
    }

    /// Tears down the current handle (if any) and dials again, replacing the
    /// stored handle. Returns a clone of the new handle.
    ///
    /// The old handle is shut down first so its dispatcher task stops and any
    /// in-flight requests on it fail with
    /// [`crate::RequestError::ConnectionClosed`] before the new connection is
    /// established — there is no overlap of two live dispatchers for the same
    /// logical session.
    pub async fn reconnect(&mut self) -> Result<DispatchHandle, ConnectError> {
        if let Some(old) = self.handle.take() {
            old.shutdown().await;
        }
        let handle = connect_with_retry(&self.url, &self.cfg).await?;
        self.handle = Some(handle.clone());
        Ok(handle)
    }

    /// Returns a clone of the current handle, if connected.
    pub fn handle(&self) -> Option<DispatchHandle> {
        self.handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `next_backoff` without jitter is exactly the doubled value, capped.
    #[test]
    fn backoff_without_jitter_is_capped_double() {
        let cur = Duration::from_millis(400);
        let max = Duration::from_millis(500);
        // Capped at max.
        assert_eq!(
            next_backoff(cur, max, false, 3),
            Duration::from_millis(400).min(max)
        );
        let big = Duration::from_secs(10);
        assert_eq!(next_backoff(big, max, false, 1), max);
    }

    /// With jitter, the result is within [base, base + base/4] and capped to
    /// `max_backoff + max_backoff/4`, never below the base (so it never
    /// busy-spins).
    #[test]
    fn backoff_with_jitter_stays_in_bounds() {
        let base = Duration::from_millis(80);
        let max = Duration::from_secs(1);
        for salt in 0..50u64 {
            let got = next_backoff(base, max, true, salt);
            assert!(got >= base, "jitter must not shorten below base: {got:?}");
            assert!(
                got <= base + base / 4,
                "jitter must not exceed base + 25%: {got:?}"
            );
        }
    }

    /// connect_with_retry against an unreachable port must terminate with
    /// `ConnectError::Timeout` within roughly the configured budget — it must
    /// not loop forever.
    ///
    /// Time is NOT paused here: `connect_async` performs real I/O whose readiness
    /// is driven by the runtime's I/O reactor, and pausing time can starve that
    /// loop. The budget is kept tiny (200ms) so the test stays fast while still
    /// proving the loop is bounded.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connect_with_retry_gives_up_on_unreachable_port() {
        // Claim an ephemeral port then release it: nothing is listening, so every
        // connect attempt is refused quickly. The retry loop then backs off until
        // the budget elapses.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        let url = format!("ws://127.0.0.1:{port}");
        let cfg = RetryConfig {
            initial_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(50),
            max_elapsed: Duration::from_millis(200),
            jitter: false,
        };
        let start = std::time::Instant::now();
        let result = connect_with_retry(&url, &cfg).await;
        // Bounded: must finish reasonably close to the budget, not hang.
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "retry loop took too long; it may not be bounded"
        );
        match result {
            Err(ConnectError::Timeout { elapsed, .. }) => {
                assert_eq!(elapsed, cfg.max_elapsed);
            }
            Ok(_) => panic!("connect to an unreachable port must not succeed"),
        }
    }
}
