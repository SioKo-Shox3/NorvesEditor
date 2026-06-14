//! Pure, sans-I/O request/response correlation state.
//!
//! [`PendingTable`] tracks in-flight requests keyed by [`CorrelationId`] so an
//! async dispatcher (a later phase) can register a waiter when it sends a
//! request and retrieve it when the matching response arrives. The table holds
//! no channels, no clock, and no async machinery: the waiter type `T` is fully
//! generic so the core crate never has to depend on `tokio`. The dispatcher
//! supplies whatever waiter it needs (e.g. a `oneshot::Sender`).
//!
//! [`SeqMonitor`] tracks per-session event `seq` monotonicity. Per
//! `message-envelope.md`, `seq` is an ordering/debugging aid and is **not** a
//! correlation key, so regressions are surfaced as advisory observations rather
//! than treated as fatal.

use std::collections::HashMap;

use crate::envelope::CorrelationId;

/// A pure map of in-flight requests awaiting a response, keyed by
/// [`CorrelationId`].
///
/// Generic over the waiter type `T` so this stays free of any async / channel
/// dependency. Insertion, retrieval, and bulk drain (for shutdown) are the only
/// operations; ordering and timeouts are the caller's concern.
#[derive(Debug)]
pub struct PendingTable<T> {
    pending: HashMap<CorrelationId, T>,
}

impl<T> PendingTable<T> {
    /// Creates an empty table.
    pub fn new() -> Self {
        PendingTable {
            pending: HashMap::new(),
        }
    }

    /// Registers `waiter` under `id`.
    ///
    /// If a waiter was already registered for `id`, it is replaced and the
    /// previous waiter is returned so the caller can decide how to fail it.
    pub fn insert(&mut self, id: CorrelationId, waiter: T) -> Option<T> {
        self.pending.insert(id, waiter)
    }

    /// Removes and returns the waiter registered for `id`, if any.
    ///
    /// Returns `None` for an unknown id (e.g. a duplicate or late response).
    pub fn take(&mut self, id: &CorrelationId) -> Option<T> {
        self.pending.remove(id)
    }

    /// Removes and yields every registered `(id, waiter)` pair, leaving the
    /// table empty. Intended for shutdown, where all outstanding requests must
    /// be failed at once.
    pub fn drain_all(&mut self) -> impl Iterator<Item = (CorrelationId, T)> + '_ {
        self.pending.drain()
    }

    /// Number of currently registered waiters.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Returns `true` if no waiters are registered.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

impl<T> Default for PendingTable<T> {
    fn default() -> Self {
        PendingTable::new()
    }
}

/// Outcome of observing one event's `seq` value.
///
/// Returned by [`SeqMonitor::observe`]. `Regressed` is **advisory** only — see
/// [`SeqMonitor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqObservation {
    /// The seq advanced by exactly one, or this is the first seq seen.
    Ok,
    /// The seq advanced by more than one: one or more intervening events were
    /// missed or carried no seq. Not an error.
    Skipped,
    /// The seq did not advance (it went backward or repeated the last value).
    Regressed {
        /// The last seq the monitor had recorded.
        last: u64,
        /// The seq just observed.
        got: u64,
    },
}

/// Per-connection monitor of event `seq` monotonicity.
///
/// `seq` is, per `message-envelope.md`, an ordering/debugging aid and **not** a
/// correlation key; its behaviour on regression is unspecified by the protocol.
/// Accordingly this monitor never treats a regression as fatal: it returns
/// [`SeqObservation::Regressed`] so the caller may emit a warning, but the
/// caller must **not** use it to decide to drop the connection.
///
/// Update policy: `last` always holds the **maximum** seq observed so far. A
/// regression therefore does **not** lower `last`; the recorded high-water mark
/// is retained so a single out-of-order event does not desynchronise subsequent
/// monotonicity checks.
#[derive(Debug, Clone, Copy, Default)]
pub struct SeqMonitor {
    last: Option<u64>,
}

impl SeqMonitor {
    /// Creates a monitor that has not yet observed any seq.
    pub fn new() -> Self {
        SeqMonitor { last: None }
    }

