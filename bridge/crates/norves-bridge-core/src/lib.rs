//! `norves-bridge-core` — NorvesEditor Bridge core message, status, and value types.
//!
//! Wire-protocol envelope, status, and value types live here. The JSON codec
//! entry points (`encode`/`decode`) and fixture-driven conformance tests are
//! added in later phases; this crate currently provides the typed model plus
//! [`Envelope::validate`] for the kind-dependent structural rules.
//!
//! This crate must not depend on Tauri, the UI layer, or NorvesLib (see
//! CLAUDE.md architecture boundaries).

pub mod common;
pub mod envelope;
pub mod error;
pub mod value;

pub use common::{
    CapabilityDescriptor, CapabilityToken, EngineState, LogLevel, ObjectId, Origin, RuntimeState,
    VersionString, ViewportState,
};
pub use envelope::{
    BridgeMarker, CorrelationId, Envelope, EventName, Kind, MethodName, BRIDGE_MARKER,
};
pub use error::{BridgeError, CodecError, ErrorCode};
pub use value::{
    PropertyBag, PropertyDefinition, PropertyEntry, PropertyValue, SceneNode, TypeDescriptor,
};
