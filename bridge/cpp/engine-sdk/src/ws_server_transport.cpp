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

// libwebsockets-backed WebSocket server transport (Workstream G / G3).
//
// This is the ONLY translation unit that includes <libwebsockets.h>. The library
// is linked PRIVATE and every lws type stays inside this file; the public header
// exposes nothing but the std + SDK-own ITransport / ILogSink seam. See ADR 0007
// for the threading discipline this file implements.
//
// Threading model (one service thread owns ALL libwebsockets state):
//   - service thread: lws_create_context -> lws_service loop -> lws_context_
//     destroy. Receives frames (LWS_CALLBACK_RECEIVE), reassembles fragments,
//     pushes them onto m_RecvQueue. Drains m_SendQueue and performs every
//     lws_write inside LWS_CALLBACK_SERVER_WRITEABLE, with partial-write
//     re-arming so a frame is finished before the next one starts (order
//     preserved). Owns the single active wsi; clears it on LWS_CALLBACK_CLOSED.
//   - external send(): push onto m_SendQueue (OverflowPolicy::Reject => false on
//     full) and lws_cancel_service() to wake the loop. NEVER touches wsi/context.
//   - consumer recv(): wait_and_pop() on m_RecvQueue; nullopt after close+drain.
//   - close(): set the close flag + lws_cancel_service(); shut down both queues
//     so a blocked recv() wakes and drains to nullopt and send() returns false.
//     Real wsi/context teardown happens on the service thread. Idempotent: the
//     flag and a once-guarded join make a second close()/destructor safe.
//
// Single-connection posture (alpha): exactly one editor client at a time. A
// second simultaneous connection is REJECTED at LWS_CALLBACK_ESTABLISHED
// (return -1, which closes the new wsi) so the existing client is never
// disrupted. After the active client disconnects the next connection is
// accepted (G5 reconnect relies on this).
namespace norves::bridge
{

    namespace
    {

        // Per-frame outbound state, carrying the partial-write cursor.
        struct OutFrame
        {
            std::vector<unsigned char> buf;  // LWS_PRE padding + payload
            std::size_t payload_len = 0;
            std::size_t sent = 0;  // payload bytes already written
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
                  // Send: back-pressure. A full send queue makes send() return false
                  // rather than evicting frames.
                  m_SendQueue(sendCapacity, OverflowPolicy::Reject, logSink),
                  // Receive: never silently drop. We treat overflow as fatal ourselves
                  // (close the connection); Reject means push() returns false so the
                  // service thread can detect the overflow and act on it.
                  m_RecvQueue(recvCapacity, OverflowPolicy::Reject, logSink)
            {
            }

            ~WebSocketServerTransport() override { close(); }

            WebSocketServerTransport(const WebSocketServerTransport&) = delete;
            WebSocketServerTransport& operator=(const WebSocketServerTransport&) = delete;
            WebSocketServerTransport(WebSocketServerTransport&&) = delete;
            WebSocketServerTransport& operator=(WebSocketServerTransport&&) = delete;

            // Creates the lws context (binds 127.0.0.1:port) and starts the service
            // thread. Returns false (no thread started, context destroyed) on bind /
            // creation failure.
            bool start(std::uint16_t port)
            {
                lws_set_log_level(LLL_ERR | LLL_WARN, nullptr);

                struct lws_context_creation_info info;
                std::memset(&info, 0, sizeof(info));
                info.port = static_cast<int>(port);
                info.iface = "127.0.0.1";  // loopback only; never 0.0.0.0
                info.protocols = m_Protocols;
                info.gid = -1;
                info.uid = -1;
                info.user = this;  // reachable from the static callback via lws_context_user

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
                // Push onto the send queue (Reject => false on full = back-pressure) and
                // wake the service thread. We do NOT touch wsi/context here.
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
                // Blocks until a frame arrives or m_RecvQueue is shut down (close()),
                // after which it drains remaining frames and yields nullopt.
                return m_RecvQueue.wait_and_pop();
            }

