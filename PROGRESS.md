# PROGRESS — NorvesEditor

セッション/反復の引き継ぎ。毎回の開始儀式で最初に読み、反復の終わりに更新する。
`git log` が第二の記録。ここには git に無いこと(判断・未解決・次に見るべき場所)を書く。

## Done
- T-001 (`34ee756`): result に optional `components:[{objectId,kind}]` を追加。fixture 160 → 163
  (positive 77 / envelope-negative 14 / payload-negative 72)。`OK: 163 fixture(s) validated.`、
  Rust `fixtures_roundtrip` 4 テスト通過、C++ `fixtures_roundtrip_test` 通過。
- T-002 (`f32c644`): mock の n-2 が camera/script を広告し、n-3 は空配列、n-1 は欄なし(適合の
  exact-match 温存)。object.changed は components を持たない(イベント側スキーマが許さない)。
  ctest 7/7、conformance 1 件、process E2E 14 件通過。

## In progress
- T-003: Rust 側の厳格パース(optional・欠落は None・未知フィールド拒否)。

## Next
- T-004: TS の型と store に components を通す。

## Notes
- 主題はコンポーネント縦断。エンジン側は `Component : Object` で `REFLECTION_CLASS` / `PROPERTY` を持ち、
  `SchemaProjection` の `BuildObjectSnapshot(const Object&)` がそのまま使える。id 体系
  `component:<entityObjectId>:<componentId>` と解決経路(`GetComponents()` 走査)はアダプタに実装済み。
  欠けているのは**エディタ側の発見手段**だけなので、`object.getSnapshot` の result に optional
  `components` を足す(合意済み)。
- 開始儀式(Linux コンテナ、2026-09-18): `git log --oneline -10` = `532d125` まで。
  fixtures `OK: 160 fixture(s) validated.` / C++ `100% tests passed, 0 tests failed out of 7` /
  Rust ルート `176 passed` / src-tauri `139 + 14 passed` / TS `579 passed` / typecheck 通過。
- 環境の癖: (1) rustc は 1.96 以上が要る(1.94 だと workspace ごと弾かれる)。
  (2) src-tauri のビルドに GTK/WebKit の開発パッケージが要る(`libwebkit2gtk-4.1-dev` 等)。
  (3) `pnpm install` 後に `pnpm rebuild esbuild` が要る(ビルドスクリプトが既定で無視される)。
  (4) NorvesLib は XAudio2 / Fiber / windows.h 依存で Linux では構築できない。NorvesLib 側の実装と
  検証は作業機(Windows)で行う。
- Codex は使えない(egress ポリシーが api.openai.com への CONNECT を 403 で拒否)。評価者は
  別文脈の Claude(`.claude/agents/evaluator.md`、fable ピン)で回す。
