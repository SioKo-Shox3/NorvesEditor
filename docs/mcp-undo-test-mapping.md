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

## E0〜E3 実HTTP受入の対応と証拠

`scripts/verify-mcp-e2e.ps1` は実mock必須の統合試験とRust単体試験をまとめて実行し、SKIP/ignored/必須成功行の欠落を拒否する。
2026-10-04の保存証拠は `.harness/runs/20261004-170201/verify-MCP-028-4.txt`（exit 0）。
Rust単体405件、実HTTP/実mockの両版シナリオ1件、独立した実mockログ購読1件が成功し、保存出力を開いて確認した。
自動試験の検証結果であり、ランナーの別文脈評価と実機GUI確認の完了を意味しない。

| 契約 | 試験と今回の証拠 |
|---|---|
| NE07 無効・認証・Host/Origin・トークン・終了 | `mcp_e2e::security_case` の実HTTP認証/Host/Origin、各版のトークン再生成と旧トークン拒否、無効化時のport閉鎖、HTTP/actor/Bridge終了のjoinが成功 |
| NE08 能力と2版通知 | 実mockの能力から公開道具を検査し、現行Discover/listenとlegacy Initialize/peerの通知を実受信。`current_http_listen_notifies_after_connection_permission_and_disconnect_changes` / `legacy_http_list_changes_are_sent_to_each_peer` も単体内の実HTTPで成功 |
| NE09 ツリー・snapshot・schema・資産・ページ | 各版の `mcp_e2e::read_case` で実mockを解析し、pageSize=1の継続、部分木根parentId=null、maxDepth、入力拒否、資産解決を確認。再接続後cursor拒否を実HTTPで検査。容量/LRU/世代境界は単体で成功 |
| NE10 ログ | 各版の実HTTPから世代の3件を取得し、afterSequenceで2件へ絞り込む。保持/欠落/世代/購読は単体で成功。`real_mock_subscription_burst_is_retained_without_a_ui_or_mcp_client` は環境指定で実mockを起動して成功 |
| NE11 画像 | 各版の実HTTPからimage/pngのbase64をPNGとして復号し、byteと寸法の上限を確認。縮小と時刻付き一覧の単体試験も成功 |
| NE12 共通列・履歴・まとまり | 両版の実HTTPで通常値設定とUI undo、begin→groupId付きの親作成→その親への複製→新IDの値設定→end→一回MCP undo/redo→UI undoが成功。親と複製対象の再採番、編集値、終了/再接続後の旧groupId拒否を検査 |
| NE13 モード・範囲・確認 | 両版でReadOnlyの拒否理由/未適用、Enabledの適用、Confirmのmain承認と別窓拒否、部分木外拒否、まとまりundo内の削除確認、削除の拒否/承認と履歴破棄を検査。`queued_enabled_write_reconfirms_changed_old_value_and_history` は列待ちの変化でEnabledも再確認することを検証 |
| NE14 記録と確定 | 実HTTP適用のrequestIdと記録/ファイルを照合し、トークン/確認ID/所有者groupIdの非記録を確認。列前の通常書き込み拒否を確定扱いにするassert、500件/保存上限/rotation/保存失敗は単体で成功 |
| 取消・部分失敗 | `both_http_versions_dispatch_writes_and_return_structured_outcomes` は取消後の現行版のunknown応答/表示ID、旧版のJSON-RPC結果なしのSSE終了を検査。両版で同じ記録のapplied/rejected確定と再送なしを照合し、4件の `cancel-response-record` 成功行を必須とする。`rejected_single_mcp_undo_is_pending_until_ui_discards_it` の保留/破棄/UI再開、部分成功/不明の再送抑止も成功 |
| 期限とID | `http_lifetime_both_protocols_enforce_30_and_125_seconds_with_paused_time`、`http_lifetime_cancellation_is_scoped_by_typed_id_and_legacy_session`、`accepted_equal_value_does_not_extend_group_idle_deadline` が成功 |
| redo認可と列内再検証 | `redo_allows_only_ordered_recreated_dependencies_inside_scope` / `redo_dependencies_do_not_bypass_existing_targets_or_component_membership` は逆順/未知/範囲外/衝突/所属/上限を拒否。`redo_dependency_review_rechecks_scope_and_existing_values` は再作成前の現在値を捏造せず、列内で既存値と範囲を再照合することを検証 |
| サービスeventによる画面更新 | `EditServiceEvents.integration.test.tsx` と操作記録/確認/履歴のvitestを集約ゲートで実行。699件成功。実機の配置・狭幅・窓間操作は未確認 |

指定ゲートと受入featureの検査は次のとおり。保存先は `.harness/runs/20261004-170201/` で、各ファイルにコマンド行とexitを保存する。

| コマンド | 保存出力 | 結果 |
|---|---|---|
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify.ps1 -Cpp` | `verify-MCP-028-1.txt` | exit 0。C++ 8/8、画面699件、bridge-types 42件、bridge-ui 45件、IPC commands 46 / events 15一致 |
| `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check` | `verify-MCP-028-5.txt` | exit 0 |
| `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings` | `verify-MCP-028-6.txt` | exit 0 |
| `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets --features mcp-e2e -- -D warnings` | `verify-MCP-028-7.txt` | exit 0 |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-mcp-e2e.ps1` | `verify-MCP-028-4.txt` | exit 0。405単体＋両版受入1件＋実mockログ1件、SKIP/ignoredなし |

