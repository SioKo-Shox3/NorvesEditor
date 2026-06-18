#include "norves/bridge/codec.hpp"

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/envelope.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"

#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>

#include <nlohmann/json.hpp>

#include "json_value_impl.hpp"

// JSON codec implementation. nlohmann/json is confined to this TU; the public
// codec.hpp exposes only std + SDK value types.
//
// The decode path mirrors the Rust reference's serde behaviour
// (bridge/crates/norves-bridge-core/src/{envelope,codec}.rs):
//   * deny_unknown_fields at the envelope object and at the error object,
//   * the bridge marker constant,
//   * validated newtype patterns (version, method/event names, error code,
//     non-empty id, seq >= 0),
//   * Envelope::validate for the kind-dependent cross-field rules.
// params / result / error.data are carried opaque (JsonValue), not inspected.
namespace norves::bridge
{

    namespace
    {

        using nlohmann::json;

        template <typename T>
        Result<T, CodecError> Err(CodecError error)
        {
            return Result<T, CodecError>::err(std::move(error));
        }

        // --- Pattern helpers (1:1 with common.rs / error.rs) -------------------------

        // ^[a-z][a-zA-Z0-9]*\.[a-zA-Z0-9]+$  (method / event names)
        bool IsNamespacedToken(std::string_view value)
        {
            const auto dot = value.find('.');
            if (dot == std::string_view::npos)
            {
                return false;
            }
            const std::string_view head = value.substr(0, dot);
            const std::string_view tail = value.substr(dot + 1);
            if (tail.find('.') != std::string_view::npos)
            {
                return false;
            }
            if (head.empty())
            {
                return false;
            }
            const char first = head.front();
            if (first < 'a' || first > 'z')
            {
                return false;
            }
            for (std::size_t i = 1; i < head.size(); ++i)
            {
                const char c = head[i];
                const bool bAlnum =
                    (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9');
                if (!bAlnum)
                {
                    return false;
                }
            }
            if (tail.empty())
            {
                return false;
            }
            for (const char c : tail)
            {
                const bool bAlnum =
                    (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9');
                if (!bAlnum)
                {
                    return false;
                }
            }
            return true;
        }

        // ^[0-9]+\.[0-9]+$  (version string)
        bool IsVersionString(std::string_view value)
        {
            const auto dot = value.find('.');
            if (dot == std::string_view::npos)
            {
                return false;
            }
            const std::string_view major = value.substr(0, dot);
            const std::string_view minor = value.substr(dot + 1);
            if (major.empty() || minor.empty())
            {
                return false;
            }
            if (minor.find('.') != std::string_view::npos)
            {
                return false;
            }
            for (const char c : major)
            {
                if (c < '0' || c > '9')
                {
                    return false;
                }
            }
            for (const char c : minor)
            {
                if (c < '0' || c > '9')
                {
                    return false;
                }
            }
            return true;
        }

        // ^[A-Z][A-Z0-9_]*$  (error code)
        bool IsErrorCode(std::string_view value)
        {
            if (value.empty())
            {
                return false;
            }
            const char first = value.front();
            if (first < 'A' || first > 'Z')
            {
                return false;
            }
            for (std::size_t i = 1; i < value.size(); ++i)
            {
                const char c = value[i];
                const bool bOk = (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_';
                if (!bOk)
                {
                    return false;
                }
            }
            return true;
        }

        // --- Field extraction --------------------------------------------------------

        JsonValue Wrap(const json& value)
        {
            auto impl = std::make_unique<detail::JsonValueImpl>();
            impl->json = value;
            return make_json_value(std::move(impl));
        }

        // Extracts an optional string field. Returns false (via *bad) if present but not
        // a string. Absent -> leaves out empty, returns true.
        bool TakeString(const json& obj, const char* key, std::optional<std::string>& out,
                        std::string& bad)
        {
            const auto it = obj.find(key);
            if (it == obj.end())
            {
                return true;
            }
            if (!it->is_string())
            {
                bad = std::string("field `") + key + "` must be a string";
                return false;
            }
            out = it->get<std::string>();
            return true;
        }

        // Decodes the error object (with its own additionalProperties: false and field
        // patterns). Mirrors envelope.schema.json#/$defs/error.
        Result<BridgeError, CodecError> DecodeError(const json& obj)
        {
            if (!obj.is_object())
            {
                return Err<BridgeError>(CodecError::invalid_field("`error` must be an object"));
            }
            // additionalProperties: false at the error object.
            for (const auto& [key, _] : obj.items())
            {
                if (key != "code" && key != "message" && key != "data")
                {
                    return Err<BridgeError>(
                        CodecError::unknown_field(std::string("unknown field in `error`: ") + key));
                }
            }
            const auto codeIt = obj.find("code");
            if (codeIt == obj.end())
            {
                return Err<BridgeError>(CodecError::invalid_field("`error` requires `code`"));
            }
            if (!codeIt->is_string())
            {
                return Err<BridgeError>(CodecError::invalid_field("`error.code` must be a string"));
            }
            const auto code = codeIt->get<std::string>();
            if (!IsErrorCode(code))
            {
                return Err<BridgeError>(
                    CodecError::invalid_field(std::string("invalid error code: ") + code));
            }
            const auto msgIt = obj.find("message");
            if (msgIt == obj.end())
            {
                return Err<BridgeError>(CodecError::invalid_field("`error` requires `message`"));
            }
            if (!msgIt->is_string())
            {
                return Err<BridgeError>(
                    CodecError::invalid_field("`error.message` must be a string"));
            }
            const auto message = msgIt->get<std::string>();
            // The JSON Schema (envelope.schema.json $defs/error.message minLength: 1) is
            // the canonical wire contract, so we reject an empty message. The Rust core
            // models this field as a plain String and does not enforce non-emptiness at
            // the type level; wire validation follows the schema, not the Rust type.
            if (message.empty())
            {
                return Err<BridgeError>(
                    CodecError::invalid_field("`error.message` must be non-empty"));
            }

            BridgeError out{code, message, std::nullopt};
            const auto dataIt = obj.find("data");
            if (dataIt != obj.end())
            {
                // data is opaque; preserved without interpretation.
                out.data = Wrap(*dataIt);
            }
            return Result<BridgeError, CodecError>::ok(std::move(out));
        }

    }  // namespace

    Result<Envelope, CodecError> decode_envelope(std::string_view wire)
    {
        json root = json::parse(wire, /*cb=*/nullptr, /*allow_exceptions=*/false);
        if (root.is_discarded())
        {
            return Err<Envelope>(CodecError::parse("malformed JSON"));
        }
        if (!root.is_object())
        {
            return Err<Envelope>(CodecError::parse("envelope must be a JSON object"));
        }

        // additionalProperties: false at the envelope object.
        static constexpr std::string_view Known[] = {"bridge", "version",   "kind",   "id",
                                                     "method", "event",     "params", "result",
                                                     "error",  "sessionId", "seq"};
        for (const auto& [key, _] : root.items())
        {
            bool bKnown = false;
            for (const auto& k : Known)
            {
                if (key == k)
                {
                    bKnown = true;
                    break;
                }
            }
            if (!bKnown)
            {
                return Err<Envelope>(
                    CodecError::unknown_field(std::string("unknown envelope field: ") + key));
            }
        }

        Envelope env;

        // bridge marker (constant).
        {
            const auto it = root.find("bridge");
            if (it == root.end())
            {
                return Err<Envelope>(CodecError::invalid_field("envelope requires `bridge`"));
            }
            if (!it->is_string())
            {
                return Err<Envelope>(CodecError::invalid_field("`bridge` must be a string"));
            }
            const auto marker = it->get<std::string>();
            if (marker != BridgeMarker)
            {
                return Err<Envelope>(
                    CodecError::invalid_field(std::string("invalid bridge marker: ") + marker));
            }
            env.bridge = marker;
        }

        // version (MAJOR.MINOR).
        {
            const auto it = root.find("version");
            if (it == root.end())
            {
                return Err<Envelope>(CodecError::invalid_field("envelope requires `version`"));
            }
            if (!it->is_string())
            {
                return Err<Envelope>(CodecError::invalid_field("`version` must be a string"));
            }
            const auto version = it->get<std::string>();
            if (!IsVersionString(version))
            {
                return Err<Envelope>(
                    CodecError::invalid_field(std::string("invalid version string: ") + version));
            }
            env.version = version;
        }

        // kind (enum).
        {
            const auto it = root.find("kind");
            if (it == root.end())
            {
                return Err<Envelope>(CodecError::invalid_field("envelope requires `kind`"));
            }
            if (!it->is_string())
            {
                return Err<Envelope>(CodecError::invalid_field("`kind` must be a string"));
            }
            const auto kind = it->get<std::string>();
            if (kind == "request")
            {
                env.kind = Kind::Request;
            }
            else if (kind == "response")
            {
                env.kind = Kind::Response;
            }
            else if (kind == "event")
            {
                env.kind = Kind::Event;
            }
            else
            {
                return Err<Envelope>(
                    CodecError::invalid_field(std::string("invalid kind: ") + kind));
            }
        }

        std::string bad;

        // id (non-empty string).
        if (!TakeString(root, "id", env.id, bad))
        {
            return Err<Envelope>(CodecError::invalid_field(bad));
        }
        if (env.id.has_value() && env.id->empty())
        {
            return Err<Envelope>(CodecError::invalid_field("`id` must be non-empty"));
        }

        // method (namespaced token).
        if (!TakeString(root, "method", env.method, bad))
        {
            return Err<Envelope>(CodecError::invalid_field(bad));
        }
        if (env.method.has_value() && !IsNamespacedToken(*env.method))
        {
            return Err<Envelope>(
                CodecError::invalid_field(std::string("invalid method name: ") + *env.method));
        }

        // event (namespaced token).
        if (!TakeString(root, "event", env.event, bad))
        {
            return Err<Envelope>(CodecError::invalid_field(bad));
        }
        if (env.event.has_value() && !IsNamespacedToken(*env.event))
        {
            return Err<Envelope>(
                CodecError::invalid_field(std::string("invalid event name: ") + *env.event));
        }

        // sessionId (non-empty string).
        if (!TakeString(root, "sessionId", env.session_id, bad))
        {
            return Err<Envelope>(CodecError::invalid_field(bad));
        }
        if (env.session_id.has_value() && env.session_id->empty())
        {
            return Err<Envelope>(CodecError::invalid_field("`sessionId` must be non-empty"));
        }

        // params (opaque object).
        {
            const auto it = root.find("params");
            if (it != root.end())
            {
                if (!it->is_object())
                {
                    return Err<Envelope>(CodecError::invalid_field("`params` must be an object"));
                }
                env.params = Wrap(*it);
            }
        }

        // result (opaque, any JSON value).
        {
            const auto it = root.find("result");
            if (it != root.end())
            {
                env.result = Wrap(*it);
            }
        }

        // error (structured object, opaque data).
        {
            const auto it = root.find("error");
            if (it != root.end())
            {
                auto decoded = DecodeError(*it);
                if (decoded.is_err())
                {
                    return Err<Envelope>(std::move(decoded).error());
                }
                env.error = std::move(decoded).value();
            }
        }

        // seq (integer >= 0). Mirrors the Rust reference's u64 and the schema's
        // integer minimum: 0. We decide signedness from nlohmann's number category
        // BEFORE calling get<std::uint64_t>(), because reading a negative number as
        // an unsigned integer would wrap around instead of being rejected.
        {
            const auto it = root.find("seq");
            if (it != root.end())
            {
                if (it->is_number_unsigned())
                {
                    env.seq = it->get<std::uint64_t>();
                }
                else if (it->is_number_integer())
                {
                    // A signed integer here is necessarily negative (a non-negative
                    // value would have been categorized as number_unsigned).
                    return Err<Envelope>(CodecError::invalid_field("`seq` must be >= 0"));
                }
                else
                {
                    return Err<Envelope>(CodecError::invalid_field("`seq` must be an integer"));
                }
            }
        }

        // Kind-dependent structural rules.
        auto validated = env.validate();
        if (validated.is_err())
        {
            return Err<Envelope>(std::move(validated).error());
        }

        return Result<Envelope, CodecError>::ok(std::move(env));
    }

    Result<std::string, CodecError> encode_envelope(const Envelope& envelope)
    {
        json root = json::object();
        root["bridge"] = envelope.bridge.empty() ? std::string(BridgeMarker) : envelope.bridge;
        root["version"] = envelope.version;
        switch (envelope.kind)
        {
            case Kind::Request:
                root["kind"] = "request";
                break;
            case Kind::Response:
                root["kind"] = "response";
                break;
            case Kind::Event:
                root["kind"] = "event";
                break;
        }
        if (envelope.id.has_value())
        {
            root["id"] = *envelope.id;
        }
        if (envelope.method.has_value())
        {
            root["method"] = *envelope.method;
        }
        if (envelope.event.has_value())
        {
            root["event"] = *envelope.event;
        }
        if (envelope.params.has_value())
        {
            root["params"] = peek(*envelope.params)->json;
        }
        if (envelope.result.has_value())
        {
            root["result"] = peek(*envelope.result)->json;
        }
        if (envelope.error.has_value())
        {
            json errorObj = json::object();
            errorObj["code"] = envelope.error->code;
            errorObj["message"] = envelope.error->message;
            if (envelope.error->data.has_value())
            {
                errorObj["data"] = peek(*envelope.error->data)->json;
            }
            root["error"] = std::move(errorObj);
        }
        if (envelope.session_id.has_value())
        {
            root["sessionId"] = *envelope.session_id;
        }
        if (envelope.seq.has_value())
        {
            root["seq"] = *envelope.seq;
        }

        return Result<std::string, CodecError>::ok(root.dump());
    }

}  // namespace norves::bridge
