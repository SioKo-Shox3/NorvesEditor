# TASKS — NorvesEditor

M2 ループが消化する機能一覧。M1(対話設計)でユーザーと合意してから書く。1タスク = 1反復で閉じる大きさ
(計画→実装→検証→コミットが1回で終わる)。閉じないと分かったら分割して行を増やす。
`status` は `todo | doing | done | blocked`。`done` へのフリップは検証出力を開いた後でしか許されない(verify-gate)。

現在の主題: MCP と編集層（E0〜E3 / NE01〜NE14）。2026-10-03 に M1 の全体承認を受けた。
基点: `700dfa8`、ブランチ: `feature/editor-mcp-edit-layer`。MCP- の29タスクを M2 で実行する。
仕様: `docs/mcp-and-edit-layer-requirements.md`、ADR 0010 / 0011、`docs/mcp-undo-test-mapping.md`。
要件12章の依存一覧と MCP-029 の MyWorkflow ガイド正本2本の同期も承認済み。NE15〜NE22と NorvesLib の変更は今回の対象外。

M2 の共通規則:
- 先行タスクが done でないタスクは実装しない。blocked の先行が残る場合は依存するタスクも理由付きで blocked にする。
- 1反復1タスク。TASKS.md / PROGRESS.md は全タスクの追跡する帳簿として更新できる。
- 全タスクをランナーの `--evaluate feature` で別文脈評価する。反復内で評価者を二重に呼ばない。
- 証拠は `.harness/` に保存し、実出力を開く。opt-in試験の SKIP を完了の証拠にしない。
- 承認範囲を外れる設計・依存、同じ手法2回の失敗、危険な操作が必要なら、合意の相談・blockedの手順に従う。
- Bridge の schema・fixture・SDK 公開APIを変更しない。mock は既存メソッドを実装する明示的な試験プロフィールを足す。
- 所有権・秘密・許可・世代の検査を省略して試験を通さない。GUI は自動操作せず、Rust / HTTP / vitest の証拠を使う。
- 既存の S- は完了済みの記録として残す。プッシュしない。

以前の主題: 小物の片付け(2026-09-23 ユーザー合意)。
合意済みの設計:
- エンジン実行ファイルのパスは、Rust 側から開く OS のファイル選択ダイアログでだけ変えられる。
  フロントエンドからパス文字列を受け取るコマンドは作らない(`launch_engine` の「Takes NO path from the frontend」を保つ)。
  JS 側にダイアログの権限(capabilities)は付与しない。
- 設定は OS のアプリ設定ディレクトリ(`app_config_dir`)の JSON に、マシン・ユーザー単位で保存する。ワークスペースには置かない。
- パスの解決順は 環境変数 `NORVES_ENGINE_PATH` > 設定 > 既定値(既存の `resolve_engine_path` の順序。e2e が環境変数に依存する)。
- 起動引数は 1 行 1 引数のリストで保存し、シェルを介さず `Command::args` で渡す。
- 強制終了時の後始末は Windows の Job Object(`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`)。エディタ 1 プロセスに Job 1 つ。
  対象はエディタが起動したエンジンだけで、アタッチ接続は対象外。
- Outliner の折りたたみ記憶は、接続が切れたら捨てる。

