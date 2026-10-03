# PROGRESS — NorvesEditor

セッション/反復の引き継ぎ。毎回の開始儀式で最初に読み、反復の終わりに更新する。
`git log` が第二の記録。ここには git に無いこと(判断・未解決・次に見るべき場所)を書く。

## Done
- MCP-011: `mcp-settings.json` に既定無効・49770を保存し、設定破損は拒否。`mcp/mcp-token.bin` は32バイトOS乱数をWindows利用者スコープDPAPIまたはUnixの0700/0600で保護し、原子的保存・再読込・破損拒否・明示的作り直し・定時間照合を追加。fmt/clippy exit 0、cargo test 227単体+14統合。証拠 `.harness/runs/20261003-035149/verify-MCP-011-1.txt`〜`verify-MCP-011-3.txt`。
- MCP-010: 通常時にUndo/Redoの横へ先頭まとまりの名前・出どころ・件数を表示。部分失敗時は再試行/状態確認/破棄を示し、保留中の編集・Ctrl+Z/Y・通常undo/redo・実行制御を無効化。破棄後の残存変更を通知し、他のキー操作とシーンの視認性を保つ。typecheck exit 0、vitest 628/628。証拠 `.harness/runs/20261003-035149/verify-MCP-010-7.txt` / `verify-MCP-010-8.txt`。
- MCP-009 差し戻し対応: 部分失敗のエラー経路でBridge世代を履歴ロックより先に読み、履歴参照側とロック順を統一してデッドロックを防止。fmt/clippy/cargo test（218単体+14統合）/IPC名照合（37 commands・13 events）/pnpm typecheck はすべて成功。証拠 `.harness/runs/20261003-035149/verify-MCP-009-1.txt`〜`verify-MCP-009-5.txt`。
- MCP-009: 名前・出どころ・時刻を持つまとまりと逆順undo/順redoを実装。部分失敗では成功位置と未処理部分を保留し、既知拒否だけを再試行して成功分を重複適用しない。結果不明のtimeout/通信断は再送せず、破棄結果に残存変更を返す。新IDのまとまり内置換、世代切り替え、保留中の編集・undo/redo・play/pause/stop拒否を試験。fmt/clippy/cargo test（218単体+14統合）/IPC名照合（37 commands・13 events）/pnpm typecheck はすべて成功。証拠 `.harness/runs/20261003-035149/verify-MCP-009-1.txt`〜`verify-MCP-009-5.txt`。
- MCP-008: 画面storeから編集履歴の正本と逆操作を外し、編集・undo/redo・実行制御を共通サービスのTauri入口へ接続。旧値/親と適用改訂の捕捉、要約イベントによる表示、捕捉不足時の再取得と再操作案内を画面側試験で確認し、連打中の二重送信と自動キー反復も抑止。typecheck exit 0、vitest 621/621、IPC名35 commands/13 events一致。証拠 `.harness/runs/20261003-035149/verify-MCP-008-7.txt`〜`verify-MCP-008-9.txt`。
- MCP-007: 編集サービスの接続世代・適用改訂・sequenceを基準化し、購読開始中に届いたイベントも要約取得後に再同期する。世代・改訂が古い通知を捨て、必要なシーン/Inspector snapshotを取り直す。サービスイベントだけで値・改名・構造変更がOutlinerとInspectorへ反映される試験を追加。typecheck exit 0、vitest 657/657。証拠 `.harness/runs/20261003-035149/verify-MCP-007-5.txt` / `verify-MCP-007-6.txt`。
- MCP-001: ADR 0010 / 0011 の承認設計と要件書に合わせ、`docs/architecture.md` の読み取り・書き込み・画面操作経路を明確化。`git diff 700dfa8 --check` exit 0、証拠 `.harness/runs/20261003-035149/verify-MCP-001-4.txt`。
- MCP-002: UI/MCP共通の容量64のactor列と世代固定Bridge handleを追加。停止時は受付を閉じ、実行中のBridge操作をキャンセルし、保留を拒否してactorをjoinする。猶予超過時の中止/joinとdispatcher停止待ちの2秒上限も確認。履歴消去・旧応答破棄・実接続の切断を含む試験を追加。既存UI入口は未変更。fmt/clippy exit 0、cargo test 172単体+14 opt-in、証拠 `verify-MCP-002-8.txt`〜`verify-MCP-002-10.txt`。
- MCP-003: `NORVES_MOCK_PROFILE=mcp-edit` の試験プロフィールに可変ツリー、作成/削除/親変更/複製、任意JSON値の設定と読み取りを追加。複製の再実行は新IDを払い出し、scene.liveUpdate無しでも編集できる。既定の能力一覧・golden応答は維持。CTest 8/8、既存conformance 1/1、証拠 `verify-MCP-003-4.txt` / `verify-MCP-003-5.txt`。
- MCP-004: 編集actorへ4種の履歴記録を追加し、accepted・newId・appliedValue・旧値・redo条件を適用。UI捕捉を改訂値/親で補正し、失われた補正情報が必要な場合だけBridge操作前に再取得を要求する。補正情報が失われていない古い捕捉と、旧値未取得時の書き込みのみの動作も確認。MCP旧値取得と書き込みの一体実行、JSON.stringify互換、512項目/4MiBの補正上限、削除時の損失改訂記録と接続世代変更時の解放を試験。fmt/clippy exit 0、cargo test exit 0（188単体・14統合試験）、証拠 `verify-MCP-004-14.txt`〜`verify-MCP-004-16.txt`。
- MCP-005: 4種の逆操作、作成/複製redoの再採番、単発失敗時の対象破棄、accepted delete・Bridge切断・終了時の履歴消去を実装。構造編集のMETHOD_NOT_SUPPORTEDだけで履歴を無効化し、値編集・RuntimeControl・undo/redoの単発失敗は後続操作を止めない。根直下の旧親省略、workspace閉鎖時の保持、空・未接続・未対応・古いcursor・連打のno-op、世代交替後に新履歴を消さないこと、undo/redoのappliedValue補正を確認。Rust 203 + 14件成功、fmt/clippy exit 0、証拠 `verify-MCP-005-22.txt`〜`verify-MCP-005-24.txt`。
- MCP-006: 既存編集とUI実行制御を共通サービスへ接続し、undo/redo/履歴取得コマンド、UI捕捉DTO、適用・履歴イベントを追加。部分失敗保留中の編集・実行制御を拒否し、履歴要約と適用改訂を返す。fmt/clippy exit 0、cargo test 211単体 + 14統合、IPC名35/13一致、pnpm typecheck完了。証拠 `verify-MCP-006-20.txt`〜`verify-MCP-006-24.txt`。
- MCP M1: 2026-10-03 に E0〜E3 の設計・29タスク・依存一覧・MyWorkflow ガイド正本2本の同期を承認済み。設計評価PASS、文書検査PASS。設計文書は `90dd0fd`。
- S-001: Outliner の折りたたみ記憶に接続の鍵(connected の間だけ sessionId から作る)を持たせ、描画時に照合して別の接続の記憶を捨てる。typecheck exit 0、vitest 630/630(新規 6 件、うち 4 件は旧実装で落ちることを確認)。
- S-001 指摘対応: 接続の世代番号を store に足し、同じ sessionId での再接続もパネル不在中に検出する。typecheck exit 0、vitest 632/632(新規の Outliner テストは旧実装で落ちることを確認)。
- S-002: Windows で起動したエンジンを `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 付きの Job に入れる(`src/job_object.rs`、Job は `ProcessState` が初回起動で 1 度だけ作り保持)。fmt/clippy exit 0、cargo test 142 + 14 件通過(Job を閉じると割り当てた子が終わる/割り当てない子は残る/不正なハンドルの割り当ては Err、の 3 件を追加)。警告を stderr へ出す `tracing_subscriber` を初期化。評価者 2 周目の残課題(配布版はコンソールが無く警告が残らない)は S-007 に切り出した。
- S-003: `src/engine_settings.rs` で `app_config_dir/engine-settings.json` を読み書き(無い・壊れている→既定、未知のキーは書き戻しで保持、一時ファイル経由で置き換え)。`launch_engine` は 環境変数 > 設定 > 既定値。`get_engine_settings` / `pick_engine_path` / `clear_engine_path` を追加し、TS の定数とラッパー・`EngineSettingsPayload` 型も追加。ダイアログは `rfd` 0.16 を Rust から直接使う。fmt/clippy exit 0、cargo test 151 + 14 件、IPC 名 commands 31 / events 11 一致。評価者(Astra)PASS。
- S-004: Settings に「エンジン」の欄(有効なパス・出所・環境変数が優先されている旨・「参照…」/「既定に戻す」)。値は `hooks/useEngineSettings.ts` がマウント時に `get_engine_settings` から取り、ローカル状態に持つ(main 窓の store は使わない)。処理中はボタンを無効化し ref でも二重実行を弾く。形の合わない応答はエラーとして表示する。typecheck exit 0、vitest 640/640(新規 8 件)。
- S-005: `set_engine_args` で起動引数(1 要素 = 1 引数)を保存し、`get_engine_settings` 系は `savedArgs` も返す。検査は `process::normalize_engine_args`(空白だけの行を捨てる。`--bridge-port` / `--bridge-port=`(前後の空白を無視)・NUL・改行・32 件超・512 バイト超を拒否)で、`launch_engine` は読み直した値にも同じ検査をかけ、`build_engine_args` でユーザーの引数 → `--bridge-port <port>` の順に `Command::args` へ渡す。Settings に複数行の入力欄・「引数を保存」・保存結果の表示。あわせて S-004 の指摘1(StrictMode の古い応答で busy が外れる)を要求の世代番号で直し、順序のテスト2件(ガードを外すと落ちることを確認)を追加。fmt/clippy exit 0、cargo test 157 + 14、IPC 名 commands 32 / events 11、typecheck exit 0、vitest 648/648。評価者 PASS。
- S-005 指摘対応(反復 6、S-008 を兼ねる): `validate_engine_path` がパスの文字列の末尾(末尾の `.` と空白を落とした後)が `.bat` / `.cmd` なら大文字小文字を問わず拒否する。std の判定と同じく `.cmd` だけの名前も対象。保存(`pick_engine_path`)と起動(`launch_engine`)は同じ関数を通る。保存待ち中に編集した起動引数は、下書きの編集の世代で判定して応答で上書きしない(新テストは旧実装で落ちることを確認)。ipc-types の英語コメントを日本語に。fmt/clippy exit 0、cargo test 159 + 14、IPC 名一致、typecheck exit 0、vitest 649/649。評価者 2 周目の指摘(`.cmd` 単独名)は対応済み、3 周目は回していない。
- S-006: `docs/norveslib-integration.md`(Step 3 と Known Limitations 3/5/6)、`docs/engine-profile.md`、`docs/build.md` を、Settings の「エンジン」欄・優先順位(環境変数 > 設定 > 既定値)・Windows の Job・バッチファイルの拒否に合わせて日本語で書き直す。NorvesLib アダプタは scene/object/schema を実装済み(ヘッダで確認)。`docs/engine-integration.md` は e2e テスト用の環境変数の記述だけで直す箇所が無かった。verify exit 0(変更前の文書では exit 1)。
- S-007: `src/backend_log.rs` で WARN 以上を stderr と `app_log_dir`(Windows は `%LOCALAPPDATA%\com.norves.editor\logs`)の `backend.log` の両方へ出す。追記で開き 1 件ごとに書く(強制終了でも残る)。起動時に 1 MiB を超えていれば `backend.log.1` へ移す(最大 2 世代)。依存は足していない(自前の `MakeWriter`)。初期化は `setup` で行う。Job の警告 2 件を日本語にし、割り当ての警告を `assign_or_warn` に切り出して、不正なハンドルでの失敗がログファイルへ届くテストを追加。fmt/clippy exit 0、cargo test 164 + 14。
- S-009: README の happy path 2、ステップ 4 の注記、Known Limitations 3 を、Settings の「エンジン」欄と優先順位(`NORVES_ENGINE_PATH` > 保存したパス > 既定値)に触れる形に直し、`docs/engine-profile.md` へ案内する。verify exit 0(変更前は exit 1)。Known Limitations 2 と 5 も古いので S-010 に切り出した。
- S-010: README の Known Limitations 5 を「Windows では Job に入れて強制終了時に終わらせる(割り当て成功後に限る。起動直後の子孫と Windows 以外は対象外)」に直す。Known Limitations 2 は古かった(`not_supported` と書いていた)ので、NorvesLib アダプタのヘッダ(`../NorvesLib/Game/Bridge/NorvesLibBridgeAdapter.h`)の override と mock engine(`mock_adapter.hpp`)を確かめて書き直した。mock はシーン編集(`scene.createObject` など)を実装していない。verify exit 0(変更前は exit 1)。

- S-006 / S-010 指摘対応: `docs/norveslib-integration.md` の Known Limitations 6 と `docs/engine-profile.md` の後始末の項を「Job への割り当て成功後」に限り、残りうる条件(作成・割り当ての失敗、割り当て前のエディタ終了、割り当て前の子孫、Windows 以外)を書く。`engine-profile.md` の既定値の説明に残っていた英語を日本語に。README の Known Limitations 2 の末尾を「実装していないメソッドは `METHOD_NOT_SUPPORTED` を返す」に直す(コードは `bridge/spec/docs/capabilities.md` で確認)。S-001 / S-004 のテスト説明とコメントの英語は、既に日本語になっていた。

- 仕上げ: S-001 の修正(`8312f19`)と S-006 / S-010 の修正(`2607fd4`)は round 2 の評価で PASS。`2607fd4` の時点で `./scripts/verify.ps1 -Cpp` exit 0(fixtures 174、bridge cargo test 183、ctest 7/7、IPC 名 commands 32 / events 11、pnpm test 42/43/649)、src-tauri の fmt / clippy exit 0、cargo test 164 + 14 件通過。`pnpm tauri dev` で起動したエディタが `%LOCALAPPDATA%\com.norves.editor\logs\backend.log` を作ることを確認した(警告が無いので中身は空)。

## In progress
- MCP / 編集層の M2: 承認済み29タスクを評価付きランナーで進行中。MCP-001〜MCP-011完了、次はMCP-012。

## Next
- 次のタスク: MCP-012（認証付き loopback HTTP の入口を実装する）。
- M2: `node ~/.agent-workflow/loop.mjs --repo . --engine codex --unattended --evaluate feature`。各反復の評価はランナーが行い、承認済み範囲内で再承認を求めない。
- MCP の範囲は NE01〜NE14。NE15〜NE22 / NorvesLib 変更は入れない。以下の既存実機確認は別主題として保持する。
- 画面操作が要る確認が残っている:
  - Settings の「参照…」で OS のファイル選択ダイアログが開き、選んだパスが表示される
  - 保存した起動引数が、エンジンのコマンドラインに `--bridge-port` より前で渡る(`Get-CimInstance Win32_Process` の CommandLine で確かめられる)
  - エンジン起動中にエディタを `taskkill /F` で終わらせると、エンジンも終わる
  - Settings の「エンジン」欄を狭い幅にしても崩れない
  - Outliner のドラッグで親を付け替えられる(Windows の WebView2)

## Notes
- 2026-10-03 MCP-011: 開始ゲート `scripts/verify.ps1` exit 0。最終 fmt / clippy / cargo test はすべて exit 0、保存した各出力を開いて確認。Windows DPAPI利用者スコープの保護・復号試験を実行済み。Unixの0700/0600試験は追加したが、この環境はWindowsのみのため実行未確認。
- 2026-10-03 MCP-010差し戻し対応: 要約イベントから通常時のUndo/Redo先頭まとまりの名前・出どころ・件数を表示し、要約イベント/コマンド呼び出しの試験を追加。保留中はCtrl+Z/Y以外のキーを止めず、背後のシーンを見やすくした。開始ゲート `scripts/verify.ps1` exit 0、最終 typecheck exit 0、vitest 628/628。保存ログ `.harness/runs/20261003-035149/preflight-MCP-010-r19.txt` / `verify-MCP-010-7.txt` / `verify-MCP-010-8.txt` を開いて確認。
- 2026-10-03 MCP-009差し戻し: 部分失敗処理で世代ロック→履歴ロックの順に統一。指定された最終ゲート5件はexit 0で、保存ログを開いて確認した。
- 2026-10-03 MCP-009: 開始ゲート `scripts/verify.ps1` exit 0。最終 fmt/clippy/cargo test（218単体・14統合）/IPC名照合（37 commands・13 events）/pnpm typecheck はすべてexit 0。実Bridge要求timeout後の結果不明・保留要約・再送なし・再接続世代変更による解消を新試験で確認し、保存ログを開いて合格を確認。
- 2026-10-03 MCP-008: 開始ゲート `./scripts/verify.ps1` と最終 `pnpm -C apps/editor typecheck` / `pnpm -C apps/editor test` / `node scripts/check-protocol-names.mjs` はすべて exit 0。保存ログを開いて確認し、フロントエンドvitest 621/621、IPC名35 commands/13 events一致。store・hookに画面履歴の記録/逆操作が残らないことも検索で確認した。
- 2026-10-03 MCP-007: 再接続後は接続要約が確定するまで編集イベントを適用せず、取得中に通知が届いた場合は要約をもう一度照会する。古い取得応答は要求ID/接続世代/適用改訂で破棄。typecheckとフロントエンドvitestを保存ログから確認。
- 2026-10-03 MCP-006: fmt / clippy / cargo test（211単体・14統合）/ IPC名検査（35 commands・13 events）/ pnpm typecheck がすべて exit 0。各出力を `.harness/runs/20261003-035149/verify-MCP-006-20.txt`〜`verify-MCP-006-24.txt` に保存して開いて確認。
- 2026-10-03 MCP-005差し戻し対応の開始ゲート `scripts/verify.ps1` exit 0（fixtures 174、Bridge Rust、IPC名32/11、frontend typecheck/build/test 42/43/649。C++は標準ゲートのためSKIP）。最終 fmt / clippy / `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml` は exit 0、Rust 203 + 14件成功。METHOD_NOT_SUPPORTEDの3経路、逆操作失敗後の残存redo、engine exit後のBridge切断、世代競合、appliedValue補正をログで確認。証拠 `.harness/runs/20261003-035149/preflight-MCP-005-r9.txt` / `verify-MCP-005-22.txt`〜`verify-MCP-005-24.txt`。
- 2026-10-03 MCP-005開始ゲート `scripts/verify.ps1` exit 0（fixtures 174、Bridge Rust、IPC 32/11、frontend typecheck/build/test 42/43/649）。最終fmt/clippy/cargo testはexit 0、Rust 196件成功。各出力を `.harness/runs/20261003-035149/preflight-MCP-005.txt` / `verify-MCP-005-19.txt`〜`verify-MCP-005-21.txt` に保存して開いて確認。
- 2026-10-03 MCP-004差し戻し対応: 補正キャッシュが失った最新改訂を追跡し、追い出し・容量超過・削除の後に古い既知値で操作する場合だけ再取得を要求する。改訂0の古いUI捕捉でも損失のない場合は維持し、値未取得なら損失後も書き込みだけ行う。開始ゲート `scripts/verify.ps1 -Cpp` exit 0、最終 fmt/clippy/cargo test もexit 0（188単体・14統合）。ログ `.harness/runs/20261003-035149/verify-MCP-004-14.txt`〜`verify-MCP-004-16.txt` を開いて確認。
- 2026-10-03 MCP-004開始ゲート `scripts/verify.ps1 -Cpp` exit 0。fixtures 174、C++ ctest 8/8、Rust bridge・typecheck/build/lint/vitest 42/43/649 が成功。libwebsockets の MSB8065 警告が1件。
- 2026-10-03 MCP-004検証ログ `.harness/runs/20261003-035149/verify-MCP-004-1.txt`〜`verify-MCP-004-3.txt` を開いて確認。UIの古い値/親は同じ対象の共通列適用値があれば補正し、補正キャッシュの範囲外・別世代では適用前に再取得を返す。MCPは読取り結果なし/失敗時に書込みを呼ばない。
- 2026-10-03 MCP-003: CTest の `mock_edit_profile_test` が編集プロフィールの構造変更/汎用値/新ID再発行/ライブ更新抑止/`log.subscribe` ack を検査。集約ゲートはfixtures 174、C++ 8/8、Rust・フロントエンド各ゲート成功。保存ログを開いて確認。
- 2026-10-03 MCP-002: 開始ゲート `./scripts/verify.ps1` exit 0(fixtures 174、bridge workspace Rust、IPC commands 32 / events 11、typecheck/build、vitest 42/43/649)。最終のfmt/clippy/cargo testはexit 0、Rustは172単体+14 opt-in。保存ログを読み、新規の停止・世代・実接続試験も通過確認。`.harness/runs/20261003-035149/verify-MCP-002-8.txt`〜`verify-MCP-002-10.txt`。
- 2026-10-03 MCP-001開始時の `./scripts/verify.ps1 -Cpp`: exit 0（fixtures 174、C++ ctest 7/7、IPC commands 32 / events 11、vitest 42/43/649、typecheck/build/fmt/clippy）。CMake の libwebsockets 生成物について MSB8065 警告が1件出たが、build と全ゲートは成功。MCP-001 の差分検証は exit 0、証拠 `.harness/runs/20261003-035149/verify-MCP-001-1.txt`。
- 2026-10-03 MCP-001反復2の開始ゲート: `./scripts/verify.ps1 -Cpp` exit 0（fixtures 174、C++ ctest 7/7、IPC commands 32 / events 11、vitest 42/43/649、typecheck/build/fmt/clippy）。CMake の libwebsockets 生成物で MSB8065 警告が1件。出力 `.harness/runs/20261003-035149/preflight-MCP-001.txt`。
- 2026-10-03 MCP-001差し戻し対応: 認証後にMCP読み取りと書き込みを分岐し、書き込みと画面の編集・実行制御を同じサービスへ接続。Bearer/Hostは全要求で検証し、Originは存在時に照合する。`git diff 700dfa8 --check` exit 0、証拠 `.harness/runs/20261003-035149/verify-MCP-001-4.txt`。
- 2026-10-03 M2開始儀式: `verify.ps1 -Cpp` exit 0、エディタ Rust 単体164件通過、vitest42/43/649件通過。opt-inの実エンジン試験は環境変数未設定でSKIP。証拠 `.harness/mcp-m2-start-verify.log` / `mcp-m2-start-tauri.log` を開いて確認した。作業ブランチは `feature/editor-mcp-edit-layer`、プッシュしない。
- 2026-10-02 MCP M1: HEAD `700dfa8`、取得済みの `origin/main` は `872e406`。小物修正ブランチは未統合。基点を `700dfa8` とする選択を受け、`feature/editor-mcp-edit-layer` に分岐した。ワークツリーは増やしていない。
- M1の決定: ポート49770/永続256 bitトークン/app_config_dir（Windows DPAPI、Unix0700/0600）。play/pause/stopは許可を通し操作記録へ、履歴には積まない。複数編集の部分失敗は未処理部分を保持し再試行/破棄、通常編集は成功分だけ残す。単発の失敗時の記録破棄は維持する。
- 開始儀式: `./scripts/verify.ps1 -Cpp` exit 0（fixtures174、C++7/7、IPC32/11、vitest42/43/649、typecheck/build/fmt/clippy）。`cargo test --manifest-path apps/editor/src-tauri/Cargo.toml` exit 0（単体164、opt-in14は環境変数未設定でSKIP）。bridgeのopt-inも未設定なので、実エンジンe2eの合格とは扱わない。証拠 `.harness/mcp-m1-baseline-verify.log` / `mcp-m1-baseline-tauri.log` を開いて確認した。
- M1の停止条件: 文書・依存・未決判断を揃え評価したら全体承認待ちで止める。承認前にコード変更・依存導入・ランナー起動をしない。MyWorkflowガイド2本の正本同期は別repo文書変更として承認対象へ含めたが、現在は変更していない。
- 2026-10-03 M1評価: 指摘対応差分はPASS、blockingなし（`.harness/mcp-m1-review-2.log`を開いて確認）。所有者groupId/現行listenと旧peer通知/まとまり内redo ID/旧値補正/連打/ログ購読の契約を評価した。非阻害指摘は切断取消、実mock SKIP対策、subscriptionId、UI改訂/実行制御の経路、ガイド内容検査の条件へ追記した。3周目は行っていない。補正キャッシュの保持上限の具体化はMCP-004の残課題としてNEXT_FINDINGSへ記録した。実装合格・実機確認済みとは扱わない。
- M1の文書検査: 29タスク/5欄/番号一意/先行順序/NE01〜14/LFはPASS（`.harness/mcp-m1-plan-check.log`）。検証コマンドのmockパス解決は-ErrorAction Stopで失敗時に停止し、SKIPへ落ちない。
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
