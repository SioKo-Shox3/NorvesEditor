// F5 end-to-end loopback round-trip test for the C++ engine SDK.
//
// The C++ analogue of the Rust editor-client's loopback_roundtrip.rs. It wires a
// client transport to an engine transport via make_loopback_pair (in-process,
// WebSocket-free, F4 BoundedFrameQueue underneath), runs a BridgeEngineServer on
// the engine side in its own thread, and drives four wire paths end to end:
//   1. bridge.hello       (typed HelloParams -> HelloResult)
//   2. engine.getStatus   (-> StatusSnapshot)
//   3. runtime.play       (empty params -> PlayAck)
//   4. log.message event  (engine emitEvent -> client decode_envelope + DTO)
//
// It also checks the typed-DTO contract directly: from_json(to_json(x)) == x for
// each DTO, and that an unknown key is rejected recursively (including the nested
// `server` object of bridge.hello.result).
//
// Only std + the SDK's public headers are used; the boundary rule (no nlohmann
// in include/) is unaffected. ctest pass/fail is the process exit code.

#include "norves/bridge/adapter.hpp"
#include "norves/bridge/codec.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/dto/events.hpp"
#include "norves/bridge/dto/methods.hpp"
#include "norves/bridge/envelope.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"
#include "norves/bridge/server.hpp"
#include "norves/bridge/transport.hpp"

#include <atomic>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <utility>

#include "test_support.hpp"

namespace
{

    using norves::bridge::BridgeEngineServer;
    using norves::bridge::BridgeError;
    using norves::bridge::decode_envelope;
    using norves::bridge::Envelope;
    using norves::bridge::IBridgeEngineAdapter;
    using norves::bridge::ITransport;
    using norves::bridge::JsonValue;
    using norves::bridge::Kind;
    using norves::bridge::make_loopback_pair;
    using norves::bridge::Result;

    using norves::bridge::dto::EngineState;
    using norves::bridge::dto::HelloParams;
    using norves::bridge::dto::HelloResult;
    using norves::bridge::dto::LogLevel;
    using norves::bridge::dto::LogMessageEvent;
    using norves::bridge::dto::PlayAck;
    using norves::bridge::dto::RuntimeState;
    using norves::bridge::dto::ServerInfo;
    using norves::bridge::dto::StatusSnapshot;

    JsonValue parse_or_fail(std::string_view text)
    {
        auto parsed = JsonValue::parse(text);
        if (parsed.is_err())
        {
            ::norves::test::report_failure("JsonValue::parse failed", __FILE__, __LINE__);
            return JsonValue();
        }
        return std::move(parsed).value();
    }

    // Engine-side adapter: answers the three round-trip methods by returning typed
    // DTOs serialized via to_json(). Other methods fall through to the default
    // METHOD_NOT_SUPPORTED (this test never calls them).
    class FakeAdapter : public IBridgeEngineAdapter
    {
    public:
        Result<JsonValue, BridgeError> hello(const JsonValue& /*params*/,
                                             std::string_view selectedProtocolVersion) override
        {
            HelloResult result;
            result.sessionId = "sess-mock-1";
            result.protocolVersion = std::string(selectedProtocolVersion);
            result.server = ServerInfo{"MockEngine", std::optional<std::string>{"0.1.0"},
                                       std::optional<std::string>{"mock"}};
            return Result<JsonValue, BridgeError>::ok(result.to_json());
        }

        Result<JsonValue, BridgeError> getCapabilities(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(
                parse_or_fail(R"({"capabilities":[{"name":"runtime.control"}]})"));
        }

        Result<JsonValue, BridgeError> getStatus(const JsonValue& /*params*/) override
        {
            StatusSnapshot snap;
            snap.engineState = EngineState::Ready;
            snap.runtimeState = RuntimeState::Edit;
            snap.engineName = "MockEngine";
            snap.engineVersion = "0.1.0";
            snap.title = "Mock Game";
            return Result<JsonValue, BridgeError>::ok(snap.to_json());
        }

