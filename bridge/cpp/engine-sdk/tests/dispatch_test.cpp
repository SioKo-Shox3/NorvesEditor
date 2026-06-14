// Dispatch / negotiation conformance test for BridgeEngineServer (F3).
//
// Verifies: bridge.hello version negotiation (success + PROTOCOL_VERSION_
// UNSUPPORTED), id / sessionId echo, adapter result pass-through, unknown method
// and unimplemented-optional-method -> METHOD_NOT_SUPPORTED, event emission, and
// that non-request / undecodable frames produce no response.
//
// Only std + the SDK's public headers are used (plus JsonValue::parse for
// value-equal comparison); the boundary rule (no nlohmann in include/) is
// unaffected.

#include <optional>
#include <string>
#include <string_view>

#include "norves/bridge/adapter.hpp"
#include "norves/bridge/codec.hpp"
#include "norves/bridge/envelope.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"
#include "norves/bridge/server.hpp"
#include "norves/bridge/version.hpp"
#include "test_support.hpp"

namespace {

using norves::bridge::BridgeError;
using norves::bridge::BridgeEngineServer;
using norves::bridge::CodecError;
using norves::bridge::decode_envelope;
using norves::bridge::Envelope;
using norves::bridge::IBridgeEngineAdapter;
using norves::bridge::JsonValue;
using norves::bridge::Kind;
using norves::bridge::Result;

// Builds a JsonValue from JSON text, failing the test (and returning null) on a
// parse error so callers can use the value inline.
JsonValue parse_or_fail(std::string_view text) {
    auto parsed = JsonValue::parse(text);
    if (parsed.is_err()) {
        ::norves::test::report_failure("JsonValue::parse failed", __FILE__, __LINE__);
        return JsonValue();
    }
    return std::move(parsed).value();
}

// Fake adapter: returns fixed JsonValue results so the test exercises the
// server's dispatch / negotiation, not real engine logic. It does NOT override
// the optional methods, so those fall through to the default
// METHOD_NOT_SUPPORTED.
class FakeAdapter : public IBridgeEngineAdapter {
  public:
    Result<JsonValue, BridgeError> hello(const JsonValue& /*params*/,
                                         std::string_view selectedProtocolVersion) override {
        // The adapter is responsible for placing the negotiated version into the
        // result's protocolVersion field.
        std::string result = std::string(R"({"sessionId":"sess-7f3a","protocolVersion":")") +
                             std::string(selectedProtocolVersion) +
                             R"(","server":{"name":"FakeEngine","version":"0.1.0","engine":"fake"}})";
        return Result<JsonValue, BridgeError>::ok(parse_or_fail(result));
    }

    Result<JsonValue, BridgeError> getCapabilities(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(
            parse_or_fail(R"({"capabilities":[{"name":"runtime.control"}]})"));
    }

    Result<JsonValue, BridgeError> getStatus(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({"engineState":"ready"})"));
    }

    Result<JsonValue, BridgeError> launchInfo(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({"launched":true})"));
    }

    Result<JsonValue, BridgeError> runtimePlay(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({"runtimeState":"playing"})"));
    }

    Result<JsonValue, BridgeError> runtimePause(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({"runtimeState":"paused"})"));
    }

    Result<JsonValue, BridgeError> runtimeStop(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({"runtimeState":"stopped"})"));
    }

    Result<JsonValue, BridgeError> runtimeFocusViewport(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({"focused":true})"));
    }

    Result<JsonValue, BridgeError> logSubscribe(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({"subscribed":true})"));
    }

    Result<JsonValue, BridgeError> logUnsubscribe(const JsonValue& /*params*/) override {
        return Result<JsonValue, BridgeError>::ok(parse_or_fail(R"({"subscribed":false})"));
    }
    // Optional methods intentionally NOT overridden.
};

// Wire-frame builders ---------------------------------------------------------

std::string request_frame(std::string_view id, std::string_view method,
                          std::string_view params_json) {
    std::string frame = R"({"bridge":"norves.editor.bridge","version":"0.1","kind":"request","id":")";
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

