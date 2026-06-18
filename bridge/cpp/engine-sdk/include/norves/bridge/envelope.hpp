#pragma once

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <variant>

// The canonical Bridge wire envelope, mirrored from the Rust reference
// implementation (`bridge/crates/norves-bridge-core/src/envelope.rs`) and
// `envelope.schema.json`.
//
// Depends on <std> and the SDK's own value types only; no third-party headers
// are included here. `params`, `result`, and `error.data` are carried as opaque
// JsonValue and are NOT interpreted at this layer.
namespace norves::bridge
{

    // Protocol marker constant for the NorvesEditor Bridge.
    inline constexpr std::string_view kBridgeMarker = "norves.editor.bridge";

    // Envelope discriminator. Schema: enum ["request", "response", "event"].
    enum class Kind
    {
        Request,
        Response,
        Event
    };

    // The flat wire envelope. Per-kind field-presence rules are NOT enforced by
    // construction; call validate() to apply the cross-field structural rules that
    // mirror Envelope::validate in the Rust reference.
    struct Envelope
    {
        // Protocol marker. Always the constant kBridgeMarker on the wire; carried as
        // a string so the value round-trips verbatim.
        std::string bridge;
        // Protocol version string, MAJOR.MINOR.
        std::string version;
        // Envelope discriminator.
        Kind kind = Kind::Request;
        // Request/response correlation id.
        std::optional<std::string> id;
        // Method name on a request.
        std::optional<std::string> method;
        // Event name on an event envelope.
        std::optional<std::string> event;
        // Method or event payload (opaque object).
        std::optional<JsonValue> params;
        // Success payload on a response (opaque). Mutually exclusive with error.
        std::optional<JsonValue> result;
        // Error payload on a response. Mutually exclusive with result.
        std::optional<BridgeError> error;
        // Optional session id assigned during the handshake.
        std::optional<std::string> session_id;
        // Optional monotonically increasing per-connection sequence number.
        // Unsigned to mirror the Rust reference (u64) and the schema's
        // integer minimum: 0; a negative value on the wire is rejected at decode.
        std::optional<std::uint64_t> seq;

        [[nodiscard]] bool operator==(const Envelope& other) const
        {
            return bridge == other.bridge && version == other.version && kind == other.kind &&
                   id == other.id && method == other.method && event == other.event &&
                   params == other.params && result == other.result && error == other.error &&
                   session_id == other.session_id && seq == other.seq;
        }
        [[nodiscard]] bool operator!=(const Envelope& other) const { return !(*this == other); }

        // Enforces the kind-dependent structural constraints of
        // envelope.schema.json's allOf. Mirrors Rust Envelope::validate 1:1:
        //   * request  - id and method required; result, error, event forbidden.
        //   * response - id required; exactly one of result / error;
        //                method, event, params forbidden.
        //   * event    - event required; id, method, result, error forbidden.
        // Returns CodecError::StructuralViolation describing the first violation.
        [[nodiscard]] Result<std::monostate, CodecError> validate() const;
    };

}  // namespace norves::bridge
