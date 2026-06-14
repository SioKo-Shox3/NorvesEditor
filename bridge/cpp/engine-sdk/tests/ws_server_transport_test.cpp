// WebSocket server transport test (Workstream G / G3).
//
// Exercises norves::bridge::make_websocket_server_transport (the public,
// lws-free ITransport seam) by standing up an in-process libwebsockets CLIENT in
// this TEST TU and driving a full round trip against it. The client uses lws
// directly here; that is fine because the BOUNDARY rule only forbids lws in the
// SDK's public include/ headers, not in tests. The SDK header used below
// (ws_server_transport.hpp) exposes no lws type.
//
// Coverage:
//   1. round trip: client->server recv(), server->client send()
//   2. multiple frames sent back-to-back arrive in order (B1)
//   3. one large frame survives partial-write re-arming (full length match)
//   4. single-connection posture: a 2nd connection is rejected, 1st survives
//   5. close() contract: recv() drains to nullopt, send() returns false,
//      close() is idempotent (called twice)
//   6. bind failure: a 2nd transport on the same port returns nullptr (Warn)
//   7. bind is 127.0.0.1 (loopback): connecting via 127.0.0.1 works

#include "norves/bridge/ws_server_transport.hpp"

#include <libwebsockets.h>

#include <atomic>
#include <chrono>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "norves/bridge/log_sink.hpp"
#include "test_support.hpp"

namespace {

using namespace std::chrono_literals;

// Collects log lines so tests can assert on Warn/Error diagnostics.
class CapturingSink : public norves::bridge::ILogSink {
  public:
    void log(norves::bridge::LogSeverity level, std::string_view message) override {
        std::lock_guard<std::mutex> lk(mutex_);
        lines_.emplace_back(level, std::string(message));
    }
    bool saw(norves::bridge::LogSeverity level) {
        std::lock_guard<std::mutex> lk(mutex_);
        for (const auto& [lvl, msg] : lines_) {
            if (lvl == level) {
                return true;
            }
        }
        return false;
    }

  private:
    std::mutex mutex_;
    std::vector<std::pair<norves::bridge::LogSeverity, std::string>> lines_;
};

// -- libwebsockets test client ----------------------------------------------
//
// A tiny single-connection client running its own context + service thread. It
// reassembles continuation fragments into whole messages (mirrors the SDK
// server) and can send frames to the server on demand from the service thread.

struct TestClient {
    struct PerSession {
        std::string acc;          // fragment reassembly
        TestClient* owner = nullptr;
    };

    std::atomic<bool> connected{false};
    std::atomic<bool> stop{false};
    std::atomic<bool> connection_error{false};

    std::mutex rx_mutex;
    std::vector<std::string> received;  // whole messages, in order

    std::mutex tx_mutex;
    std::vector<std::string> to_send;   // queued outbound payloads

    struct lws_context* ctx = nullptr;
    struct lws* wsi = nullptr;
    std::thread thread;
    PerSession session;

    static int cb(struct lws* wsi, enum lws_callback_reasons reason,
                  void* user, void* in, size_t len) {
        auto* ps = static_cast<PerSession*>(user);
        switch (reason) {
            case LWS_CALLBACK_CLIENT_ESTABLISHED:
                ps->owner->connected.store(true);
                break;
            case LWS_CALLBACK_CLIENT_RECEIVE: {
                ps->acc.append(static_cast<char*>(in), len);
                if (lws_is_final_fragment(wsi) &&
                    lws_remaining_packet_payload(wsi) == 0) {
                    std::lock_guard<std::mutex> lk(ps->owner->rx_mutex);
                    ps->owner->received.push_back(ps->acc);
                    ps->acc.clear();
                }
                break;
            }
            case LWS_CALLBACK_CLIENT_WRITEABLE: {
                std::string payload;
                {
                    std::lock_guard<std::mutex> lk(ps->owner->tx_mutex);
                    if (ps->owner->to_send.empty()) {
                        break;
                    }
                    payload = std::move(ps->owner->to_send.front());
                    ps->owner->to_send.erase(ps->owner->to_send.begin());
                }
                std::vector<unsigned char> buf(LWS_PRE + payload.size());
                std::memcpy(buf.data() + LWS_PRE, payload.data(), payload.size());
                lws_write(wsi, buf.data() + LWS_PRE, payload.size(),
                          LWS_WRITE_TEXT);
                {
                    std::lock_guard<std::mutex> lk(ps->owner->tx_mutex);
                    if (!ps->owner->to_send.empty()) {
                        lws_callback_on_writable(wsi);
                    }
                }
                break;
            }
            case LWS_CALLBACK_CLIENT_CONNECTION_ERROR:
                ps->owner->connection_error.store(true);
                break;
            default:
                break;
        }
        return 0;
    }

