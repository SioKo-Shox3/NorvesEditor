# MCP・編集層移行の取り消し試験対応表

基準: NorvesEditor `700dfa8`。1/2/3/9はMCP-004、4〜8はMCP-005で追加したRust試験名と証拠を示す。
NE03 の1〜9を独立した試験にし、各行の複数条件も必要なケースへ分ける。
単発操作の観測結果を維持し、画面試験は IPC 応答とバックエンドイベントの模型へ替える。

| 規則 | 現行 vitest の所在と観測 | Rust 試験または移植先の予定名 |
|---|---|---|
| 1 accepted のみ | `useBridge.lifecycle.test.tsx:1492` 作成、`:1518` 拒否、`:1540` 複製、`:1565` 親変更、`:1906` 値設定 | `edit_service::history::tests::records_only_accepted_edits_and_uses_new_ids_and_applied_values`、`edit_service::history::tests::accepted_create_and_duplicate_without_new_ids_are_not_recorded`: 4種の accepted 判定、作成・複製の newId、engine の appliedValue を実試験 |
| 2 同値を記録しない | 同ファイル`:1973`、`store.test.ts:1754` 値比較 | `edit_service::history::tests::skips_equal_and_missing_old_values_without_clearing_redo`、`edit_service::history::tests::json_stringify_compatibility_keeps_key_order_and_javascript_number_rules`: scalar/null/配列/オブジェクト、JS の数値・整数キー順・通常キー順・負のゼロ、同値時に redo を保持 |
| 3 新しい記録で redo を消す | `store.test.ts:1796`、`:1808` | `edit_service::history::tests::new_record_invalidates_redo_for_every_history_kind`: 4種の新記録で redo を消し、拒否・同値・旧値不在では保持 |
| 4 逆操作は履歴へ積まない | `useBridge.lifecycle.test.tsx:1668`、`:1707`、`:2019`。旧親の同期捕捉`:1565`、根直下`:1601`。`store.test.ts:1709` / `:2055` | `edit_service::tests::undo_uses_internal_inverse_for_four_record_kinds`: create/duplicate→直接delete、reparent→捕捉済み親（rootは省略）、property→旧値。4種すべてで逆操作を再記録せず、内部deleteでも既存redoを保持 |
| 5 redo の新ID | `useBridge.lifecycle.test.tsx:1742`、`store.test.ts:1850` / `:1861`。ID安定の値設定は`useBridge.lifecycle.test.tsx:2048` / `:2079` | `edit_service::tests::redo_replaces_created_ids_and_next_undo_uses_the_new_id`、`edit_service::tests::redo_reuses_stable_reparent_and_property_targets`: create/duplicate→新IDで次のundo、reparent/property→IDと値を維持 |
| 6 単発失敗で捨てる | `useBridge.lifecycle.test.tsx:1793` / `:1819` / `:2112` / `:2134`、`store.test.ts:1930` | `edit_service::tests::single_undo_redo_failure_drops_only_the_attempted_entry_and_reports_error`: accepted:false とMETHOD_NOT_SUPPORTED応答で対象だけを捨て、呼び出し元へエラーを返す。複数まとまりの NE06 とは分離 |
| 7 公開削除は両履歴を消す | `useBridge.lifecycle.test.tsx:1634` / `:2157`、`store.test.ts:1949` | `edit_service::tests::public_delete_clears_both_histories_only_after_acceptance`: 拒否時は保持し、公開削除の成功時だけ両履歴を消す。規則4でundo内の内部deleteは保持を検査 |
| 8 接続・終了・workspace | `store.test.ts:1977`。切断`:1985` / `:2019`、終了`:2003` / `:2038`、workspace`:2009`、接続中`:1994`。未接続undoは`useBridge.lifecycle.test.tsx:1848` | `edit_service::tests::history_clears_on_disconnect_and_exit_but_survives_workspace_close`、`edit_service::tests::stale_empty_disconnected_and_unsupported_history_requests_are_noops`、`edit_service::tests::duplicate_undo_and_redo_requests_apply_only_the_expected_head_once`: 切断・サービス終了で両履歴を消す。workspace閉鎖はBridge世代を変えないため、同世代同期では履歴を保持する。空・切断・未対応・先頭ID/改訂不一致・undo/redo連打はno-op |
| 9 旧値の取得 | `useBridge.lifecycle.test.tsx:1906` / `:1931`（liveイベントとの競合）/ `:1989`（旧値なしでも書く） | `edit_service::tests::live_updates_do_not_replace_the_ui_captured_old_value`、`edit_service::tests::stale_ui_capture_uses_latest_service_value`、`edit_service::tests::stale_ui_capture_uses_latest_service_parent`、`edit_service::tests::unlost_stale_capture_uses_its_ui_value_after_an_unrelated_edit`、`edit_service::tests::stale_capture_without_old_value_writes_without_recording`、`edit_service::tests::lost_stale_correction_requests_reload_before_running_the_edit`、`edit_service::tests::mcp_snapshot_and_write_stay_in_one_queue_item_and_read_failure_writes_nothing`、`edit_service::history::tests::records_known_null_prior_and_skips_unavailable_prior`: UI捕捉値は後続live更新だけでは上書きしない。捕捉改訂より新しい共通列の値/親があればundo先へ補正する。失われた補正情報が必要な古い値は適用前に再取得を求める。補正値を失っていない古い捕捉は使い、旧値未取得は書き込みだけ行う。MCPの旧値照会と書き込みは列内で連続し、照会失敗・値なしでは書き込みを呼ばない |

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
- Rustの `edit_service::tests::duplicate_undo_and_redo_requests_apply_only_the_expected_head_once` は、同じ先頭ID/改訂の連打を一度だけ適用する。画面の実行中ガード（`useBridge.ts:1233` / `:1309`）とキーリピートは、MCP-008 の vitest で検査する。
- `stale_ui_capture_uses_latest_service_value` と `stale_ui_parent_uses_latest_service_parent`: MCP→UIが交互に適用され、イベント到着前でも直前の値/親へundoする。後続live更新だけのケースとは分ける。

