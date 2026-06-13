# NorvesEditor

NorvesEditor は、C++ ゲームエンジンと接続して利用することを前提にした、モダンなデスクトップゲームエディターです。

最初の大目標は、完全なシーンエディターではなく、**エンジンを起動・接続・制御できるアルファ版**です。NorvesEditor から外部エンジンプロセスを起動し、Bridge 接続を確立し、Game View パネルからゲーム実行状態を操作できるところまでを最初の成功ラインにします。

> Status: planning / pre-alpha

---

## Vision

NorvesEditor は、ゲームエンジン本体とエディター UI を強く結合させず、Bridge 経由で接続する開発環境を目指します。

```text
NorvesEditor
  - Tauri desktop editor
  - Rust backend
  - TypeScript frontend
  - Bridge subsystem

C++ Engine
  - NorvesBridge-compatible endpoint
  - scene/runtime/log/status provider
  - external native game viewport
```

NorvesLib は最初の reference engine integration になりますが、NorvesEditor は NorvesLib 専用 UI にはしません。Engine 側との接続境界は C++ engine-side SDK と Bridge protocol に置きます。

---

## Alpha Goal

Alpha の目標は、NorvesEditor から C++ エンジンを起動し、接続し、実行状態を確認・操作できることです。

Minimum happy path:

```text
1. NorvesEditor を起動する。
2. workspace / engine executable を設定する。
3. Game View パネルから engine process を起動する。
4. Engine が local Bridge endpoint を公開する。
5. NorvesEditor が WebSocket + JSON で接続する。
6. Editor が hello / capabilities / status / log events を受信する。
7. Game View パネルに process state / connection state / runtime state を表示する。
8. Launch / Stop Process / Reconnect / Play / Pause / Stop / Focus Window を操作できる。
```

Alpha における “viewport” は、Tauri window 内に Vulkan / DirectX の native viewport を埋め込むものではありません。Engine は外部ネイティブウィンドウを持ち、NorvesEditor の Game View パネルは、その起動・接続・制御・状態表示を担当します。

Native viewport embedding、shared GPU texture、frame streaming、docked render target composition は post-alpha research とします。

---

## Non-Goals for Alpha

Alpha では、以下を対象外にします。

```text
- 完全な scene hierarchy editor
- 全 component / property を扱う inspector
- asset import pipeline
- undo / redo transaction system
- native viewport embedding
- frame streaming
- graph editor / shader graph / visual scripting
- multiplayer / online runtime networking
- Norves-gRPC integration
- UDP telemetry channel
- general-purpose public bridge standardization
```

Bridge subsystem は将来的に切り出せる境界を保ちますが、Alpha では独立 repository にはしません。

---

## Repository Layout

Recommended initial tree:

```text
NorvesEditor/
  README.md
  AGENTS.md
  CLAUDE.md

  docs/
    vision.md
    alpha-scope.md
    architecture.md
    technology-decisions.md
    engine-integration.md
    viewport-strategy.md
    memory-buffer-policy.md
    adr/
      0001-editor-owned-bridge-subsystem.md
      0002-tauri-rust-editor-backend.md
      0003-websocket-json-bridge-control-channel.md
      0004-cpp-engine-sdk.md
      0005-external-engine-viewport-for-alpha.md

  apps/
    editor/
      src/                 # TypeScript frontend
      src-tauri/           # Tauri Rust backend

  bridge/
    spec/
      schema/              # JSON Schema
      fixtures/            # Golden protocol fixtures
      docs/                # Bridge protocol docs

    crates/
      norves-bridge-core/
      norves-bridge-editor-client/
      norves-bridge-tools/

    ts/
      packages/
        bridge-types/
        bridge-ui/

    cpp/
      engine-sdk/
      examples/
        mock-engine/

    tools/
      bridge-inspector/

    conformance/
      fixtures/
      runners/

  scripts/
  tests/
```

`bridge/` は NorvesEditor repository 内のサブシステムです。UI と混ぜず、protocol / transport / SDK / conformance / mock engine を扱う別レイヤーとして管理します。

---

## Architecture

Alpha architecture:

```text
┌──────────────────────────────────────────────┐
│ NorvesEditor                                 │
│                                              │
│  TypeScript Frontend                         │
│    - Game View panel                         │
│    - Log panel                               │
│    - Connection state                        │
│    - Workspace/settings UI                   │
│        │                                     │
│        │ Tauri commands / events             │
│        ▼                                     │
│  Rust Backend                                │
│    - engine process lifecycle                │
│    - Bridge client runtime                   │
│    - reconnect / heartbeat / session state   │
│    - event routing to frontend               │
└──────────────────┬───────────────────────────┘
                   │ WebSocket + JSON
                   ▼
┌──────────────────────────────────────────────┐
│ C++ Engine Process                            │
│                                              │
│  Bridge Engine SDK                            │
│    - WebSocket endpoint                       │
│    - request dispatch                         │
│    - response/event emission                  │
│        │                                     │
│        ▼                                     │
│  Engine Adapter                               │
│    - capabilities                             │
│    - runtime status                           │
│    - logs                                     │
│    - runtime commands                         │
│    - optional scene/object data later         │
│                                              │
│  External Native Viewport Window              │
└──────────────────────────────────────────────┘
```

### Key boundaries

```text
apps/editor:
  Editor UI, workspace UX, Tauri shell integration.

bridge/spec:
  Wire protocol, JSON Schema, fixtures, protocol docs.

bridge/crates:
  Rust editor-side Bridge runtime and tools.

bridge/ts:
  TypeScript UI-facing types, command wrappers, event helpers.

bridge/cpp:
  Standalone C++ engine-side SDK and mock engine.

NorvesLib adapter:
  NorvesLib-specific mapping. Not part of the generic bridge SDK.
```

The TypeScript UI must not own raw WebSocket state. The Tauri Rust backend owns engine process state and Bridge connection state. The C++ engine SDK must not depend on Tauri, React, TypeScript, or NorvesLib internals.

---

## Technology Stack

Planned Alpha stack:

```text
Application shell:
  Tauri 2

Editor backend:
  Rust
  Tokio
  serde
  tracing

Editor frontend:
  TypeScript
  Vite
  React by default unless an ADR changes it

Bridge control channel:
  WebSocket
  JSON text messages
  JSON-RPC inspired envelope

Protocol validation:
  JSON Schema
  golden fixtures

Engine SDK:
  C++20 or C++23
  CMake
  standalone from NorvesLib
```

### Why Tauri

NorvesEditor needs a modern UI, native process/filesystem integration, and a backend that can safely own long-lived engine connections. Tauri gives the editor a web-based UI surface while keeping process management and Bridge runtime in Rust.

### Why Rust on the Editor side

The Editor backend owns async process lifecycle, connection state, reconnection, heartbeat, event fan-out, and file/workspace operations. Rust and Tokio are a good fit for these long-lived, stateful, asynchronous tasks.

### Why C++ on the Engine side

Most target engines, including NorvesLib, are expected to have C++ cores. The engine-side SDK should be easy to embed into a C++ engine without requiring Rust runtime, Cargo integration, or FFI at the engine boundary.

### Why WebSocket + JSON first

Alpha prioritizes visibility, debuggability, and iteration speed. WebSocket + JSON lets the Editor and Engine exchange reliable request / response / event messages while keeping the wire format easy to inspect and validate.

Binary codecs, Protobuf, UDP telemetry, shared memory, and frame streaming are post-alpha topics.

---

## Bridge Protocol Direction

The Bridge protocol is JSON-RPC inspired but not strict JSON-RPC 2.0.

Every wire message should be represented by:

```text
- JSON Schema
- positive golden fixture
- negative fixture when validation behavior matters
- Rust test
- TypeScript test or type validation
- C++ test when engine-side handling is involved
```

Example request shape:

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

Example event shape:

```json
{
  "bridge": "norves.editor.bridge",
  "version": "0.1",
  "kind": "event",
  "event": "engine.log",
  "params": {
    "level": "info",
    "message": "Engine started"
  }
}
```

Alpha method/event candidates:

