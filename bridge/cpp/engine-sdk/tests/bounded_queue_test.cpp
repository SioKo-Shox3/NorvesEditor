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
            std::lock_guard<std::mutex> lock(mutex_);
            if (level == LogSeverity::Warn)
            {
                ++warn_count_;
                last_warn_ = std::string(message);
            }
        }

        int warn_count() const
        {
            std::lock_guard<std::mutex> lock(mutex_);
            return warn_count_;
        }

    private:
        mutable std::mutex mutex_;
        int warn_count_ = 0;
        std::string last_warn_;
    };

    void test_basic_fifo()
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

    void test_capacity_clamped_to_one()
    {
        BoundedFrameQueue q(0);
        NORVES_CHECK_EQ(q.capacity(), static_cast<std::size_t>(1));
        NORVES_CHECK(q.push("x"));
        NORVES_CHECK_EQ(q.size(), static_cast<std::size_t>(1));
    }

    void test_drop_oldest()
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

    void test_drop_newest()
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

    void test_reject()
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

    void test_wait_and_pop_across_threads()
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

    void test_shutdown_wakes_waiter()
    {
        BoundedFrameQueue q(4);
        std::optional<std::string> result;
        bool result_set = false;
        std::mutex result_mutex;

        std::thread consumer(
            [&]
            {
                auto r = q.wait_and_pop();
                std::lock_guard<std::mutex> lock(result_mutex);
                result = r;
                result_set = true;
            });

        std::this_thread::sleep_for(std::chrono::milliseconds(20));
        q.shutdown();
        consumer.join();

        std::lock_guard<std::mutex> lock(result_mutex);
        NORVES_CHECK(result_set);
        NORVES_CHECK(!result.has_value());
        NORVES_CHECK(q.closed());
    }

    void test_post_shutdown_drain_then_nullopt()
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
    test_basic_fifo();
    test_capacity_clamped_to_one();
    test_drop_oldest();
    test_drop_newest();
    test_reject();
    test_wait_and_pop_across_threads();
    test_shutdown_wakes_waiter();
    test_post_shutdown_drain_then_nullopt();
    return norves::test::summary();
}
