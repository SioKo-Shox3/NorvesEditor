#pragma once

#include <array>
#include <string_view>

/// @file
/// @brief スタンドアロン C++ エンジン SDK のバージョンメタデータ。
/// @note 依存は <std> のみ。サードパーティヘッダはここに含めない。
namespace Norves::Bridge
{

    /// @brief この SDK ビルドのバージョン（ワイヤープロトコルバージョンとは独立）。
    inline constexpr std::string_view SdkVersion = "0.1.0";

    /// @brief この SDK が対応するワイヤープロトコルバージョン（MAJOR.MINOR 文字列）。
    ///
    /// @note 突き合わせ規則: この集合の要素との厳密な MAJOR.MINOR 文字列一致による。
    ///       semver 範囲 / 互換ウィンドウのロジックは一切ない。ピアのプロトコル
    ///       バージョンは、これらの文字列のいずれかとバイト単位で一致する場合に限り
    ///       受理される。F3 のバージョンネゴシエーションロジックはリテラルを埋め込まず
    ///       この定数を参照しなければならず、対応集合がただ一箇所に存在するようにする。
    inline constexpr std::array<std::string_view, 2> SupportedProtocolVersions = {
        std::string_view{"0.2"},
        std::string_view{"0.1"},
    };

}  // namespace Norves::Bridge
