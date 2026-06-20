// @brief Workstream H-A: 常駐型 WebSocket モックエンジン。
//
// エディタバックエンドが起動、接続、駆動（hello / status / runtime control /
// ログストリーミング）、停止できるスタンドアロンの長時間動作エンジンプロセス。
// G4 ws_test_server（Rust 専用テストハーネスとして残る）の本番形態の対応物:
// そのハーネスとは異なり、このプロセスは「常駐型（RESIDENTIAL）」であり、
// クライアント切断ではプロセスが終了しない。明示的な停止（シグナル -> close()）まで
// 次の接続を待ち続ける。
//
// 境界: この TU は SDK 公開ヘッダのみをインクルードする。WebSocket ライブラリは
// ITransport pImpl の背後に隠蔽されているため、libwebsockets / nlohmann はここには
// 現れず、/W4 も維持される。
//
// エディタバックエンド / ランチャーとのライフサイクル契約:
//   * argv: --bridge-port <p> は必須。不正・欠落ポートはハードエラー（非ゼロ終了）となり、
//     設定ミスは即座に失敗する（ws_test_server::ParsePort と同一契約）。
//   * バインド成功後は "READY <port>\n" を stdout に出力してフラッシュする。
//     ランチャーはダイヤル前にこれを待機する。stdout はこの 1 行専用とし、
//     診断はすべて stderr に出力する。
//   * その後、1 つの常駐 recv ループを実行する: 受信した各ワイヤーフレームを
//     BridgeEngineServer::handleFrame に渡し、レスポンスがあれば返送し、
//     log.subscribe の ack 後は SEVERAL 個の log.message イベントを連続発行する。
//
// 停止 / シグナル安全性（必読）:
//   * recv() は close() の後にのみ nullopt を返す（クライアント切断ではブロックを
//     解除しない。トランスポートは次の接続を待機する）。よってクリーンな停止は、
//     transport->close() を駆動することを意味し、これによりブロック中の recv() が
//     nullopt を返してループが終了する。
//   * シグナル / コンソールハンドラは async-signal-safe でなければならない。
//     transport->close() はミューテックスを取得し、condvar にシグナルを送り、
//     サービススレッドを join する — これらはどれも POSIX シグナルハンドラ内では
//     合法ではない。そのため、ハンドラは async-signal-safe な唯一の操作のみを行う:
//     atomic フラグへの g_stop.store(true)。
//   * main() で起動する専用のウォッチャースレッドが g_stop をポーリングし、
//     セットされたら transport->close() を呼び出す。それにより recv() ループの
//     ブロックが解除される。これは POSIX と Windows で同一である（ハンドラは
//     常に atomic のみを扱う）ため、OS 固有の終了パスはない — ハンドラの
//     「登録」のみが異なり、#if defined(_WIN32) で分岐する。

#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <thread>

#if defined(_WIN32)
#include <windows.h>
#else
#include <csignal>
#endif

#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/dto/events.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/log_sink.hpp"
#include "norves/bridge/server.hpp"
#include "norves/bridge/transport.hpp"
#include "norves/bridge/ws_server_transport.hpp"

#include "mock_adapter.hpp"

namespace
{

    using norves::bridge::BridgeEngineServer;
    using norves::bridge::ILogSink;
    using norves::bridge::ITransport;
    using norves::bridge::LogSeverity;
    using norves::bridge::make_websocket_server_transport;

    using norves::bridge::dto::LogLevel;
    using norves::bridge::dto::LogMessageEvent;

    using norves::mock::MockAdapter;

    // @brief log.subscribe の ack 後に連続発行する log.message イベントの数。
    // @note クライアントが順序付き複数フレーム配送（G4
    // バーストパターン）をアサートできるようにする。
    constexpr int LogBurst = 3;

    // @brief 翻訳単位の停止フラグ。シグナル / コンソールハンドラのみがセットする
    // （atomic store は async-signal-safe）。ウォッチャースレッドがポーリングし、
    // 実際の close() を実行する。ハンドラ内から close() を直接呼び出さない。
    std::atomic<bool> g_stop{false};

#if defined(_WIN32)
    // @brief Windows コンソール制御ハンドラ。OS が注入したスレッドで実行されるが、
    // 停止パスを POSIX と同一にするため atomic store のみに限定する。
    BOOL WINAPI ConsoleHandler(DWORD ctrlType)
    {
        switch (ctrlType)
        {
            case CTRL_C_EVENT:
            case CTRL_BREAK_EVENT:
            case CTRL_CLOSE_EVENT:
            case CTRL_SHUTDOWN_EVENT:
                g_stop.store(true);
                return TRUE;
            default:
                return FALSE;
        }
    }

