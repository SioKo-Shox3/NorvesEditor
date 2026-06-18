#include "norves/bridge/server.hpp"

#include "norves/bridge/adapter.hpp"
#include "norves/bridge/codec.hpp"
#include "norves/bridge/envelope.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/log_sink.hpp"
#include "norves/bridge/result.hpp"
#include "norves/bridge/version.hpp"

#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "json_value_impl.hpp"

// BridgeEngineServer implementation. nlohmann/json is confined to this TU; the
// public server.hpp exposes only std + SDK value types.
//
// Responsibilities:
//   * decode an inbound wire frame (via codec.hpp),
//   * own the bridge.hello protocol-version negotiation,
//   * dispatch other known requests into IBridgeEngineAdapter,
//   * encode the response (result or wire error), echoing the request id.
namespace norves::bridge
{

    namespace
    {

        using nlohmann::json;

        // Wraps a concrete nlohmann::json into an opaque JsonValue (same helper shape as
        // codec.cpp; nlohmann stays inside this TU).
        JsonValue Wrap(json value)
        {
            auto impl = std::make_unique<detail::JsonValueImpl>(std::move(value));
            return make_json_value(std::move(impl));
        }

        // The wire protocol version this SDK speaks on envelopes. Negotiation selects a
        // supported version for the handshake result; the envelope version itself is the
        // SDK's single supported version (SupportedProtocolVersions[0] for the alpha).
        std::string EnvelopeVersion() { return std::string(SupportedProtocolVersions.front()); }

        // Builds a response Envelope carrying a successful result, echoing `id`. If the
        // result payload is a JSON object containing a string "sessionId", that value is
        // also echoed at the envelope level (matching response-valid.json, where the
        // envelope sessionId mirrors result.sessionId).
        Envelope MakeResultResponse(const std::string& id, JsonValue result)
        {
            Envelope env;
            env.bridge = std::string(BridgeMarker);
            env.version = EnvelopeVersion();
            env.kind = Kind::Response;
            env.id = id;

            const json& payload = peek(result)->json;
            if (payload.is_object())
            {
                const auto it = payload.find("sessionId");
                if (it != payload.end() && it->is_string())
                {
                    env.session_id = it->get<std::string>();
                }
            }

            env.result = std::move(result);
            return env;
        }

        // Builds a response Envelope carrying a wire error, echoing `id`.
        Envelope MakeErrorResponse(const std::string& id, BridgeError error)
        {
            Envelope env;
            env.bridge = std::string(BridgeMarker);
            env.version = EnvelopeVersion();
            env.kind = Kind::Response;
            env.id = id;
            env.error = std::move(error);
            return env;
        }

        // Extracts params["protocolVersions"] as a list of strings in client preference
        // order. Non-string elements are skipped; absence / non-array yields an empty
        // list (negotiation then necessarily fails as unsupported).
        std::vector<std::string> OfferedVersions(const JsonValue& params)
        {
            std::vector<std::string> offered;
            const json& obj = peek(params)->json;
            if (!obj.is_object())
            {
                return offered;
            }
            const auto it = obj.find("protocolVersions");
            if (it == obj.end() || !it->is_array())
            {
                return offered;
            }
            for (const auto& element : *it)
            {
                if (element.is_string())
                {
                    offered.push_back(element.get<std::string>());
                }
            }
            return offered;
        }

        // Negotiation: the first offered version (client preference order) that is also
        // in SupportedProtocolVersions. std::nullopt if the intersection is empty.
        std::optional<std::string> NegotiateVersion(const std::vector<std::string>& offered)
        {
            for (const auto& candidate : offered)
            {
                for (const auto& supported : SupportedProtocolVersions)
                {
                    if (candidate == supported)
                    {
                        return candidate;
                    }
                }
            }
            return std::nullopt;
        }

        // Builds the PROTOCOL_VERSION_UNSUPPORTED error.data payload:
        //   { "offered": <client-offered array>, "supported": <SupportedProtocolVersions> }
        JsonValue VersionUnsupportedData(const std::vector<std::string>& offered)
        {
            json data = json::object();
            data["offered"] = offered;
            json supported = json::array();
            for (const auto& version : SupportedProtocolVersions)
            {
                supported.push_back(std::string(version));
            }
            data["supported"] = std::move(supported);
            return Wrap(std::move(data));
        }

    }  // namespace

    // --- Impl --------------------------------------------------------------------

    struct BridgeEngineServer::Impl
    {
        IBridgeEngineAdapter& adapter;
        ILogSink* log_sink;

        Impl(IBridgeEngineAdapter& adapterRef, ILogSink* sink) : adapter(adapterRef), log_sink(sink)
        {
        }

        void log(LogSeverity level, std::string_view message) const
        {
            if (log_sink != nullptr)
            {
                log_sink->log(level, message);
            }
        }