    void start(std::uint16_t port) {
        const struct lws_protocols protocols[] = {
            {"norves-bridge", &TestClient::cb, sizeof(PerSession), 0, 0, nullptr, 0},
            LWS_PROTOCOL_LIST_TERM,
        };
        lws_set_log_level(LLL_ERR | LLL_WARN, nullptr);

        struct lws_context_creation_info info;
        std::memset(&info, 0, sizeof(info));
        info.port = CONTEXT_PORT_NO_LISTEN;
        info.protocols = protocols;
        info.gid = -1;
        info.uid = -1;
        ctx = lws_create_context(&info);

        session.owner = this;

        struct lws_client_connect_info ci;
        std::memset(&ci, 0, sizeof(ci));
        ci.context = ctx;
        ci.address = "127.0.0.1";
        ci.port = port;
        ci.path = "/";
        ci.host = "127.0.0.1";
        ci.origin = "127.0.0.1";
        ci.protocol = "norves-bridge";
        ci.userdata = &session;
        wsi = lws_client_connect_via_info(&ci);

        thread = std::thread([this] {
            while (!stop.load()) {
                lws_service(ctx, 50);
                // Honour pending send requests ON the service thread: this is
                // the only thread allowed to call lws_callback_on_writable.
                if (arm_writable.exchange(false) && wsi != nullptr) {
                    bool has_data;
                    {
                        std::lock_guard<std::mutex> lk(tx_mutex);
                        has_data = !to_send.empty();
                    }
                    if (has_data) {
                        lws_callback_on_writable(wsi);
                    }
                }
            }
            lws_context_destroy(ctx);
            ctx = nullptr;
        });
    }

    void enqueue(const std::string& payload) {
        {
            std::lock_guard<std::mutex> lk(tx_mutex);
            to_send.push_back(payload);
        }
        // Off-thread: only set a flag + wake the loop. The loop (service thread)
        // turns the flag into lws_callback_on_writable.
        arm_writable.store(true);
        if (ctx != nullptr) {
            lws_cancel_service(ctx);
        }
    }

    std::atomic<bool> arm_writable{false};

    size_t received_count() {
        std::lock_guard<std::mutex> lk(rx_mutex);
        return received.size();
    }

    std::vector<std::string> snapshot() {
        std::lock_guard<std::mutex> lk(rx_mutex);
        return received;
    }

    void shutdown() {
        stop.store(true);
        if (ctx != nullptr) {
            lws_cancel_service(ctx);
        }
        if (thread.joinable()) {
            thread.join();
        }
    }
};

template <typename Pred>
bool wait_until(Pred pred, std::chrono::milliseconds timeout) {
    auto deadline = std::chrono::steady_clock::now() + timeout;
    while (std::chrono::steady_clock::now() < deadline) {
        if (pred()) {
            return true;
        }
        std::this_thread::sleep_for(5ms);
    }
    return pred();
}

}  // namespace