    void InstallSignalHandlers() { SetConsoleCtrlHandler(ConsoleHandler, TRUE); }
#else
    extern "C" void PosixSignalHandler(int /*signum*/)
    {
        // Async-signal-safe: atomic store はシグナルハンドラ内で許可されている。
        g_stop.store(true);
    }

    void InstallSignalHandlers()
    {
        std::signal(SIGINT, PosixSignalHandler);
        std::signal(SIGTERM, PosixSignalHandler);
    }
#endif

    // @brief 最小限の stderr シンク。SDK の Warn/Error 診断を可視化する。
    // @note stdout は単一の READY 行に予約されているため、診断は stderr のみに出力する。
    class StderrSink : public ILogSink
    {
    public:
        void log(LogSeverity level, std::string_view message) override
        {
            if (level == LogSeverity::Warn || level == LogSeverity::Error)
            {
                std::cerr << "mock-engine[sink]: " << message << '\n';
            }
        }
    };

    // @brief --bridge-port を読み取る。成功時はポートを返す。不正・欠落時は
    // stderr にエラーを出力して nullopt を返す（ws_test_server::ParsePort と同一契約）。
    std::optional<std::uint16_t> ParsePort(int argc, char** argv)
    {
        for (int i = 1; i < argc; ++i)
        {
            std::string_view arg = argv[i];
            if (arg == "--bridge-port")
            {
                if (i + 1 >= argc)
                {
                    std::cerr << "mock-engine: --bridge-port requires a value\n";
                    return std::nullopt;
                }
                const std::string value = argv[i + 1];
                try
                {
                    const unsigned long parsed = std::stoul(value);
                    if (parsed == 0 || parsed > 65535)
                    {
                        std::cerr << "mock-engine: port out of range: " << value << '\n';
                        return std::nullopt;
                    }
                    return static_cast<std::uint16_t>(parsed);
                }
                catch (const std::exception&)
                {
                    std::cerr << "mock-engine: invalid port: " << value << '\n';
                    return std::nullopt;
                }
            }
        }
        std::cerr << "mock-engine: missing required --bridge-port <port>\n";
        return std::nullopt;
    }

    // @brief WebSocket サーバーをバインドする。一時的なバインド失敗時は短い sleep を
    // 挟みながらリトライする。kill->同一ポート再起動では OS が前のリスナーを解放するまで
    // 一時的に失敗することがある。リトライによって吸収する（ws_test_server と同一アプローチ）。
    // すべての試行が失敗した場合のみ nullptr を返す。
    std::unique_ptr<ITransport> BindWithRetry(std::uint16_t port, std::size_t sendCap,
                                              std::size_t recvCap, ILogSink* sink)
    {
        constexpr int MaxAttempts = 20;
        constexpr auto RetryDelay = std::chrono::milliseconds(100);
        for (int attempt = 0; attempt < MaxAttempts; ++attempt)
        {
            std::unique_ptr<ITransport> transport =
                make_websocket_server_transport(port, sendCap, recvCap, sink);
            if (transport != nullptr)
            {
                return transport;
            }
            std::this_thread::sleep_for(RetryDelay);
        }
        return nullptr;
    }

}  // namespace

