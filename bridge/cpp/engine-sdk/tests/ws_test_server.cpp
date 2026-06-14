// Workstream G / G4 end-to-end test harness: a standalone WebSocket engine
// server process that the Rust editor-client e2e test (ws_roundtrip.rs) drives
// over a real local socket.
//
// This is a TEST harness, NOT production code: it is built under tests/ (never
// src/), is deliberately a separate executable from the Workstream H mock
// engine, and is NOT registered with CTest (Rust launches it directly, so a
// CTest entry would double-run it). It links the engine SDK and stands up a real
// WebSocketServerTransport bound to a caller-supplied port.
//
// Lifecycle contract with the Rust side:
//   * argv: --bridge-port <p> is required. A missing/invalid port is a hard
//     error (non-zero exit) so a misconfigured test fails fast.
//   * On a successful bind it prints exactly "READY <port>\n" to stdout and
//     flushes, which the Rust harness waits for before dialing. std::cout is
//     allowed here because this is a test harness (cpp.md only forbids standard-
//     stream logging inside the SDK src/include).
//   * It then runs a single recv loop: every inbound wire frame is fed to
//     BridgeEngineServer::handleFrame, any response is sent back, and after a
//     log.subscribe is acked it emits SEVERAL log.message events back-to-back to
//     exercise ordered multi-frame delivery (the G4 burst requirement).
//   * recv() returning nullopt means the client closed; the loop ends and the
//     process exits 0.
//
// The boundary rule (no libwebsockets type under the SDK public include/) is
// untouched: this TU includes only SDK public headers, and the only third-party
// surface is hidden behind the ITransport pImpl.

#include <atomic>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <optional>
#include <string>
#include <string_view>

#include "norves/bridge/adapter.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/dto/events.hpp"
#include "norves/bridge/dto/methods.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/log_sink.hpp"
#include "norves/bridge/result.hpp"
#include "norves/bridge/server.hpp"
#include "norves/bridge/transport.hpp"
#include "norves/bridge/ws_server_transport.hpp"

namespace {

using norves::bridge::BridgeError;
using norves::bridge::BridgeEngineServer;
using norves::bridge::IBridgeEngineAdapter;
using norves::bridge::ILogSink;
using norves::bridge::ITransport;
using norves::bridge::JsonValue;
using norves::bridge::LogSeverity;
using norves::bridge::make_websocket_server_transport;
using norves::bridge::Result;

using norves::bridge::dto::EngineState;
using norves::bridge::dto::HelloResult;
using norves::bridge::dto::LogLevel;
using norves::bridge::dto::LogMessageEvent;
using norves::bridge::dto::PlayAck;
using norves::bridge::dto::RuntimeState;
using norves::bridge::dto::ServerInfo;
using norves::bridge::dto::StatusSnapshot;

// Number of log.message events emitted (back-to-back) after a log.subscribe ack.
// More than one so the Rust side can assert ordered multi-frame delivery (the
// G4 burst requirement); the SDK transport guarantees in-order delivery.
constexpr int kLogBurst = 3;

// Parses a JSON literal or aborts the harness: the literals below are
// compile-time constants we control, so a parse failure is a programming error,
// not a runtime condition worth recovering from.
JsonValue parse_or_die(std::string_view text) {
    auto parsed = JsonValue::parse(text);
    if (parsed.is_err()) {
        std::cerr << "ws_test_server: internal JSON literal failed to parse\n";
        std::exit(2);
    }
    return std::move(parsed).value();
}

// A minimal stderr log sink so SDK Warn/Error diagnostics are visible in the
// child's captured stderr when a test goes wrong. stdout is reserved for the
// single READY line, so diagnostics go to stderr only.
class StderrSink : public ILogSink {
  public:
    void log(LogSeverity level, std::string_view message) override {
        if (level == LogSeverity::Warn || level == LogSeverity::Error) {
            std::cerr << "ws_test_server[sink]: " << message << '\n';
        }
    }
};

// Engine-side adapter mirroring loopback_roundtrip_test.cpp's FakeAdapter:
// answers each in-scope method with a typed DTO. logSubscribe additionally sets
// a flag the recv loop reads so it knows to emit the log.message burst after the
// subscribe ack (the same "set flag, emit after ack" pattern as the loopback
// test, but driven by the adapter rather than the test thread).
class FakeAdapter : public IBridgeEngineAdapter {
  public:
    Result<JsonValue, BridgeError> hello(const JsonValue& /*params*/,
                                         std::string_view selectedProtocolVersion) override {
        HelloResult result;
        result.sessionId = "sess-mock-1";
        result.protocolVersion = std::string(selectedProtocolVersion);
        result.server = ServerInfo{"MockEngine", std::optional<std::string>{"0.1.0"},
                                   std::optional<std::string>{"mock"}};
        return Result<JsonValue, BridgeError>::ok(result.to_json());
    }

    Result<JsonValue, BridgeError> getCapabilities(const JsonValue& /*params*/) override {
        // A single, deterministic capability descriptor so the Rust e2e can
        // assert a concrete round trip (not just an empty list). Shape matches
        // bridge.getCapabilities.result schema: capabilityDescriptor with a
        // namespaced name token and a MAJOR.MINOR version.
        return Result<JsonValue, BridgeError>::ok(
            parse_or_die(R"({"capabilities":[{"name":"runtime.control","version":"0.1"}]})"));
    }

