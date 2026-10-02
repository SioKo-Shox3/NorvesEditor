# MCP・編集層移行の取り消し試験対応表

基準: NorvesEditor `700dfa8`。Rust の試験名は移植先の予定名であり、実装済みではない。
NE03 の1〜9を独立した試験にし、各行の複数条件も必要なケースへ分ける。
単発操作の観測結果を維持し、画面試験は IPC 応答とバックエンドイベントの模型へ替える。

| 規則 | 現行 vitest の所在と観測 | Rust へ移す契約・予定名 |
|---|---|---|
| 1 accepted のみ | `useBridge.lifecycle.test.tsx:1492` 作成、`:1518` 拒否、`:1540` 複製、`:1565` 親変更、`:1906` 値設定 | `records_only_accepted_edits`: true/false/例外、作成・複製の newId 不在、適用された newValue を使うこと |
| 2 同値を記録しない | 同ファイル`:1973`、`store.test.ts:1754` 値比較 | `skips_equal_property_values`: scalar/null/配列/オブジェクト、JS の数字表現とキー順、同値時に redo を消さないこと |
| 3 新しい記録で redo を消す | `store.test.ts:1796`、`:1808` | `new_record_invalidates_redo`: 4種それぞれと拒否・同値で redo が維持されること |
| 4 逆操作は履歴へ積まない | `useBridge.lifecycle.test.tsx:1668`、`:1707`、`:2019`。旧親の同期捕捉`:1565`、根直下`:1601`。`store.test.ts:1709` / `:2055` | `undo_uses_internal_inverse`: create/duplicate→内部delete、reparent→捕捉済み親（rootは省略）、property→旧値、別の履歴と redo を誤って消さないこと |
| 5 redo の新ID | `useBridge.lifecycle.test.tsx:1742`、`store.test.ts:1850` / `:1861`。ID安定の値設定は`useBridge.lifecycle.test.tsx:2048` / `:2079` | `redo_replaces_created_id`: create/duplicate→新IDで次のundo、reparent/property→ID不変。別まとまりの依存ID連鎖の制限を維持 |
| 6 単発失敗で捨てる | `useBridge.lifecycle.test.tsx:1793` / `:1819` / `:2112` / `:2134`、`store.test.ts:1930` | `single_undo_redo_failure_drops_entry`: accepted:false と例外、対象先頭だけを捨ててエラーを通知。複数まとまりの NE06 の試験とは分離 |
| 7 公開削除は両履歴を消す | `useBridge.lifecycle.test.tsx:1634` / `:2157`、`store.test.ts:1949` | `public_delete_clears_history_on_accept`: 成功のみ両履歴を消す。undo 内部deleteでは消さない |
| 8 接続・終了・workspace | `store.test.ts:1977`。切断`:1985` / `:2019`、終了`:2003` / `:2038`、workspace`:2009`、接続中`:1994`。未接続undoは`useBridge.lifecycle.test.tsx:1848` | `history_follows_connection_generation`: 切断・終了・同sessionId再接続で消す。workspace閉鎖・同世代の通知では保持。旧世代の遅延応答では記録・通知しない。未接続undoはno-op |
| 9 旧値の取得 | `useBridge.lifecycle.test.tsx:1906` / `:1931`（liveイベントとの競合）/ `:1989`（旧値なしでも書く） | `captures_prior_value_by_origin`: UIの捕捉値を後続live更新で上書きしない。捕捉改訂より新しい共通列の適用値は旧値へ補正する。旧値なしは履歴に積まずnullと区別。MCPはgetSnapshot→setProperty順を保証し、取得失敗・値なしでは拒否 |

所在の省略名:

- `store.test.ts`: `apps/editor/src/state/__tests__/store.test.ts`
- `useBridge.lifecycle.test.tsx`: `apps/editor/src/hooks/__tests__/useBridge.lifecycle.test.tsx`

現行の同値判定は JSON.stringify の結果の一致。Rust の serde_json の標準マップ順・
数字表現・Value の等価だけで置き換えず、キー順・整数と小数表現・負のゼロの互換を試験する。
画面からの旧値不在でも書き込む振る舞いと、MCP の旧値取得失敗で拒否する振る舞いは別の契約。

## 画面の観測を残す試験

- `apps/editor/src/hooks/__tests__/useUndoRedoKeybindings.test.tsx` の11件: Ctrl+Z / Ctrl+Y、入力欄の除外、無効状態、購読解除。
- `apps/editor/src/components/shell/__tests__/ToolbarActions.test.tsx` の取り消し・やり直しのケース: 要約イベントで有効・無効が変わり、適切な Tauri コマンドを呼ぶ。
- `store.test.ts:1826` の空履歴と`:1908` の混在LIFO: 逆操作の契約は Rust へ、表示用要約の更新は vitest へ移す。
- 値の設定・undo/redoの appliedValue の反映と、接続の世代が古いイベントを捨てる表示試験を残す。
- Engine の live イベントを一切発行せず、サービスの適用イベントだけで Inspector の値、rename の Outliner 名、構造編集後のツリーが更新される試験を追加する。
- 現行コードの実行中ガード（`useBridge.ts:1233` / `:1309`）に対応する試験はまだ無い。Rustの `duplicate_ui_history_request_is_noop` とvitestの連打・キーリピート試験を足し、実行中ガードと先頭ID/改訂の照合で多重undoを防ぐ。
- `stale_ui_capture_uses_latest_service_value` と `stale_ui_parent_uses_latest_service_parent`: MCP→UIが交互に適用され、イベント到着前でも直前の値/親へundoする。後続live更新だけのケースとは分ける。

## まとまりで追加する試験（NE06）

`group_undo_is_reverse_order`、`group_redo_is_forward_order`、`group_partial_failure_resumes_remaining`、
`failed_group_blocks_new_writes`、`discard_reports_partial_state`、`forward_failure_keeps_successful_prefix`。
`group_redo_remaps_dependent_ids`、`group_redo_remap_survives_partial_failure`も追加する。
作成→値設定、作成→子作成、複製→親変更の新ID参照と次のundo、3件の中間失敗、
再試行でも二重実行しないこと、期限・認証失効・接続世代変更・groupIdなし/別ID/UIでの閉鎖を含む。

## 完了の証拠

移植時に各予定名を実際の `path::test_name` と証拠ログへ置き換える。
Rust側が `cargo test`、画面側が `pnpm -C apps/editor typecheck` / `test` に通り、
対応の空欄が無いことを評価する。ファイル名や行番号の一致だけでは移植完了にしない。
