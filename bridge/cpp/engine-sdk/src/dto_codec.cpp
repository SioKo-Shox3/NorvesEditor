#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/dto/events.hpp"
#include "norves/bridge/dto/methods.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"

#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "json_value_impl.hpp"

// Typed-DTO codec for the F5 round-trip. nlohmann/json is confined to this TU;
// the public dto/*.hpp headers expose only std + SDK value/enum types.
//
// Validation matches the F2 envelope codec's strictness and the per-payload
// schemas: additionalProperties:false is enforced recursively at every object
// layer, required fields and JSON types are checked, and enum strings outside
// the schema's set are rejected (mirrors serde's deny_unknown_fields plus the
// enum membership check in the Rust reference).
namespace norves::bridge::dto
{

    namespace
    {

        using nlohmann::json;

        template <typename T>
        Result<T, CodecError> fail(CodecError error)
        {
            return Result<T, CodecError>::err(std::move(error));
        }

        // Wraps a concrete nlohmann::json into an opaque JsonValue via the src-only
        // bridge helper (same pattern as codec.cpp).
        JsonValue wrap(json value)
        {
            auto impl = std::make_unique<detail::JsonValueImpl>();
            impl->json = std::move(value);
            return make_json_value(std::move(impl));
        }

        // Rejects any key not in `known`. Returns an unknown-field CodecError on the
        // first offending key (additionalProperties:false). `context` names the object
        // layer for the message (e.g. "bridge.hello.result" or its "server").
        std::optional<CodecError> reject_unknown_keys(const json& obj, const char* context,
                                                      std::initializer_list<std::string_view> known)
        {
            for (const auto& [key, _] : obj.items())
            {
                bool found = false;
                for (const auto& k : known)
                {
                    if (key == k)
                    {
                        found = true;
                        break;
                    }
                }
                if (!found)
                {
                    return CodecError::unknown_field(std::string("unknown field in `") + context +
                                                     "`: " + key);
                }
            }
            return std::nullopt;
        }

        // Reads a REQUIRED string field. Sets `out`; returns a CodecError on absence or
        // wrong type.
        std::optional<CodecError> required_string(const json& obj, const char* context,
                                                  const char* key, std::string& out)
        {
            const auto it = obj.find(key);
            if (it == obj.end())
            {
                return CodecError::invalid_field(std::string("`") + context + "` requires `" + key +
                                                 "`");
            }
            if (!it->is_string())
            {
                return CodecError::invalid_field(std::string("`") + context + "." + key +
                                                 "` must be a string");
            }
            out = it->get<std::string>();
            return std::nullopt;
        }

        // Reads an OPTIONAL string field. Leaves `out` unset when absent; returns a
        // CodecError when present with a non-string type.
        std::optional<CodecError> optional_string(const json& obj, const char* context,
                                                  const char* key, std::optional<std::string>& out)
        {
            const auto it = obj.find(key);
            if (it == obj.end())
            {
                return std::nullopt;
            }
            if (!it->is_string())
            {
                return CodecError::invalid_field(std::string("`") + context + "." + key +
                                                 "` must be a string");
            }
            out = it->get<std::string>();
            return std::nullopt;
        }

    }  // namespace

    // --- Enum wire conversions ---------------------------------------------------

    std::string_view to_wire(EngineState value)
    {
        switch (value)
        {
            case EngineState::Initializing:
                return "initializing";
            case EngineState::Ready:
                return "ready";
            case EngineState::Running:
                return "running";
            case EngineState::Error:
                return "error";
        }
        return "initializing";  // unreachable; keeps the function total for MSVC.
    }

    std::string_view to_wire(RuntimeState value)
    {
        switch (value)
        {
            case RuntimeState::Edit:
                return "edit";
            case RuntimeState::Playing:
                return "playing";
            case RuntimeState::Paused:
                return "paused";
            case RuntimeState::Stopped:
                return "stopped";
            case RuntimeState::Unknown:
                return "unknown";
        }
        return "unknown";  // unreachable.
    }

