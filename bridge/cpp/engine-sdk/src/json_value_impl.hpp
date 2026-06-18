#pragma once

#include "norves/bridge/json_value.hpp"

#include <memory>
#include <utility>

#include <nlohmann/json.hpp>

// Private (src-only) completion of the JsonValue pImpl. This header includes
// nlohmann/json and therefore must NEVER be reachable from include/. It is
// shared by json_value.cpp and codec.cpp so that detail::JsonValueImpl has a
// single ODR-correct definition.
namespace norves::bridge
{

    namespace detail
    {

        // Concrete pImpl: a single owned nlohmann::json value.
        struct JsonValueImpl
        {
            nlohmann::json json;

            JsonValueImpl() = default;
            explicit JsonValueImpl(nlohmann::json value) : json(std::move(value)) {}
        };

    }  // namespace detail

    // Internal bridge helpers (declared as friends in json_value.hpp), defined in
    // json_value.cpp. They let codec.cpp build a JsonValue from a concrete
    // nlohmann::json and read it back without widening the public API.
    JsonValue make_json_value(std::unique_ptr<detail::JsonValueImpl> impl);
    const detail::JsonValueImpl* peek(const JsonValue& value);

}  // namespace norves::bridge
