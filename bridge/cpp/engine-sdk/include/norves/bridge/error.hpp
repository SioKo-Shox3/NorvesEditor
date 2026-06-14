#ifndef NORVES_BRIDGE_ERROR_HPP
#define NORVES_BRIDGE_ERROR_HPP

#include <string>
#include <string_view>

// Wire error codes and the SDK error value type.
// Depends on <std> only; no third-party headers are included here.
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
// F1 keeps this minimal: a code (typically one of the constants above, but any
// string from the open registry is valid) plus a human-readable message. F2
// will extend BridgeError with an opaque `data` payload for structured error
// detail; that field is intentionally absent here.
struct BridgeError {
    std::string code;
    std::string message;
};

}  // namespace norves::bridge

#endif  // NORVES_BRIDGE_ERROR_HPP
