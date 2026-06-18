#pragma once

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <variant>

/// @file
/// @brief 正規の Bridge ワイヤーエンベロープ。Rust リファレンス実装
///        （`bridge/crates/norves-bridge-core/src/envelope.rs`）および
///        `envelope.schema.json` を反映する。
///
/// @note 依存は <std> と SDK 自身の値型のみ。サードパーティヘッダはここに含めない。
///       `params`、`result`、`error.data` は opaque な JsonValue として運ばれ、この層では
///       解釈されない。
namespace norves::bridge
{

    /// @brief NorvesEditor Bridge のプロトコルマーカー定数。
    inline constexpr std::string_view BridgeMarker = "norves.editor.bridge";

    /// @brief エンベロープの判別子。スキーマ: enum ["request", "response", "event"]。
    enum class Kind
    {
        Request,
        Response,
        Event
    };

    /// @brief フラットなワイヤーエンベロープ。kind ごとのフィールド存在規則は構築では
    ///        強制されない。Rust リファレンスの Envelope::validate を反映するクロス
    ///        フィールド構造規則を適用するには validate() を呼ぶこと。
    struct Envelope
    {
        /// プロトコルマーカー。ワイヤー上では常に定数 BridgeMarker。値がそのまま
        /// ラウンドトリップするよう文字列として運ぶ。
        std::string bridge;
        /// プロトコルバージョン文字列、MAJOR.MINOR。
        std::string version;
        /// エンベロープの判別子。
        Kind kind = Kind::Request;
        /// リクエスト/レスポンスの相関 id。
        std::optional<std::string> id;
        /// リクエスト上のメソッド名。
        std::optional<std::string> method;
        /// イベントエンベロープ上のイベント名。
        std::optional<std::string> event;
        /// メソッドまたはイベントのペイロード（opaque なオブジェクト）。
        std::optional<JsonValue> params;
        /// レスポンス上の成功ペイロード（opaque）。error と相互排他。
        std::optional<JsonValue> result;
        /// レスポンス上のエラーペイロード。result と相互排他。
        std::optional<BridgeError> error;
        /// ハンドシェイク中に割り当てられるオプションのセッション id。
        std::optional<std::string> session_id;
        /// オプションの、接続ごとに単調増加するシーケンス番号。
        /// Rust リファレンス（u64）およびスキーマの integer minimum: 0 を反映して
        /// 符号なし。ワイヤー上の負値はデコード時に拒否される。
        std::optional<std::uint64_t> seq;

        [[nodiscard]] bool operator==(const Envelope& other) const
        {
            return bridge == other.bridge && version == other.version && kind == other.kind &&
                   id == other.id && method == other.method && event == other.event &&
                   params == other.params && result == other.result && error == other.error &&
                   session_id == other.session_id && seq == other.seq;
        }
        [[nodiscard]] bool operator!=(const Envelope& other) const { return !(*this == other); }

        /// @brief envelope.schema.json の allOf による kind 依存の構造制約を強制する。
        ///        Rust の Envelope::validate を 1:1 で反映する:
        ///   * request  - id と method が必須。result, error, event は禁止。
        ///   * response - id が必須。result / error のうち厳密に 1 つ。
        ///                method, event, params は禁止。
        ///   * event    - event が必須。id, method, result, error は禁止。
        /// @return 最初の違反を記述する CodecError::StructuralViolation。
        [[nodiscard]] Result<std::monostate, CodecError> validate() const;
    };

}  // namespace norves::bridge
