# Rust 規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

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
