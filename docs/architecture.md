# NorvesEditor Architecture

Status: planning / pre-alpha

This document summarizes the initial architecture for the NorvesEditor alpha. The detailed project plan lives in `docs/alpha-project-plan.md`; agent operating rules live in `AGENTS.md` and `CLAUDE.md`.

## Alpha Shape

NorvesEditor is a Tauri desktop editor with a Rust backend and TypeScript frontend. The alpha goal is a narrow vertical slice: launch or attach to a C++ engine process, connect through the Bridge protocol, display engine status/logs, and control runtime state from a Game View panel.

The alpha Game View is not an embedded native GPU viewport. The engine owns an external native viewport window. NorvesEditor controls and reflects that external viewport through process management and Bridge messages.

## Layer Boundaries

E0〜E3 の承認済み設計は ADR 0010 / 0011 と `mcp-and-edit-layer-requirements.md` に記録する。編集サービスと MCP サーバーは Rust バックエンド内の層であり、次の所有権と境界を持つ。

| 層 | 所有するもの | 境界 |
| --- | --- | --- |
| Rust の編集サービス | UI と MCP からの編集・取り消し・やり直し・実行制御の有界な共通列、履歴、接続世代に結び付いた適用状態、適用結果と履歴要約のイベント | Bridge の I/O 中に状態ロックを保持しない。Bridge 接続そのものはバックエンドが所有する |
| Rust の MCP サーバー | loopback の Streamable HTTP、Bearer トークン認証、Origin/Host 検証、能力に基づく道具一覧、入力・結果検証、許可・確認、操作記録 | 読み取りは既存の Bridge 照会を使い、書き込み・undo/redo・実行制御は編集サービスを使う。プロセス操作・エンジン設定を公開しない |

```text
画面の編集・実行制御 → Tauri コマンド → 編集サービス ──────┐
MCP → loopback サーバー → 全要求の Bearer/Host 検証・Origin は存在時に照合
  ├→ 読み取り道具 → 既存の Bridge 照会 ──────────────────┤
  └→ 書き込み・undo/redo・実行制御の道具 → 許可（必要な操作は確認） → 編集サービス ─┤
                                                          └→ Bridge クライアント → エンジン
編集サービス → 適用結果・履歴要約 → Tauri イベント → 画面
```

MCP は既定で無効、127.0.0.1 のみ、既定ポート49770。書き込みは読み取りのみから始める。
適用結果の通知は編集サービスが所有し、エンジンの best-effort イベントに依存しない。
秘密はアプリ設定ディレクトリ、操作記録はアプリログディレクトリに置く。

| 層 | 所有するもの | 所有しないもの・境界 |
| --- | --- | --- |
| `apps/editor/src` TypeScript UI | パネル、表示状態、編集履歴の要約表示 | 生の WebSocket、エンジンプロセス、正本の編集履歴、エンジンのライブメモリ |
| `apps/editor/src-tauri` Rust バックエンド | エンジンプロセス、Bridge 接続と再接続、接続世代、Tauri コマンド登録、画面へのイベント配信 | C++ SDK の公開 API、NorvesLib 内部、UI 描画 |
| Rust 編集サービス | UI と MCP の編集・undo/redo・実行制御の受付順、有界列、正本の履歴、部分失敗の保留、適用結果と履歴要約のイベント | MCP 固有の HTTP・認証・許可。Bridge I/O の await 中の状態ロック |
| Rust MCP サーバー | loopback MCP 通信、認証、Origin/Host 検証、道具とスキーマ、許可・確認、操作記録 | エンジンプロセスの起動・終了、エンジン設定、編集サービスを迂回する書き込み |
| `bridge/spec` | 通信プロトコル、JSON Schema、fixture、プロトコル文書 | 製品 UI の状態、プロセス管理 |
| `bridge/crates` | Rust のプロトコル型と codec、エディタ側 Bridge runtime、ツール | Tauri UI、C++ エンジンアダプタ |
| `bridge/ts` | TypeScript DTO、Tauri コマンドラッパー、画面向けイベント補助 | 生の通信 socket、プロセス管理 |
| `bridge/cpp/engine-sdk` | 独立した C++ エンジン側 SDK の境界 | Tauri、React、TypeScript、NorvesLib 固有型 |
| NorvesLib adapter | NorvesLib 固有の Bridge DTO 変換 | 汎用 Bridge SDK の振る舞い |

