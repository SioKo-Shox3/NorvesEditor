#include "norves/bridge/ws_server_transport.hpp"

#include <libwebsockets.h>

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

#include "norves/bridge/bounded_queue.hpp"
#include "norves/bridge/log_sink.hpp"
#include "norves/bridge/transport.hpp"

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
//     pushes them onto recv_queue_. Drains send_queue_ and performs every
//     lws_write inside LWS_CALLBACK_SERVER_WRITEABLE, with partial-write
//     re-arming so a frame is finished before the next one starts (order
//     preserved). Owns the single active wsi; clears it on LWS_CALLBACK_CLOSED.
//   - external send(): push onto send_queue_ (OverflowPolicy::Reject => false on
//     full) and lws_cancel_service() to wake the loop. NEVER touches wsi/context.
//   - consumer recv(): wait_and_pop() on recv_queue_; nullopt after close+drain.
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
namespace norves::bridge {

namespace {

// Per-frame outbound state, carrying the partial-write cursor.
struct OutFrame {
    std::vector<unsigned char> buf;  // LWS_PRE padding + payload
    std::size_t payload_len = 0;
    std::size_t sent = 0;  // payload bytes already written
};

OutFrame make_out_frame(const std::string& payload) {
    OutFrame f;
    f.buf.resize(LWS_PRE + payload.size());
    if (!payload.empty()) {
        std::memcpy(f.buf.data() + LWS_PRE, payload.data(), payload.size());
    }
    f.payload_len = payload.size();
    return f;
}

class WebSocketServerTransport : public ITransport {
  public:
    WebSocketServerTransport(std::size_t send_capacity,
                             std::size_t recv_capacity,
                             ILogSink* log_sink)
        : log_sink_(log_sink),
          // Send: back-pressure. A full send queue makes send() return false
          // rather than evicting frames.
          send_queue_(send_capacity, OverflowPolicy::Reject, log_sink),
          // Receive: never silently drop. We treat overflow as fatal ourselves
          // (close the connection); Reject means push() returns false so the
          // service thread can detect the overflow and act on it.
          recv_queue_(recv_capacity, OverflowPolicy::Reject, log_sink) {}

    ~WebSocketServerTransport() override { close(); }

    WebSocketServerTransport(const WebSocketServerTransport&) = delete;
    WebSocketServerTransport& operator=(const WebSocketServerTransport&) = delete;
    WebSocketServerTransport(WebSocketServerTransport&&) = delete;
    WebSocketServerTransport& operator=(WebSocketServerTransport&&) = delete;

    // Creates the lws context (binds 127.0.0.1:port) and starts the service
    // thread. Returns false (no thread started, context destroyed) on bind /
    // creation failure.
    bool start(std::uint16_t port) {
        lws_set_log_level(LLL_ERR | LLL_WARN, nullptr);

        struct lws_context_creation_info info;
        std::memset(&info, 0, sizeof(info));
        info.port = static_cast<int>(port);
        info.iface = "127.0.0.1";  // loopback only; never 0.0.0.0
        info.protocols = protocols_;
        info.gid = -1;
        info.uid = -1;
        info.user = this;  // reachable from the static callback via lws_context_user

        context_ = lws_create_context(&info);
        if (context_ == nullptr) {
            warn("failed to create lws context / bind 127.0.0.1:" +
                 std::to_string(port) + " (port in use?)");
            return false;
        }

        service_thread_ = std::thread([this] { service_loop(); });
        return true;
    }

    bool send(std::string frame) override {
        if (closed_.load(std::memory_order_acquire)) {
            return false;
        }
        // Push onto the send queue (Reject => false on full = back-pressure) and
        // wake the service thread. We do NOT touch wsi/context here.
        if (!send_queue_.push(std::move(frame))) {
            return false;
        }
        if (context_ != nullptr) {
            lws_cancel_service(context_);
        }
        return true;
    }

    std::optional<std::string> recv() override {
        // Blocks until a frame arrives or recv_queue_ is shut down (close()),
        // after which it drains remaining frames and yields nullopt.
        return recv_queue_.wait_and_pop();
    }