            void close() override
            {
                // Idempotent: only the first caller flips the flag, shuts the queues,
                // wakes the service loop and joins it.
                bool expected = false;
                if (!m_bClosed.compare_exchange_strong(expected, true, std::memory_order_acq_rel))
                {
                    return;
                }

                // Wake a blocked recv() so it drains and returns nullopt, and make
                // subsequent send() (after the flag) return false.
                m_RecvQueue.shutdown();
                m_SendQueue.shutdown();

                // Ask the service thread to leave its loop. lws_cancel_service is the
                // only cross-thread-safe lws call; the real teardown is on that thread.
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
            // -- service thread only below this line (except where noted) -------------

            void service_loop()
            {
                while (!m_bClosed.load(std::memory_order_acquire))
                {
                    // Returns on the timeout, on incoming traffic, or when woken by
                    // lws_cancel_service (send() / close()). All wsi/write work happens
                    // in callbacks dispatched from here.
                    lws_service(m_Context, 50);
                    pump_writable();
                }
                // Drain a final time so a frame enqueued just before close still gets a
                // writable request honoured if the client is still up; harmless if not.
                pump_writable();
                lws_context_destroy(m_Context);
                // Intentionally do NOT null m_Context here. After start() (which writes it
                // before the service thread exists, establishing happens-before) m_Context
                // is never written again, so it is a read-only shared value: the service
                // thread reads it in lws_service(); external close()/send() read it under
                // the m_bClosed guard before join(). Writing nullptr here would race those
                // external reads. lws_context_destroy makes the handle dangling, but no
                // path dereferences it afterwards: close() is idempotent (CAS rejects the
                // second caller before touching m_Context), and send() returns false once
                // m_bClosed is set, so neither lws_cancel_service nor lws_service runs again.
            }

            // If there is an active connection and pending outbound frames, ask lws for
            // a writable callback. Called only from the service thread.
            void pump_writable()
            {
                if (m_ActiveWsi != nullptr && (m_CurrentOut.has_value() || m_SendQueue.size() > 0))
                {
                    lws_callback_on_writable(m_ActiveWsi);
                }
            }

            // LWS_CALLBACK_SERVER_WRITEABLE handler (service thread). Sends at most one
            // chunk; re-arms until the current frame is fully written, then moves on.
            int on_writable(struct lws* wsi)
            {
                if (wsi != m_ActiveWsi)
                {
                    return 0;  // stale wsi; ignore
                }
                if (!m_CurrentOut.has_value())
                {
                    auto next = m_SendQueue.pop();
                    if (!next.has_value())
                    {
                        return 0;  // nothing to send right now
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

                // Continuation chunks reuse the already-sent region of fr.buf as scratch
                // for lws_write's LWS_PRE prefix: those payload bytes were already written
                // to the wire and are never re-read, so overwriting them here is safe.
                unsigned char* start = fr.buf.data() + LWS_PRE + fr.sent;
                int n = lws_write(wsi, start, attempt, static_cast<enum lws_write_protocol>(flags));
                if (n < 0)
                {
                    warn("lws_write failed; closing connection");
                    return -1;  // closes this wsi
                }
                // Partial write: advance only by what was accepted and re-arm to resume
                // the SAME frame from the new offset (byte order preserved).
                fr.sent += static_cast<std::size_t>(n);
                if (fr.sent >= fr.payload_len)
                {
                    m_CurrentOut.reset();  // frame done; next writable picks the next one
                }
                if (m_CurrentOut.has_value() || m_SendQueue.size() > 0)
                {
                    lws_callback_on_writable(wsi);
                }
                return 0;
            }

            // LWS_CALLBACK_RECEIVE handler (service thread). Reassembles continuation
            // fragments into one message, then pushes it onto m_RecvQueue.
            int on_receive(struct lws* wsi, void* in, std::size_t len)
            {
                if (wsi != m_ActiveWsi)
                {
                    return 0;  // not the active connection; ignore
                }
                if (in != nullptr && len > 0)
                {
                    m_RecvAcc.append(static_cast<const char*>(in), len);
                }
                if (lws_is_final_fragment(wsi) && lws_remaining_packet_payload(wsi) == 0)
                {
                    std::string message = std::move(m_RecvAcc);
                    m_RecvAcc.clear();
                    // Receive overflow is FATAL: losing an inbound frame breaks
                    // request/response correlation. push() returns false on a full
                    // Reject queue; we then close the connection and let the upper layer
                    // (G5) reconnect.
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
                        return -1;  // closes this wsi
                    }
                }
                return 0;
            }

            int on_established(struct lws* wsi)
            {
                if (m_ActiveWsi != nullptr)
                {
                    // Single-connection alpha posture: keep the existing client, reject
                    // the newcomer (return -1 closes only the new wsi).
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
                    m_ActiveWsi = nullptr;  // never touch this wsi again
                    m_RecvAcc.clear();
                    m_CurrentOut.reset();  // drop a half-sent frame to the gone client
                }
            }

            // Static trampoline: recovers the instance from the context user pointer and
            // dispatches to the member handlers. Runs on the service thread.
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

            // Cap a single lws_write so large frames exercise the partial-write re-arm
            // path; also keeps per-callback work bounded.
            static constexpr std::size_t ChunkCap = 4096;

            const struct lws_protocols m_Protocols[2] = {
                {"norves-bridge", &WebSocketServerTransport::callback, 0, 0, 0, nullptr, 0},
                LWS_PROTOCOL_LIST_TERM,
            };

            ILogSink* m_LogSink;  // non-owned, may be null

            // Queues are thread-safe; touched from external send()/consumer recv() and
            // the service thread.
            BoundedFrameQueue m_SendQueue;
            BoundedFrameQueue m_RecvQueue;

            std::atomic<bool> m_bClosed{false};

            std::thread m_ServiceThread;

            // Service-thread-only state.
            struct lws_context* m_Context = nullptr;
            struct lws* m_ActiveWsi = nullptr;
            std::string m_RecvAcc;                 // fragment reassembly buffer
            std::optional<OutFrame> m_CurrentOut;  // frame in flight (partial-write)
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
            return nullptr;  // bind / context-creation failed (already logged Warn)
        }
        // Upcast to the lws-free public seam: the caller never sees any lws handle.
        return transport;
    }

}  // namespace norves::bridge
