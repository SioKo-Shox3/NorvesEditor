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
scene.query         read the engine's scene tree via scene.getTree (Phase 3 surface)
object.query        read an object's properties / the type schema via
                    object.getSnapshot and schema.getSnapshot (Phase 4 surface)
object.edit         write an object's property via object.setProperty (Phase 5 surface)
scene.edit         create/delete/reparent/rename/duplicate scene objects
                    (scene structure editing surface; advertised by engines
                    that implement the edit methods)
scene.liveUpdate    push scene.treeChanged / object.changed events without
                    polling (Phase 6 surface, protocol 0.2)
viewport.thumbnail  return a low-frequency still thumbnail of the external
                    viewport via viewport.getThumbnail (Phase 7b surface,
                    protocol 0.2)
asset.read          read asset resolution health and loaded manifest snapshots
                    via asset.resolve and asset.getManifest (asset browser live
                    overlay surface, protocol 0.2)
```

The `scene.query` token advertises that an engine can answer `scene.getTree`
(read-only scene hierarchy). It is an optional, engine-agnostic feature
advertisement: an engine that does not implement scene query simply omits the
token and answers `scene.getTree` with `METHOD_NOT_SUPPORTED`, which the editor
degrades on gracefully (no scene Outliner content). The token is independent of
protocol version negotiation.

The `object.query` token advertises that an engine can answer
`object.getSnapshot` (a single object's read-only property bag) and
`schema.getSnapshot` (the generic type descriptors). It is an optional,
engine-agnostic advertisement in the same vein: an engine that does not implement
object/schema query omits the token and answers those methods with
`METHOD_NOT_SUPPORTED`, which the editor degrades on gracefully (no Inspector
content). The token is independent of protocol version negotiation.

The `object.edit` token advertises that an engine can answer
`object.setProperty` (write a single property value on an object). It is the
write counterpart to `object.query`, and likewise optional and engine-agnostic:
an engine that does not implement object editing omits the token and answers
`object.setProperty` with `METHOD_NOT_SUPPORTED`, which the editor degrades on
gracefully (a read-only Inspector). The token is independent of protocol version
negotiation.

The `scene.edit` token advertises that an engine can edit scene structure. In
this protocol slice it covers `scene.createObject`, `scene.deleteObject`, and
`scene.reparentObject`; future additive methods such as rename and duplicate are
intended to live behind the same token. It is optional and engine-agnostic: an
engine that does not implement scene structure editing omits the token and
answers these methods with `METHOD_NOT_SUPPORTED`, which the editor degrades on
gracefully. Actual advertisement by NorvesLib is tracked separately.

The `scene.liveUpdate` token advertises that an engine pushes live-update events
(`scene.treeChanged`, `object.changed`) over the wire instead of requiring the
editor to poll. These events are additive and were introduced in protocol
version 0.2. It is an optional, engine-agnostic advertisement: an engine that
does not emit live updates simply omits the token, and the editor falls back to
its connect-time and selection-time fetches (live updates are best-effort, not
the primary guarantee). The token is independent of protocol version negotiation
and also serves as the degradation signal for a 0.1-only engine that cannot send
these 0.2 events.

The `viewport.thumbnail` token advertises that an engine can answer
`viewport.getThumbnail` (a low-frequency still image of the external viewport,
returned inline as a base64 string). It is an optional, engine-agnostic
advertisement introduced in protocol version 0.2: an engine that does not provide
thumbnails omits the token and answers `viewport.getThumbnail` with
`METHOD_NOT_SUPPORTED`, which the editor degrades on gracefully (it falls back to
the external-window notice). The token is independent of protocol version
negotiation. The thumbnail path's large-payload limits (PNG, max 640x360,
256 KiB hard cap, pull-style, <= 1 fps) are specified in
`docs/memory-buffer-policy.md`.

The `asset.read` token advertises that an engine can answer `asset.resolve`
(single logical-path resolution / health metadata) and `asset.getManifest` (a
snapshot of the engine's currently loaded manifest). Both methods are optional
and read-only. An engine that does not implement live asset reads omits the token
and answers those methods with `METHOD_NOT_SUPPORTED`; the editor can continue
using its offline workspace manifest. Returned values are DTO snapshots only and
must not contain references to loaded asset memory, package buffers, or engine
manifest storage.

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
