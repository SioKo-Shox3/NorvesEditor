// @brief WebSocket サーバートランスポートテスト（Workstream G / G3）。
//
// Norves::Bridge::make_websocket_server_transport（公開の、lws フリーの ITransport シーム）を
// 検証するため、このテスト TU 内でインプロセスの libwebsockets クライアントを立ち上げ、
// フルラウンドトリップを実行する。クライアントはここで lws を直接使用する。これは
// 境界ルールが SDK の公開 include/ ヘッダに lws を含めることのみを禁止しており、
// テストには適用されないため問題ない。以下で使用する SDK ヘッダ
// （ws_server_transport.hpp）は lws 型を一切露出しない。
//
// カバレッジ:
//   1. ラウンドトリップ: クライアント->サーバ recv()、サーバ->クライアント send()
//   2. 複数フレームを連続送信した場合、順序を維持して到着すること（B1）
//   3. 大きなフレームが部分書き込みの再アーミングを経ても完全長で一致すること
//   4. シングル接続姿勢: 2 番目の接続が拒否され、1 番目が存続すること
//   5. close() 契約: recv() が nullopt にドレインされる、send() が false を返す、
//      close() が冪等（2 回呼び出し）
//   6. バインド失敗: 同一ポートに 2 番目のトランスポートを作成すると nullptr が返る（Warn）
//   7. バインドは 127.0.0.1（ループバック）: 127.0.0.1 経由の接続が機能すること

#include "Norves/Bridge/ws_server_transport.hpp"

#include "Norves/Bridge/log_sink.hpp"

#include <atomic>
#include <chrono>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include <libwebsockets.h>

#include "test_support.hpp"

namespace
{

    using namespace std::chrono_literals;

    // @brief テストが Warn/Error 診断をアサートできるようにログ行を収集する。
    class CapturingSink : public Norves::Bridge::ILogSink
    {
    public:
        void log(Norves::Bridge::LogSeverity level, std::string_view message) override
        {
            std::lock_guard<std::mutex> lk(m_Mutex);
            m_Lines.emplace_back(level, std::string(message));
        }
        bool saw(Norves::Bridge::LogSeverity level)
        {
            std::lock_guard<std::mutex> lk(m_Mutex);
            for (const auto& [lvl, msg] : m_Lines)
            {
                if (lvl == level)
                {
                    return true;
                }
            }
            return false;
        }

    private:
        std::mutex m_Mutex;
        std::vector<std::pair<Norves::Bridge::LogSeverity, std::string>> m_Lines;
    };

    // -- libwebsockets テストクライアント -------------------------------------------
    //
    // 独自のコンテキスト + サービススレッドで動作する小規模なシングル接続クライアント。
    // 継続フラグメントをまとめてメッセージに再アセンブルし（SDK サーバのミラー）、
    // サービススレッドからオンデマンドでサーバにフレームを送信できる。

    struct TestClient
    {
        struct PerSession
        {
            std::string acc;  // フラグメント再アセンブル用バッファ
            TestClient* owner = nullptr;
        };

        std::atomic<bool> connected{false};
        std::atomic<bool> stop{false};
        std::atomic<bool> connection_error{false};

        std::mutex rx_mutex;
        std::vector<std::string> received;  // 完全なメッセージ（受信順）

        std::mutex tx_mutex;
        std::vector<std::string> to_send;  // 送信キュー（ペイロード）

        struct lws_context* ctx = nullptr;
        struct lws* wsi = nullptr;
        std::thread thread;
        PerSession session;

        static int cb(struct lws* wsi, enum lws_callback_reasons reason, void* user, void* in,
                      size_t len)
        {
            auto* ps = static_cast<PerSession*>(user);
            switch (reason)
            {
                case LWS_CALLBACK_CLIENT_ESTABLISHED:
                    ps->owner->connected.store(true);
                    break;
                case LWS_CALLBACK_CLIENT_RECEIVE:
                {
                    ps->acc.append(static_cast<char*>(in), len);
                    if (lws_is_final_fragment(wsi) && lws_remaining_packet_payload(wsi) == 0)
                    {
                        std::lock_guard<std::mutex> lk(ps->owner->rx_mutex);
                        ps->owner->received.push_back(ps->acc);
                        ps->acc.clear();
                    }
                    break;
                }
                case LWS_CALLBACK_CLIENT_WRITEABLE:
                {
                    std::string payload;
                    {
                        std::lock_guard<std::mutex> lk(ps->owner->tx_mutex);
                        if (ps->owner->to_send.empty())
                        {
                            break;
                        }
                        payload = std::move(ps->owner->to_send.front());
                        ps->owner->to_send.erase(ps->owner->to_send.begin());
                    }
                    std::vector<unsigned char> buf(LWS_PRE + payload.size());
                    std::memcpy(buf.data() + LWS_PRE, payload.data(), payload.size());
                    lws_write(wsi, buf.data() + LWS_PRE, payload.size(), LWS_WRITE_TEXT);
                    {
                        std::lock_guard<std::mutex> lk(ps->owner->tx_mutex);
                        if (!ps->owner->to_send.empty())
                        {
                            lws_callback_on_writable(wsi);
                        }
                    }
                    break;
                }
                case LWS_CALLBACK_CLIENT_CONNECTION_ERROR:
                    ps->owner->connection_error.store(true);
                    break;
                default:
                    break;
            }
            return 0;
        }

