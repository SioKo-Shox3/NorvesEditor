# NorvesEditor

NorvesEditor は、C++ ゲームエンジンと接続して利用することを前提にした、モダンなデスクトップゲームエディターです。

最初の大目標は、完全なシーンエディターではなく、**エンジンを起動・接続・制御できるアルファ版**です。NorvesEditor から外部エンジンプロセスを起動し、Bridge 接続を確立し、Game View パネルからゲーム実行状態を操作できるところまでを最初の成功ラインにします。

> Status: alpha

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
  - NorvesEditor Bridge-compatible endpoint
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
2. NORVES_ENGINE_PATH でエンジン実行ファイルを指定する。
3. Game View パネルから engine process を起動する。
4. Engine が local Bridge endpoint を公開する。
5. NorvesEditor が WebSocket + JSON で接続する。
6. Editor が hello / capabilities / status / log events を受信する。
7. Game View パネルに process state / connection state / runtime state を表示する。
8. Launch / Stop Process / Reconnect / Play / Pause / Stop / Focus Window を操作できる。
```

Alpha における "viewport" は、Tauri window 内に Vulkan / DirectX の native viewport を埋め込むものではありません。Engine は外部ネイティブウィンドウを持ち、NorvesEditor の Game View パネルは、その起動・接続・制御・状態表示を担当します。

Native viewport embedding、shared GPU texture、frame streaming、docked render target composition は post-alpha とします。

---

## Alpha Quick Start

### 前提ツール

| ツール | バージョン | 備考 |
|---|---|---|
| Rust (rustup) | stable | `rustup update stable` |
| Node.js + pnpm | pnpm 11.6.0 | `npm install -g pnpm@11.6.0` |
| CMake | 3.21+ | |
| Visual Studio 2022 | MSVC ツールセット付き | C++ engine SDK のビルドに必要 |
| Python 3 + jsonschema | — | `pip install jsonschema` |
| Vulkan SDK | — | **NorvesLib を使う場合のみ**。mock engine では不要 |

### ステップ 1: JS 依存のインストール

```powershell
# リポジトリルートで実行
pnpm install
```

### ステップ 2: C++ mock engine のビルド

初回 configure では libwebsockets (v4.3.3) を FetchContent でダウンロードするため、ネットワーク接続が必要です。

```powershell
# configure
cmake -S bridge/cpp -B build/cpp -G "Visual Studio 17 2022"

# build
cmake --build build/cpp --config Debug
```

成果物: `build/cpp/examples/mock-engine/Debug/norves_mock_engine.exe`

### ステップ 3: エディターの開発起動

```powershell
cd apps/editor
pnpm tauri dev
```

このコマンドは `tauri.conf.json` の `beforeDevCommand` によって Vite dev server（localhost:1420）を先に起動し、その後 Tauri ウィンドウを開きます。

> **注意:** `pnpm tauri dev` の動作は実機での最終確認を推奨します。ビルド環境・ドライバ構成によって挙動が異なる場合があります。

### ステップ 4: Game View からエンジンを起動して動作確認

```powershell
# PowerShell で環境変数を設定し、mock engine の絶対パスを指定する
$env:NORVES_ENGINE_PATH = "C:\path\to\NorvesEditor\build\cpp\examples\mock-engine\Debug\norves_mock_engine.exe"

# その後 pnpm tauri dev を実行（またはすでに起動中のターミナルと同じセッションで起動）
```

エディターが起動したら、Game View パネルから **Launch** を押してエンジンを起動します。接続後、status / log / runtime 制御（Play / Pause / Stop / Focus Window）が使えます。

エンジンパスの Settings UI は alpha 未実装です。`NORVES_ENGINE_PATH` 環境変数での指定が唯一の方法です。

---

## Verification

集約ランナーを使うのが最も簡単です。

```powershell
# protocol fixtures + Rust（C++ 未構成でも通る）
./scripts/verify.ps1

# C++ も含めて検証（build/cpp が cmake configure 済みの場合のみ）
./scripts/verify.ps1 -Cpp
```

個別ゲートの詳細は [`docs/agent-guide/build-and-verify.md`](docs/agent-guide/build-and-verify.md) を参照してください。

生成物（`node_modules/`、Cargo `target/`、CMake build ディレクトリ、Tauri 生成ファイル、`__pycache__`）はコミットしないでください。`pnpm-lock.yaml` と `Cargo.lock` はコミット対象です。

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

---

## Repository Layout

```text
NorvesEditor/
  README.md
  AGENTS.md
  CLAUDE.md

  docs/
    vision.md
    alpha-project-plan.md
    alpha-scope.md
    architecture.md
    technology-decisions.md
    engine-integration.md
    viewport-strategy.md
    memory-buffer-policy.md
    agent-guide/
      build-and-verify.md
      norveslib-adapter.md
      rust.md   (他のレイヤーガイド)
    adr/

  apps/
    editor/
      src/                 # TypeScript frontend
      src-tauri/           # Tauri Rust backend（独立 Cargo workspace）

  bridge/
    spec/
      schema/              # JSON Schema
      fixtures/            # Golden protocol fixtures
      docs/                # Bridge protocol docs

    crates/
      norves-bridge-core/
      norves-bridge-editor-client/
      norves-bridge-tools/
      norves-bridge-dump/

    ts/
      packages/
        bridge-types/
        bridge-ui/

    cpp/
      engine-sdk/
      examples/
        mock-engine/

  scripts/
    verify.ps1             # 集約ゲートランナー
    validate-bridge-fixtures.py
    check-protocol-names.mjs