## S-001: Outliner の折りたたみ記憶を切断で捨てる
- status: done
- done-when: 接続の世代(`state.connection.sessionId` の変化、または接続状態が connected から外れたこと)が変わったあとに描画される Outliner では、前の接続で折りたたんだノードが折りたたまれていない。Outliner がアンマウントされている間(dockview でタブを離れている間)に切断・再接続しても同じ。同じ接続の中でタブを離れて戻ったときは、従来どおり折りたたみが残る。絞り込み文字列(`rememberedFilter`)は切断で消さない。これらを確かめる vitest が `apps/editor/src/components/__tests__/` にあり、既存のテストと合わせて全件通る。
- verify: `pnpm -C apps/editor typecheck`
- verify: `pnpm -C apps/editor test`
- paths: apps/editor/src/components/SceneOutlinerPanel.tsx, apps/editor/src/components/__tests__/**
- notes: 記憶はモジュールスコープの `rememberedCollapsed`(SceneOutlinerPanel.tsx:40)。アンマウント中は effect が走らないので、記憶に「どの接続の記憶か」を持たせ、描画時に今の接続と照合する形が素直。

## S-002: エディタが強制終了してもエンジンが残らないようにする(Windows Job Object)
- status: done
- done-when: Windows で `launch_engine` が起動した子プロセスを、`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` を設定した Job に割り当てる。Job のハンドルはエディタのプロセスが生きている間ずっと保持される(`ProcessState` か、それと同じ寿命の所有者)。割り当てに失敗しても起動は失敗させず、警告ログを出して続ける。Windows 限定のテストが、長く生きる子プロセスを Job に割り当ててから Job のハンドルを閉じると、子が数秒以内に終了することを確かめて通る。Windows 以外の挙動は変わらない。`process_runtime.rs` のモジュール文書の「Residual orphan risk」を、実装に合わせて日本語で書き直す。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/**, apps/editor/src-tauri/tests/**, apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock
- notes: 危険地帯(プロセス寿命)なので評価者を必ず通す。`windows-sys` は既に依存木にあるので、新しく足すなら同じ系列に合わせる。Job の生成を `launch_engine` のたびに行うか 1 度だけにするかは実装で決めてよいが、エディタ 1 プロセスに Job 1 つを保つ。

## S-003: エンジン設定を保存し、パスの解決に使う(バックエンド)
- status: done
- done-when: エンジン設定を OS のアプリ設定ディレクトリの JSON に読み書きするモジュールがある。ファイルが無い・壊れている場合は既定の設定として扱い、起動を妨げない。今後の項目追加に備えて、未知のキーや欠けたキーを許す。`launch_engine` は 環境変数 > 設定 > 既定値 の順でパスを解決する(`resolve_engine_path` の `config` 引数に設定値を渡す)。Tauri コマンドを 3 つ足す:
  - `get_engine_settings`: 有効なパス、その出所(env / settings / default)、保存済みのパスを返す。
  - `pick_engine_path`: Rust 側で OS のファイル選択ダイアログを開く。選ばれたファイルを `validate_engine_path` で確かめてから保存する。キャンセルなら何も変えない。
  - `clear_engine_path`: 保存済みのパスを消す。
  フロントエンドからパス文字列を受け取るコマンドは無い。JS 側の capabilities にダイアログの権限を足さない。設定ファイルの読み書き(無い・壊れている・保存)と解決の優先順位のユニットテストが通る。IPC 名の同期チェックが通る(TS 側のコマンド定数も同じコミットで足す)。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `node scripts/check-protocol-names.mjs`
- paths: apps/editor/src-tauri/src/**, apps/editor/src-tauri/tests/**, apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock, bridge/ts/packages/bridge-ui/src/**, bridge/ts/packages/bridge-types/src/**
- notes: 危険地帯(Tauri のプロセス/セキュリティ権限)なので評価者を必ず通す。ダイアログは `tauri-plugin-dialog` を Rust 側からだけ使う想定。依存を足すなら、Tauri 本体と揃う版を選び、packaging への影響をコミット本文に書く(`docs/agent-guide/tauri-security.md`)。起動引数はこのタスクでは扱わない(S-005)。

## S-004: Settings ウィンドウにエンジンの設定欄を出す
- status: done
- done-when: Settings パネルに「エンジン」の欄がある。欄には次を置く:
  - 有効なパスとその出所の表示
  - `pick_engine_path` を呼ぶ「参照…」ボタンと、`clear_engine_path` を呼ぶ「既定に戻す」ボタン
  - 環境変数で上書きされているときは、設定よりも環境変数が優先されている旨の表示
  ダイアログのキャンセルでは表示が変わらない。エラーのときはエラーを表示する。ボタンは処理中に二重に押せない。これらの表示と呼び出しを確かめる vitest が通る。欄はウィンドウの幅を狭めても崩れない(手書き CSS の確認手順は `docs/agent-guide/typescript.md`)。
- verify: `pnpm -C apps/editor typecheck`
- verify: `pnpm -C apps/editor test`
- paths: apps/editor/src/**, bridge/ts/packages/bridge-ui/src/**, bridge/ts/packages/bridge-types/src/**
- notes: S-003 のコマンドに依存する。Settings は別ウィンドウ(secondary-windows)で開くので、main 窓の store に頼らずに値を取得する。

## S-005: 起動引数を設定できるようにする
- status: done
- done-when: 起動引数を 1 行 1 引数のリストとして保存するコマンド `set_engine_args` があり、`get_engine_settings` が保存済みの引数も返す。`launch_engine` は保存された引数を `--bridge-port <port>` より前に、シェルを介さず渡す。保存の前に検証し、次の引数を拒否してエラーを返す:
  - `--bridge-port` そのもの、または `--bridge-port=` で始まる引数
  - NUL を含む引数
  - 件数か長さの上限を超える引数(上限値は実装で決め、定数にする)
  空行は捨てる。Settings の「エンジン」欄に複数行の入力欄と保存ボタンがあり、保存の結果とエラーを表示する。Rust の検証と引数の組み立てのユニットテスト、UI の vitest が通る。IPC 名の同期チェックが通る。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `node scripts/check-protocol-names.mjs`
- verify: `pnpm -C apps/editor typecheck`
- verify: `pnpm -C apps/editor test`
- paths: apps/editor/src-tauri/src/**, apps/editor/src-tauri/tests/**, apps/editor/src/**, bridge/ts/packages/bridge-ui/src/**, bridge/ts/packages/bridge-types/src/**
- notes: 危険地帯(Tauri のプロセス起動)なので評価者を必ず通す。フロントエンドが渡せるのは引数だけで、実行ファイルはダイアログで選んだものに固定される。この前提をコミット本文に書く。

## S-006: 古いドキュメントを実装に合わせる
- status: done
- done-when: `docs/norveslib-integration.md` の Known Limitations を実装に合わせる。
  - 3「scene / object / schema methods not supported in alpha」は、NorvesLib アダプタの結線状況に合わせて削除するか書き換える。
  - 5「Engine path Settings UI not implemented」と 6「Orphan risk on editor force-quit」は、S-002〜S-005 の実装に合わせて更新する。
  同じ文書の Step 3 と `docs/engine-integration.md` など、`NORVES_ENGINE_PATH` を唯一の指定手段として書いている箇所を、設定欄と優先順位(環境変数 > 設定 > 既定値)に触れる形に直す。
- verify: `node -e "const s=require('fs').readFileSync('docs/norveslib-integration.md','utf8');if(/Settings UI not implemented|not supported in alpha/.test(s))process.exit(1)"`
- paths: docs/**
- notes: 触ったブロックだけ日本語にしてよい(既存の英語文書を一括翻訳しない)。S-002〜S-005 の完了後に着手する。

## S-007: バックエンドの警告を配布版でも残す
- status: done
- done-when: Windows の配布版(`windows_subsystem = "windows"` でコンソールが無い)でも、バックエンドの `tracing` の WARN 以上がどこかに残る(例: アプリのログディレクトリのファイル)。出力先と保持方針を決め、Job への割り当て失敗などの警告がそこへ出ることを確かめるテストが通る。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/**, apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock
- notes: S-002 の評価で判明。今は `tracing_subscriber` が stderr にだけ出すので、配布版では失われる。出力先(ファイルの場所・ローテーション)は設計判断が要るので、決められなければ blocked に書く。評価者は「S-007 への切り出しだけでは S-002 の完了条件を満たさない」と判定しているので、S-002 の残りとして優先して扱う。あわせて `process_runtime.rs` の Job 関連の警告本文(英語)を日本語にする。

## S-008: バッチファイルをエンジンとして起動させない
- status: done
- done-when: `validate_engine_path` が `.bat` / `.cmd`(大文字小文字を問わない)を拒否してエラーを返し、ダイアログで選んでも保存されず、環境変数や設定ファイルで指定されても `launch_engine` が起動しない。Rust std は `.bat` / `.cmd` を cmd.exe 経由で起動するので、起動引数がシェルを通らない前提を守るため。拒否のユニットテストが通る。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/**
- notes: S-005 の評価で判明(S-003 からの持ち越し)。ダイアログは「すべてのファイル」を許し、`validate_engine_path` は `is_file` しか見ていない。危険地帯(Tauri のプロセス起動)なので評価者を通す。

## S-009: README のエンジン指定の記述を実装に合わせる
- status: done
- done-when: `README.md` の「NORVES_ENGINE_PATH でエンジン実行ファイルを指定する」「エンジンパスの Settings UI は alpha 未実装」「エンジンパスの Settings UI 未実装」の箇所を、Settings の「エンジン」欄と優先順位(環境変数 > 設定 > 既定値)に触れる形に直す。
- verify: `node -e "const s=require('fs').readFileSync('README.md','utf8');if(/Settings UI は alpha 未実装|Settings UI 未実装/.test(s))process.exit(1)"`
- paths: README.md
- notes: S-006 で見つけた(S-006 の paths は docs/** のみで README を触れなかった)。

## S-010: README の Known Limitations 2 と 5 を実装に合わせる
- status: done
- done-when: `README.md` の Known Limitations 5「Windows Job Object は post-alpha」を、Windows では起動したエンジンを Job に入れて強制終了時に終わらせる(割り当て成功後に限る。Windows 以外は対象外)形に直す。Known Limitations 2(scene/object/schema は `not_supported`)を、NorvesLib アダプタの実装状況に合わせて確認し、違っていれば直す。
- verify: `node -e "const s=require('fs').readFileSync('README.md','utf8');if(/Windows Job Object は post-alpha/.test(s))process.exit(1)"`
- paths: README.md
- notes: S-009 で見つけた(S-009 の done-when は Settings UI の3箇所だけ)。

## MCP-001: E0 の設計記録と層の境界を確定する
- status: done
- done-when: NE01。ADR 0010 / 0011 が承認済みの設計を記録し、docs/architecture.md の層と所有者が一致する。ADR2本と層の更新が評価されている。
- verify: `git diff 700dfa8 --check`
- paths: docs/adr/0010-backend-edit-service-and-history.md, docs/adr/0011-local-mcp-interface.md, docs/adr/README.md, docs/architecture.md, docs/mcp-and-edit-layer-requirements.md
- notes: 先行なし。NorvesEditorの製品文書を確定する。管理外agent-guideの正本同期は独立タスクに分け、実装の先行条件にしない。

## MCP-002: 編集サービスの実行列と接続世代を用意する
- status: done
- done-when: NE02。UI/MCP の2入口から来た編集を受付順に1件ずつ実行する。undo中に来た編集はundo完了後に実行する。接続世代が変わると履歴と保留が無効になり、旧応答を新世代へ記録しない。BridgeのI/O中に接続・履歴のロックが取得できる試験がある。受付停止・キャンセル・終了join・キュー満杯の拒否を試験する。終了を延期して非同期停止し、2秒の猶予後の中止/joinも試験する。同期フックで無期限のblocking_lockを追加しない。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/bridge_state.rs, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/src/process_runtime.rs, apps/editor/src-tauri/src/error.rs
- notes: 先行 MCP-001。actor所有・有界キュー。ここでは既存UI入口を切り替えない。世代とhandleを固定するBridgeの内部ファサードを用意する。Mutexによる全I/Oの直列化は禁止。async寿命の危険地帯、評価・コミット本文必須。

## MCP-003: モックに可変シーンの試験プロフィールを足す
- status: done
- done-when: NE02/NE03/NE12の試験前提。明示的なMCP試験プロフィールで既存のcreate/delete/reparent/duplicateと汎用の値設定を実装し、読み取りに結果が反映される。redoは新IDを返す。liveイベントを抑止しても編集できる。既定MockAdapterの能力・golden応答は変えず、既存conformanceを実行して通る。試験プロフィールのlog.subscribeはspec準拠のsubscriptionIdを返し、既定の不適合ackを変更しない。
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify.ps1 -Cpp`
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -Command "$env:NORVES_MOCK_ENGINE=(Resolve-Path 'build/cpp/examples/mock-engine/Debug/norves_mock_engine.exe' -ErrorAction Stop).Path; cargo test -p norves-bridge-editor-client --test conformance -- --nocapture; exit $LASTEXITCODE"`
- paths: bridge/cpp/examples/mock-engine/**, bridge/cpp/engine-sdk/tests/CMakeLists.txt, bridge/cpp/engine-sdk/tests/mock_edit_profile_test.cpp
- notes: 先行 MCP-001。既存メソッドの実装のみ。新schema・能力トークン・SDK APIを足さない。型名・プロパティは試験用の汎用名。公開ヘッダを編集しない。新プロフィールの試験をCMake/ctestに登録する。

## MCP-004: 4種の履歴記録と記録条件を移植する
- status: done
- done-when: NE03の1/2/3/9。accepted:trueだけを記録し、作成・複製のnewId、既知の旧値、engineのappliedValueを扱う。同値と旧値不在は記録せずredoを保持する。新記録はredoを消す。UIの捕捉後のlive更新だけでは旧値を替えないが、捕捉改訂より新しい共通列の適用値/親があれば補正する。MCPのB適用→古いUIからC適用→undo先B、親変更の同等ケースを試験する。補正情報不足では再取得を求める。MCPは列内で旧値を照会し、取得失敗では書かない。JSON.stringifyのキー順/整数キー/数値/null/負のゼロを再現する。対応表の実試験と証拠を埋める。補正情報はキーとJSON値の合計4MiB・512項目まで保持し、超過した場合は再取得を求める。削除と接続世代変更で補正情報を解放する試験を置く。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/dto.rs, apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock, docs/mcp-undo-test-mapping.md
- notes: 先行 MCP-002。serde_jsonの値等価をそのまま現行のJSON.stringify比較へ代用しない。未取得とnullを別の型で表す。コンポーネント編集は現行の履歴対象を勝手に広げない。serde_jsonのpreserve_orderを有効にし、標準のValue等価やキー順無視に変更しない。

## MCP-005: 単発の取り消し・やり直しと履歴寿命を移植する
- status: done
- done-when: NE03の4/5/6/7/8をそれぞれ独立したRust試験で確かめ、1〜9の全行が埋まる。4種の逆操作は公開編集手順を通らず、履歴に積まず、内部deleteでredoを消さない。根直下の旧親は省略する。redoの新IDへ置換し次のundoが新IDを使う。単発失敗は対象記録を捨てて通知する。公開delete成功・切断・終了で両履歴を消し、workspace閉鎖では保持する。空・切断・未対応時のundo/redoは無操作。UIの先頭ID/履歴改訂が一致しない要求、同じ先頭へのundo/redo連打、未接続要求はno-opになるRust試験がある。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/bridge_state.rs, apps/editor/src-tauri/src/process_runtime.rs, docs/mcp-undo-test-mapping.md
- notes: 先行 MCP-004。依存ID連鎖・削除復元は対象外。単発失敗をNE06の複数編集の保留へ変えない。

## MCP-006: 編集・履歴の Tauri コマンドとサービスイベントを結ぶ
- status: done
- done-when: NE04/NE05。既存の編集入口と新しいundo/redo/履歴取得が共通サービスへ渡る。旧値・旧親の捕捉情報をUI専用DTOで受け、Bridge paramsには出さない。適用ごとに対象・プロパティ・値・新ID・出どころ・まとまりID・世代・改訂を発行し、undo/redoにも発行する。履歴要約を初期取得できる。protocol_names.rsとTSのcommand/event定数、型、ラッパーが一致する。UI捕捉改訂と履歴先頭ID/改訂をDTOに含め、UI起源を明示する。UIのplay/pause/stopもサービスへ渡し、部分失敗保留の拒否を返す。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `node scripts/check-protocol-names.mjs`
- verify: `pnpm -r --if-present typecheck`
- paths: apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/bridge_state.rs, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/src/dto.rs, apps/editor/src-tauri/src/protocol_names.rs, bridge/ts/packages/bridge-ui/src/**, bridge/ts/packages/bridge-types/src/**
- notes: 先行 MCP-005。UIの移行完了までは旧アクションと同じ単発結果を維持し、捕捉DTOの省略をMCP起源として扱わない。プロトコルとTauri IPCを区別する。JS権限を広げない。

## MCP-007: サービスイベントから画面の表示を更新する
- status: done
- done-when: NE05。UIの外から来た値設定がInspectorとOutlinerに出るvitestがあり、renameと構造編集も更新される。engineのscene.treeChanged/object.changedを発行しない模型で通る。世代・改訂が古いイベントを捨て、購読開始と欠落時に要約・必要なスナップショットを再取得する。イベント購読を解除しStrictModeの古い取得応答で上書きしない。表示snapshot/treeに適用改訂を保持し、古い取得応答で改訂を巻き戻さない。
- verify: `pnpm -C apps/editor typecheck`
- verify: `pnpm -C apps/editor test`
- paths: apps/editor/src/state/**, apps/editor/src/hooks/useBridge.ts, apps/editor/src/hooks/__tests__/useBridge.lifecycle.test.tsx, apps/editor/src/components/SceneOutlinerPanel.tsx, apps/editor/src/components/PropertyInspectorPanel.tsx, apps/editor/src/components/__tests__/**
- notes: 先行 MCP-006。この段では既存のundoStack表示を維持できる互換投影を残し、次のタスクで正本を外す。編集の確定通知を画面のgetSceneTree成功やbest-effort live更新のみに依存させない。

## MCP-008: 画面の編集と取り消しをバックエンド履歴へ切り替える
- status: done
- done-when: NE04。store.tsからundoStack/redoStackと逆操作の正本を外し、useBridgeの編集・undo/redoはTauriを呼ぶだけになる。UI旧値・旧親の捕捉は発行前に維持する。Ctrl+Z/Ctrl+Y、空・未接続時、ボタン、入力欄の挙動を維持し、要約イベントで有効/無効が変わる試験がある。既存試験はRustへ移した内部履歴の検査を除き、IPC/eventの模型差し替えで観測結果が同じ。対応表に残った表示試験を列挙する。UIの実行中ガードと先頭ID/改訂を維持し、連打・キーリピートが多重undoにならないvitestがある。画面の捕捉値/親とその時点の適用改訂を送る。補正情報不足の拒否は再取得して再操作を案内し、自動で書き直さない。画面のplay/pause/stopも共通列のTauri入口へ通す。
- verify: `pnpm -C apps/editor typecheck`
- verify: `pnpm -C apps/editor test`
- verify: `node scripts/check-protocol-names.mjs`
- paths: apps/editor/src/state/**, apps/editor/src/hooks/useBridge.ts, apps/editor/src/hooks/useUndoRedoKeybindings.ts, apps/editor/src/hooks/__tests__/**, apps/editor/src/components/shell/ToolbarActions.tsx, apps/editor/src/components/shell/__tests__/ToolbarActions.test.tsx, docs/mcp-undo-test-mapping.md
- notes: 先行 MCP-007。試験削除で数だけ合わせず、移した契約の対応を示す。画面自身が履歴を記録する残存経路が無いことを検索する。E1の単発操作の互換境界として評価する。

## MCP-009: まとまりと部分失敗の進行位置を実装する
- status: done
- done-when: NE06。まとまりに名前・人/MCPの出どころ・時刻がある。3件がundoで逆順、redoで順に1回で戻る。複数編集の途中失敗は成功位置と未処理部分を持ち先頭に残り、再試行は成功済みを重複実行しない。保留中の新書き込みを拒否し、破棄は残る状態を報告する。順操作の失敗では成功分だけ残す。単発のNE03-6は維持する。新IDの置換と世代切り替えの試験もある。同じまとまりの対象ID欄を旧ID→新IDで置換し、create→set/create子/duplicate→reparentと途中失敗後の再試行、次のundoを試験する。保留中は通常undo/redoとplay/pause/stopも拒否する。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `node scripts/check-protocol-names.mjs`
- verify: `pnpm -r --if-present typecheck`
- paths: apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/src/dto.rs, apps/editor/src-tauri/src/protocol_names.rs, bridge/ts/packages/bridge-ui/src/**, bridge/ts/packages/bridge-types/src/**, docs/mcp-undo-test-mapping.md
- notes: 先行 MCP-008。再試行/破棄コマンドと要約もこのタスクで出す。自動rollbackはしない。拒否で未適用が確定したものだけ再試行する。timeout/通信断は結果不明として同じ操作を再送せず、保留・状態確認・破棄/再接続を扱う試験を足す。NE06の追加試験を対応表へ実名で記載する。

## MCP-010: まとまりの要約と失敗時の選択を画面へ出す
- status: done
- done-when: NE06。先頭のまとまりの名前・出どころ・件数と部分失敗を表示し、再試行/破棄を選べる。保留中は通常の編集を無効にし、破棄で一部変更が残ることを示す。単発操作の既存ボタンとキーの挙動を変えない。要約イベントとコマンド呼び出しのvitestが通る。保留中のCtrl+Z/Y・通常undo/redoと実行制御を無効にし、結果不明には再試行を出さず状態確認/破棄を示す。
- verify: `pnpm -C apps/editor typecheck`
- verify: `pnpm -C apps/editor test`
- paths: apps/editor/src/components/shell/ToolbarActions.tsx, apps/editor/src/components/shell/__tests__/ToolbarActions.test.tsx, apps/editor/src/components/HistoryProblemNotice.tsx, apps/editor/src/components/__tests__/HistoryProblemNotice.test.tsx, apps/editor/src/hooks/useBridge.ts, apps/editor/src/state/**, apps/editor/src/**/*.css
- notes: 先行 MCP-009。E1の完了境界。クリックはTauri経由の共通列へ渡す。