旧保存先 `.harness/runs/20261004-144511/` の `acceptance-MCP-028-4.txt` と `verify-MCP-028-7.txt` / `-8.txt` は、MCP redoが未再作成の旧IDの所属検査で拒否された失敗証拠として保持する。
40分超過と停止時点の未評価は `PROGRESS.md` と `.harness/mcp-stopped-run-20261004-144511.json` に保持する。
今回の `verify-MCP-028-2.txt` は取消応答の結果分類をcancelledと期待した試験の失敗（exit 101）。開始済みはunknownを優先する既存契約をコードで照合し、応答・記録・後続確定を検査した成功出力が `-4.txt` である。
公開MCP redo、元の完了条件と検証を維持する。MCP-028-Bの前提評価は `.harness/runs/20261004-170201/eval-1.out.txt` のPASSを確認済み。
受入評価の差分は `b32115d` からHEADまで（`175a8ab` / `ae90c34` を含む）の製品修正と受入全体で、再開反復だけの差分に限定しない。
PowerShell 5.1でも日本語の道具応答・エラー文をUTF-8で保存する。

### MCP redoの依存ID認可（MCP-028-B）

保存証拠は `.harness/runs/20261004-170201/verify-MCP-028-B-5.txt`（fmt）、
`verify-MCP-028-B-6.txt`（mcp-e2e付きclippy）、`verify-MCP-028-B-7.txt`（実HTTP/実mock受入）。
指定3コマンドはすべてexit 0。Rust単体405件、両版を含む実HTTP受入1件、実mockログ購読1件が成功し、SKIPと無視された試験はない。

- `redo_allows_only_ordered_recreated_dependencies_inside_scope`: 先行作成・複製の依存、子作成、作成した子からの親省略の再複製を許可し、値設定・子作成・親変更・複製元の逆順参照を拒否する。
- `redo_dependencies_do_not_bypass_existing_targets_or_component_membership`: 既存ID・component所属・上限・範囲を検査し、親省略の複製先と履歴の親とは異なる現在の移動元も範囲外なら拒否する。
- `redo_dependency_review_rechecks_scope_and_existing_values`: 既存対象の現在値と範囲の変化を列内で検出する。未再作成IDの現在値は保存値で代用しない。
- `confirmed_mcp_redo_retries_dependencies_with_remapped_ids`: 確認承認後と列内で依存関係を検査し、先行作成成功後の値設定拒否から新IDを維持して再試行する。親変更・子作成・次のundoも新IDを使う。
- `real_http_and_mock_share_production_services`: Discover / Initializeの各版で親作成→その親への複製→新IDの値設定→end→MCP undo/redo→UI undoを実行し、親ID・値設定対象IDの再採番を検査する。

### まとまり制御の列前拒否と記録の終端（NE14 / MCP-030）

`edit_begin_group` / `edit_end_group` は、制御ticketの発行前に拒否された場合も
`outcome=notApplied` / `actorFinished=true` で記録を確定する。列へ入った制御の取消は
actorの処理まで未確定とし、同じrequestIdへ後続結果を反映する。

| 試験 | 検証内容 |
|---|---|
| `both_http_versions_dispatch_writes_and_return_structured_outcomes` | 両版HTTPのbegin/endで入力不正、ReadOnly、未接続による道具非公開と、beginのサービス側ReadOnlyを拒否。各版7ケースで受付番号が進まないこと、拒否理由、result/outcome/actorFinished、保存JSONLとの一致、トークン・秘密groupId・確認ID相当の入力値の非記録を検査する。正常begin/endと列内の再end拒否も確定済みとなる |
| `public_group_prequeue_denials_are_terminal` | begin/endの許可失効、認証リースなし、Bridge未接続、受付停止、列満杯を公開道具入口から拒否。ticket未発行、未適用、確定済み、適用0件、保留なし、再送なし、Bridge送信なしを検査する |
| `queued_group_cancellation_waits_for_actor_and_preserves_terminal_result` | begin/endの列待ち中に要求取消、呼出future破棄、許可失効、125秒期限超過を発生させる。actor処理前の未確定と処理後の同じIDへの確定、取消・拒否・期限の分類、未送信、自動再送禁止を検査する |
| `cancelled_queued_public_write_stays_not_sent_when_actor_finishes` / `late_actor_result_replaces_unknown_and_cannot_be_overwritten_by_http` | 通常書き込みの列待ち取消と後続確定、およびHTTP側の遅い結果による確定記録の上書き防止を維持する |

保存先は `.harness/runs/20261004-170201/`。次の出力を開いて成功を確認した。

| コマンド | 保存出力 | 結果 |
|---|---|---|
| `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check` | `verify-MCP-030-2.txt` | exit 0 |
| `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets --features mcp-e2e -- -D warnings` | `verify-MCP-030-3.txt` | exit 0 |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-mcp-e2e.ps1` | `verify-MCP-030-4.txt` | exit 0。Rust単体407件、両版の実HTTP/実mock受入1件、実mockログ購読1件。SKIP/ignoredなし。`MCP_GROUP_TERMINAL_OK` は両版それぞれ7行 |

まとまり関連21件の個別検証は `verify-MCP-030-1.txt`（exit 0）に保存した。
画面への判定値はバックエンドの操作記録で検証しており、GUI実機の表示・操作は未確認。
