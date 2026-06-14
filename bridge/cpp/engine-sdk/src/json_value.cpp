#include "norves/bridge/json_value.hpp"

#include <memory>
#include <utility>

#include "json_value_impl.hpp"

// nlohmann/json is confined to this translation unit (and codec.cpp), reached
// only via the src-private json_value_impl.hpp. The public header exposes only
// an opaque pImpl, so the type never leaks.
namespace norves::bridge {

// --- Internal bridge helpers (declared as friends in json_value.hpp) ---------
//
// These let codec.cpp build a JsonValue from a concrete nlohmann::json and read
// the underlying value back, without widening the public API.

JsonValue make_json_value(std::unique_ptr<detail::JsonValueImpl> impl) {
    return JsonValue(std::move(impl));
}

const detail::JsonValueImpl* peek(const JsonValue& value) { return value.impl(); }

// --- JsonValue special members ----------------------------------------------

JsonValue::JsonValue() : impl_(std::make_unique<detail::JsonValueImpl>()) {}

JsonValue::~JsonValue() = default;

JsonValue::JsonValue(std::unique_ptr<detail::JsonValueImpl> impl) : impl_(std::move(impl)) {
    // Construction from a moved-out source elsewhere may pass null; normalize to
    // a JSON null so the invariant "impl_ is never null" holds.
    if (impl_ == nullptr) {
        impl_ = std::make_unique<detail::JsonValueImpl>();
    }
}

JsonValue::JsonValue(const JsonValue& other)
    // A moved-from source has a null impl_; fall back to the default null-JSON
    // state so the invariant "impl_ is never null" holds on the copy path too,
    // matching the null guards in operator== and is_null.
    : impl_(other.impl_ == nullptr ? std::make_unique<detail::JsonValueImpl>()
                                   : std::make_unique<detail::JsonValueImpl>(other.impl_->json)) {}

JsonValue::JsonValue(JsonValue&& other) noexcept = default;

JsonValue& JsonValue::operator=(const JsonValue& other) {
    if (this != &other) {
        // Same null guard as the copy ctor: never deref a moved-from source.
        impl_ = other.impl_ == nullptr
                    ? std::make_unique<detail::JsonValueImpl>()
                    : std::make_unique<detail::JsonValueImpl>(other.impl_->json);
    }
    return *this;
}

JsonValue& JsonValue::operator=(JsonValue&& other) noexcept = default;

bool JsonValue::operator==(const JsonValue& other) const {
    // A moved-from JsonValue has a null impl_; treat it as not-equal to any
    // live value except another moved-from one. Live values delegate to
    // nlohmann's semantic equality.
    if (impl_ == nullptr || other.impl_ == nullptr) {
        return impl_ == nullptr && other.impl_ == nullptr;
    }
    return impl_->json == other.impl_->json;
}

bool JsonValue::is_null() const { return impl_ == nullptr || impl_->json.is_null(); }

}  // namespace norves::bridge
