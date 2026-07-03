#include "Norves/Bridge/codec_error.hpp"
#include "Norves/Bridge/Dto/common.hpp"
#include "Norves/Bridge/Dto/events.hpp"
#include "Norves/Bridge/Dto/methods.hpp"
#include "Norves/Bridge/json_value.hpp"
#include "Norves/Bridge/result.hpp"

#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "json_value_impl.hpp"

// F5 ラウンドトリップのための型付き DTO コーデック。nlohmann/json はこの TU に
// 閉じ込められる。公開の dto/*.hpp ヘッダは std + SDK 値/enum 型のみを露出する。
//
// 検証は F2 エンベロープコーデックの厳格さと、ペイロードごとのスキーマに一致する。
// すなわち additionalProperties:false がすべてのオブジェクト層で再帰的に強制され、
// 必須フィールドと JSON 型がチェックされ、スキーマの集合の外にある enum 文字列は拒否
// される（serde の deny_unknown_fields と、Rust リファレンスでの enum メンバシップ
// チェックを反映する）。
namespace Norves::Bridge::Dto
{

    namespace
    {

        using nlohmann::json;

        template <typename T>
        Result<T, CodecError> Fail(CodecError error)
        {
            return Result<T, CodecError>::err(std::move(error));
        }

        // src 専用のブリッジヘルパを介して、具体的な nlohmann::json を opaque な JsonValue へ
        // ラップする（codec.cpp と同じパターン）。
        JsonValue Wrap(json value)
        {
            auto impl = std::make_unique<Detail::JsonValueImpl>();
            impl->json = std::move(value);
            return make_json_value(std::move(impl));
        }

        // `known` にないキーをすべて拒否する。最初に違反したキーで unknown-field の
        // CodecError を返す（additionalProperties:false）。`context` はメッセージのために
        // オブジェクト層を名付ける（例: "bridge.hello.result" やその "server"）。
        std::optional<CodecError> RejectUnknownKeys(const json& obj, const char* context,
                                                    std::initializer_list<std::string_view> known)
        {
            for (const auto& [key, _] : obj.items())
            {
                bool bFound = false;
                for (const auto& k : known)
                {
                    if (key == k)
                    {
                        bFound = true;
                        break;
                    }
                }
                if (!bFound)
                {
                    return CodecError::unknown_field(std::string("unknown field in `") + context +
                                                     "`: " + key);
                }
            }
            return std::nullopt;
        }

        // 必須（REQUIRED）の文字列フィールドを読む。`out` を設定する。不在または誤った型の
        // 場合は CodecError を返す。
        std::optional<CodecError> RequiredString(const json& obj, const char* context,
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

        // オプション（OPTIONAL）の文字列フィールドを読む。不在のときは `out` を未設定の
        // ままにする。非文字列の型で存在するときは CodecError を返す。
        std::optional<CodecError> OptionalString(const json& obj, const char* context,
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

    // --- enum のワイヤー変換 -----------------------------------------------------

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
        return "initializing";  // 到達不能。MSVC のために関数を全域的に保つ。
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
        return "unknown";  // 到達不能。
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
        return "info";  // 到達不能。
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
        return Wrap(std::move(obj));
    }

    Result<HelloParams, CodecError> HelloParams::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return Fail<HelloParams>(
                CodecError::invalid_field("bridge.hello.params must be an object"));
        }
        if (auto e = RejectUnknownKeys(obj, "bridge.hello.params",
                                       {"role", "clientName", "clientVersion", "protocolVersions"}))
        {
            return Fail<HelloParams>(std::move(*e));
        }

        HelloParams out;
        if (auto e = RequiredString(obj, "bridge.hello.params", "role", out.role))
        {
            return Fail<HelloParams>(std::move(*e));
        }
        if (auto e = RequiredString(obj, "bridge.hello.params", "clientName", out.clientName))
        {
            return Fail<HelloParams>(std::move(*e));
        }
        if (auto e = OptionalString(obj, "bridge.hello.params", "clientVersion", out.clientVersion))
        {
            return Fail<HelloParams>(std::move(*e));
        }

