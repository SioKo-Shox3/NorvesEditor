#include "norves/bridge/bounded_queue.hpp"

#include <algorithm>
#include <utility>

namespace norves::bridge
{

    BoundedFrameQueue::BoundedFrameQueue(std::size_t capacity, OverflowPolicy policy,
                                         ILogSink* sink)
        // 容量 0 の有界バッファは何も保持できない。キューが常に少なくとも 1 フレーム分の
        // 余地を持ち、オーバーフロー経路が意味を持ち続けるよう、1 にクランプする。
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
                // ここでは診断しない。shutdown 後の push は、オーバーフローではなく
                // シャットダウン経路上で想定された無害な no-op である。
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
                        // キューを変更せずに保つ。到着したフレームはドロップされる。
                        break;
                }
            }
            else
            {
                m_Frames.push_back(std::move(frame));
                bAppended = true;
            }
        }

        // ロックの外で通知する。フレームが追加された（空きがあった）ので、待機者を
        // 起こす。DropOldest 経路と同様であり、起こされたスレッドは述語を再チェックする
        // ため、mutex を解放した後にこれを行うのは安全であり、condition variable の
        // ウェイクアップを跨いでロックを保持することを避けられる。
        if (bAppended)
        {
            m_NotEmpty.notify_one();
            return true;
        }

        // シンクコールバックを跨いで mutex を保持しないよう、ロックの外でログする。
        if (bDroppedOldest)
        {
            warn("BoundedFrameQueue full: dropped oldest frame (DropOldest)");
            // 新しいフレームが古いものを置き換えた。やはり待機者を起こす。
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

        // shutdown 後でも残りのフレームをドレインする。キューがクローズされ、かつ空に
        // なって初めて nullopt を返す。
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