```

`bridge/` は NorvesEditor リポジトリ内のサブシステムです。UI と混ぜず、protocol / transport / SDK / conformance / mock engine を扱う別レイヤーとして管理します。

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
│    - runtime status / logs                   │
│    - runtime commands                         │
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

TypeScript UI は raw WebSocket state を持ちません。Tauri Rust backend がエンジンプロセス状態と Bridge 接続状態を所有します。C++ engine SDK は Tauri、React、TypeScript、NorvesLib に依存してはいけません。

---

## Technology Stack

```text
Application shell:
  Tauri 2

Editor backend:
  Rust / Tokio / serde / tracing

Editor frontend:
  TypeScript / Vite / React

Bridge control channel:
  WebSocket + JSON text messages (JSON-RPC inspired envelope)

Protocol validation:
  JSON Schema + golden fixtures

Engine SDK:
  C++20 / CMake（standalone from NorvesLib）
```

---

## Known Limitations

Alpha の既知の制限事項（詳細は各ドキュメントを参照）:

1. **Native viewport 非対応** — エンジンが外部ネイティブウィンドウを持つ。Tauri 内への埋め込みは post-alpha（[`docs/viewport-strategy.md`](docs/viewport-strategy.md)）。
2. **scene/object/schema 系は alpha 対象外** — `scene.getTree` / `object.*` / `schema.*` は `not_supported`（[`docs/alpha-project-plan.md`](docs/alpha-project-plan.md)）。
3. **エンジンパスの Settings UI 未実装** — `NORVES_ENGINE_PATH` 環境変数のみ。
4. **log フィルタ未対応** — サーバー側フィルタなし、クライアント側フィルタのみ、単一購読のみ。
5. **オーファンリスク** — エディター強制終了時のエンジンプロセス残留（Windows Job Object は post-alpha）。
6. **NorvesLib は Windows + Vulkan SDK 必須** — mock engine は Vulkan 不要。
7. **C++ configure 初回はネットワーク必要** — libwebsockets を FetchContent で取得（v4.3.3）。
8. **localhost 専用** — `ws://127.0.0.1` のみ。remote エンドポイントは未対応。

---

## Documentation Map

| ドキュメント | 内容 |
|---|---|
| [`docs/alpha-project-plan.md`](docs/alpha-project-plan.md) | Alpha 全体計画・Workstream 一覧 |
| [`docs/engine-integration.md`](docs/engine-integration.md) | 接続契約（Connection Contract）・Launch Sequence・検証済みメソッド/イベント一覧 |
| [`docs/build.md`](docs/build.md) | ビルド・検証手順（人間向け quick build）と troubleshooting |
| [`docs/engine-profile.md`](docs/engine-profile.md) | エンジン実行ファイルのパス解決と env（`NORVES_ENGINE_PATH`） |
| [`docs/protocol-debugging.md`](docs/protocol-debugging.md) | Bridge プロトコルのデバッグ（schema/fixtures・bridge-dump・ログの見方） |
| [`docs/norveslib-integration.md`](docs/norveslib-integration.md) | NorvesLib を参照エンジンとして接続・ビルドする手順 |
| [`docs/viewport-strategy.md`](docs/viewport-strategy.md) | Viewport 戦略（alpha: 外部ウィンドウ、post-alpha: 埋め込み研究） |
| [`docs/architecture.md`](docs/architecture.md) | アーキテクチャ詳細 |
| [`docs/memory-buffer-policy.md`](docs/memory-buffer-policy.md) | メモリ・バッファ所有権ポリシー |
| [`docs/agent-guide/build-and-verify.md`](docs/agent-guide/build-and-verify.md) | ビルド・検証ゲートの詳細規約 |
| [`docs/agent-guide/norveslib-adapter.md`](docs/agent-guide/norveslib-adapter.md) | NorvesLib adapter 規約 |
| [`docs/agent-guide/README.md`](docs/agent-guide/README.md) | agent-guide インデックス |
| [`docs/adr/`](docs/adr/) | Architecture Decision Records |

---

## Alpha Workstreams

Alpha development is organized into workstreams:

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

詳細: [`docs/alpha-project-plan.md`](docs/alpha-project-plan.md)

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

## Development Workflow

このプロジェクトは coding agents（Codex / Claude Code）を使って実装されています。変更を加える前に `AGENTS.md`（または `CLAUDE.md`）を読んでください。

ワークフロー: Investigation → Planning → Plan Review → Implementation → Implementation Review → Integration / Verification / Commit

詳細: [`docs/agent-guide/orchestration.md`](docs/agent-guide/orchestration.md)

---

## Contributing

アーキテクチャ境界を守ってください:

```text
- Bridge protocol and SDK must not be mixed into UI components.
- TypeScript UI must not own raw WebSocket transport state.
- Rust backend owns process and connection lifecycle.
- C++ engine SDK must remain standalone from NorvesLib.
- NorvesLib integration must be an adapter, not a hard dependency.
- Memory and buffer ownership must be explicit.
```

非自明な変更を加える前に、関連する plan と ADR を作成・更新してください。

---

## License

This repository currently uses the Apache License 2.0. See `LICENSE`.
Confirm project copyright and NOTICE metadata before public release.
