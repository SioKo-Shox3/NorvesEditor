# 0006: Bridge Error Model and Versioning

Status: Accepted for alpha

## Context

Through phases C1–C4 the Bridge envelope carried a *provisional* error object:
`error.code` was pattern-constrained screaming-snake-case, but there was no
registry of which codes exist, what they mean, whether they are retryable, or
which side produces them. The `data` shape was left "defined later".

Two gaps were specifically undefined:

- The wire representation of a **version negotiation failure** during
  `bridge.hello` was unspecified — it was unclear whether a failure should be an
  error response, an `error.reported` event, or something else.
- There was no statement of **MAJOR/MINOR backward-compatibility semantics** for
  the protocol version, nor of capability compatibility rules, leaving future
  evolution ungoverned.

Phase C5 closes these gaps without touching any JSON Schema.

## Decision

1. **Open registry.** The error code registry is open: any code matching the
   envelope `error.code` pattern (`^[A-Z][A-Z0-9_]*$`) is schema-valid. It is
   **not** a closed enum, and the schema is not modified to enforce membership.
2. **Registry scope.** The registry governs only the envelope `error` object's
   `code`. Other symbolic codes — notably `bridge.disconnected` `params.code`
   (e.g. `SOCKET_CLOSED`) — are explicitly **excluded**.
3. **Version negotiation failure path.** When `bridge.hello` cannot agree on a
   protocol version, the canonical outcome is an **id-bearing error response to
   `bridge.hello`** with `error.code` = `PROTOCOL_VERSION_UNSUPPORTED`, whose
   `data` informatively carries `offered`/`supported`. `error.reported` is not
   used for this path.
4. **Backward-compatibility semantics.** Protocol version is `MAJOR.MINOR`: MINOR
   is additive/backward-compatible, MAJOR is breaking. Adding a capability or a
   registry code is non-breaking; removing or changing the meaning of either is
   breaking.
5. **Stable core codes.** Three core codes — `PROTOCOL_VERSION_UNSUPPORTED`,
   `METHOD_NOT_SUPPORTED`, `BRIDGE_TRANSPORT_ERROR` — are stability-guaranteed
   and engine independent. Adding to or changing this core set requires an ADR
   revision.
6. **Documentation is canonical.** Phase C5 changes no schema; the canonical
   source for the registry and the compatibility/versioning rules is
   [`bridge/spec/docs/error-model.md`](../../bridge/spec/docs/error-model.md).

## Consequences

- Because the registry is open, engine endpoints may define their own extension
  codes (e.g. an `ENGINE_`-prefixed namespace), which keeps the generic Bridge
  independent of any specific engine, consistent with the project's
  generic-bridge boundary.
- Registry conformance is **outside** JSON Schema validation; the code ↔ fixture
  correspondence table in `error-model.md` is the canonical coverage source.
- A future phase could make the registry machine-readable (a registry JSON file)
  and add a registry ↔ fixture cross-check to
  `scripts/validate-bridge-fixtures.py`; this ADR leaves room for that without
  requiring it now.
- If `errorCategory` (or `retryable`) ever needs to appear on the wire or in
  generated types, it can be promoted to an enum later; for the alpha it stays
  registry metadata only.

## Affected workstreams

- **C (protocol spec):** introduces `error-model.md` and the new
  `response-version-unsupported.json` fixture; no schema change.
- **D (Rust), E (TypeScript), F (C++):** future implementations reference the
  registry and the version-negotiation-failure path defined here.

## Verification or migration notes

- `python scripts/validate-bridge-fixtures.py` passes (the new fixture validates
  at the envelope layer as an error response).
- This is a purely additive change — new docs and one new positive fixture — so
  it is **not** a breaking migration.