## まとまりで追加する試験（NE06）

`group_undo_is_reverse_order`、`group_redo_is_forward_order`、`group_partial_failure_resumes_remaining`、
`failed_group_blocks_new_writes`、`discard_reports_partial_state`、`forward_failure_keeps_successful_prefix`。
`group_redo_remaps_dependent_ids`、`group_redo_remap_survives_partial_failure`も追加する。
作成→値設定、作成→子作成、複製→親変更の新ID参照と次のundo、3件の中間失敗、
再試行でも二重実行しないこと、期限・認証失効・接続世代変更・groupIdなし/別ID/UIでの閉鎖を含む。

## 完了の証拠

MCP-004で完了した行は実際のRust試験名と保存証拠を記録する。`edit_service::history::tests::records_only_accepted_edits_and_uses_new_ids_and_applied_values`、`edit_service::history::tests::accepted_create_and_duplicate_without_new_ids_are_not_recorded`、`edit_service::history::tests::skips_equal_and_missing_old_values_without_clearing_redo`、`edit_service::history::tests::records_known_null_prior_and_skips_unavailable_prior`、`edit_service::history::tests::json_stringify_compatibility_keeps_key_order_and_javascript_number_rules`、`edit_service::history::tests::new_record_invalidates_redo_for_every_history_kind`、`edit_service::history::tests::correction_cache_is_bounded_by_entry_count_and_total_bytes`、`edit_service::history::tests::accepted_delete_and_generation_change_release_correction_information`、旧値補正・列内照会の各試験は `cargo test` の証拠 `.harness/runs/20261003-035149/verify-MCP-004-16.txt` で確認する。fmt・clippy・cargo test の各ゲートは `verify-MCP-004-14.txt`〜`verify-MCP-004-16.txt` に保存した。補正キャッシュの512項目/4MiB上限、削除・接続世代変更時の解放、失った改訂の追跡もcargo testの出力に含む。

MCP-005で完了したNE03の4〜8は、表の各行に実試験名を記録した。新規の逆操作、redo再採番、単発失敗、公開削除、切断・終了、workspace閉鎖がBridge世代を変えない場合の履歴保持、古い要求と連打の試験は `.harness/runs/20261003-035149/verify-MCP-005-21.txt` で確認する。fmt・clippy・cargo test は `verify-MCP-005-19.txt`〜`verify-MCP-005-21.txt` に保存した。NE03の1〜9に予定名のままの行や空欄はない。

未完のNE06の規則は、実装後に予定名を実際の `path::test_name` と証拠ログへ置き換える。Rust側が `cargo test`、画面側が `pnpm -C apps/editor typecheck` / `test` に通り、対応の空欄が無いことを評価する。ファイル名や行番号の一致だけでは移植完了にしない。
