#include "norves/bridge/envelope.hpp"

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/result.hpp"

#include <variant>

// validate() は Rust リファレンスの Envelope::validate
// （bridge/crates/norves-bridge-core/src/envelope.rs, fn validate）の 1:1 移植である。
// 2 つの実装が同じ最初の違反を報告するよう、チェックの順序と違反メッセージは同一に
// 保たれている。
namespace norves::bridge
{

    namespace
    {

        Result<std::monostate, CodecError> Violation(const char* message)
        {
            return Result<std::monostate, CodecError>::err(
                CodecError::structural_violation(message));
        }

        Result<std::monostate, CodecError> Ok()
        {
            return Result<std::monostate, CodecError>::ok(std::monostate{});
        }

    }  // namespace

    Result<std::monostate, CodecError> Envelope::validate() const
    {
        switch (kind)
        {
            case Kind::Request:
                if (!id.has_value())
                {
                    return Violation("request envelope requires `id`");
                }
                if (!method.has_value())
                {
                    return Violation("request envelope requires `method`");
                }
                if (result.has_value())
                {
                    return Violation("request envelope must not carry `result`");
                }
                if (error.has_value())
                {
                    return Violation("request envelope must not carry `error`");
                }
                if (event.has_value())
                {
                    return Violation("request envelope must not carry `event`");
                }
                break;
            case Kind::Response:
            {
                if (!id.has_value())
                {
                    return Violation("response envelope requires `id`");
                }
                const bool bHasResult = result.has_value();
                const bool bHasError = error.has_value();
                if (bHasResult && bHasError)
                {
                    return Violation("response envelope must not carry both `result` and `error`");
                }
                if (!bHasResult && !bHasError)
                {
                    return Violation(
                        "response envelope requires exactly one of `result` or `error`");
                }
                if (method.has_value())
                {
                    return Violation("response envelope must not carry `method`");
                }
                if (event.has_value())
                {
                    return Violation("response envelope must not carry `event`");
                }
                if (params.has_value())
                {
                    return Violation("response envelope must not carry `params`");
                }
                break;
            }
            case Kind::Event:
                if (!event.has_value())
                {
                    return Violation("event envelope requires `event`");
                }
                if (id.has_value())
                {
                    return Violation("event envelope must not carry `id`");
                }
                if (method.has_value())
                {
                    return Violation("event envelope must not carry `method`");
                }
                if (result.has_value())
                {
                    return Violation("event envelope must not carry `result`");
                }
                if (error.has_value())
                {
                    return Violation("event envelope must not carry `error`");
                }
                break;
        }
        return Ok();
    }

}  // namespace norves::bridge