```text
bridge.hello
bridge.getCapabilities
engine.status
engine.log
runtime.play
runtime.pause
runtime.stop
runtime.getState
process.started
process.exited
connection.stateChanged
```

Scene hierarchy, object snapshots, inspector editing, and asset browser integration are intentionally later work unless they become necessary for the alpha vertical slice.

---

## Memory and Buffer Policy

Editor connection traffic is not expected to be game-runtime critical, so small control messages may be copied. However, APIs must not make later optimization impossible.

Rules:

```text
- Do not expose live engine memory to the transport layer.
- Engine adapters must convert engine state into snapshots, DTOs, or serialized values.
- Buffer ownership must be explicit in public APIs.
- Borrowed views are valid only for the documented callback scope.
- Owned buffers remain valid until send completion, release, or explicit drop.
- Large payload paths must define size limits and queue limits.
- Large binary payloads should use attachments, streaming, file references, or later transport-specific mechanisms.
```

Alpha does not optimize for zero-copy transport. It does optimize for safe ownership boundaries.

---

## Development Workflow

This project is expected to be implemented with coding agents such as Codex. Read `AGENTS.md` before making changes.

High-level workflow:

```text
1. Investigation
2. Planning
3. Plan Review
4. Implementation
5. Implementation Review
6. Integration / Verification / Commit
```

Non-trivial tasks should be split into reviewable phases. Implementation and review should be performed by separate agents when using multi-agent orchestration.

`AGENTS.md` and `CLAUDE.md` must be kept identical. If one is changed, apply the same change to the other.

---

## Build and Verification

The exact commands will be finalized when the repository is initialized. The expected shape is:

```powershell
# Frontend / Tauri app
pnpm install
pnpm --filter @norves/editor dev
pnpm --filter @norves/editor build
pnpm lint
pnpm test

# Rust workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# C++ engine SDK / mock engine
cmake -S bridge/cpp -B build/cpp -G "Visual Studio 17 2022"
cmake --build build/cpp --config Debug
ctest --test-dir build/cpp -C Debug --output-on-failure
```

Once available, the preferred full verification command should be:

```powershell
./scripts/verify.ps1
```

Generated outputs such as `node_modules/`, Cargo `target/`, CMake build directories, Tauri generated files, logs, and packaged binaries must not be committed.

---

## Alpha Workstreams

Alpha development is organized into workstreams. Workstreams are planning areas, not necessarily single implementation tasks.

```text
A. Repository foundation
B. Product architecture and ADRs
C. Protocol specification and fixtures
D. Rust bridge core and editor client runtime
E. TypeScript UI-facing bridge package
F. C++ engine-side SDK
G. WebSocket transport
H. Mock engine and conformance tools
I. Tauri app shell
J. Engine process lifecycle and external viewport control
K. Game View alpha panel
L. NorvesLib reference adapter
M. Documentation and developer experience
N. Post-alpha foundations
```

Recommended order:

```text
repository foundation
→ architecture docs / ADRs
→ bridge protocol fixtures
→ Rust editor client runtime
→ C++ engine SDK and mock engine
→ WebSocket connection
→ Tauri app shell
→ engine process lifecycle
→ Game View panel
→ NorvesLib adapter
```

---

## Relationship to Other Norves Projects

```text
NorvesEditor:
  Desktop editor and Bridge subsystem.

NorvesLib:
  C++ game engine and first reference engine integration.
  NorvesLib-specific code must live in a NorvesLib adapter.

Norves-gRPC / NorvesOnline:
  Runtime online/multiplayer/backend service foundation.
  Not part of the NorvesEditor alpha Bridge path.
```

---

## Contributing

This repository is in early planning. Contributions should preserve the core boundaries:

```text
- Bridge protocol and SDK must not be mixed into UI components.
- TypeScript UI must not own raw WebSocket transport state.
- Rust backend owns process and connection lifecycle.
- C++ engine SDK must remain standalone from NorvesLib.
- NorvesLib integration must be an adapter, not a hard dependency.
- Memory and buffer ownership must be explicit.
```

Before implementing non-trivial changes, create or update the relevant plan and ADR.

---

## License

License is not finalized yet. Add the final license before public release.
