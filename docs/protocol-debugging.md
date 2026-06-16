# Bridge Protocol Debugging

This guide describes the tools and techniques available for debugging the
NorvesEditor Bridge protocol. It covers schema validation, human-readable
envelope inspection, name-drift detection, and reading logs in the editor UI.
It does not duplicate the authoritative specs; links point to the primary sources.

## Schema & Fixtures

### Location

```text
bridge/spec/schema/        JSON Schema files (envelope + per-method/per-event)
bridge/spec/fixtures/      Golden test fixtures (positive and negative)
```

Schema files:

- `bridge/spec/schema/envelope.schema.json` — outer envelope structure for all
  message kinds (`request` / `response` / `event`).
- `bridge/spec/schema/common.schema.json` — shared `$defs`
  (`versionString`, `logLevel`, `engineState`, `runtimeState`, etc.).
- `bridge/spec/schema/methods/<method>.{params,result}.schema.json` — one pair
  per method.
- `bridge/spec/schema/events/<event>.params.schema.json` — one file per event.

Fixtures under `bridge/spec/fixtures/` follow a two-layer layout:

```text
fixtures/
  envelope/         envelope-only fixtures (phase C1)
    positive/       MUST validate
    negative/       MUST fail (but still parse as JSON)
  methods/<method>/ phase C2+ method fixtures
    positive/
    negative/
  events/<event>/   phase C2+ event fixtures
    positive/
    negative/
```

Event fixtures encode their producer in the file name:
`event-engine-*` means produced over the WebSocket wire by the engine;
`event-synthesized-*` means produced by the editor backend without a wire frame.
See the lifecycle table in
[`bridge/spec/docs/protocol-overview.md`](../bridge/spec/docs/protocol-overview.md).

The full fixture README is at
[`bridge/spec/fixtures/README.md`](../bridge/spec/fixtures/README.md).

### Running Fixture Validation

```powershell
pip install jsonschema
python scripts/validate-bridge-fixtures.py
```

