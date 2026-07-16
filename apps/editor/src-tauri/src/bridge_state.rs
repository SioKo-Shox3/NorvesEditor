//! The Tauri-managed backend that OWNS the Bridge connection and lifecycle.
//!
//! # Ownership / threading model
//!
//! The UI never sees a raw socket. All interaction goes through the
//! `#[tauri::command]` functions here, which read a cloned [`DispatchHandle`]
//! out of the managed [`BridgeState`] and drive the editor-client runtime.
//!
//! ## No-lock-across-await (plan M1)
//!
//! The state is guarded by a `tokio::sync::Mutex`. The hard rule: **never hold
//! the guard across an `.await` that does I/O.** Two patterns enforce this:
//!
//! * `bridge_connect` / `bridge_reconnect`: briefly lock to transition the
//!   connection *phase* (rejecting overlap), then drop the guard *before* the
//!   `connect_with_retry` / `subscribe` / `spawn relay` / `hello` / capability
//!   discovery work, then
//!   briefly lock again to store the result.
//! * Every request-issuing command: lock, clone the [`DispatchHandle`] (cheap —
//!   it is `Clone`), drop the guard, then `.await` the request on the clone.
//!
//! ## Relay teardown (plan M1)
//!
//! On reconnect/disconnect the OLD relay task must stop so events never
//! double-fire. We make double-emit impossible two ways at once:
//!
//! 1. `old_handle.shutdown().await` closes the old broadcast, so the old relay's
//!    `events.recv()` returns `Closed` and the task exits on its own; and
//! 2. we also hold the old relay's [`tokio::task::AbortHandle`] and `.abort()`
//!    it, then `await` its [`JoinHandle`] so the task has demonstrably ended
//!    before we spawn the replacement.
//!
//! The connect/reconnect double-relay behavior is exercised end-to-end against
//! the mock engine in P6; here it is structural plus the pure mapping tests.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use norves_bridge_core::{
    CapabilityDescriptor, CorrelationId, MethodName, ResponsePayload, ValidatedEnvelope,
    VersionString,
};
use norves_bridge_editor_client::{
    connect_with_retry, hello_error_to_handshake, parse_hello_result, DispatchHandle, HelloOutcome,
    HelloParams, RetryConfig,
};
use serde_json::Value;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex;

use crate::dto::ConnectionStatePayload;
use crate::error::BackendError;
use crate::events_map::ui_channel_for_event;
use crate::protocol_names::events;

/// Wire protocol version this editor stamps on every envelope it sends.
///
/// This is the envelope `version` field, fixed to the current protocol
/// generation. It does NOT follow the negotiated value: the editor always
/// stamps `PROTOCOL_VERSION`, and engines accept it because the wire decoder
/// does not validate the envelope version against a peer-supported set. Keep
/// `build_request` stamping this constant verbatim — do not make it track the
/// negotiated `HelloOutcome::protocol_version`.
const PROTOCOL_VERSION: &str = "0.2";
/// Protocol versions offered in `bridge.hello`, in preference order.
///
/// `["0.2", "0.1"]` negotiates 0.2 with a 0.2-capable engine and falls back to
/// 0.1 with a legacy 0.1-only engine. The negotiated result lives in
/// `HelloOutcome::protocol_version`; the envelope `version` stays
/// `PROTOCOL_VERSION` regardless (see above).
const OFFERED_PROTOCOL_VERSIONS: [&str; 2] = ["0.2", "0.1"];
/// Product name sent in `bridge.hello`.
const CLIENT_NAME: &str = "NorvesEditor";
/// Default per-request timeout for engine method calls.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Builds the loopback WebSocket URL for a local engine `port`.
///
/// The alpha only ever dials a local engine, so the host is fixed to
/// `127.0.0.1`; the command takes a `port: u16` and this builds the `ws://` URL.
fn ws_url_for_port(port: u16) -> String {
    format!("ws://127.0.0.1:{port}")
}

/// The live half of the connection: the dispatcher handle plus the running
/// relay task's stop levers and the session metadata captured during setup.
struct LiveConnection {
    /// Unique token for this connect attempt, taken from
    /// `BridgeState::next_generation`. The relay carries the same token and only
    /// resets the backend phase on an unsolicited close if it still owns the
    /// current attempt/session.
    generation: u64,
    handle: DispatchHandle,
    /// Join handle for the relay task; awaited (after abort / shutdown) to prove
    /// the old relay has ended before a replacement is spawned.
    relay: JoinHandle<()>,
    endpoint: String,
    session_id: String,
    server_name: String,
    capabilities: Vec<CapabilityDescriptor>,
}

/// Connection phase. `Connecting(token)` rejects overlap and prevents stale
/// success/failure paths from committing over a disconnected or newer attempt.
enum Phase {
    Disconnected,
    Connecting(u64),
    Connected(LiveConnection),
}

/// Tauri-managed backend state. Guarded by a `tokio::sync::Mutex`; the guard is
/// never held across an I/O `.await` (see module docs).
pub struct BridgeState {
    inner: Mutex<Phase>,
    /// Monotonic source of unique request correlation ids. Shared so every
    /// in-flight request across all commands gets a distinct id.
    next_request_id: AtomicU64,
    /// Monotonic source of unique connect-attempt tokens. Bumped when an attempt
    /// starts so setup, relay self-heal, and commit share one identity.
    next_generation: AtomicU64,
}

impl Default for BridgeState {
    fn default() -> Self {
        BridgeState {
            inner: Mutex::new(Phase::Disconnected),
            next_request_id: AtomicU64::new(0),
            next_generation: AtomicU64::new(0),
        }
    }
}

impl BridgeState {
    /// Allocates a unique correlation id for an in-flight request.
    fn alloc_request_id(&self) -> CorrelationId {
        let n = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        // CorrelationId only rejects empty strings; "req-{n}" is always
        // non-empty, so this is infallible.
        CorrelationId::try_from(format!("req-{n}")).expect("generated id is valid")
    }

    /// Allocates a unique token for a new connect attempt.
    fn alloc_generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::Relaxed)
    }
}

/// Builds a request-kind [`ValidatedEnvelope`] for `method` with `params`.
fn build_request(
    id: CorrelationId,
    method: &str,
    params: Option<serde_json::Map<String, Value>>,
) -> Result<ValidatedEnvelope, BackendError> {
    let version = VersionString::try_from(PROTOCOL_VERSION.to_owned()).map_err(|_| {
        BackendError::Request {
            message: "internal: invalid protocol version constant".to_owned(),
        }
    })?;
    let method = MethodName::try_from(method.to_owned()).map_err(|_| BackendError::Request {
        message: format!("internal: invalid method name {method}"),
    })?;
    Ok(ValidatedEnvelope::Request {
        version,
        id,
        method,
        params,
        session_id: None,
        seq: None,
    })
}

/// Builds the mandatory same-session capability discovery request.
fn build_capabilities_request(state: &BridgeState) -> Result<ValidatedEnvelope, BackendError> {
    build_request(
        state.alloc_request_id(),
        "bridge.getCapabilities",
        Some(serde_json::Map::new()),
    )
}

/// Strictly extracts the authoritative descriptor set from a capability result.
fn parse_connection_capabilities(value: &Value) -> Result<Vec<CapabilityDescriptor>, BackendError> {
    norves_bridge_editor_client::parse_capabilities_result(value)
        .map(|result| result.capabilities)
        .map_err(|err| BackendError::Request {
            message: format!("malformed bridge.getCapabilities result: {err}"),
        })
}

/// Discovers capabilities through the already-handshaken dispatcher handle.
async fn request_connection_capabilities(
    handle: &DispatchHandle,
    state: &BridgeState,
) -> Result<Vec<CapabilityDescriptor>, BackendError> {
    let request = build_capabilities_request(state)?;
    match handle.request(request, REQUEST_TIMEOUT).await {
        Ok(ResponsePayload::Result(value)) => parse_connection_capabilities(&value),
        Ok(ResponsePayload::Error(bridge_err)) => Err(bridge_err.into()),
        Err(req_err) => Err(req_err.into()),
    }
}

/// Builds the authoritative connected payload from the stored live session.
fn connection_state_payload(conn: &LiveConnection) -> ConnectionStatePayload {
    ConnectionStatePayload::connected(
        conn.session_id.clone(),
        conn.server_name.clone(),
        conn.endpoint.clone(),
        conn.capabilities.clone(),
    )
}

/// Resets only the connect attempt that still owns `token`.
fn reset_connecting_if_matches(phase: &mut Phase, token: u64) -> bool {
    let matches_token = matches!(&*phase, Phase::Connecting(current) if *current == token);
    if matches_token {
        *phase = Phase::Disconnected;
    }
    matches_token
}

/// Commits `conn` only while its originating connect token is still current.
fn try_commit_connection<F>(
    phase: &mut Phase,
    token: u64,
    conn: LiveConnection,
    on_commit: F,
) -> Option<LiveConnection>
where
    F: FnOnce(&Phase),
{
    if matches!(&*phase, Phase::Connecting(current) if *current == token) {
        *phase = Phase::Connected(conn);
        on_commit(phase);
        None
    } else {
        Some(conn)
    }
}

