#pragma once

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/result.hpp"

#include <memory>
#include <string>
#include <string_view>

/// @file
/// @brief エンジン SDK のための opaque な JSON 値ラッパ。
///
/// @note 依存は <std> のみ。サードパーティヘッダはここに含めない。この型の唯一の目的は、
///       任意の JSON 値（`params` オブジェクト、レスポンスの `result`、または `error.data`
///       ペイロード）を、解釈せず、かつ基底の JSON ライブラリをいかなる公開ヘッダにも
///       露出せずに、SDK を通して運ぶことである。.cpp 実装が、ベンダリングされた JSON
///       ライブラリを include してよい唯一の翻訳単位である。
namespace norves::bridge
{

    namespace detail
    {
        /// @brief .cpp 実装でのみ定義される。基底の JSON ライブラリが pImpl の背後に
        ///        隠れたままになるよう、公開ヘッダでは決して完全型にしない。
        struct JsonValueImpl;
    }  // namespace detail

    /// @brief 値所有の opaque な JSON 値。
    ///
    /// @note 構築すると JSON `null` を生じる。コピー/move は基底値の所有を複製/移譲する。
    ///       等価性は意味的（値が等しいかどうか）であり、フィールド順序や無意味な空白は
    ///       比較に影響しない。これは Rust リファレンス実装の `serde_json::Value` の
    ///       等価性に一致する。
    ///
    /// @note 不変条件: live（move されていない）JsonValue は常に有効な値を保持する。
    ///       move はソースを moved-from のままにする。moved-from のソースをコピーすると
    ///       JSON `null` を生じ（コピーは null なソースを決して逆参照しない）、
    ///       is_null/operator== は moved-from の値を `null` として扱う。
    ///
    /// @note いかなるアクセサも基底の表現を露出しない。内容の解釈はペイロード層
    ///       （後のフェーズ）が所有する。
    class JsonValue
    {
    public:
        /// @brief JSON `null` 値を構築する。
        JsonValue();
        ~JsonValue();

        JsonValue(const JsonValue& other);
        JsonValue(JsonValue&& other) noexcept;
        JsonValue& operator=(const JsonValue& other);
        JsonValue& operator=(JsonValue&& other) noexcept;

        /// @brief 意味的（値が等しいか）な比較。実装 TU 内で基底の JSON 等価性に委譲する。
        [[nodiscard]] bool operator==(const JsonValue& other) const;
        [[nodiscard]] bool operator!=(const JsonValue& other) const { return !(*this == other); }

        /// @brief この値が JSON `null` のときに限り true。
        [[nodiscard]] bool is_null() const;

        /// @brief JSON テキストを opaque な JsonValue へパースする。
        /// @note ベンダリングされた JSON ライブラリは .cpp 実装内でのみ使われるため、
        ///       これは SDK 自身の値型以外を一切露出しない。不正な入力に対しては
        ///       CodecError（kind Parse）を返す。任意の有効な JSON 値（オブジェクト、
        ///       配列、スカラー、null）を受理する。
        /// @param text パース対象の JSON テキスト。
        /// @return 成功時は JsonValue、失敗時は CodecError。
        [[nodiscard]] static Result<JsonValue, CodecError> parse(std::string_view text);

        /// @brief この値をコンパクトな JSON テキスト（整形なし）へシリアライズする。
        /// @note live な値は常にシリアライズされる。moved-from の値は JSON `null` として
        ///       シリアライズされる。
        [[nodiscard]] std::string dump() const;

    private:
        /// codec / json_value の .cpp TU は detail ブリッジを介して具体的な基底値から
        /// JsonValue を構築する。それらがこの構築を見られる唯一の TU である。
        friend struct detail::JsonValueImpl;
        explicit JsonValue(std::unique_ptr<detail::JsonValueImpl> impl);

        std::unique_ptr<detail::JsonValueImpl> m_Impl;

        /// 実装 TU（codec.cpp, json_value.cpp）でのみ使われる内部アクセサ。opaque な
        /// impl ポインタを返す。それらの TU の外の呼び出し側は
        /// `detail::JsonValueImpl` を完全型にできないため、これは何も漏らさない。
        [[nodiscard]] const detail::JsonValueImpl* impl() const { return m_Impl.get(); }
        [[nodiscard]] detail::JsonValueImpl* impl() { return m_Impl.get(); }

        /// 実装側の自由ヘルパ（.cpp TU で定義）は、これらを通して private メンバへ
        /// アクセスする。
        friend JsonValue make_json_value(std::unique_ptr<detail::JsonValueImpl> impl);
        friend const detail::JsonValueImpl* peek(const JsonValue& value);
    };

}  // namespace norves::bridge
