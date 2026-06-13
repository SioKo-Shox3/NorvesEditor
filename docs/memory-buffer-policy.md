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
