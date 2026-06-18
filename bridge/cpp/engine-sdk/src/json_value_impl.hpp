#pragma once

#include "norves/bridge/json_value.hpp"

#include <memory>
#include <utility>

#include <nlohmann/json.hpp>

/// @file
/// @brief JsonValue pImpl の private（src 専用）な完全定義。このヘッダは nlohmann/json を
///        include するため、include/ から決して（NEVER）到達できてはならない。
///        detail::JsonValueImpl が ODR 上正しい単一の定義を持つよう、json_value.cpp と
///        codec.cpp で共有される。
namespace norves::bridge
{

    namespace detail
    {

        /// @brief 具体的な pImpl。所有された単一の nlohmann::json 値。
        struct JsonValueImpl
        {
            nlohmann::json json;

            JsonValueImpl() = default;
            explicit JsonValueImpl(nlohmann::json value) : json(std::move(value)) {}
        };

    }  // namespace detail

    /// @brief 内部ブリッジヘルパ（json_value.hpp で friend 宣言され、json_value.cpp で
    ///        定義される）。codec.cpp が具体的な nlohmann::json から JsonValue を構築し、
    ///        それを読み戻すことを、公開 API を広げずに可能にする。
    JsonValue make_json_value(std::unique_ptr<detail::JsonValueImpl> impl);
    const detail::JsonValueImpl* peek(const JsonValue& value);

}  // namespace norves::bridge
