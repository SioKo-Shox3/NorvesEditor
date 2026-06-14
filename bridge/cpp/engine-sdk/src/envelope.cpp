#include "norves/bridge/envelope.hpp"

#include <variant>

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/result.hpp"

// validate() is a 1:1 port of Envelope::validate in the Rust reference
// (bridge/crates/norves-bridge-core/src/envelope.rs, fn validate). The order of
// the checks and the violation messages are kept identical so the two
// implementations report the same first violation.
namespace norves::bridge {

namespace {

Result<std::monostate, CodecError> violation(const char* message) {
    return Result<std::monostate, CodecError>::err(CodecError::structural_violation(message));
}

Result<std::monostate, CodecError> ok() {
    return Result<std::monostate, CodecError>::ok(std::monostate{});
}

}  // namespace

Result<std::monostate, CodecError> Envelope::validate() const {
    switch (kind) {
        case Kind::Request:
            if (!id.has_value()) {
                return violation("request envelope requires `id`");
            }
            if (!method.has_value()) {
                return violation("request envelope requires `method`");
            }
            if (result.has_value()) {
                return violation("request envelope must not carry `result`");
            }
            if (error.has_value()) {
                return violation("request envelope must not carry `error`");
            }
            if (event.has_value()) {
                return violation("request envelope must not carry `event`");
            }
            break;
        case Kind::Response: {
            if (!id.has_value()) {
                return violation("response envelope requires `id`");
            }
            const bool has_result = result.has_value();
            const bool has_error = error.has_value();
            if (has_result && has_error) {
                return violation("response envelope must not carry both `result` and `error`");
            }
            if (!has_result && !has_error) {
                return violation("response envelope requires exactly one of `result` or `error`");
            }
            if (method.has_value()) {
                return violation("response envelope must not carry `method`");
            }
            if (event.has_value()) {
                return violation("response envelope must not carry `event`");
            }
            if (params.has_value()) {
                return violation("response envelope must not carry `params`");
            }
            break;
        }
        case Kind::Event:
            if (!event.has_value()) {
                return violation("event envelope requires `event`");
            }
            if (id.has_value()) {
                return violation("event envelope must not carry `id`");
            }
            if (method.has_value()) {
                return violation("event envelope must not carry `method`");
            }
            if (result.has_value()) {
                return violation("event envelope must not carry `result`");
            }
            if (error.has_value()) {
                return violation("event envelope must not carry `error`");
            }
            break;
    }
    return ok();
}

}  // namespace norves::bridge
