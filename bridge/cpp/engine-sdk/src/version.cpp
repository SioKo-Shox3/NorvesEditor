// Minimal translation unit so the STATIC library always has at least one
// object file. It also anchors a compile-time check that the version header is
// self-consistent. No runtime symbols are exported yet (F2 will add them).
#include "norves/bridge/version.hpp"

namespace norves::bridge::detail {

static_assert(!kSupportedProtocolVersions.empty(),
              "the SDK must support at least one protocol version");
static_assert(!kSdkVersion.empty(), "kSdkVersion must be non-empty");

}  // namespace norves::bridge::detail