    void close() override {
        // Idempotent: only the first caller flips the flag, shuts the queues,
        // wakes the service loop and joins it.
        bool expected = false;
        if (!closed_.compare_exchange_strong(expected, true,
                                             std::memory_order_acq_rel)) {
            return;
        }

        // Wake a blocked recv() so it drains and returns nullopt, and make
        // subsequent send() (after the flag) return false.
        recv_queue_.shutdown();
        send_queue_.shutdown();

        // Ask the service thread to leave its loop. lws_cancel_service is the
        // only cross-thread-safe lws call; the real teardown is on that thread.
        if (context_ != nullptr) {
            lws_cancel_service(context_);
        }
        if (service_thread_.joinable()) {
            service_thread_.join();
        }
    }

  private:
    // -- service thread only below this line (except where noted) -------------

    void service_loop() {
        while (!closed_.load(std::memory_order_acquire)) {
            // Returns on the timeout, on incoming traffic, or when woken by
            // lws_cancel_service (send() / close()). All wsi/write work happens
            // in callbacks dispatched from here.
            lws_service(context_, 50);
            pump_writable();
        }
        // Drain a final time so a frame enqueued just before close still gets a
        // writable request honoured if the client is still up; harmless if not.
        pump_writable();
        lws_context_destroy(context_);
        // Intentionally do NOT null context_ here. After start() (which writes it
        // before the service thread exists, establishing happens-before) context_
        // is never written again, so it is a read-only shared value: the service
        // thread reads it in lws_service(); external close()/send() read it under
        // the closed_ guard before join(). Writing nullptr here would race those
        // external reads. lws_context_destroy makes the handle dangling, but no
        // path dereferences it afterwards: close() is idempotent (CAS rejects the
        // second caller before touching context_), and send() returns false once
        // closed_ is set, so neither lws_cancel_service nor lws_service runs again.
    }

    // If there is an active connection and pending outbound frames, ask lws for
    // a writable callback. Called only from the service thread.
    void pump_writable() {
        if (active_wsi_ != nullptr &&
            (current_out_.has_value() || send_queue_.size() > 0)) {
            lws_callback_on_writable(active_wsi_);
        }
    }

    // LWS_CALLBACK_SERVER_WRITEABLE handler (service thread). Sends at most one
    // chunk; re-arms until the current frame is fully written, then moves on.
    int on_writable(struct lws* wsi) {
        if (wsi != active_wsi_) {
            return 0;  // stale wsi; ignore
        }
        if (!current_out_.has_value()) {
            auto next = send_queue_.pop();
            if (!next.has_value()) {
                return 0;  // nothing to send right now
            }
            current_out_ = make_out_frame(*next);
        }

        OutFrame& fr = *current_out_;
        std::size_t remaining = fr.payload_len - fr.sent;
        std::size_t attempt = remaining < kChunkCap ? remaining : kChunkCap;

        const bool first_chunk = (fr.sent == 0);
        const bool last_chunk = (fr.sent + attempt) >= fr.payload_len;

        int flags = first_chunk ? LWS_WRITE_TEXT : LWS_WRITE_CONTINUATION;
        if (!last_chunk) {
            flags |= LWS_WRITE_NO_FIN;
        }

        // Continuation chunks reuse the already-sent region of fr.buf as scratch
        // for lws_write's LWS_PRE prefix: those payload bytes were already written
        // to the wire and are never re-read, so overwriting them here is safe.
        unsigned char* start = fr.buf.data() + LWS_PRE + fr.sent;
        int n = lws_write(wsi, start, attempt,
                          static_cast<enum lws_write_protocol>(flags));
        if (n < 0) {
            warn("lws_write failed; closing connection");
            return -1;  // closes this wsi
        }
        // Partial write: advance only by what was accepted and re-arm to resume
        // the SAME frame from the new offset (byte order preserved).
        fr.sent += static_cast<std::size_t>(n);
        if (fr.sent >= fr.payload_len) {
            current_out_.reset();  // frame done; next writable picks the next one
        }
        if (current_out_.has_value() || send_queue_.size() > 0) {
            lws_callback_on_writable(wsi);
        }
        return 0;
    }

