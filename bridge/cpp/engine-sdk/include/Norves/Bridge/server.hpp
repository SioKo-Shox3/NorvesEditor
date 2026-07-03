#pragma once

#include "Norves/Bridge/json_value.hpp"

#include <memory>
#include <optional>
#include <string>
#include <string_view>

/// @file
/// @brief Bridge のエンジン側リクエストディスパッチャ。
///
/// @note 依存は <std> と SDK 自身の値型のみ。サードパーティヘッダはここに含めない。
///       基底の JSON ライブラリは server.cpp に閉じ込められている。
///
/// @note BridgeEngineServer は受信ワイヤーフレームをデコードし、bridge.hello の
///       バージョンネゴシエーションを所有し、既知のリクエストを IBridgeEngineAdapter へ
///       ディスパッチし、レスポンスフレームをエンコードする。トランスポート
///       （ソケットの読み書き）は後のフェーズであり、この型の一部ではない。組み込み側が
///       ワイヤーフレームを供給し、返されたワイヤーフレームを送出する。
namespace Norves::Bridge
{

    class IBridgeEngineAdapter;
    class ILogSink;

    class BridgeEngineServer
    {
    public:
        /// @brief `adapter`（参照で保持）とオプションの `logSink`（沈黙サーバのためには
        ///        nullptr でよい）に束縛されたサーバを構築する。
        ///
        /// @note 所有権 / 寿命: サーバは `adapter` への参照と `logSink` への raw ポインタを
        ///       格納するが、どちらも所有しない（NEITHER）。呼び出し側はサーバの全寿命の間、
        ///       両者を生存させ続けなければならない（MUST。adapter と sink はサーバより
        ///       長生きする）。サーバはこれ以外の長寿命状態を格納しない。
        /// @param adapter リクエストのディスパッチ先となるエンジンアダプタ（参照で保持）。
        /// @param logSink オプションのログシンク。沈黙サーバのためには nullptr でよい。
        explicit BridgeEngineServer(IBridgeEngineAdapter& adapter, ILogSink* logSink = nullptr);

        ~BridgeEngineServer();

        BridgeEngineServer(const BridgeEngineServer&) = delete;
        BridgeEngineServer& operator=(const BridgeEngineServer&) = delete;
        BridgeEngineServer(BridgeEngineServer&&) noexcept;
        BridgeEngineServer& operator=(BridgeEngineServer&&) noexcept;

        /// @brief 受信ワイヤーフレームを 1 つ処理し、送出すべきレスポンスワイヤーフレームが
        ///        あればそれを返す。
        ///
        /// @note `wire` の寿命: この呼び出しの間のみ借用される。サーバはそれをデコードし
        ///       （opaque なペイロードを含め、必要なものをすべてコピーアウトする）、返る前に
        ///       同期的にアダプタを呼び出す。サーバは `wire` へのビューを一切保持しない。
        ///       `wire` はこの呼び出しが返るまで有効であればよい。返される std::string
        ///       （存在する場合）は呼び出し側が所有する。
        ///
        /// @return 次の場合に std::nullopt（送出すべきレスポンスなし）を返す:
        ///   * フレームのデコードに失敗した場合（不正なフレームは回復可能な相関 id を
        ///     持たないため、有効なレスポンスエンベロープを構築できない。失敗は Warn で
        ///     ログシンクへ報告される）、
        ///   * フレームがリクエストではなくレスポンスまたはイベントである場合（サーバは
        ///     リクエストのみを処理する。Debug でログされる）。
        /// それ以外の場合は、エンコード済みのレスポンスエンベロープ（成功時は result、
        /// 失敗時はワイヤーエラー）を、リクエストの相関 id を反映して返す。
        [[nodiscard]] std::optional<std::string> handleFrame(std::string_view wire);

        /// @brief `eventName` と `params` を運ぶイベントエンベロープ（kind=event）を構築・
        ///        エンコードし、ワイヤーフレームを返す。それを送出するのは組み込み側の仕事で
        ///        ある（トランスポートは後のフェーズ）。`eventName` の例: "log.message",
        ///        "engine.statusChanged"。`params` はエンベロープへコピーされる。
        [[nodiscard]] std::string emitEvent(std::string_view eventName, const JsonValue& params);

    private:
        /// pImpl はディスパッチテーブルの内部と JSON ライブラリの利用を、この公開ヘッダの
        /// 外に保つ。
        struct Impl;
        std::unique_ptr<Impl> m_Impl;
    };

}  // namespace Norves::Bridge