// Decodes a server response frame, failing the test on error.
Envelope decode_or_fail(std::string_view wire) {
    auto decoded = decode_envelope(wire);
    if (decoded.is_err()) {
        ::norves::test::report_failure("decode_envelope of response failed", __FILE__, __LINE__);
        return Envelope();
    }
    return std::move(decoded).value();
}

// Tests -----------------------------------------------------------------------

void test_hello_success_echoes_id_session_and_version() {
    FakeAdapter adapter;
    BridgeEngineServer server(adapter);

    const std::string frame = request_frame(
        "req-1", "bridge.hello",
        R"({"role":"editor","clientName":"NorvesEditor","protocolVersions":["0.1"]})");
    auto response = server.handleFrame(frame);
    NORVES_CHECK(response.has_value());
    if (!response.has_value()) {
        return;
    }

    const Envelope env = decode_or_fail(*response);
    NORVES_CHECK(env.kind == Kind::Response);
    NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-1"});
    NORVES_CHECK(env.result.has_value());
    NORVES_CHECK(!env.error.has_value());
    // Envelope sessionId is echoed from the result's sessionId.
    NORVES_CHECK_EQ(env.session_id, std::optional<std::string>{"sess-7f3a"});

    // Result payload carries the negotiated version and server identity.
    const JsonValue expected = parse_or_fail(
        R"({"sessionId":"sess-7f3a","protocolVersion":"0.1","server":{"name":"FakeEngine","version":"0.1.0","engine":"fake"}})");
    NORVES_CHECK(env.result.has_value() && *env.result == expected);
}

void test_hello_version_unsupported() {
    FakeAdapter adapter;
    BridgeEngineServer server(adapter);

    const std::string frame = request_frame(
        "req-1", "bridge.hello",
        R"({"role":"editor","clientName":"NorvesEditor","protocolVersions":["2.0"]})");
    auto response = server.handleFrame(frame);
    NORVES_CHECK(response.has_value());
    if (!response.has_value()) {
        return;
    }

    const Envelope env = decode_or_fail(*response);
    NORVES_CHECK(env.kind == Kind::Response);
    NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-1"});
    NORVES_CHECK(env.error.has_value());
    NORVES_CHECK(!env.result.has_value());
    if (!env.error.has_value()) {
        return;
    }
    NORVES_CHECK_EQ(env.error->code, std::string{"PROTOCOL_VERSION_UNSUPPORTED"});
    NORVES_CHECK(!env.error->message.empty());

    // error.data: offered is what the client sent; supported is
    // kSupportedProtocolVersions ("0.1" for the alpha). The canonical fixture
    // (response-version-unsupported.json) uses offered:["2.0"],
    // supported:["0.1","1.0"]; supported tracks the SDK's actual set, so we
    // assert structure + offered + supported contents directly.
    NORVES_CHECK(env.error->data.has_value());
    if (env.error->data.has_value()) {
        const JsonValue expected_data = parse_or_fail(R"({"offered":["2.0"],"supported":["0.1"]})");
        NORVES_CHECK(*env.error->data == expected_data);
    }
}

void test_known_method_passes_adapter_result() {
    FakeAdapter adapter;
    BridgeEngineServer server(adapter);

    {
        const std::string frame = request_frame("s-1", "engine.getStatus", "");
        auto response = server.handleFrame(frame);
        NORVES_CHECK(response.has_value());
        if (response.has_value()) {
            const Envelope env = decode_or_fail(*response);
            NORVES_CHECK_EQ(env.id, std::optional<std::string>{"s-1"});
            NORVES_CHECK(env.result.has_value());
            const JsonValue expected = parse_or_fail(R"({"engineState":"ready"})");
            NORVES_CHECK(env.result.has_value() && *env.result == expected);
        }
    }

    {
        const std::string frame = request_frame("p-1", "runtime.play", "");
        auto response = server.handleFrame(frame);
        NORVES_CHECK(response.has_value());
        if (response.has_value()) {
            const Envelope env = decode_or_fail(*response);
            NORVES_CHECK_EQ(env.id, std::optional<std::string>{"p-1"});
            const JsonValue expected = parse_or_fail(R"({"runtimeState":"playing"})");
            NORVES_CHECK(env.result.has_value() && *env.result == expected);
        }
    }
}