        void start(std::uint16_t port)
        {
            const struct lws_protocols protocols[] = {
                {"norves-bridge", &TestClient::cb, sizeof(PerSession), 0, 0, nullptr, 0},
                LWS_PROTOCOL_LIST_TERM,
            };
            lws_set_log_level(LLL_ERR | LLL_WARN, nullptr);

            struct lws_context_creation_info info;
            std::memset(&info, 0, sizeof(info));
            info.port = CONTEXT_PORT_NO_LISTEN;
            info.protocols = protocols;
            info.gid = -1;
            info.uid = -1;
            ctx = lws_create_context(&info);

            session.owner = this;

            struct lws_client_connect_info ci;
            std::memset(&ci, 0, sizeof(ci));
            ci.context = ctx;
            ci.address = "127.0.0.1";
            ci.port = port;
            ci.path = "/";
            ci.host = "127.0.0.1";
            ci.origin = "127.0.0.1";
            ci.protocol = "norves-bridge";
            ci.userdata = &session;
            wsi = lws_client_connect_via_info(&ci);

            thread = std::thread(
                [this]
                {
                    while (!stop.load())
                    {
                        lws_service(ctx, 50);
                        // 保留中の送信要求をサービススレッド上で処理する: lws_callback_on_writable
                        // を呼び出せるのはこのスレッドのみ。
                        if (arm_writable.exchange(false) && wsi != nullptr)
                        {
                            bool has_data;
                            {
                                std::lock_guard<std::mutex> lk(tx_mutex);
                                has_data = !to_send.empty();
                            }
                            if (has_data)
                            {
                                lws_callback_on_writable(wsi);
                            }
                        }
                    }
                    lws_context_destroy(ctx);
                    ctx = nullptr;
                });
        }

        void enqueue(const std::string& payload)
        {
            {
                std::lock_guard<std::mutex> lk(tx_mutex);
                to_send.push_back(payload);
            }
            // スレッド外: フラグをセットしてループを起床させるだけ。ループ（サービススレッド）が
            // フラグを lws_callback_on_writable に変換する。
            arm_writable.store(true);
            if (ctx != nullptr)
            {
                lws_cancel_service(ctx);
            }
        }

        std::atomic<bool> arm_writable{false};

        size_t received_count()
        {
            std::lock_guard<std::mutex> lk(rx_mutex);
            return received.size();
        }

        std::vector<std::string> snapshot()
        {
            std::lock_guard<std::mutex> lk(rx_mutex);
            return received;
        }

        void shutdown()
        {
            stop.store(true);
            if (ctx != nullptr)
            {
                lws_cancel_service(ctx);
            }
            if (thread.joinable())
            {
                thread.join();
            }
        }
    };

    template <typename Pred>
    bool WaitUntil(Pred pred, std::chrono::milliseconds timeout)
    {
        auto deadline = std::chrono::steady_clock::now() + timeout;
        while (std::chrono::steady_clock::now() < deadline)
        {
            if (pred())
            {
                return true;
            }
            std::this_thread::sleep_for(5ms);
        }
        return pred();
    }

}  // namespace

