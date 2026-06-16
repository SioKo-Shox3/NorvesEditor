# Engine Profile

This document explains how the editor resolves and configures the engine
executable it will launch. It covers the alpha path-resolution rules,
how to point the editor at the mock engine or a NorvesLib build, and the
distinction between runtime configuration variables and test-only variables.

For the launch handshake that follows once the executable is resolved, see
[docs/engine-integration.md — Connection Contract](engine-integration.md#connection-contract).

---

## Engine Path Resolution (Alpha)

`apps/editor/src-tauri/src/process_runtime.rs` reads a single environment
variable and passes it through the pure resolver in `process.rs`:

```
NORVES_ENGINE_PATH (env)
  -> config (alpha: not implemented, always None)
    -> default: "norves_mock_engine" (bare name, resolved against cwd)
```

Rules applied by `resolve_engine_path`:

- A value that is present but blank or whitespace-only is treated as **absent**
  and the resolver falls through to the next source.
- The first non-blank source wins.
- The default is the bare string `norves_mock_engine`, resolved against the
  process working directory. For alpha use, always set `NORVES_ENGINE_PATH` to
  an absolute path.

The resolver itself is pure (no filesystem access). After resolution,
`validate_engine_path` checks that the resolved path exists and is a regular
file; if not, the launch command returns an error before spawning anything.

---

## Using the Mock Engine

Build the mock engine first (requires CMake and Visual Studio 17 2022;
**no Vulkan SDK needed**):

```powershell
# Configure (first run downloads libwebsockets via FetchContent — network required)
cmake -S bridge/cpp -B build/cpp -G "Visual Studio 17 2022"

# Build
cmake --build build/cpp --config Debug
```

The artifact is:

```
build/cpp/examples/mock-engine/Debug/norves_mock_engine.exe
```

> **Do not commit** the `build/cpp/` directory.

Set `NORVES_ENGINE_PATH` to the absolute path before launching the editor:

```powershell
$env:NORVES_ENGINE_PATH = "C:\...\NorvesEditor\build\cpp\examples\mock-engine\Debug\norves_mock_engine.exe"
pnpm tauri dev   # run from apps/editor
```

### Mock Engine Limitations

The reference mock engine implements most Bridge methods but does **not** fully
comply with the `engine.launchInfo` response schema: it returns
`{launched: true}` instead of the required `{pid, title}`. Similarly,
`log.subscribe` returns `{subscribed: true}` instead of `{subscriptionId}`.
These deviations are acceptable for transport/protocol smoke-testing but mean
the mock engine cannot pass the `engine_launch_info_schema_compliance_contract`
e2e test. See
[docs/engine-integration.md — Schema Compliance](engine-integration.md#schema-compliance-enginelaunchinfo)
for the exact requirements.

---

## Using a NorvesLib Engine Build

NorvesLib is the first reference engine adapter. Building it requires
**Windows and the Vulkan SDK**.

Build NorvesLib with the embedded Bridge SDK (run in the NorvesLib repository,
not in NorvesEditor):

```powershell
cmake -B build -S . -DNORVES_BRIDGE_SDK_DIR=<NorvesEditor root>/bridge/cpp
cmake --build build --config Debug --target Game
```

The artifact is:

```
build/Game/Debug/Game.exe
```

For full adapter details and NorvesLib-specific rules, see
[docs/agent-guide/norveslib-adapter.md](agent-guide/norveslib-adapter.md).

Set `NORVES_ENGINE_PATH` to the absolute path before launching the editor:

```powershell
$env:NORVES_ENGINE_PATH = "C:\...\NorvesLib\build\Game\Debug\Game.exe"
pnpm tauri dev   # run from apps/editor
```

The NorvesLib adapter returns schema-compliant responses for `engine.launchInfo`
(`{pid, title}`) and `log.subscribe` (`{subscriptionId}`), which pass the
full alpha e2e contract.

---

## `NORVES_NORVESLIB_ENGINE_PATH` — Test-Only Variable

`NORVES_ENGINE_PATH` is the **runtime** override consumed by the editor itself.

`NORVES_NORVESLIB_ENGINE_PATH` is a **separate, test-only** variable consumed
exclusively by the env-gated e2e tests in `apps/editor/src-tauri`. It is never
read by `process_runtime.rs` and has no effect on the running editor.

| Variable | Who reads it | Purpose |
|---|---|---|
| `NORVES_ENGINE_PATH` | `process_runtime.rs` (editor backend); also the mock e2e `engine_launch_kill_relaunch_contract` in `tests/process_e2e.rs` | Select engine executable at runtime |
| `NORVES_NORVESLIB_ENGINE_PATH` | `tests/process_e2e.rs` only | Opt in to NorvesLib e2e test run |

Example — running the e2e suite against NorvesLib (from `apps/editor/src-tauri`):

```powershell
$env:NORVES_NORVESLIB_ENGINE_PATH = "C:\...\NorvesLib\build\Game\Debug\Game.exe"
cargo test --test process_e2e
```

When neither variable is set, each e2e test prints `[SKIP]` and returns
immediately; the suite still passes. See
[docs/engine-integration.md — Running the env-gated e2e Tests](engine-integration.md#running-the-env-gated-e2e-tests)
for the full invocation reference.

---

## Launch Sequence

Once the executable is resolved and validated, the Rust backend:

1. Picks a free ephemeral port.
2. Spawns the engine with `--bridge-port <port>`.
3. Reads stdout until the engine writes `READY <port>` (10-second timeout).
4. Connects to `ws://127.0.0.1:<port>` and begins the Bridge session.

Full details are in
[docs/engine-integration.md — Connection Contract](engine-integration.md#connection-contract).

---

## Settings UI (Alpha: Not Implemented)

Alpha does not include a Settings UI for the engine path. The only supported
override mechanism is the `NORVES_ENGINE_PATH` environment variable, set
before launching the editor.

The `config` slot in `resolve_engine_path` is present in the source but wired
to `None` for the entire alpha. A Settings panel that persists and supplies this
value is a post-alpha task.

---

## Known Limitations (Alpha)

- **No Settings UI for engine path.** `NORVES_ENGINE_PATH` is the only
  override. The Settings UI and persistent config are post-alpha. (See
  [docs/viewport-strategy.md](viewport-strategy.md) for the broader alpha
  scope context.)
- **localhost only.** The backend always connects to `ws://127.0.0.1:<port>`.
  Remote engine connections are not supported.
- **NorvesLib requires Windows + Vulkan SDK.** The mock engine does not require
  Vulkan.
- **C++ configure requires network on first run.** `libwebsockets` (v4.3.3) is
  fetched via CMake FetchContent on the first configure.
- **Orphan risk on editor force-quit.** If the editor is killed abruptly, the
  engine process may not be cleaned up. Windows Job Object mitigation is
  post-alpha.

---

## Development Launch (Editor)

```powershell
# From repo root
pnpm install

# From apps/editor
pnpm tauri dev
```

`tauri.conf.json` sets `beforeDevCommand` to `pnpm --filter @norves/editor dev`,
which starts the Vite dev server on `localhost:1420` before Tauri opens the
WebView. The `NORVES_ENGINE_PATH` variable must be set in the same shell session
before running `pnpm tauri dev`.

> Note: the exact invocation above should be verified on your target machine,
> as platform-specific environment and toolchain differences may require
> adjustments.
