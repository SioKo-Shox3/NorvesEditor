#include "norves/bridge/ws_server_transport.hpp"

#include "norves/bridge/bounded_queue.hpp"
#include "norves/bridge/log_sink.hpp"
#include "norves/bridge/transport.hpp"

#include <atomic>
#include <cstddef>
#include <cstring>
#include <deque>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <thread>
#include <utility>
#include <vector>

#include <libwebsockets.h>

// libwebsockets を裏側に持つ WebSocket サーバトランスポート（Workstream G / G3）。
//
// これは <libwebsockets.h> を include する唯一の（ONLY）翻訳単位である。ライブラリは
// PRIVATE でリンクされ、すべての lws 型はこのファイル内に留まる。公開ヘッダは std +
// SDK 自身の ITransport / ILogSink の接続点以外、何も露出しない。このファイルが実装する
// スレッド規律については ADR 0007 を参照。
//
// スレッドモデル（1 つのサービススレッドがすべての（ALL）libwebsockets 状態を所有する）:
//   - サービススレッド: lws_create_context -> lws_service ループ -> lws_context_
//     destroy。フレームを受信し（LWS_CALLBACK_RECEIVE）、フラグメントを再構成し、
//     それらを m_RecvQueue へ push する。m_SendQueue をドレインし、すべての lws_write を
//     LWS_CALLBACK_SERVER_WRITEABLE 内で実行する。部分書き込みの再アームにより、次の
//     フレームが始まる前に 1 つのフレームが完了する（順序が保たれる）。単一のアクティブな
//     wsi を所有し、LWS_CALLBACK_CLOSED でそれをクリアする。
//   - 外部の send(): m_SendQueue へ push し（OverflowPolicy::Reject => 満杯で false）、
//     ループを起こすために lws_cancel_service() する。wsi/context には決して（NEVER）
//     触れない。
//   - コンシューマの recv(): m_RecvQueue で wait_and_pop()。close+ドレイン後は nullopt。
//   - close(): close フラグを立てて lws_cancel_service() し、両方のキューをシャットダウン
//     することで、ブロックした recv() が起きて nullopt へドレインし、send() が false を
//     返すようにする。実際の wsi/context のティアダウンはサービススレッド上で起こる。
//     冪等: フラグと一度だけガードされた join により、2 度目の close()/デストラクタが
//     安全になる。
//
// 単一接続の姿勢（alpha）: 一度にちょうど 1 つのエディタクライアント。同時の 2 つ目の
// 接続は LWS_CALLBACK_ESTABLISHED で拒否される（REJECTED。-1 を返し、新しい wsi を
// クローズする）ため、既存のクライアントが妨げられることは決してない。アクティブな
// クライアントが切断した後、次の接続は受け入れられる（G5 の再接続はこれに依存する）。
namespace norves::bridge
{

    namespace
    {

        // フレームごとの送信状態。部分書き込みのカーソルを運ぶ。
        struct OutFrame
        {
            std::vector<unsigned char> buf;  // LWS_PRE のパディング + ペイロード
            std::size_t payload_len = 0;
            std::size_t sent = 0;  // すでに書き込まれたペイロードバイト数
        };

        OutFrame MakeOutFrame(const std::string& payload)
        {
            OutFrame f;
            f.buf.resize(LWS_PRE + payload.size());
            if (!payload.empty())
            {
                std::memcpy(f.buf.data() + LWS_PRE, payload.data(), payload.size());
            }
            f.payload_len = payload.size();
            return f;
        }

        class WebSocketServerTransport : public ITransport
        {
        public:
            WebSocketServerTransport(std::size_t sendCapacity, std::size_t recvCapacity,
                                     ILogSink* logSink)
                : m_LogSink(logSink),
                  // 送信: バックプレッシャー。満杯の送信キューは、フレームを退避させる
                  // のではなく send() を false にする。
                  m_SendQueue(sendCapacity, OverflowPolicy::Reject, logSink),
                  // 受信: 決して黙ってドロップしない。我々自身がオーバーフローを致命的
                  // （接続をクローズする）として扱う。Reject は push() が false を返すこと
                  // を意味し、サービススレッドがオーバーフローを検出して対処できる。
                  m_RecvQueue(recvCapacity, OverflowPolicy::Reject, logSink)
            {
            }

