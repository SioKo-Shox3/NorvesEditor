#pragma once

#include <array>
#include <string_view>

// Standalone C++ engine SDK version metadata.
// Depends on <std> only; no third-party headers are included here.
namespace norves::bridge
{

    // Version of this SDK build (independent from the wire protocol version).
    inline constexpr std::string_view SdkVersion = "0.1.0";

    // Wire protocol versions this SDK supports, as MAJOR.MINOR strings.
    //
    // Matching rule: an exact MAJOR.MINOR string equality against an element of
    // this set. There is NO semver range / compatibility-window logic here: a
    // peer's protocol version is accepted only if it is byte-for-byte one of these
    // strings. The F3 version-negotiation logic must consult this constant rather
    // than embedding literals, so the supported set lives in exactly one place.
    inline constexpr std::array<std::string_view, 1> SupportedProtocolVersions = {
        std::string_view{"0.1"},
    };

}  // namespace norves::bridge