int main() {
    using norves::bridge::LogSeverity;
    using norves::bridge::make_websocket_server_transport;

    const std::uint16_t kPort = 39071;
    constexpr std::size_t kSendCap = 256;
    constexpr std::size_t kRecvCap = 256;

    // ---- setup: server transport + connected client ----------------------
    CapturingSink sink;
    auto server = make_websocket_server_transport(kPort, kSendCap, kRecvCap, &sink);
    NORVES_CHECK(server != nullptr);
    if (server == nullptr) {
        return norves::test::summary();
    }

    TestClient client;
    client.start(kPort);

    bool connected = wait_until([&] { return client.connected.load(); }, 5s);
    NORVES_CHECK(connected);

    // Test 1 + 7: client -> server round trip (proves 127.0.0.1 listen works).
    client.enqueue("hello-from-client");
    auto got = server->recv();
    NORVES_CHECK(got.has_value());
    if (got.has_value()) {
        NORVES_CHECK_EQ(*got, std::string("hello-from-client"));
    }

    // Test 1: server -> client
    NORVES_CHECK(server->send("hello-from-server"));
    bool got_one = wait_until([&] { return client.received_count() >= 1; }, 5s);
    NORVES_CHECK(got_one);
    if (got_one) {
        NORVES_CHECK_EQ(client.snapshot().at(0), std::string("hello-from-server"));
    }

    // Test 2: multiple frames in order (B1)
    const int kBurst = 8;
    for (int i = 0; i < kBurst; ++i) {
        NORVES_CHECK(server->send("burst-" + std::to_string(i)));
    }
    bool got_burst =
        wait_until([&] { return client.received_count() >= 1 + kBurst; }, 5s);
    NORVES_CHECK(got_burst);
    if (got_burst) {
        auto frames = client.snapshot();
        for (int i = 0; i < kBurst; ++i) {
            NORVES_CHECK_EQ(frames.at(1 + i), "burst-" + std::to_string(i));
        }
    }

    // Test 3: large frame survives partial-write re-arming (full length match)
    const std::size_t kBig = 50000;
    std::string big = "BIG:";
    big.append(kBig, 'X');
    NORVES_CHECK(server->send(big));
    const size_t expect_after_big = 1 + kBurst + 1;
    bool got_big =
        wait_until([&] { return client.received_count() >= expect_after_big; }, 10s);
    NORVES_CHECK(got_big);
    if (got_big) {
        auto frames = client.snapshot();
        const std::string& last = frames.at(expect_after_big - 1);
        NORVES_CHECK_EQ(last.size(), big.size());
        NORVES_CHECK(last == big);
    }

    // Test 4: single-connection posture -- a 2nd client is rejected, 1st stays.
    {
        TestClient client2;
        client2.start(kPort);
        // The server rejects at ESTABLISHED (-1), so client2 either errors or
        // closes without establishing a usable session. Either way the FIRST
        // client must keep working afterwards.
        wait_until([&] { return client2.connection_error.load(); }, 3s);
        client2.shutdown();
    }
    // First client still works:
    NORVES_CHECK(server->send("after-reject"));
    bool still_ok =
        wait_until([&] { return client.received_count() >= expect_after_big + 1; }, 5s);
    NORVES_CHECK(still_ok);
    if (still_ok) {
        auto frames = client.snapshot();
        NORVES_CHECK_EQ(frames.back(), std::string("after-reject"));
    }

    client.shutdown();

    // Test 5: close() contract -- recv() drains to nullopt, send() false,
    // idempotent.
    server->close();
    auto after_close = server->recv();
    NORVES_CHECK(!after_close.has_value());          // drained to nullopt
    NORVES_CHECK(server->send("dropped") == false);  // closed => false
    server->close();                                 // idempotent: must not hang/crash

    // Test 6: bind failure -- a 2nd transport on the same port returns nullptr.
    {
        // Re-bind the same port using a fresh server, then try a duplicate.
        CapturingSink sink_a;
        auto a = make_websocket_server_transport(kPort, kSendCap, kRecvCap, &sink_a);
        NORVES_CHECK(a != nullptr);

        CapturingSink sink_dup;
        auto dup =
            make_websocket_server_transport(kPort, kSendCap, kRecvCap, &sink_dup);
        NORVES_CHECK(dup == nullptr);
        NORVES_CHECK(sink_dup.saw(LogSeverity::Warn));

        if (a != nullptr) {
            a->close();
        }
    }

    return norves::test::summary();
}