        const auto pv = obj.find("protocolVersions");
        if (pv == obj.end())
        {
            return Fail<HelloParams>(
                CodecError::invalid_field("`bridge.hello.params` requires `protocolVersions`"));
        }
        if (!pv->is_array())
        {
            return Fail<HelloParams>(CodecError::invalid_field(
                "`bridge.hello.params.protocolVersions` must be an array"));
        }
        for (const auto& item : *pv)
        {
            if (!item.is_string())
            {
                return Fail<HelloParams>(CodecError::invalid_field(
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
        return Wrap(std::move(obj));
    }

    Result<ServerInfo, CodecError> ServerInfo::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return Fail<ServerInfo>(
                CodecError::invalid_field("bridge.hello.result.server must be an object"));
        }
        if (auto e =
                RejectUnknownKeys(obj, "bridge.hello.result.server", {"name", "version", "engine"}))
        {
            return Fail<ServerInfo>(std::move(*e));
        }

        ServerInfo out;
        if (auto e = RequiredString(obj, "bridge.hello.result.server", "name", out.name))
        {
            return Fail<ServerInfo>(std::move(*e));
        }
        if (auto e = OptionalString(obj, "bridge.hello.result.server", "version", out.version))
        {
            return Fail<ServerInfo>(std::move(*e));
        }
        if (auto e = OptionalString(obj, "bridge.hello.result.server", "engine", out.engine))
        {
            return Fail<ServerInfo>(std::move(*e));
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
        return Wrap(std::move(obj));
    }

    Result<HelloResult, CodecError> HelloResult::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return Fail<HelloResult>(
                CodecError::invalid_field("bridge.hello.result must be an object"));
        }
        if (auto e = RejectUnknownKeys(obj, "bridge.hello.result",
                                       {"sessionId", "protocolVersion", "server"}))
        {
            return Fail<HelloResult>(std::move(*e));
        }

        HelloResult out;
        if (auto e = RequiredString(obj, "bridge.hello.result", "sessionId", out.sessionId))
        {
            return Fail<HelloResult>(std::move(*e));
        }
        if (auto e =
                RequiredString(obj, "bridge.hello.result", "protocolVersion", out.protocolVersion))
        {
            return Fail<HelloResult>(std::move(*e));
        }

        const auto serverIt = obj.find("server");
        if (serverIt == obj.end())
        {
            return Fail<HelloResult>(
                CodecError::invalid_field("`bridge.hello.result` requires `server`"));
        }
        auto server = ServerInfo::from_json(Wrap(*serverIt));
        if (server.is_err())
        {
            return Fail<HelloResult>(std::move(server).error());
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
        return Wrap(std::move(obj));
    }

    Result<StatusSnapshot, CodecError> StatusSnapshot::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return Fail<StatusSnapshot>(
                CodecError::invalid_field("engine.getStatus.result must be an object"));
        }
        if (auto e = RejectUnknownKeys(
                obj, "engine.getStatus.result",
                {"engineState", "runtimeState", "engineName", "engineVersion", "title"}))
        {
            return Fail<StatusSnapshot>(std::move(*e));
        }

        StatusSnapshot out;

        std::string engineState;
        if (auto e = RequiredString(obj, "engine.getStatus.result", "engineState", engineState))
        {
            return Fail<StatusSnapshot>(std::move(*e));
        }
        const auto es = engine_state_from_wire(engineState);
        if (!es.has_value())
        {
            return Fail<StatusSnapshot>(
                CodecError::invalid_field(std::string("invalid engineState: ") + engineState));
        }
        out.engineState = *es;

