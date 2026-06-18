#pragma once

#include "norves/bridge/log_sink.hpp"
#include "norves/bridge/transport.hpp"

#include <cstddef>
#include <cstdint>
#include <memory>

// WebSocket server transport for the engine SDK (Workstream G / G3).
//
// This header exposes a WebSocket server endpoint as a plain ITransport through
// a single factory. It depends on <std> + the SDK's own ITransport / ILogSink
// only: NO third-party WebSocket type (the underlying context / connection
// handle) ever appears here. The third-party library is hidden behind the
// ITransport pImpl and linked PRIVATE into the matching .cpp; the boundary
// (third-party tokens under include/ == 0 hits) is enforced by CI. See ADR 0007.
//
// Thread model (normative; enforced in the .cpp): the transport owns ONE service
// thread that runs the WebSocket event loop and exclusively touches every
// underlying handle. ITransport::send() (any external thread) only enqueues a
// frame and wakes the service thread; ITransport::recv() (a consumer thread)
// blocks on the inbound queue. close() (any thread) only signals the service
// thread and shuts the queues; the real teardown happens on the service thread.
namespace norves::bridge
{

    // Creates a WebSocket server transport listening on 127.0.0.1:`port` (loopback
    // only; 0.0.0.0 is never used, matching the localhost-only alpha scope). TLS is
    // off.
    //
    // On success returns a ready ITransport whose service thread is already running
    // and accepting a single editor-client connection. On bind / context-creation
    // failure (e.g. the port is in use) returns nullptr and, if `log_sink` is
    // non-null, emits one Warn line describing the reason.
    //
    // `send_capacity` / `recv_capacity` bound the per-direction frame queues. The
    // send queue uses back-pressure (a full queue makes send() return false). The
    // receive queue treats overflow as FATAL: dropping an inbound frame would break
    // request/response correlation, so a full receive queue closes the connection
    // (Error logged) and leaves recovery to the upper layer (reconnect). Pick
    // `recv_capacity` generously.
    //
    // The returned value is std::unique_ptr<ITransport>; no underlying WebSocket
    // handle is observable through it. close() (or destruction) tears the server
    // down and makes recv() drain to nullopt and send() return false.
    [[nodiscard]] std::unique_ptr<ITransport> make_websocket_server_transport(
        std::uint16_t port, std::size_t send_capacity, std::size_t recv_capacity,
        ILogSink* log_sink = nullptr);

}  // namespace norves::bridge
