# NorvesEditor Bridge Message Envelope

Status: alpha, phase C1

This document specifies the Bridge envelope: the outer JSON object shared by
every Bridge message. It is the authoritative prose companion to
[`bridge/spec/schema/envelope.schema.json`](../schema/envelope.schema.json).
Where prose and schema disagree, the schema and its golden fixtures win.

## Shape

Every message is a single JSON object. The three message kinds (`request`,
`response`, `event`) share the same envelope and differ only in which fields are
required and which are forbidden.

```json
{
  "bridge": "norves.editor.bridge",
  "version": "0.1",
  "kind": "request",
  "id": "req-42",
  "method": "runtime.play",
  "params": {}
}
```

## Fields

| field | type | applies to | required | description |
| --- | --- | --- | --- | --- |
| `bridge` | string const `"norves.editor.bridge"` | all | yes | protocol marker; rejects non-Bridge traffic. |
| `version` | string `MAJOR.MINOR` | all | yes | protocol version, e.g. `"0.1"`. |
| `kind` | enum `request`/`response`/`event` | all | yes | envelope discriminator. |
| `id` | non-empty string | request, response | conditional | correlation id; required on requests and on the matching response; forbidden on events. |
| `method` | namespaced string | request | conditional | method name, e.g. `runtime.play`; forbidden on responses and events. |
| `event` | namespaced string | event | conditional | event name, e.g. `log.message`; forbidden on requests and responses. |
| `params` | object | request, event | optional | method/event payload; forbidden on responses. |
| `result` | any | response | conditional | success payload; mutually exclusive with `error`. |
| `error` | error object | response | conditional | error payload; mutually exclusive with `result`. |
| `sessionId` | non-empty string | all | optional | session id assigned during handshake; may appear on any kind once a session exists. |
| `seq` | integer ≥ 0 | all | optional | monotonically increasing per-connection sequence number. |

`additionalProperties` is `false` for the alpha. Unknown or misspelled fields are
rejected so that typos surface early. Adding a new envelope field is a protocol
change that requires review (per `AGENTS.md` / `CLAUDE.md`).

### Namespaced names

`method` and `event` names match `^[a-z][a-zA-Z0-9]*\.[a-zA-Z0-9]+$`: a
lower-camel namespace, a dot, then a member, e.g. `bridge.hello`,
`engine.statusChanged`. The envelope schema validates only the *shape* of the
name. Which names are legal, and their `params`/`result` payloads, is enforced by
per-method and per-event schemas in later phases.

### Error object

Provisional for phase C1; the full error model (code registry, retryability,
`data` shapes) is formalized in `error-model.md` in a later phase.

| field | type | required | description |
| --- | --- | --- | --- |
| `code` | screaming-snake-case string `^[A-Z][A-Z0-9_]*$` | yes | symbolic error code, e.g. `METHOD_NOT_SUPPORTED`. |
| `message` | non-empty string | yes | human-readable message. |
| `data` | any | optional | structured detail; shape is code-specific and defined later. |

`additionalProperties` is `false` on the error object as well.

## Per-kind rules

### request

- Required: `bridge`, `version`, `kind="request"`, `id`, `method`.
- Optional: `params`, `sessionId`, `seq`.
- Forbidden: `result`, `error`, `event`.

```json
{
  "bridge": "norves.editor.bridge",
  "version": "0.1",
  "kind": "request",
  "id": "req-1",
  "method": "bridge.hello",
  "params": { "role": "editor", "clientName": "NorvesEditor" }
}
```

### response

- Required: `bridge`, `version`, `kind="response"`, `id`, and **exactly one** of
  `result` or `error`.
- Optional: `sessionId`, `seq`.
- Forbidden: `method`, `event`, `params`.

Success:

```json
{
  "bridge": "norves.editor.bridge",
  "version": "0.1",
  "kind": "response",
  "id": "req-1",
  "sessionId": "sess-7f3a",
  "result": { "ok": true }
}
```

Error:

```json
{
  "bridge": "norves.editor.bridge",
  "version": "0.1",
  "kind": "response",
  "id": "req-42",
  "error": {
    "code": "METHOD_NOT_SUPPORTED",
    "message": "Engine does not support runtime.play in the current state.",
    "data": { "method": "runtime.play" }
  }
}
```

### event

- Required: `bridge`, `version`, `kind="event"`, `event`.
- Optional: `params`, `sessionId`, `seq`.
- Forbidden: `id`, `method`, `result`, `error`.

```json
{
  "bridge": "norves.editor.bridge",
  "version": "0.1",
  "kind": "event",
  "event": "log.message",
  "sessionId": "sess-7f3a",
  "seq": 12,
  "params": { "level": "info", "category": "Engine", "message": "Game started" }
}
```

Events are produced either by the engine over the wire or synthesized by the
editor backend. The envelope is identical in both cases; see the lifecycle table
in [`protocol-overview.md`](./protocol-overview.md).

## Correlation and ordering

- A `response` MUST echo the `id` of the `request` it answers. `id` values are
  unique per connection while the request is in flight.
- `seq`, when present, is per-connection and monotonically increasing. It is an
  ordering/debugging aid, not a correlation key; correlation is always by `id`.
- `sessionId` is assigned during the handshake (`bridge.hello` flow) and, once
  known, may be attached to subsequent messages.

## Fixtures

Golden fixtures for this envelope live under
[`bridge/spec/fixtures/envelope/`](../fixtures/envelope/), split into `positive/`
(must validate) and `negative/` (must fail). Run the validator described in
[`protocol-overview.md`](./protocol-overview.md) to check them.
