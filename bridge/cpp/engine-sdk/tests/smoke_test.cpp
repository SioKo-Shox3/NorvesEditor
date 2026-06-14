#include <string>
#include <string_view>

#include "norves/bridge/error.hpp"
#include "norves/bridge/result.hpp"
#include "norves/bridge/version.hpp"
#include "test_support.hpp"

namespace {

using norves::bridge::BridgeError;
using norves::bridge::Result;

void test_result_ok() {
    auto r = Result<int, std::string>::ok(42);
    NORVES_CHECK(r.is_ok());
    NORVES_CHECK(!r.is_err());
    NORVES_CHECK_EQ(r.value(), 42);
}

void test_result_err() {
    auto r = Result<int, std::string>::err(std::string{"boom"});
    NORVES_CHECK(r.is_err());
    NORVES_CHECK(!r.is_ok());
    NORVES_CHECK_EQ(r.error(), std::string{"boom"});
}

void test_result_assignment() {
    auto a = Result<int, std::string>::ok(1);
    auto b = Result<int, std::string>::err(std::string{"e"});
    a = b;
    NORVES_CHECK(a.is_err());
    NORVES_CHECK_EQ(a.error(), std::string{"e"});

    auto c = Result<int, std::string>::ok(7);
    a = std::move(c);
    NORVES_CHECK(a.is_ok());
    NORVES_CHECK_EQ(a.value(), 7);
}

void test_error_constants() {
    NORVES_CHECK_EQ(norves::bridge::kErrorProtocolVersionUnsupported,
                    std::string_view{"PROTOCOL_VERSION_UNSUPPORTED"});
    NORVES_CHECK_EQ(norves::bridge::kErrorMethodNotSupported,
                    std::string_view{"METHOD_NOT_SUPPORTED"});
    NORVES_CHECK_EQ(norves::bridge::kErrorBridgeTransportError,
                    std::string_view{"BRIDGE_TRANSPORT_ERROR"});

    BridgeError err{std::string{norves::bridge::kErrorMethodNotSupported},
                    "no such method"};
    NORVES_CHECK_EQ(err.code, std::string{"METHOD_NOT_SUPPORTED"});
    NORVES_CHECK_EQ(err.message, std::string{"no such method"});
}

void test_supported_protocol_versions() {
    NORVES_CHECK_EQ(norves::bridge::kSupportedProtocolVersions.size(),
                    static_cast<std::size_t>(1));
    NORVES_CHECK_EQ(norves::bridge::kSupportedProtocolVersions[0],
                    std::string_view{"0.1"});
    NORVES_CHECK(!norves::bridge::kSdkVersion.empty());
}

}  // namespace

int main() {
    test_result_ok();
    test_result_err();
    test_result_assignment();
    test_error_constants();
    test_supported_protocol_versions();
    return norves::test::summary();
}