/// Pure decision for the relay's self-heal: should a relay whose connection had
/// `relay_generation` reset the backend `phase` to `Disconnected`?
///
/// Returns `true` only when the phase is the `Connecting` attempt or `Connected`
/// session that owns the relay's generation. A different generation means a
/// newer attempt already replaced this one, so the relay must leave it alone.
fn relay_should_reset_phase(phase: &Phase, relay_generation: u64) -> bool {
    matches!(phase, Phase::Connecting(token) if *token == relay_generation)
        || matches!(phase, Phase::Connected(conn) if conn.generation == relay_generation)
}

/// Transitions an owned attempt/session to disconnected and runs the synchronous
/// callback only after that transition. Callers keep the state mutex held while
/// this function executes, making transition + event publication one ordering
/// unit without introducing an await.
fn reset_owned_phase_and_then<F>(phase: &mut Phase, generation: u64, on_reset: F) -> bool
where
    F: FnOnce(&Phase),
{
    if relay_should_reset_phase(phase, generation) {
        *phase = Phase::Disconnected;
        on_reset(phase);
        true
    } else {
        false
    }
}

/// Spawns the backend->UI relay task and returns its join handle.
///
/// The task loops `events.recv().await`, maps each Bridge event NAME to a Tauri
/// channel, and emits the event `params` as a raw [`Value`] (already wire-shaped
/// — never re-modeled). On `Closed`, it publishes disconnected state only when
/// it still owns the current generation; it logs-and-continues on `Lagged` and
/// on unknown event names.
///
/// `generation` is this relay's attempt/session id. On an unsolicited `Closed`,
/// the relay self-heals and publishes `Disconnected` only if the current phase
/// still carries that generation, so it cannot clobber or misreport a newer
/// attempt/session. It reaches `BridgeState` via
/// the Tauri managed state (`app.state::<BridgeState>()`); no extra `Arc` is
/// threaded through because the state is already managed by this `AppHandle`.
fn spawn_relay(
    app: AppHandle,
    generation: u64,
    mut events: tokio::sync::broadcast::Receiver<Arc<ValidatedEnvelope>>,
) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(envelope) => {
                    if let ValidatedEnvelope::Event { event, params, .. } = &*envelope {
                        let name = event.as_str();
                        match ui_channel_for_event(name) {
                            Some(channel) => {
                                // Forward params as raw wire JSON; an absent
                                // params object emits `null`.
                                let payload = params
                                    .as_ref()
                                    .map(|map| Value::Object(map.clone()))
                                    .unwrap_or(Value::Null);
                                if let Err(err) = app.emit(channel, payload) {
                                    tracing::warn!(
                                        channel,
                                        error = %err,
                                        "relay: failed to emit event to UI"
                                    );
                                }
                            }
                            None => {
                                tracing::debug!(event = name, "relay: unknown event, skipping");
                            }
                        }
                    } else {
                        // The dispatcher only broadcasts Event variants; anything
                        // else is unexpected but non-fatal.
                        tracing::debug!("relay: non-event envelope on broadcast, skipping");
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "relay: broadcast lagged, continuing");
                }
                Err(RecvError::Closed) => {
                    tracing::debug!("relay: broadcast closed, checking generation and exiting");
                    let state = app.state::<BridgeState>();
                    let mut guard = state.inner.lock().await;
                    reset_owned_phase_and_then(&mut guard, generation, |_| {
                        // `emit` is synchronous in Tauri 2. Keep transition and
                        // publication in this one mutex interval so a connect
                        // commit cannot interleave between them.
                        let payload = ConnectionStatePayload::disconnected(Some(
                            "connection closed".to_owned(),
                        ));
                        if let Err(err) = app.emit(events::CONNECTION_STATE, payload) {
                            tracing::warn!(
                                error = %err,
                                "relay: failed to emit disconnect state"
                            );
                        }
                    });
                    drop(guard);
                    return;
                }
            }
        }
    })
}

/// Reliably stops an old [`LiveConnection`]'s relay and dispatcher.
///
/// Order (see module docs): abort the relay task, shut down the dispatcher
/// handle (closes the broadcast), then await the relay's join handle so the task
/// has demonstrably ended. Idempotent: a JoinError from the abort is expected
/// and ignored.
async fn tear_down(conn: LiveConnection) {
    conn.relay.abort();
    conn.handle.shutdown().await;
    // Await the aborted task so it cannot outlive this call and double-emit.
    let _ = conn.relay.await;
}

/// The shared connect flow used by `bridge_connect` (and, after teardown, by
/// `bridge_reconnect`).
///
/// Runs WITHOUT the state lock held: dial with retry, subscribe BEFORE hello,
/// spawn the relay, perform the handshake, then strictly discover capabilities
/// on the same dispatcher. On any failure it tears down whatever it built so
/// nothing leaks, and returns a [`BackendError`].
async fn run_connect_flow(
    app: AppHandle,
    state: &BridgeState,
    endpoint: String,
    generation: u64,
) -> Result<LiveConnection, BackendError> {
    // 1. Dial with retry -> DispatchHandle.
    let handle = connect_with_retry(&endpoint, &RetryConfig::default()).await?;

    // 2. Subscribe to events BEFORE hello so no early event is missed.
    let events = handle.subscribe_events();

    // 3. Spawn the relay BEFORE hello with the attempt token allocated by the
    //    caller, so setup, relay self-heal, and commit use one generation.
    let relay = spawn_relay(app, generation, events);

    let setup = complete_connection_setup(handle, relay, state).await?;

    Ok(LiveConnection {
        generation,
        handle: setup.handle,
        relay: setup.relay,
        endpoint,
        session_id: setup.hello.session_id,
        server_name: setup.hello.server_name,
        capabilities: setup.capabilities,
    })
}

/// Resources and authoritative metadata produced by connection setup.
///
/// The dispatcher handle and relay stay owned by this value on success so the
/// caller can assemble a [`LiveConnection`] without replacing either resource.
struct SetupOutcome {
    handle: DispatchHandle,
    relay: JoinHandle<()>,
    hello: HelloOutcome,
    capabilities: Vec<CapabilityDescriptor>,
}

/// Performs hello and same-handle capability discovery for a partial connection.
///
/// This helper owns both resources. Every error path shuts down the dispatcher,
/// aborts and awaits the relay, then returns the original setup error. It does
/// not know about Tauri application state or publish a connected phase.
async fn complete_connection_setup(
    handle: DispatchHandle,
    relay: JoinHandle<()>,
    state: &BridgeState,
) -> Result<SetupOutcome, BackendError> {
    let result: Result<(HelloOutcome, Vec<CapabilityDescriptor>), BackendError> = async {
        // Hello handshake.
        let mut protocol_versions = Vec::with_capacity(OFFERED_PROTOCOL_VERSIONS.len());
        for offered in OFFERED_PROTOCOL_VERSIONS {
            protocol_versions.push(VersionString::try_from(offered.to_owned()).map_err(|_| {
                BackendError::Handshake {
                    message: "internal: invalid protocol version constant".to_owned(),
                }
            })?);
        }
        let hello_params = HelloParams::new(CLIENT_NAME, protocol_versions);
        let params = hello_params.to_params()?;
        let request = build_request(state.alloc_request_id(), "bridge.hello", Some(params))?;

        let hello = match handle.request(request, REQUEST_TIMEOUT).await {
            Ok(ResponsePayload::Result(value)) => parse_hello_result(&value)?,
            Ok(ResponsePayload::Error(bridge_err)) => {
                return Err(hello_error_to_handshake(bridge_err).into())
            }
            Err(req_err) => return Err(req_err.into()),
        };

        // Capability discovery is a hard gate on this same dispatcher handle.
        let capabilities = request_connection_capabilities(&handle, state).await?;
        Ok((hello, capabilities))
    }
    .await;

    match result {
        Ok((hello, capabilities)) => Ok(SetupOutcome {
            handle,
            relay,
            hello,
            capabilities,
        }),
        Err(err) => {
            tear_down_partial(handle, relay).await;
            Err(err)
        }
    }
}

/// Tears down a half-built connection (handle + relay) when handshake or
/// capability discovery fails before a [`LiveConnection`] is assembled.
async fn tear_down_partial(handle: DispatchHandle, relay: JoinHandle<()>) {
    relay.abort();
    handle.shutdown().await;
    let _ = relay.await;
}

/// Reads a cloned [`DispatchHandle`] out of state without holding the lock
/// across the caller's subsequent request `.await`. Returns
/// [`BackendError::NotConnected`] if no live connection exists.
async fn handle_clone(state: &BridgeState) -> Result<DispatchHandle, BackendError> {
    let guard = state.inner.lock().await;
    match &*guard {
        Phase::Connected(conn) => Ok(conn.handle.clone()),
        _ => Err(BackendError::NotConnected),
    }
    // guard dropped here, before the caller awaits the request.
}

