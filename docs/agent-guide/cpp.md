# C++ Engine SDK 規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

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
