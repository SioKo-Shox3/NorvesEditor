# NorvesEditor Bridge Capabilities

Status: alpha, phase C2

Capabilities describe **optional** protocol features an engine endpoint supports
beyond the always-required alpha surface. They let the editor and the engine
agree on what is usable on a given connection without versioning the whole
protocol for every feature.

This document is the prose companion to the capability shapes in
[`../schema/common.schema.json`](../schema/common.schema.json) (`capabilityToken`,
`capabilityDescriptor`) and the handshake schemas under
[`../schema/methods/`](../schema/methods/). Where prose and schema disagree, the
schema and its golden fixtures win.

## What is always available

The required alpha method/event surface (see
[`protocol-overview.md`](./protocol-overview.md)) is **not** gated by
capabilities. `bridge.hello`, `bridge.getCapabilities`, `engine.getStatus`,
`engine.launchInfo`, `log.subscribe`, `log.unsubscribe`, and the required events
must work on every compliant endpoint. Capabilities only describe optional or
post-alpha extensions layered on top.

## Capability tokens

A capability is identified by a namespaced token with the same lowerCamel
`namespace.member` shape as method/event names
(`^[a-z][a-zA-Z0-9]*\.[a-zA-Z0-9]+$`), for example:

```text
runtime.control     runtime play/pause/stop control (phase C3 surface)
viewport.focus      best-effort focus/raise of the external engine window
log.stream          push log.message events without polling
```

Tokens are the unit of negotiation. A `capabilityDescriptor` wraps a token with
optional metadata:

| field | type | required | description |
| --- | --- | --- | --- |
| `name` | capability token | yes | the capability identifier. |
| `version` | `MAJOR.MINOR` string | no | capability feature version, independent of protocol version. |
| `description` | non-empty string | no | human-readable explanation. |

The concrete registry of defined tokens grows as later phases add features; this
phase only fixes the **shape** and the negotiation flow, not a closed token list.

## Negotiation flow

1. The editor sends `bridge.hello` with the protocol versions it supports and,
   optionally, a `capabilities` array of tokens it offers or requests.
2. The engine replies to `bridge.hello` with the selected `protocolVersion` and
   MAY include a `capabilities` array of descriptors it advertises.
3. The authoritative engine capability list is always retrievable via
   `bridge.getCapabilities`, whose `result.capabilities` is an array of
   descriptors. A client that did not read them from the hello result, or that
   wants to refresh them, calls `bridge.getCapabilities`.
4. The **effective capability set** for a connection is the intersection of what
   the client uses and what the engine advertises. A client must not assume an
   optional feature is present unless its token appears in the engine's
   advertised set.

Tokens the client offers in `bridge.hello` that the engine does not advertise are
simply not in the effective set; this is not an error. Invoking a method that
depends on an unadvertised capability is answered with an error response (the
error model is formalized in [`error-model.md`](./error-model.md)).

## Versioning relationship

Protocol version (`version` envelope field, `MAJOR.MINOR`) governs the envelope
and the required surface. Capability `version` governs an individual optional
feature and moves independently. Protocol compatibility and version negotiation
rules in full — including the canonical capability backward-compatibility rules —
are specified in [`error-model.md`](./error-model.md); this document only
establishes that capabilities are negotiated per connection during the handshake.

## Fixtures

Handshake fixtures that exercise capabilities live under
[`../fixtures/methods/bridge.hello/`](../fixtures/methods/bridge.hello/) and
[`../fixtures/methods/bridge.getCapabilities/`](../fixtures/methods/bridge.getCapabilities/).
See [`message-payloads.md`](./message-payloads.md) for the per-message payload
reference and [`../fixtures/README.md`](../fixtures/README.md) for the validation
procedure.
