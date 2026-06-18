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
        : m_Capacity((std::max)(capacity, static_cast<std::size_t>(1))),
          m_Policy(policy),
          m_Sink(sink)
    {
    }

    void BoundedFrameQueue::warn(std::string_view message)
    {
        if (m_Sink != nullptr)
        {
            m_Sink->log(LogSeverity::Warn, message);
        }
    }

    bool BoundedFrameQueue::push(OwnedFrame frame)
    {
        bool bDroppedOldest = false;
        bool bAppended = false;
        {
            std::lock_guard<std::mutex> lock(m_Mutex);
            if (m_bClosed)
            {
                // No diagnostics here: a push after shutdown is an expected,
                // benign no-op on the shutdown path, not an overflow.
                return false;
            }

            if (m_Frames.size() >= m_Capacity)
            {
                switch (m_Policy)
                {
                    case OverflowPolicy::DropOldest:
                        m_Frames.pop_front();
                        m_Frames.push_back(std::move(frame));
                        bDroppedOldest = true;
                        break;
                    case OverflowPolicy::DropNewest:
                    case OverflowPolicy::Reject:
                        // Keep the queue unchanged; the incoming frame is dropped.
                        break;
                }
            }
            else
            {
                m_Frames.push_back(std::move(frame));
                bAppended = true;
            }
        }

        // Notify outside the lock: a frame was added (room was available), so wake a
        // waiter. Matches the DropOldest path; the woken thread re-checks the
        // predicate, so doing this after releasing the mutex is safe and avoids
        // holding the lock across the condition-variable wakeup.
        if (bAppended)
        {
            m_NotEmpty.notify_one();
            return true;
        }

        // Log outside the lock to avoid holding the mutex across a sink callback.
        if (bDroppedOldest)
        {
            warn("BoundedFrameQueue full: dropped oldest frame (DropOldest)");
            // The new frame replaced an old one; still wake a waiter.
            m_NotEmpty.notify_one();
            return true;
        }

        if (m_Policy == OverflowPolicy::DropNewest)
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
        std::lock_guard<std::mutex> lock(m_Mutex);
        if (m_Frames.empty())
        {
            return std::nullopt;
        }
        OwnedFrame frame = std::move(m_Frames.front());
        m_Frames.pop_front();
        return frame;
    }

    std::optional<OwnedFrame> BoundedFrameQueue::wait_and_pop()
    {
        std::unique_lock<std::mutex> lock(m_Mutex);
        m_NotEmpty.wait(lock, [this] { return !m_Frames.empty() || m_bClosed; });

        // Drain remaining frames even after shutdown; only return nullopt once the
        // queue is both closed and empty.
        if (m_Frames.empty())
        {
            return std::nullopt;
        }
        OwnedFrame frame = std::move(m_Frames.front());
        m_Frames.pop_front();
        return frame;
    }

    void BoundedFrameQueue::shutdown()
    {
        {
            std::lock_guard<std::mutex> lock(m_Mutex);
            if (m_bClosed)
            {
                return;
            }
            m_bClosed = true;
        }
        m_NotEmpty.notify_all();
    }

    std::size_t BoundedFrameQueue::size() const
    {
        std::lock_guard<std::mutex> lock(m_Mutex);
        return m_Frames.size();
    }

    std::size_t BoundedFrameQueue::capacity() const
    {
        std::lock_guard<std::mutex> lock(m_Mutex);
        return m_Capacity;
    }

    bool BoundedFrameQueue::closed() const
    {
        std::lock_guard<std::mutex> lock(m_Mutex);
        return m_bClosed;
    }

}  // namespace norves::bridge