            ~WebSocketServerTransport() override { close(); }

            WebSocketServerTransport(const WebSocketServerTransport&) = delete;
            WebSocketServerTransport& operator=(const WebSocketServerTransport&) = delete;
            WebSocketServerTransport(WebSocketServerTransport&&) = delete;
            WebSocketServerTransport& operator=(WebSocketServerTransport&&) = delete;

            // lws コンテキストを生成し（127.0.0.1:port にバインド）、サービススレッドを
            // 開始する。バインド / 生成失敗時は false を返す（スレッドは開始されず、
            // コンテキストは破棄される）。
            bool start(std::uint16_t port)
            {
                lws_set_log_level(LLL_ERR | LLL_WARN, nullptr);

                struct lws_context_creation_info info;
                std::memset(&info, 0, sizeof(info));
                info.port = static_cast<int>(port);
                info.iface = "127.0.0.1";  // ループバックのみ。決して 0.0.0.0 ではない
                info.protocols = m_Protocols;
                info.gid = -1;
                info.uid = -1;
                info.user = this;  // lws_context_user 経由で静的コールバックから到達可能

                m_Context = lws_create_context(&info);
                if (m_Context == nullptr)
                {
                    warn("failed to create lws context / bind 127.0.0.1:" + std::to_string(port) +
                         " (port in use?)");
                    return false;
                }

                m_ServiceThread = std::thread([this] { service_loop(); });
                return true;
            }

            bool send(std::string frame) override
            {
                if (m_bClosed.load(std::memory_order_acquire))
                {
                    return false;
                }
                // 送信キューへ push し（Reject => 満杯で false = バックプレッシャー）、
                // サービススレッドを起こす。ここでは wsi/context に触れない（NOT）。
                if (!m_SendQueue.push(std::move(frame)))
                {
                    return false;
                }
                if (m_Context != nullptr)
                {
                    lws_cancel_service(m_Context);
                }
                return true;
            }

            std::optional<std::string> recv() override
            {
                // フレームが届くか m_RecvQueue がシャットダウンされる（close()）まで
                // ブロックする。その後、残りのフレームをドレインして nullopt を生じる。
                return m_RecvQueue.wait_and_pop();
            }

            void close() override
            {
                // 冪等: 最初の呼び出し側だけがフラグを反転させ、キューをシャットダウンし、
                // サービスループを起こして join する。
                bool expected = false;
                if (!m_bClosed.compare_exchange_strong(expected, true, std::memory_order_acq_rel))
                {
                    return;
                }

                // ブロックした recv() を起こしてドレインさせ nullopt を返させる。また
                // （フラグ設定後の）以後の send() を false にする。
                m_RecvQueue.shutdown();
                m_SendQueue.shutdown();

                // サービススレッドにループから抜けるよう依頼する。lws_cancel_service は
                // 唯一のスレッド間で安全な lws 呼び出しである。実際のティアダウンはその
                // スレッド上で行われる。
                if (m_Context != nullptr)
                {
                    lws_cancel_service(m_Context);
                }
                if (m_ServiceThread.joinable())
                {
                    m_ServiceThread.join();
                }
            }

        private:
            // -- この行より下はサービススレッド専用（注記のある箇所を除く） -------------

            void service_loop()
            {
                while (!m_bClosed.load(std::memory_order_acquire))
                {
                    // タイムアウト時、受信トラフィック時、または lws_cancel_service
                    // （send() / close()）によって起こされたときに返る。すべての
                    // wsi/書き込み作業は、ここからディスパッチされるコールバック内で起こる。
                    lws_service(m_Context, 50);
                    pump_writable();
                }
                // クローズ直前にエンキューされたフレームが、クライアントがまだ生きていれば
                // writable リクエストを尊重されるよう、最後にもう一度ドレインする。生きて
                // いなければ無害。
                pump_writable();
                lws_context_destroy(m_Context);
                // ここで m_Context を意図的に null にしない（NOT）。start()（サービス
                // スレッドが存在する前にこれを書き、happens-before を確立する）の後、
                // m_Context は二度と書かれないため、読み取り専用の共有値である。すなわち
                // サービススレッドは lws_service() でそれを読み、外部の close()/send() は
                // join() の前に m_bClosed ガードの下でそれを読む。ここで nullptr を書くと
                // それら外部の読み取りと競合する。lws_context_destroy はハンドルを
                // ダングリングにするが、その後どの経路もそれを逆参照しない。close() は
                // 冪等であり（CAS は m_Context に触れる前に 2 番目の呼び出し側を拒否する）、
                // m_bClosed が一度立つと send() は false を返すため、lws_cancel_service も
                // lws_service も再び走らない。
            }

