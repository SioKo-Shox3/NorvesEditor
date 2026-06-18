#pragma once

#include "norves/bridge/json_value.hpp"

#include <memory>
#include <optional>
#include <string>
#include <string_view>

// Bridge engine-side request dispatcher.
//
// Depends on <std> and the SDK's own value types only; no third-party headers
// are included here. The underlying JSON library is confined to server.cpp.
//
// BridgeEngineServer decodes an incoming wire frame, owns the bridge.hello
// version negotiation, dispatches known requests into the IBridgeEngineAdapter,
// and encodes the response frame. Transport (reading/writing the socket) is a
// later phase and is NOT part of this type: the embedder feeds wire frames in
// and sends the returned wire frames out.
namespace norves::bridge
{

    class IBridgeEngineAdapter;
    class ILogSink;

    class BridgeEngineServer
    {
    public:
        // Constructs a server bound to `adapter` (held by reference) and an OPTIONAL
        // `log_sink` (may be nullptr for a silent server).
        //
        // Ownership / lifetime: the server stores a reference to `adapter` and a raw
        // pointer to `log_sink`; it owns NEITHER. The caller MUST keep both alive
        // for the entire lifetime of the server (adapter and sink outlive the
        // server). The server stores no other long-lived state.
        explicit BridgeEngineServer(IBridgeEngineAdapter& adapter, ILogSink* log_sink = nullptr);

        ~BridgeEngineServer();

        BridgeEngineServer(const BridgeEngineServer&) = delete;
        BridgeEngineServer& operator=(const BridgeEngineServer&) = delete;
        BridgeEngineServer(BridgeEngineServer&&) noexcept;
        BridgeEngineServer& operator=(BridgeEngineServer&&) noexcept;

        // Handles one inbound wire frame and returns the response wire frame to
        // send, if any.
        //
        // Lifetime of `wire`: borrowed for the duration of this call only. The
        // server decodes it (copying out everything it needs, including opaque
        // payloads) and synchronously invokes the adapter before returning; it
        // retains no view into `wire`. `wire` need only stay valid until this call
        // returns. The returned std::string (when present) is owned by the caller.
        //
        // Returns std::nullopt (no response to send) when:
        //   * the frame fails to decode (a malformed frame has no recoverable
        //     correlation id, so no valid response envelope can be built; the
        //     failure is reported to the log sink at Warn),
        //   * the frame is a response or event rather than a request (the server
        //     processes requests only; logged at Debug).
        // Otherwise it returns the encoded response envelope (a result on success,
        // a wire error on failure), echoing the request's correlation id.
        [[nodiscard]] std::optional<std::string> handleFrame(std::string_view wire);

        // Builds and encodes an event envelope (kind=event) carrying `event_name`
        // and `params`, returning the wire frame. Sending it is the embedder's job
        // (transport is a later phase). `event_name` examples: "log.message",
        // "engine.statusChanged". `params` is copied into the envelope.
        [[nodiscard]] std::string emitEvent(std::string_view event_name, const JsonValue& params);

    private:
        // pImpl keeps the dispatch-table internals and any JSON-library usage out
        // of this public header.
        struct Impl;
        std::unique_ptr<Impl> impl_;
    };

}  // namespace norves::bridge