/// Sends `method` with `params`, returning the raw `result` Value on success.
///
/// Clones the handle out of state (lock dropped), then awaits the request on the
/// clone. Engine protocol errors become [`BackendError::Engine`]; transport /
/// timeout / encode failures become [`BackendError::Request`].
async fn send_method(
    state: &BridgeState,
    method: &str,
    params: Option<serde_json::Map<String, Value>>,
) -> Result<Value, BackendError> {
    let handle = handle_clone(state).await?;
    let request = build_request(state.alloc_request_id(), method, params)?;
    match handle.request(request, REQUEST_TIMEOUT).await {
        Ok(ResponsePayload::Result(value)) => Ok(value),
        Ok(ResponsePayload::Error(bridge_err)) => Err(bridge_err.into()),
        Err(req_err) => Err(req_err.into()),
    }
}

// ===========================================================================
// Tauri commands. Fn names MUST equal the P3 `protocol_names::commands` consts.
// ===========================================================================

/// Shared connect entrypoint used by BOTH `bridge_connect` and the process
/// runtime's `launch_engine` (plan J3).
///
/// This is exactly `bridge_connect`'s body, factored out so the process module
/// can establish a connection WITHOUT duplicating the phase-transition guard or
/// the `Phase` type (which stays private to this module). It:
///
/// 1. briefly locks to transition `Disconnected -> Connecting` (rejecting an
///    overlapping connect/launch with [`BackendError::AlreadyConnected`]), drops
///    the guard, then
/// 2. runs the full connect flow WITHOUT the lock held, then
/// 3. on success briefly locks to store the `Connected` phase and emits
///    `CONNECTION_STATE`; on failure resets the phase to `Disconnected`.
///
/// No lock is ever held across the connect `.await` (see module docs).
pub(crate) async fn connect_on_port(
    app: AppHandle,
    state: &BridgeState,
    port: u16,
) -> Result<ConnectionStatePayload, BackendError> {
    // Brief lock: transition Disconnected -> Connecting, reject overlap.
    let token = state.alloc_generation();
    {
        let mut guard = state.inner.lock().await;
        match &*guard {
            Phase::Disconnected => *guard = Phase::Connecting(token),
            Phase::Connecting(_) | Phase::Connected(_) => {
                return Err(BackendError::AlreadyConnected);
            }
        }
    } // guard dropped: connect I/O runs WITHOUT the lock.

    let endpoint = ws_url_for_port(port);
    let result = run_connect_flow(app.clone(), state, endpoint, token).await;

    match result {
        Ok(conn) => {
            let payload = connection_state_payload(&conn);
            let commit = {
                let mut guard = state.inner.lock().await;
                try_commit_connection(&mut guard, token, conn, |_| {
                    // Synchronous emit under the same lock interval as commit.
                    let _ = app.emit(events::CONNECTION_STATE, payload.clone());
                })
            };
            if let Some(conn) = commit {
                // Disconnect or a newer attempt invalidated this token while
                // setup was in flight. Tear down without publishing ready.
                tear_down(conn).await;
                return Err(BackendError::NotConnected);
            }
            Ok(payload)
        }
        Err(err) => {
            // Reset only this failed attempt; never clobber a newer token.
            let mut guard = state.inner.lock().await;
            reset_connecting_if_matches(&mut guard, token);
            Err(err)
        }
    }
}

/// `bridge_connect`: dial `port`, handshake, store the connection, emit state.
///
/// Rejects with [`BackendError::AlreadyConnected`] if already connected or a
/// connect is in progress (the transient `Connecting` phase). Delegates to the
/// shared [`connect_on_port`] so `launch_engine` reuses the identical logic.
#[tauri::command]
pub async fn bridge_connect(
    state: State<'_, BridgeState>,
    app: AppHandle,
    port: u16,
) -> Result<ConnectionStatePayload, BackendError> {
    connect_on_port(app, state.inner(), port).await
}

/// Tears down a live bridge connection WITHOUT emitting a connection-state event,
/// resetting the phase to `Disconnected`. Used by `stop_engine` (plan J3) for a
/// best-effort graceful WS close before the engine is killed; a no-op if nothing
/// is connected.
///
/// This does NOT emit, and CANNOT rely on the relay to emit: `tear_down` ABORTS
/// the relay task before its `Closed` self-heal path can run, so that path's
/// disconnected emit never fires. The CALLER is therefore responsible for
/// emitting the disconnected `CONNECTION_STATE` (`stop_engine` does so, as its
/// single source).
pub(crate) async fn disconnect_quietly(state: &BridgeState) {
    let taken = {
        let mut guard = state.inner.lock().await;
        match std::mem::replace(&mut *guard, Phase::Disconnected) {
            Phase::Connected(conn) => Some(conn),
            _ => None,
        }
    };
    if let Some(conn) = taken {
        tear_down(conn).await;
    }
}

/// `bridge_disconnect`: stop the relay, shut down the handle, clear state, emit
/// a disconnected connection-state. Idempotent: disconnecting while already
/// disconnected is a no-op success.
#[tauri::command]
pub async fn bridge_disconnect(
    state: State<'_, BridgeState>,
    app: AppHandle,
) -> Result<ConnectionStatePayload, BackendError> {
    // Take the live connection out under the lock, then tear down WITHOUT it.
    let taken = {
        let mut guard = state.inner.lock().await;
        match std::mem::replace(&mut *guard, Phase::Disconnected) {
            Phase::Connected(conn) => Some(conn),
            // Connecting or Disconnected: nothing live to tear down.
            _ => None,
        }
    };
    if let Some(conn) = taken {
        tear_down(conn).await;
    }
    let payload = ConnectionStatePayload::disconnected(Some("disconnected by editor".to_owned()));
    let _ = app.emit(events::CONNECTION_STATE, payload.clone());
    Ok(payload)
}

/// `bridge_reconnect`: reliably stop the old relay + handle, re-dial, re-spawn
/// the relay, re-handshake, store, emit. No double relay (see [`tear_down`]).
#[tauri::command]
pub async fn bridge_reconnect(
    state: State<'_, BridgeState>,
    app: AppHandle,
) -> Result<ConnectionStatePayload, BackendError> {
    // Take the old connection (and remember its endpoint) under the lock, then
    // move to Connecting so an overlapping connect/reconnect is rejected.
    let token = state.alloc_generation();
    let (old, endpoint) = {
        let mut guard = state.inner.lock().await;
        match std::mem::replace(&mut *guard, Phase::Connecting(token)) {
            Phase::Connected(conn) => {
                let endpoint = conn.endpoint.clone();
                (conn, endpoint)
            }
            Phase::Connecting(current) => {
                // A connect/reconnect is already in progress: do not disturb it.
                *guard = Phase::Connecting(current);
                return Err(BackendError::AlreadyConnected);
            }
            Phase::Disconnected => {
                *guard = Phase::Disconnected;
                return Err(BackendError::NotConnected);
            }
        }
    };

    // Tear down the OLD relay + handle WITHOUT the lock held.
    tear_down(old).await;

    // Re-run the full connect flow (subscribe -> spawn relay -> hello).
    let result = run_connect_flow(app.clone(), state.inner(), endpoint, token).await;
    match result {
        Ok(conn) => {
            let payload = connection_state_payload(&conn);
            let commit = {
                let mut guard = state.inner.lock().await;
                try_commit_connection(&mut guard, token, conn, |_| {
                    let _ = app.emit(events::CONNECTION_STATE, payload.clone());
                })
            };
            if let Some(conn) = commit {
                tear_down(conn).await;
                return Err(BackendError::NotConnected);
            }
            Ok(payload)
        }
        Err(err) => {
            let mut guard = state.inner.lock().await;
            reset_connecting_if_matches(&mut guard, token);
            Err(err)
        }
    }
}

/// `get_status`: `engine.getStatus` with no params. Returns the raw wire-shaped
/// `result` Value (UI types it as `GetStatusResult`). Validated with
/// `parse_status_result` so a malformed result surfaces as a clean backend
/// error rather than being forwarded.
#[tauri::command]
pub async fn get_status(state: State<'_, BridgeState>) -> Result<Value, BackendError> {
    let value = send_method(state.inner(), "engine.getStatus", None).await?;
    // Validate shape (drift guard) but forward the original wire Value so the UI
    // sees exactly what the engine sent.
    norves_bridge_editor_client::parse_status_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed engine.getStatus result: {err}"),
        }
    })?;
    Ok(value)
}

/// `scene_get_tree`: `scene.getTree` with an empty params object. Returns the
/// raw wire-shaped `result` Value (UI types it as `SceneGetTreeResult`).
///
/// Validated with `parse_scene_tree_result` so a malformed result surfaces as a
/// clean backend error rather than being forwarded; the ORIGINAL wire Value is
/// still returned so the UI sees exactly what the engine sent (same
/// validate-then-forward pattern as `get_status`). An engine that does not
/// implement scene query answers with a protocol error, which `send_method`
/// maps to [`BackendError::Engine`] (e.g. `METHOD_NOT_SUPPORTED`) for the UI to
/// degrade on.
#[tauri::command]
pub async fn scene_get_tree(state: State<'_, BridgeState>) -> Result<Value, BackendError> {
    let value = send_method(state.inner(), "scene.getTree", Some(serde_json::Map::new())).await?;
    // Validate shape (drift guard) but forward the original wire Value.
    norves_bridge_editor_client::parse_scene_tree_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed scene.getTree result: {err}"),
        }
    })?;
    Ok(value)
}

