// BoundedFrameQueue unit test (F4).
//
// Verifies: FIFO push/pop and size/capacity; the three OverflowPolicy variants
// (DropOldest / DropNewest / Reject) at capacity; blocking wait_and_pop across
// threads; shutdown waking a blocked waiter; post-shutdown push no-op and
// drain-then-nullopt semantics; and Warn diagnostics on drop via a fake sink.
//
// Only std + the SDK's public headers are used; the boundary rule (no nlohmann
// in include/) is unaffected.

#include "norves/bridge/bounded_queue.hpp"

#include "norves/bridge/log_sink.hpp"

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

    using norves::bridge::BoundedFrameQueue;
    using norves::bridge::ILogSink;
    using norves::bridge::LogSeverity;
    using norves::bridge::OverflowPolicy;

    // Records Warn-level messages so a test can assert drop diagnostics fire.
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
        // Overflow: oldest ("1") evicted, "4" appended; push reports success.
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
        // Overflow: incoming dropped, push reports failure, contents unchanged.
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

        // Blocks until the producer pushes.
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

        // Push after shutdown is a no-op returning false.
        NORVES_CHECK(!q.push("z"));
        NORVES_CHECK_EQ(q.size(), static_cast<std::size_t>(2));

        // wait_and_pop drains the remaining frames first, then returns nullopt.
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
