# PROGRESS — NorvesEditor

セッション/反復の引き継ぎ。毎回の開始儀式で最初に読み、反復の終わりに更新する。
`git log` が第二の記録。ここには git に無いこと(判断・未解決・次に見るべき場所)を書く。

## Done
- (反復ごとに1行: タスク id、コミット、検証の要点)
- S-001: Outliner の折りたたみ記憶に接続の鍵(connected の間だけ sessionId から作る)を持たせ、描画時に照合して別の接続の記憶を捨てる。typecheck exit 0、vitest 630/630(新規 6 件、うち 4 件は旧実装で落ちることを確認)。
- S-001 指摘対応: 接続の世代番号を store に足し、同じ sessionId での再接続もパネル不在中に検出する。typecheck exit 0、vitest 632/632(新規の Outliner テストは旧実装で落ちることを確認)。
- S-002: Windows で起動したエンジンを `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 付きの Job に入れる(`src/job_object.rs`、Job は `ProcessState` が初回起動で 1 度だけ作り保持)。fmt/clippy exit 0、cargo test 142 + 14 件通過(Job を閉じると割り当てた子が終わる/割り当てない子は残る/不正なハンドルの割り当ては Err、の 3 件を追加)。警告を stderr へ出す `tracing_subscriber` を初期化。評価者 2 周目の残課題(配布版はコンソールが無く警告が残らない)は S-007 に切り出した。
- S-003: `src/engine_settings.rs` で `app_config_dir/engine-settings.json` を読み書き(無い・壊れている→既定、未知のキーは書き戻しで保持、一時ファイル経由で置き換え)。`launch_engine` は 環境変数 > 設定 > 既定値。`get_engine_settings` / `pick_engine_path` / `clear_engine_path` を追加し、TS の定数とラッパー・`EngineSettingsPayload` 型も追加。ダイアログは `rfd` 0.16 を Rust から直接使う。fmt/clippy exit 0、cargo test 151 + 14 件、IPC 名 commands 31 / events 11 一致。評価者(Astra)PASS。
- S-004: Settings に「エンジン」の欄(有効なパス・出所・環境変数が優先されている旨・「参照…」/「既定に戻す」)。値は `hooks/useEngineSettings.ts` がマウント時に `get_engine_settings` から取り、ローカル状態に持つ(main 窓の store は使わない)。処理中はボタンを無効化し ref でも二重実行を弾く。形の合わない応答はエラーとして表示する。typecheck exit 0、vitest 640/640(新規 8 件)。
- S-005: `set_engine_args` で起動引数(1 要素 = 1 引数)を保存し、`get_engine_settings` 系は `savedArgs` も返す。検査は `process::normalize_engine_args`(空白だけの行を捨てる。`--bridge-port` / `--bridge-port=`(前後の空白を無視)・NUL・改行・32 件超・512 バイト超を拒否)で、`launch_engine` は読み直した値にも同じ検査をかけ、`build_engine_args` でユーザーの引数 → `--bridge-port <port>` の順に `Command::args` へ渡す。Settings に複数行の入力欄・「引数を保存」・保存結果の表示。あわせて S-004 の指摘1(StrictMode の古い応答で busy が外れる)を要求の世代番号で直し、順序のテスト2件(ガードを外すと落ちることを確認)を追加。fmt/clippy exit 0、cargo test 157 + 14、IPC 名 commands 32 / events 11、typecheck exit 0、vitest 648/648。評価者 PASS。

## In progress
- なし

## Next
- S-007 を優先(評価者は S-002 の残りと判定)。その後 S-008、S-006。
- ダイアログの見た目・起動引数の実際の受け渡し・強制終了後にエンジンが残らないことは、自動テストでは確かめられない。全タスクの完了後に Tauri アプリを実機で起動して確かめる。

## Notes
- S-005: 上限は 32 件 × 512 バイト(Windows のコマンドライン上限 32767 文字に収めるため)。S-004 の指摘2(狭い幅での見た目の証拠)はこの環境で GUI を起動できず未対応のまま。起動引数の入力欄も含めて、全タスク後の実機確認で幅を狭めて確かめ、スクリーンショットを残す。`.bat` / `.cmd` を選べる問題は S-008 に切り出した。
- S-004: 欄の幅の確認は jsdom ではレイアウトが無く自動化できない。`docs/agent-guide/typescript.md` に手書き CSS の確認手順は見当たらなかった。CSS は折り返し前提(`.settings-engine__*`、パスは `overflow-wrap: anywhere`)にし、実機で狭いウィンドウにして崩れないことを全タスク後の手動確認に含める。`extractBackendError` を `useBridge.ts` から export して再利用した。
- S-003: `tauri-plugin-dialog` は使わない。登録すると webview の `window.confirm` が Promise を返す版に差し替わり、`PropertyInspectorPanel.tsx` の確認が常に真になる(確認なしでコンポーネントを外す)。rfd はプラグインと同じ版・機能(`common-controls-v6`, `gtk3`)。rfd は windows-sys 0.60 を引くので依存木に 0.60 と 0.61 が並ぶ。
- S-003 の新しいエラー種別は `BackendError::Settings`(kind `settings`)。S-004 の表示で扱う。
- S-002 の限界: spawn から Job への割り当てまでの間は Job の外にいる(tokio の `Command` では suspended 起動ができない)。この間にエンジンが起動した子孫と、Windows 以外の強制終了は対象外。実機での強制終了の確認は全タスク後の手動確認に含める。
- S-001 の限界(同じ sessionId でのアンマウント中の再接続)は評価者の指摘で解消した: store の `connection.generation` を connected へ新しく入るたびに進め、Outliner の記憶の鍵に含める。
- 2026-09-23 開始儀式: main `872e406`(PR #1 のマージ)の tree は `f4d6644` と同一。`f4d6644` で `./scripts/verify.ps1 -Cpp` が exit 0 だった(fixtures 174、bridge cargo test 183、ctest 7/7、IPC 名 commands 28 / events 11、pnpm test 42/43/624)。`cargo test --manifest-path apps/editor/src-tauri/Cargo.toml` は 139 + 14 件が通過。Windows 11 で実行。
- ループは cmd.exe で `verify:` を実行する(`spawnSync(..., { shell: true })`)。verify に bash の構文を書かない。
- `pnpm lint` は実体が無い(どのパッケージにも lint スクリプトが無い)。緑でも何も検査していない。
- NorvesLib の作業ツリーは `feature/rendering-r4-ddgi` の未コミットの変更を抱えている。今回の主題では NorvesLib に触らない。
