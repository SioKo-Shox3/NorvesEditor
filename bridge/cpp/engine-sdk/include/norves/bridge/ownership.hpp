#ifndef NORVES_BRIDGE_OWNERSHIP_HPP
#define NORVES_BRIDGE_OWNERSHIP_HPP

#include <string>

// Buffer ownership policy for the engine SDK.
//
// Depends on <std> only; no third-party headers are included here. This header
// encodes, as a type alias plus normative documentation, the memory/buffer
// policy that the rest of the SDK obeys (see docs/memory-buffer-policy.md and
// CLAUDE.md "Engine live memory is never sent over the transport").
namespace norves::bridge {

// OwnedFrame is a self-owned wire payload: a complete control-channel JSON
// document encoded as UTF-8 text. The SDK only ever moves OwnedFrame values
// across thread / queue / transport boundaries.
//
// Ownership rules (normative; the SDK is written to uphold them and embedders
// must follow them too):
//
//   1. Owned, never borrowed. Anything handed to a queue or to the transport is
//      an OwnedFrame (a value-owning std::string) or a value-owning JsonValue
//      snapshot. The SDK never enqueues or transmits a borrowed view
//      (std::string_view / span / pointer) into engine live memory.
//
//   2. Ownership transfers on enqueue. push()-ing a frame into a queue moves the
//      caller's storage into the queue; after the call the caller must treat its
//      moved-from value as empty. pop()/wait_and_pop() move ownership back out
//      to the consumer.
//
//   3. The queue owns what it holds, and frees it. On overflow (drop), on
//      shutdown, or on destruction the queue releases the frames it still holds.
//      No frame outlives the queue except the ones the consumer popped out.
//
//   4. Snapshot before crossing the boundary. Engine live memory is converted to
//      an owned snapshot (OwnedFrame / JsonValue) before it reaches a queue or
//      the transport. Live memory is never aliased past the call that produced
//      the snapshot.
//
// Allocator / buffer-pool hook: intentionally absent in this layer. OwnedFrame
// uses std::string's default allocator. A custom allocator or buffer pool is an
// optional future extension (cpp.md keeps the allocator / buffer-pool hook
// optional, with the default allocator used when none is supplied); until one is
// added the SDK operates entirely on the default allocator.
using OwnedFrame = std::string;

}  // namespace norves::bridge

#endif  // NORVES_BRIDGE_OWNERSHIP_HPP