## MCP-011: MCP の設定と保護したトークンを保存する
- status: done
- done-when: NE07。既定無効・ポート49770を保存し、32バイトのOS乱数から作った永続トークンをapp_config_dirだけへ保存する。Windowsは利用者スコープDPAPI、Unixは0700/0600。保存/保護失敗時は起動できず、秘密をエラーやログへ含めない。再読込・破損・原子的置換・作り直し・権限の試験が通る。依存は要件12章のgetrandom/base64/subtle/urlと既存windows-sys機能追加に限りlockをコミットする。秘密はapp_config_dir/mcp/mcp-token.bin、Unixの0700は専用mcpディレクトリだけに適用する。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/mcp_settings.rs, apps/editor/src-tauri/src/mcp_token.rs, apps/editor/src-tauri/src/error.rs, apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock, docs/mcp-and-edit-layer-requirements.md
- notes: 先行 MCP-001。試験では一時ディレクトリだけを使う。DPAPI・unsafe・秘密の境界を評価しコミット本文を書く。OS対応を確認できない箇所は未確認と報告する。

## MCP-012: 認証付き loopback HTTP の入口を実装する
- status: done
- done-when: NE07。127.0.0.1だけへbindし、不正Origin403、Originなしの認証済みCLI、トークンなし/違う/失効済みの拒否、Host完全一致を実HTTPで試験する。axumとrmcpの検証設定を明示する。body1MiB/接続32/通常並行16/stream8の上限と通常30秒を守り、listen/GET SSEへ通常timeoutを適用しない。長寿命streamはアイドル5分/最大30分。Discover2026-07-28とInitialize2025-11-25を明示した実クライアントで空tools/listが返る。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/error.rs, apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock, docs/mcp-and-edit-layer-requirements.md, apps/editor/src-tauri/src/lib.rs
- notes: 先行 MCP-010/MCP-011。rmcp/axum/tokio-utilとdev用reqwest/towerを承認一覧から導入する。上限はaxum/tokioで実装し、製品towerの追加は要らない。CORS/Tauri権限を広げない。legacy_session_modeを明示。推移依存増分をcargo tree/lockで示す。危険地帯の評価・本文必須。

