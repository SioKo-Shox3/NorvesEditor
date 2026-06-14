#ifndef NORVES_BRIDGE_JSON_VALUE_HPP
#define NORVES_BRIDGE_JSON_VALUE_HPP

#include <memory>

// Opaque JSON value wrapper for the engine SDK.
//
// Depends on <std> only; no third-party headers are included here. The whole
// point of this type is to carry an arbitrary JSON value (a `params` object, a
// response `result`, or an `error.data` payload) THROUGH the SDK without
// interpreting it and WITHOUT exposing the underlying JSON library in any
// public header. The .cpp implementation is the only translation unit that may
// include the vendored JSON library.
namespace norves::bridge {

namespace detail {
// Defined only in the .cpp implementation; never completed in a public header
// so that the underlying JSON library stays hidden behind the pImpl.
struct JsonValueImpl;
}  // namespace detail

// A value-owning, opaque JSON value.
//
// Construction yields JSON `null`. Copy/move duplicate/transfer ownership of
// the underlying value. Equality is semantic (value-equal): field order and
// insignificant whitespace do not affect comparison, matching the Rust
// reference implementation's `serde_json::Value` equality.
//
// Invariant: a live (non-moved-from) JsonValue always holds a valid value.
// A move leaves the source moved-from; copying a moved-from source yields a
// JSON `null` (copy never dereferences a null source), and is_null/operator==
// treat a moved-from value as `null`.
//
// No accessor exposes the underlying representation; the payload layer (a later
// phase) owns interpretation of the contents.
class JsonValue {
  public:
    // Constructs a JSON `null` value.
    JsonValue();
    ~JsonValue();

    JsonValue(const JsonValue& other);
    JsonValue(JsonValue&& other) noexcept;
    JsonValue& operator=(const JsonValue& other);
    JsonValue& operator=(JsonValue&& other) noexcept;

    // Semantic (value-equal) comparison; delegates to the underlying JSON
    // equality inside the implementation TU.
    [[nodiscard]] bool operator==(const JsonValue& other) const;
    [[nodiscard]] bool operator!=(const JsonValue& other) const { return !(*this == other); }

    // True iff this value is JSON `null`.
    [[nodiscard]] bool is_null() const;

  private:
    // The codec / json_value .cpp TUs construct JsonValue from a concrete
    // underlying value via the detail bridge; they are the only TUs that see it.
    friend struct detail::JsonValueImpl;
    explicit JsonValue(std::unique_ptr<detail::JsonValueImpl> impl);

    std::unique_ptr<detail::JsonValueImpl> impl_;

    // Internal accessors used only by implementation TUs (codec.cpp,
    // json_value.cpp). They return the opaque impl pointer; callers outside
    // those TUs cannot complete `detail::JsonValueImpl`, so this leaks nothing.
    [[nodiscard]] const detail::JsonValueImpl* impl() const { return impl_.get(); }
    [[nodiscard]] detail::JsonValueImpl* impl() { return impl_.get(); }

    // Implementation-side free helpers (defined in the .cpp TUs) reach the
    // private members through these.
    friend JsonValue make_json_value(std::unique_ptr<detail::JsonValueImpl> impl);
    friend const detail::JsonValueImpl* peek(const JsonValue& value);
};

}  // namespace norves::bridge

#endif  // NORVES_BRIDGE_JSON_VALUE_HPP