/// `scene_create_object`: `scene.createObject` with optional `parentId` / `kind`.
/// Returns the raw wire-shaped `result` Value (UI types it as
/// `SceneCreateObjectResult`).
#[tauri::command]
pub async fn scene_create_object(
    state: State<'_, BridgeState>,
    parent_id: Option<String>,
    kind: Option<String>,
) -> Result<Value, BackendError> {
    let mut params = serde_json::Map::new();
    if let Some(parent_id) = parent_id {
        params.insert("parentId".to_owned(), Value::String(parent_id));
    }
    if let Some(kind) = kind {
        params.insert("kind".to_owned(), Value::String(kind));
    }

    let value = send_method(state.inner(), "scene.createObject", Some(params)).await?;
    norves_bridge_editor_client::parse_create_object_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed scene.createObject result: {err}"),
        }
    })?;
    Ok(value)
}

/// `scene_delete_object`: `scene.deleteObject` for `object_id`.
#[tauri::command]
pub async fn scene_delete_object(
    state: State<'_, BridgeState>,
    object_id: String,
) -> Result<Value, BackendError> {
    let mut params = serde_json::Map::new();
    params.insert("objectId".to_owned(), Value::String(object_id));

    let value = send_method(state.inner(), "scene.deleteObject", Some(params)).await?;
    norves_bridge_editor_client::parse_delete_object_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed scene.deleteObject result: {err}"),
        }
    })?;
    Ok(value)
}

/// `scene_reparent_object`: `scene.reparentObject` for `object_id` and optional
/// `new_parent_id`. Omitting `new_parent_id` moves the object to the scene root.
#[tauri::command]
pub async fn scene_reparent_object(
    state: State<'_, BridgeState>,
    object_id: String,
    new_parent_id: Option<String>,
) -> Result<Value, BackendError> {
    let mut params = serde_json::Map::new();
    params.insert("objectId".to_owned(), Value::String(object_id));
    if let Some(new_parent_id) = new_parent_id {
        params.insert("newParentId".to_owned(), Value::String(new_parent_id));
    }

    let value = send_method(state.inner(), "scene.reparentObject", Some(params)).await?;
    norves_bridge_editor_client::parse_reparent_object_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed scene.reparentObject result: {err}"),
        }
    })?;
    Ok(value)
}

/// `scene_duplicate_object`: `scene.duplicateObject` for `object_id` and optional
/// `new_parent_id`. Omitting `new_parent_id` places the copy alongside the
/// original. Returns the raw wire-shaped `result` Value (UI types it as
/// `SceneDuplicateObjectResult`).
#[tauri::command]
pub async fn scene_duplicate_object(
    state: State<'_, BridgeState>,
    object_id: String,
    new_parent_id: Option<String>,
) -> Result<Value, BackendError> {
    let mut params = serde_json::Map::new();
    params.insert("objectId".to_owned(), Value::String(object_id));
    if let Some(new_parent_id) = new_parent_id {
        params.insert("newParentId".to_owned(), Value::String(new_parent_id));
    }

    let value = send_method(state.inner(), "scene.duplicateObject", Some(params)).await?;
    norves_bridge_editor_client::parse_duplicate_object_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed scene.duplicateObject result: {err}"),
        }
    })?;
    Ok(value)
}

/// `object_get_snapshot`: `object.getSnapshot` for `object_id`. Returns the raw
/// wire-shaped `result` Value (UI types it as `ObjectSnapshot`).
///
/// Sends `params = { objectId: object_id }` (the schema requires `objectId`).
/// Validated with `parse_object_snapshot_result` so a malformed result surfaces
/// as a clean backend error rather than being forwarded; the ORIGINAL wire Value
/// is still returned (same validate-then-forward pattern as `get_status` /
/// `scene_get_tree`). An engine that does not implement object query answers with
/// a protocol error, which `send_method` maps to [`BackendError::Engine`] (e.g.
/// `METHOD_NOT_SUPPORTED`) for the UI to degrade on.
#[tauri::command]
pub async fn object_get_snapshot(
    state: State<'_, BridgeState>,
    object_id: String,
) -> Result<Value, BackendError> {
    let mut params = serde_json::Map::new();
    params.insert("objectId".to_owned(), Value::String(object_id));
    let value = send_method(state.inner(), "object.getSnapshot", Some(params)).await?;
    // Validate shape (drift guard) but forward the original wire Value.
    norves_bridge_editor_client::parse_object_snapshot_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed object.getSnapshot result: {err}"),
        }
    })?;
    Ok(value)
}

/// `object_set_property`: `object.setProperty` for `object_id` / `property` /
/// `value`. Returns the raw wire-shaped `result` Value (UI types it as
/// `SetObjectPropertyResult`).
///
/// Sends `params = { objectId, property, value }` (all required by the schema;
/// `value` is forwarded verbatim as arbitrary JSON — string/number/boolean/null/
/// array/object — so a structured edit reaches the engine unchanged). Validated
/// with `parse_set_property_result` so a malformed ack surfaces as a clean
/// backend error rather than being forwarded; the ORIGINAL wire Value (carrying
/// the engine's `appliedValue`) is still returned (same validate-then-forward
/// pattern as the read commands). An engine that does not implement object edit
/// answers with a protocol error, which `send_method` maps to
/// [`BackendError::Engine`] (e.g. `METHOD_NOT_SUPPORTED`) for the UI to degrade
/// on.
///
/// This is the only WRITE path among the commands; it carries no extra state
/// (no lock held across the request `.await` — `send_method` clones the handle
/// out of state and drops the guard before awaiting, see module docs).
#[tauri::command]
pub async fn object_set_property(
    state: State<'_, BridgeState>,
    object_id: String,
    property: String,
    value: Value,
) -> Result<Value, BackendError> {
    let mut params = serde_json::Map::new();
    params.insert("objectId".to_owned(), Value::String(object_id));
    params.insert("property".to_owned(), Value::String(property));
    // `value` is forwarded verbatim: a snapshot copy of the edited value, never a
    // live engine pointer. The engine echoes (or normalizes) it back as
    // `appliedValue`.
    params.insert("value".to_owned(), value);
    let result = send_method(state.inner(), "object.setProperty", Some(params)).await?;
    // Validate shape (drift guard) but forward the original wire Value so the UI
    // sees the engine's actual appliedValue.
    norves_bridge_editor_client::parse_set_property_result(&result).map_err(|err| {
        BackendError::Request {
            message: format!("malformed object.setProperty result: {err}"),
        }
    })?;
    Ok(result)
}

/// `schema_get_snapshot`: `schema.getSnapshot` with an empty params object.
/// Returns the raw wire-shaped `result` Value (UI types it as `SchemaSnapshot`).
///
/// Validated with `parse_schema_snapshot_result` so a malformed result surfaces
/// as a clean backend error rather than being forwarded; the ORIGINAL wire Value
/// is still returned (same validate-then-forward pattern as `get_status` /
/// `scene_get_tree`). An engine without schema query answers with a protocol
/// error, which `send_method` maps to [`BackendError::Engine`] (e.g.
/// `METHOD_NOT_SUPPORTED`) for the UI to degrade on.
#[tauri::command]
pub async fn schema_get_snapshot(state: State<'_, BridgeState>) -> Result<Value, BackendError> {
    let value = send_method(
        state.inner(),
        "schema.getSnapshot",
        Some(serde_json::Map::new()),
    )
    .await?;
    // Validate shape (drift guard) but forward the original wire Value.
    norves_bridge_editor_client::parse_schema_snapshot_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed schema.getSnapshot result: {err}"),
        }
    })?;
    Ok(value)
}

/// `viewport_get_thumbnail`: `viewport.getThumbnail` with optional `maxWidth` /
/// `maxHeight`. Returns the raw wire-shaped `result` Value (UI types it as
/// `ViewportThumbnail`).
///
/// Sends `params = { maxWidth?, maxHeight? }` (both optional; omitted keys let the
/// engine pick). Validated with `parse_thumbnail_result` so a malformed result
/// surfaces as a clean backend error rather than being forwarded; the ORIGINAL
/// wire Value (carrying the engine's base64 image) is still returned (same
/// validate-then-forward pattern as the read commands). The image is a snapshot
/// copy carried inline as base64, never a live engine pointer (see
/// docs/memory-buffer-policy.md large-payload strategy: PNG, max 640x360, 256 KiB
/// hard cap, pull-style, <= 1 fps). An engine that does not provide thumbnails
/// answers with a protocol error, which `send_method` maps to
/// [`BackendError::Engine`] (e.g. `METHOD_NOT_SUPPORTED`) for the UI to degrade
/// on (it falls back to the external-window notice).
///
/// No lock is held across the request `.await` — `send_method` clones the handle
/// out of state and drops the guard before awaiting (see module docs).
#[tauri::command]
pub async fn viewport_get_thumbnail(
    state: State<'_, BridgeState>,
    max_width: Option<u32>,
    max_height: Option<u32>,
) -> Result<Value, BackendError> {
    let mut params = serde_json::Map::new();
    if let Some(w) = max_width {
        params.insert("maxWidth".to_owned(), Value::from(w));
    }
    if let Some(h) = max_height {
        params.insert("maxHeight".to_owned(), Value::from(h));
    }
    let value = send_method(state.inner(), "viewport.getThumbnail", Some(params)).await?;
    // Validate shape (drift guard) but forward the original wire Value so the UI
    // sees exactly the engine's base64 image and mimeType.
    norves_bridge_editor_client::parse_thumbnail_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed viewport.getThumbnail result: {err}"),
        }
    })?;
    Ok(value)
}