            // アクティブな接続と保留中の送信フレームがある場合、lws に writable コール
            // バックを要求する。サービススレッドからのみ呼ばれる。
            void pump_writable()
            {
                if (m_ActiveWsi != nullptr && (m_CurrentOut.has_value() || m_SendQueue.size() > 0))
                {
                    lws_callback_on_writable(m_ActiveWsi);
                }
            }

            // LWS_CALLBACK_SERVER_WRITEABLE のハンドラ（サービススレッド）。たかだか 1 つの
            // チャンクを送る。現在のフレームが完全に書き込まれるまで再アームし、その後
            // 次へ進む。
            int on_writable(struct lws* wsi)
            {
                if (wsi != m_ActiveWsi)
                {
                    return 0;  // 古い wsi。無視する
                }
                if (!m_CurrentOut.has_value())
                {
                    auto next = m_SendQueue.pop();
                    if (!next.has_value())
                    {
                        return 0;  // 今は送るものがない
                    }
                    m_CurrentOut = MakeOutFrame(*next);
                }

                OutFrame& fr = *m_CurrentOut;
                std::size_t remaining = fr.payload_len - fr.sent;
                std::size_t attempt = remaining < ChunkCap ? remaining : ChunkCap;

                const bool bFirstChunk = (fr.sent == 0);
                const bool bLastChunk = (fr.sent + attempt) >= fr.payload_len;

                int flags = bFirstChunk ? LWS_WRITE_TEXT : LWS_WRITE_CONTINUATION;
                if (!bLastChunk)
                {
                    flags |= LWS_WRITE_NO_FIN;
                }

                // 継続チャンクは、fr.buf の既に送信済みの領域を lws_write の LWS_PRE
                // 接頭辞のためのスクラッチとして再利用する。それらのペイロードバイトは
                // 既にワイヤーへ書き込まれており二度と読み戻されないため、ここで上書き
                // することは安全である。
                unsigned char* start = fr.buf.data() + LWS_PRE + fr.sent;
                int n = lws_write(wsi, start, attempt, static_cast<enum lws_write_protocol>(flags));
                if (n < 0)
                {
                    warn("lws_write failed; closing connection");
                    return -1;  // この wsi をクローズする
                }
                // 部分書き込み: 受理された分だけ進め、同じ（SAME）フレームを新しい
                // オフセットから再開するよう再アームする（バイト順は保たれる）。
                fr.sent += static_cast<std::size_t>(n);
                if (fr.sent >= fr.payload_len)
                {
                    m_CurrentOut.reset();  // フレーム完了。次の writable が次のものを取る
                }
                if (m_CurrentOut.has_value() || m_SendQueue.size() > 0)
                {
                    lws_callback_on_writable(wsi);
                }
                return 0;
            }

            // LWS_CALLBACK_RECEIVE のハンドラ（サービススレッド）。継続フラグメントを 1 つの
            // メッセージへ再構成し、それを m_RecvQueue へ push する。
            int on_receive(struct lws* wsi, void* in, std::size_t len)
            {
                if (wsi != m_ActiveWsi)
                {
                    return 0;  // アクティブな接続ではない。無視する
                }
                if (in != nullptr && len > 0)
                {
                    m_RecvAcc.append(static_cast<const char*>(in), len);
                }
                if (lws_is_final_fragment(wsi) && lws_remaining_packet_payload(wsi) == 0)
                {
                    std::string message = std::move(m_RecvAcc);
                    m_RecvAcc.clear();
                    // 受信オーバーフローは致命的（FATAL）である。受信フレームを失うと
                    // リクエスト/レスポンスの相関が壊れる。満杯の Reject キューでは push() が
                    // false を返す。その場合は接続をクローズし、回復は上位層（G5）の
                    // 再接続に委ねる。
                    if (!m_RecvQueue.push(std::move(message)))
                    {
                        warn(
                            "recv queue full; closing connection (frame loss would "
                            "break correlation)");
                        if (m_LogSink != nullptr)
                        {
                            m_LogSink->log(LogSeverity::Error,
                                           "ws_server_transport: recv overflow");
                        }
                        return -1;  // この wsi をクローズする
                    }
                }
                return 0;
            }

