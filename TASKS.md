# TASKS — NorvesEditor

M2 ループが消化する機能一覧。M1(対話設計)でユーザーと合意してから書く。1タスク = 1反復で閉じる大きさ
(計画→実装→検証→コミットが1回で終わる)。閉じないと分かったら分割して行を増やす。
`status` は `todo | doing | done | blocked`。`done` へのフリップは検証出力を開いた後でしか許されない(verify-gate)。

主題: 小物の片付け(2026-09-23 ユーザー合意)。
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
