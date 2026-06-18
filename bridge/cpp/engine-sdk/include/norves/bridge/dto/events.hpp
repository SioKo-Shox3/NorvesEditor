#pragma once

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"

#include <optional>
#include <string>

/// @file
/// @brief F5 ラウンドトリップのための型付きイベントペイロード DTO: log.message。Rust
///        editor-client の parse_log_message の C++ 対応物。
///
/// @note 依存は <std> と SDK 自身の値/enum 型のみ。サードパーティヘッダはここに露出しない。
///       JSON の構築 / 検証は src/dto_codec.cpp にある。メソッド DTO と同じ厳格な契約が
///       適用される（additionalProperties: false、必須フィールドおよび enum のチェック）。
namespace norves::bridge::dto
{

    /// @brief events/log.message.params.schema.json。`message` は自由形式の文字列
    ///        （空でもよい）。`category` / `timestamp` はオプションであり、スキーマに従い
    ///        存在する場合は非空。
    struct LogMessageEvent
    {
        LogLevel level = LogLevel::Info;
        std::string message;
        std::optional<std::string> category;
        std::optional<std::string> timestamp;

        [[nodiscard]] bool operator==(const LogMessageEvent& other) const
        {
            return level == other.level && message == other.message && category == other.category &&
                   timestamp == other.timestamp;
        }
        [[nodiscard]] bool operator!=(const LogMessageEvent& other) const
        {
            return !(*this == other);
        }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<LogMessageEvent, CodecError> from_json(const JsonValue& value);
    };

}  // namespace norves::bridge::dto
