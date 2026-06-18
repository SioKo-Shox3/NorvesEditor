#include "norves/bridge/json_value.hpp"

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/result.hpp"

#include <memory>
#include <string>
#include <string_view>
#include <utility>

#include <nlohmann/json.hpp>

#include "json_value_impl.hpp"

// nlohmann/json is confined to this translation unit (and codec.cpp), reached
// only via the src-private json_value_impl.hpp. The public header exposes only
// an opaque pImpl, so the type never leaks.
namespace norves::bridge
{

    // --- Internal bridge helpers (declared as friends in json_value.hpp) ---------
    //
    // These let codec.cpp build a JsonValue from a concrete nlohmann::json and read
    // the underlying value back, without widening the public API.

    JsonValue make_json_value(std::unique_ptr<detail::JsonValueImpl> impl)
    {
        return JsonValue(std::move(impl));
    }

    const detail::JsonValueImpl* peek(const JsonValue& value) { return value.impl(); }

    // --- JsonValue special members ----------------------------------------------

    JsonValue::JsonValue() : m_Impl(std::make_unique<detail::JsonValueImpl>()) {}

    JsonValue::~JsonValue() = default;

    JsonValue::JsonValue(std::unique_ptr<detail::JsonValueImpl> impl) : m_Impl(std::move(impl))
    {
        // Construction from a moved-out source elsewhere may pass null; normalize to
        // a JSON null so the invariant "m_Impl is never null" holds.
        if (m_Impl == nullptr)
        {
            m_Impl = std::make_unique<detail::JsonValueImpl>();
        }
    }

    JsonValue::JsonValue(const JsonValue& other)
        // A moved-from source has a null m_Impl; fall back to the default null-JSON
        // state so the invariant "m_Impl is never null" holds on the copy path too,
        // matching the null guards in operator== and is_null.
        : m_Impl(other.m_Impl == nullptr
                     ? std::make_unique<detail::JsonValueImpl>()
                     : std::make_unique<detail::JsonValueImpl>(other.m_Impl->json))
    {
    }

    JsonValue::JsonValue(JsonValue&& other) noexcept = default;

    JsonValue& JsonValue::operator=(const JsonValue& other)
    {
        if (this != &other)
        {
            // Same null guard as the copy ctor: never deref a moved-from source.
            m_Impl = other.m_Impl == nullptr
                         ? std::make_unique<detail::JsonValueImpl>()
                         : std::make_unique<detail::JsonValueImpl>(other.m_Impl->json);
        }
        return *this;
    }

    JsonValue& JsonValue::operator=(JsonValue&& other) noexcept = default;

    bool JsonValue::operator==(const JsonValue& other) const
    {
        // A moved-from JsonValue has a null m_Impl; treat it as not-equal to any
        // live value except another moved-from one. Live values delegate to
        // nlohmann's semantic equality.
        if (m_Impl == nullptr || other.m_Impl == nullptr)
        {
            return m_Impl == nullptr && other.m_Impl == nullptr;
        }
        return m_Impl->json == other.m_Impl->json;
    }

    bool JsonValue::is_null() const { return m_Impl == nullptr || m_Impl->json.is_null(); }

    // --- Text parse / dump (nlohmann confined to this TU) ------------------------

    Result<JsonValue, CodecError> JsonValue::parse(std::string_view text)
    {
        nlohmann::json parsed =
            nlohmann::json::parse(text, /*cb=*/nullptr, /*allow_exceptions=*/false);
        if (parsed.is_discarded())
        {
            return Result<JsonValue, CodecError>::err(CodecError::parse("malformed JSON"));
        }
        auto impl = std::make_unique<detail::JsonValueImpl>(std::move(parsed));
        return Result<JsonValue, CodecError>::ok(make_json_value(std::move(impl)));
    }

    std::string JsonValue::dump() const
    {
        // A moved-from value has a null m_Impl; serialize it as JSON null, matching
        // the is_null / operator== treatment of a moved-from value.
        if (m_Impl == nullptr)
        {
            return "null";
        }
        return m_Impl->json.dump();
    }

}  // namespace norves::bridge
