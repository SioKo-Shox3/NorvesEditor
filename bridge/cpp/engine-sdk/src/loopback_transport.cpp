#include "norves/bridge/bounded_queue.hpp"
#include "norves/bridge/transport.hpp"

#include <cstddef>
#include <memory>
#include <optional>
#include <string>
#include <utility>

// プロセス内ループバックトランスポート。WebSocket は後のフェーズでありここでは
// モデル化されていない。これは F5 のエンドツーエンドラウンドトリップテストで使われる
// Rust の `loopback_pair` の C++ 対応物である。
//
// 設計: 各エンドポイントは 2 つの共有 BoundedFrameQueue ハンドルを所有する。1 つは
// 送り込む（INTO）先（自身の送信方向）、もう 1 つは受信する（FROM）元（自身の受信方向）
// であり、ピアとクロス配線されているため A の送信キューは B の受信キューであり、その逆も
// 成り立つ。BoundedFrameQueue はコピー不可 / move 不可なので、2 つのキューはヒープに
// 確保され std::shared_ptr で共有される。2 つのエンドポイントが唯一の所有者であり、
// キューは最後のエンドポイントとともに消滅する。
//
// ブロッキングな recv() == BoundedFrameQueue::wait_and_pop()。close() はこの
// エンドポイントの送信（OUTBOUND）キューをシャットダウンするが、それはピアの受信
// （INBOUND）キューである。ピアの wait_and_pop() はまだキューに残っているフレームを
// ドレインしたのち nullopt を返し、その読み取りループを終了させる。各方向は独立した
// キューなので、一方のエンドポイントでブロックした recv() がピアの send() を決して
// ブロックしない。
namespace norves::bridge
{

    namespace
    {

        class LoopbackTransport : public ITransport
        {
        public:
            // `outbound` はこのエンドポイントが送り込むキュー。`inbound` は受信元の
            // キュー。ピアは両者を入れ替えて構築される。
            LoopbackTransport(std::shared_ptr<BoundedFrameQueue> outbound,
                              std::shared_ptr<BoundedFrameQueue> inbound)
                : m_Outbound(std::move(outbound)), m_Inbound(std::move(inbound))
            {
            }

            bool send(std::string frame) override
            {
                // クローズされた送信キュー（自分が close() を呼んだか、ピアがティアダウン
                // 中）は push() を false を返す no-op にする。フレームはドロップされ、
                // 呼び出し側はトランスポートが消えたことを知る。OverflowPolicy::Reject では
                // 満杯（FULL）のキューもフレームを格納せず false を返すため、満杯の
                // トランスポートはデータを黙って失うのではなくバックプレッシャーを表面化
                // させる。これは ITransport::send() の契約に一致する。
                return m_Outbound->push(std::move(frame));
            }

            std::optional<std::string> recv() override
            {
                // フレームが届くか、自身の受信キューがシャットダウンされる（ピアが
                // クローズした）までブロックする。shutdown 後は残りのフレームをドレイン
                // したのち nullopt を生じる。これがクリーンな EOF のシグナルである。
                return m_Inbound->wait_and_pop();
            }

            void close() override
            {
                // ピアの recv() が読むキュー（自身の送信キュー）をクローズすることで、
                // ピアの recv() にストリーム終端を通知する。自身の受信キューはシャット
                // ダウンしない（NOT）。ピアが既に自分へ送ったフレームはドレイン可能な
                // ままであり、自身の recv() を終わらせるのはピアの責任である（ピアは
                // 自分が完了したときにクローズする）。
                m_Outbound->shutdown();
            }

        private:
            std::shared_ptr<BoundedFrameQueue> m_Outbound;
            std::shared_ptr<BoundedFrameQueue> m_Inbound;
        };

    }  // namespace

    std::pair<std::unique_ptr<ITransport>, std::unique_ptr<ITransport>> make_loopback_pair(
        std::size_t capacity)
    {
        // 方向ごとに 1 つのキュー。BoundedFrameQueue は容量 0 を 1 へクランプする。
        // OverflowPolicy::Reject にすることで、満杯のキューが最古のフレームを黙って退避
        // させるのではなく push()（ひいては send()）を false にする。満杯のトランスポートは
        // 呼び出し側へバックプレッシャーを表面化させなければならず、データを失っては
        // ならない。これは ITransport::send() の契約（クローズまたは満杯で false）に
        // 一致する。
        auto aToB = std::make_shared<BoundedFrameQueue>(capacity, OverflowPolicy::Reject);
        auto bToA = std::make_shared<BoundedFrameQueue>(capacity, OverflowPolicy::Reject);

        // エンドポイント A は aToB へ送り bToA から受信する。エンドポイント B はその鏡像で
        // ある。両キューはちょうど 2 つのエンドポイントで共有される。
        auto a = std::make_unique<LoopbackTransport>(aToB, bToA);
        auto b = std::make_unique<LoopbackTransport>(bToA, aToB);
        return {std::move(a), std::move(b)};
    }

}  // namespace norves::bridge
