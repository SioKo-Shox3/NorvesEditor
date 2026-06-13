# TypeScript / UI 規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

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
