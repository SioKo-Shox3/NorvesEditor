//! Thin editor-side wiring over the core [`SeqMonitor`].
//!
//! Phase D4 keeps this purely observational: the core monitor does all the
//! work; this module only extracts an event's `seq` and feeds it in, returning
//! the resulting [`SeqObservation`]. Acting on a regression (e.g. a `warn!`
//! log) is deferred to D5a once `tracing` is introduced, per the plan.

use norves_bridge_core::{SeqMonitor, SeqObservation, ValidatedEnvelope};

/// Feeds the `seq` of `event`'s envelope into `monitor` and returns the
/// observation.
///
/// Only [`ValidatedEnvelope::Event`] carries a meaningful per-connection event
/// `seq`; for any other envelope kind this observes `None` (treated as
/// [`SeqObservation::Skipped`] by the monitor) so callers can pass any envelope
/// without special-casing.
pub fn observe_event_seq(monitor: &mut SeqMonitor, event: &ValidatedEnvelope) -> SeqObservation {
    let seq = match event {
        ValidatedEnvelope::Event { seq, .. } => *seq,
        _ => None,
    };
    monitor.observe(seq)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_with_seq(seq: u64) -> ValidatedEnvelope {
        let json = format!(
            r#"{{
                "bridge": "norves.editor.bridge",
                "version": "0.1",
                "kind": "event",
                "event": "log.message",
                "seq": {seq},
                "params": {{ "level": "info", "message": "hi" }}
            }}"#
        );
        let env: norves_bridge_core::Envelope =
            serde_json::from_str(&json).expect("fixture deserializes");
        ValidatedEnvelope::try_from(env).expect("fixture validates")
    }

    #[test]
    fn observes_monotonic_event_seq() {
        let mut monitor = SeqMonitor::new();
        assert_eq!(
            observe_event_seq(&mut monitor, &event_with_seq(1)),
            SeqObservation::Ok
        );
        assert_eq!(
            observe_event_seq(&mut monitor, &event_with_seq(2)),
            SeqObservation::Ok
        );
    }

    #[test]
    fn observes_regression_advisory() {
        let mut monitor = SeqMonitor::new();
        observe_event_seq(&mut monitor, &event_with_seq(5));
        assert_eq!(
            observe_event_seq(&mut monitor, &event_with_seq(3)),
            SeqObservation::Regressed { last: 5, got: 3 }
        );
    }
}