    std::string_view to_wire(LogLevel value)
    {
        switch (value)
        {
            case LogLevel::Trace:
                return "trace";
            case LogLevel::Debug:
                return "debug";
            case LogLevel::Info:
                return "info";
            case LogLevel::Warn:
                return "warn";
            case LogLevel::Error:
                return "error";
        }
        return "info";  // unreachable.
    }

    std::optional<EngineState> engine_state_from_wire(std::string_view text)
    {
        if (text == "initializing")
        {
            return EngineState::Initializing;
        }
        if (text == "ready")
        {
            return EngineState::Ready;
        }
        if (text == "running")
        {
            return EngineState::Running;
        }
        if (text == "error")
        {
            return EngineState::Error;
        }
        return std::nullopt;
    }

    std::optional<RuntimeState> runtime_state_from_wire(std::string_view text)
    {
        if (text == "edit")
        {
            return RuntimeState::Edit;
        }
        if (text == "playing")
        {
            return RuntimeState::Playing;
        }
        if (text == "paused")
        {
            return RuntimeState::Paused;
        }
        if (text == "stopped")
        {
            return RuntimeState::Stopped;
        }
        if (text == "unknown")
        {
            return RuntimeState::Unknown;
        }
        return std::nullopt;
    }

    std::optional<LogLevel> log_level_from_wire(std::string_view text)
    {
        if (text == "trace")
        {
            return LogLevel::Trace;
        }
        if (text == "debug")
        {
            return LogLevel::Debug;
        }
        if (text == "info")
        {
            return LogLevel::Info;
        }
        if (text == "warn")
        {
            return LogLevel::Warn;
        }
        if (text == "error")
        {
            return LogLevel::Error;
        }
        return std::nullopt;
    }

    // --- HelloParams -------------------------------------------------------------

    JsonValue HelloParams::to_json() const
    {
        json obj = json::object();
        obj["role"] = role;
        obj["clientName"] = clientName;
        if (clientVersion.has_value())
        {
            obj["clientVersion"] = *clientVersion;
        }
        obj["protocolVersions"] = protocolVersions;
        return wrap(std::move(obj));
    }

