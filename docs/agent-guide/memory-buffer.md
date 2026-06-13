# Memory / Buffer 規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

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
