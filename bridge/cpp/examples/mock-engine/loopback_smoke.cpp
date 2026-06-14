// Workstream H-A: loopback smoke for the residential mock engine.
//
// The CTest-registered companion of the long-running norves_mock_engine (which
// cannot itself be a CTest target -- it blocks in recv() until closed). It runs
// the SAME MockAdapter behind a BridgeEngineServer on an engine thread, wired to
// a client over an in-process loopback pair, and drives the minimal launch->drive
// path the editor backend uses: hello -> runtime.play -> log.subscribe (+ the
// log.message burst). This proves the adapter's wire shapes and the recv-loop
// emit-after-ack ordering without standing up a real socket.
//
// Only std + the SDK's public headers (plus test_support.hpp) are used; the
// public-header boundary is unaffected. ctest pass/fail is the process exit code.
//
// Termination: the client closes its endpoint, which drains the engine's recv()
// to nullopt and ends the engine loop, so the engine thread joins deterministically
// (no hang).

#include <atomic>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <utility>

#include "norves/bridge/codec.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/dto/events.hpp"
#include "norves/bridge/dto/methods.hpp"
#include "norves/bridge/envelope.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/server.hpp"
#include "norves/bridge/transport.hpp"

#include "mock_adapter.hpp"
#include "test_support.hpp"

namespace {

using norves::bridge::BridgeEngineServer;
using norves::bridge::decode_envelope;
using norves::bridge::Envelope;
using norves::bridge::ITransport;
using norves::bridge::JsonValue;
using norves::bridge::Kind;
using norves::bridge::make_loopback_pair;

using norves::bridge::dto::HelloResult;
using norves::bridge::dto::LogLevel;
using norves::bridge::dto::LogMessageEvent;
using norves::bridge::dto::PlayAck;
using norves::bridge::dto::RuntimeState;

using norves::mock::MockAdapter;

// Matches the residential recv loop's behaviour: after sending a response, if the
// adapter flagged a log.subscribe, emit the log.message burst. Ends on clean EOF
// (client close), mirroring main.cpp's loop termination via close().
constexpr int kLogBurst = 3;

Envelope decode_or_fail(std::string_view wire) {
    auto decoded = decode_envelope(wire);
    if (decoded.is_err()) {
        ::norves::test::report_failure("decode_envelope failed", __FILE__, __LINE__);
        return Envelope();
    }
    return std::move(decoded).value();
}

// Engine read loop, structurally identical to main.cpp's residential loop but
// driven over the loopback transport. Returns when recv() reports clean EOF.
void run_engine(ITransport& engine, BridgeEngineServer& server, MockAdapter& adapter,
                const std::string& log_event_frame) {
    while (true) {
        std::optional<std::string> frame = engine.recv();
        if (!frame.has_value()) {
            return;  // client closed and the inbound queue drained.
        }
        std::optional<std::string> response = server.handleFrame(*frame);
        if (response.has_value()) {
            if (!engine.send(std::move(*response))) {
                return;
            }
        }
        if (adapter.emit_log_burst.exchange(false)) {
            for (int i = 0; i < kLogBurst; ++i) {
                if (!engine.send(std::string(log_event_frame))) {
                    return;
                }
            }
        }
    }
}

// Builds a request wire frame (same shape as the SDK tests' helper).
std::string request_frame(std::string_view id, std::string_view method,
                          std::string_view params_json) {
    std::string frame =
        R"({"bridge":"norves.editor.bridge","version":"0.1","kind":"request","id":")";
    frame += std::string(id);
    frame += R"(","method":")";
    frame += std::string(method);
    frame += R"(")";
    if (!params_json.empty()) {
        frame += R"(,"params":)";
        frame += std::string(params_json);
    }
    frame += "}";
    return frame;
}

