#ifndef NORVES_BRIDGE_CODEC_ERROR_HPP
#define NORVES_BRIDGE_CODEC_ERROR_HPP

#include <string>
#include <utility>

// Local failure raised while decoding or validating a Bridge envelope.
//
// Depends on <std> only; no third-party headers are included here. This is the
// C++ analogue of the Rust reference's `CodecError`
// (`bridge/crates/norves-bridge-core/src/error.rs`): a local, never-on-the-wire
// processing failure. It is distinct from BridgeError (the wire error object).
namespace norves::bridge {

// Coarse classification of a decode/validate failure. The kinds collapse the
// Rust CodecError variants into the categories that matter at the envelope
// layer; a human-readable message carries the specifics.
enum class CodecErrorKind {
    // JSON parse failure (malformed input).
    Parse,
    // An unknown / unexpected field (additionalProperties: false violation).
    UnknownField,
    // A field value violated its pattern / type / presence constraint
    // (bridge marker, version, method/event name, error code, id, seq, ...).
    InvalidField,
    // An envelope violated a kind-dependent structural constraint
    // (mirrors Rust CodecError::StructuralViolation).
    StructuralViolation,
};

// Local decode/validate error value.
struct CodecError {
    CodecErrorKind kind = CodecErrorKind::Parse;
    std::string message;

    [[nodiscard]] bool operator==(const CodecError& other) const {
        return kind == other.kind && message == other.message;
    }
    [[nodiscard]] bool operator!=(const CodecError& other) const { return !(*this == other); }

    static CodecError parse(std::string message) {
        return CodecError{CodecErrorKind::Parse, std::move(message)};
    }
    static CodecError unknown_field(std::string message) {
        return CodecError{CodecErrorKind::UnknownField, std::move(message)};
    }
    static CodecError invalid_field(std::string message) {
        return CodecError{CodecErrorKind::InvalidField, std::move(message)};
    }
    static CodecError structural_violation(std::string message) {
        return CodecError{CodecErrorKind::StructuralViolation, std::move(message)};
    }
};

}  // namespace norves::bridge

#endif  // NORVES_BRIDGE_CODEC_ERROR_HPP
