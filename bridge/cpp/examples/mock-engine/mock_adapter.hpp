#ifndef NORVES_MOCK_ENGINE_MOCK_ADAPTER_HPP
#define NORVES_MOCK_ENGINE_MOCK_ADAPTER_HPP

// Workstream H-A: engine-side adapter for the residential mock engine.
//
// This MockAdapter intentionally duplicates the FakeAdapter in
// engine-sdk/tests/ws_test_server.cpp. ws_test_server is a G4 test asset and is
// left untouched to avoid breaking its e2e. If the two drift, the H-D
// conformance runner detects it.
//
// An adapter is an engine-implementation responsibility, not SDK surface, so it
// lives under examples/ (not engine-sdk/src). It depends on <std> and the SDK's
// public headers only: every payload is built from the typed DTOs' to_json() or
// from JsonValue::parse, never from a third-party JSON type directly. That keeps
// this directory free of any libwebsockets / nlohmann include.

#include <atomic>
#include <optional>
#include <string>
#include <string_view>

#include "norves/bridge/adapter.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/dto/methods.hpp"
#include "norves/bridge/result.hpp"

namespace norves::mock {

// Parses a JSON literal or aborts: the literals below are compile-time constants
// we control, so a parse failure is a programming error, not a runtime
// condition. The mock engine has no recoverable path for a broken literal.
inline norves::bridge::JsonValue parse_or_die(std::string_view text) {
    auto parsed = norves::bridge::JsonValue::parse(text);
    if (parsed.is_err()) {
        std::exit(2);
    }
    return std::move(parsed).value();
}

// Mock engine adapter. Response values match the G4 FakeAdapter one-for-one so
// the editor backend observes the same wire shapes whether it drives the mock
// engine over WebSocket (main.cpp) or the loopback smoke.
class MockAdapter : public norves::bridge::IBridgeEngineAdapter {
  public:
    norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
    hello(const norves::bridge::JsonValue& /*params*/,
          std::string_view selectedProtocolVersion) override {
        norves::bridge::dto::HelloResult result;
        result.sessionId = "sess-mock-1";
        result.protocolVersion = std::string(selectedProtocolVersion);
        result.server = norves::bridge::dto::ServerInfo{
            "MockEngine", std::optional<std::string>{"0.1.0"},
            std::optional<std::string>{"mock"}};
        return norves::bridge::Result<norves::bridge::JsonValue,
                                      norves::bridge::BridgeError>::ok(result.to_json());
    }

    norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
    getCapabilities(const norves::bridge::JsonValue& /*params*/) override {
        // Shape matches bridge.getCapabilities.result: a capabilityDescriptor
        // with a namespaced name token and a MAJOR.MINOR version.
        return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::ok(
            parse_or_die(R"({"capabilities":[{"name":"runtime.control","version":"0.1"}]})"));
    }

    norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
    getStatus(const norves::bridge::JsonValue& /*params*/) override {
        norves::bridge::dto::StatusSnapshot snap;
        snap.engineState = norves::bridge::dto::EngineState::Ready;
        snap.runtimeState = norves::bridge::dto::RuntimeState::Edit;
        snap.engineName = "MockEngine";
        snap.engineVersion = "0.1.0";
        snap.title = "Mock Game";
        return norves::bridge::Result<norves::bridge::JsonValue,
                                      norves::bridge::BridgeError>::ok(snap.to_json());
    }

    norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
    launchInfo(const norves::bridge::JsonValue& /*params*/) override {
        // engine.launchInfo is a required (pure-virtual) method, so a minimal
        // success result is returned rather than METHOD_NOT_SUPPORTED.
        return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::ok(
            parse_or_die(R"({"launched":true})"));
    }

    norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
    runtimePlay(const norves::bridge::JsonValue& /*params*/) override {
        norves::bridge::dto::PlayAck ack;
        ack.accepted = true;
        ack.requestedState = norves::bridge::dto::RuntimeState::Playing;
        return norves::bridge::Result<norves::bridge::JsonValue,
                                      norves::bridge::BridgeError>::ok(ack.to_json());
    }

    norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
    runtimePause(const norves::bridge::JsonValue& /*params*/) override {
        return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::ok(
            parse_or_die(R"({"accepted":true})"));
    }

    norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
    runtimeStop(const norves::bridge::JsonValue& /*params*/) override {
        return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::ok(
            parse_or_die(R"({"accepted":true})"));
    }

    norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
    runtimeFocusViewport(const norves::bridge::JsonValue& /*params*/) override {
        return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::ok(
            parse_or_die(R"({"focused":true})"));
    }

    norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
    logSubscribe(const norves::bridge::JsonValue& /*params*/) override {
        // Flag the recv loop to emit the log.message burst AFTER this ack is
        // sent, keeping ack-before-event ordering deterministic (same "set flag,
        // emit after ack" pattern as ws_test_server's FakeAdapter).
        emit_log_burst.store(true);
        return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::ok(
            parse_or_die(R"({"subscribed":true})"));
    }

    norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
    logUnsubscribe(const norves::bridge::JsonValue& /*params*/) override {
        return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::ok(
            parse_or_die(R"({"unsubscribed":true})"));
    }

    // Set by logSubscribe(), consumed by the recv loop. handleFrame and the loop
    // run on the same thread, so this is a single-threaded handoff; atomic anyway
    // for a clear cross-method contract.
    std::atomic<bool> emit_log_burst{false};
};

}  // namespace norves::mock

#endif  // NORVES_MOCK_ENGINE_MOCK_ADAPTER_HPP
