# Bridge Golden Fixtures

Golden fixtures are the cross-language contract for the NorvesEditor Bridge
protocol. Rust, TypeScript, and C++ implementers validate against the same files.

## Layout

Every fixture is a complete Bridge envelope. Fixtures are grouped by what they
exercise:

```text
fixtures/
  envelope/                     # phase C1: envelope structure only
    positive/                   #   MUST validate against the envelope schema
    negative/                   #   MUST fail (and must still parse as JSON)
  methods/<method>/             # phase C2+: one directory per method
    positive/                   #   request/response fixtures that MUST validate
    negative/                   #   fixtures that MUST fail (envelope or payload)
  events/<event>/               # phase C2+: one directory per event
    positive/
    negative/
```

The directory name under `methods/`/`events/` is the exact method/event name,
e.g. `methods/bridge.hello/`, `events/log.message/`.

```text
envelope/                 -> validated by ../schema/envelope.schema.json
methods/<method>/         -> envelope + ../schema/methods/<method>.{params,result}.schema.json
events/<event>/           -> envelope + ../schema/events/<event>.params.schema.json
```

## Two-layer validation

Fixtures under `methods/` and `events/` are validated in two composed layers:

1. **Envelope layer** — the whole fixture validates against
   `../schema/envelope.schema.json` (unchanged from phase C1; `method`/`event`
   names are checked by pattern only).
2. **Payload layer** — the payload additionally validates against a
   per-message schema selected by directory name and `kind`:
   - a `request` fixture validates `params` against `<method>.params.schema.json`;
   - a `response` fixture with `result` validates it against
     `<method>.result.schema.json`;
   - a `response` fixture with `error` is covered by the envelope layer only;
   - an `event` fixture validates `params` against `<event>.params.schema.json`.

A positive fixture must pass **both** layers; a negative fixture must fail **at
least one**. Cross-file `$ref` (e.g. `error.reported` → envelope `$defs/error`,
or payload schemas → `common.schema.json`) resolves through a `referencing`
registry built from every `*.schema.json` keyed by its `$id`.

See [`../docs/message-payloads.md`](../docs/message-payloads.md) for the payload
reference.

## Naming

- Envelope-group fixtures are named `<kind>-<topic>.json`.
- Method-group fixtures are named by kind, e.g. `request-valid.json`,
  `response-valid.json`; negatives are named after the rule they violate, e.g.
  `request-missing-role.json`, `response-pid-not-integer.json`.
- Event fixtures encode their producer so implementers know which side emits
  them:
  - `event-engine-*` — produced by the engine over the WebSocket wire.
  - `event-synthesized-*` — synthesized by the editor backend (no wire frame).
  See the lifecycle table in `../docs/protocol-overview.md`.
- Negative fixtures are named after the rule they violate, e.g.
  `request-missing-id.json`, `response-result-and-error.json`,
  `unknown-field.json`, `event-invalid-level.json`.

## Validation

```powershell
pip install jsonschema
python scripts/validate-bridge-fixtures.py
```

The script asserts that every `positive/` fixture validates and every
`negative/` fixture fails, and that all fixtures parse as JSON. `jsonschema`
(which pulls in `referencing`) is a developer tool only and is not vendored into
the repository.

When adding a protocol message, add the payload schema(s), a positive fixture,
and a negative fixture when validation behavior matters, in the same phase.
