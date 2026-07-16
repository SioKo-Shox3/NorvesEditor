# NorvesEditor Bridge Protocol Overview

Status: alpha, phases C1–C4 (envelope + handshake/capabilities/status/log payloads + runtime control + scene/object/schema)

Protocol version: **0.2** (MINOR bump from 0.1; backward-compatible). The editor
offers `["0.2", "0.1"]` in `bridge.hello` so it negotiates 0.2 with a 0.2-capable
engine and falls back to 0.1 with a legacy 0.1-only engine. See
[`error-model.md`](./error-model.md#hello-version-negotiation).

This document gives a high-level overview of the NorvesEditor Bridge protocol.
The exact envelope structure is specified in
[`message-envelope.md`](./message-envelope.md) and enforced by
[`bridge/spec/schema/envelope.schema.json`](../schema/envelope.schema.json) with
golden fixtures under [`bridge/spec/fixtures/`](../fixtures/).

## Purpose

The Bridge is the control channel between NorvesEditor and a C++ engine process.
It carries connection handshakes, capability negotiation, engine status, logs,
and runtime commands. It is **not** a data plane: engine live memory, scene
graphs, and large payloads are out of scope for the alpha (see
[`docs/memory-buffer-policy.md`](../../../docs/memory-buffer-policy.md)).

## Transport and codec

- Transport: WebSocket (local, localhost only for alpha).
- Canonical codec: JSON text frames.
- Each WebSocket text frame carries exactly one Bridge envelope (one JSON
  object). Batching multiple envelopes in one frame is not used in the alpha.

See [`docs/adr/0003-websocket-json-bridge-control-channel.md`](../../../docs/adr/0003-websocket-json-bridge-control-channel.md).

## Envelope kinds

Every message is one envelope of one of three kinds. This is JSON-RPC inspired
but **not** strict JSON-RPC 2.0; the NorvesEditor envelope is canonical.

| kind | direction (alpha) | purpose |
| --- | --- | --- |
| `request` | editor → engine | invoke a method, expects one matching response |
| `response` | engine → editor | result or error correlated to a request `id` |
| `event` | engine → editor, or editor-internal | one-way notification, no response |

Correlation is by `id`: a `response` echoes the `id` of the `request` it answers.
Events never carry an `id`.

## Alpha method and event surface

These names are the planned alpha surface. Phase C1 specifies only the envelope.
Per-method and per-event `params`/`result` schemas are added incrementally:
phase C2 covers handshake/capabilities/status/log, phase C3 adds runtime
control, and phase C4 adds the scene/object/schema methods (see
[`message-payloads.md`](./message-payloads.md) and
[`capabilities.md`](./capabilities.md)).

The runtime/viewport surface follows a deliberate namespace split: the focus
*operation* is the runtime method `runtime.focusViewport`, while the resulting
window *state notification* is the `viewport.stateChanged` event. Their capability
tokens mirror this — `viewport.focus` gates the focus operation and
`runtime.control` gates the play/pause/stop methods.

Methods (editor → engine):

```text
bridge.hello
bridge.getCapabilities
engine.getStatus
engine.launchInfo
runtime.play
runtime.pause
runtime.stop
runtime.focusViewport
log.subscribe
log.unsubscribe
scene.getTree
scene.createObject
scene.deleteObject
scene.reparentObject
scene.duplicateObject
object.getSnapshot
object.setProperty
schema.getSnapshot
asset.resolve
asset.getManifest
asset.reloadManifest
```

Events:

```text
bridge.connected
bridge.disconnected
engine.statusChanged
engine.processExited
runtime.stateChanged
viewport.stateChanged
log.message
error.reported
scene.treeChanged
object.changed
```

Scene, object, and schema methods (`scene.getTree`, `scene.createObject`,
`scene.deleteObject`, `scene.reparentObject`, `scene.duplicateObject`,
`object.getSnapshot`, `object.setProperty`, `schema.getSnapshot`) are additive
scene/object/schema surface. They carry
serialized snapshots/DTOs only — never references into engine live memory (see
[`docs/memory-buffer-policy.md`](../../../docs/memory-buffer-policy.md)).

Asset read methods (`asset.resolve`, `asset.getManifest`) are additive optional
methods in protocol **0.2**. They carry resolution health and manifest DTO
snapshots only — never asset bytes, package buffers, or references into loaded
engine memory/storage — and are advertised by the `asset.read` capability token.

`asset.reloadManifest` is an additive optional method in protocol **0.2**. It
asks the engine to synchronously reload the manifest path configured at engine
startup, carries no path in its empty params object, and is advertised by the
`asset.reload` capability token. Its boolean `accepted` result distinguishes a
completed runtime switch from a valid request rejected by runtime preconditions.

The live-update events `scene.treeChanged` and `object.changed` are added in
protocol **0.2** (additive). They are engine-wire events that push scene/object
changes so the editor need not poll; like the C4 methods they carry serialized
snapshots/DTOs only. They are best-effort — the editor's connect-time and
selection-time fetches remain the primary guarantee — and are advertised by the
`scene.liveUpdate` capability token.

## Lifecycle events: engine wire vs editor-backend synthesized

A subtle but important point: not every event the TypeScript UI observes is an
event that arrived over the WebSocket wire. The editor backend owns process
lifecycle and connection state, so it **synthesizes** some lifecycle events
locally. Both kinds use the identical envelope shape; the distinction is about
*who produces the envelope*, not its structure.

| event | producer | notes |
| --- | --- | --- |
| `bridge.connected` | **editor backend (synthesized)** | emitted when the backend establishes the WebSocket session; the engine does not send it. |
| `bridge.disconnected` | **editor backend (synthesized)** | emitted on socket close/drop, including cases where the engine sent nothing. |
| `engine.processExited` | **editor backend (synthesized)** | the backend owns the process handle and reports exit; a dead engine cannot send this. |
| `engine.statusChanged` | **engine (wire)** | sent by the engine over the WebSocket. |
| `runtime.stateChanged` | **engine (wire)** | sent by the engine when play/pause/stop state changes. |
| `viewport.stateChanged` | **engine (wire)** | sent by the engine about its external native viewport. |
| `log.message` | **engine (wire)** | engine log line forwarded to the editor. |
| `error.reported` | **both** | the engine may report engine-side errors over the wire; the editor backend may synthesize transport/process errors locally. |
| `scene.treeChanged` | **engine (wire)** | sent by the engine when its scene tree changes (protocol 0.2, best-effort). |
| `object.changed` | **engine (wire)** | sent by the engine when an object's properties change, e.g. after `object.setProperty` (protocol 0.2, best-effort). |

Why this matters:

- Wire events require a live WebSocket session. Synthesized events can fire when
  there is no session (for example `bridge.disconnected` or
  `engine.processExited` after a crash).
- Conformance and mock-engine tests should only assert *wire* events against the
  engine SDK. Synthesized events are the editor backend's responsibility and are
  validated on the Rust backend side, not by the C++ engine SDK.
- Fixtures label this distinction in their file names
  (`event-engine-*` vs `event-synthesized-*`) so implementers can tell which
  side is expected to produce them. The optional `params.origin` field in the
  synthesized fixtures (`"editor-backend"`) is illustrative; per-event payload
  schemas in later phases will define the authoritative payload.

## Validation procedure

Every fixture must parse as JSON and must match its expected validity:

```powershell
pip install jsonschema
python scripts/validate-bridge-fixtures.py
```

Positive fixtures must validate against the envelope schema; negative fixtures
must fail. From phase C2 the validator also applies a payload layer to
`methods/` and `events/` fixtures; see
[`bridge/spec/fixtures/README.md`](../fixtures/README.md) and
[`message-payloads.md`](./message-payloads.md).
