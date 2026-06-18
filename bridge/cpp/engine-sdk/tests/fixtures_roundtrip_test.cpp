// Fixture-driven conformance test for the C++ engine SDK codec (envelope layer).
//
// Mirrors the Rust reference test
// (bridge/crates/norves-bridge-core/tests/fixtures_roundtrip.rs): it walks
// bridge/spec/fixtures and classifies every *.json purely by path, then:
//
//   * positive (envelope/positive, methods/.../positive, events/.../positive)
//       -> decode_envelope must succeed (decode already applies validate()) and
//          re-encode must round-trip value-equal.
//   * envelope/negative -> decode_envelope must be rejected (Err).
//   * methods|events/negative (payload-only) -> ACCEPTED at the envelope layer
//          (payload schemas are a later phase), matching the Rust boundary.
//
// nlohmann/json is used here for value-equal comparison only; this is a test TU,
// not part of the SDK's public surface, so the boundary rule (no nlohmann in
// include/) is unaffected.

#include "norves/bridge/codec.hpp"
#include "norves/bridge/envelope.hpp"

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

    // Path-based classification, identical in spirit to the Rust `classify`.
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
        std::string s = path.generic_string();  // already uses '/'
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
                auto decoded = norves::bridge::decode_envelope(wire);
                NORVES_CHECK(decoded.is_ok());
                if (decoded.is_err())
                {
                    std::fprintf(stderr, "  positive failed to decode: %s (%s)\n",
                                 normalized.c_str(), decoded.error().message.c_str());
                    break;
                }
                auto encoded = norves::bridge::encode_envelope(decoded.value());
                NORVES_CHECK(encoded.is_ok());
                if (encoded.is_err())
                {
                    break;
                }
                // value-equal round-trip: parse original and re-encoded, compare
                // as nlohmann::json (field order / whitespace independent).
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
                auto decoded = norves::bridge::decode_envelope(wire);
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
                // Valid envelope; only the payload is wrong, which this layer
                // does not validate yet. Must be ACCEPTED here.
                auto decoded = norves::bridge::decode_envelope(wire);
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

    // Exhaustive counts: identical to the Rust reference (D2 totals). A drift in
    // the fixture corpus breaks here first and points at the discrepancy.
    NORVES_CHECK_EQ(positive, static_cast<std::size_t>(55));
    NORVES_CHECK_EQ(envelopeRejectable, static_cast<std::size_t>(14));
    NORVES_CHECK_EQ(payloadOnly, static_cast<std::size_t>(45));
    NORVES_CHECK_EQ(positive + envelopeRejectable + payloadOnly, static_cast<std::size_t>(114));
    NORVES_CHECK_EQ(ignored, static_cast<std::size_t>(0));

    std::fprintf(stderr,
                 "counts: positive=%zu envelope_negative=%zu payload_only=%zu ignored=%zu\n",
                 positive, envelopeRejectable, payloadOnly, ignored);

    return norves::test::summary();
}
