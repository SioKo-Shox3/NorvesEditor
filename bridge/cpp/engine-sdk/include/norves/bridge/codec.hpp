#ifndef NORVES_BRIDGE_CODEC_HPP
#define NORVES_BRIDGE_CODEC_HPP

#include <string>
#include <string_view>

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/envelope.hpp"
#include "norves/bridge/result.hpp"

// JSON codec entry points for the Bridge wire Envelope.
//
// Depends on <std> and the SDK's own value types only; no third-party headers
// are included here. The vendored JSON library is used solely inside the .cpp
// implementation. Mirrors the
// Rust reference (`bridge/crates/norves-bridge-core/src/codec.rs`): only the
// ENVELOPE layer is validated; per-method / per-event payload schemas are a
// later phase, so params / result / error.data are carried opaque.
namespace norves::bridge {

// Decodes a JSON string into an Envelope, applying the envelope-layer rules:
//   * valid JSON object,
//   * additionalProperties: false at the envelope and at the error object,
//   * field patterns (bridge marker, version, method/event names, error.code,
//     non-empty id, seq >= 0),
//   * kind-dependent structural rules via Envelope::validate.
// params / result / error.data are preserved as opaque JsonValue and not
// inspected further.
//
// Returns CodecError on any violation.
[[nodiscard]] Result<Envelope, CodecError> decode_envelope(std::string_view wire);

// Encodes an Envelope back to a compact JSON string. Returns CodecError if the
// envelope cannot be serialized.
[[nodiscard]] Result<std::string, CodecError> encode_envelope(const Envelope& envelope);

}  // namespace norves::bridge

#endif  // NORVES_BRIDGE_CODEC_HPP
