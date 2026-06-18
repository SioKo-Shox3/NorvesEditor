// Workstream H-A: residential WebSocket mock engine.
//
// A standalone, long-running engine process the editor backend can launch,
// connect to, drive (hello / status / runtime control / log streaming), and
// stop. It is the production-shaped counterpart of the G4 ws_test_server (which
// stays a Rust-only test harness): unlike that harness, this process is
// RESIDENTIAL -- a client disconnect does NOT end it; it keeps listening for the
// next connection until it is explicitly stopped (signal -> close()).
//
// Boundary: this TU includes only SDK public headers. The WebSocket library is
// hidden behind the ITransport pImpl, so libwebsockets / nlohmann never appear
// here and /W4 stays clean.
//
// Lifecycle contract with the editor backend / launcher:
//   * argv: --bridge-port <p> is required. A missing/invalid port is a hard
//     error (non-zero exit) so a misconfiguration fails fast (same contract as
//     ws_test_server::ParsePort).
//   * On a successful bind it prints exactly "READY <port>\n" to stdout and
//     flushes, which the launcher waits for before dialing. stdout is reserved
//     for that single line; all diagnostics go to stderr.
//   * It then runs ONE residential recv loop: every inbound wire frame is fed to
//     BridgeEngineServer::handleFrame, any response is sent back, and after a
//     log.subscribe ack it emits SEVERAL log.message events back-to-back.
//
// Stop / signal safety (REQUIRED reading):
//   * recv() returns nullopt ONLY after close() (a client disconnect does not
//     unblock it; the transport waits for the next connection). So a clean stop
//     means: drive transport->close(), which makes the blocked recv() return
//     nullopt and ends the loop.
//   * A signal/console handler must be async-signal-safe. transport->close()
//     takes a mutex, signals a condvar and joins the service thread -- none of
//     that is legal from a POSIX signal handler. So the handler does the ONLY
//     async-signal-safe thing: g_stop.store(true) on an atomic flag.
//   * A dedicated watcher thread, started in main(), polls g_stop and calls
//     transport->close() once it is set. That unblocks the recv() loop. This is
//     identical on POSIX and Windows (the handler only ever touches the atomic),
//     so there is no OS-specific teardown path -- only the handler REGISTRATION
//     differs (signal() vs SetConsoleCtrlHandler), guarded by #if defined(_WIN32).

#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <thread>

#if defined(_WIN32)
#include <windows.h>
#else
#include <csignal>
#endif

#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/dto/events.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/log_sink.hpp"
#include "norves/bridge/server.hpp"
#include "norves/bridge/transport.hpp"
#include "norves/bridge/ws_server_transport.hpp"

#include "mock_adapter.hpp"

namespace
{

    using norves::bridge::BridgeEngineServer;
    using norves::bridge::ILogSink;
    using norves::bridge::ITransport;
    using norves::bridge::LogSeverity;
    using norves::bridge::make_websocket_server_transport;

    using norves::bridge::dto::LogLevel;
    using norves::bridge::dto::LogMessageEvent;

    using norves::mock::MockAdapter;

    // Number of log.message events emitted back-to-back after a log.subscribe ack,
    // so a client can assert ordered multi-frame delivery (the G4 burst pattern).
    constexpr int LogBurst = 3;

    // Translation-unit stop flag. Set ONLY by the signal/console handler (an atomic
    // store is async-signal-safe) and polled by the watcher thread, which performs
    // the actual close(). Never touched by close() directly inside the handler.
    std::atomic<bool> g_stop{false};

#if defined(_WIN32)
    // Windows console control handler. Runs on a thread the OS injects, but we still
    // confine it to the atomic store so the stop path is identical to POSIX.
    BOOL WINAPI ConsoleHandler(DWORD ctrlType)
    {
        switch (ctrlType)
        {
            case CTRL_C_EVENT:
            case CTRL_BREAK_EVENT:
            case CTRL_CLOSE_EVENT:
            case CTRL_SHUTDOWN_EVENT:
                g_stop.store(true);
                return TRUE;
            default:
                return FALSE;
        }
    }

    void InstallSignalHandlers() { SetConsoleCtrlHandler(ConsoleHandler, TRUE); }
#else
    extern "C" void PosixSignalHandler(int /*signum*/)
    {
        // Async-signal-safe: an atomic store is permitted from a signal handler.
        g_stop.store(true);
    }

    void InstallSignalHandlers()
    {
        std::signal(SIGINT, PosixSignalHandler);
        std::signal(SIGTERM, PosixSignalHandler);
    }
#endif

    // Minimal stderr sink so SDK Warn/Error diagnostics are visible. stdout is
    // reserved for the single READY line, so diagnostics go to stderr only.
    class StderrSink : public ILogSink
    {
    public:
        void log(LogSeverity level, std::string_view message) override
        {
            if (level == LogSeverity::Warn || level == LogSeverity::Error)
            {
                std::cerr << "mock-engine[sink]: " << message << '\n';
            }
        }
    };

