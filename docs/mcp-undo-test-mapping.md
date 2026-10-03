# MCP・編集層移行の取り消し試験対応表

基準: NorvesEditor `700dfa8`。履歴の記録・逆操作・redo再採番・寿命は編集サービスのRust試験で検査する。画面側はTauri IPCと編集サービスeventを模型化し、表示・キー・ボタン・捕捉値の契約を検査する。

| 規則 | 編集サービスの履歴試験 | 画面側に残す観測 |
|---|---|---|
| 1 accepted のみ | `edit_service::history::tests::records_only_accepted_edits_and_uses_new_ids_and_applied_values`、`accepted_create_and_duplicate_without_new_ids_are_not_recorded` | scene編集のIPC・選択表示を `useBridge.lifecycle.test.tsx` で確認。画面自身は履歴へ記録しない |
| 2 同値を記録しない | `skips_equal_and_missing_old_values_without_clearing_redo`、`json_stringify_compatibility_keeps_key_order_and_javascript_number_rules` | 旧値と改訂を含む `object_set_property` 引数、および適用値表示を確認 |
| 3 新しい記録で redo を消す | `new_record_invalidates_redo_for_every_history_kind` | redo可否は履歴要約eventから表示へ反映 |
| 4 逆操作は履歴へ積まない | `edit_service::tests::undo_uses_internal_inverse_for_four_record_kinds` | `edit_undo` commandとevent後の要約表示を確認 |
| 5 redo の新ID | `redo_replaces_created_ids_and_next_undo_uses_the_new_id`、`redo_reuses_stable_reparent_and_property_targets` | `edit_redo` commandとevent後の要約表示を確認 |
| 6 単発失敗で捨てる | `single_undo_redo_failure_drops_only_the_attempted_entry_and_reports_error`、`unsupported_structure_edit_makes_undo_a_noop`、`unsupported_property_edit_keeps_undo_available`、`unsupported_runtime_control_keeps_undo_available` | undo/redoのIPC拒否を表示し、返された要約で履歴ボタンを更新 |
| 7 公開削除は両履歴を消す | `public_delete_clears_both_histories_only_after_acceptance` | 削除Tauri command後、サービス要約eventの表示を確認 |
| 8 接続・終了・workspace | `engine_exit_bridge_disconnect_clears_both_histories`、`history_clears_on_editor_service_shutdown`、`history_survives_workspace_close_without_bridge_generation_change`、`stale_empty_disconnected_and_unsupported_history_requests_are_noops`、`stale_history_admission_does_not_clear_the_new_generation`、`duplicate_undo_and_redo_requests_apply_only_the_expected_head_once` | 接続世代・改訂が古いイベントの破棄、未接続/空履歴の無操作を確認 |
| 9 旧値の取得 | `live_updates_do_not_replace_the_ui_captured_old_value`、`stale_ui_capture_uses_latest_service_value`、`stale_ui_capture_uses_latest_service_parent`、`unlost_stale_capture_uses_its_ui_value_after_an_unrelated_edit`、`stale_capture_without_old_value_writes_without_recording`、`lost_stale_correction_requests_reload_before_running_the_edit`、`mcp_snapshot_and_write_stay_in_one_queue_item_and_read_failure_writes_nothing`、`records_known_null_prior_and_skips_unavailable_prior`、`undo_redo_correction_cache_uses_engine_applied_value` | 画面は旧値/旧親とsnapshotの世代・改訂を発行前に捕捉する。安全に補正できない拒否では対象を再取得し、同じ編集を自動再送しない |

## 画面の観測を残す試験

- `hooks/__tests__/useUndoRedoKeybindings.test.tsx`: Ctrl+Z/Ctrl+Y/Ctrl+Shift+Z、入力欄とcontentEditableの除外、既に処理済みのevent、購読解除を検査する。自動キー反復の2回目を抑止する `キーを押し続けたときの自動反復では追加のundoを送らない` と、実行中に同じundo commandを一度だけ送る境界は `useBridge.lifecycle.test.tsx` の `キーリピート中は先頭ID・改訂のundoを一度だけ送る` で検査する。
- `components/shell/__tests__/ToolbarActions.test.tsx`: 空/有効/未接続/未対応/保留中の要約に対するUndo/Redoボタン、要約event後の有効・無効切り替え、保留中のPlay/Pause/Stop無効化、クリック先を検査する。
- `hooks/__tests__/useBridge.lifecycle.test.tsx`: 親変更前の同期捕捉、シーン直下の親null、値編集の旧値とsnapshot改訂、旧値なしの書き込み、捕捉不足時の再取得と自動再送なし、先頭ID/改訂付きundo/redo、play/pause/stopのTauri入口を検査する。
- `hooks/__tests__/useBridge.lifecycle.test.tsx` の購読試験: `購読開始時に履歴を取得し、改訂が古いイベントを捨て、欠落時に再同期する` と `再接続後は新しい接続の世代と改訂で履歴要約を受け入れる`。
- `components/__tests__/EditServiceEvents.integration.test.tsx`: エンジンlive通知を送らず、編集適用eventだけでInspectorの値・Outlinerの改名/構造変更が表示へ反映される。
- 表示だけの確認として `components/__tests__/PropertyInspectorPanel.test.tsx` の `外部の値設定を選択中snapshotへ反映する`、`components/__tests__/SceneOutlinerPanel.test.tsx` の `Nameの値設定でノード名を更新する` と `構造編集後にツリーを取り直して追加ノードを表示する` を残す。

