# NorvesLib adapter 規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

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
