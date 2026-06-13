# AGENTS.md

This file provides guidance to Codex and other coding agents when working in the NorvesEditor repository.

> **この文書は `CLAUDE.md` と完全に同一内容で運用する。** Claude Code は `CLAUDE.md`、Codex は `AGENTS.md` を読むが、中身は常に揃える。一方を編集したら必ずもう一方へ同じ変更を反映すること。外部ファイル（旧 `.github/copilot-instructions.md` 等）は参照しない。この文書が唯一の一次情報。

---

## コミュニケーション言語

- ユーザーへの提示・報告・説明・質問は、原則として日本語で行う。進捗報告、レビュー報告、計画提示、リスク説明、選択肢の提示などはすべて日本語を基本とする。
- 技術用語、API 名、型名、識別子、コマンド、ファイルパス、エラーメッセージ、コード断片などはそのままの表記（英語・原語）を維持する。無理に和訳しない。
- ユーザーが別の言語で明示的に依頼した場合は、その依頼に従う。
- コミットメッセージは「コミットメッセージ」節の規約（主に日本語）に従う。コード内コメントや docs などの成果物の言語は各レイヤーの既存方針・周辺コードに合わせる。

---

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

## ビルドと検証

実際のコマンドはリポジトリ初期化時に確定する。確定後はこの節を更新すること。

想定される標準コマンド:

```powershell
# TypeScript / Tauri frontend
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

ルートに `scripts/verify.ps1` または同等の検証スクリプトを作成したら、標準ゲートはそれを優先する。

```powershell
./scripts/verify.ps1
```

生成物、ビルドディレクトリ、Tauri generated schema、node_modules、Cargo target、CMake build 出力はコミットしない。

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

---

## Protocol / Schema 規約

- Bridge protocol の canonical debug format は JSON。
- すべての wire message は `bridge/spec/schema/` の JSON Schema と `bridge/spec/fixtures/` の fixture で表現する。
- 新しい message / method / event を追加する場合は、schema、positive fixture、必要に応じて negative fixture、Rust/TypeScript/C++ の検証を同じフェーズに含める。
- JSON-RPC 2.0 の request/response/notification の考え方は参考にするが、厳密準拠はしない。NorvesEditor 独自の role、session、capability、event、attachment を扱うため、NorvesEditor Bridge envelope を正とする。
- Protocol 互換性を壊す変更は ADR または明示的な protocol migration note を要求する。

---

## Memory / Buffer 規約

Editor 接続用なので小さい control message はコピーを許容する。ただし、後で最適化できない API にしてはいけない。

必須規則:

```text
- Engine live memory を transport に直接渡さない。
- Engine adapter は snapshot / DTO / serialized value に変換してから Bridge layer に渡す。
- Buffer ownership は API で明示する。
- Borrowed view は callback 中だけ有効とする。
- Owned buffer は送信完了または release まで有効とする。
- Large payload は size limit、queue limit、attachment/streaming 方針を明記する。
- Public API に third-party WebSocket buffer types を露出しない。
```

計画・レビューで必ず確認する項目:

```text
- 誰が buffer を所有するか。
- callback 後も生きるか。
- thread boundary を越えるか。
- queue に積まれる場合の最大サイズはあるか。
- failure / disconnect 時の release 経路はあるか。
- raw pointer / string_view / span の寿命は明示されているか。
```

---

## Rust 規約

対象:

```text
apps/editor/src-tauri/src/
bridge/crates/**
```

規則:

```text
- `cargo fmt` と `cargo clippy -- -D warnings` を通す。
- Library code で `unwrap()` / `expect()` を使わない。テスト、例、明確に失敗即 abort する初期化は例外だが理由をコメントする。
- Error type は library では `thiserror` 等で型を定義し、binary/tool では `anyhow` を使用可。
- Logging は `tracing` を使う。`println!` は CLI output、examples、tests 以外では避ける。
- Async task では blocking I/O を直接行わない。必要なら `spawn_blocking` または dedicated task に分離する。
- Tokio channel は原則 bounded。unbounded を使う場合は計画に理由を書く。
- Long-lived task は shutdown path を持つ。
- Tauri command 引数・戻り値・event payload は `serde` serialize/deserialize 可能な DTO にする。
- Frontend-facing DTO は `camelCase` を基本とする。Wire protocol は schema に従う。
- Panic が FFI / Tauri command / task boundary に漏れないようにする。
```

---

## TypeScript / UI 規約

対象:

```text
apps/editor/src/
bridge/ts/**
```

規則:

```text
- TypeScript strict mode を前提にする。
- `any` は禁止。必要な場合は `unknown` から明示的に narrow する。
- Transport state と UI state を混ぜない。
- UI から raw WebSocket を直接扱わない。alpha では Tauri command wrapper / event wrapper を使う。
- Tauri command 名・event 名は central module で定義する。
- DTO は schema/fixture と対応させる。
- React component は UI 表示に集中し、process/connection side effect は hook/service に分離する。
- Styling/theme token は中央管理する。inline style の乱用を避ける。
- Log/connection/runtime state は replay/debug しやすい store 構造にする。
```

---

## C++ Engine SDK 規約

対象:

```text
bridge/cpp/engine-sdk/**
bridge/cpp/examples/**
```

規則:

```text
- Standalone C++ SDK は NorvesLib に依存しない。
- C++20 minimum。C++23 は ADR または toolchain policy がある場合に使用可。
- CMake target を明確に分ける。
- Public headers で third-party WebSocket types を露出しない。
- Public API は RAII と明示的 ownership を基本にする。
- Engine adapter callbacks は thread affinity を明示する。
- Engine state mutation は engine/main thread へ marshal できる設計にする。
- Raw engine memory を protocol/transport に直接送らない。
- Allocator / buffer pool hook は optional にし、未指定なら default allocator で動作する。
- Tests/examples 以外で `std::cout` に依存した logging をしない。SDK logging は callback or sink interface にする。
```

C++ WebSocket backend は internal implementation detail。IXWebSocket、Boost.Beast、libwebsockets 等の候補から ADR で決定する。決定前に public API に特定ライブラリの型を混ぜない。

---

## NorvesLib adapter 規約

NorvesLib adapter を NorvesEditor repo に置く場合でも、Generic SDK と混ぜない。NorvesLib repository 側で実装する場合は NorvesLib の `AGENTS.md` に従う。

```text
Generic C++ Engine SDK:
  standard library allowed, NorvesLib dependency forbidden

NorvesLib adapter:
  NorvesLib-specific containers, Object/Resource/Thread/Memory rules follow NorvesLib repository policy
```

NorvesLib adapter の責務:

```text
- NorvesLib runtime/log/status を Bridge DTO に変換する。
- Bridge runtime commands を NorvesLib の安全な thread/context に marshal する。
- NorvesLib live object memory を直接 Bridge に渡さない。
- Bridge SDK public API を NorvesLib 内部型で汚染しない。
```

---

## Tauri / Security 規約

- Engine process launch は Rust backend が所有する。
- UI から任意コマンドを直接実行させない。
- Tauri shell/sidecar permissions は最小限にする。
- capabilities/default.json の permission 追加は計画レビュー必須。
- External binary / sidecar 設定を追加する場合は platform target triple と packaging impact を文書化する。
- Remote connection は alpha では無効。Default は localhost のみ。
- Secret / token / user-specific path はリポジトリにコミットしない。

---

## コーディングスタイル共通規約

- 新規テキストファイルは UTF-8。行末は LF を基本とする。既存ファイルを編集する場合は既存行末を不要に変更しない。
- 例外: NorvesLib repository 側で編集する C++ ファイルは NorvesLib 側の行末/BOM 規約に従う。
- Generated files は生成元と生成手順を明記する。手編集しない。
- Public API 変更には docs または ADR の更新を伴わせる。
- TODO を残す場合は owner/context を書く。曖昧な `TODO: fix` は不可。

---

## ブランチ運用

NorvesEditor では、`main` や単一の長期作業ブランチに直接作業を積み続けない。Workstream / Phase / 修正単位ごとにブランチを切り、レビュー可能な単位で統合する。

### 基本方針

```text
main:
  常にビルド可能な安定ブランチ。直接コミットしない。

integration branch:
  必要な場合だけ設ける短〜中期の統合ブランチ。
  develop 一本運用の代替として常用しない。

work branch:
  通常の作業ブランチ。実装・修正・文書化はここで行う。
```

非自明な作業では、調査・計画の後、実装に入る前に作業ブランチを作成する。小さな typo 修正や README の軽微な追記でも、リモート運用がある場合は原則として専用ブランチを使う。

### ブランチ命名

ブランチ名は英小文字の kebab-case を基本とし、作業種別と対象を含める。

```text
feature/<area>-<summary>
fix/<area>-<summary>
docs/<area>-<summary>
chore/<area>-<summary>
refactor/<area>-<summary>
spike/<area>-<summary>
```

例:

```text
feature/bridge-message-envelope
feature/editor-game-view-alpha-panel
fix/bridge-reconnect-timeout
docs/architecture-adr-bootstrap
chore/repository-foundation
spike/native-window-focus
```

Workstream に紐づく作業では、必要に応じて area に Workstream 名を反映する。

```text
feature/protocol-fixtures
feature/rust-bridge-client-runtime
feature/cpp-engine-sdk-skeleton
feature/tauri-process-lifecycle
```

### ブランチ作成・更新ルール

- 作業ブランチは最新の `main`、またはオーケストレーターが明示した統合ブランチから切る。
- 既存ブランチで別目的の作業を始めない。目的が変わったら新しいブランチを切る。
- 1 ブランチ 1 論理テーマを守る。複数 Workstream にまたがる変更は、計画レビューで分割方針を決める。
- 長期化したブランチは定期的に基点ブランチへ追従する。rebase / merge の選択は作業状況に応じて計画または PR に記録する。
- 他者または他エージェントが使っている共有ブランチを無断で rebase / reset / force-push しない。
- 個人・エージェント専用ブランチであっても force-push は `--force-with-lease` を使い、理由を記録する。
- `spike/` ブランチは検証用。採用する場合は、結果を整理して `feature/` ブランチへ移すか、明確なコミット列に整えてから統合する。

### PR / 統合ルール

GitHub を使う場合、作業ブランチは PR または Draft PR を通して統合する。

PR には最低限以下を書く。

```text
- 目的
- 対象 Workstream / Phase
- 変更内容
- 実行した検証コマンド
- 未解決リスク / 既知の制限
- UI 変更がある場合はスクリーンショットまたは確認ログ
- protocol / security / memory ownership / thread affinity に影響がある場合はその説明
```

統合前の最低条件:

```text
- 関連する format / lint / build / test が通っている。
- AGENTS.md と CLAUDE.md の差分がない。
- 生成物や build output が含まれていない。
- Workstream / Phase の Done criteria を満たしている。
- 必要なレビューが完了している。
```

### マルチエージェント時のブランチ規約

- 実装担当エージェントごとに作業ブランチまたは worktree を分ける。
- サブエージェント間で同じファイルを同時編集しない。共有ファイルは単一オーナーを決めるか、オーケストレーターが順次統合する。
- 計画レビュー・実装レビュー担当は原則 read-only で確認し、直接 push しない。
- オーケストレーターはフェーズ完了時に branch status、未コミット差分、検証結果、push 状態を確認する。
- 途中で別ブランチへ切り替える場合は、現在の差分を commit / stash / discard のどれで扱ったかを明示する。

### 禁止事項

```text
- `main` への直接コミット。
- `develop` 一本に全作業を積み続ける運用。
- 目的の違う変更を同じブランチに混ぜること。
- レビュー前の大規模一括統合。
- 他者の未統合コミットを無断で rewrite すること。
- build output / generated noise を含んだまま PR すること。
```


## コミットメッセージ

「正しいコミットメッセージとは何か」を毎回考えて書く。テンプレ的な一文で済ませない。

- **言語**: 主に日本語。命令形で「何をしたか」を書く。
- **粒度**: 1 コミット 1 論理変更。無関係な変更を混ぜない。
- **件名**: 対象と意図を具体的に表す。曖昧語（「修正」「更新」単体）で終わらせない。
- **本文**: 自明でない変更には本文を付け、なぜ必要か、何をどう変えたか、影響範囲を書く。
- 本文必須の変更:
  - protocol schema / fixture
  - Tauri security permissions
  - process launch / kill behavior
  - Bridge public API
  - C++ SDK ownership / buffer / thread rules
  - Rust async task lifecycle
  - NorvesLib adapter
  - viewport strategy
- オーケストレーション作業ではコミットを工程境界に合わせる。
- リモート設定が許せばコミット後に現在の作業ブランチをプッシュする。main / develop への直接 push は原則行わない。

---

## マルチエージェント・オーケストレーション

非自明な作業はメインエージェント（オーケストレーター）が工程を分割し、サブエージェントに割り当てて進める。

### 役割分担の原則

- **オーケストレーター**が担うもの: 工程分割、設計判断、順序付け、スコープ割り当て、サブエージェント結果の統合、検証実行、ブランチ/コミット境界管理、最終受け入れ。
- オーケストレーター自身は **具体的なフェーズ計画の作成・ファイル編集・自分が監督した実装のレビューを行わない**。これらはサブエージェントに割り当てる。
- **実装担当とレビュー担当は必ず別エージェント**にする。
- 各工程は独立してレビュー・検証・コミットできる最小単位にする。
- 各工程は原則として専用ブランチまたは明確にスコープ分離された stacked branch で進める。
- 先行する依存フェーズが実装レビューと検証ゲートを通過するまで、後続フェーズの実装を始めない。

### 工程の流れ

1. **調査** — 既存コード・依存・命名規約の把握。読み取り専用。
2. **計画** — フェーズごとに具体的な実装計画を書く。
3. **計画レビュー** — API 境界、依存方向、所有権、寿命、スレッド安全性、security permission、protocol compatibility、検証コマンドを確認。
4. **実装** — 承認された計画に沿って編集。
5. **実装レビュー** — 実 diff を承認済み計画と突き合わせる。
6. **統合・検証・コミット** — 関連検証を実行し、ブランチ方針とコミット境界に沿ってコミットする。

### 計画に必ず含める項目

```text
- フェーズの目的と期待される挙動変化。
- 影響するモジュール・公開 API・具体的なファイル/ディレクトリ。
- 実装方針。
- 所有権/寿命/スレッド/async task/permission の前提。
- Protocol schema/fixture 変更の有無。
- base branch / 作業ブランチ名 / stacked branch 依存関係。
- 走らせる検証コマンド。
- リスクレベルと封じ込め方針。
- コミット可能になる条件。
```

### 実装レビューで必ず見る項目

```text
- 計画との差分。
- Public API 形状。
- Protocol schema/fixture/test の整合。
- Buffer ownership / thread affinity / async shutdown。
- Tauri permission scope。
- UI state と transport/backend state の分離。
- C++ SDK が NorvesLib や third-party WebSocket types を漏らしていないか。
- Generated/build files が混ざっていないか。
- 作業ブランチのスコープが計画と一致しているか。
- main / develop に直接作業を積んでいないか。
- 検証コマンドの実行結果。
```

---

## Workstream 運用

Workstream は実装単位ではなく、計画領域です。作業開始時は該当 Workstream をさらに小さい Phase に分割する。

現在の Workstream:

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

Workstream をまたぐ変更は原則として計画レビュー必須。

---

## モデル選定

判断の質が後工程全体を左右する役割（計画・レビュー＝品質保証）に、その時点で利用可能な最上位モデルを使い、手を動かす分割タスクでコストを下げる。

| 役割 | モデル方針 |
|------|-----------|
| オーケストレーター | 最上位〜上位。判断の質を優先。 |
| 計画作成 | 必ず最上位。 |
| 計画レビュー | 必ず最上位。 |
| 実装レビュー | 必ず最上位。 |
| 実装 | 既定は下位。難所では上位〜最上位へ昇格。 |
| 調査・機械的作業 | 最安で十分。設計理解を伴う調査は中位以上。 |

昇格条件:

```text
- Protocol schema / compatibility
- Tauri process/security permissions
- Rust async task lifecycle
- WebSocket transport / reconnect
- C++ SDK public API
- Buffer ownership / memory policy
- Thread affinity / engine main-thread marshaling
- NorvesLib adapter
- Viewport strategy
```

---

## MCP / 外部参照

- ライブラリ/フレームワーク/API/ツールのドキュメントは Context7 MCP を最初に使う。より広い調査・最新リリースノート・ベンダーページ・標準仕様はウェブ検索を使う。
- 技術的主張は一次情報（公式ドキュメント、仕様、ベンダーのリリースノート、ソースリポジトリ、標準文書）を優先する。
- GitHub の状態に依存するタスクは GitHub MCP / 連携アプリの情報を優先する。
- MCP の認証情報はリポジトリに置かない。ユーザーレベルの環境変数かホスト管理のプロンプトを使う。

---

## Alpha acceptance checklist

Alpha 作業の最終確認:

```text
- AGENTS.md と CLAUDE.md が一致している。
- Tauri app が起動する。
- Mock engine がビルドできる。
- Editor から mock engine を launch できる。
- Editor backend が WebSocket + JSON で接続できる。
- Game View panel が process/connection/runtime state を表示する。
- Launch / Stop Process / Reconnect / Play / Pause / Stop が操作できる。
- Log panel が log.message event を表示する。
- Protocol fixtures/conformance tests が通る。
- Tauri security permission が最小化されている。
- Known limitations と post-alpha viewport plan が docs にある。
```