`store.test.ts` にあったundo/redo配列、逆操作、redo再採番、削除/切断時の履歴消去、同値判定の試験はRust側へ移したため削除した。storeには画面表示に必要な履歴要約・適用eventの世代/改訂処理を残す。親IDの捕捉に使う `findParentId` / `normalizeOldParentId` の試験は画面側に残す。

## まとまりで追加した試験（NE06）

- `edit_service::tests::named_group_undo_runs_reverse_and_redo_runs_forward_once`: 名前・出どころ・時刻・件数を要約し、3件を逆順undo・順redoで1回ずつ実行する。
- `edit_service::tests::redo_remaps_created_ids_across_children_and_resumes_after_known_rejection`: 作成→値設定→子作成を再実行し、作成IDを同じまとまりの対象ID/親IDへ置換する。値中の同じ文字列は置換せず、確定拒否後は成功済み作成を再実行せず、再試行後の次undoが新IDを使う。
- `edit_service::tests::duplicate_redo_remaps_reparent_target_and_the_next_undo_target`: 複製の新IDを親変更の対象へ置換し、再redoと次undoが再採番後のIDを使う。
- `edit_service::tests::forward_group_failure_keeps_only_the_successful_prefix`: 前進中の拒否で、成功した2件だけをまとまりとして保持し、undoする。
- `edit_service::tests::unknown_group_result_is_not_retried_and_discard_reports_remaining_changes`: 通信断で結果不明を保持し、再試行を拒否する。破棄結果に完了件数・総数・不明状態・残存変更を返す。
- `edit_service::tests::timed_out_group_result_is_held_until_reconnect_without_retrying`: 実Bridge要求のtimeoutで結果不明と進行位置を要約し、同じ操作を送らず、再接続の世代変更で保留・旧履歴を解消する。
- `edit_service::tests::pending_partial_failure_rejects_edits_undo_redo_and_runtime_controls`: 保留中の通常編集、undo/redo、play/pause/stop相当の実行制御を拒否する。
- `edit_service::tests::generation_change_clears_pending_group_and_rejects_old_group_handle`: 世代交替で保留・履歴・旧まとまりハンドルを無効化する。
- 単発のNE03-6は `edit_service::tests::single_undo_redo_failure_drops_only_the_attempted_entry_and_reports_error` で維持する。

## 完了の証拠

MCP-004で完了した行は実際のRust試験名と保存証拠を記録する。`edit_service::history::tests::records_only_accepted_edits_and_uses_new_ids_and_applied_values`、`edit_service::history::tests::accepted_create_and_duplicate_without_new_ids_are_not_recorded`、`edit_service::history::tests::skips_equal_and_missing_old_values_without_clearing_redo`、`edit_service::history::tests::records_known_null_prior_and_skips_unavailable_prior`、`edit_service::history::tests::json_stringify_compatibility_keeps_key_order_and_javascript_number_rules`、`edit_service::history::tests::new_record_invalidates_redo_for_every_history_kind`、`edit_service::history::tests::correction_cache_is_bounded_by_entry_count_and_total_bytes`、`edit_service::history::tests::accepted_delete_and_generation_change_release_correction_information`、旧値補正・列内照会の各試験は `cargo test` の証拠 `.harness/runs/20261003-035149/verify-MCP-004-16.txt` で確認する。fmt・clippy・cargo test の各ゲートは `verify-MCP-004-14.txt`〜`verify-MCP-004-16.txt` に保存した。補正キャッシュの512項目/4MiB上限、削除・接続世代変更時の解放、失った改訂の追跡もcargo testの出力に含む。

MCP-005で完了したNE03の4〜8は、表の各行に実試験名を記録した。新規の逆操作、redo再採番、単発失敗、公開削除、エンジン終了後のBridge切断、エディタ終了、workspace閉鎖がBridge世代を変えない場合の履歴保持、世代をまたぐ古い要求と連打の試験は `.harness/runs/20261003-035149/verify-MCP-005-24.txt` で確認する。fmt・clippy・cargo test は `verify-MCP-005-22.txt`〜`verify-MCP-005-24.txt` に保存した。NE03の1〜9に予定名のままの行や空欄はない。

NE06のRust側試験は `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml` の証拠 `.harness/runs/20261003-035149/verify-MCP-009-3.txt` で確認する。fmt、clippy、IPC名照合、TypeScript型検査も `verify-MCP-009-1.txt`〜`verify-MCP-009-5.txt` に保存する。