## MCP-013: MCP サーバーの設定連携と寿命を管理する
- status: done
- done-when: NE07。既定無効でポートが開かず、bind失敗をUI向け状態で返す。有効化/無効化/ポート変更/トークン再生成/終了を直列化する。旧版session/GET SSEと現行listenを停止し、保留確認・開いたまとまり・投入済み要求の認証改訂を失効させる。MCP actor実行直前も認証改訂を検査する。停止は2秒猶予後の中止/joinまで試験し、秘密をDTOや通常イベントへ含めない。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `node scripts/check-protocol-names.mjs`
- verify: `pnpm -r --if-present typecheck`
- paths: apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/mcp_settings.rs, apps/editor/src-tauri/src/mcp_token.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/src/dto.rs, apps/editor/src-tauri/src/error.rs, apps/editor/src-tauri/src/protocol_names.rs, bridge/ts/packages/bridge-ui/src/**, bridge/ts/packages/bridge-types/src/**
- notes: 先行 MCP-012。信頼したSettings窓/main窓のcommandだけで設定変更・秘密表示を行う。将来の確認/group実装が登録するキャンセルを含め寿命の所有者を定める。危険地帯の評価・本文必須。

## MCP-014: Settings に MCP の接続設定を出す
- status: done
- done-when: NE07。有効化・ポート・状態・bindエラー・秘密の明示表示と再生成・接続手順がある。再生成の影響を示す。別ウィンドウからバックエンドの状態を取得し、StrictMode/連打/キャンセル/応答順の試験が通る。秘密を未要求時に表示しない。接続名はnorves-editorなど用途に基づく名前。
- verify: `pnpm -C apps/editor typecheck`
- verify: `pnpm -C apps/editor test`
- paths: apps/editor/src/components/SettingsPanel.tsx, apps/editor/src/components/__tests__/SettingsPanel.test.tsx, apps/editor/src/hooks/useMcpSettings.ts, apps/editor/src/hooks/__tests__/useMcpSettings.test.tsx, apps/editor/src/**/*.css
- notes: 先行 MCP-013。mainのstoreを別窓へ共有しない。トークン入りの実値を証拠・スナップショットへ入れない。UIは日本語。