    /// Records `seq` and classifies it relative to the previously seen value.
    ///
    /// * `None` -> [`SeqObservation::Skipped`] (the event carried no seq; `last`
    ///   is left unchanged).
    /// * first seq seen, or `last + 1` -> [`SeqObservation::Ok`].
    /// * a jump of more than one -> [`SeqObservation::Skipped`].
    /// * a value `<= last` -> [`SeqObservation::Regressed`].
    ///
    /// `last` is updated to the maximum of the prior `last` and `seq`, so a
    /// regression never lowers the recorded high-water mark.
    pub fn observe(&mut self, seq: Option<u64>) -> SeqObservation {
        let Some(seq) = seq else {
            return SeqObservation::Skipped;
        };

        let observation = match self.last {
            None => SeqObservation::Ok,
            Some(last) if seq == last + 1 => SeqObservation::Ok,
            Some(last) if seq > last => SeqObservation::Skipped,
            Some(last) => SeqObservation::Regressed { last, got: seq },
        };

        // `last` retains the maximum observed seq (high-water mark).
        self.last = Some(match self.last {
            Some(last) => last.max(seq),
            None => seq,
        });

        observation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> CorrelationId {
        CorrelationId::try_from(value.to_owned()).expect("non-empty id is valid")
    }

    #[test]
    fn pending_table_insert_and_take() {
        let mut table: PendingTable<i32> = PendingTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);

        assert_eq!(table.insert(id("req-1"), 10), None);
        assert_eq!(table.len(), 1);
        assert!(!table.is_empty());

        assert_eq!(table.take(&id("req-1")), Some(10));
        assert_eq!(table.len(), 0);
        assert!(table.is_empty());
    }

    #[test]
    fn pending_table_take_unknown_is_none() {
        let mut table: PendingTable<i32> = PendingTable::new();
        assert_eq!(table.take(&id("missing")), None);
    }

    #[test]
    fn pending_table_duplicate_insert_returns_previous() {
        let mut table: PendingTable<i32> = PendingTable::new();
        assert_eq!(table.insert(id("req-1"), 10), None);
        // Re-inserting the same id replaces and returns the prior waiter.
        assert_eq!(table.insert(id("req-1"), 20), Some(10));
        assert_eq!(table.len(), 1);
        assert_eq!(table.take(&id("req-1")), Some(20));
    }

    #[test]
    fn pending_table_drain_all_removes_everything() {
        let mut table: PendingTable<i32> = PendingTable::new();
        table.insert(id("req-1"), 1);
        table.insert(id("req-2"), 2);
        table.insert(id("req-3"), 3);

        let mut drained: Vec<(String, i32)> = table
            .drain_all()
            .map(|(k, v)| (k.as_str().to_owned(), v))
            .collect();
        drained.sort();
        assert_eq!(
            drained,
            vec![
                ("req-1".to_owned(), 1),
                ("req-2".to_owned(), 2),
                ("req-3".to_owned(), 3),
            ]
        );
        assert!(table.is_empty());
    }

    #[test]
    fn pending_table_default_is_empty() {
        let table: PendingTable<i32> = PendingTable::default();
        assert!(table.is_empty());
    }

    #[test]
    fn seq_monitor_none_is_skipped_without_advancing() {
        let mut monitor = SeqMonitor::new();
        assert_eq!(monitor.observe(None), SeqObservation::Skipped);
        // A None observation must not establish a baseline.
        assert_eq!(monitor.observe(Some(5)), SeqObservation::Ok);
    }

    #[test]
    fn seq_monitor_first_and_monotonic_are_ok() {
        let mut monitor = SeqMonitor::new();
        assert_eq!(monitor.observe(Some(1)), SeqObservation::Ok);
        assert_eq!(monitor.observe(Some(2)), SeqObservation::Ok);
        assert_eq!(monitor.observe(Some(3)), SeqObservation::Ok);
    }

    #[test]
    fn seq_monitor_gap_is_skipped() {
        let mut monitor = SeqMonitor::new();
        assert_eq!(monitor.observe(Some(1)), SeqObservation::Ok);
        assert_eq!(monitor.observe(Some(5)), SeqObservation::Skipped);
        // After a skip, last is 5, so 6 is Ok.
        assert_eq!(monitor.observe(Some(6)), SeqObservation::Ok);
    }

    #[test]
    fn seq_monitor_backward_is_regressed() {
        let mut monitor = SeqMonitor::new();
        assert_eq!(monitor.observe(Some(5)), SeqObservation::Ok);
        assert_eq!(
            monitor.observe(Some(3)),
            SeqObservation::Regressed { last: 5, got: 3 }
        );
    }

    #[test]
    fn seq_monitor_equal_is_regressed() {
        let mut monitor = SeqMonitor::new();
        assert_eq!(monitor.observe(Some(5)), SeqObservation::Ok);
        assert_eq!(
            monitor.observe(Some(5)),
            SeqObservation::Regressed { last: 5, got: 5 }
        );
    }

    #[test]
    fn seq_monitor_retains_high_water_mark_after_regression() {
        let mut monitor = SeqMonitor::new();
        assert_eq!(monitor.observe(Some(5)), SeqObservation::Ok);
        // Regression does not lower the recorded maximum...
        assert_eq!(
            monitor.observe(Some(2)),
            SeqObservation::Regressed { last: 5, got: 2 }
        );
        // ...so 6 (5 + 1) is Ok, confirming last stayed at 5.
        assert_eq!(monitor.observe(Some(6)), SeqObservation::Ok);
    }
}
