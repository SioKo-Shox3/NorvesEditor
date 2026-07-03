#pragma once

#include "Norves/Bridge/codec_error.hpp"
#include "Norves/Bridge/envelope.hpp"
#include "Norves/Bridge/result.hpp"

#include <string>
#include <string_view>

/// @file
/// @brief Bridge ワイヤー Envelope のための JSON コーデックのエントリポイント。
///
/// @note 依存は <std> と SDK 自身の値型のみ。サードパーティヘッダはここに含めない。
///       ベンダリングされた JSON ライブラリは .cpp 実装内でのみ使われる。Rust リファレンス
///       （`bridge/crates/norves-bridge-core/src/codec.rs`）を反映する。すなわち
///       ENVELOPE 層のみが検証される。メソッドごと / イベントごとのペイロードスキーマは
///       後のフェーズであり、params / result / error.data は opaque なまま運ばれる。
namespace Norves::Bridge
{

    /// @brief JSON 文字列を Envelope へデコードし、エンベロープ層の規則を適用する:
    ///   * 有効な JSON オブジェクトであること、
    ///   * エンベロープおよび error オブジェクトでの additionalProperties: false、
    ///   * フィールドパターン（bridge マーカー、version、method/event 名、error.code、
    ///     非空の id、seq >= 0）、
    ///   * Envelope::validate を介した kind 依存の構造規則。
    /// params / result / error.data は opaque な JsonValue として保存され、それ以上
    /// 検査されない。
    /// @param wire デコード対象のワイヤー JSON テキスト。
    /// @return 成功時は Envelope。いずれかの違反時は CodecError を返す。
    [[nodiscard]] Result<Envelope, CodecError> decode_envelope(std::string_view wire);

    /// @brief Envelope をコンパクトな JSON 文字列へエンコードして戻す。
    /// @param envelope エンコード対象のエンベロープ。
    /// @return 成功時は JSON 文字列。エンベロープをシリアライズできない場合は CodecError。
    [[nodiscard]] Result<std::string, CodecError> encode_envelope(const Envelope& envelope);

}  // namespace Norves::Bridge
