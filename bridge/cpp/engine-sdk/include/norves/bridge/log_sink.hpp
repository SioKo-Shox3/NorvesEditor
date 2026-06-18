#pragma once

#include <string_view>

/// @file
/// @brief エンジン SDK のログシンク。
///
/// @note 依存は <std> のみ。サードパーティヘッダはここに含めない。SDK のログは
///       標準ストリームへ直接書き込むことは決してない（cpp.md は tests/examples 以外での
///       標準ストリームログを禁止している）。SDK が発するすべての診断は、組み込み側が
///       供給する ILogSink を経由してルーティングされる。シンクが供給されない場合、
///       SDK は沈黙する。
namespace norves::bridge
{

    /// @brief ログ重大度。common.schema.json#/$defs/logLevel
    ///        （trace/debug/info/warn/error）を反映し、SDK 診断がワイヤーの logLevel
    ///        語彙へきれいに対応するようにする。
    enum class LogSeverity
    {
        Trace,
        Debug,
        Info,
        Warn,
        Error
    };

    /// @brief 純粋仮想のログシンク。組み込み側がこれを実装し、SDK 診断を好きな場所
    ///        （自前のロガー、ファイル、UI チャネル）へルーティングする。
    ///
    /// @note 寿命 / スレッドアフィニティ: SDK は SDK を駆動するスレッド（例えば
    ///       BridgeEngineServer::handleFrame を呼んだスレッド）上で log() を同期的に呼ぶ。
    ///       実装はそのスレッド上で呼び出して安全でなければならない。共有状態に触れる場合は
    ///       実装側が同期を所有する。`message` は呼び出しの間のみ有効な借用ビューであり、
    ///       保持してはならない。
    class ILogSink
    {
    public:
        virtual ~ILogSink() = default;

        /// @brief 指定された重大度で 1 行のログを発する。
        /// @param level ログ重大度。
        /// @param message この呼び出しの間のみ借用されるログメッセージ。
        virtual void log(LogSeverity level, std::string_view message) = 0;

    protected:
        ILogSink() = default;
        ILogSink(const ILogSink&) = default;
        ILogSink(ILogSink&&) = default;
        ILogSink& operator=(const ILogSink&) = default;
        ILogSink& operator=(ILogSink&&) = default;
    };

}  // namespace norves::bridge
