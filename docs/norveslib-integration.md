# NorvesLib Integration

This document describes how to build NorvesLib with the Bridge SDK embedded and
connect it to NorvesEditor as the reference engine.

For the generic connection contract (launch sequence, session, verified methods
and events) see [`docs/engine-integration.md`](engine-integration.md). For
adapter boundary rules (generic SDK vs. NorvesLib-specific adapter) see
[`docs/engine-integration.md` §NorvesLib Adapter](engine-integration.md#norveslib-adapter) and
[`docs/agent-guide/norveslib-adapter.md`](agent-guide/norveslib-adapter.md).

---

## Prerequisites

- **NorvesLib repository** checked out separately (not inside NorvesEditor).
  NorvesLib is not a submodule of NorvesEditor.
- **Windows** — NorvesLib builds and runs on Windows only.
- **Vulkan SDK** installed and on `PATH` / `VULKAN_SDK` env var set.
  The mock engine does **not** require Vulkan; only NorvesLib does.
- **Visual Studio 2022** (C++ workload, MSVC toolchain).
- **CMake 3.21+**.
- Network access for the first CMake configure of NorvesEditor's C++ layer
  (`libwebsockets` is fetched via FetchContent on first run; see
  [Known Limitations](#known-limitations)).

---

## Step 1 — Build the NorvesEditor C++ Bridge SDK

NorvesLib links against the C++ engine-side SDK that lives in
`bridge/cpp/engine-sdk` inside NorvesEditor. You do not need to build the full
NorvesEditor C++ tree; you only need the SDK headers and the CMake target it
exports.

No separate build step is required: NorvesLib's CMake will consume the SDK
in-source via `-DNORVES_BRIDGE_SDK_DIR`.

> The CMake flag `NORVES_BRIDGE_SDK_DIR` tells NorvesLib where to find the
> NorvesEditor `bridge/cpp` tree. The SDK is fetched as a CMake subdirectory
> (`add_subdirectory`), so `libwebsockets` is downloaded during this configure
> step (network required on first run).

---

## Step 2 — Build NorvesLib with Bridge SDK Embedded

Run the following commands inside the **NorvesLib repository** (not inside
NorvesEditor):

```powershell
# Configure — point at the NorvesEditor bridge/cpp tree
cmake -B build -S . -DNORVES_BRIDGE_SDK_DIR="<absolute path to NorvesEditor root>/bridge/cpp"

# Build the Game target (Vulkan SDK required)
cmake --build build --config Debug --target Game
```

The resulting executable:

```
build/Game/Debug/Game.exe
```

> Do **not** commit `build/` output. CMake build directories are not tracked by
> version control.

---

## Step 3 — Launch NorvesLib from NorvesEditor

Set the `NORVES_NORVESLIB_ENGINE_PATH` environment variable to the absolute path
of `Game.exe` and then start NorvesEditor (or set `NORVES_ENGINE_PATH` to the
same path if you want the editor's default engine resolution to pick it up):

```powershell
# Option A — tell the editor to use NorvesLib as the default engine
$env:NORVES_ENGINE_PATH = "<absolute path>\build\Game\Debug\Game.exe"
cd apps/editor
pnpm tauri dev
```

Engine path resolution in `apps/editor/src-tauri`:

1. `NORVES_ENGINE_PATH` environment variable (absolute path).
2. Persisted settings (alpha — **not yet implemented**).
3. Default fallback: `norves_mock_engine` (bare name, resolved against the
   working directory).

For the alpha, the only supported path is option 1 (`NORVES_ENGINE_PATH`). A
Settings UI for the engine path is a post-alpha feature.

> `pnpm tauri dev` is the documented dev-mode launch command. Confirm it works
> on your machine before running the full integration scenario; local Vulkan /
> Tauri environment differences may require additional setup.

---

## Step 4 — Run env-gated e2e Tests Against NorvesLib

`apps/editor/src-tauri` is a **separate Cargo workspace** from the repository
root. Run these tests from inside that directory:

```powershell
# Set the NorvesLib engine path
$env:NORVES_NORVESLIB_ENGINE_PATH = "<absolute path>\build\Game\Debug\Game.exe"

# Run the integration test suite
cd apps/editor/src-tauri
cargo test --test process_e2e
```

- When `NORVES_NORVESLIB_ENGINE_PATH` is set, the following contracts are
  exercised: `engine_runtime_control_contract`,
  `engine_launch_info_schema_compliance_contract`, and
  `engine_event_streaming_contract`.
- When the variable is **not** set, each test function prints a `[SKIP]` line
  and returns immediately; the suite still passes. CI uses this opt-in pattern.

Use `cargo test --test process_e2e` (integration test by filename), not
`cargo test -p <crate>`, because the crate lives in an excluded workspace and
must be addressed by changing into its directory first.

---

## Schema Compliance: NorvesLib vs. Mock Engine

The `engine.launchInfo` and `log.subscribe` response schemas use
`additionalProperties: false`. NorvesLib and the reference mock engine differ
in compliance:

| Method | NorvesLib adapter (compliant) | `norves_mock_engine` (non-compliant) |
|---|---|---|
| `engine.launchInfo` | `{ "pid": <int ≥ 0>, "title": "<string>" }` | `{ "launched": true }` |
| `log.subscribe` | `{ "subscriptionId": "<non-empty string>" }` | `{ "subscribed": true }` |

The `engine_launch_info_schema_compliance_contract` e2e test asserts:

- `pid` is present and `>= 0`.
- `title` is present and non-empty.
- The `launched` key is absent (confirming `additionalProperties: false`
  compliance).

The `engine_event_streaming_contract` e2e test exercises `log.subscribe` and
`runtime.stateChanged` event delivery against the live NorvesLib adapter.

For the full table of verified methods and events see
[`docs/engine-integration.md` §Verified Methods](engine-integration.md#verified-methods-alpha).

---

## Adapter Boundary

The generic C++ Bridge SDK (`bridge/cpp/engine-sdk`) must not contain NorvesLib
headers or NorvesLib-specific logic. The NorvesLib adapter (which lives in the
NorvesLib repository) is responsible for:

- Mapping NorvesLib runtime/log/status into Bridge DTOs.
- Marshaling Bridge runtime commands onto the safe NorvesLib thread/context.
- Avoiding direct transport of NorvesLib live object memory.
- Keeping NorvesLib-specific containers and object rules out of the generic SDK.

See [`docs/engine-integration.md` §Generic Boundary](engine-integration.md#generic-boundary)
and [`docs/agent-guide/norveslib-adapter.md`](agent-guide/norveslib-adapter.md)
for the full boundary contract.

---

## Known Limitations

1. **Windows + Vulkan SDK required.** NorvesLib builds and runs on Windows only
   and requires a Vulkan SDK. The reference mock engine (`norves_mock_engine`)
   does not require Vulkan and can be used for protocol development on any
   supported platform.

2. **Network required on first CMake configure.** The C++ Bridge SDK uses
   CMake `FetchContent` to download `libwebsockets` v4.3.3. Subsequent builds
   use the cached download.

3. **scene / object / schema methods not supported in alpha.**
   `scene.getTree`, `object.*`, and `schema.*` are out of scope for the alpha
   (see `docs/alpha-project-plan.md` §3 and Workstream C4 / L5 optional items).
   Calling these methods returns a `not_supported` error.

4. **No native viewport embedding.** The engine runs its own native window;
   NorvesEditor does not embed GPU output inside the Tauri WebView. See
   [`docs/viewport-strategy.md`](viewport-strategy.md) for the alpha viewport
   approach and post-alpha research directions.

5. **Engine path Settings UI not implemented.** The engine executable path must
   be provided via the `NORVES_ENGINE_PATH` environment variable. A Settings UI
   for persistent path configuration is a post-alpha feature.

6. **Orphan risk on editor force-quit.** If NorvesEditor is force-terminated,
   the engine process may be left running. Windows Job Object integration to
   guarantee cleanup is a post-alpha item.

7. **localhost only.** The Bridge transport binds to `ws://127.0.0.1:<port>`.
   Remote or cross-machine connections are not supported.
