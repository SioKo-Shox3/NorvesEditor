#include "Norves/Bridge/json_value.hpp"

#include "Norves/Bridge/codec_error.hpp"
#include "Norves/Bridge/result.hpp"

#include <memory>
#include <string>
#include <string_view>
#include <utility>

#include <nlohmann/json.hpp>

#include "json_value_impl.hpp"

// nlohmann/json はこの翻訳単位（および codec.cpp）に閉じ込められ、src-private な
// json_value_impl.hpp を介してのみ到達される。公開ヘッダは opaque な pImpl のみを
// 露出するため、この型は決して漏れない。
namespace Norves::Bridge
{

    // --- 内部ブリッジヘルパ（json_value.hpp で friend 宣言される） ---------------
    //
    // これらは codec.cpp が具体的な nlohmann::json から JsonValue を構築し、基底値を
    // 読み戻すことを、公開 API を広げずに可能にする。

    JsonValue make_json_value(std::unique_ptr<Detail::JsonValueImpl> impl)
    {
        return JsonValue(std::move(impl));
    }

    const Detail::JsonValueImpl* peek(const JsonValue& value) { return value.impl(); }

    // --- JsonValue の特殊メンバ --------------------------------------------------

    JsonValue::JsonValue() : m_Impl(std::make_unique<Detail::JsonValueImpl>()) {}

    JsonValue::~JsonValue() = default;

    JsonValue::JsonValue(std::unique_ptr<Detail::JsonValueImpl> impl) : m_Impl(std::move(impl))
    {
        // どこか別の場所で move-out されたソースからの構築は null を渡しうる。不変条件
        // 「m_Impl は決して null にならない」を保つため、JSON null に正規化する。
        if (m_Impl == nullptr)
        {
            m_Impl = std::make_unique<Detail::JsonValueImpl>();
        }
    }

    JsonValue::JsonValue(const JsonValue& other)
        // moved-from のソースは null な m_Impl を持つ。コピー経路でも不変条件
        // 「m_Impl は決して null にならない」が保たれるよう、デフォルトの null-JSON 状態へ
        // フォールバックする。これは operator== と is_null の null ガードに一致する。
        : m_Impl(other.m_Impl == nullptr
                     ? std::make_unique<Detail::JsonValueImpl>()
                     : std::make_unique<Detail::JsonValueImpl>(other.m_Impl->json))
    {
    }

    JsonValue::JsonValue(JsonValue&& other) noexcept = default;

    JsonValue& JsonValue::operator=(const JsonValue& other)
    {
        if (this != &other)
        {
            // コピーコンストラクタと同じ null ガード。moved-from のソースを決して逆参照
            // しない。
            m_Impl = other.m_Impl == nullptr
                         ? std::make_unique<Detail::JsonValueImpl>()
                         : std::make_unique<Detail::JsonValueImpl>(other.m_Impl->json);
        }
        return *this;
    }

    JsonValue& JsonValue::operator=(JsonValue&& other) noexcept = default;

    bool JsonValue::operator==(const JsonValue& other) const
    {
        // moved-from の JsonValue は null な m_Impl を持つ。それを、別の moved-from な
        // ものを除くいかなる live な値とも非等価として扱う。live な値は nlohmann の
        // 意味的等価性に委譲する。
        if (m_Impl == nullptr || other.m_Impl == nullptr)
        {
            return m_Impl == nullptr && other.m_Impl == nullptr;
        }
        return m_Impl->json == other.m_Impl->json;
    }

    bool JsonValue::is_null() const { return m_Impl == nullptr || m_Impl->json.is_null(); }

    // --- テキストのパース / dump（nlohmann はこの TU に閉じ込められる） ----------

    Result<JsonValue, CodecError> JsonValue::parse(std::string_view text)
    {
        nlohmann::json parsed =
            nlohmann::json::parse(text, /*cb=*/nullptr, /*allow_exceptions=*/false);
        if (parsed.is_discarded())
        {
            return Result<JsonValue, CodecError>::err(CodecError::parse("malformed JSON"));
        }
        auto impl = std::make_unique<Detail::JsonValueImpl>(std::move(parsed));
        return Result<JsonValue, CodecError>::ok(make_json_value(std::move(impl)));
    }

    std::string JsonValue::dump() const
    {
        // moved-from の値は null な m_Impl を持つ。moved-from な値に対する is_null /
        // operator== の扱いに一致するよう、それを JSON null としてシリアライズする。
        if (m_Impl == nullptr)
        {
            return "null";
        }
        return m_Impl->json.dump();
    }

}  // namespace Norves::Bridge
