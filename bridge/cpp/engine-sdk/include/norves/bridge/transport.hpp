#ifndef NORVES_BRIDGE_TRANSPORT_HPP
#define NORVES_BRIDGE_TRANSPORT_HPP

#include <cstddef>
#include <memory>
#include <optional>
#include <string>
#include <utility>

// Frame-oriented, blocking transport seam for the engine SDK.
//
// Depends on <std> only; no third-party headers are exposed here. This is the
// C++ analogue of the Rust editor-client's `Transport` trait
// (bridge/crates/norves-bridge-editor-client/src/transport.rs): a thin pipe
// that carries exactly one wire envelope per frame as its raw JSON text. It is
// deliberately ignorant of the Bridge protocol -- it never decodes an Envelope
// and does no schema work; all encode/decode lives above it (codec / server).
//
// The blocking shape (vs. the Rust async trait) matches the SDK's synchronous
// world: a consumer drives recv() on its own thread and blocks until a frame
// arrives or the peer closes. The WebSocket transport is a later phase and is
// NOT modelled here; only the in-process loopback pair (below) exists today.
namespace norves::bridge {

// A bidirectional, frame-oriented transport. One frame == one wire envelope,
// carried as its JSON text.
//
// Thread model: send() and recv() may be driven from different threads (the
// typical pattern is one thread per direction). An implementation must make the
// two directions independent so a blocked recv() never blocks the peer's send().
// A single endpoint object is NOT required to be safe for concurrent calls to
// the SAME method from multiple threads.
class ITransport {
  public:
    virtual ~ITransport() = default;

    // Sends one frame (a complete wire envelope as JSON text), moving ownership
    // in. Returns true once the frame is handed off (queued for the peer).
    // Returns false, WITHOUT sending the frame, when the transport is closed
    // (the peer endpoint is gone) OR the send buffer is full (its capacity bound
    // is reached). A false return must never silently drop data: it surfaces the
    // closed/full condition as back-pressure so the caller can retry or fail.
    virtual bool send(std::string frame) = 0;

    // Receives the next frame, BLOCKING until one is available or the transport
    // is closed. Returns nullopt only when the transport is closed AND no more
    // frames remain to drain (the analogue of the Rust `Ok(None)` clean EOF).
    virtual std::optional<std::string> recv() = 0;

    // Closes this endpoint. After close(): this endpoint's send() is a no-op
    // returning false, and the PEER's recv() -- once it has drained any frames
    // already in flight -- observes nullopt so its read loop can terminate.
    // close() does not discard frames the peer has not yet received; it only
    // signals end-of-stream. Idempotent.
    virtual void close() = 0;

  protected:
    ITransport() = default;
    ITransport(const ITransport&) = default;
    ITransport(ITransport&&) = default;
    ITransport& operator=(const ITransport&) = default;
    ITransport& operator=(ITransport&&) = default;
};

// Creates two connected in-process loopback endpoints whose send/recv are
// cross-wired: a frame sent on one arrives on the other's recv(). The analogue
// of the Rust `loopback_pair`. `capacity` bounds the buffered frames per
// direction (clamped to a minimum of 1 by the underlying queue). The two
// endpoints are independent objects; either may be driven from its own thread.
//
// Close semantics: calling close() on (or destroying) one endpoint causes the
// PEER's recv() to return nullopt after draining. The implementation owns the
// shared per-direction queues; the returned endpoints are the only handles.
[[nodiscard]] std::pair<std::unique_ptr<ITransport>, std::unique_ptr<ITransport>>
make_loopback_pair(std::size_t capacity);

}  // namespace norves::bridge

#endif  // NORVES_BRIDGE_TRANSPORT_HPP
