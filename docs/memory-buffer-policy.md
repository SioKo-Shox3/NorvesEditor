# Memory And Buffer Policy

The Bridge is an editor connection channel. Small control messages may be copied, but APIs must preserve explicit ownership and lifetime rules so later optimizations remain possible.

## Required Rules

```text
- Engine live memory is never passed directly to transport.
- Engine adapters convert engine state into snapshots, DTOs, or serialized values.
- Public API ownership must be explicit.
- Borrowed views are valid only for the documented callback scope.
- Owned buffers remain valid until send completion, release, or explicit drop.
- Large payloads require size limits and queue limits.
- Attachment or streaming strategy must be documented before adding large payload paths.
- Public APIs do not expose third-party WebSocket buffer types.
```

## Review Questions

Use these questions for protocol, SDK, and runtime reviews:

```text
- Who owns this buffer?
- Does it live after the callback returns?
- Does it cross a thread boundary?
- Can it be queued, and if so what is the maximum queue size?
- What releases it on failure or disconnect?
- Are raw pointer, string_view, or span lifetimes documented?
```

Alpha does not optimize for zero-copy transport. It does optimize for safe, reviewable ownership boundaries.

## Large-payload strategy: viewport thumbnails

The `viewport.getThumbnail` method (protocol 0.2) returns a still image of the
engine's external viewport as a base64-encoded string inside the method result.
This is the first large-payload path in the Bridge, so its attachment strategy is
documented here **before** the path exists, as the policy above requires
("Attachment or streaming strategy must be documented before adding large payload
paths.").

```text
- Transport mode:  pull (request/response), never push.
- Image format:    PNG (lossless; mimeType allows a future JPEG switch).
- Max resolution:  640 x 360 (16:9 thumbnail). Larger frames are downscaled by
                   the engine before encoding.
- Hard byte cap:   256 KiB for the raw image bytes (~342 KiB once base64-encoded).
- Max frequency:   <= 1 fps (the UI polls no faster than once per 1000 ms, and
                   only while the panel is visible). No continuous streaming.
- Attachment:      the image is carried inline as a base64 string field
                   (imageBase64) in the JSON result. No out-of-band channel.
- Ownership:       the engine adapter builds the base64 string by value from a
                   snapshot of its framebuffer; it never hands a live engine
                   pointer, span, or framebuffer view to the transport.
```

### Why pull, not push

A push event (`viewport.frame`) would flow through the editor's event broadcast
ring, which is a bounded queue. A high-frequency frame stream would overrun that
ring and force the relay to drop frames (`Lagged`). A pull-style method response
is correlated 1:1 with a request and does **not** travel through the broadcast
ring, so it cannot starve unrelated events. The UI therefore drives cadence
explicitly and never asks faster than 1 fps.

### Why 256 KiB is the cap (and what it is NOT compared against)

The cap is justified against the **WebSocket frame / JSON envelope practical
limit**: a thumbnail result is a single JSON message carried in one WebSocket
text frame, and 256 KiB of image bytes (~342 KiB base64, plus a few hundred bytes
of envelope) stays comfortably within a single frame the transport handles
without fragmentation concerns. 256 KiB is deliberately generous for a
640 x 360 PNG while remaining a hard safety ceiling: an engine that cannot meet
it must downscale further or return an error rather than emit an oversized frame.

This cap is **not** related to `EVENT_BROADCAST_CAPACITY` (the broadcast ring's
**stage count** — how many events may be buffered — not a byte budget). The pull
response does not pass through that ring at all; conflating the two is a category
error. The byte cap governs a single response payload; the broadcast capacity
governs how many small event messages may queue.

### Frequency and continuous streaming

Continuous frame streaming, shared GPU textures, and native window embedding are
explicitly out of scope (see `docs/viewport-strategy.md`, "Post-Alpha Research").
The thumbnail path is a low-frequency still image only.
