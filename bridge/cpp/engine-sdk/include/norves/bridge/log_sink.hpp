#ifndef NORVES_BRIDGE_LOG_SINK_HPP
#define NORVES_BRIDGE_LOG_SINK_HPP

#include <string_view>

// Logging sink for the engine SDK.
//
// Depends on <std> only; no third-party headers are included here. SDK logging
// never writes to a standard stream directly (cpp.md forbids standard-stream
// logging outside tests/examples): every diagnostic the SDK emits is routed
// through an ILogSink the embedder supplies. When no sink is supplied the SDK is
// silent.
namespace norves::bridge {

// Log severity. Mirrors common.schema.json#/$defs/logLevel
// (trace/debug/info/warn/error) so SDK diagnostics map cleanly onto the wire
// logLevel vocabulary.
enum class LogSeverity { Trace, Debug, Info, Warn, Error };

// Pure-virtual logging sink. The embedder implements this to route SDK
// diagnostics wherever it wants (its own logger, a file, a UI channel).
//
// Lifetime / thread affinity: the SDK calls log() synchronously on whatever
// thread drives the SDK (e.g. the thread that invoked BridgeEngineServer::
// handleFrame). The implementation must be safe to call on that thread; if it
// touches shared state it owns the synchronization. `message` is a borrowed
// view valid only for the duration of the call and must not be retained.
class ILogSink {
  public:
    virtual ~ILogSink() = default;

    // Emits a single log line at the given severity. `message` is borrowed for
    // the call only.
    virtual void log(LogSeverity level, std::string_view message) = 0;

  protected:
    ILogSink() = default;
    ILogSink(const ILogSink&) = default;
    ILogSink(ILogSink&&) = default;
    ILogSink& operator=(const ILogSink&) = default;
    ILogSink& operator=(ILogSink&&) = default;
};

}  // namespace norves::bridge

#endif  // NORVES_BRIDGE_LOG_SINK_HPP