void test_unknown_method_is_method_not_supported() {
    FakeAdapter adapter;
    BridgeEngineServer server(adapter);

    const std::string frame = request_frame("u-1", "foo.bar", "");
    auto response = server.handleFrame(frame);
    NORVES_CHECK(response.has_value());
    if (!response.has_value()) {
        return;
    }
    const Envelope env = decode_or_fail(*response);
    NORVES_CHECK_EQ(env.id, std::optional<std::string>{"u-1"});
    NORVES_CHECK(env.error.has_value());
    if (env.error.has_value()) {
        NORVES_CHECK_EQ(env.error->code, std::string{"METHOD_NOT_SUPPORTED"});
    }
}

void test_unimplemented_optional_method_is_method_not_supported() {
    FakeAdapter adapter;  // does not override scene.getTree
    BridgeEngineServer server(adapter);

    const std::string frame = request_frame("o-1", "scene.getTree", "");
    auto response = server.handleFrame(frame);
    NORVES_CHECK(response.has_value());
    if (!response.has_value()) {
        return;
    }
    const Envelope env = decode_or_fail(*response);
    NORVES_CHECK_EQ(env.id, std::optional<std::string>{"o-1"});
    NORVES_CHECK(env.error.has_value());
    if (env.error.has_value()) {
        NORVES_CHECK_EQ(env.error->code, std::string{"METHOD_NOT_SUPPORTED"});
    }
}

void test_emit_event_round_trips() {
    FakeAdapter adapter;
    BridgeEngineServer server(adapter);

    const JsonValue params = parse_or_fail(R"({"level":"info","message":"hello"})");
    const std::string wire = server.emitEvent("log.message", params);
    NORVES_CHECK(!wire.empty());

    const Envelope env = decode_or_fail(wire);
    NORVES_CHECK(env.kind == Kind::Event);
    NORVES_CHECK_EQ(env.event, std::optional<std::string>{"log.message"});
    NORVES_CHECK(env.params.has_value() && *env.params == params);
}

// Sanity check on JsonValue itself: a non-trivial value survives a
// parse -> dump -> parse round trip, value-equal to the original. This guards
// the codec the dispatch path relies on for every result/error/event payload.
void test_json_value_parse_dump_round_trips() {
    constexpr std::string_view kSource = R"({"a":1,"b":[true,null,"x"],"c":{"d":2.5}})";

    auto first = JsonValue::parse(kSource);
    NORVES_CHECK(first.is_ok());
    if (first.is_err()) {
        return;
    }
    const JsonValue original = std::move(first).value();

    auto second = JsonValue::parse(original.dump());
    NORVES_CHECK(second.is_ok());
    if (second.is_err()) {
        return;
    }
    const JsonValue reparsed = std::move(second).value();

    NORVES_CHECK(reparsed == original);
}

void test_non_request_frame_returns_nullopt() {
    FakeAdapter adapter;
    BridgeEngineServer server(adapter);

    // A valid response frame fed to the server: not ours to answer.
    const std::string response_frame =
        R"({"bridge":"norves.editor.bridge","version":"0.1","kind":"response","id":"req-1","result":{"ok":true}})";
    auto out = server.handleFrame(response_frame);
    NORVES_CHECK(!out.has_value());
}

void test_undecodable_frame_returns_nullopt() {
    FakeAdapter adapter;
    BridgeEngineServer server(adapter);

    auto out = server.handleFrame("{ this is not valid json");
    NORVES_CHECK(!out.has_value());
}

}  // namespace

int main() {
    test_hello_success_echoes_id_session_and_version();
    test_hello_version_unsupported();
    test_known_method_passes_adapter_result();
    test_unknown_method_is_method_not_supported();
    test_unimplemented_optional_method_is_method_not_supported();
    test_emit_event_round_trips();
    test_json_value_parse_dump_round_trips();
    test_non_request_frame_returns_nullopt();
    test_undecodable_frame_returns_nullopt();
    return norves::test::summary();
}