## MCP-015: 能力と仕様から道具と入力検証を生成する
- status: done
- done-when: NE08。能力なし/未接続/read-onlyのwrite道具は出ず、接続/許可変更で一覧を更新する。現行accepted_subscription_filter/listenを実装してSubscriptionSinkへ通知し、legacyは各peerへ通知する。両版の実HTTP試験とstream失効試験がある。書き込みの外枠params/groupIdと独自道具のschemaを分け、params部分だけは埋め込みspecと意味が一致する。unknown field/不正型/上限違反を事前拒否し、外部schemaを取得しない。絶対$idとfragment参照の解決を試験する。必要能力の組を定義し、不足時は公開しない。現行listen/旧GET SSEの停止・期限・再購読を実HTTPで試験する。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/bridge_state.rs, apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock, docs/mcp-and-edit-layer-requirements.md
- notes: 先行 MCP-013。jsonschemaを承認した系列・既定機能無効で導入する。Bridgeのschema自体は編集しない。ページング/ログ/履歴/まとまりは独自schemaを明示する。二つのMCP版それぞれのlist変更経路を試験する。

## MCP-016: 最近のエンジンログをバックエンドへ保持する
- status: done
- done-when: NE10。1000件または合計2MiBの上限を超えると古い順に消える。世代・時刻・連番で絞れ、保持範囲と欠落が分かる。relayのlog.messageをUIへのemit前に保管し、UI不在でも読める。切断・再接続と古いrelayのログが混ざらない試験が通る。log.stream接続時にlog.subscribeを世代固定で1回送り、UI/MCP不在でも保持する。購読失敗を状態で返し、切断時のunsubscribeは最善努力、終了/世代変更でタスクを取り消す。古いackを捨てる。実mockのsubscribe後のバーストを保持する試験がある。subscriptionId不在は不適合状態で、IDを捏造してunsubscribeしない。受信ログは保持する。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/mcp/log_buffer.rs, apps/editor/src-tauri/src/bridge_state.rs, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/tests/mcp_reads.rs
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -Command "$env:NORVES_ENGINE_PATH=(Resolve-Path 'build/cpp/examples/mock-engine/Debug/norves_mock_engine.exe' -ErrorAction Stop).Path; cargo test --manifest-path apps/editor/src-tauri/Cargo.toml --test mcp_reads -- --nocapture; exit $LASTEXITCODE"`
- notes: 先行 MCP-002。ログの内容は非信頼データ。無制限の文字列を件数上限だけで保管しない。broadcastのlagはログ件数と断定せず、取りこぼした通知数と欠落の可能性を別で示す。Bridge購読寿命の危険地帯として評価・本文必須。

## MCP-017: モックの試験プロフィールに資産の読み取りを足す
- status: done
- done-when: NE09の試験前提。MCP試験プロフィールが既存asset.readのmanifestとresolveを実装し、能力・logicalPath・未知パスの結果が一致する。既定プロフィールのgoldenは変えない。型schema・可変シーン・資産の読み取りを実プロセスから照会する試験がある。
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify.ps1 -Cpp`
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -Command "$env:NORVES_MOCK_ENGINE=(Resolve-Path 'build/cpp/examples/mock-engine/Debug/norves_mock_engine.exe' -ErrorAction Stop).Path; cargo test -p norves-bridge-editor-client --test conformance -- --nocapture; exit $LASTEXITCODE"`
- paths: bridge/cpp/examples/mock-engine/**, bridge/cpp/engine-sdk/tests/mock_edit_profile_test.cpp, bridge/crates/norves-bridge-editor-client/tests/mcp_mock_profile.rs
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -Command "$env:NORVES_MOCK_ENGINE=(Resolve-Path 'build/cpp/examples/mock-engine/Debug/norves_mock_engine.exe' -ErrorAction Stop).Path; cargo test -p norves-bridge-editor-client --test mcp_mock_profile -- --nocapture; exit $LASTEXITCODE"`
- notes: 先行 MCP-003。新規asset.editやasset.saveを作らない。既存capabilityだけを広告する。

