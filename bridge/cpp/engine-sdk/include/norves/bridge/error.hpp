#pragma once

#include "norves/bridge/json_value.hpp"

#include <optional>
#include <string>
#include <string_view>

/// @file
/// @brief ワイヤーエラーコードと SDK のエラー値型。
/// @note 依存は <std> と SDK 自身の opaque な JsonValue のみ。サードパーティヘッダは
///       ここに含めない。
namespace norves::bridge
{

    /// @brief ワイヤーエラーコード。プロトコルはコード空間を開かれた（OPEN）レジストリと
    ///        して扱うため、これらは閉じた enum ではなく string_view 定数である。すなわち
    ///        SDK は定数を持たないコードを観測しうる。値は SCREAMING_SNAKE_CASE。
    inline constexpr std::string_view ErrorProtocolVersionUnsupported =
        "PROTOCOL_VERSION_UNSUPPORTED";
    inline constexpr std::string_view ErrorMethodNotSupported = "METHOD_NOT_SUPPORTED";
    inline constexpr std::string_view ErrorBridgeTransportError = "BRIDGE_TRANSPORT_ERROR";

    /// @brief 失敗パスで Result が運ぶエラー値。
    ///
    /// @note envelope.schema.json の `error` $def を反映する。すなわち code（通常は
    ///       上記定数のいずれかだが、開かれたレジストリの任意の文字列が有効）、人間可読の
    ///       message、および構造化されたエラーコード固有の詳細のためのオプションの opaque な
    ///       `data` ペイロード。`data` は opaque な JsonValue として保持され、エンベロープ
    ///       層では解釈されない（ペイロード層は後のフェーズ）。
    struct BridgeError
    {
        std::string code;
        std::string message;
        std::optional<JsonValue> data;

        [[nodiscard]] bool operator==(const BridgeError& other) const
        {
            return code == other.code && message == other.message && data == other.data;
        }
        [[nodiscard]] bool operator!=(const BridgeError& other) const { return !(*this == other); }
    };

}  // namespace norves::bridge
