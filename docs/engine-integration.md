# Engine Integration

NorvesEditor integrates with engines through the Bridge protocol and a standalone C++ engine-side SDK. The generic Bridge layer must remain independent from any one engine.

## Generic Boundary

```text
NorvesEditor UI
  -> Tauri command/event wrappers
Rust backend
  -> Bridge editor client runtime
WebSocket + JSON
C++ engine-side SDK
  -> engine adapter
```

The C++ SDK public API must not expose Tauri, React, TypeScript, NorvesLib, or third-party WebSocket types.

## NorvesLib Adapter

NorvesLib is the first reference engine adapter candidate. Its adapter is responsible for:

```text
- Mapping NorvesLib runtime/log/status into Bridge DTOs.
- Marshaling runtime commands onto the safe NorvesLib thread/context.
- Avoiding direct transport of NorvesLib live object memory.
- Keeping NorvesLib-specific containers and object rules out of the generic SDK.
```

If the adapter lives in the NorvesLib repository, follow NorvesLib repository rules there. If an integration shim is needed in NorvesEditor, isolate it from the generic C++ SDK.

## Mock Engine First

Early implementation should prove protocol and transport behavior with a mock engine before binding to NorvesLib. Mock-engine parity is acceptable for internal alpha progress; public alpha should include NorvesLib integration or a documented adapter task list.

## Connection Contract

This section records the connection and session contract that was implemented and
verified in phases L-P2 through L-P3b. Only verified, working behavior is described.

### Launch Sequence

The Rust backend owns the entire engine process lifecycle:

1. The backend picks a free OS-assigned ephemeral port via
   `TcpListener::bind("127.0.0.1:0")`, drops the listener, and passes the port
   to the engine as `--bridge-port <port>`.
2. The engine binds the port and writes `READY <port>` to **stdout** (the raw
   inherited pipe handle). In bridge mode the engine does not write any other
   lines to stdout before READY — the channel is kept clean so the backend can
   parse reliably.
3. The backend reads stdout line-by-line, finds the `READY <port>` line within
   a 10-second timeout, validates the port matches the injected value, and then
   connects to `ws://127.0.0.1:<port>`.

If the READY line does not arrive within the timeout, the backend kills the
engine and returns an error to the caller.

### WebSocket + JSON Bridge Session

After the READY handshake:

- `connect_with_retry` establishes the WebSocket connection with exponential
  back-off (up to 5 seconds).
- `bridge.hello` is sent first on every new connection; the response contains
  the `sessionId` that identifies the session for the engine's lifetime.

The Rust backend keeps a single persistent `DispatchHandle` per engine instance.
Requests and push events share the same underlying WebSocket connection.

### Verified Methods (Alpha)

The following methods were verified end-to-end against the NorvesLib adapter
(see `engine_runtime_control_contract` and `engine_launch_info_schema_compliance_contract`
e2e tests):

| Method | Key result fields |
|---|---|
| `bridge.hello` | `sessionId` (non-empty string), `protocolVersion`, `server` |
| `bridge.getCapabilities` | `capabilities` (array of descriptor objects `{name, version?, description?}`) |
| `engine.getStatus` | `engineState`, `runtimeState`, `engineName`, `engineVersion` |
| `engine.launchInfo` | `pid` (integer ≥ 0), `title` (non-empty string) |
| `runtime.play` | `accepted` (bool), `requestedState` |
| `runtime.pause` | `accepted` (bool), `requestedState` |
| `runtime.stop` | `accepted` (bool), `requestedState` |
| `runtime.focusViewport` | `focused` (bool) |
| `log.subscribe` | `subscriptionId` (non-empty string) |
| `log.unsubscribe` | `ok` (bool) |

### Verified Events (Alpha)

| Event | Key params fields |
|---|---|
| `runtime.stateChanged` | `state` (string: playing/paused/stopped/edit), `previous` |
| `log.message` | `level` (Trace/Debug/Info/Warn/Error), `message` (non-empty string), `category` (optional) |

### Schema Compliance: engine.launchInfo

The `engine.launchInfo` result schema (`additionalProperties: false`) requires:

- `pid`: integer, minimum 0 — the OS process id as reported by the engine.
- `title`: string — the engine/game window title.

The NorvesLib adapter returns a schema-compliant `{pid, title}` object. This
contrasts with the reference mock engine (`norves_mock_engine`), which returns
the non-compliant `{launched: true}`. Similarly, `log.subscribe` returns the
compliant `{subscriptionId}` while the mock returns `{subscribed: true}`.

The `engine_launch_info_schema_compliance_contract` e2e test (see below) asserts:

- `pid` is present and `>= 0`.
- `title` is present and non-empty (the exact string is not hardcoded).
- The `launched` key is absent, confirming `additionalProperties:false` compliance.

### Running the env-gated e2e Tests

`apps/editor/src-tauri` is a **separate Cargo workspace** from the repository
root. It is excluded from `cargo test --workspace` at the root and from
`scripts/verify.ps1`. Run these tests from within `apps/editor/src-tauri`:

```powershell
# Against the reference mock engine
$env:NORVES_ENGINE_PATH = "<absolute path to norves_mock_engine.exe>"
cd apps/editor/src-tauri; cargo test --test process_e2e

# Against a NorvesLib engine build (runtime control / event streaming / launchInfo)
$env:NORVES_NORVESLIB_ENGINE_PATH = "<absolute path to NorvesLib Game.exe>"
cd apps/editor/src-tauri; cargo test --test process_e2e
```

When neither env variable is set, each e2e function prints a `[SKIP]` line and
returns immediately (the test suite passes). CI and manual verification set the
appropriate variable to opt in to the real run.

Note: use `cargo test --test process_e2e` (integration test by name), not
`cargo test -p <name>`, because the crate is in an excluded workspace and must
be addressed by changing into its directory first.
