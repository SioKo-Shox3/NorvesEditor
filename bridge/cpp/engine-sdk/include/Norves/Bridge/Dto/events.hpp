#pragma once

#include "Norves/Bridge/codec_error.hpp"
#include "Norves/Bridge/Dto/common.hpp"
#include "Norves/Bridge/json_value.hpp"
#include "Norves/Bridge/result.hpp"

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>

/// @file
/// @brief 型付きイベントペイロード DTO: log.message と viewport.captureCompleted。
///
/// @note 依存は <std> と SDK 自身の値/enum 型のみ。サードパーティヘッダはここに露出しない。
///       JSON の構築 / 検証は src/dto_codec.cpp にある。from_json の検証はメソッド DTO と
///       同じ厳格な契約（未知の欄・必須欄・enum の検査）に従う。撮影完了 DTO は送信専用。
namespace Norves::Bridge::Dto
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

    inline constexpr std::string_view ViewportCaptureCompletedEventName =
        "viewport.captureCompleted";

    /// @brief 撮影完了の状態。成功と失敗のペイロードは同時に送らない。
    enum class ViewportCaptureStatus
    {
        Succeeded,
        Failed
    };

    /// @brief PNG 符号化前の、上から下・行の詰め物なしの RGBA8 画素の要約値。
    ///        algorithm は fnv1a64-rgba8 固定。value は小文字の 16 桁の 16 進数。
    struct CapturePixelDigest
    {
        std::string value;
    };

    /// @brief 撮影失敗の理由。code は OUTPUT_WRITE_FAILED / RENDER_FAILED など。
    struct CaptureFailure
    {
        std::string code;
        std::string message;
    };

    /// @brief viewport.captureCompleted の params。値を所有し、画像バイトを含まない。
    /// @note to_json は status に応じた欄だけを出力する。成功時は failure を省略し、
    ///       失敗時は成功の欄をすべて省略する。送信側が仕様の値の制約を満たすこと。
    ///       frame は撮影エポック内の刻みであり、ランタイム全体の刻みではない。
    struct ViewportCaptureCompletedEvent
    {
        std::string captureId;
        ViewportCaptureStatus status = ViewportCaptureStatus::Succeeded;
        std::string path;
        std::uint32_t width = 0;
        std::uint32_t height = 0;
        std::uint64_t frame = 0;
        CapturePixelDigest pixelDigest;
        std::uint64_t fileBytes = 0;
        CaptureFailure failure;

        [[nodiscard]] JsonValue to_json() const;
    };

}  // namespace Norves::Bridge::Dto