        Result<JsonValue, BridgeError> launchInfo(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({"launched":true})"));
        }

        Result<JsonValue, BridgeError> runtimePlay(const JsonValue& /*params*/) override
        {
            PlayAck ack;
            ack.accepted = true;
            ack.requestedState = RuntimeState::Playing;
            return Result<JsonValue, BridgeError>::ok(ack.to_json());
        }

        Result<JsonValue, BridgeError> runtimePause(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(
                parse_or_fail(R"({"runtimeState":"paused"})"));
        }

        Result<JsonValue, BridgeError> runtimeStop(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(
                parse_or_fail(R"({"runtimeState":"stopped"})"));
        }

        Result<JsonValue, BridgeError> runtimeFocusViewport(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({"focused":true})"));
        }

        Result<JsonValue, BridgeError> logSubscribe(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({})"));
        }

        Result<JsonValue, BridgeError> logUnsubscribe(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({})"));
        }
    };

    // Wire request builder (same shape as dispatch_test's helper).
    std::string request_frame(std::string_view id, std::string_view method,
                              std::string_view params_json)
    {
        std::string frame =
            R"({"bridge":"norves.editor.bridge","version":"0.1","kind":"request","id":")";
        frame += std::string(id);
        frame += R"(","method":")";
        frame += std::string(method);
        frame += R"(")";
        if (!params_json.empty())
        {
            frame += R"(,"params":)";
            frame += std::string(params_json);
        }
        frame += "}";
        return frame;
    }

    Envelope decode_or_fail(std::string_view wire)
    {
        auto decoded = decode_envelope(wire);
        if (decoded.is_err())
        {
            ::norves::test::report_failure("decode_envelope failed", __FILE__, __LINE__);
            return Envelope();
        }
        return std::move(decoded).value();
    }

    // The engine read loop: pull a frame, dispatch through the server, send any
    // response back. Returns when recv() reports clean EOF (client closed), so the
    // spawning test can join it deterministically.
    void run_engine(ITransport& engine, BridgeEngineServer& server,
                    const std::string& log_event_frame, std::atomic<bool>& emit_log)
    {
        while (true)
        {
            std::optional<std::string> frame = engine.recv();
            if (!frame.has_value())
            {
                return;  // peer closed and the inbound queue is drained.
            }
            std::optional<std::string> response = server.handleFrame(*frame);
            if (response.has_value())
            {
                if (!engine.send(std::move(*response)))
                {
                    return;  // peer gone mid-flight.
                }
            }
            // The client sets emit_log just before sending log.subscribe, so the
            // iteration that handles the subscribe frame sees the flag. On that
            // iteration we emit exactly one log.message event AFTER sending the
            // subscribe response above. Emitting strictly after the response keeps
            // delivery deterministic: the ack reaches the client's inbound queue
            // before the event, so the client recv()s them in that fixed order.
            if (emit_log.exchange(false))
            {
                if (!engine.send(std::string(log_event_frame)))
                {
                    return;
                }
            }
        }
    }

    // --- End-to-end round trip ---------------------------------------------------

    void test_loopback_round_trip()
    {
        auto [client, engine] = make_loopback_pair(16);

        FakeAdapter adapter;
        BridgeEngineServer server(adapter);

        // The log.message event the engine emits after the subscribe ack.
        LogMessageEvent log;
        log.level = LogLevel::Info;
        log.message = "Game started";
        log.category = "Engine";
        const std::string log_event_frame = server.emitEvent("log.message", log.to_json());

        std::atomic<bool> emit_log{false};
        std::thread engine_thread(run_engine, std::ref(*engine), std::ref(server),
                                  std::cref(log_event_frame), std::ref(emit_log));

        // 1. bridge.hello -------------------------------------------------------
        HelloParams hello;
        hello.role = "editor";
        hello.clientName = "NorvesEditor";
        hello.protocolVersions = {"0.1"};
        client->send(request_frame("req-hello", "bridge.hello", hello.to_json().dump()));

        {
            std::optional<std::string> resp = client->recv();
            NORVES_CHECK(resp.has_value());
            if (resp.has_value())
            {
                const Envelope env = decode_or_fail(*resp);
                NORVES_CHECK(env.kind == Kind::Response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-hello"});
                NORVES_CHECK(env.result.has_value());
                if (env.result.has_value())
                {
                    auto parsed = HelloResult::from_json(*env.result);
                    NORVES_CHECK(parsed.is_ok());
                    if (parsed.is_ok())
                    {
                        const HelloResult& r = parsed.value();
                        NORVES_CHECK_EQ(r.sessionId, std::string{"sess-mock-1"});
                        NORVES_CHECK_EQ(r.protocolVersion, std::string{"0.1"});
                        NORVES_CHECK_EQ(r.server.name, std::string{"MockEngine"});
                        NORVES_CHECK_EQ(r.server.engine, std::optional<std::string>{"mock"});
                    }
                }
            }
        }

        // 2. engine.getStatus ---------------------------------------------------
        client->send(request_frame("req-status", "engine.getStatus", ""));
        {
            std::optional<std::string> resp = client->recv();
            NORVES_CHECK(resp.has_value());
            if (resp.has_value())
            {
                const Envelope env = decode_or_fail(*resp);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-status"});
                NORVES_CHECK(env.result.has_value());
                if (env.result.has_value())
                {
                    auto parsed = StatusSnapshot::from_json(*env.result);
                    NORVES_CHECK(parsed.is_ok());
                    if (parsed.is_ok())
                    {
                        const StatusSnapshot& s = parsed.value();
                        NORVES_CHECK(s.engineState == EngineState::Ready);
                        NORVES_CHECK(s.runtimeState == RuntimeState::Edit);
                        NORVES_CHECK_EQ(s.engineName, std::optional<std::string>{"MockEngine"});
                        NORVES_CHECK_EQ(s.title, std::optional<std::string>{"Mock Game"});
                    }
                }
            }
        }

        // 3. runtime.play (empty params) ----------------------------------------
        client->send(request_frame("req-play", "runtime.play", "{}"));
        {
            std::optional<std::string> resp = client->recv();
            NORVES_CHECK(resp.has_value());
            if (resp.has_value())
            {
                const Envelope env = decode_or_fail(*resp);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-play"});
                NORVES_CHECK(env.result.has_value());
                if (env.result.has_value())
                {
                    auto parsed = PlayAck::from_json(*env.result);
                    NORVES_CHECK(parsed.is_ok());
                    if (parsed.is_ok())
                    {
                        const PlayAck& a = parsed.value();
                        NORVES_CHECK(a.accepted);
                        NORVES_CHECK(a.requestedState.has_value() &&
                                     *a.requestedState == RuntimeState::Playing);
                    }
                }
            }
        }

        // 4. log.message event --------------------------------------------------
        // Subscribe, then the engine emits one event after acking. The ack and the
        // event both arrive on the client's inbound queue, in that order.
        emit_log.store(true);
        client->send(request_frame("req-logsub", "log.subscribe", ""));
        {
            std::optional<std::string> ack = client->recv();
            NORVES_CHECK(ack.has_value());  // the log.subscribe response.
            if (ack.has_value())
            {
                const Envelope env = decode_or_fail(*ack);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-logsub"});
                NORVES_CHECK(env.result.has_value());
            }

            std::optional<std::string> event = client->recv();
            NORVES_CHECK(event.has_value());  // the log.message event.
            if (event.has_value())
            {
                const Envelope env = decode_or_fail(*event);
                NORVES_CHECK(env.kind == Kind::Event);
                NORVES_CHECK_EQ(env.event, std::optional<std::string>{"log.message"});
                NORVES_CHECK(env.params.has_value());
                if (env.params.has_value())
                {
                    auto parsed = LogMessageEvent::from_json(*env.params);
                    NORVES_CHECK(parsed.is_ok());
                    if (parsed.is_ok())
                    {
                        const LogMessageEvent& e = parsed.value();
                        NORVES_CHECK(e.level == LogLevel::Info);
                        NORVES_CHECK_EQ(e.message, std::string{"Game started"});
                    }
                }
            }
        }

        // Orderly teardown: close the client's outbound direction so the engine's
        // recv() drains and returns nullopt, ending its loop; then join.
        client->close();
        engine_thread.join();
    }

    // --- DTO round-trip + unknown-key rejection ----------------------------------

    void test_dto_round_trips()
    {
        {
            HelloParams x;
            x.role = "editor";
            x.clientName = "NorvesEditor";
            x.clientVersion = "0.9.0";
            x.protocolVersions = {"0.1", "1.0"};
            auto back = HelloParams::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            HelloResult x;
            x.sessionId = "s-1";
            x.protocolVersion = "0.1";
            x.server =
                ServerInfo{"E", std::optional<std::string>{"1.2"}, std::optional<std::string>{"e"}};
            auto back = HelloResult::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            ServerInfo x{"OnlyName", std::nullopt, std::nullopt};
            auto back = ServerInfo::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            StatusSnapshot x;
            x.engineState = EngineState::Running;
            x.runtimeState = RuntimeState::Paused;
            x.engineName = "E";
            // engineVersion / title left unset to exercise omit-on-absent.
            auto back = StatusSnapshot::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            PlayAck x;
            x.accepted = true;
            x.requestedState = RuntimeState::Playing;
            auto back = PlayAck::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            PlayAck x;  // requestedState unset.
            x.accepted = false;
            auto back = PlayAck::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            LogMessageEvent x;
            x.level = LogLevel::Warn;
            x.message = "careful";
            x.category = "Render";
            x.timestamp = "2026-01-01T00:00:00Z";
            auto back = LogMessageEvent::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
    }

    void test_unknown_key_rejected()
    {
        // Top-level unknown key.
        {
            auto bad = HelloResult::from_json(parse_or_fail(
                R"({"sessionId":"s","protocolVersion":"0.1","server":{"name":"E"},"extra":1})"));
            NORVES_CHECK(bad.is_err());
        }
        // Unknown key in the nested `server` object (recursive additionalProperties).
        {
            auto bad = HelloResult::from_json(parse_or_fail(
                R"({"sessionId":"s","protocolVersion":"0.1","server":{"name":"E","rogue":true}})"));
            NORVES_CHECK(bad.is_err());
        }
        // Unknown key in params.
        {
            auto bad = HelloParams::from_json(parse_or_fail(
                R"({"role":"editor","clientName":"N","protocolVersions":["0.1"],"caps":[]})"));
            NORVES_CHECK(bad.is_err());
        }
        // Unknown key in status snapshot.
        {
            auto bad = StatusSnapshot::from_json(
                parse_or_fail(R"({"engineState":"ready","runtimeState":"edit","weird":0})"));
            NORVES_CHECK(bad.is_err());
        }
        // Unknown key in log event.
        {
            auto bad = LogMessageEvent::from_json(
                parse_or_fail(R"({"level":"info","message":"m","mystery":1})"));
            NORVES_CHECK(bad.is_err());
        }
        // Required-field omission is also rejected.
        {
            auto bad = StatusSnapshot::from_json(parse_or_fail(R"({"engineState":"ready"})"));
            NORVES_CHECK(bad.is_err());
        }
        // Out-of-enum value is rejected.
        {
            auto bad = StatusSnapshot::from_json(
                parse_or_fail(R"({"engineState":"booting","runtimeState":"edit"})"));
            NORVES_CHECK(bad.is_err());
        }
    }

}  // namespace

int main()
{
    test_loopback_round_trip();
    test_dto_round_trips();
    test_unknown_key_rejected();
    return norves::test::summary();
}
