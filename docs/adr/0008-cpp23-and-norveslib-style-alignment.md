# 0008: C++23 and NorvesLib Style Alignment (Frozen Public API)

Status: Accepted for alpha

## Context

The C++ Bridge SDK (`bridge/cpp/engine-sdk`, `bridge/cpp/examples`) and the first
reference engine **NorvesLib** are developed by the same team and read by the
same people. Keeping the two codebases stylistically consistent (formatting,
naming, comment conventions) lowers the cost of moving between them.

Two hard constraints shape *how far* that alignment can go:

- **The SDK must not depend on NorvesLib (ADR 0004).** It targets std types,
  carries its own `Result<T, E>` / `JsonValue` value types, uses pImpl, and is
  built no-exceptions. NorvesLib's *type-library* conventions — its bespoke
  container/string types and macros — therefore **cannot** be adopted: doing so
  would couple the generic SDK to a specific engine.
- **NorvesLib consumes the SDK's public API directly.** Its
  `NorvesLibBridgeAdapter` implements `IBridgeEngineAdapter`, and its
  `BridgeServerHost` drives `BridgeEngineServer` / `ITransport` and the public
  free functions. Renaming any of those symbols would ripple into a separate
  repository and break its build.

NorvesLib's house style is C++23 with Allman braces, 4-space indent, left-aligned
pointers, `#pragma once`, `m_PascalCase` members (`m_b` for bools), `PascalCase`
enums, and Japanese Doxygen comments. The SDK currently targets C++20 and a
Google-derived layout, and `docs/agent-guide/cpp.md` previously required an ADR
before moving past C++20.

## Decision

1. **Adopt C++23.** Raise the whole `bridge/cpp` tree to C++23
   (`set(CMAKE_CXX_STANDARD 23)`, `target_compile_features(... cxx_std_23)`),
   matching NorvesLib's language level. This ADR is the approval that
   `docs/agent-guide/cpp.md` required.
2. **Adopt NorvesLib *style* only; reject its *type-library* conventions.** Align
   formatting, naming, and comment conventions with NorvesLib via
   `bridge/cpp/.clang-format` and `bridge/cpp/.clang-tidy`. **Keep** std types,
   `Result<T, E>`, pImpl, and no-exceptions. **Do not** introduce NorvesLib's
   bespoke types or macros into the SDK.
3. **Freeze the public API symbol names (Option A).** Because NorvesLib consumes
   them across a repository boundary, the public API symbol names are frozen as
   the cross-repo contract. Internal identifiers follow the new style; public
   symbols are left as-is. The `.clang-tidy` config enforces internal identifiers
   only, so `clang-tidy --fix` never renames the public API. Frozen categories:
   - `IBridgeEngineAdapter` (base class name) and its virtual methods: `hello`,
     `getCapabilities`, `getStatus`, `launchInfo`, `runtimePlay`,
     `runtimePause`, `runtimeStop`, `runtimeFocusViewport`, `logSubscribe`,
     `logUnsubscribe`.
   - `BridgeEngineServer` and its methods `handleFrame` / `emitEvent`, plus the
     constructor signature.
   - `ITransport` and its methods `recv` / `send` / `close`.
   - Public free functions `make_websocket_server_transport` /
     `make_loopback_pair` / `to_wire` / `*_from_wire`.
   - Public member functions on value types (`Result` / `JsonValue` etc.):
     `is_ok` / `is_err` / `value` / `error` / `ok` / `err` / `parse` / `dump` /
     `is_null` and similar — kept in `snake_case`.
   - DTO public struct member names **and their order** (they match wire keys and
     are positionally consumed by aggregate initialization, so reordering is also
     forbidden), and each `to_json`.
   - enum type and enumerator names, the `BridgeError` type, and public type
     aliases.
4. **Hold namespace, line endings, and DTO members fixed.** Keep the
   `norves::bridge` (lower-case) namespace, keep SDK sources LF / UTF-8 (no BOM;
   see `docs/agent-guide/coding-style.md`), and leave DTO member names and order
   unchanged per item 3.

Public constants (`kSdkVersion` etc.) may drop the `k` prefix **only** after
`git grep` confirms NorvesLib does not reference them.

## Consequences

- Public API naming stays **mixed** on purpose: it is not rewritten into
  NorvesLib-style `PascalCase`. New public API added later keeps the existing
  naming convention for compatibility rather than the new internal style.
- **NorvesLib needs no changes.** Its adapter / host build against the same
  symbols as before.
- The **wire protocol is unchanged**: DTO members, their order, and the `to_json`
  output are untouched, so existing fixtures still hold.
- Internal code (private members, locals, parameters, file-local helpers, enums)
  will be migrated to the NorvesLib-aligned style in later passes; this ADR
  records the rules but the source rename is staged separately.

## Affected workstreams

- **F / G / H (C++ SDK, WS transport, mock engine):** new C++ standard and
  style/naming tooling apply to these trees.
- **L (NorvesLib adapter):** depends on the public API freeze; the cross-repo
  contract guarantees NorvesLib's adapter/host keep building unchanged.

## Verification or migration notes

- `scripts/verify.ps1 -Cpp` re-configures, builds, and runs `ctest` green under
  C++23 against the **pre-rename** sources (this pass changes config only, no C++
  source rename).
- `python scripts/validate-bridge-fixtures.py` passes — the wire protocol is
  unchanged.
- NorvesLib builds without modification against the frozen public API.
- The naming convention itself (Allman braces, `m_b` bool prefix, `I`/`T` class
  prefixes) is partly beyond what `.clang-tidy` can express and is enforced by
  review per `docs/agent-guide/cpp.md`.
