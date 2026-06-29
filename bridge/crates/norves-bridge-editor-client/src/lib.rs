//! `norves-bridge-editor-client` — editor-side Bridge runtime built on `norves-bridge-core`.
//!
//! Phase D4 added the pure (sans-I/O, synchronous) domain layer: the
//! `bridge.hello` handshake params/result types, `engine.getStatus` and
//! `log.message` payload extraction, and a thin wrapper over the core
//! [`norves_bridge_core::SeqMonitor`].
//!
//! Phase D5a adds the async runtime layer on top: a [`Transport`] trait (the
//! frame boundary), a [`Dispatcher`] actor task that drives the core
//! [`norves_bridge_core::PendingTable`] for request/response correlation and
//! broadcasts inbound events, and a [`DispatchHandle`] front end with a
//! timeout-bounded `request` and a `shutdown` path. An in-memory
//! [`LoopbackTransport`] (via [`loopback_pair`]) is provided to drive and test
//! the dispatcher; engine response logic and round-trip integration land in a
//! later phase.

pub mod asset;
pub mod dispatcher;
pub mod handshake;
pub mod log;
pub mod object;
pub mod reconnect;
pub mod scene;
pub mod seq;
pub mod status;
pub mod transport;
pub mod viewport;
pub mod ws_transport;

pub use asset::{
    parse_asset_manifest_result, parse_asset_resolve_result, AssetEntry, AssetError,
    AssetManifestResult, AssetResolveResult, AssetResolveSource, AssetResolveStatus,
};
pub use dispatcher::{DispatchHandle, Dispatcher, RequestError};
pub use handshake::{
    hello_error_to_handshake, parse_hello_result, HandshakeError, HelloOutcome, HelloParams,
    HelloRole,
};
pub use log::{parse_log_message, LogError, LogMessage};
pub use object::{
    parse_object_snapshot_result, parse_schema_snapshot_result, parse_set_property_result,
    ObjectError, ObjectSnapshot, PropertyDefinition, PropertyEntry, SchemaSnapshot, SetPropertyAck,
    TypeDescriptor,
};
pub use reconnect::{connect_with_retry, ConnectError, ReconnectManager, RetryConfig};
pub use scene::{parse_scene_tree_result, SceneError, SceneNode, SceneTree};
pub use seq::observe_event_seq;
pub use status::{parse_status_result, StatusError, StatusSnapshot};
pub use transport::{loopback_pair, LoopbackTransport, Transport, TransportError};
pub use viewport::{parse_thumbnail_result, ViewportError, ViewportThumbnail};
pub use ws_transport::WsClientTransport;