/// `asset_resolve`: `asset.resolve` for `logical_path` plus optional
/// `kind`/`variant` hints. Returns the raw wire-shaped `result` Value (UI types
/// it as `AssetResolveResult`).
///
/// Validated with `parse_asset_resolve_result` so a malformed result surfaces
/// as a clean backend error rather than being forwarded; the ORIGINAL wire
/// Value is still returned. The result is resolution metadata only — asset bytes
/// and live engine memory never cross this transport boundary.
#[tauri::command]
pub async fn asset_resolve(
    state: State<'_, BridgeState>,
    logical_path: String,
    kind: Option<String>,
    variant: Option<String>,
) -> Result<Value, BackendError> {
    let mut params = serde_json::Map::new();
    params.insert("logicalPath".to_owned(), Value::String(logical_path));
    if let Some(kind) = kind {
        params.insert("kind".to_owned(), Value::String(kind));
    }
    if let Some(variant) = variant {
        params.insert("variant".to_owned(), Value::String(variant));
    }

    let value = send_method(state.inner(), "asset.resolve", Some(params)).await?;
    // Validate shape (drift guard) but forward the original wire Value.
    norves_bridge_editor_client::parse_asset_resolve_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed asset.resolve result: {err}"),
        }
    })?;
    Ok(value)
}

/// `asset_get_manifest`: `asset.getManifest` with optional filter/page/pageSize.
/// Returns the raw wire-shaped `result` Value (UI types it as
/// `AssetManifestResult`).
///
/// Validated with `parse_asset_manifest_result` so a malformed result surfaces
/// as a clean backend error rather than being forwarded; the ORIGINAL wire
/// Value is still returned. Entries are manifest DTO snapshots, never live
/// engine storage.
#[tauri::command]
pub async fn asset_get_manifest(
    state: State<'_, BridgeState>,
    filter: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
) -> Result<Value, BackendError> {
    let mut params = serde_json::Map::new();
    if let Some(filter) = filter {
        params.insert("filter".to_owned(), Value::String(filter));
    }
    if let Some(page) = page {
        params.insert("page".to_owned(), Value::from(page));
    }
    if let Some(page_size) = page_size {
        params.insert("pageSize".to_owned(), Value::from(page_size));
    }

    let value = send_method(state.inner(), "asset.getManifest", Some(params)).await?;
    // Validate shape (drift guard) but forward the original wire Value.
    norves_bridge_editor_client::parse_asset_manifest_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed asset.getManifest result: {err}"),
        }
    })?;
    Ok(value)
}

/// Returns the exact wire method and required empty params for manifest reload.
fn asset_reload_manifest_request_parts() -> (&'static str, serde_json::Map<String, Value>) {
    ("asset.reloadManifest", serde_json::Map::new())
}

/// Strictly validates the acknowledgement while preserving the original wire
/// value for the frontend.
fn validate_asset_reload_manifest_result(value: Value) -> Result<Value, BackendError> {
    norves_bridge_editor_client::parse_asset_reload_manifest_result(&value).map_err(|err| {
        BackendError::Request {
            message: format!("malformed asset.reloadManifest result: {err}"),
        }
    })?;
    Ok(value)
}

/// `asset_reload_manifest`: asks the live engine to atomically reload its
/// manifest. Strictly validates the acknowledgement, then returns the original
/// wire-shaped result value unchanged.
#[tauri::command]
pub async fn asset_reload_manifest(state: State<'_, BridgeState>) -> Result<Value, BackendError> {
    let (method, params) = asset_reload_manifest_request_parts();
    let value = send_method(state.inner(), method, Some(params)).await?;
    validate_asset_reload_manifest_result(value)
}

/// `runtime_play`: `runtime.play` with an empty params object. Returns the raw
/// result Value.
#[tauri::command]
pub async fn runtime_play(state: State<'_, BridgeState>) -> Result<Value, BackendError> {
    send_method(state.inner(), "runtime.play", Some(serde_json::Map::new())).await
}

/// `runtime_pause`: `runtime.pause` with an empty params object.
#[tauri::command]
pub async fn runtime_pause(state: State<'_, BridgeState>) -> Result<Value, BackendError> {
    send_method(state.inner(), "runtime.pause", Some(serde_json::Map::new())).await
}

/// `runtime_stop`: `runtime.stop` with an empty params object.
#[tauri::command]
pub async fn runtime_stop(state: State<'_, BridgeState>) -> Result<Value, BackendError> {
    send_method(state.inner(), "runtime.stop", Some(serde_json::Map::new())).await
}

