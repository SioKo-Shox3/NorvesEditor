#include "Norves/Bridge/error.hpp"
#include "Norves/Bridge/result.hpp"
#include "Norves/Bridge/version.hpp"

#include <string>
#include <string_view>

#include "test_support.hpp"

namespace
{

    using Norves::Bridge::BridgeError;
    using Norves::Bridge::Result;

    void TestResultOk()
    {
        auto r = Result<int, std::string>::ok(42);
        NORVES_CHECK(r.is_ok());
        NORVES_CHECK(!r.is_err());
        NORVES_CHECK_EQ(r.value(), 42);
    }

    void TestResultErr()
    {
        auto r = Result<int, std::string>::err(std::string{"boom"});
        NORVES_CHECK(r.is_err());
        NORVES_CHECK(!r.is_ok());
        NORVES_CHECK_EQ(r.error(), std::string{"boom"});
    }

    void TestResultAssignment()
    {
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

    void TestErrorConstants()
    {
        NORVES_CHECK_EQ(Norves::Bridge::ErrorProtocolVersionUnsupported,
                        std::string_view{"PROTOCOL_VERSION_UNSUPPORTED"});
        NORVES_CHECK_EQ(Norves::Bridge::ErrorMethodNotSupported,
                        std::string_view{"METHOD_NOT_SUPPORTED"});
        NORVES_CHECK_EQ(Norves::Bridge::ErrorBridgeTransportError,
                        std::string_view{"BRIDGE_TRANSPORT_ERROR"});

        BridgeError err{std::string{Norves::Bridge::ErrorMethodNotSupported}, "no such method"};
        NORVES_CHECK_EQ(err.code, std::string{"METHOD_NOT_SUPPORTED"});
        NORVES_CHECK_EQ(err.message, std::string{"no such method"});
    }

    void TestSupportedProtocolVersions()
    {
        NORVES_CHECK_EQ(Norves::Bridge::SupportedProtocolVersions.size(),
                        static_cast<std::size_t>(2));
        NORVES_CHECK_EQ(Norves::Bridge::SupportedProtocolVersions[0], std::string_view{"0.2"});
        NORVES_CHECK(!Norves::Bridge::SdkVersion.empty());
    }

}  // namespace

int main()
{
    TestResultOk();
    TestResultErr();
    TestResultAssignment();
    TestErrorConstants();
    TestSupportedProtocolVersions();
    return norves::test::summary();
}
