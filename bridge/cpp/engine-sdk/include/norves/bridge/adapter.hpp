#ifndef NORVES_BRIDGE_ADAPTER_HPP
#define NORVES_BRIDGE_ADAPTER_HPP

#include <optional>
#include <string>
#include <string_view>

#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"

// Engine adapter interface: the seam an engine integration implements so that
// BridgeEngineServer can dispatch decoded Bridge requests into engine logic.
//
// Depends on <std> and the SDK's own value types only; no third-party headers
// are included here. Every payload (request params, success result,
// error.data) is carried as the opaque JsonValue: this layer does NOT interpret
// payload contents. Typed per-method DTOs are a later phase; an adapter that
// wants typed access parses the JsonValue itself.
//
// Thread affinity (REQUIRED reading):
//   * The SDK invokes each adapter method synchronously, on the SAME thread
//     that called BridgeEngineServer::handleFrame. The SDK never spawns a
//     thread to call the adapter and never calls it concurrently with itself.
//   * If an adapter implementation holds state that may only be touched on the
//     engine main thread, the EMBEDDER is responsible for driving
//     handleFrame on that engine main thread. The SDK provides no marshaling.
//   * An adapter MUST NOT return engine live memory through a JsonValue. It
//     converts engine state into a snapshot/DTO value first
//     (docs/memory-buffer-policy.md). The returned JsonValue is owned by the
//     caller after the call.
//   * `params` is borrowed for the duration of the call only; an adapter must
//     not retain the reference past return.
namespace norves::bridge {

// Pure-virtual engine adapter. In-scope methods are pure virtual (the engine
// MUST implement them). The optional scene/object/schema methods are NON-pure
// with a default that reports METHOD_NOT_SUPPORTED, reflecting the protocol's
// open method registry: an engine overrides only what it supports.
class IBridgeEngineAdapter {
  public:
    virtual ~IBridgeEngineAdapter() = default;

    // --- Handshake -----------------------------------------------------------

    // bridge.hello. `selectedProtocolVersion` is the version the server already
    // chose by intersecting the client's offered protocolVersions (in client
    // preference order) with this SDK's kSupportedProtocolVersions. The adapter
    // builds the result payload (sessionId / protocolVersion / server /
    // optional capabilities) as a JsonValue and is responsible for placing
    // `selectedProtocolVersion` into the result's protocolVersion field.
    virtual Result<JsonValue, BridgeError> hello(const JsonValue& params,
                                                 std::string_view selectedProtocolVersion) = 0;

    // bridge.getCapabilities.
    virtual Result<JsonValue, BridgeError> getCapabilities(const JsonValue& params) = 0;

    // --- Engine status / launch ----------------------------------------------

    // engine.getStatus.
    virtual Result<JsonValue, BridgeError> getStatus(const JsonValue& params) = 0;

    // engine.launchInfo.
    virtual Result<JsonValue, BridgeError> launchInfo(const JsonValue& params) = 0;

    // --- Runtime control -----------------------------------------------------

    // runtime.play.
    virtual Result<JsonValue, BridgeError> runtimePlay(const JsonValue& params) = 0;

    // runtime.pause.
    virtual Result<JsonValue, BridgeError> runtimePause(const JsonValue& params) = 0;

    // runtime.stop.
    virtual Result<JsonValue, BridgeError> runtimeStop(const JsonValue& params) = 0;

    // runtime.focusViewport.
    virtual Result<JsonValue, BridgeError> runtimeFocusViewport(const JsonValue& params) = 0;

    // --- Log streaming -------------------------------------------------------

    // log.subscribe.
    virtual Result<JsonValue, BridgeError> logSubscribe(const JsonValue& params) = 0;

    // log.unsubscribe.
    virtual Result<JsonValue, BridgeError> logUnsubscribe(const JsonValue& params) = 0;

    // --- Optional (open registry) --------------------------------------------
    //
    // These have a default implementation that reports METHOD_NOT_SUPPORTED.
    // An engine that supports them overrides the relevant method.

    // scene.getTree.
    virtual Result<JsonValue, BridgeError> sceneGetTree(const JsonValue& params) {
        return not_supported(params);
    }

    // object.getSnapshot.
    virtual Result<JsonValue, BridgeError> objectGetSnapshot(const JsonValue& params) {
        return not_supported(params);
    }

    // object.setProperty.
    virtual Result<JsonValue, BridgeError> objectSetProperty(const JsonValue& params) {
        return not_supported(params);
    }

    // schema.getSnapshot.
    virtual Result<JsonValue, BridgeError> schemaGetSnapshot(const JsonValue& params) {
        return not_supported(params);
    }

  protected:
    IBridgeEngineAdapter() = default;
    IBridgeEngineAdapter(const IBridgeEngineAdapter&) = default;
    IBridgeEngineAdapter(IBridgeEngineAdapter&&) = default;
    IBridgeEngineAdapter& operator=(const IBridgeEngineAdapter&) = default;
    IBridgeEngineAdapter& operator=(IBridgeEngineAdapter&&) = default;

    // Shared default-implementation body for the optional methods: an
    // unimplemented optional method reports METHOD_NOT_SUPPORTED.
    static Result<JsonValue, BridgeError> not_supported(const JsonValue& /*params*/) {
        return Result<JsonValue, BridgeError>::err(
            BridgeError{std::string(kErrorMethodNotSupported),
                        "Method is not supported by this engine adapter.", std::nullopt});
    }
};

}  // namespace norves::bridge

#endif  // NORVES_BRIDGE_ADAPTER_HPP