            int on_established(struct lws* wsi)
            {
                if (m_ActiveWsi != nullptr)
                {
                    // 単一接続の alpha 姿勢: 既存のクライアントを維持し、新参者を拒否する
                    // （-1 を返すと新しい wsi のみがクローズされる）。
                    warn("rejecting second connection (single editor client only)");
                    return -1;
                }
                m_ActiveWsi = wsi;
                m_RecvAcc.clear();
                return 0;
            }

            void on_closed(struct lws* wsi)
            {
                if (wsi == m_ActiveWsi)
                {
                    m_ActiveWsi = nullptr;  // この wsi には二度と触れない
                    m_RecvAcc.clear();
                    m_CurrentOut.reset();  // 消えたクライアントへの送信途中のフレームを捨てる
                }
            }

            // 静的トランポリン: コンテキストの user ポインタからインスタンスを復元し、
            // メンバハンドラへディスパッチする。サービススレッド上で走る。
            static int callback(struct lws* wsi, enum lws_callback_reasons reason, void* /*user*/,
                                void* in, std::size_t len)
            {
                auto* self =
                    static_cast<WebSocketServerTransport*>(lws_context_user(lws_get_context(wsi)));
                if (self == nullptr)
                {
                    return 0;
                }
                switch (reason)
                {
                    case LWS_CALLBACK_ESTABLISHED:
                        return self->on_established(wsi);
                    case LWS_CALLBACK_SERVER_WRITEABLE:
                        return self->on_writable(wsi);
                    case LWS_CALLBACK_RECEIVE:
                        return self->on_receive(wsi, in, len);
                    case LWS_CALLBACK_CLOSED:
                        self->on_closed(wsi);
                        return 0;
                    default:
                        return 0;
                }
            }

            void warn(const std::string& message)
            {
                if (m_LogSink != nullptr)
                {
                    m_LogSink->log(LogSeverity::Warn, "ws_server_transport: " + message);
                }
            }

            // 大きなフレームが部分書き込みの再アーム経路を行使するよう、単一の lws_write を
            // 上限で制限する。コールバックごとの作業も有界に保つ。
            static constexpr std::size_t ChunkCap = 4096;

            const struct lws_protocols m_Protocols[2] = {
                {"norves-bridge", &WebSocketServerTransport::callback, 0, 0, 0, nullptr, 0},
                LWS_PROTOCOL_LIST_TERM,
            };

            ILogSink* m_LogSink;  // 所有しない。null でよい

            // キューはスレッドセーフ。外部の send()/コンシューマの recv() および
            // サービススレッドから触られる。
            BoundedFrameQueue m_SendQueue;
            BoundedFrameQueue m_RecvQueue;

            std::atomic<bool> m_bClosed{false};

            std::thread m_ServiceThread;

            // サービススレッド専用の状態。
            struct lws_context* m_Context = nullptr;
            struct lws* m_ActiveWsi = nullptr;
            std::string m_RecvAcc;                 // フラグメント再構成バッファ
            std::optional<OutFrame> m_CurrentOut;  // 飛行中のフレーム（部分書き込み）
        };

    }  // namespace

    std::unique_ptr<ITransport> make_websocket_server_transport(std::uint16_t port,
                                                                std::size_t sendCapacity,
                                                                std::size_t recvCapacity,
                                                                ILogSink* logSink)
    {
        auto transport =
            std::make_unique<WebSocketServerTransport>(sendCapacity, recvCapacity, logSink);
        if (!transport->start(port))
        {
            return nullptr;  // バインド / コンテキスト生成に失敗（すでに Warn でログ済み）
        }
        // lws を含まない公開接続点へアップキャストする。呼び出し側は lws ハンドルを
        // 一切見ない。
        return transport;
    }

}  // namespace norves::bridge