The script asserts that every `positive/` fixture validates and every
`negative/` fixture fails. Both the envelope layer and the per-message payload
layer are checked for `methods/` and `events/` fixtures
(see [two-layer validation](../bridge/spec/fixtures/README.md#two-layer-validation)).

Use `scripts/verify.ps1` to run this together with the Rust gates:

```powershell
./scripts/verify.ps1
```

> Do not commit generated build output (`node_modules/`, `target/`, `build/`,
> `__pycache__`). `Cargo.lock` and `pnpm-lock.yaml` are committed.

### Primary Protocol Docs

| Topic | File |
| --- | --- |
| Envelope structure and per-kind field rules | [`bridge/spec/docs/message-envelope.md`](../bridge/spec/docs/message-envelope.md) |
| Alpha method/event surface, lifecycle events | [`bridge/spec/docs/protocol-overview.md`](../bridge/spec/docs/protocol-overview.md) |
| Per-method and per-event payload shapes | [`bridge/spec/docs/message-payloads.md`](../bridge/spec/docs/message-payloads.md) |
| Error code registry, versioning semantics | [`bridge/spec/docs/error-model.md`](../bridge/spec/docs/error-model.md) |

---

## bridge-dump CLI

`bridge-dump` reads a single Bridge envelope (from a file or stdin), decodes it
into a `ValidatedEnvelope`, and prints a structured human-readable summary to
stdout. Decode failures go to stderr and exit non-zero. The tool is synchronous
and has no Tokio dependency.

### Build

```powershell
cargo build -p norves-bridge-dump
```

Binary: `target/debug/bridge-dump`

### Usage

```powershell
# From a file
target/debug/bridge-dump --file path/to/envelope.json

# From stdin
cat envelope.json | target/debug/bridge-dump
```

### Sample output

```text
Kind      : request
Version   : 0.1
ID        : req-1
Method    : bridge.hello
Params    :
  {
    "role": "editor",
    "clientName": "NorvesEditor"
  }
```

For a response with an error payload:

```text
Kind      : response
Version   : 0.1
ID        : req-42
Payload   : error
  Code    : METHOD_NOT_SUPPORTED
  Message : Engine does not support runtime.play in the current state.
  Data    :
    {
      "method": "runtime.play"
    }
```

### Typical workflow

Capture a raw envelope from a fixture, a log file, or a network sniffer, then
pipe or pass it to `bridge-dump` to confirm the kind, method, and payload fields
at a glance — without writing a custom JSON viewer.

---

## Protocol Name Integrity Check

The TypeScript bridge-ui package and the Rust backend (`apps/editor/src-tauri`)
each maintain a set of Tauri IPC name constants (commands in snake_case, events
as `bridge:`-prefixed strings). The check script asserts that the two sets are
byte-identical:

```powershell
node scripts/check-protocol-names.mjs
```

Files compared:

| File | Contents |
| --- | --- |
| `bridge/ts/packages/bridge-ui/src/commands.ts` | TS command name constants |
| `bridge/ts/packages/bridge-ui/src/events.ts` | TS event name constants |
| `apps/editor/src-tauri/src/protocol_names.rs` | Rust `commands` and `events` modules |

Exit 0 means the two sides are in sync. Exit 1 prints the drift and indicates
which names are present in TS only or Rust only. Fix by updating one side to
match the other, then re-run.

---

## Reading Logs

### Game View panel and Log panel (editor UI)

The editor UI shows engine log output in two places:

- **Game View panel** — connection status badge, error banner, and runtime
  control buttons. The error banner displays the `kind` and `message` from the
  last bridge error, and can be dismissed.
- **Log panel** — scrollable list of `log.message` entries received from the
  engine. Each entry shows time, level (`TRACE` / `DEBUG` / `INFO` / `WARN` /
  `ERROR`), and message text. The optional `category` field is not surfaced
  directly in the current alpha Log panel.

### log.subscribe and log.message

The editor sends `log.subscribe` on session establishment. The engine then pushes
`log.message` events for each log line.

`log.message` event params fields:

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `level` | `trace`/`debug`/`info`/`warn`/`error` | yes | log severity |
| `message` | non-empty string | yes | log text |
| `category` | string | no | channel name, e.g. `Engine`, `Render`, `Audio` |
| `timestamp` | string (ISO 8601) | no | engine-side timestamp |

Full payload spec: [`bridge/spec/docs/message-payloads.md`](../bridge/spec/docs/message-payloads.md).

### Engine stderr

Engine stderr is **not** forwarded over the Bridge connection. In the alpha the
Rust backend does not capture or relay the engine's stderr. When debugging engine
crashes or startup failures, check:

1. The terminal/shell where `pnpm tauri dev` was launched — the backend prints
   process lifecycle events there.
2. The native console of the engine process (if it has one) or a separate log
   file written by the engine itself.

The READY handshake uses **stdout** only; the engine must not write other lines
to stdout before the `READY <port>` line (see the Connection Contract in
[`docs/engine-integration.md`](engine-integration.md#launch-sequence)).

---

## Verified Methods and Events (Alpha)

The full list of verified alpha methods and events — with their key result fields
— is the Connection Contract in
[`docs/engine-integration.md`](engine-integration.md#verified-methods-alpha).
Do not duplicate it here; consult that section as the single source of truth.

Summary: methods `bridge.hello`, `bridge.getCapabilities`, `engine.getStatus`,
`engine.launchInfo`, `runtime.play`, `runtime.pause`, `runtime.stop`,
`runtime.focusViewport`, `log.subscribe`, `log.unsubscribe`; events
`runtime.stateChanged`, `log.message`.

> Scene, object, and schema methods (`scene.getTree`, `object.getSnapshot`,
> `object.setProperty`, `schema.getSnapshot`) are specified in the schema but
> are **not supported in alpha** — the engine returns `METHOD_NOT_SUPPORTED`.

---

## Known Limitations: log.subscribe

1. **Server-side filter not implemented.** The `filter` object in
   `log.subscribe` params (fields `minLevel`, `categories`) is parsed and stored
   by the adapter, but filtering is applied client-side in the editor, not
   engine-side. The engine delivers all log lines regardless of the requested
   filter; the editor drops entries that do not pass the filter locally. This is
   an alpha limitation.

2. **Single subscription only.** The alpha supports one active `log.subscribe`
   per session. Subscribing a second time without first calling `log.unsubscribe`
   is not defined behavior for the alpha.

These limitations are part of the alpha non-goals. See the Non-Goals section of
[`docs/alpha-scope.md`](alpha-scope.md) and the broader known-limitations list
in [`docs/alpha-project-plan.md`](alpha-project-plan.md).