## Connection Flow

```text
TypeScript 画面
  -> 型付き Tauri コマンドラッパー -> Rust バックエンド
     ├-> 編集・undo/redo・実行制御 -> Rust 編集サービス
     └-> エンジンプロセスの起動・停止、Bridge 接続・再接続、状態照会
MCP クライアント
  -> loopback MCP サーバー -> 全要求の Bearer/Host 検証・Origin は存在時に照合
     ├-> 読み取り道具 -> 既存の Bridge 照会
     └-> 書き込み・undo/redo・実行制御 -> 許可（必要な操作は確認） -> Rust 編集サービス
Rust 編集サービス ──────────┐
既存の Bridge 照会 ─────────┴-> Rust Bridge エディタクライアント runtime
  -> WebSocket + JSON -> C++ エンジンプロセス
C++ エンジンプロセス
  -> 独立した Bridge エンジン SDK
  -> エンジンアダプタ
```

Tauri Rust バックエンドがエンジンプロセスと Bridge 接続の状態を所有する。画面はコマンドとイベントで状態を観測し、生の WebSocket 状態を持たない。MCP は全要求で Bearer と Host を検証し、Origin は存在する場合に照合する。読み取りは既存の Bridge 照会を使い、書き込みは許可を通し、確認が必要な操作では確認を経て画面と同じ編集サービスを使う。

## Bridge Subsystem

`bridge/` is a subsystem inside this repository, not an independent repository for the alpha. It contains protocol, schema, fixtures, editor-side runtime, TypeScript wrappers, engine-side SDK, mock engine, tools, and conformance tests.

The subsystem must stay generic enough that `NorvesLib` is only one reference adapter. Generic Bridge code must not include NorvesLib headers, expose NorvesLib types, or encode NorvesLib-only assumptions.

## Protocol Direction

The alpha control channel is WebSocket + JSON. Messages use a NorvesEditor Bridge envelope inspired by JSON-RPC request/response/event patterns, but the project envelope is canonical.

Every protocol addition should update:

```text
- JSON Schema under bridge/spec/schema/
- positive fixture under bridge/spec/fixtures/
- negative fixture when validation behavior matters
- Rust/TypeScript/C++ tests or conformance checks for affected layers
```

## Memory And Buffer Ownership

Small control messages may be copied. APIs must still preserve explicit ownership and lifetimes.

Required constraints:

```text
- Engine live memory is never passed directly to transport.
- Engine adapters convert state into snapshots, DTOs, or serialized values first.
- Borrowed views are valid only for the documented callback scope.
- Owned buffers remain valid until send completion, release, or drop.
- Large payload paths require size limits, queue limits, and attachment/streaming policy.
- Public SDK APIs do not expose third-party WebSocket buffer types.
```

## Initial Review Checklist

Use this checklist before approving alpha plans:

```text
- NorvesEditor and bridge subsystem boundaries are explicit.
- Bridge remains generic and is not NorvesLib-specific.
- C++ engine-side SDK and Tauri Rust editor-side runtime responsibilities are separated.
- UI does not directly manage raw WebSocket or engine process lifecycle.
- Engine live memory is not transported directly.
- Memory/buffer ownership and lifetime are documented for public APIs.
- Work happens on a dedicated work branch, not main/develop.
- Workstreams are decomposed into reviewable phases.
- Alpha goals and non-goals match README, AGENTS/CLAUDE, and docs/alpha-project-plan.md.
```
