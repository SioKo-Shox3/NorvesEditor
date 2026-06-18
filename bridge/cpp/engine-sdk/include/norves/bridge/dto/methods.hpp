#pragma once

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"

#include <optional>
#include <string>
#include <vector>

// Typed method-payload DTOs for the F5 round-trip (four wire paths only):
// bridge.hello (params + result), engine.getStatus (result), runtime.play
// (result). These are the C++ analogue of the Rust editor-client's typed
// parse helpers (parse_hello_result / parse_status_result, HelloParams).
//
// Depends on <std> + the SDK's own value/enum types only; no third-party
// headers are exposed here. Each DTO is a plain struct; the JSON construction /
// validation lives in src/dto_codec.cpp, which is the only TU that touches the
// vendored JSON library.
//
// Validation contract (matches the F2 envelope codec's strictness and the
// schema's `additionalProperties: false`):
//   * from_json rejects any unknown key, recursively at every object layer
//     (including the nested `server` object of HelloResult),
//   * from_json rejects a missing required field, a wrong JSON type, and an
//     out-of-enum string,
//   * to_json omits absent optional fields (it never writes JSON `null` for an
//     unset std::optional), so from_json(to_json(x)) == x for any well-formed x.
//
// Scope: these four payloads only. capabilities (hello) and the open value
// $defs are deliberately not modelled here and stay opaque.
namespace norves::bridge::dto
{

    // bridge.hello request params. The `capabilities` field of the schema is not
    // used by the round trip and is therefore omitted from this DTO.
    struct HelloParams
    {
        std::string role;
        std::string clientName;
        std::optional<std::string> clientVersion;
        std::vector<std::string> protocolVersions;

        [[nodiscard]] bool operator==(const HelloParams& other) const
        {
            return role == other.role && clientName == other.clientName &&
                   clientVersion == other.clientVersion &&
                   protocolVersions == other.protocolVersions;
        }
        [[nodiscard]] bool operator!=(const HelloParams& other) const { return !(*this == other); }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<HelloParams, CodecError> from_json(const JsonValue& value);
    };

    // Engine identity nested inside HelloResult (bridge.hello.result#/server). The
    // schema's `engine` field is a free-form generic label, not an engine-specific
    // type name.
    struct ServerInfo
    {
        std::string name;
        std::optional<std::string> version;
        std::optional<std::string> engine;

        [[nodiscard]] bool operator==(const ServerInfo& other) const
        {
            return name == other.name && version == other.version && engine == other.engine;
        }
        [[nodiscard]] bool operator!=(const ServerInfo& other) const { return !(*this == other); }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<ServerInfo, CodecError> from_json(const JsonValue& value);
    };

    // bridge.hello response result. The schema's `capabilities` array is not used by
    // the round trip and is omitted.
    struct HelloResult
    {
        std::string sessionId;
        std::string protocolVersion;
        ServerInfo server;

        [[nodiscard]] bool operator==(const HelloResult& other) const
        {
            return sessionId == other.sessionId && protocolVersion == other.protocolVersion &&
                   server == other.server;
        }
        [[nodiscard]] bool operator!=(const HelloResult& other) const { return !(*this == other); }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<HelloResult, CodecError> from_json(const JsonValue& value);
    };

    // engine.getStatus response result.
    struct StatusSnapshot
    {
        EngineState engineState = EngineState::Initializing;
        RuntimeState runtimeState = RuntimeState::Unknown;
        std::optional<std::string> engineName;
        std::optional<std::string> engineVersion;
        std::optional<std::string> title;

        [[nodiscard]] bool operator==(const StatusSnapshot& other) const
        {
            return engineState == other.engineState && runtimeState == other.runtimeState &&
                   engineName == other.engineName && engineVersion == other.engineVersion &&
                   title == other.title;
        }
        [[nodiscard]] bool operator!=(const StatusSnapshot& other) const
        {
            return !(*this == other);
        }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<StatusSnapshot, CodecError> from_json(const JsonValue& value);
    };

    // runtime.play response result. The request params are an empty object, so only
    // the result is modelled. `requestedState` is the optional target runtime state.
    struct PlayAck
    {
        bool accepted = false;
        std::optional<RuntimeState> requestedState;

        [[nodiscard]] bool operator==(const PlayAck& other) const
        {
            return accepted == other.accepted && requestedState == other.requestedState;
        }
        [[nodiscard]] bool operator!=(const PlayAck& other) const { return !(*this == other); }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<PlayAck, CodecError> from_json(const JsonValue& value);
    };

}  // namespace norves::bridge::dto
