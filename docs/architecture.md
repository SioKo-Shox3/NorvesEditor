# NorvesEditor Architecture

Status: planning / pre-alpha

This document summarizes the initial architecture for the NorvesEditor alpha. The detailed project plan lives in `docs/alpha-project-plan.md`; agent operating rules live in `AGENTS.md` and `CLAUDE.md`.

## Alpha Shape

NorvesEditor is a Tauri desktop editor with a Rust backend and TypeScript frontend. The alpha goal is a narrow vertical slice: launch or attach to a C++ engine process, connect through the Bridge protocol, display engine status/logs, and control runtime state from a Game View panel.

The alpha Game View is not an embedded native GPU viewport. The engine owns an external native viewport window. NorvesEditor controls and reflects that external viewport through process management and Bridge messages.

## Layer Boundaries

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