void test_mock_engine_loopback() {
    auto [client, engine] = make_loopback_pair(16);

    MockAdapter adapter;
    BridgeEngineServer server(adapter);

    LogMessageEvent log;
    log.level = LogLevel::Info;
    log.message = "Game started";
    log.category = "Engine";
    const std::string log_event_frame = server.emitEvent("log.message", log.to_json());

    std::thread engine_thread(run_engine, std::ref(*engine), std::ref(server), std::ref(adapter),
                              std::cref(log_event_frame));

    // 1. bridge.hello -------------------------------------------------------
    client->send(request_frame(
        "req-hello", "bridge.hello",
        R"({"role":"editor","clientName":"NorvesEditor","protocolVersions":["0.1"]})"));
    {
        std::optional<std::string> resp = client->recv();
        NORVES_CHECK(resp.has_value());
        if (resp.has_value()) {
            const Envelope env = decode_or_fail(*resp);
            NORVES_CHECK(env.kind == Kind::Response);
            NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-hello"});
            NORVES_CHECK(env.result.has_value());
            if (env.result.has_value()) {
                auto parsed = HelloResult::from_json(*env.result);
                NORVES_CHECK(parsed.is_ok());
                if (parsed.is_ok()) {
                    const HelloResult& r = parsed.value();
                    NORVES_CHECK_EQ(r.sessionId, std::string{"sess-mock-1"});
                    NORVES_CHECK_EQ(r.protocolVersion, std::string{"0.1"});
                    NORVES_CHECK_EQ(r.server.name, std::string{"MockEngine"});
                    NORVES_CHECK_EQ(r.server.engine, std::optional<std::string>{"mock"});
                }
            }
        }
    }

    // 2. runtime.play -------------------------------------------------------
    client->send(request_frame("req-play", "runtime.play", "{}"));
    {
        std::optional<std::string> resp = client->recv();
        NORVES_CHECK(resp.has_value());
        if (resp.has_value()) {
            const Envelope env = decode_or_fail(*resp);
            NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-play"});
            NORVES_CHECK(env.result.has_value());
            if (env.result.has_value()) {
                auto parsed = PlayAck::from_json(*env.result);
                NORVES_CHECK(parsed.is_ok());
                if (parsed.is_ok()) {
                    const PlayAck& a = parsed.value();
                    NORVES_CHECK(a.accepted);
                    NORVES_CHECK(a.requestedState.has_value() &&
                                 *a.requestedState == RuntimeState::Playing);
                }
            }
        }
    }

    // 3. log.subscribe + log.message burst ----------------------------------
    client->send(request_frame("req-logsub", "log.subscribe", ""));
    {
        std::optional<std::string> ack = client->recv();
        NORVES_CHECK(ack.has_value());  // the log.subscribe response.
        if (ack.has_value()) {
            const Envelope env = decode_or_fail(*ack);
            NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-logsub"});
            NORVES_CHECK(env.result.has_value());
        }

        // The engine emits kLogBurst log.message events after the ack, in order.
        for (int i = 0; i < kLogBurst; ++i) {
            std::optional<std::string> event = client->recv();
            NORVES_CHECK(event.has_value());
            if (event.has_value()) {
                const Envelope env = decode_or_fail(*event);
                NORVES_CHECK(env.kind == Kind::Event);
                NORVES_CHECK_EQ(env.event, std::optional<std::string>{"log.message"});
                NORVES_CHECK(env.params.has_value());
                if (env.params.has_value()) {
                    auto parsed = LogMessageEvent::from_json(*env.params);
                    NORVES_CHECK(parsed.is_ok());
                    if (parsed.is_ok()) {
                        const LogMessageEvent& e = parsed.value();
                        NORVES_CHECK(e.level == LogLevel::Info);
                        NORVES_CHECK_EQ(e.message, std::string{"Game started"});
                    }
                }
            }
        }
    }

    // Orderly teardown: close the client's outbound direction so the engine's
    // recv() drains to nullopt, ending its loop; then join (no hang).
    client->close();
    engine_thread.join();
}

}  // namespace

int main() {
    test_mock_engine_loopback();
    return norves::test::summary();
}
