// @brief C++ エンジン SDK コーデック（エンベロープ層）のフィクスチャ駆動適合テスト。
//
// Rust リファレンステスト（bridge/crates/norves-bridge-core/tests/fixtures_roundtrip.rs）を
// ミラーする: bridge/spec/fixtures 配下のすべての *.json をパスのみで分類し、以下を実行する:
//
//   * positive（envelope/positive、methods/.../positive、events/.../positive）
//       -> decode_envelope が成功すること（decode は validate() を内部で適用する）、
//          および再エンコードが値等価でラウンドトリップすること。
//   * envelope/negative -> decode_envelope が拒否されること（Err）。
//   * methods|events/negative（ペイロード専用）-> エンベロープ層では ACCEPTED となる
//          （ペイロードスキーマは後続フェーズで検証する）。Rust の境界と一致する。
//
// nlohmann/json は値等価比較のみに使用する。これはテスト TU であり SDK の公開面ではないため、
// 境界ルール（include/ に nlohmann を含めない）には影響しない。

#include "Norves/Bridge/codec.hpp"
#include "Norves/Bridge/envelope.hpp"

#include <cstdint>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

#include <nlohmann/json.hpp>

#include "test_support.hpp"

#ifndef NORVES_FIXTURES_DIR
#error "NORVES_FIXTURES_DIR must be defined by the build (path to bridge/spec/fixtures)"
#endif

namespace fs = std::filesystem;

namespace
{

    enum class Group
    {
        Positive,
        EnvelopeRejectable,
        PayloadOnly,
        Ignored
    };

    // @brief パスによる分類。Rust の `classify` と精神的に同一。
    Group Classify(const std::string& normalized)
    {
        const bool bIsPositive = normalized.find("/positive/") != std::string::npos;
        const bool bIsNegative = normalized.find("/negative/") != std::string::npos;
        const bool bIsEnvelope = normalized.find("/fixtures/envelope/") != std::string::npos;
        const bool bIsMethod = normalized.find("/fixtures/methods/") != std::string::npos;
        const bool bIsEvent = normalized.find("/fixtures/events/") != std::string::npos;

        if (bIsPositive && (bIsEnvelope || bIsMethod || bIsEvent))
        {
            return Group::Positive;
        }
        if (bIsNegative && bIsEnvelope)
        {
            return Group::EnvelopeRejectable;
        }
        if (bIsNegative && (bIsMethod || bIsEvent))
        {
            return Group::PayloadOnly;
        }
        return Group::Ignored;
    }

    std::string Normalize(const fs::path& path)
    {
        std::string s = path.generic_string();  // すでに '/' を使用している
        return s;
    }

    std::string ReadFile(const fs::path& path)
    {
        std::ifstream in(path, std::ios::binary);
        std::ostringstream ss;
        ss << in.rdbuf();
        return ss.str();
    }

    std::vector<fs::path> CollectJson(const fs::path& root)
    {
        std::vector<fs::path> out;
        for (const auto& entry : fs::recursive_directory_iterator(root))
        {
            if (entry.is_regular_file() && entry.path().extension() == ".json")
            {
                out.push_back(entry.path());
            }
        }
        return out;
    }

}  // namespace

int main()
{
    const fs::path root(NORVES_FIXTURES_DIR);
    NORVES_CHECK(fs::is_directory(root));
    if (!fs::is_directory(root))
    {
        return norves::test::summary();
    }

    NORVES_CHECK(fs::is_regular_file(
        root / "methods/asset.reloadManifest/positive/request-valid.json"));
    NORVES_CHECK(fs::is_regular_file(
        root / "methods/asset.reloadManifest/positive/response-valid.json"));
    NORVES_CHECK(fs::is_regular_file(
        root / "envelope/positive/response-error-engine-invalid-params.json"));

    const auto files = CollectJson(root);

    std::size_t positive = 0;
    std::size_t envelopeRejectable = 0;
    std::size_t payloadOnly = 0;
    std::size_t ignored = 0;

    for (const auto& path : files)
    {
        const std::string normalized = Normalize(path);
        const Group group = Classify(normalized);
        const std::string wire = ReadFile(path);

        switch (group)
        {
            case Group::Positive:
            {
                ++positive;
                auto decoded = Norves::Bridge::decode_envelope(wire);
                NORVES_CHECK(decoded.is_ok());
                if (decoded.is_err())
                {
                    std::fprintf(stderr, "  positive failed to decode: %s (%s)\n",
                                 normalized.c_str(), decoded.error().message.c_str());
                    break;
                }
                auto encoded = Norves::Bridge::encode_envelope(decoded.value());
                NORVES_CHECK(encoded.is_ok());
                if (encoded.is_err())
                {
                    break;
                }
                // 値等価ラウンドトリップ: 元データと再エンコード済みデータをそれぞれ
                // nlohmann::json としてパースし比較する（フィールド順・空白に依存しない）。
                const auto orig = nlohmann::json::parse(wire, nullptr, false);
                const auto again = nlohmann::json::parse(encoded.value(), nullptr, false);
                NORVES_CHECK(!orig.is_discarded());
                NORVES_CHECK(!again.is_discarded());
                const bool bEqual = (orig == again);
                NORVES_CHECK(bEqual);
                if (!bEqual)
                {
                    std::fprintf(stderr, "  positive did not round-trip value-equal: %s\n",
                                 normalized.c_str());
                }
                break;
            }
            case Group::EnvelopeRejectable:
            {
                ++envelopeRejectable;
                auto decoded = Norves::Bridge::decode_envelope(wire);
                NORVES_CHECK(decoded.is_err());
                if (decoded.is_ok())
                {
                    std::fprintf(stderr, "  envelope negative unexpectedly accepted: %s\n",
                                 normalized.c_str());
                }
                break;
            }
            case Group::PayloadOnly:
            {
                ++payloadOnly;
                // エンベロープとしては有効。ペイロードのみが不正だが、
                // この層ではまだ検証しない。ACCEPTED でなければならない。
                auto decoded = Norves::Bridge::decode_envelope(wire);
                NORVES_CHECK(decoded.is_ok());
                if (decoded.is_err())
                {
                    std::fprintf(stderr,
                                 "  payload-only negative rejected at envelope layer: %s (%s)\n",
                                 normalized.c_str(), decoded.error().message.c_str());
                }
                break;
            }
            case Group::Ignored:
                ++ignored;
                break;
        }
    }

    // 網羅的カウント: Rust リファレンス（D2 合計）と同一。フィクスチャコーパスに
    // 乖離があれば最初にここで検出され、差分を指摘する。
    NORVES_CHECK_EQ(positive, static_cast<std::size_t>(76));
    NORVES_CHECK_EQ(envelopeRejectable, static_cast<std::size_t>(14));
    NORVES_CHECK_EQ(payloadOnly, static_cast<std::size_t>(70));
    NORVES_CHECK_EQ(positive + envelopeRejectable + payloadOnly, static_cast<std::size_t>(160));
    NORVES_CHECK_EQ(ignored, static_cast<std::size_t>(0));

    std::fprintf(stderr,
                 "counts: positive=%zu envelope_negative=%zu payload_only=%zu ignored=%zu\n",
                 positive, envelopeRejectable, payloadOnly, ignored);

    return norves::test::summary();
}