        // Routes a decoded request envelope to the right handler and returns the
        // response envelope.
        Envelope dispatch(const Envelope& request)
        {
            const std::string id = *request.id;  // Present: validated for requests.
            const std::string& method = *request.method;

            // The adapter contract takes `const JsonValue&`; supply a JSON-null
            // value when the request omitted params.
            const JsonValue emptyParams;
            const JsonValue& params = request.params.has_value() ? *request.params : emptyParams;

            if (method == "bridge.hello")
            {
                return handle_hello(id, params);
            }

            // Known method -> adapter dispatch. Each branch maps Ok->result,
            // Err->error, echoing the id.
            if (method == "bridge.getCapabilities")
            {
                return finish(id, adapter.getCapabilities(params));
            }
            if (method == "engine.getStatus")
            {
                return finish(id, adapter.getStatus(params));
            }
            if (method == "engine.launchInfo")
            {
                return finish(id, adapter.launchInfo(params));
            }
            if (method == "runtime.play")
            {
                return finish(id, adapter.runtimePlay(params));
            }
            if (method == "runtime.pause")
            {
                return finish(id, adapter.runtimePause(params));
            }
            if (method == "runtime.stop")
            {
                return finish(id, adapter.runtimeStop(params));
            }
            if (method == "runtime.focusViewport")
            {
                return finish(id, adapter.runtimeFocusViewport(params));
            }
            if (method == "log.subscribe")
            {
                return finish(id, adapter.logSubscribe(params));
            }
            if (method == "log.unsubscribe")
            {
                return finish(id, adapter.logUnsubscribe(params));
            }
            if (method == "scene.getTree")
            {
                return finish(id, adapter.sceneGetTree(params));
            }
            if (method == "object.getSnapshot")
            {
                return finish(id, adapter.objectGetSnapshot(params));
            }
            if (method == "object.setProperty")
            {
                return finish(id, adapter.objectSetProperty(params));
            }
            if (method == "schema.getSnapshot")
            {
                return finish(id, adapter.schemaGetSnapshot(params));
            }

            // Unknown method (not in the dispatch table) -> METHOD_NOT_SUPPORTED.
            log(LogSeverity::Debug, std::string("unknown method: ") + method);
            return MakeErrorResponse(
                id, BridgeError{std::string(ErrorMethodNotSupported),
                                std::string("Unknown method: ") + method, std::nullopt});
        }

        // bridge.hello: server-owned version negotiation, then delegate to the
        // adapter for the result payload.
        Envelope handle_hello(const std::string& id, const JsonValue& params)
        {
            const std::vector<std::string> offered = OfferedVersions(params);
            const std::optional<std::string> selected = NegotiateVersion(offered);
            if (!selected.has_value())
            {
                log(LogSeverity::Warn, "bridge.hello: no offered protocol version is supported");
                return MakeErrorResponse(
                    id, BridgeError{std::string(ErrorProtocolVersionUnsupported),
                                    "None of the offered protocol versions are supported by this "
                                    "engine.",
                                    VersionUnsupportedData(offered)});
            }
            return finish(id, adapter.hello(params, *selected));
        }

        // Maps an adapter outcome onto a response envelope, echoing the id.
        Envelope finish(const std::string& id, Result<JsonValue, BridgeError> outcome)
        {
            if (outcome.is_ok())
            {
                return MakeResultResponse(id, std::move(outcome).value());
            }
            return MakeErrorResponse(id, std::move(outcome).error());
        }
    };

    // --- BridgeEngineServer ------------------------------------------------------

    BridgeEngineServer::BridgeEngineServer(IBridgeEngineAdapter& adapter, ILogSink* logSink)
        : m_Impl(std::make_unique<Impl>(adapter, logSink))
    {
    }

    BridgeEngineServer::~BridgeEngineServer() = default;

    BridgeEngineServer::BridgeEngineServer(BridgeEngineServer&&) noexcept = default;

    BridgeEngineServer& BridgeEngineServer::operator=(BridgeEngineServer&&) noexcept = default;

    std::optional<std::string> BridgeEngineServer::handleFrame(std::string_view wire)
    {
        auto decoded = decode_envelope(wire);
        if (decoded.is_err())
        {
            // No recoverable correlation id -> no valid response envelope can be
            // built. Report and drop the frame.
            m_Impl->log(LogSeverity::Warn,
                        std::string("dropping undecodable frame: ") + decoded.error().message);
            return std::nullopt;
        }

        const Envelope request = std::move(decoded).value();
        if (request.kind != Kind::Request)
        {
            // The server processes requests only; responses/events are not ours to
            // answer.
            m_Impl->log(LogSeverity::Debug, "ignoring non-request frame");
            return std::nullopt;
        }

        const Envelope response = m_Impl->dispatch(request);
        auto encoded = encode_envelope(response);
        if (encoded.is_err())
        {
            // Encoding a server-built envelope should not fail; if it does there is
            // nothing valid to send.
            m_Impl->log(LogSeverity::Error,
                        std::string("failed to encode response: ") + encoded.error().message);
            return std::nullopt;
        }
        return std::move(encoded).value();
    }

    std::string BridgeEngineServer::emitEvent(std::string_view eventName, const JsonValue& params)
    {
        Envelope env;
        env.bridge = std::string(BridgeMarker);
        env.version = EnvelopeVersion();
        env.kind = Kind::Event;
        env.event = std::string(eventName);
        env.params = params;

        auto encoded = encode_envelope(env);
        if (encoded.is_err())
        {
            m_Impl->log(LogSeverity::Error,
                        std::string("failed to encode event: ") + encoded.error().message);
            return std::string();
        }
        return std::move(encoded).value();
    }

}  // namespace norves::bridge
