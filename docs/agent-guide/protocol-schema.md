# Protocol / Schema 規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## Protocol / Schema 規約

- Bridge protocol の canonical debug format は JSON。
- すべての wire message は `bridge/spec/schema/` の JSON Schema と `bridge/spec/fixtures/` の fixture で表現する。
- 新しい message / method / event を追加する場合は、schema、positive fixture、必要に応じて negative fixture、Rust/TypeScript/C++ の検証を同じフェーズに含める。
- JSON-RPC 2.0 の request/response/notification の考え方は参考にするが、厳密準拠はしない。NorvesEditor 独自の role、session、capability、event、attachment を扱うため、NorvesEditor Bridge envelope を正とする。
- Protocol 互換性を壊す変更は ADR または明示的な protocol migration note を要求する。