    // LWS_CALLBACK_RECEIVE handler (service thread). Reassembles continuation
    // fragments into one message, then pushes it onto recv_queue_.
    int on_receive(struct lws* wsi, void* in, std::size_t len) {
        if (wsi != active_wsi_) {
            return 0;  // not the active connection; ignore
        }
        if (in != nullptr && len > 0) {
            recv_acc_.append(static_cast<const char*>(in), len);
        }
        if (lws_is_final_fragment(wsi) &&
            lws_remaining_packet_payload(wsi) == 0) {
            std::string message = std::move(recv_acc_);
            recv_acc_.clear();
            // Receive overflow is FATAL: losing an inbound frame breaks
            // request/response correlation. push() returns false on a full
            // Reject queue; we then close the connection and let the upper layer
            // (G5) reconnect.
            if (!recv_queue_.push(std::move(message))) {
                warn("recv queue full; closing connection (frame loss would "
                     "break correlation)");
                if (log_sink_ != nullptr) {
                    log_sink_->log(LogSeverity::Error,
                                   "ws_server_transport: recv overflow");
                }
                return -1;  // closes this wsi
            }
        }
        return 0;
    }

    int on_established(struct lws* wsi) {
        if (active_wsi_ != nullptr) {
            // Single-connection alpha posture: keep the existing client, reject
            // the newcomer (return -1 closes only the new wsi).
            warn("rejecting second connection (single editor client only)");
            return -1;
        }
        active_wsi_ = wsi;
        recv_acc_.clear();
        return 0;
    }

    void on_closed(struct lws* wsi) {
        if (wsi == active_wsi_) {
            active_wsi_ = nullptr;  // never touch this wsi again
            recv_acc_.clear();
            current_out_.reset();  // drop a half-sent frame to the gone client
        }
    }

    // Static trampoline: recovers the instance from the context user pointer and
    // dispatches to the member handlers. Runs on the service thread.
    static int callback(struct lws* wsi, enum lws_callback_reasons reason,
                        void* /*user*/, void* in, std::size_t len) {
        auto* self = static_cast<WebSocketServerTransport*>(
            lws_context_user(lws_get_context(wsi)));
        if (self == nullptr) {
            return 0;
        }
        switch (reason) {
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

    void warn(const std::string& message) {
        if (log_sink_ != nullptr) {
            log_sink_->log(LogSeverity::Warn,
                           "ws_server_transport: " + message);
        }
    }

    // Cap a single lws_write so large frames exercise the partial-write re-arm
    // path; also keeps per-callback work bounded.
    static constexpr std::size_t kChunkCap = 4096;

    const struct lws_protocols protocols_[2] = {
        {"norves-bridge", &WebSocketServerTransport::callback, 0, 0, 0, nullptr, 0},
        LWS_PROTOCOL_LIST_TERM,
    };

    ILogSink* log_sink_;  // non-owned, may be null

    // Queues are thread-safe; touched from external send()/consumer recv() and
    // the service thread.
    BoundedFrameQueue send_queue_;
    BoundedFrameQueue recv_queue_;

    std::atomic<bool> closed_{false};

    std::thread service_thread_;

    // Service-thread-only state.
    struct lws_context* context_ = nullptr;
    struct lws* active_wsi_ = nullptr;
    std::string recv_acc_;                 // fragment reassembly buffer
    std::optional<OutFrame> current_out_;  // frame in flight (partial-write)
};

}  // namespace

std::unique_ptr<ITransport>
make_websocket_server_transport(std::uint16_t port,
                                std::size_t send_capacity,
                                std::size_t recv_capacity,
                                ILogSink* log_sink) {
    auto transport = std::make_unique<WebSocketServerTransport>(
        send_capacity, recv_capacity, log_sink);
    if (!transport->start(port)) {
        return nullptr;  // bind / context-creation failed (already logged Warn)
    }
    // Upcast to the lws-free public seam: the caller never sees any lws handle.
    return transport;
}

}  // namespace norves::bridge
