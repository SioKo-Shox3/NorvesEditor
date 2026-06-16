# Build and Verification Guide

This guide covers every build and verification step for NorvesEditor. It is
written for humans; the agent-facing rules live in
[`docs/agent-guide/build-and-verify.md`](agent-guide/build-and-verify.md).

---

## Prerequisites

| Tool | Version / Notes |
|---|---|
| Node.js | 20 LTS or later |
| pnpm | 11.6.0 (declared in `packageManager` field of `package.json`) |
| Rust toolchain | stable (managed by `rust-toolchain.toml` if present) |
| Python | 3.9 or later (for fixture validation script) |
| jsonschema (Python) | `pip install jsonschema` |
| CMake | 3.21 or later (for C++ mock engine) |
| Visual Studio 2022 | Windows: "Desktop development with C++" workload (for MSVC generator) |
| Git | any recent version |
| Tauri CLI | bundled via `pnpm tauri` from `apps/editor` — no separate install needed |

> **Note:** Vulkan SDK is **not** required to build or run the mock engine. It
> is only needed when building/running a NorvesLib engine (see
> [env-gated e2e](#env-gated-e2e) below).

---

## 1. Install JS Dependencies

Run once after cloning, and again whenever `pnpm-lock.yaml` changes:

```powershell
pnpm install
```

This installs packages for all workspaces (`apps/*`,
`bridge/ts/packages/*`). `pnpm-lock.yaml` is committed and must not be
deleted. `node_modules/` must never be committed.

---

## 2. Protocol Fixtures Gate

Validates that every JSON fixture in `bridge/spec/fixtures/` conforms to the
protocol schema.

```powershell
# Install the Python dependency once if not already installed
pip install jsonschema

# Run the validator (from the repository root)
python scripts/validate-bridge-fixtures.py
```

The script exits non-zero on any schema violation and prints the offending
fixture path.

---

## 3. Rust Bridge Workspace Gates

The **root `Cargo.toml`** manages four bridge crates only:

- `bridge/crates/norves-bridge-core`
- `bridge/crates/norves-bridge-editor-client`
- `bridge/crates/norves-bridge-tools`
- `bridge/crates/norves-bridge-dump`

`apps/editor/src-tauri` is a **separate, excluded** Cargo workspace and is
**not** covered by the commands below. Run all three gates from the repository
root:

```powershell
# Format check
cargo fmt --all -- --check

# Lint (warnings are errors)
cargo clippy --workspace --all-targets -- -D warnings

# Unit + integration tests
cargo test --workspace
```

---

## 4. C++ Mock Engine

The mock engine (`norves_mock_engine`) lets you verify the protocol without a
real NorvesLib build. It does not require Vulkan.

### 4.1 Configure (first time — requires network)

CMake fetches `libwebsockets v4.3.3` via `FetchContent` on the first
configure. Ensure internet access is available.

```powershell
cmake -S bridge/cpp -B build/cpp -G "Visual Studio 17 2022"
```

Subsequent configures (after source changes only) skip the network fetch if
the downloaded sources are already in CMake's cache directory.

### 4.2 Build

```powershell
cmake --build build/cpp --config Debug
```

### 4.3 Artifact

```text
build/cpp/examples/mock-engine/Debug/norves_mock_engine.exe
```

### 4.4 Run Tests

```powershell
ctest --test-dir build/cpp -C Debug --output-on-failure
```

> `build/cpp/` is a generated directory and must **not** be committed.

---

## 5. Tauri Editor Dev Launch

Make sure JS dependencies are installed first (`pnpm install` from the
repository root), then:

```powershell
cd apps/editor
pnpm tauri dev
```

`tauri.conf.json`'s `beforeDevCommand` is `pnpm --filter @norves/editor dev`,
which starts a Vite dev server on `localhost:1420` before Tauri opens its
window. The Tauri window connects to that Vite server at startup.

> **Recommendation:** The exact behavior of `pnpm tauri dev` depends on the
> local environment (installed Rust toolchain, Visual Studio build tools, PATH
> configuration). Verify this command on the actual target machine before
> relying on it.

To point the editor at an engine executable, set `NORVES_ENGINE_PATH` before
launching (see [env-gated e2e](#env-gated-e2e) for format). The engine path
Settings UI is not implemented in alpha.

---

## 6. Frontend Typecheck

```powershell
pnpm -r --if-present typecheck
```

The `--if-present` flag means that if no workspace package defines a
`typecheck` script, the command exits cleanly without error. This is expected
while frontend packages are sparse.

---

## 7. Aggregate Runner

`scripts/verify.ps1` runs all gates in order and stops on the first failure.
Use this as the primary gate before committing or merging.

```powershell
# Protocol fixtures + Rust gates + frontend typecheck
./scripts/verify.ps1

# Also include C++ build and ctest (requires build/cpp to be pre-configured)
./scripts/verify.ps1 -Cpp

# Skip frontend pnpm gates
./scripts/verify.ps1 -SkipFrontend
```

When `-Cpp` is passed but `build/cpp` does not exist, the script prints a
`[SKIP]` notice and continues. Always run the script from the repository root.

---

## 8. Env-Gated E2E Tests

The end-to-end tests in `apps/editor/src-tauri` exercise the full
launch → READY → WebSocket → Bridge session contract against a real engine
binary. Because `apps/editor/src-tauri` is an **excluded, separate Cargo
workspace**, these tests must be run from inside that directory.

### 8.1 Against the Mock Engine

```powershell
$env:NORVES_ENGINE_PATH = "<absolute path to norves_mock_engine.exe>"
cd apps/editor/src-tauri
cargo test --test process_e2e
```

With only `NORVES_ENGINE_PATH` set, the single mock-compatible contract
`engine_launch_kill_relaunch_contract` runs. The runtime-control, launchInfo,
and event-streaming contracts assert NorvesLib-specific behavior and therefore
require the NorvesLib engine (§8.2); they `[SKIP]` under the mock.

### 8.2 Against a NorvesLib Engine Build

> Requires Windows + Vulkan SDK. Build the NorvesLib engine separately using
> its own CMake build (see the NorvesLib repository for instructions). The
> `NORVES_BRIDGE_SDK_DIR` variable points it at the SDK from this repository.

```powershell
$env:NORVES_NORVESLIB_ENGINE_PATH = "<absolute path to NorvesLib Game.exe>"
cd apps/editor/src-tauri
cargo test --test process_e2e
```

### 8.3 Skip Behavior

When **neither** `NORVES_ENGINE_PATH` nor `NORVES_NORVESLIB_ENGINE_PATH` is
set, every e2e function prints `[SKIP]` and returns immediately. The test
suite still passes with exit code 0. This is the expected behavior in CI
environments without an engine binary.

> Use `cargo test --test process_e2e`, not `cargo test -p <crate-name>`,
> because the crate is in a workspace that is excluded from the root and must
> be addressed by `cd`-ing into its directory first.

---

## 9. Troubleshooting

### Engine does not send READY / launch times out

- Check that `NORVES_ENGINE_PATH` points to the correct executable and that it
  is executable.
- The engine must write `READY <port>` to **stdout** within 10 seconds of
  startup. If the engine writes other lines to stdout before `READY`, the
  backend parser may miss it — the engine's bridge mode must keep stdout clean.
- Verify the engine is compiled with bridge support enabled (not running in a
  headless or non-bridge mode).
- See the [Connection Contract](engine-integration.md#connection-contract) for
  the full launch sequence.

### Port conflict

- The backend selects an OS-assigned ephemeral port via
  `TcpListener::bind("127.0.0.1:0")`. Port conflicts are unlikely but possible
  if another process holds every port in the ephemeral range (extremely rare).
- If the engine reports the port is already in use, verify no other editor
  instance is running and retry.

### WebSocket connection fails after READY

- Ensure the engine actually binds to `127.0.0.1:<port>` — not `0.0.0.0` or
  a different address. The editor connects to `ws://127.0.0.1:<port>` only.
- Check that a local firewall or security tool is not blocking loopback
  WebSocket connections.
- `connect_with_retry` retries with exponential back-off up to 5 seconds; if
  the connection still fails, the error is surfaced to the Game View panel.

### CMake configure fails (C++ mock engine)

- The first configure fetches `libwebsockets v4.3.3` via `FetchContent`.
  Ensure internet access is available. A corporate proxy or firewall may block
  the download; configure the proxy for `cmake` or pre-download the sources.
- Verify CMake 3.21 or later: `cmake --version`.
- On Windows, make sure the "Visual Studio 17 2022" generator is available
  (`cmake --help` lists installed generators).

### pnpm version mismatch

- The repository declares `"packageManager": "pnpm@11.6.0"` in `package.json`.
  If your globally installed `pnpm` differs, use `corepack enable` and
  `corepack prepare pnpm@11.6.0 --activate`, or install the exact version via
  `npm install -g pnpm@11.6.0`.
- Running `pnpm install` with a mismatched version may produce a different
  `pnpm-lock.yaml`; do not commit a lock file generated by the wrong version.

### E2E tests show `[SKIP]` — is that an error?

No. `[SKIP]` is the correct behavior when the engine env variable is not set.
The test suite still exits with code 0. To run the real tests, set
`NORVES_ENGINE_PATH` (mock) or `NORVES_NORVESLIB_ENGINE_PATH` (NorvesLib) to
an absolute path before running `cargo test --test process_e2e`.

---

## Generated Artifacts — Do Not Commit

The following directories and files are build outputs and must not be added to
version control (`.gitignore` already excludes them):

| Path | Description |
|---|---|
| `node_modules/` | JS package installs |
| `.pnpm-store/` | pnpm content-addressable store |
| `target/` | Rust bridge workspace build output |
| `apps/editor/src-tauri/target/` | Tauri/editor Cargo build output |
| `apps/editor/src-tauri/gen/` | Tauri generated schema / bindings |
| `build/` | CMake build trees (including `build/cpp/`) |
| `__pycache__/` | Python bytecode cache |

`pnpm-lock.yaml` and `Cargo.lock` **are** committed and must not be deleted.