/// `focus_viewport`: `runtime.focusViewport` with an empty params object.
///
/// ENGINE-RUNTIME ONLY: this asks the engine to focus its own game window. It
/// deliberately does NOT touch the OS / Tauri window focus — native window
/// management is Workstream J5, out of scope for Workstream I.
#[tauri::command]
pub async fn focus_viewport(state: State<'_, BridgeState>) -> Result<Value, BackendError> {
    send_method(
        state.inner(),
        "runtime.focusViewport",
        Some(serde_json::Map::new()),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use norves_bridge_core::{decode_typed, encode_envelope, BridgeError, Envelope, ErrorCode};
    use norves_bridge_editor_client::{loopback_pair, Dispatcher, LoopbackTransport, Transport};
    use tokio::sync::oneshot;

    const CONNECTION_SETUP_TIMEOUT: Duration = Duration::from_secs(2);

    struct RelayDropProbe(Option<oneshot::Sender<()>>);

    impl Drop for RelayDropProbe {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    fn connection_setup_relay_probe() -> (JoinHandle<()>, oneshot::Receiver<()>) {
        let (sender, receiver) = oneshot::channel();
        let probe = RelayDropProbe(Some(sender));
        let relay = tauri::async_runtime::spawn(async move {
            let _probe = probe;
            std::future::pending::<()>().await;
        });
        (relay, receiver)
    }

    fn connection_setup_response_frame(id: CorrelationId, payload: ResponsePayload) -> String {
        let envelope: Envelope = ValidatedEnvelope::Response {
            version: VersionString::try_from(PROTOCOL_VERSION.to_owned())
                .expect("protocol version is valid"),
            id,
            payload,
            session_id: None,
            seq: None,
        }
        .into();
        encode_envelope(&envelope).expect("response envelope encodes")
    }

    async fn connection_setup_request(
        engine: &mut LoopbackTransport,
    ) -> (
        CorrelationId,
        String,
        Option<serde_json::Map<String, Value>>,
    ) {
        let frame = tokio::time::timeout(CONNECTION_SETUP_TIMEOUT, engine.recv())
            .await
            .expect("timed out waiting for setup request")
            .expect("engine transport receive succeeds")
            .expect("dispatcher sent a setup request");
        match decode_typed(&frame).expect("setup request decodes") {
            ValidatedEnvelope::Request {
                id, method, params, ..
            } => (id, method.as_str().to_owned(), params),
            other => panic!("expected setup request, got {other:?}"),
        }
    }

    async fn connection_setup_serve_hello(engine: &mut LoopbackTransport) {
        let (id, method, params) = connection_setup_request(engine).await;
        assert_eq!(method, "bridge.hello");
        assert!(params.is_some());
        engine
            .send(connection_setup_response_frame(
                id,
                ResponsePayload::Result(serde_json::json!({
                    "sessionId": "setup-session",
                    "protocolVersion": "0.2",
                    "server": {
                        "name": "LoopbackEngine",
                        "version": "0.1.0",
                        "engine": "loopback"
                    }
                })),
            ))
            .await
            .expect("hello response sends");
    }

    async fn connection_setup_capabilities_request(
        engine: &mut LoopbackTransport,
    ) -> CorrelationId {
        let (id, method, params) = connection_setup_request(engine).await;
        assert_eq!(method, "bridge.getCapabilities");
        assert!(matches!(params, Some(params) if params.is_empty()));
        id
    }

    async fn connection_setup_observe_peer_closed(engine: &mut LoopbackTransport) {
        let closed = tokio::time::timeout(CONNECTION_SETUP_TIMEOUT, engine.recv())
            .await
            .expect("timed out waiting for dispatcher transport close")
            .expect("engine transport receive succeeds");
        assert!(closed.is_none(), "dispatcher transport remained open");
    }

    async fn connection_setup_assert_failure_cleanup(
        state: &BridgeState,
        mut relay_dropped: oneshot::Receiver<()>,
        engine: JoinHandle<()>,
    ) {
        relay_dropped
            .try_recv()
            .expect("setup returned before the relay future was dropped");
        tokio::time::timeout(CONNECTION_SETUP_TIMEOUT, engine)
            .await
            .expect("timed out waiting for loopback engine")
            .expect("loopback engine task joins");
        assert!(matches!(*state.inner.lock().await, Phase::Disconnected));
    }

    #[test]
    fn ws_url_uses_loopback_host_and_port() {
        assert_eq!(ws_url_for_port(8123), "ws://127.0.0.1:8123");
        assert_eq!(ws_url_for_port(0), "ws://127.0.0.1:0");
        assert_eq!(ws_url_for_port(65535), "ws://127.0.0.1:65535");
    }

    /// Pure mapping check exercised here too: constructing an Event envelope and
    /// running the relay's channel-selection logic (`ui_channel_for_event`)
    /// yields the right channel. A full task test needs a runtime + live engine
    /// and is deferred to P6 (which also covers reconnect double-relay).
    #[test]
    fn event_envelope_maps_to_expected_channel() {
        use norves_bridge_core::EventName;
        let env = ValidatedEnvelope::Event {
            version: VersionString::try_from("0.1".to_owned()).expect("valid version"),
            event: EventName::try_from("log.message".to_owned()).expect("valid event"),
            params: None,
            session_id: None,
            seq: Some(1),
        };
        let name = match &env {
            ValidatedEnvelope::Event { event, .. } => event.as_str(),
            _ => unreachable!("constructed an Event"),
        };
        assert_eq!(ui_channel_for_event(name), Some(events::LOG_MESSAGE));
    }

    #[test]
    fn build_request_produces_request_envelope() {
        let id = CorrelationId::try_from("req-7".to_owned()).expect("valid id");
        let env = build_request(id, "engine.getStatus", None).expect("builds");
        match env {
            ValidatedEnvelope::Request { method, .. } => {
                assert_eq!(method.as_str(), "engine.getStatus");
            }
            _ => panic!("expected a request envelope"),
        }
    }

    #[test]
    fn connection_capabilities_request_uses_empty_params() {
        let state = BridgeState::default();
        let env = build_capabilities_request(&state).expect("builds");
        match env {
            ValidatedEnvelope::Request {
                method,
                params: Some(params),
                ..
            } => {
                assert_eq!(method.as_str(), "bridge.getCapabilities");
                assert!(params.is_empty());
            }
            _ => panic!("expected a request envelope with empty params"),
        }
    }

    #[test]
    fn connection_capabilities_strict_result_is_propagated() {
        let value = serde_json::json!({
            "capabilities": [{ "name": "asset.reload", "version": "0.2" }]
        });
        let capabilities = parse_connection_capabilities(&value).expect("parses");
        assert_eq!(capabilities.len(), 1);
        assert_eq!(capabilities[0].name.as_str(), "asset.reload");
    }

    #[test]
    fn connection_capabilities_malformed_result_is_rejected() {
        let value = serde_json::json!({
            "capabilities": [{ "name": "asset.reload", "extra": true }]
        });
        assert!(parse_connection_capabilities(&value).is_err());
    }

    #[tokio::test]
    async fn connection_setup_real_dispatcher_completes_hello_then_capabilities() {
        let state = BridgeState::default();
        let (client, mut engine_transport) = loopback_pair(16);
        let handle = Dispatcher::spawn(client);
        let (relay, mut relay_dropped) = connection_setup_relay_probe();

        let engine = tauri::async_runtime::spawn(async move {
            connection_setup_serve_hello(&mut engine_transport).await;
            let id = connection_setup_capabilities_request(&mut engine_transport).await;
            engine_transport
                .send(connection_setup_response_frame(
                    id,
                    ResponsePayload::Result(serde_json::json!({
                        "capabilities": [
                            { "name": "asset.reload", "version": "0.2" },
                            { "name": "scene.read", "version": "0.1" }
                        ]
                    })),
                ))
                .await
                .expect("capability response sends");
            connection_setup_observe_peer_closed(&mut engine_transport).await;
        });

        let setup: SetupOutcome = tokio::time::timeout(
            CONNECTION_SETUP_TIMEOUT,
            complete_connection_setup(handle, relay, &state),
        )
        .await
        .expect("timed out waiting for connection setup")
        .expect("connection setup succeeds");
        assert_eq!(setup.hello.session_id, "setup-session");
        assert_eq!(setup.hello.server_name, "LoopbackEngine");
        assert_eq!(setup.capabilities.len(), 2);
        assert_eq!(setup.capabilities[0].name.as_str(), "asset.reload");

        tear_down_partial(setup.handle, setup.relay).await;
        relay_dropped
            .try_recv()
            .expect("teardown returned before the relay future was dropped");
        tokio::time::timeout(CONNECTION_SETUP_TIMEOUT, engine)
            .await
            .expect("timed out waiting for loopback engine")
            .expect("loopback engine task joins");
    }

    #[tokio::test]
    async fn connection_setup_malformed_capabilities_tears_down_partial_connection() {
        let state = BridgeState::default();
        let (client, mut engine_transport) = loopback_pair(16);
        let handle = Dispatcher::spawn(client);
        let (relay, relay_dropped) = connection_setup_relay_probe();
        let engine = tauri::async_runtime::spawn(async move {
            connection_setup_serve_hello(&mut engine_transport).await;
            let id = connection_setup_capabilities_request(&mut engine_transport).await;
            engine_transport
                .send(connection_setup_response_frame(
                    id,
                    ResponsePayload::Result(serde_json::json!({
                        "capabilities": [
                            { "name": "asset.reload", "version": "0.2", "extra": true }
                        ]
                    })),
                ))
                .await
                .expect("malformed capability response sends");
            connection_setup_observe_peer_closed(&mut engine_transport).await;
        });

        let result = tokio::time::timeout(
            CONNECTION_SETUP_TIMEOUT,
            complete_connection_setup(handle, relay, &state),
        )
        .await
        .expect("timed out waiting for malformed capability rejection");
        assert!(matches!(result, Err(BackendError::Request { .. })));
        connection_setup_assert_failure_cleanup(&state, relay_dropped, engine).await;
    }

    #[tokio::test]
    async fn connection_setup_engine_error_tears_down_partial_connection() {
        let state = BridgeState::default();
        let (client, mut engine_transport) = loopback_pair(16);
        let handle = Dispatcher::spawn(client);
        let (relay, relay_dropped) = connection_setup_relay_probe();
        let engine = tauri::async_runtime::spawn(async move {
            let (id, method, _) = connection_setup_request(&mut engine_transport).await;
            assert_eq!(method, "bridge.hello");
            engine_transport
                .send(connection_setup_response_frame(
                    id,
                    ResponsePayload::Error(BridgeError {
                        code: ErrorCode::protocol_version_unsupported(),
                        message: "no common protocol version".to_owned(),
                        data: None,
                    }),
                ))
                .await
                .expect("engine error response sends");
            connection_setup_observe_peer_closed(&mut engine_transport).await;
        });

        let result = tokio::time::timeout(
            CONNECTION_SETUP_TIMEOUT,
            complete_connection_setup(handle, relay, &state),
        )
        .await
        .expect("timed out waiting for engine error rejection");
        assert!(matches!(result, Err(BackendError::Handshake { .. })));
        connection_setup_assert_failure_cleanup(&state, relay_dropped, engine).await;
    }

    #[tokio::test]
    async fn connection_setup_peer_drop_during_capability_request_tears_down_relay() {
        let state = BridgeState::default();
        let (client, mut engine_transport) = loopback_pair(16);
        let handle = Dispatcher::spawn(client);
        let (relay, relay_dropped) = connection_setup_relay_probe();
        let engine = tauri::async_runtime::spawn(async move {
            connection_setup_serve_hello(&mut engine_transport).await;
            let _ = connection_setup_capabilities_request(&mut engine_transport).await;
            drop(engine_transport);
        });

        let result = tokio::time::timeout(
            CONNECTION_SETUP_TIMEOUT,
            complete_connection_setup(handle, relay, &state),
        )
        .await
        .expect("timed out waiting for transport failure");
        assert!(matches!(result, Err(BackendError::Request { .. })));
        connection_setup_assert_failure_cleanup(&state, relay_dropped, engine).await;
    }

    /// A relay must ignore disconnected state and a newer connecting token.
    #[test]
    fn connection_generation_relay_ignores_disconnected_and_stale_connecting() {
        assert!(!relay_should_reset_phase(&Phase::Disconnected, 0));
        assert!(!relay_should_reset_phase(&Phase::Connecting(1), 0));
    }

    /// Builds a real `LiveConnection` (via a loopback-backed dispatcher and a
    /// trivial relay task) carrying `generation`. Needs a runtime, hence the
    /// `#[tokio::test]` caller.
    fn live_connection_with_generation(generation: u64) -> LiveConnection {
        let (transport, _peer) = norves_bridge_editor_client::loopback_pair(4);
        let handle = norves_bridge_editor_client::Dispatcher::spawn(transport);
        // A do-nothing relay handle: the guard logic only reads `generation`.
        let relay = tauri::async_runtime::spawn(async {});
        LiveConnection {
            generation,
            handle,
            relay,
            endpoint: "ws://127.0.0.1:0".to_owned(),
            session_id: "session".to_owned(),
            server_name: "server".to_owned(),
            capabilities: vec![serde_json::from_value(serde_json::json!({
                "name": "asset.reload",
                "version": "0.2"
            }))
            .expect("valid capability descriptor")],
        }
    }

    #[tokio::test]
    async fn connection_capabilities_live_connection_propagates_to_payload() {
        let conn = live_connection_with_generation(7);
        let payload = connection_state_payload(&conn);
        let value = serde_json::to_value(payload).expect("serializes");
        assert_eq!(
            value["capabilities"],
            serde_json::json!([{ "name": "asset.reload", "version": "0.2" }])
        );
        tear_down(conn).await;
    }

    #[test]
    fn connection_generation_stale_failure_cannot_reset_newer_connecting() {
        let mut phase = Phase::Connecting(2);
        assert!(!reset_connecting_if_matches(&mut phase, 1));
        assert!(matches!(phase, Phase::Connecting(2)));
    }

    #[test]
    fn connection_generation_matching_failure_resets_connecting() {
        let mut phase = Phase::Connecting(3);
        assert!(reset_connecting_if_matches(&mut phase, 3));
        assert!(matches!(phase, Phase::Disconnected));
    }

    #[tokio::test]
    async fn connection_generation_stale_success_cannot_commit_after_disconnect() {
        let mut phase = Phase::Disconnected;
        let conn = live_connection_with_generation(4);
        let rejected = match try_commit_connection(&mut phase, 4, conn, |_| {}) {
            None => panic!("stale connection committed after disconnect"),
            Some(conn) => conn,
        };
        assert!(matches!(phase, Phase::Disconnected));
        tear_down(rejected).await;
    }

    #[tokio::test]
    async fn connection_generation_stale_success_cannot_commit_over_newer_connecting() {
        let mut phase = Phase::Connecting(6);
        let conn = live_connection_with_generation(5);
        let rejected = match try_commit_connection(&mut phase, 5, conn, |_| {}) {
            None => panic!("stale connection replaced a newer attempt"),
            Some(conn) => conn,
        };
        assert!(matches!(phase, Phase::Connecting(6)));
        tear_down(rejected).await;
    }

    #[tokio::test]
    async fn connection_generation_matching_success_commits() {
        let mut phase = Phase::Connecting(7);
        let conn = live_connection_with_generation(7);
        assert!(try_commit_connection(&mut phase, 7, conn, |_| {}).is_none());
        let committed = match std::mem::replace(&mut phase, Phase::Disconnected) {
            Phase::Connected(conn) => conn,
            _ => panic!("matching connection was not committed"),
        };
        tear_down(committed).await;
    }

    #[tokio::test]
    async fn connection_generation_relay_resets_only_matching_attempt_or_connection() {
        assert!(relay_should_reset_phase(&Phase::Connecting(8), 8));
        assert!(!relay_should_reset_phase(&Phase::Connecting(9), 8));

        let connected = Phase::Connected(live_connection_with_generation(8));
        assert!(relay_should_reset_phase(&connected, 8));
        assert!(!relay_should_reset_phase(&connected, 9));
        if let Phase::Connected(conn) = connected {
            tear_down(conn).await;
        }
    }

    #[tokio::test]
    async fn connection_generation_commit_callback_runs_only_after_matching_transition() {
        use std::cell::Cell;

        let matching_callback = Cell::new(false);
        let mut matching = Phase::Connecting(10);
        let conn = live_connection_with_generation(10);
        let rejected = try_commit_connection(&mut matching, 10, conn, |phase| {
            matching_callback.set(matches!(phase, Phase::Connected(conn) if conn.generation == 10));
        });
        assert!(rejected.is_none());
        assert!(matching_callback.get());
        let committed = match std::mem::replace(&mut matching, Phase::Disconnected) {
            Phase::Connected(conn) => conn,
            _ => panic!("matching connection was not committed"),
        };
        tear_down(committed).await;

        let stale_callback_count = Cell::new(0);
        let mut newer = Phase::Connecting(12);
        let stale = live_connection_with_generation(11);
        let rejected = try_commit_connection(&mut newer, 11, stale, |_| {
            stale_callback_count.set(stale_callback_count.get() + 1);
        })
        .expect("stale connection is returned for teardown");
        assert_eq!(stale_callback_count.get(), 0);
        assert!(matches!(newer, Phase::Connecting(12)));
        tear_down(rejected).await;
    }

    #[test]
    fn connection_generation_relay_callback_runs_only_after_matching_reset() {
        use std::cell::Cell;

        let callback_count = Cell::new(0);
        let mut phase = Phase::Connecting(14);
        assert!(!reset_owned_phase_and_then(&mut phase, 13, |_| {
            callback_count.set(callback_count.get() + 1);
        }));
        assert_eq!(callback_count.get(), 0);
        assert!(matches!(phase, Phase::Connecting(14)));

        assert!(reset_owned_phase_and_then(&mut phase, 14, |phase| {
            assert!(matches!(phase, Phase::Disconnected));
            callback_count.set(callback_count.get() + 1);
        }));
        assert_eq!(callback_count.get(), 1);
        assert!(matches!(phase, Phase::Disconnected));
    }

    /// `send_method` (and thus `scene_get_tree`) must fail with `NotConnected`
    /// when no live connection exists, never panicking or hanging. This is the
    /// disconnected-path drift guard for the new command; the connected
    /// round-trip is covered by the conformance / process e2e suites against the
    /// real mock engine.
    #[tokio::test]
    async fn send_method_when_disconnected_is_not_connected() {
        let state = BridgeState::default();
        let result = send_method(&state, "scene.getTree", Some(serde_json::Map::new())).await;
        assert!(matches!(result, Err(BackendError::NotConnected)));
    }

    /// The object/schema snapshot commands (via `send_method`) must also fail
    /// with `NotConnected` when no live connection exists, never panicking or
    /// hanging. This is the disconnected-path drift guard for the new commands;
    /// the connected round-trip is covered by the conformance / process e2e
    /// suites against the real mock engine.
    #[tokio::test]
    async fn object_and_schema_snapshot_when_disconnected_is_not_connected() {
        let state = BridgeState::default();

        let mut params = serde_json::Map::new();
        params.insert("objectId".to_owned(), Value::String("n-1".to_owned()));
        let object_result = send_method(&state, "object.getSnapshot", Some(params)).await;
        assert!(matches!(object_result, Err(BackendError::NotConnected)));

        let schema_result =
            send_method(&state, "schema.getSnapshot", Some(serde_json::Map::new())).await;
        assert!(matches!(schema_result, Err(BackendError::NotConnected)));
    }

    /// The write command (`object.setProperty`, via `send_method`) must also fail
    /// with `NotConnected` when no live connection exists, never panicking or
    /// hanging. This is the disconnected-path drift guard for the new write
    /// command; the connected round-trip (including the in-memory mock mutation)
    /// is covered by the conformance / process e2e suites against the real mock
    /// engine.
    #[tokio::test]
    async fn object_set_property_when_disconnected_is_not_connected() {
        let state = BridgeState::default();

        let mut params = serde_json::Map::new();
        params.insert("objectId".to_owned(), Value::String("n-1".to_owned()));
        params.insert(
            "property".to_owned(),
            Value::String("fieldOfView".to_owned()),
        );
        params.insert("value".to_owned(), Value::from(75));
        let result = send_method(&state, "object.setProperty", Some(params)).await;
        assert!(matches!(result, Err(BackendError::NotConnected)));
    }

    /// The write command forwards an arbitrary JSON `value` verbatim into the
    /// request params (string/number/boolean/null/array/object). This is a pure
    /// shaping check on `build_request` so a structured (array/object) edit is not
    /// silently dropped or coerced; the round-trip is covered by e2e.
    #[test]
    fn object_set_property_forwards_structured_value_verbatim() {
        let mut params = serde_json::Map::new();
        params.insert("objectId".to_owned(), Value::String("n-1".to_owned()));
        params.insert("property".to_owned(), Value::String("position".to_owned()));
        // A nested array value, the kind the JSON editor produces.
        params.insert("value".to_owned(), serde_json::json!([0, 1.5, -10]));
        let env = build_request(
            CorrelationId::try_from("req-1".to_owned()).expect("valid id"),
            "object.setProperty",
            Some(params),
        )
        .expect("builds");
        match env {
            ValidatedEnvelope::Request {
                method,
                params: Some(map),
                ..
            } => {
                assert_eq!(method.as_str(), "object.setProperty");
                assert_eq!(map.get("value"), Some(&serde_json::json!([0, 1.5, -10])));
            }
            _ => panic!("expected a request envelope with params"),
        }
    }

    #[tokio::test]
    async fn scene_edit_methods_when_disconnected_are_not_connected() {
        let state = BridgeState::default();

        let mut create_params = serde_json::Map::new();
        create_params.insert("parentId".to_owned(), Value::String("n-0".to_owned()));
        let create_result = send_method(&state, "scene.createObject", Some(create_params)).await;
        assert!(matches!(create_result, Err(BackendError::NotConnected)));

        let mut delete_params = serde_json::Map::new();
        delete_params.insert("objectId".to_owned(), Value::String("n-1".to_owned()));
        let delete_result = send_method(&state, "scene.deleteObject", Some(delete_params)).await;
        assert!(matches!(delete_result, Err(BackendError::NotConnected)));

        let mut reparent_params = serde_json::Map::new();
        reparent_params.insert("objectId".to_owned(), Value::String("n-1".to_owned()));
        let reparent_result =
            send_method(&state, "scene.reparentObject", Some(reparent_params)).await;
        assert!(matches!(reparent_result, Err(BackendError::NotConnected)));

        let mut duplicate_params = serde_json::Map::new();
        duplicate_params.insert("objectId".to_owned(), Value::String("n-1".to_owned()));
        let duplicate_result =
            send_method(&state, "scene.duplicateObject", Some(duplicate_params)).await;
        assert!(matches!(duplicate_result, Err(BackendError::NotConnected)));
    }

    #[test]
    fn scene_reparent_shapes_root_move_without_new_parent_id() {
        let mut params = serde_json::Map::new();
        params.insert("objectId".to_owned(), Value::String("n-1".to_owned()));
        let env = build_request(
            CorrelationId::try_from("req-1".to_owned()).expect("valid id"),
            "scene.reparentObject",
            Some(params),
        )
        .expect("builds");
        match env {
            ValidatedEnvelope::Request {
                method,
                params: Some(map),
                ..
            } => {
                assert_eq!(method.as_str(), "scene.reparentObject");
                assert_eq!(map.get("objectId"), Some(&Value::String("n-1".to_owned())));
                assert_eq!(map.get("newParentId"), None);
            }
            _ => panic!("expected a request envelope with params"),
        }
    }

    #[test]
    fn scene_duplicate_shapes_sibling_without_new_parent_id() {
        // Mirrors the reparent shape guard: omitting new_parent_id must leave
        // `newParentId` absent from the wire params (engine duplicates as a
        // sibling under the source's parent), while `objectId` is always present.
        let mut params = serde_json::Map::new();
        params.insert("objectId".to_owned(), Value::String("n-1".to_owned()));
        let env = build_request(
            CorrelationId::try_from("req-1".to_owned()).expect("valid id"),
            "scene.duplicateObject",
            Some(params),
        )
        .expect("builds");
        match env {
            ValidatedEnvelope::Request {
                method,
                params: Some(map),
                ..
            } => {
                assert_eq!(method.as_str(), "scene.duplicateObject");
                assert_eq!(map.get("objectId"), Some(&Value::String("n-1".to_owned())));
                assert_eq!(map.get("newParentId"), None);
            }
            _ => panic!("expected a request envelope with params"),
        }
    }
    /// The thumbnail command (`viewport.getThumbnail`, via `send_method`) must
    /// also fail with `NotConnected` when no live connection exists, never
    /// panicking or hanging. This is the disconnected-path drift guard for the new
    /// pull-style command; the connected round-trip (the mock's fixed PNG) is
    /// covered by the conformance / process e2e suites against the real mock
    /// engine.
    #[tokio::test]
    async fn viewport_get_thumbnail_when_disconnected_is_not_connected() {
        let state = BridgeState::default();
        let mut params = serde_json::Map::new();
        params.insert("maxWidth".to_owned(), Value::from(640u32));
        params.insert("maxHeight".to_owned(), Value::from(360u32));
        let result = send_method(&state, "viewport.getThumbnail", Some(params)).await;
        assert!(matches!(result, Err(BackendError::NotConnected)));
    }

    /// The asset read commands (via `send_method`) must also fail with
    /// `NotConnected` when no live connection exists, never panicking or
    /// hanging. The connected round-trip is covered by conformance / process e2e
    /// suites once an engine implements the optional methods.
    #[tokio::test]
    async fn asset_methods_when_disconnected_are_not_connected() {
        let state = BridgeState::default();

        let mut resolve_params = serde_json::Map::new();
        resolve_params.insert(
            "logicalPath".to_owned(),
            Value::String("textures/hero.png".to_owned()),
        );
        let resolve_result = send_method(&state, "asset.resolve", Some(resolve_params)).await;
        assert!(matches!(resolve_result, Err(BackendError::NotConnected)));

        let manifest_result =
            send_method(&state, "asset.getManifest", Some(serde_json::Map::new())).await;
        assert!(matches!(manifest_result, Err(BackendError::NotConnected)));
    }

    /// `viewport_get_thumbnail` includes only the dimension keys that were
    /// supplied: with both `maxWidth`/`maxHeight` present they appear; this is a
    /// pure shaping check on `build_request` so optional params are not silently
    /// coerced. The round-trip is covered by e2e.
    #[test]
    fn viewport_get_thumbnail_shapes_optional_dimensions() {
        // Both present.
        let mut params = serde_json::Map::new();
        params.insert("maxWidth".to_owned(), Value::from(640u32));
        params.insert("maxHeight".to_owned(), Value::from(360u32));
        let env = build_request(
            CorrelationId::try_from("req-1".to_owned()).expect("valid id"),
            "viewport.getThumbnail",
            Some(params),
        )
        .expect("builds");
        match env {
            ValidatedEnvelope::Request {
                method,
                params: Some(map),
                ..
            } => {
                assert_eq!(method.as_str(), "viewport.getThumbnail");
                assert_eq!(map.get("maxWidth"), Some(&Value::from(640u32)));
                assert_eq!(map.get("maxHeight"), Some(&Value::from(360u32)));
            }
            _ => panic!("expected a request envelope with params"),
        }

        // Empty params object is also valid (engine picks the size).
        let env = build_request(
            CorrelationId::try_from("req-2".to_owned()).expect("valid id"),
            "viewport.getThumbnail",
            Some(serde_json::Map::new()),
        )
        .expect("builds");
        match env {
            ValidatedEnvelope::Request {
                params: Some(map), ..
            } => assert!(map.is_empty()),
            _ => panic!("expected a request envelope with empty params"),
        }
    }

    /// `asset.resolve` includes the required logicalPath and only the optional
    /// hints that were supplied.
    #[test]
    fn asset_resolve_shapes_optional_hints() {
        let mut params = serde_json::Map::new();
        params.insert(
            "logicalPath".to_owned(),
            Value::String("textures/hero.png".to_owned()),
        );
        params.insert("kind".to_owned(), Value::String("texture".to_owned()));
        let env = build_request(
            CorrelationId::try_from("req-1".to_owned()).expect("valid id"),
            "asset.resolve",
            Some(params),
        )
        .expect("builds");
        match env {
            ValidatedEnvelope::Request {
                method,
                params: Some(map),
                ..
            } => {
                assert_eq!(method.as_str(), "asset.resolve");
                assert_eq!(
                    map.get("logicalPath"),
                    Some(&Value::String("textures/hero.png".to_owned()))
                );
                assert_eq!(map.get("kind"), Some(&Value::String("texture".to_owned())));
                assert_eq!(map.get("variant"), None);
            }
            _ => panic!("expected a request envelope with params"),
        }
    }

    /// `asset.getManifest` includes only the filter/page fields that were
    /// supplied.
    #[test]
    fn asset_get_manifest_shapes_optional_bounds() {
        let mut params = serde_json::Map::new();
        params.insert("filter".to_owned(), Value::String("texture".to_owned()));
        params.insert("pageSize".to_owned(), Value::from(50));
        let env = build_request(
            CorrelationId::try_from("req-1".to_owned()).expect("valid id"),
            "asset.getManifest",
            Some(params),
        )
        .expect("builds");
        match env {
            ValidatedEnvelope::Request {
                method,
                params: Some(map),
                ..
            } => {
                assert_eq!(method.as_str(), "asset.getManifest");
                assert_eq!(
                    map.get("filter"),
                    Some(&Value::String("texture".to_owned()))
                );
                assert_eq!(map.get("page"), None);
                assert_eq!(map.get("pageSize"), Some(&Value::from(50)));
            }
            _ => panic!("expected a request envelope with params"),
        }
    }

    #[test]
    fn bridge_state_asset_reload_manifest_request_parts_are_exact() {
        let (method, params) = asset_reload_manifest_request_parts();
        assert_eq!(method, "asset.reloadManifest");
        assert!(params.is_empty());
        let _command = asset_reload_manifest;
    }

    #[test]
    fn bridge_state_asset_reload_manifest_validation_returns_original_value() {
        let value = serde_json::json!({ "accepted": false });
        let returned = validate_asset_reload_manifest_result(value.clone()).expect("valid result");
        assert_eq!(returned, value);

        let malformed = serde_json::json!({ "accepted": true, "extra": 1 });
        assert!(validate_asset_reload_manifest_result(malformed).is_err());
    }

    /// Core of Fix 1: a relay whose generation no longer matches the current
    /// `Connected` connection must NOT reset the phase (a concurrent reconnect
    /// already replaced it); a relay whose generation matches must reset it.
    #[tokio::test]
    async fn relay_resets_only_on_matching_generation() {
        // Current connection is generation 1 (e.g. after a reconnect).
        let phase = Phase::Connected(live_connection_with_generation(1));

        // A stale relay from generation 0 closing must not clobber it.
        assert!(!relay_should_reset_phase(&phase, 0));
        // The relay that owns the current generation may reset it.
        assert!(relay_should_reset_phase(&phase, 1));
    }
}
