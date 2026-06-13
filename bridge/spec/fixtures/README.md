# Bridge Golden Fixtures

Golden fixtures are the cross-language contract for the NorvesEditor Bridge
protocol. Rust, TypeScript, and C++ implementers validate against the same files.

## Layout

```text
fixtures/
  <group>/
    positive/   fixtures that MUST validate against the group schema
    negative/   fixtures that MUST fail validation (and must still parse as JSON)
```

Phase C1 ships one group:

```text
envelope/  -> validated by ../schema/envelope.schema.json
```

## Naming

- Positive envelope fixtures are named `<kind>-<topic>.json`.
- Event fixtures additionally encode their producer so implementers know which
  side emits them:
  - `event-engine-*` — produced by the engine over the WebSocket wire.
  - `event-synthesized-*` — synthesized by the editor backend (no wire frame).
  See the lifecycle table in `../docs/protocol-overview.md`.
- Negative fixtures are named after the rule they violate, e.g.
  `request-missing-id.json`, `response-result-and-error.json`,
  `unknown-field.json`.

## Validation

```powershell
pip install jsonschema
python scripts/validate-bridge-fixtures.py
```

The script asserts that every `positive/` fixture validates and every
`negative/` fixture fails, and that all fixtures parse as JSON. `jsonschema` is a
developer tool only and is not vendored into the repository.

When adding a protocol message, add the schema, a positive fixture, and a
negative fixture when validation behavior matters, in the same phase.
