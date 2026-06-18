#include "norves/bridge/bounded_queue.hpp"

#include <algorithm>
#include <utility>

namespace norves::bridge
{

    BoundedFrameQueue::BoundedFrameQueue(std::size_t capacity, OverflowPolicy policy,
                                         ILogSink* sink)
        // A bounded buffer with capacity 0 cannot hold anything; clamp to 1 so the
        // queue always has room for at least one frame and the overflow paths stay
        // meaningful.
        : capacity_((std::max)(capacity, static_cast<std::size_t>(1))), policy_(policy), sink_(sink)
    {
    }

    void BoundedFrameQueue::warn(std::string_view message)
    {
        if (sink_ != nullptr)
        {
            sink_->log(LogSeverity::Warn, message);
        }
    }

    bool BoundedFrameQueue::push(OwnedFrame frame)
    {
        bool dropped_oldest = false;
        bool appended = false;
        {
            std::lock_guard<std::mutex> lock(mutex_);
            if (closed_)
            {
                // No diagnostics here: a push after shutdown is an expected,
                // benign no-op on the shutdown path, not an overflow.
                return false;
            }

            if (frames_.size() >= capacity_)
            {
                switch (policy_)
                {
                    case OverflowPolicy::DropOldest:
                        frames_.pop_front();
                        frames_.push_back(std::move(frame));
                        dropped_oldest = true;
                        break;
                    case OverflowPolicy::DropNewest:
                    case OverflowPolicy::Reject:
                        // Keep the queue unchanged; the incoming frame is dropped.
                        break;
                }
            }
            else
            {
                frames_.push_back(std::move(frame));
                appended = true;
            }
        }

        // Notify outside the lock: a frame was added (room was available), so wake a
        // waiter. Matches the DropOldest path; the woken thread re-checks the
        // predicate, so doing this after releasing the mutex is safe and avoids
        // holding the lock across the condition-variable wakeup.
        if (appended)
        {
            not_empty_.notify_one();
            return true;
        }

        // Log outside the lock to avoid holding the mutex across a sink callback.
        if (dropped_oldest)
        {
            warn("BoundedFrameQueue full: dropped oldest frame (DropOldest)");
            // The new frame replaced an old one; still wake a waiter.
            not_empty_.notify_one();
            return true;
        }

        if (policy_ == OverflowPolicy::DropNewest)
        {
            warn("BoundedFrameQueue full: dropped incoming frame (DropNewest)");
        }
        else
        {
            warn("BoundedFrameQueue full: rejected incoming frame (Reject)");
        }
        return false;
    }

    std::optional<OwnedFrame> BoundedFrameQueue::pop()
    {
        std::lock_guard<std::mutex> lock(mutex_);
        if (frames_.empty())
        {
            return std::nullopt;
        }
        OwnedFrame frame = std::move(frames_.front());
        frames_.pop_front();
        return frame;
    }

    std::optional<OwnedFrame> BoundedFrameQueue::wait_and_pop()
    {
        std::unique_lock<std::mutex> lock(mutex_);
        not_empty_.wait(lock, [this] { return !frames_.empty() || closed_; });

        // Drain remaining frames even after shutdown; only return nullopt once the
        // queue is both closed and empty.
        if (frames_.empty())
        {
            return std::nullopt;
        }
        OwnedFrame frame = std::move(frames_.front());
        frames_.pop_front();
        return frame;
    }

    void BoundedFrameQueue::shutdown()
    {
        {
            std::lock_guard<std::mutex> lock(mutex_);
            if (closed_)
            {
                return;
            }
            closed_ = true;
        }
        not_empty_.notify_all();
    }

    std::size_t BoundedFrameQueue::size() const
    {
        std::lock_guard<std::mutex> lock(mutex_);
        return frames_.size();
    }

    std::size_t BoundedFrameQueue::capacity() const
    {
        std::lock_guard<std::mutex> lock(mutex_);
        return capacity_;
    }

    bool BoundedFrameQueue::closed() const
    {
        std::lock_guard<std::mutex> lock(mutex_);
        return closed_;
    }

}  // namespace norves::bridge