int main(int argc, char** argv)
{
    const std::optional<std::uint16_t> port = ParsePort(argc, argv);
    if (!port.has_value())
    {
        return 2;
    }

    constexpr std::size_t SendCap = 256;
    constexpr std::size_t RecvCap = 256;

    StderrSink sink;
    std::unique_ptr<ITransport> transport = BindWithRetry(*port, SendCap, RecvCap, &sink);
    if (transport == nullptr)
    {
        std::cerr << "mock-engine: failed to bind WebSocket server on port " << *port << '\n';
        return 3;
    }

    InstallSignalHandlers();

    // ウォッチャースレッド: g_stop をポーリングし、シグナルハンドラから close() を駆動する。
    // close() は冪等なため、後続のデストラクタによる close() は無害。
    // transport の生ポインタをキャプチャする（join 済みのウォッチャーより長く生存する）。
    ITransport* transportPtr = transport.get();
    std::thread watcher(
        [transportPtr]()
        {
            constexpr auto Poll = std::chrono::milliseconds(50);
            while (!g_stop.load())
            {
                std::this_thread::sleep_for(Poll);
            }
            transportPtr->close();  // 常駐 recv() ループのブロックを解除する。
        });

    MockAdapter adapter;
    BridgeEngineServer server(adapter, &sink);

    // log.message イベントフレームを事前に 1 回ビルドする。log.subscribe の ack 後に
    // （LogBurst 回）発行される。
    LogMessageEvent log;
    log.level = LogLevel::Info;
    log.message = "Game started";
    log.category = "Engine";
    const std::string logEventFrame = server.emitEvent("log.message", log.to_json());

    // バインド成功後に準備完了を通知する。これにより ランチャーはリッスン済みの
    // ソケットにのみダイヤルする。stdout はこの 1 行専用とし、
    // 親プロセスが即座に受け取れるようフラッシュする。
    std::cout << "READY " << *port << '\n';
    std::cout.flush();

    // 常駐 recv ループ。recv() は close() の後にのみ nullopt を返す（ウォッチャーが
    // 停止時に駆動する）。クライアント切断ではループが終了しない。handleFrame と
    // アダプタはこのスレッドで実行されるため、logSubscribe 内でセットされた
    // emit_log_burst フラグは handleFrame が返った直後に参照できる。subscribe の
    // ack を先に送信し、その後バーストを送るため、クライアントは ack-before-events の
    // 順序で受信する。
    while (true)
    {
        std::optional<std::string> frame = transport->recv();
        if (!frame.has_value())
        {
            break;  // close() が駆動された（停止要求）: クリーン終了。
        }

        std::optional<std::string> response = server.handleFrame(*frame);
        if (response.has_value())
        {
            if (!transport->send(std::move(*response)))
            {
                // フライト中にピアがいなくなった。終了しない: このエンジンは常駐型のため、
                // このフレームを破棄し次の接続の提供を継続する。レスポンスに紐づく保留中の
                // 発行フラグはすべてここでクリアし、次のリクエストに残留しないようにする
                // （emit_log_burst と対称）。
                adapter.emit_log_burst.store(false);
                adapter.emit_object_changed.store(false);
                adapter.emit_scene_tree_changed.store(false);
                continue;
            }
        }

        if (adapter.emit_log_burst.exchange(false))
        {
            for (int i = 0; i < LogBurst; ++i)
            {
                if (!transport->send(std::string(logEventFrame)))
                {
                    break;  // フライト中にピアがいなくなった。バーストを中断し提供を継続する。
                }
            }
        }

        // Phase 6: setProperty の ack 後にライブ更新イベントを発行する。フレームは実行時に
        // 1 回ビルドする（params は更新済みのインメモリ状態に依存するため）。ack を先に送って
        // あるので、レスポンスを id で相関しイベントを別扱いする conformance ランナーの
        // exact-match を壊さない（log.message バーストと同じ共存パターン）。1 リクエストあたり
        // 最大 2 イベントであり SendCap=256 を超えない。
        if (adapter.emit_object_changed.exchange(false))
        {
            const std::string objectChangedFrame =
                server.emitEvent("object.changed", adapter.object_changed_params());
            transport->send(std::string(objectChangedFrame));
        }
        if (adapter.emit_scene_tree_changed.exchange(false))
        {
            const std::string sceneTreeChangedFrame =
                server.emitEvent("scene.treeChanged", MockAdapter::scene_tree_changed_params());
            transport->send(std::string(sceneTreeChangedFrame));
        }
    }

    // 順序ある終了: ウォッチャーを停止して join し、その後 transport unique_ptr の
    // デストラクタが再度 close() を呼び出す（冪等）。シグナル以外のパスでループが
    // 終了した場合もウォッチャーが終了するよう g_stop をセットする。
    g_stop.store(true);
    if (watcher.joinable())
    {
        watcher.join();
    }

    return 0;
}
