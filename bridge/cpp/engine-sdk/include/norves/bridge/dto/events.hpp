#pragma once

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"

#include <optional>
#include <string>

// Typed event-payload DTO for the F5 round-trip: log.message. The C++ analogue
// of the Rust editor-client's parse_log_message.
//
// Depends on <std> + the SDK's own value/enum types only; no third-party
// headers are exposed here. JSON construction / validation lives in
// src/dto_codec.cpp. The same strict contract as the method DTOs applies
// (additionalProperties: false, required-field and enum checks).
namespace norves::bridge::dto
{

    // events/log.message.params.schema.json. `message` is a free-form string
    // (may be empty); `category` / `timestamp` are optional and, per the schema,
    // non-empty when present.
    struct LogMessageEvent
    {
        LogLevel level = LogLevel::Info;
        std::string message;
        std::optional<std::string> category;
        std::optional<std::string> timestamp;

        [[nodiscard]] bool operator==(const LogMessageEvent& other) const
        {
            return level == other.level && message == other.message && category == other.category &&
                   timestamp == other.timestamp;
        }
        [[nodiscard]] bool operator!=(const LogMessageEvent& other) const
        {
            return !(*this == other);
        }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<LogMessageEvent, CodecError> from_json(const JsonValue& value);
    };

}  // namespace norves::bridge::dto