    Result<HelloParams, CodecError> HelloParams::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return fail<HelloParams>(
                CodecError::invalid_field("bridge.hello.params must be an object"));
        }
        if (auto e =
                reject_unknown_keys(obj, "bridge.hello.params",
                                    {"role", "clientName", "clientVersion", "protocolVersions"}))
        {
            return fail<HelloParams>(std::move(*e));
        }

        HelloParams out;
        if (auto e = required_string(obj, "bridge.hello.params", "role", out.role))
        {
            return fail<HelloParams>(std::move(*e));
        }
        if (auto e = required_string(obj, "bridge.hello.params", "clientName", out.clientName))
        {
            return fail<HelloParams>(std::move(*e));
        }
        if (auto e =
                optional_string(obj, "bridge.hello.params", "clientVersion", out.clientVersion))
        {
            return fail<HelloParams>(std::move(*e));
        }

        const auto pv = obj.find("protocolVersions");
        if (pv == obj.end())
        {
            return fail<HelloParams>(
                CodecError::invalid_field("`bridge.hello.params` requires `protocolVersions`"));
        }
        if (!pv->is_array())
        {
            return fail<HelloParams>(CodecError::invalid_field(
                "`bridge.hello.params.protocolVersions` must be an array"));
        }
        for (const auto& item : *pv)
        {
            if (!item.is_string())
            {
                return fail<HelloParams>(CodecError::invalid_field(
                    "`bridge.hello.params.protocolVersions` items must be strings"));
            }
            out.protocolVersions.push_back(item.get<std::string>());
        }
        return Result<HelloParams, CodecError>::ok(std::move(out));
    }

    // --- ServerInfo --------------------------------------------------------------

    JsonValue ServerInfo::to_json() const
    {
        json obj = json::object();
        obj["name"] = name;
        if (version.has_value())
        {
            obj["version"] = *version;
        }
        if (engine.has_value())
        {
            obj["engine"] = *engine;
        }
        return wrap(std::move(obj));
    }

    Result<ServerInfo, CodecError> ServerInfo::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return fail<ServerInfo>(
                CodecError::invalid_field("bridge.hello.result.server must be an object"));
        }
        if (auto e = reject_unknown_keys(obj, "bridge.hello.result.server",
                                         {"name", "version", "engine"}))
        {
            return fail<ServerInfo>(std::move(*e));
        }

        ServerInfo out;
        if (auto e = required_string(obj, "bridge.hello.result.server", "name", out.name))
        {
            return fail<ServerInfo>(std::move(*e));
        }
        if (auto e = optional_string(obj, "bridge.hello.result.server", "version", out.version))
        {
            return fail<ServerInfo>(std::move(*e));
        }
        if (auto e = optional_string(obj, "bridge.hello.result.server", "engine", out.engine))
        {
            return fail<ServerInfo>(std::move(*e));
        }
        return Result<ServerInfo, CodecError>::ok(std::move(out));
    }

    // --- HelloResult -------------------------------------------------------------

    JsonValue HelloResult::to_json() const
    {
        json obj = json::object();
        obj["sessionId"] = sessionId;
        obj["protocolVersion"] = protocolVersion;
        obj["server"] = peek(server.to_json())->json;
        return wrap(std::move(obj));
    }

    Result<HelloResult, CodecError> HelloResult::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return fail<HelloResult>(
                CodecError::invalid_field("bridge.hello.result must be an object"));
        }
        if (auto e = reject_unknown_keys(obj, "bridge.hello.result",
                                         {"sessionId", "protocolVersion", "server"}))
        {
            return fail<HelloResult>(std::move(*e));
        }

        HelloResult out;
        if (auto e = required_string(obj, "bridge.hello.result", "sessionId", out.sessionId))
        {
            return fail<HelloResult>(std::move(*e));
        }
        if (auto e =
                required_string(obj, "bridge.hello.result", "protocolVersion", out.protocolVersion))
        {
            return fail<HelloResult>(std::move(*e));
        }

        const auto server_it = obj.find("server");
        if (server_it == obj.end())
        {
            return fail<HelloResult>(
                CodecError::invalid_field("`bridge.hello.result` requires `server`"));
        }
        auto server = ServerInfo::from_json(wrap(*server_it));
        if (server.is_err())
        {
            return fail<HelloResult>(std::move(server).error());
        }
        out.server = std::move(server).value();
        return Result<HelloResult, CodecError>::ok(std::move(out));
    }

    // --- StatusSnapshot ----------------------------------------------------------

    JsonValue StatusSnapshot::to_json() const
    {
        json obj = json::object();
        obj["engineState"] = std::string(to_wire(engineState));
        obj["runtimeState"] = std::string(to_wire(runtimeState));
        if (engineName.has_value())
        {
            obj["engineName"] = *engineName;
        }
        if (engineVersion.has_value())
        {
            obj["engineVersion"] = *engineVersion;
        }
        if (title.has_value())
        {
            obj["title"] = *title;
        }
        return wrap(std::move(obj));
    }

    Result<StatusSnapshot, CodecError> StatusSnapshot::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return fail<StatusSnapshot>(
                CodecError::invalid_field("engine.getStatus.result must be an object"));
        }
        if (auto e = reject_unknown_keys(
                obj, "engine.getStatus.result",
                {"engineState", "runtimeState", "engineName", "engineVersion", "title"}))
        {
            return fail<StatusSnapshot>(std::move(*e));
        }

        StatusSnapshot out;

        std::string engine_state;
        if (auto e = required_string(obj, "engine.getStatus.result", "engineState", engine_state))
        {
            return fail<StatusSnapshot>(std::move(*e));
        }
        const auto es = engine_state_from_wire(engine_state);
        if (!es.has_value())
        {
            return fail<StatusSnapshot>(
                CodecError::invalid_field(std::string("invalid engineState: ") + engine_state));
        }
        out.engineState = *es;

        std::string runtime_state;
        if (auto e = required_string(obj, "engine.getStatus.result", "runtimeState", runtime_state))
        {
            return fail<StatusSnapshot>(std::move(*e));
        }
        const auto rs = runtime_state_from_wire(runtime_state);
        if (!rs.has_value())
        {
            return fail<StatusSnapshot>(
                CodecError::invalid_field(std::string("invalid runtimeState: ") + runtime_state));
        }
        out.runtimeState = *rs;

        if (auto e = optional_string(obj, "engine.getStatus.result", "engineName", out.engineName))
        {
            return fail<StatusSnapshot>(std::move(*e));
        }
        if (auto e =
                optional_string(obj, "engine.getStatus.result", "engineVersion", out.engineVersion))
        {
            return fail<StatusSnapshot>(std::move(*e));
        }
        if (auto e = optional_string(obj, "engine.getStatus.result", "title", out.title))
        {
            return fail<StatusSnapshot>(std::move(*e));
        }
        return Result<StatusSnapshot, CodecError>::ok(std::move(out));
    }

    // --- PlayAck -----------------------------------------------------------------

    JsonValue PlayAck::to_json() const
    {
        json obj = json::object();
        obj["accepted"] = accepted;
        if (requestedState.has_value())
        {
            obj["requestedState"] = std::string(to_wire(*requestedState));
        }
        return wrap(std::move(obj));
    }

    Result<PlayAck, CodecError> PlayAck::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return fail<PlayAck>(
                CodecError::invalid_field("runtime.play.result must be an object"));
        }
        if (auto e =
                reject_unknown_keys(obj, "runtime.play.result", {"accepted", "requestedState"}))
        {
            return fail<PlayAck>(std::move(*e));
        }

        PlayAck out;
        const auto accepted_it = obj.find("accepted");
        if (accepted_it == obj.end())
        {
            return fail<PlayAck>(
                CodecError::invalid_field("`runtime.play.result` requires `accepted`"));
        }
        if (!accepted_it->is_boolean())
        {
            return fail<PlayAck>(
                CodecError::invalid_field("`runtime.play.result.accepted` must be a boolean"));
        }
        out.accepted = accepted_it->get<bool>();

        const auto state_it = obj.find("requestedState");
        if (state_it != obj.end())
        {
            if (!state_it->is_string())
            {
                return fail<PlayAck>(CodecError::invalid_field(
                    "`runtime.play.result.requestedState` must be a string"));
            }
            const auto rs = runtime_state_from_wire(state_it->get<std::string>());
            if (!rs.has_value())
            {
                return fail<PlayAck>(CodecError::invalid_field(
                    std::string("invalid requestedState: ") + state_it->get<std::string>()));
            }
            out.requestedState = *rs;
        }
        return Result<PlayAck, CodecError>::ok(std::move(out));
    }

    // --- LogMessageEvent ---------------------------------------------------------

    JsonValue LogMessageEvent::to_json() const
    {
        json obj = json::object();
        obj["level"] = std::string(to_wire(level));
        obj["message"] = message;
        if (category.has_value())
        {
            obj["category"] = *category;
        }
        if (timestamp.has_value())
        {
            obj["timestamp"] = *timestamp;
        }
        return wrap(std::move(obj));
    }

    Result<LogMessageEvent, CodecError> LogMessageEvent::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return fail<LogMessageEvent>(
                CodecError::invalid_field("log.message.params must be an object"));
        }
        if (auto e = reject_unknown_keys(obj, "log.message.params",
                                         {"level", "message", "category", "timestamp"}))
        {
            return fail<LogMessageEvent>(std::move(*e));
        }

        LogMessageEvent out;

        std::string level;
        if (auto e = required_string(obj, "log.message.params", "level", level))
        {
            return fail<LogMessageEvent>(std::move(*e));
        }
        const auto lvl = log_level_from_wire(level);
        if (!lvl.has_value())
        {
            return fail<LogMessageEvent>(
                CodecError::invalid_field(std::string("invalid log level: ") + level));
        }
        out.level = *lvl;

        if (auto e = required_string(obj, "log.message.params", "message", out.message))
        {
            return fail<LogMessageEvent>(std::move(*e));
        }
        if (auto e = optional_string(obj, "log.message.params", "category", out.category))
        {
            return fail<LogMessageEvent>(std::move(*e));
        }
        if (auto e = optional_string(obj, "log.message.params", "timestamp", out.timestamp))
        {
            return fail<LogMessageEvent>(std::move(*e));
        }
        return Result<LogMessageEvent, CodecError>::ok(std::move(out));
    }

}  // namespace norves::bridge::dto
