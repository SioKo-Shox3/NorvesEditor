// STATIC ライブラリが常に少なくとも 1 つのオブジェクトファイルを持つようにするための
// 最小の翻訳単位。version ヘッダが自己整合していることのコンパイル時チェックの拠り所
// でもある。ランタイムシンボルはまだエクスポートされていない（F2 が追加する）。
#include "Norves/Bridge/version.hpp"

namespace Norves::Bridge::Detail
{

    static_assert(!SupportedProtocolVersions.empty(),
                  "the SDK must support at least one protocol version");
    static_assert(!SdkVersion.empty(), "SdkVersion must be non-empty");

}  // namespace Norves::Bridge::Detail
