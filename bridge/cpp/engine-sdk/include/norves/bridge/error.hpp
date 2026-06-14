#ifndef NORVES_BRIDGE_ERROR_HPP
#define NORVES_BRIDGE_ERROR_HPP

#include <optional>
#include <string>
#include <string_view>

#include "norves/bridge/json_value.hpp"

// Wire error codes and the SDK error value type.
// Depends on <std> and the SDK's own opaque JsonValue only; no third-party
// headers are included here.
namespace norves::bridge {

// Wire error codes. The protocol treats the code space as an OPEN registry, so
// these are string_view constants rather than a closed enum: an SDK may observe
// codes it does not have a constant for. Values are SCREAMING_SNAKE_CASE.
inline constexpr std::string_view kErrorProtocolVersionUnsupported =
    "PROTOCOL_VERSION_UNSUPPORTED";
inline constexpr std::string_view kErrorMethodNotSupported = "METHOD_NOT_SUPPORTED";
inline constexpr std::string_view kErrorBridgeTransportError = "BRIDGE_TRANSPORT_ERROR";

// Error value carried by a Result on the failure path.
//
// Mirrors the `error` $def of envelope.schema.json: a code (typically one of
// the constants above, but any string from the open registry is valid), a
// human-readable message, and an OPTIONAL opaque `data` payload for structured,
// error-code-specific detail. `data` is held as an opaque JsonValue and is not
// interpreted at the envelope layer (the payload layer is a later phase).
struct BridgeError {
    std::string code;
    std::string message;
    std::optional<JsonValue> data;

    [[nodiscard]] bool operator==(const BridgeError& other) const {
        return code == other.code && message == other.message && data == other.data;
    }
    [[nodiscard]] bool operator!=(const BridgeError& other) const { return !(*this == other); }
};

}  // namespace norves::bridge

#endif  // NORVES_BRIDGE_ERROR_HPP
