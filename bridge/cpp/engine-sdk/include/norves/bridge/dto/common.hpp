#pragma once

#include <optional>
#include <string_view>

/// @file
/// @brief 型付きペイロード DTO（F5）のための共有ワイヤー enum。
///
/// @note 依存は <std> のみ。サードパーティヘッダはここに露出しない。これらは
///       bridge/spec/schema/common.schema.json の enum $def（および Rust リファレンス
///       bridge/crates/norves-bridge-core/src/common.rs）を 1 対 1 で反映する。下記の
///       ワイヤー文字列は正規の契約である。enum メンバまたはそのワイヤー文字列の
///       いずれかを変えることはプロトコル変更である。
///
/// @note F5 ラウンドトリップで実際に使われる enum（engineState, runtimeState, logLevel）
///       のみがモデル化されている。viewportState / origin および開かれた値 $def
///       （propertyValue, sceneNode, ...）はここでは意図的に型付けしない（NOT）。それらは
///       後のフェーズが必要とするまで opaque（JsonValue として運ばれる）のままである。
namespace norves::bridge::dto
{

    /// @brief common.schema.json#/$defs/engineState: ["initializing","ready","running","error"]。
    enum class EngineState
    {
        Initializing,
        Ready,
        Running,
        Error
    };

    /// @brief common.schema.json#/$defs/runtimeState:
    /// ["edit","playing","paused","stopped","unknown"]。
    enum class RuntimeState
    {
        Edit,
        Playing,
        Paused,
        Stopped,
        Unknown
    };

    /// @brief common.schema.json#/$defs/logLevel: ["trace","debug","info","warn","error"]。
    enum class LogLevel
    {
        Trace,
        Debug,
        Info,
        Warn,
        Error
    };

    /// @brief ワイヤー文字列変換。to_wire は全域的である（すべての列挙子が文字列を持つ）。
    ///        from_wire は未知のメンバに対して nullopt を返し、serde による列挙範囲外の値の
    ///        拒否を反映する。
    [[nodiscard]] std::string_view to_wire(EngineState value);
    [[nodiscard]] std::string_view to_wire(RuntimeState value);
    [[nodiscard]] std::string_view to_wire(LogLevel value);

    [[nodiscard]] std::optional<EngineState> engine_state_from_wire(std::string_view text);
    [[nodiscard]] std::optional<RuntimeState> runtime_state_from_wire(std::string_view text);
    [[nodiscard]] std::optional<LogLevel> log_level_from_wire(std::string_view text);

}  // namespace norves::bridge::dto