    Result<JsonValue, BridgeError> getStatus(const JsonValue& /*params*/) override {
        StatusSnapshot snap;
        snap.engineState = EngineState::Ready;
        snap.runtimeState = RuntimeState::Edit;
        snap.engineName = "MockEngine";
        snap.engineVersion = "0.1.0";
        snap.title = "Mock Game";
        return Result<JsonValue, BridgeError>::ok(snap.to_json());
    }

    Result<JsonValue, BridgeError> launchInfo(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_die(R"({"launched":true})"));
    }

    Result<JsonValue, BridgeError> runtimePlay(const JsonValue& /*params*/) override {
        PlayAck ack;
        ack.accepted = true;
        ack.requestedState = RuntimeState::Playing;
        return Result<JsonValue, BridgeError>::ok(ack.to_json());
    }

    Result<JsonValue, BridgeError> runtimePause(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_die(R"({"accepted":true})"));
    }

    Result<JsonValue, BridgeError> runtimeStop(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_die(R"({"accepted":true})"));
    }

    Result<JsonValue, BridgeError> runtimeFocusViewport(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_die(R"({"focused":true})"));
    }

    Result<JsonValue, BridgeError> logSubscribe(const JsonValue& /*params*/) override {
        // Flag the recv loop to emit the log.message burst AFTER this ack is
        // sent, keeping ack-before-event ordering deterministic.
        emit_log_burst.store(true);
        return Result<JsonValue, BridgeError>::ok(parse_or_die(R"({"subscribed":true})"));
    }

    Result<JsonValue, BridgeError> logUnsubscribe(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_die(R"({"unsubscribed":true})"));
    }

    // Set by logSubscribe(), consumed by the recv loop. Single-threaded in
    // practice (handleFrame and the loop run on the same thread); atomic anyway
    // for a clear cross-method handoff.
    std::atomic<bool> emit_log_burst{false};
};

// Reads --bridge-port. Returns the port on success; prints an error and returns
// nullopt on a missing/invalid value.
std::optional<std::uint16_t> parse_port(int argc, char** argv) {
    for (int i = 1; i < argc; ++i) {
        std::string_view arg = argv[i];
        if (arg == "--bridge-port") {
            if (i + 1 >= argc) {
                std::cerr << "ws_test_server: --bridge-port requires a value\n";
                return std::nullopt;
            }
            const std::string value = argv[i + 1];
            try {
                const unsigned long parsed = std::stoul(value);
                if (parsed == 0 || parsed > 65535) {
                    std::cerr << "ws_test_server: port out of range: " << value << '\n';
                    return std::nullopt;
                }
                return static_cast<std::uint16_t>(parsed);
            } catch (const std::exception&) {
                std::cerr << "ws_test_server: invalid port: " << value << '\n';
                return std::nullopt;
            }
        }
    }
    std::cerr << "ws_test_server: missing required --bridge-port <port>\n";
    return std::nullopt;
}

}  // namespace

int main(int argc, char** argv) {
    const std::optional<std::uint16_t> port = parse_port(argc, argv);
    if (!port.has_value()) {
        return 2;
    }

    constexpr std::size_t kSendCap = 256;
    constexpr std::size_t kRecvCap = 256;

    StderrSink sink;
    std::unique_ptr<ITransport> transport =
        make_websocket_server_transport(*port, kSendCap, kRecvCap, &sink);
    if (transport == nullptr) {
        std::cerr << "ws_test_server: failed to bind WebSocket server on port " << *port << '\n';
        return 3;
    }

    FakeAdapter adapter;
    BridgeEngineServer server(adapter, &sink);

    // Pre-build the log.message event frame once; emitted (kLogBurst times) after
    // a log.subscribe ack.
    LogMessageEvent log;
    log.level = LogLevel::Info;
    log.message = "Game started";
    log.category = "Engine";
    const std::string log_event_frame = server.emitEvent("log.message", log.to_json());

    // Signal readiness AFTER a successful bind so the Rust harness only dials a
    // listening socket. stdout is reserved for this single line; flush so the
    // parent observes it promptly.
    std::cout << "READY " << *port << '\n';
    std::cout.flush();

    // Single recv loop. handleFrame and the adapter run on this thread, so the
    // emit_log_burst flag set inside logSubscribe is visible right after
    // handleFrame returns. We send the subscribe ack first, then the burst, so
    // the client sees ack-before-events.
    while (true) {
        std::optional<std::string> frame = transport->recv();
        if (!frame.has_value()) {
            break;  // client closed and the inbound queue drained: clean exit.
        }

        std::optional<std::string> response = server.handleFrame(*frame);
        if (response.has_value()) {
            if (!transport->send(std::move(*response))) {
                break;  // peer gone mid-flight.
            }
        }

        if (adapter.emit_log_burst.exchange(false)) {
            for (int i = 0; i < kLogBurst; ++i) {
                if (!transport->send(std::string(log_event_frame))) {
                    break;
                }
            }
        }
    }

    return 0;
}