## MCP-018: 読み取り道具と上限付きの続きを実装する
- status: done
- done-when: NE09/NE10。状態・能力・ツリー・snapshot・schema・asset一覧/resolve・最近のログを、モックに対する試験で確認する。1応答256KiB/200項目で切り、切った位置と有効なcursorで続きを返す。rootId/maxDepthを相手が無視してもバックエンドで範囲を絞る。世代変更・期限・改竄cursorを拒否する。snapshot保持16MiB、単一項目超過を明示し、非信頼のエンジン文字列をデータとして返す。ツリーはid/parentId/depthを持つ平坦な深さ優先項目。cursorは5分/世代に結び、LRUで16MiB超過時に古いsnapshotを追い出す。追い出し/単一snapshot超過を試験する。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/bridge_state.rs, apps/editor/src-tauri/tests/mcp_reads.rs
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -Command "$env:NORVES_ENGINE_PATH=(Resolve-Path 'build/cpp/examples/mock-engine/Debug/norves_mock_engine.exe' -ErrorAction Stop).Path; cargo test --manifest-path apps/editor/src-tauri/Cargo.toml --test mcp_reads -- --nocapture; exit $LASTEXITCODE"`
- notes: 先行 MCP-015/MCP-016/MCP-017。実mockを使うmcp_readsをこのタスクで実行する。envなしの通常cargo testではopt-inをSKIPと明示し、env指定の不正パス/起動失敗は必ず失敗させる。cursorをBridgeへ送らない。MCP-022の評価PASS後、別AIへ過去3失敗/評価差戻し1回の証拠を渡して相談済み（.harness/mcp-018-recovery-advisor.log）。旧f2fabdaの応答タスク内shutdownと受信処理の競合が原因で、6feb491の途中保存で順序は修正済みだが、このタスクは未検証のためtodoへ戻す。試験では応答taskがpeerを返し、call_tool後に先にjoinして応答taskのpanicを診断し、strip_prefix失敗では実際のエラー先頭をUTF-8境界で最大256文字だけ表示する。全assertと上限検査を維持し、shutdownは検査後に行う。応答前のshutdownを明示して旧失敗経路を決定的に再現し、接続断とエンジン由来JSONデータを区別する回帰試験を残す。兄弟試験の100ms後shutdownも応答task外へ出し、sleepや成功回数に頼らない。必要な世代送信側は保持し、dispatcherへのbiased追加や製品の停止意味変更はしない。旧評価の英語表示と触ったブロックは規約に合わせる。元のdone-when/verifyは維持し、診断・指定検証・別文脈再評価まで通してからdoneにする。過去の停止/失敗/未評価を隠さず保持する。

## MCP-019: PNG を検証・縮小して MCP に返す
- status: done
- done-when: NE11。memory-buffer-policyにMCPの処理上限と所有権を先に追記する。既存viewport.getThumbnailをMCP画像(base64/mimeType)で返し、全入口で1fpsを共有する。長辺上限を超えた画像の縮小試験、PNG不正・base64不正・宣言寸法不一致・寸法爆弾・byte超過の拒否試験がある。既存Bridge上限640x360/256KiBを緩めず、処理の有界workerと停止を試験する。UI/MCPは同じin-flight取得に合流し、世代別の最新1秒PNGを共有する。MCP要求でGameViewをbackoffさせない試験がある。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: docs/memory-buffer-policy.md, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/bridge_state.rs, apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock
- notes: 先行 MCP-018。imageをPNGのみで導入。復号前の寸法確認と合計128MiB予算を設け、imageの非strictなmax_allocだけを根拠にしない。バッファ所有権の評価・本文必須。

## MCP-020: 時刻見出し付きの画像一覧ヘルパーを作る
- status: done
- done-when: NE11。最大16枚を入力順・時刻見出しで一覧へ並べ、並びと文字の位置を画像の試験で確認する。最大入力4096各辺/16777216画素、作業128MiB、長辺2048、PNG2MiBを守り、空/枚数超過/不正時刻/overflowを拒否する。テキスト結果に時刻と並びを併記する。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/images.rs, apps/editor/src-tauri/src/mcp/images/**, docs/memory-buffer-policy.md
- notes: 先行 MCP-019。E2の完了境界。E4のcapture要求やファイル経路は作らず、boundedな画像値のヘルパーと試験に閉じる。数字・記号の固定字形でフォント依存を増やさない。

## MCP-021: 書き込みモードと対象範囲をバックエンドで検査する
- status: done
- done-when: NE13。毎起動時read-onlyでwriteを拒否し、write可/都度確認の各モードを試験する。scene部分木の外、parent先、duplicate先、componentの所属、delete子孫、undo/redoまとまりの全対象を検査する。runtime全体制御は全体の許可を要求する。設定変更・再接続後に旧許可を使えない。未対応能力や未知IDを拒否し理由を返す。範囲解決は2000ノード/200snapshot/合計2MiB/5秒で、不足能力・不明所属・超過はfail-closed。MCPが人の先頭まとまりをundoする場合も全対象を検査する。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `node scripts/check-protocol-names.mjs`
- verify: `pnpm -r --if-present typecheck`
- paths: apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/src/dto.rs, apps/editor/src-tauri/src/protocol_names.rs, bridge/ts/packages/bridge-ui/src/**, bridge/ts/packages/bridge-types/src/**
- notes: 先行 MCP-020。この段ではwrite道具を公開しない。資産のprefix許可はNE19まで実装せず範囲外として記録する。Tauriセキュリティの評価・本文必須。再接続後の古いまとまりハンドルは未接続または認可失効として拒否する。

## MCP-022-A: 確認brokerの寿命とmain画面の承認境界を閉じる
- status: done
- done-when: NE13の列外部分。途中のconfirmation.rsを追跡し、試験DTO/型import/可変receiver/ProtocolVersionの所有権を修復してHEADだけでテストをビルドできる。確認登録は承認/拒否/120秒/要求取消/許可改訂/世代変更/future破棄で必ず消える。future破棄はRAIIで清掃し、16件の上限、一回限りで要求固有の承認、取消と拒否の区別、秘密IDの非記録を試験する。取得/承認/拒否はmain画面だけが使え、MCPに承認道具を公開しない。read-onlyでdelete/component.remove/undo内部deleteを拒否し、書き込み可でも確認必須。承認後に列外で旧値/対象/範囲/許可改訂/undo先頭ID/履歴改訂/delete履歴破棄影響を照合し、変更なら新しい確認IDで再確認する。確認待ち中もUI編集と終了が進み、終了で待機を取り下げる。
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml --no-run`
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `node scripts/check-protocol-names.mjs`
- verify: `pnpm -r --if-present typecheck`
- paths: apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/src/dto.rs, apps/editor/src-tauri/src/protocol_names.rs, bridge/ts/packages/bridge-ui/src/**, bridge/ts/packages/bridge-types/src/**
- notes: 先行MCP-021。MCP-022の2回の40分超過を受けた分割であり、新しい機能追加ではない。列内の状態再検証と開始済み操作の結果保全はMCP-022で完成させる。この段のedit_service.rsは途中試験のコンパイル修復と確認待ちが列を占有しない試験に限定する。Broker/DTO/配線と全ゲートを閉じ、元のMCP-022をdoneにしない。危険地帯の評価とコミット本文必須。

## MCP-022-B: HTTP要求の取消と全体期限を確認待ちへ結ぶ
- status: done
- done-when: NE13のHTTP要求寿命。書き込みtools/callは125秒、その他は30秒であることを両版のHTTPで試験する。要求ID別notifications/cancelledとRequestContext.ctをその要求のリースへ結び、他の要求を取り消さない。現行/旧版ともHTTP応答破棄でリースを取り消す。確認/再照会/範囲走査/列待ちに合計125秒の期限を適用し、確認期限はmin(120秒,残り)とする。HTTP→要求取消、要求取消→確認登録清掃、SDK context→要求取消を製品の経路で試験し、期限はpaused timeでも反証する。未対応SDK挙動を推測せずrmcpの使用版のコードと実受信で確認する。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `node scripts/check-protocol-names.mjs`
- verify: `pnpm -r --if-present typecheck`
- paths: apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock, apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/dto.rs
- notes: 先行MCP-022-A。MCP-022のHTTP切断と全体期限の契約を引き継ぐ。層をつないだ最終受入はMCP-028でも行い、ここでは実HTTPのリース取消とbroker清掃を別々に証明する。書き込み道具の本公開はMCP-024。既存Tokioのdev-dependenciesにtest-utilだけを追加できる。新しいcrateや系列は追加せず、製品側の機能は維持する。M1の既存依存の機能追加と継続指示の範囲にある計画修正であり、再承認は不要。停止・未評価の履歴は保持する。危険地帯の評価とコミット本文必須。

## MCP-022: 要求ごとの書き込み確認をバックエンドで管理する
- status: done
- done-when: NE13。delete/component.removeとMCP undo内部deleteはwrite可でも確認必須、read-onlyでは拒否する。列外の確認brokerが120秒/拒否/HTTP切断/認証改訂/世代変更で取り下げる。一度だけの要求固有確認で、承認後に旧値/対象/範囲/許可改訂/undo先頭IDと履歴改訂/deleteの履歴破棄影響を照合し、違えば再確認する。待機中もUI編集と終了が進む。MCPから承認できず、信頼したmain画面commandだけが承認する。要求全体125秒に再照会/走査/列待ちも含める。旧版はHTTP応答破棄と要求ID別cancelを結び、両版の切断取消を試験する。未開始は列から取消、開始済みは次の操作へ進まずBridge結果を確認し、適用済みと結果不明を区別する。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `node scripts/check-protocol-names.mjs`
- verify: `pnpm -r --if-present typecheck`
- paths: apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/src/dto.rs, apps/editor/src-tauri/src/protocol_names.rs, bridge/ts/packages/bridge-ui/src/**, bridge/ts/packages/bridge-types/src/**
- notes: 先行MCP-022-A/MCP-022-B。元のdone-whenをすべて保持する最終統合。permitを所有権ごと列へ渡し、Bridge I/O前に許可/世代/対象の状態指紋/旧値/履歴改訂/undo先頭を再検証する。変更は型付きの再確認要求で列外へ戻し、確認待ちを列内へ持ち込まない。未開始の取消は送信ゼロ、開始済みはHTTP切断/期限/認証失効でもBridge futureを破棄せず、Bridge自身の期限内に適用済み/拒否/結果不明を区別して履歴へ反映し、多段操作の次の段へ進まない。終了時だけ従来の2秒猶予で中止する。既存のactor_cancels_an_in_flight_mcp_request_when_its_lease_is_revokedはNE13の開始済み結果保全と衝突するため、この契約に沿った試験へ置き換えることを許可し、理由と結果を記録する。Bridge I/O中はロックを持たない。確認IDは秘密として通常ログへ出さない。UI表示はMCP-023。危険地帯の評価・本文必須。

## MCP-023: 許可モードと確認待ちを画面へ出す
- status: done
- done-when: NE13。モード/部分木指定と確認パネルに対象/前後/出どころ/取り消し可否/delete全履歴破棄が出る。閉じたパネルにも通知と待ち件数が出てクリックで開ける。承認/拒否/120秒期限/取り下げ/初期取得/購読解除をvitestで試験する。確認待ち中もUI編集が動き、read-onlyに承認ボタンを出さない。
- verify: `pnpm -C apps/editor typecheck`
- verify: `pnpm -C apps/editor test`
- paths: apps/editor/src/components/AppLayout.tsx, apps/editor/src/components/SettingsPanel.tsx, apps/editor/src/components/McpApprovalPanel.tsx, apps/editor/src/components/__tests__/**, apps/editor/src/hooks/useMcpSettings.ts, apps/editor/src/hooks/useMcpApprovals.ts, apps/editor/src/hooks/__tests__/**, apps/editor/src/**/*.css, apps/editor/src/shell/**
- notes: 先行 MCP-022。既存dockview方式へ登録する。main画面だけが承認する。JS dialog pluginを追加しない。実機レイアウト未確認は別記する。

## MCP-024: 許可を通した書き込み道具と実行制御を公開する
- status: done
- done-when: NE12/NE13。値設定/create/duplicate/reparent/delete/component付け外し/undo/redo/play/pause/stopが共通列へ渡り、MCPからBridgeへ直接書かない。値設定は言語モデル起源の履歴に積まれ、UIのCtrl+Z相当のundoで戻る。read-only・拒否・確認未完了・能力なしは適用されない。runtimeは許可と操作結果を通り履歴へ積まない。process起動/終了・設定・asset.reloadManifest・任意ファイル道具が存在しない。通信断/timeoutの結果不明は拒否と区別し、自動再送禁止と表示用request IDを返す。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `pnpm -C apps/editor test`
- paths: apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/bridge_state.rs, apps/editor/src/hooks/__tests__/useBridge.lifecycle.test.tsx, apps/editor/src/components/shell/__tests__/ToolbarActions.test.tsx
- notes: 先行 MCP-023。asset編集/object.invoke/runtime.stepはE4。1呼び出し1まとまり。component付け外しは既存の非取り消し操作であることを結果へ明示する。

## MCP-025: 名前付きまとまりの開始・終了と自動閉鎖を公開する
- status: done
- done-when: NE12/NE06。beginの256 bit groupIdをparams外枠で受け、正しいID所持を所有者とする。秘密と表示用IDを分け、共有トークン/自己申告client名を同一性に使わない。複数編集が1回undoで戻り、groupIdなし/別ID/UI/undo/redo/非取り消し操作の前に閉じる。128編集/無操作5分/全体15分/無効化/認証失効で閉じる。期限後IDは拒否する。人の列を止めず、同じまとまり内redoの依存IDを置換し、部分失敗はNE06を使う。Bridge切断/世代変更で閉じて旧groupIdを拒否する。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- paths: apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**
- notes: 先行 MCP-024。stateless HTTP断でクライアント同一性を推定しない。legacy session IDは補助のみ。別まとまり横断のID復元は行わない。

## MCP-026: 言語モデルの操作結果を記録・保存する
- status: done
- done-when: NE14。時刻・道具・対象・要約・結果・まとまりIDを成功/拒否/失敗/部分成功/時間切れすべてで記録し、UIへ通知して初期取得できる。app_log_dirのmcp-operations.jsonlへ1MiB×2世代で保存する。トークン・巨大値・本文を含めず、ファイル失敗を表示する。boundedな保持、rotation、秘密の非記録のRust試験がある。結果不明と自動再送禁止を明示し、所有者groupId/確認IDを保存しない。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `node scripts/check-protocol-names.mjs`
- verify: `pnpm -r --if-present typecheck`
- paths: apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/src/dto.rs, apps/editor/src-tauri/src/protocol_names.rs, bridge/ts/packages/bridge-ui/src/**, bridge/ts/packages/bridge-types/src/**
- notes: 先行 MCP-025。backend.logと同じディレクトリ、別ファイル。試験は一時ディレクトリ。操作記録の要約はエンジン由来文字列を命令として使わない。MCP-023の評価指摘に従い、既存の確認取得/承認/拒否と型をbridge-uiのindexから再公開する（実装は既存commandsを使う）。MCP-027で確認UIの相対パス中継を解消できるようにする。MCP-024の評価残課題をNE14の範囲で閉じる。表示用requestIdと操作記録を対応させ、再起動後に衝突しない識別または起動を区別する記録を使う。単発MCP undo拒否後の保留、未送信/拒否/適用済み/結果不明、HTTP取消後の開始済みBridge結果を記録・表示と整合させて検証する。actor側の結果捕捉は追加paths内で行い、Bridge I/O中にロックを持たない。秘密groupId/確認IDを記録せずdisplayGroupIdを使う。元のdone-when/verifyを維持する。

## MCP-027: AI の操作パネルと履歴へのリンクを出す
- status: done
- done-when: NE14。AIの操作パネルに時刻・道具・対象・要約・結果・まとまりIDと確認待ちを表示し、取り消し可能なまとまりのリンクで共通列のundoを行う。履歴の順番を飛び越して対象だけを消さず、現在取り消せないリンクは理由とともに無効にする。初期取得・イベント・拒否・部分失敗・ファイル失敗・購読解除のvitestが通る。
- verify: `pnpm -C apps/editor typecheck`
- verify: `pnpm -C apps/editor test`
- paths: apps/editor/src/components/McpOperationsPanel.tsx, apps/editor/src/components/__tests__/McpOperationsPanel.test.tsx, apps/editor/src/components/McpApprovalPanel.tsx, apps/editor/src/hooks/useMcpOperations.ts, apps/editor/src/hooks/__tests__/useMcpOperations.test.tsx, apps/editor/src/hooks/useMcpApprovals.ts, apps/editor/src/hooks/__tests__/useMcpApprovals.test.tsx, apps/editor/src/shell/**, apps/editor/src/**/*.css, apps/editor/src/components/AppLayout.tsx
- notes: 先行 MCP-026。既存dockviewのパネル登録方式に揃える。実装工程やモデル名をUIへ書かない。MCP-023の評価指摘もここで閉じる。確認許可の取得失敗後の成功で旧エラーを消し、新しい確認通知を隠さないことをvitestで確認する。MCP-026で再公開したbridge-uiの入口を使い、確認UIの相対パス中継を解消する。許可が未確認の間の承認抑止は維持する。