        std::string runtimeState;
        if (auto e = RequiredString(obj, "engine.getStatus.result", "runtimeState", runtimeState))
        {
            return Fail<StatusSnapshot>(std::move(*e));
        }
        const auto rs = runtime_state_from_wire(runtimeState);
        if (!rs.has_value())
        {
            return Fail<StatusSnapshot>(
                CodecError::invalid_field(std::string("invalid runtimeState: ") + runtimeState));
        }
        out.runtimeState = *rs;

        if (auto e = OptionalString(obj, "engine.getStatus.result", "engineName", out.engineName))
        {
            return Fail<StatusSnapshot>(std::move(*e));
        }
        if (auto e =
                OptionalString(obj, "engine.getStatus.result", "engineVersion", out.engineVersion))
        {
            return Fail<StatusSnapshot>(std::move(*e));
        }
        if (auto e = OptionalString(obj, "engine.getStatus.result", "title", out.title))
        {
            return Fail<StatusSnapshot>(std::move(*e));
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
        return Wrap(std::move(obj));
    }

    Result<PlayAck, CodecError> PlayAck::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return Fail<PlayAck>(
                CodecError::invalid_field("runtime.play.result must be an object"));
        }
        if (auto e = RejectUnknownKeys(obj, "runtime.play.result", {"accepted", "requestedState"}))
        {
            return Fail<PlayAck>(std::move(*e));
        }

        PlayAck out;
        const auto acceptedIt = obj.find("accepted");
        if (acceptedIt == obj.end())
        {
            return Fail<PlayAck>(
                CodecError::invalid_field("`runtime.play.result` requires `accepted`"));
        }
        if (!acceptedIt->is_boolean())
        {
            return Fail<PlayAck>(
                CodecError::invalid_field("`runtime.play.result.accepted` must be a boolean"));
        }
        out.accepted = acceptedIt->get<bool>();

        const auto stateIt = obj.find("requestedState");
        if (stateIt != obj.end())
        {
            if (!stateIt->is_string())
            {
                return Fail<PlayAck>(CodecError::invalid_field(
                    "`runtime.play.result.requestedState` must be a string"));
            }
            const auto rs = runtime_state_from_wire(stateIt->get<std::string>());
            if (!rs.has_value())
            {
                return Fail<PlayAck>(CodecError::invalid_field(
                    std::string("invalid requestedState: ") + stateIt->get<std::string>()));
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
        return Wrap(std::move(obj));
    }

    Result<LogMessageEvent, CodecError> LogMessageEvent::from_json(const JsonValue& value)
    {
        const json& obj = peek(value)->json;
        if (!obj.is_object())
        {
            return Fail<LogMessageEvent>(
                CodecError::invalid_field("log.message.params must be an object"));
        }
        if (auto e = RejectUnknownKeys(obj, "log.message.params",
                                       {"level", "message", "category", "timestamp"}))
        {
            return Fail<LogMessageEvent>(std::move(*e));
        }

        LogMessageEvent out;

        std::string level;
        if (auto e = RequiredString(obj, "log.message.params", "level", level))
        {
            return Fail<LogMessageEvent>(std::move(*e));
        }
        const auto lvl = log_level_from_wire(level);
        if (!lvl.has_value())
        {
            return Fail<LogMessageEvent>(
                CodecError::invalid_field(std::string("invalid log level: ") + level));
        }
        out.level = *lvl;

        if (auto e = RequiredString(obj, "log.message.params", "message", out.message))
        {
            return Fail<LogMessageEvent>(std::move(*e));
        }
        if (auto e = OptionalString(obj, "log.message.params", "category", out.category))
        {
            return Fail<LogMessageEvent>(std::move(*e));
        }
        if (auto e = OptionalString(obj, "log.message.params", "timestamp", out.timestamp))
        {
            return Fail<LogMessageEvent>(std::move(*e));
        }
        return Result<LogMessageEvent, CodecError>::ok(std::move(out));
    }

}  // namespace Norves::Bridge::Dto
