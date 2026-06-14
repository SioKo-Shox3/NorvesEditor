//! `norves-bridge-editor-client` — editor-side Bridge runtime built on `norves-bridge-core`.
//!
//! Phase D4 adds the pure (sans-I/O, synchronous) domain layer: the
//! `bridge.hello` handshake params/result types, `engine.getStatus` and
//! `log.message` payload extraction, and a thin wrapper over the core
//! [`norves_bridge_core::SeqMonitor`]. No `tokio` / `tracing` yet — the async
//! transport and logging arrive in a later phase.

pub mod handshake;
pub mod log;
pub mod seq;
pub mod status;

pub use handshake::{
    hello_error_to_handshake, parse_hello_result, HandshakeError, HelloOutcome, HelloParams,
    HelloRole,
};
pub use log::{parse_log_message, LogError, LogMessage};
pub use seq::observe_event_seq;
pub use status::{parse_status_result, StatusError, StatusSnapshot};
