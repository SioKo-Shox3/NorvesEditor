# NorvesEditor Bridge Message Payloads

Status: alpha, phase C3 (hello / capabilities / status / log + runtime control)

This document is the payload reference for the phase C2 Bridge surface: the
`params`/`result` shapes carried inside the
[envelope](./message-envelope.md). The envelope schema validates only envelope
structure; these payloads are validated by separate per-method/per-event schemas
and composed by the fixture validator (see
[two-layer validation](#two-layer-validation)). Where prose and schema disagree,
the schema and its golden fixtures win.

Phase C2 covered handshake, capabilities, status, launch info, and logging.
Phase C3 adds runtime control (`runtime.play`/`pause`/`stop`/`focusViewport` and
the `runtime.stateChanged`/`viewport.stateChanged` events), which is specified
here. Scene/object/schema methods are alpha-optional. The full error model
(`error-model.md`) is **phase C5**; until then the surface reuses the provisional
error object from the envelope schema.

All payload field names are `camelCase`, matching the frontend-facing DTO
convention in `AGENTS.md` / `CLAUDE.md`.

## Shared definitions

[`../schema/common.schema.json`](../schema/common.schema.json) defines reusable
`$defs` referenced by the payload schemas below:

| `$def` | shape |
| --- | --- |
| `versionString` | `MAJOR.MINOR` string. |
| `capabilityToken` | namespaced `namespace.member` token. |
| `capabilityDescriptor` | `{ name, version?, description? }`. |
| `logLevel` | `trace` \| `debug` \| `info` \| `warn` \| `error`. |
| `engineState` | `initializing` \| `ready` \| `running` \| `error`. |
| `runtimeState` | `edit` \| `playing` \| `paused` \| `stopped` \| `unknown`. |
| `viewportState` | `focused` \| `visible` \| `hidden` \| `minimized` \| `unknown`. |
| `origin` | `engine` \| `editor-backend`. |

## Methods

Methods are editor → engine requests with a correlated response. Each method
below lists its `params` (request) and `result` (success response) schema. Error
responses use the envelope `error` object and are not re-specified per method.

### bridge.hello

Handshake initiated by the editor. Schemas:
[params](../schema/methods/bridge.hello.params.schema.json),
[result](../schema/methods/bridge.hello.result.schema.json).

`params`:

| field | type | required | description |
| --- | --- | --- | --- |
| `role` | `"editor"` | yes | connecting peer role; editor is the client in alpha. |
| `clientName` | string | yes | client product name. |
| `clientVersion` | string | no | client product version. |
| `protocolVersions` | array of `versionString`, ≥1 | yes | supported protocol versions, preference order. |
| `capabilities` | array of `capabilityToken` | no | capability tokens offered/requested. |

`result`:

| field | type | required | description |
| --- | --- | --- | --- |
| `sessionId` | non-empty string | yes | session id for this connection; also echoed at envelope level afterward. |
| `protocolVersion` | `versionString` | yes | protocol version selected by the engine. |
| `server` | object `{ name, version?, engine? }` | yes | engine endpoint identity (generic fields only). |
| `capabilities` | array of `capabilityDescriptor` | no | capabilities advertised by the engine. |

### bridge.getCapabilities

Authoritative capability list. Schemas:
[params](../schema/methods/bridge.getCapabilities.params.schema.json),
[result](../schema/methods/bridge.getCapabilities.result.schema.json).

- `params`: empty object (no parameters).
- `result`: `{ capabilities: capabilityDescriptor[] }`.

See [`capabilities.md`](./capabilities.md) for negotiation semantics.

### engine.getStatus

Engine/runtime status snapshot. Schemas:
[params](../schema/methods/engine.getStatus.params.schema.json),
[result](../schema/methods/engine.getStatus.result.schema.json).

- `params`: empty object.
- `result`:

| field | type | required | description |
| --- | --- | --- | --- |
| `engineState` | `engineState` | yes | engine lifecycle state. |
| `runtimeState` | `runtimeState` | yes | runtime play/pause snapshot. |
| `engineName` | string | no | engine integration name. |
| `engineVersion` | string | no | engine version. |
| `title` | string | no | current window/game title. |

The editor backend separately owns *process* state (not started/starting/
running/exited/crashed); that is not part of this engine-served snapshot.

### engine.launchInfo

Engine's view of its process and external window. Schemas:
[params](../schema/methods/engine.launchInfo.params.schema.json),
[result](../schema/methods/engine.launchInfo.result.schema.json).

- `params`: empty object.
- `result`:

| field | type | required | description |
| --- | --- | --- | --- |
| `pid` | integer ≥ 0 | yes | engine OS process id. |
| `title` | string | yes | external window title. |
| `endpoint` | non-empty string | no | bridge endpoint the engine listens on. |
| `executable` | non-empty string | no | engine executable path/name as the engine reports it. |
| `argv` | array of string | no | launch arguments as the engine reports them. |
| `startedAt` | non-empty string | no | start timestamp (ISO 8601). |

### log.subscribe / log.unsubscribe

Log subscription management. Schemas:
[subscribe params](../schema/methods/log.subscribe.params.schema.json),
[subscribe result](../schema/methods/log.subscribe.result.schema.json),
[unsubscribe params](../schema/methods/log.unsubscribe.params.schema.json),
[unsubscribe result](../schema/methods/log.unsubscribe.result.schema.json).

`log.subscribe` `params`:

| field | type | required | description |
| --- | --- | --- | --- |
| `filter` | object | no | absent ⇒ subscribe to everything. |
| `filter.minLevel` | `logLevel` | no | minimum severity delivered. |
| `filter.categories` | array of non-empty string | no | if present and non-empty, deliver only these categories. |

`log.subscribe` `result`: `{ subscriptionId, effectiveFilter? }`, where
`subscriptionId` is a non-empty string and `effectiveFilter` echoes the
normalized filter the engine applied.

`log.unsubscribe` `params`: `{ subscriptionId }` (the id from a prior subscribe).
`result`: `{ ok: boolean }` — `true` when the subscription was found and removed.

### runtime.play / runtime.pause / runtime.stop

Runtime control requests. Each asks the engine to transition into the named
runtime state; the request is accepted or rejected synchronously, while the
actual state change is reported asynchronously by the
[`runtime.stateChanged`](#events) event. Schemas:
[play params](../schema/methods/runtime.play.params.schema.json),
[play result](../schema/methods/runtime.play.result.schema.json),
[pause params](../schema/methods/runtime.pause.params.schema.json),
[pause result](../schema/methods/runtime.pause.result.schema.json),
[stop params](../schema/methods/runtime.stop.params.schema.json),
[stop result](../schema/methods/runtime.stop.result.schema.json).

- `params`: empty object (no parameters) for all three.
- `result`:

| field | type | required | description |
| --- | --- | --- | --- |
| `accepted` | boolean | yes | whether the engine accepted the transition request. |
| `requestedState` | `runtimeState` | no | runtime state the engine is targeting (`playing` / `paused` / `stopped`). |

### runtime.focusViewport

Best-effort request to focus/raise the external engine window. Alpha has a single
window, so no target is specified. Schemas:
[params](../schema/methods/runtime.focusViewport.params.schema.json),
[result](../schema/methods/runtime.focusViewport.result.schema.json).

- `params`: empty object.
- `result`:

| field | type | required | description |
| --- | --- | --- | --- |
| `focused` | boolean | yes | `true` when the window was focused/raised; `false` when it could not be honored. |

## Events

Events are one-way notifications. Some arrive over the engine **wire**; others
are **synthesized** by the editor backend. The envelope is identical in both
cases; the producer column below mirrors the lifecycle table in
[`protocol-overview.md`](./protocol-overview.md).

| event | producer | `params` schema | required fields |
| --- | --- | --- | --- |
| `bridge.connected` | editor backend (synthesized) | [schema](../schema/events/bridge.connected.params.schema.json) | `endpoint`; optional `origin`. |
| `bridge.disconnected` | editor backend (synthesized) | [schema](../schema/events/bridge.disconnected.params.schema.json) | `reason`; optional `code` (screaming-snake-case), `willReconnect`, `origin`. |
| `engine.statusChanged` | engine (wire) | [schema](../schema/events/engine.statusChanged.params.schema.json) | `engineState`; optional `previous`, `runtimeState`, `title`. |
| `engine.processExited` | editor backend (synthesized) | [schema](../schema/events/engine.processExited.params.schema.json) | `exitCode`; optional `signal`, `origin`. |
| `runtime.stateChanged` | engine (wire) | [schema](../schema/events/runtime.stateChanged.params.schema.json) | `state` (`runtimeState`); optional `previous`. |
| `viewport.stateChanged` | engine (wire) | [schema](../schema/events/viewport.stateChanged.params.schema.json) | `state` (`viewportState`); optional `previous`. |
| `log.message` | engine (wire) | [schema](../schema/events/log.message.params.schema.json) | `level`, `message`; optional `category`, `timestamp`. |
| `error.reported` | both | [schema](../schema/events/error.reported.params.schema.json) | `error` (envelope error object); optional `origin`. |

`error.reported.params.error` reuses
[`envelope.schema.json#/$defs/error`](../schema/envelope.schema.json) by
reference, so the reported error has exactly the same `{ code, message, data? }`
shape as a response error. The full error code registry is phase C5.

## Two-layer validation

Every fixture under `methods/` and `events/` is a complete envelope and is
validated in two composed layers:

1. **Envelope layer** — the whole fixture validates against
   [`../schema/envelope.schema.json`](../schema/envelope.schema.json). The
   envelope schema is unchanged from phase C1; `method`/`event` names are checked
   by pattern only.
2. **Payload layer** — `params` (requests/events) or `result` (success
   responses) additionally validates against the per-method/per-event schema
   selected by directory name and `kind`.

Cross-file `$ref` (e.g. `error.reported` → envelope `$defs/error`, or any payload
schema → `common.schema.json`) resolves through a `referencing` registry built
from every `*.schema.json` keyed by its absolute `$id`.

Run the validator as described in
[`../fixtures/README.md`](../fixtures/README.md):

```powershell
pip install jsonschema
python scripts/validate-bridge-fixtures.py
```
