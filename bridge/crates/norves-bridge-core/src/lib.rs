//! `norves-bridge-core` — NorvesEditor Bridge core message, status, and value types.
//!
//! Wire-protocol envelope, status, and value types live here, along with the
//! JSON codec entry points ([`decode_envelope`] / [`decode_validated`] /
//! [`encode_envelope`]) and [`Envelope::validate`] for the kind-dependent
//! structural rules. Envelope-layer round-trip is covered by fixture-driven
//! conformance tests; per-method / per-event payload schema validation is a
//! later phase.
//!
//! This crate must not depend on Tauri, the UI layer, or NorvesLib (see
//! CLAUDE.md architecture boundaries).

pub mod codec;
pub mod common;
pub mod correlation;
pub mod envelope;
pub mod error;
pub mod value;

pub use codec::{decode_envelope, decode_typed, decode_validated, encode_envelope};
pub use common::{
    CapabilityDescriptor, CapabilityToken, EngineState, LogLevel, ObjectId, Origin, RuntimeState,
    VersionString, ViewportState,
};
pub use correlation::{PendingTable, SeqMonitor, SeqObservation};
pub use envelope::{
    BridgeMarker, CorrelationId, Envelope, EventName, Kind, MethodName, ResponsePayload,
    ValidatedEnvelope, BRIDGE_MARKER,
};
pub use error::{BridgeError, CodecError, ErrorCode};
pub use value::{
    PropertyBag, PropertyDefinition, PropertyEntry, PropertyValue, SceneNode, TypeDescriptor,
};
