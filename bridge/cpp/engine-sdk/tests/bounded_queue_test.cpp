// @brief BoundedFrameQueue ユニットテスト（F4）。
//
// 検証項目: FIFO の push/pop と size/capacity; 容量上限時の 3 種 OverflowPolicy
// （DropOldest / DropNewest / Reject）; スレッド間の wait_and_pop ブロッキング;
// shutdown によるブロック解除; シャットダウン後の push が no-op であること、
// およびドレイン後に nullopt を返すセマンティクス; drop 発生時に
// Warn 診断が偽シンクへ届くこと。
//
// std とSDKの公開ヘッダのみを使用する。境界ルール（include/ に nlohmann を
// 含めない）には影響しない。

#include "Norves/Bridge/bounded_queue.hpp"

#include "Norves/Bridge/log_sink.hpp"

#include <chrono>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

#include "test_support.hpp"

namespace
{

    using Norves::Bridge::BoundedFrameQueue;
    using Norves::Bridge::ILogSink;
    using Norves::Bridge::LogSeverity;
    using Norves::Bridge::OverflowPolicy;

    // @brief Warn レベルのメッセージを記録し、テストが drop 診断の発火を検証できるようにする。
    class RecordingSink final : public ILogSink
    {
    public:
        void log(LogSeverity level, std::string_view message) override
        {
            std::lock_guard<std::mutex> lock(m_Mutex);
            if (level == LogSeverity::Warn)
            {
                ++m_WarnCount;
                m_LastWarn = std::string(message);
            }
        }

        int warn_count() const
        {
            std::lock_guard<std::mutex> lock(m_Mutex);
            return m_WarnCount;
        }

    private:
        mutable std::mutex m_Mutex;
        int m_WarnCount = 0;
        std::string m_LastWarn;
    };

    void TestBasicFifo()
    {
        BoundedFrameQueue q(4);
        NORVES_CHECK_EQ(q.capacity(), static_cast<std::size_t>(4));
        NORVES_CHECK_EQ(q.size(), static_cast<std::size_t>(0));
        NORVES_CHECK(!q.closed());
        NORVES_CHECK(!q.pop().has_value());

        NORVES_CHECK(q.push("a"));
        NORVES_CHECK(q.push("b"));
        NORVES_CHECK(q.push("c"));
        NORVES_CHECK_EQ(q.size(), static_cast<std::size_t>(3));

        auto a = q.pop();
        NORVES_CHECK(a.has_value() && *a == "a");
        auto b = q.pop();
        NORVES_CHECK(b.has_value() && *b == "b");
        auto c = q.pop();
        NORVES_CHECK(c.has_value() && *c == "c");
        NORVES_CHECK(!q.pop().has_value());
        NORVES_CHECK_EQ(q.size(), static_cast<std::size_t>(0));
    }

    void TestCapacityClampedToOne()
    {
        BoundedFrameQueue q(0);
        NORVES_CHECK_EQ(q.capacity(), static_cast<std::size_t>(1));
        NORVES_CHECK(q.push("x"));
        NORVES_CHECK_EQ(q.size(), static_cast<std::size_t>(1));
    }

    void TestDropOldest()
    {
        RecordingSink sink;
        BoundedFrameQueue q(3, OverflowPolicy::DropOldest, &sink);
        NORVES_CHECK(q.push("1"));
        NORVES_CHECK(q.push("2"));
        NORVES_CHECK(q.push("3"));
        // オーバーフロー: 最古の要素（"1"）が退出され "4" が末尾に追加される。push は成功を返す。
        NORVES_CHECK(q.push("4"));
        NORVES_CHECK_EQ(q.size(), static_cast<std::size_t>(3));

        auto a = q.pop();
        auto b = q.pop();
        auto c = q.pop();
        NORVES_CHECK(a.has_value() && *a == "2");
        NORVES_CHECK(b.has_value() && *b == "3");
        NORVES_CHECK(c.has_value() && *c == "4");
        NORVES_CHECK_EQ(sink.warn_count(), 1);
    }

    void TestDropNewest()
    {
        RecordingSink sink;
        BoundedFrameQueue q(2, OverflowPolicy::DropNewest, &sink);
        NORVES_CHECK(q.push("1"));
        NORVES_CHECK(q.push("2"));
        // オーバーフロー: 新着要素が破棄され push は失敗を返す。既存の内容は変化しない。
        NORVES_CHECK(!q.push("3"));
        NORVES_CHECK_EQ(q.size(), static_cast<std::size_t>(2));

        auto a = q.pop();
        auto b = q.pop();
        NORVES_CHECK(a.has_value() && *a == "1");
        NORVES_CHECK(b.has_value() && *b == "2");
        NORVES_CHECK_EQ(sink.warn_count(), 1);
    }

    void TestReject()
    {
        RecordingSink sink;
        BoundedFrameQueue q(2, OverflowPolicy::Reject, &sink);
        NORVES_CHECK(q.push("1"));
        NORVES_CHECK(q.push("2"));
        NORVES_CHECK(!q.push("3"));
        NORVES_CHECK_EQ(q.size(), static_cast<std::size_t>(2));

        auto a = q.pop();
        auto b = q.pop();
        NORVES_CHECK(a.has_value() && *a == "1");
        NORVES_CHECK(b.has_value() && *b == "2");
        NORVES_CHECK_EQ(sink.warn_count(), 1);
    }

    void TestWaitAndPopAcrossThreads()
    {
        BoundedFrameQueue q(8);
        std::thread producer(
            [&q]
            {
                std::this_thread::sleep_for(std::chrono::milliseconds(20));
                q.push("hello");
            });

        // プロデューサーが push するまでブロックする。
        auto frame = q.wait_and_pop();
        producer.join();
        NORVES_CHECK(frame.has_value() && *frame == "hello");
    }

    void TestShutdownWakesWaiter()
    {
        BoundedFrameQueue q(4);
        std::optional<std::string> result;
        bool bResultSet = false;
        std::mutex resultMutex;

        std::thread consumer(
            [&]
            {
                auto r = q.wait_and_pop();
                std::lock_guard<std::mutex> lock(resultMutex);
                result = r;
                bResultSet = true;
            });

        std::this_thread::sleep_for(std::chrono::milliseconds(20));
        q.shutdown();
        consumer.join();

        std::lock_guard<std::mutex> lock(resultMutex);
        NORVES_CHECK(bResultSet);
        NORVES_CHECK(!result.has_value());
        NORVES_CHECK(q.closed());
    }

    void TestPostShutdownDrainThenNullopt()
    {
        BoundedFrameQueue q(4);
        NORVES_CHECK(q.push("x"));
        NORVES_CHECK(q.push("y"));
        q.shutdown();

        // シャットダウン後の push は no-op で false を返す。
        NORVES_CHECK(!q.push("z"));
        NORVES_CHECK_EQ(q.size(), static_cast<std::size_t>(2));

        // wait_and_pop は残存フレームを先にドレインし、その後 nullopt を返す。
        auto a = q.wait_and_pop();
        auto b = q.wait_and_pop();
        NORVES_CHECK(a.has_value() && *a == "x");
        NORVES_CHECK(b.has_value() && *b == "y");
        NORVES_CHECK(!q.wait_and_pop().has_value());
        NORVES_CHECK(!q.pop().has_value());
    }

}  // namespace

int main()
{
    TestBasicFifo();
    TestCapacityClampedToOne();
    TestDropOldest();
    TestDropNewest();
    TestReject();
    TestWaitAndPopAcrossThreads();
    TestShutdownWakesWaiter();
    TestPostShutdownDrainThenNullopt();
    return norves::test::summary();
}
