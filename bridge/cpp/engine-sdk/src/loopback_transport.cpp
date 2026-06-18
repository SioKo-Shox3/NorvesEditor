#include "norves/bridge/bounded_queue.hpp"
#include "norves/bridge/transport.hpp"

#include <cstddef>
#include <memory>
#include <optional>
#include <string>
#include <utility>

// In-process loopback transport. WebSocket is a later phase and not modelled
// here; this is the C++ analogue of the Rust `loopback_pair` used by the F5
// end-to-end round-trip test.
//
// Design: each endpoint owns two shared BoundedFrameQueue handles -- one it
// sends INTO (its outbound direction) and one it receives FROM (its inbound
// direction) -- cross-wired with the peer so A's outbound queue is B's inbound
// queue and vice versa. BoundedFrameQueue is non-copyable / non-movable, so the
// two queues are heap-allocated and shared via std::shared_ptr; the two
// endpoints are the only owners and the queues die with the last endpoint.
//
// Blocking recv() == BoundedFrameQueue::wait_and_pop(). close() shuts down this
// endpoint's OUTBOUND queue, which is the peer's INBOUND queue: the peer's
// wait_and_pop() drains any frames still queued and then returns nullopt,
// terminating its read loop. Each direction is an independent queue, so a
// blocked recv() on one endpoint never blocks the peer's send().
namespace norves::bridge
{

    namespace
    {

        class LoopbackTransport : public ITransport
        {
        public:
            // `outbound` is the queue this endpoint sends into; `inbound` is the queue
            // it receives from. The peer is constructed with the two swapped.
            LoopbackTransport(std::shared_ptr<BoundedFrameQueue> outbound,
                              std::shared_ptr<BoundedFrameQueue> inbound)
                : outbound_(std::move(outbound)), inbound_(std::move(inbound))
            {
            }

            bool send(std::string frame) override
            {
                // A closed outbound queue (we called close(), or the peer is being torn
                // down) makes push() a no-op returning false: the frame is dropped and
                // the caller learns the transport is gone. With OverflowPolicy::Reject a
                // FULL queue also returns false without storing the frame, so a full
                // transport surfaces back-pressure rather than silently losing data --
                // matching the ITransport::send() contract.
                return outbound_->push(std::move(frame));
            }

            std::optional<std::string> recv() override
            {
                // Blocks until a frame arrives or our inbound queue is shut down (peer
                // closed). After shutdown it drains remaining frames, then yields
                // nullopt -- the clean-EOF signal.
                return inbound_->wait_and_pop();
            }

            void close() override
            {
                // Signal end-of-stream to the peer's recv() by closing the queue it
                // reads from (our outbound). We do NOT shut down our own inbound queue:
                // frames the peer already sent us stay drainable, and our own recv()
                // ending is the peer's responsibility (it closes when it is done).
                outbound_->shutdown();
            }

        private:
            std::shared_ptr<BoundedFrameQueue> outbound_;
            std::shared_ptr<BoundedFrameQueue> inbound_;
        };

    }  // namespace

    std::pair<std::unique_ptr<ITransport>, std::unique_ptr<ITransport>> make_loopback_pair(
        std::size_t capacity)
    {
        // One queue per direction. BoundedFrameQueue clamps a capacity of 0 up to 1.
        // OverflowPolicy::Reject so a full queue makes push() (and thus send())
        // return false instead of silently evicting the oldest frame: a full
        // transport must surface back-pressure to the caller, never lose data. This
        // matches the ITransport::send() contract (false on closed OR full).
        auto a_to_b = std::make_shared<BoundedFrameQueue>(capacity, OverflowPolicy::Reject);
        auto b_to_a = std::make_shared<BoundedFrameQueue>(capacity, OverflowPolicy::Reject);

        // Endpoint A sends into a_to_b and receives from b_to_a; endpoint B is the
        // mirror image. Both queues are shared by exactly the two endpoints.
        auto a = std::make_unique<LoopbackTransport>(a_to_b, b_to_a);
        auto b = std::make_unique<LoopbackTransport>(b_to_a, a_to_b);
        return {std::move(a), std::move(b)};
    }

}  // namespace norves::bridge
