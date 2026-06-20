# NorvesEditor Bridge Error Model and Versioning

Status: alpha, phase C5 (compatibility/versioning rules + error model)

This document is the canonical prose companion for two related concerns: the
symbolic **error code registry** carried on the Bridge envelope, and the
**compatibility / versioning rules** that govern how the protocol and its
capabilities evolve. Where prose and schema disagree, the schema and its golden
fixtures win — but the registry itself and the versioning semantics below are
canonical *here*, because phase C5 deliberately changes no schema.

## Scope

This registry governs exactly one thing: the symbolic error `code` that appears
in the envelope `error` object, i.e.
[`envelope.schema.json#/$defs/error`](../schema/envelope.schema.json)
(`#/$defs/error`). It does not govern any other `code`-named field elsewhere in
the protocol (see [Related symbolic codes](#related-symbolic-codes-out-of-registry-scope)).

## Error object shape

The error object is defined by the envelope schema's
[`$defs/error`](../schema/envelope.schema.json): `code` (required), `message`
(required), and `data` (optional), with `additionalProperties: false`. The
canonical definition of the `code` pattern (`^[A-Z][A-Z0-9_]*$`,
screaming-snake-case) lives in that schema; it is **not** re-defined here to
avoid a second source of truth.

`data` is code-specific. For the alpha its shape is **not** schema-enforced and
is purely informative; a future phase may tighten `data` per code, which is a
backward-compatible addition (it only constrains shapes that were previously
unconstrained).

## Error code registry (open)

The registry is **open**: any code that satisfies the envelope `error.code`
pattern is schema-valid. This is *not* a closed enum, and the schema is not
modified to enforce membership. Membership in the table below is a
documentation/compatibility contract, not a wire-level constraint.

Codes are layered into **core** codes (protocol/transport-defined, engine
independent, stability-guaranteed) and **engine extension** codes
(engine-defined, pattern-compliant only, no stability guarantee).

`category` and `retryable` below are **registry metadata** — they live in this
table only and are never carried on the wire.

### Core codes (stable, engine independent)

These three form the closed, stability-guaranteed core set. Adding or changing a
core code requires an ADR revision (see
[ADR 0006](../../../docs/adr/0006-bridge-error-model-and-versioning.md)).

| code | category | retryable | route | expected `data` (informative) | meaning |
| --- | --- | --- | --- | --- | --- |
| `PROTOCOL_VERSION_UNSUPPORTED` | core | no | `bridge.hello` error response (id-bearing) | `offered`, `supported` (version arrays) | handshake version negotiation found no common version. |
| `METHOD_NOT_SUPPORTED` | core | no | response error | `method` | the method is unknown, or depends on an un-negotiated capability. |
| `BRIDGE_TRANSPORT_ERROR` | core | maybe | `error.reported` event (editor-backend synthesized) | none required | transport/socket failure observed by the editor backend. |

### Engine extension codes (unstable, engine defined)

Engine endpoints MAY define their own codes. They are valid as long as they match
the envelope pattern; they carry **no** stability guarantee and are not part of
the core set. Engines SHOULD use an engine namespace prefix (e.g. `ENGINE_`) to
keep extension codes visually distinct from core codes. This is a naming
convention only — there is no closed enumeration of engine codes.

| code | category | retryable | route | expected `data` (informative) | meaning |
| --- | --- | --- | --- | --- | --- |
| `ENGINE_ASSET_LOAD_FAILED` | extension (example) | maybe | `error.reported` event (engine wire) | `asset` | example engine-defined code: an asset failed to load. |

## Code ↔ fixture correspondence

This table is the **single canonical source for registry coverage**: every
registry code maps to a golden fixture that exercises it. Paths are relative to
this document (`bridge/spec/docs/`).

| code | fixture |
| --- | --- |
| `PROTOCOL_VERSION_UNSUPPORTED` | [`../fixtures/methods/bridge.hello/positive/response-version-unsupported.json`](../fixtures/methods/bridge.hello/positive/response-version-unsupported.json) |
| `METHOD_NOT_SUPPORTED` | [`../fixtures/envelope/positive/response-error.json`](../fixtures/envelope/positive/response-error.json) |
| `BRIDGE_TRANSPORT_ERROR` | [`../fixtures/events/error.reported/positive/event-synthesized-valid.json`](../fixtures/events/error.reported/positive/event-synthesized-valid.json) |
| `ENGINE_ASSET_LOAD_FAILED` | [`../fixtures/events/error.reported/positive/event-engine-valid.json`](../fixtures/events/error.reported/positive/event-engine-valid.json) |

## Update discipline

Any change that adds or removes a registry code MUST update the fixture column of
the table above in the **same commit**. When introducing a new code that a
fixture will use, add the registry table row **first**, then add the fixture —
the table leads, the fixture follows.

## Related symbolic codes (out of registry scope)

The `code` field of
[`bridge.disconnected`](../schema/events/bridge.disconnected.params.schema.json)
params (e.g. `SOCKET_CLOSED`) is a **different** field: a disconnect-reason hint,
not an envelope `error.code`. It is **deliberately not** a member of this
registry. It shares the same screaming-snake-case pattern only because that
casing convention is reused protocol-wide; the shared pattern does not make it a
registry member.

## Compatibility and versioning

### Protocol version semantics

The protocol version is `MAJOR.MINOR` (the envelope `version` field, shaped by
[`common.schema.json#/$defs/versionString`](../schema/common.schema.json)).
The current protocol generation is **0.2** — a MINOR bump from 0.1, hence
backward-compatible. Backward-compatibility semantics:

- **MINOR** bumps are backward-compatible, purely additive changes.
- **MAJOR** bumps are breaking changes.

Because 0.1 -> 0.2 is a MINOR bump, the editor offers `["0.2", "0.1"]` and a
legacy 0.1-only engine still negotiates successfully, falling back to 0.1 (see
[Hello version negotiation](#hello-version-negotiation)).

### Hello version negotiation

1. The client offers candidate versions in `bridge.hello` `params.protocolVersions`
   (preference order).
2. The engine selects exactly one and echoes it in `result.protocolVersion`.
3. If the engine can select **none** of the offered versions, the canonical
   outcome is an **id-bearing error response to `bridge.hello`** with
   `error.code` = `PROTOCOL_VERSION_UNSUPPORTED`. `error.data` SHOULD include
   `offered` and `supported` version arrays, informatively.

The `error.reported` event is **out-of-band only** and is not used for this
handshake path. (If a future need arises to signal version invalidation
out-of-band — outside a request/response pair — `error.reported` would be the
vehicle, but the handshake failure itself always travels as the hello response.)

### Capability backward-compatibility rules

This document is the **canonical** source for capability version/compat rules;
[`capabilities.md`](./capabilities.md) links here rather than duplicating them.

- Adding a capability is **non-breaking**.
- Removing a capability, or changing the meaning of an existing one, is
  **breaking**.

A capability's own `version` (`MAJOR.MINOR`) moves independently of the protocol
version and follows the same additive-MINOR / breaking-MAJOR semantics for that
individual feature.

### Compatibility of registry changes

Because the registry is open:

- **Adding** a code is **non-breaking** (MINOR-equivalent): existing peers that
  do not recognize it already tolerate unknown pattern-valid codes.
- **Changing the meaning of, or removing, a code** is **breaking** and requires
  an ADR, consistent with `docs/agent-guide/protocol-schema.md` ("compatibility
  breaking changes require an ADR or explicit migration note").

### Related ADR

The decisions captured here are recorded in
[ADR 0006: Bridge error model and versioning](../../../docs/adr/0006-bridge-error-model-and-versioning.md).
