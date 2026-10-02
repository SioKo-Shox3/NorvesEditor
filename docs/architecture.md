# NorvesEditor Architecture

Status: planning / pre-alpha

This document summarizes the initial architecture for the NorvesEditor alpha. The detailed project plan lives in `docs/alpha-project-plan.md`; agent operating rules live in `AGENTS.md` and `CLAUDE.md`.

## Alpha Shape

NorvesEditor is a Tauri desktop editor with a Rust backend and TypeScript frontend. The alpha goal is a narrow vertical slice: launch or attach to a C++ engine process, connect through the Bridge protocol, display engine status/logs, and control runtime state from a Game View panel.

The alpha Game View is not an embedded native GPU viewport. The engine owns an external native viewport window. NorvesEditor controls and reflects that external viewport through process management and Bridge messages.

## Layer Boundaries

E0〜E3 の拡張案は ADR 0010 / 0011 と `mcp-and-edit-layer-requirements.md` に記録する。
次の2層は実装予定であり、全体承認後に追加する。

| 層 | 所有するもの | 境界 |
| --- | --- | --- |
| Rust の編集サービス | 編集・取り消し・やり直しの共通列、履歴、接続世代、適用結果のイベント | UI と MCP が同じ入口を使う。Bridge の I/O 中に状態ロックを保持しない |
| Rust の MCP の口 | loopback HTTP、トークン認証、Origin/Host 検証、道具一覧、許可・確認、操作記録 | 読み取りは既存の照会、書き込みは編集サービスを使う。プロセス操作・エンジン設定を公開しない |

```text
画面 → Tauri のコマンド ─┐
                       ├→ 編集サービス → Bridge クライアント → エンジン
言語モデル → MCP → 許可 ─┘
                       └→ 適用結果・履歴要約 → Tauri イベント → 画面
```

MCP は既定で無効、127.0.0.1 のみ、既定ポート49770。書き込みは読み取りのみから始める。
適用結果の通知は編集サービスが所有し、エンジンの best-effort イベントに依存しない。
秘密はアプリ設定ディレクトリ、操作記録はアプリログディレクトリに置く。

| Layer | Owns | Must Not Own |
| --- | --- | --- |
| `apps/editor` TypeScript UI | panels, presentation state, command/event wrappers | raw WebSocket transport, engine process spawning, engine live memory |
| `apps/editor/src-tauri` Rust backend | engine process lifecycle, Bridge client connection, reconnect/session state, frontend event fan-out | C++ SDK public API, NorvesLib internals, direct UI rendering logic |
| `bridge/spec` | wire protocol, JSON Schema, fixtures, protocol docs | product UI state, process management |
| `bridge/crates` | Rust protocol model, codec, editor-side Bridge runtime, tools | Tauri-specific UI components, C++ engine adapter implementation |
| `bridge/ts` | TypeScript DTOs, Tauri command wrappers, frontend event helpers | raw transport sockets, process lifecycle |
| `bridge/cpp/engine-sdk` | standalone C++ engine-side SDK boundary | Tauri, React, TypeScript, NorvesLib-specific types |
| NorvesLib adapter | NorvesLib-specific mapping to Bridge DTOs | generic Bridge SDK behavior |

## Connection Flow

```text
TypeScript UI
  -> typed Tauri command wrappers
Tauri Rust backend
  -> engine process lifecycle service
  -> Rust Bridge editor client runtime
  -> WebSocket + JSON
C++ engine process
  -> standalone Bridge engine SDK
  -> engine adapter
```

The Rust backend is the owner of process state and Bridge connection state. The UI observes state through commands/events and never owns raw WebSocket state.

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