int main()
{
    using Norves::Bridge::LogSeverity;
    using Norves::Bridge::make_websocket_server_transport;

    const std::uint16_t Port = 39071;
    constexpr std::size_t SendCap = 256;
    constexpr std::size_t RecvCap = 256;

    // ---- セットアップ: サーバートランスポート + 接続済みクライアント ----------------
    CapturingSink sink;
    auto server = make_websocket_server_transport(Port, SendCap, RecvCap, &sink);
    NORVES_CHECK(server != nullptr);
    if (server == nullptr)
    {
        return norves::test::summary();
    }

    TestClient client;
    client.start(Port);

    bool bConnected = WaitUntil([&] { return client.connected.load(); }, 5s);
    NORVES_CHECK(bConnected);

    // テスト 1 + 7: クライアント -> サーバ ラウンドトリップ（127.0.0.1
    // リッスンが機能することを証明）。
    client.enqueue("hello-from-client");
    auto got = server->recv();
    NORVES_CHECK(got.has_value());
    if (got.has_value())
    {
        NORVES_CHECK_EQ(*got, std::string("hello-from-client"));
    }

    // テスト 1: サーバ -> クライアント
    NORVES_CHECK(server->send("hello-from-server"));
    bool bGotOne = WaitUntil([&] { return client.received_count() >= 1; }, 5s);
    NORVES_CHECK(bGotOne);
    if (bGotOne)
    {
        NORVES_CHECK_EQ(client.snapshot().at(0), std::string("hello-from-server"));
    }

    // テスト 2: 複数フレームが順序を維持して到着すること（B1）
    const int Burst = 8;
    for (int i = 0; i < Burst; ++i)
    {
        NORVES_CHECK(server->send("burst-" + std::to_string(i)));
    }
    bool bGotBurst = WaitUntil([&] { return client.received_count() >= 1 + Burst; }, 5s);
    NORVES_CHECK(bGotBurst);
    if (bGotBurst)
    {
        auto frames = client.snapshot();
        for (int i = 0; i < Burst; ++i)
        {
            NORVES_CHECK_EQ(frames.at(1 + i), "burst-" + std::to_string(i));
        }
    }

    // テスト 3: 大きなフレームが部分書き込みの再アーミングを経ても完全長で一致すること
    const std::size_t Big = 50000;
    std::string big = "BIG:";
    big.append(Big, 'X');
    NORVES_CHECK(server->send(big));
    const size_t expectAfterBig = 1 + Burst + 1;
    bool bGotBig = WaitUntil([&] { return client.received_count() >= expectAfterBig; }, 10s);
    NORVES_CHECK(bGotBig);
    if (bGotBig)
    {
        auto frames = client.snapshot();
        const std::string& last = frames.at(expectAfterBig - 1);
        NORVES_CHECK_EQ(last.size(), big.size());
        NORVES_CHECK(last == big);
    }

    // テスト 4: シングル接続姿勢 -- 2 番目のクライアントが拒否され、1 番目が存続すること。
    {
        TestClient client2;
        client2.start(Port);
        // サーバは ESTABLISHED（-1）で拒否するため、client2 はエラーになるか
        // 使用可能なセッションを確立せずにクローズする。いずれにせよ 1 番目の
        // クライアントはその後も機能し続けなければならない。
        WaitUntil([&] { return client2.connection_error.load(); }, 3s);
        client2.shutdown();
    }
    // 1 番目のクライアントが依然として機能すること:
    NORVES_CHECK(server->send("after-reject"));
    bool bStillOk = WaitUntil([&] { return client.received_count() >= expectAfterBig + 1; }, 5s);
    NORVES_CHECK(bStillOk);
    if (bStillOk)
    {
        auto frames = client.snapshot();
        NORVES_CHECK_EQ(frames.back(), std::string("after-reject"));
    }

    client.shutdown();

    // テスト 5: close() 契約 -- recv() が nullopt にドレインされる、send() が false を返す、
    // 冪等性（2 回呼び出し）。
    server->close();
    auto afterClose = server->recv();
    NORVES_CHECK(!afterClose.has_value());           // nullopt にドレインされた
    NORVES_CHECK(server->send("dropped") == false);  // クローズ済み => false
    server->close();                                 // 冪等: ハング/クラッシュしてはならない

    // テスト 6: バインド失敗 -- 同一ポートに 2 番目のトランスポートを作成すると nullptr
    // が返ること。
    {
        // 新しいサーバで同一ポートに再バインドし、次に重複を試みる。
        CapturingSink sinkA;
        auto a = make_websocket_server_transport(Port, SendCap, RecvCap, &sinkA);
        NORVES_CHECK(a != nullptr);

        CapturingSink sinkDup;
        auto dup = make_websocket_server_transport(Port, SendCap, RecvCap, &sinkDup);
        NORVES_CHECK(dup == nullptr);
        NORVES_CHECK(sinkDup.saw(LogSeverity::Warn));

        if (a != nullptr)
        {
            a->close();
        }
    }

    return norves::test::summary();
}
