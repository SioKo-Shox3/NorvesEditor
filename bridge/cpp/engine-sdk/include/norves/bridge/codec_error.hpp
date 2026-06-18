#pragma once

#include <string>
#include <utility>

/// @file
/// @brief Bridge エンベロープのデコードまたは検証中に発生するローカル失敗。
///
/// @note 依存は <std> のみ。サードパーティヘッダはここに含めない。これは Rust リファレンス
///       の `CodecError`（`bridge/crates/norves-bridge-core/src/error.rs`）の C++ 対応物で
///       あり、ローカルかつワイヤー上には決して載らない処理失敗である。BridgeError
///       （ワイヤーのエラーオブジェクト）とは区別される。
namespace norves::bridge
{

    /// @brief デコード/検証失敗の粗い分類。これらの種別は Rust CodecError のバリアントを、
    ///        エンベロープ層で意味を持つカテゴリへ畳み込んだものである。具体的な詳細は
    ///        人間可読の message が運ぶ。
    enum class CodecErrorKind
    {
        /// JSON パース失敗（不正な入力）。
        Parse,
        /// 未知 / 想定外のフィールド（additionalProperties: false 違反）。
        UnknownField,
        /// フィールド値がそのパターン / 型 / 存在制約に違反した
        /// （bridge マーカー、version、method/event 名、error コード、id、seq、...）。
        InvalidField,
        /// エンベロープが kind 依存の構造制約に違反した
        /// （Rust CodecError::StructuralViolation を反映）。
        StructuralViolation,
    };

    /// @brief ローカルなデコード/検証エラー値。
    struct CodecError
    {
        CodecErrorKind kind = CodecErrorKind::Parse;
        std::string message;

        [[nodiscard]] bool operator==(const CodecError& other) const
        {
            return kind == other.kind && message == other.message;
        }
        [[nodiscard]] bool operator!=(const CodecError& other) const { return !(*this == other); }

        static CodecError parse(std::string message)
        {
            return CodecError{CodecErrorKind::Parse, std::move(message)};
        }
        static CodecError unknown_field(std::string message)
        {
            return CodecError{CodecErrorKind::UnknownField, std::move(message)};
        }
        static CodecError invalid_field(std::string message)
        {
            return CodecError{CodecErrorKind::InvalidField, std::move(message)};
        }
        static CodecError structural_violation(std::string message)
        {
            return CodecError{CodecErrorKind::StructuralViolation, std::move(message)};
        }
    };

}  // namespace norves::bridge
