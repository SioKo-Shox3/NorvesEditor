# アーキテクチャ概要

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## プロジェクト概要

NorvesEditor は Tauri / Rust / TypeScript を中心にしたモダンなゲームエディターです。最初の大目標はアルファ版で、エンジンと接続し、Game View パネルから外部エンジンプロセスを起動・接続・制御できる状態を作ることです。

このリポジトリでは、旧構想の `NorvesBridge` を独立リポジトリではなく `bridge/` サブシステムとして扱います。

```text
NorvesEditor/
  apps/editor/          Tauri app 本体
  bridge/spec/          Bridge protocol schema / fixtures / docs
  bridge/crates/        Rust editor-side Bridge runtime / tools
  bridge/ts/            TypeScript UI-facing Bridge API
  bridge/cpp/           C++ engine-side SDK / mock engine
  bridge/tools/         Bridge inspector / debug tools
```

`NorvesLib` は最初の reference engine integration ですが、NorvesEditor の generic bridge / C++ SDK は NorvesLib に依存してはいけません。NorvesLib 固有の規約は NorvesLib adapter 内だけに閉じ込めます。

---

## アルファ版の到達点

アルファ版では以下を実現する。

```text
- NorvesEditor が Tauri desktop app として起動する。
- Game View パネルが存在する。
- Editor backend が外部 engine process を起動・停止・監視できる。
- Editor backend が C++ engine-side Bridge endpoint に WebSocket + JSON で接続できる。
- Editor UI が connection state / process state / runtime state / logs を表示できる。
- Game View パネルから Launch / Stop Process / Reconnect / Play / Pause / Stop / Focus Window を操作できる。
- Mock engine との接続が自動/手動テストで検証できる。
- 可能なら NorvesLib Game との reference connection を追加する。
```

アルファ版では native viewport embedding は行わない。Engine は外部ネイティブウィンドウを持ち、NorvesEditor の Game View はその起動・接続・制御・状態表示を担当する。

---

## ディレクトリ責務

```text
apps/editor/src/                  TypeScript / React UI
apps/editor/src-tauri/src/        Tauri Rust backend
bridge/spec/schema/               JSON Schema
bridge/spec/fixtures/             Golden protocol fixtures
bridge/spec/docs/                 Bridge protocol docs
bridge/crates/norves-bridge-core/ Rust protocol model / codec traits
bridge/crates/norves-bridge-editor-client/ Rust editor client runtime
bridge/crates/norves-bridge-tools/ Rust CLI / conformance tools
bridge/ts/packages/bridge-types/  TypeScript protocol/UI-facing types
bridge/ts/packages/bridge-ui/     Tauri command/event wrappers
bridge/cpp/engine-sdk/            Standalone C++ engine-side SDK
bridge/cpp/examples/mock-engine/  Mock C++ engine endpoint
bridge/tools/bridge-inspector/    Protocol/debug inspector
scripts/                          Build/test/dev scripts
```

---

## 全体アーキテクチャ規約

### Bridge は Editor 内部の別レイヤー

`bridge/` は NorvesEditor 内にあるが、UI と混ぜてはいけない。

```text
apps/editor:
  Editor UI / UX / workspace / Tauri shell

bridge:
  protocol / schema / transport / SDK / conformance / mock engine

NorvesLib adapter:
  NorvesLib-specific mapping only
```

禁止:

```text
- C++ engine SDK が Tauri / TypeScript / React に依存すること。
- Generic Bridge layer が NorvesLib headers を include すること。
- UI component が raw WebSocket protocol を直接扱うこと。
- Engine adapter が Editor UI state を知ること。
```

### Editor backend が接続状態を所有する

Alpha では Bridge connection は Tauri Rust backend が所有する。TypeScript frontend は Tauri command wrapper と event subscription を通じて状態を扱う。

```text
TypeScript UI
  -> Tauri invoke commands
Rust backend
  -> Bridge editor client runtime
  -> WebSocket + JSON
C++ Engine SDK
```

### 外部 viewport 方針

Alpha では engine viewport は外部ネイティブウィンドウ。Tauri 内への native viewport embedding、shared texture、frame streaming は post-alpha research とする。

Game View panel は以下だけを担当する。

```text
- launch / stop / reconnect
- runtime controls
- state display
- logs/status display
- focus/raise external window best effort
```