## MCP-028-A: 本番と受入試験のサービス入口を共有する
- status: todo
- done-when: NE07〜NE14の受入基盤。AppHandleを使うアダプタと、実mock/実HTTPの試験が同じMCP handler・許可/確認・共通編集列・操作記録を使う構成へ切り出す。試験用にcfg(test)だけの別の書き込み/許可実装を作らない。Wry/WebView2を試験にロードせず、Bridgeの実mockを起動して現行Discover/旧版Initializeの両入口から状態読み取りと許可付き編集が共通列へ届く最小試験を通す。起動失敗/不正パス/SKIPは失敗にし、全体の受入を完了扱いにしない。HTTP/Bridge/actorの終了をjoinし、トークン・秘密IDを露出しない。
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-mcp-e2e.ps1`
- paths: apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock, apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/bridge_state.rs, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/tests/mcp_e2e.rs, apps/editor/src-tauri/tests/mcp_reads.rs, scripts/verify-mcp-e2e.ps1
- notes: 先行MCP-027。MCP-028で計画済みの製品入口切り出しを独立させたもので、新しい機能/依存は追加しない。manifest変更は既存依存のdev-only機能と試験用featureに限定する。Wryを除くサービス入口を本番アダプタからも使い、同じものを実mockへ接続する。最小の実経路の証明と所有権/寿命を閉じ、元のMCP-028の全done-when/verifyは後続で保持する。危険地帯の評価とコミット本文必須。

## MCP-028: モックと実 HTTP で E0〜E3 の受入試験を実行する
- status: todo
- done-when: NE07〜NE14。スクリプトがmock実行ファイルを確認してNORVES_ENGINE_PATHを設定し、MCP試験プロフィールと実HTTPクライアントを使う。無効/認証/Origin/Host/能力/2版の通知/ページング/ログ/画像/モード/範囲/必須確認/共通列/履歴/名前付きまとまり/部分失敗/再接続/トークン再生成/終了を試験し、SKIPがあれば非ゼロで終わる。表示はサービスイベントだけのvitestで確認する。全ゲートと実行ログが開かれ、評価されている。Discover/Initializeを明示し、現行listenとlegacy通知の実受信を確認する。
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify.ps1 -Cpp`
- verify: `cargo fmt --manifest-path apps/editor/src-tauri/Cargo.toml --all -- --check`
- verify: `cargo clippy --manifest-path apps/editor/src-tauri/Cargo.toml --all-targets -- -D warnings`
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-mcp-e2e.ps1`
- paths: apps/editor/src-tauri/Cargo.toml, apps/editor/src-tauri/Cargo.lock, apps/editor/src-tauri/src/mcp.rs, apps/editor/src-tauri/src/mcp/**, apps/editor/src-tauri/src/edit_service.rs, apps/editor/src-tauri/src/edit_service/**, apps/editor/src-tauri/src/bridge_state.rs, apps/editor/src-tauri/src/lib.rs, apps/editor/src-tauri/tests/mcp_e2e.rs, apps/editor/src-tauri/tests/mcp_reads.rs, scripts/verify-mcp-e2e.ps1, docs/mcp-and-edit-layer-requirements.md, docs/mcp-undo-test-mapping.md, docs/architecture.md, docs/adr/0010-backend-edit-service-and-history.md, docs/adr/0011-local-mcp-interface.md
- notes: 先行 MCP-028-A。共通入口はAで切り出し済みのものを使い、元の全受入条件をこの反復で閉じる。追加pathsは共通入口と受入の検証に限定し、cfg(test)だけの別実装で合格にしない。manifest変更は既存依存のdev-only機能と受入の試験用featureに限り、新規crate/系列は追加しない。画面のコマンド/eventはvitestと名前照合で覆う。NorvesLib用e2eを起動しない。未確認の実機表示は明記し、全体確認済みとしない。新規不具合は別タスクとして記録し、受入契約を満たしてからdoneにする。NEXT_FINDINGSと.harness/mcp-review-follow-up-018.md / mcp-review-follow-up-022-B.md / mcp-review-follow-up-022.md / mcp-review-follow-up-024.md / mcp-review-follow-up-025.mdを照合し、修正済みの証拠か残る挙動・制限を文書と最終報告へ残す。特に現行版の複数クライアント間ID衝突、本文受信30秒と要求全体125秒、Enabledでの再確認、単発MCP undo拒否後の保留/破棄によるUI再開、未送信/拒否/結果不明の区別、書き込みのcancel通知後の応答と記録、本番actorの実mock経路、部分木parentId/cursor世代を確認する。実HTTPで要求をまたいでgroupId付き編集を送り、作成したIDへの値設定のundo/redoで依存ID置換を試験する。同じ値の受理だけでは無操作期限が延長されない挙動も文書へ明記する。元の受入done-whenは縮めない。

## MCP-029: 管理用ガイドの正本へ所有権とセキュリティを同期する
- status: todo
- done-when: NE01の管理用文書同期。MyWorkflowの正本architecture/tauri-securityが編集サービス・loopback MCP・秘密保護を記録し、deploy後の管理外コピーが一致する。NorvesEditor製品文書と矛盾しない。
- verify: `git -C ../MyWorkflow diff main --check`
- verify: `powershell -NoProfile -ExecutionPolicy Bypass -Command "if ((Get-FileHash 'docs/agent-guide/architecture.md').Hash -ne (Get-FileHash '../MyWorkflow/projects/NorvesEditor/agent-guide/architecture.md').Hash) { exit 1 }; if ((Get-FileHash 'docs/agent-guide/tauri-security.md').Hash -ne (Get-FileHash '../MyWorkflow/projects/NorvesEditor/agent-guide/tauri-security.md').Hash) { exit 1 }"`
- verify: `node -e "const fs=require('fs');const a=fs.readFileSync('../MyWorkflow/projects/NorvesEditor/agent-guide/architecture.md','utf8');const t=fs.readFileSync('../MyWorkflow/projects/NorvesEditor/agent-guide/tauri-security.md','utf8');if(!a.includes('edit_service')||!a.includes('127.0.0.1')||!t.includes('mcp-token.bin')||!t.includes('127.0.0.1'))process.exit(1)"`
- paths: ../MyWorkflow/projects/NorvesEditor/agent-guide/architecture.md, ../MyWorkflow/projects/NorvesEditor/agent-guide/tauri-security.md
- notes: 先行 MCP-001。別repo文書2本だけの変更をM1承認へ含める。MyWorkflowの合意を読み専用ブランチで編集・コミット・deployする。展開コピーの直接編集/mainコミット/push禁止。不可ならblockedにして他タスクを止めない。MyWorkflowのコミットも進捗へ記録する。
