#pragma once

#include "norves/bridge/log_sink.hpp"
#include "norves/bridge/ownership.hpp"

#include <condition_variable>
#include <cstddef>
#include <deque>
#include <mutex>
#include <optional>
#include <string>

// Bounded, thread-safe queue of owned wire frames for the engine SDK.
//
// Depends on <std> + the SDK's own ILogSink / OwnedFrame only; no third-party
// headers are exposed here. The vendored JSON library is never reachable from
// this header.
namespace norves::bridge
{

    // What the queue does when push() arrives at a full queue.
    //
    //   DropOldest (default): evict the front (oldest) frame, append the new one.
    //                         push() reports success (true). Mirrors the Rust
    //                         dispatcher's bounded-broadcast "lag => drop oldest"
    //                         posture, where a slow consumer loses the stalest data
    //                         rather than stalling the producer.
    //   DropNewest:           discard the incoming frame, keep the queue as-is.
    //                         push() reports failure (false).
    //   Reject:               discard the incoming frame, keep the queue as-is.
    //                         push() reports failure (false). Same observable effect
    //                         as DropNewest today; kept distinct so callers can
    //                         express intent ("never overwrite" vs "back-pressure")
    //                         and so future variants (e.g. error propagation) can
    //                         diverge without changing call sites.
    //
    // Every drop is reported to the ILogSink (if one was supplied) at Warn.
    enum class OverflowPolicy
    {
        DropOldest,
        DropNewest,
        Reject
    };

    // BoundedFrameQueue is a single bounded FIFO of OwnedFrame, shared by one or
    // more producer threads and one or more consumer threads.
    //
    // Thread-safety: every observable operation locks an internal std::mutex that
    // guards all state; wait_and_pop() blocks on a std::condition_variable. The
    // queue is designed for the producer and consumer to run on *different* threads
    // (e.g. the SDK's outbound event producer vs. the transport writer, or the
    // F5 loopback consumer). All public members are safe to call concurrently.
    //
    // Capacity: fixed at construction. A capacity of 0 is meaningless for a bounded
    // buffer, so it is clamped up to 1 (the queue always holds at least one frame).
    // There is no compile-time default capacity here because the right size depends
    // on the channel; the Rust reference uses 64 for the command channel and 256 for
    // the event broadcast (bridge/crates/.../dispatcher.rs:
    // COMMAND_CHANNEL_CAPACITY / EVENT_BROADCAST_CAPACITY). Those are the suggested
    // orders of magnitude for the analogous SDK channels.
    //
    // Lifetime: non-copyable and non-movable (it owns a mutex and a condition
    // variable). Construct it where it lives and pass it by reference / pointer.
    //
    // Ownership: see ownership.hpp. push() moves the caller's frame in; pop() /
    // wait_and_pop() move a frame out; on overflow, shutdown, or destruction the
    // queue frees the frames it still holds.
    class BoundedFrameQueue
    {
    public:
        // `capacity` is clamped to a minimum of 1. `policy` decides full-queue
        // behaviour. `sink` is a NON-OWNED diagnostic sink; it may be nullptr (then
        // the queue is silent) and, if non-null, must outlive this queue. The sink
        // is invoked from whatever thread calls push().
        explicit BoundedFrameQueue(std::size_t capacity,
                                   OverflowPolicy policy = OverflowPolicy::DropOldest,
                                   ILogSink* sink = nullptr);

        ~BoundedFrameQueue() = default;

        BoundedFrameQueue(const BoundedFrameQueue&) = delete;
        BoundedFrameQueue& operator=(const BoundedFrameQueue&) = delete;
        BoundedFrameQueue(BoundedFrameQueue&&) = delete;
        BoundedFrameQueue& operator=(BoundedFrameQueue&&) = delete;

        // Enqueues `frame`, moving ownership in. Returns whether the frame is now
        // queued: true when stored (including the DropOldest case where an older
        // frame was evicted to make room), false when the frame was dropped
        // (DropNewest / Reject on a full queue) or the queue is shut down. After a
        // shutdown() this is a no-op returning false.
        bool push(OwnedFrame frame);

        // Removes and returns the front frame, or nullopt if the queue is empty.
        // Never blocks. Safe to call after shutdown() (drains remaining frames).
        std::optional<OwnedFrame> pop();

        // Removes and returns the front frame, blocking until one is available or
        // the queue is shut down. Returns nullopt only when the queue is shut down
        // AND empty. After shutdown() it first drains remaining frames, then returns
        // nullopt. Spurious wakeups are handled by the wait predicate.
        std::optional<OwnedFrame> wait_and_pop();

        // Closes the queue: wakes every thread blocked in wait_and_pop() so they can
        // drain remaining frames and then observe nullopt; makes every subsequent
        // push() a no-op returning false. Idempotent.
        void shutdown();

        [[nodiscard]] std::size_t size() const;
        [[nodiscard]] std::size_t capacity() const;
        [[nodiscard]] bool closed() const;

    private:
        void warn(std::string_view message);

        mutable std::mutex m_Mutex;
        std::condition_variable m_NotEmpty;
        std::deque<OwnedFrame> m_Frames;
        std::size_t m_Capacity;
        OverflowPolicy m_Policy;
        ILogSink* m_Sink;  // non-owned, may be null
        bool m_bClosed = false;
    };

}  // namespace norves::bridge