    // Reads --bridge-port. Returns the port on success; prints an error to stderr
    // and returns nullopt on a missing/invalid value (same contract as
    // ws_test_server::ParsePort).
    std::optional<std::uint16_t> ParsePort(int argc, char** argv)
    {
        for (int i = 1; i < argc; ++i)
        {
            std::string_view arg = argv[i];
            if (arg == "--bridge-port")
            {
                if (i + 1 >= argc)
                {
                    std::cerr << "mock-engine: --bridge-port requires a value\n";
                    return std::nullopt;
                }
                const std::string value = argv[i + 1];
                try
                {
                    const unsigned long parsed = std::stoul(value);
                    if (parsed == 0 || parsed > 65535)
                    {
                        std::cerr << "mock-engine: port out of range: " << value << '\n';
                        return std::nullopt;
                    }
                    return static_cast<std::uint16_t>(parsed);
                }
                catch (const std::exception&)
                {
                    std::cerr << "mock-engine: invalid port: " << value << '\n';
                    return std::nullopt;
                }
            }
        }
        std::cerr << "mock-engine: missing required --bridge-port <port>\n";
        return std::nullopt;
    }

    // Binds the WebSocket server, retrying a transient bind failure a few times with
    // short sleeps. A kill->same-port-restart can briefly fail to bind while the OS
    // releases the previous listener; retrying absorbs that (same approach as
    // ws_test_server). Returns nullptr only if every attempt fails.
    std::unique_ptr<ITransport> BindWithRetry(std::uint16_t port, std::size_t sendCap,
                                              std::size_t recvCap, ILogSink* sink)
    {
        constexpr int MaxAttempts = 20;
        constexpr auto RetryDelay = std::chrono::milliseconds(100);
        for (int attempt = 0; attempt < MaxAttempts; ++attempt)
        {
            std::unique_ptr<ITransport> transport =
                make_websocket_server_transport(port, sendCap, recvCap, sink);
            if (transport != nullptr)
            {
                return transport;
            }
            std::this_thread::sleep_for(RetryDelay);
        }
        return nullptr;
    }

}  // namespace

int main(int argc, char** argv)
{
    const std::optional<std::uint16_t> port = ParsePort(argc, argv);
    if (!port.has_value())
    {
        return 2;
    }

    constexpr std::size_t SendCap = 256;
    constexpr std::size_t RecvCap = 256;

    StderrSink sink;
    std::unique_ptr<ITransport> transport = BindWithRetry(*port, SendCap, RecvCap, &sink);
    if (transport == nullptr)
    {
        std::cerr << "mock-engine: failed to bind WebSocket server on port " << *port << '\n';
        return 3;
    }

    InstallSignalHandlers();

    // Watcher thread: polls g_stop and drives close() off the signal handler.
    // close() is idempotent, so a later destructor close() is harmless. Captures
    // a raw pointer to the transport, which outlives the joined watcher.
    ITransport* transportPtr = transport.get();
    std::thread watcher(
        [transportPtr]()
        {
            constexpr auto Poll = std::chrono::milliseconds(50);
            while (!g_stop.load())
            {
                std::this_thread::sleep_for(Poll);
            }
            transportPtr->close();  // unblocks the residential recv() loop.
        });

    MockAdapter adapter;
    BridgeEngineServer server(adapter, &sink);

    // Pre-build the log.message event frame once; emitted (LogBurst times) after
    // a log.subscribe ack.
    LogMessageEvent log;
    log.level = LogLevel::Info;
    log.message = "Game started";
    log.category = "Engine";
    const std::string logEventFrame = server.emitEvent("log.message", log.to_json());

    // Signal readiness AFTER a successful bind so the launcher only dials a
    // listening socket. stdout is reserved for this single line; flush so the
    // parent observes it promptly.
    std::cout << "READY " << *port << '\n';
    std::cout.flush();

    // Residential recv loop. recv() returns nullopt only after close() (driven by
    // the watcher on stop); a client disconnect does NOT end the loop. handleFrame
    // and the adapter run on this thread, so the emit_log_burst flag set inside
    // logSubscribe is visible right after handleFrame returns. We send the
    // subscribe ack first, then the burst, so the client sees ack-before-events.
    while (true)
    {
        std::optional<std::string> frame = transport->recv();
        if (!frame.has_value())
        {
            break;  // close() was driven (stop requested): clean exit.
        }

        std::optional<std::string> response = server.handleFrame(*frame);
        if (response.has_value())
        {
            if (!transport->send(std::move(*response)))
            {
                // Peer gone mid-flight. Do NOT exit: the engine is residential,
                // so drop this frame and keep serving the next connection.
                adapter.emit_log_burst.store(false);
                continue;
            }
        }

        if (adapter.emit_log_burst.exchange(false))
        {
            for (int i = 0; i < LogBurst; ++i)
            {
                if (!transport->send(std::string(logEventFrame)))
                {
                    break;  // peer gone mid-flight; stop the burst, keep serving.
                }
            }
        }
    }

    // Orderly teardown: stop and join the watcher, then the transport unique_ptr
    // destructor closes again (idempotent). Set g_stop so the watcher exits even
    // when the loop ended via a path other than a signal.
    g_stop.store(true);
    if (watcher.joinable())
    {
        watcher.join();
    }

    return 0;
}
