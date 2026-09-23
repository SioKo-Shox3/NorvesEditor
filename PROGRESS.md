# PROGRESS — NorvesEditor

セッション/反復の引き継ぎ。毎回の開始儀式で最初に読み、反復の終わりに更新する。
`git log` が第二の記録。ここには git に無いこと(判断・未解決・次に見るべき場所)を書く。

## Done
- (反復ごとに1行: タスク id、コミット、検証の要点)
- S-001: Outliner の折りたたみ記憶に接続の鍵(connected の間だけ sessionId から作る)を持たせ、描画時に照合して別の接続の記憶を捨てる。typecheck exit 0、vitest 630/630(新規 6 件、うち 4 件は旧実装で落ちることを確認)。

## In progress
- なし

## Next
- S-001 から順に。S-004 は S-003 に、S-005 は S-003/S-004 に依存する。S-006 は S-002〜S-005 の後。
- ダイアログの見た目・起動引数の実際の受け渡し・強制終了後にエンジンが残らないことは、自動テストでは確かめられない。全タスクの完了後に Tauri アプリを実機で起動して確かめる。

## Notes
- S-001 の限界: パネルがアンマウントされている間に切断され、**同じ sessionId** で繋ぎ直された場合は検出できない(アンマウント中は状態の遷移を見られず、store に接続の世代番号が無い)。エンジンが接続ごとに sessionId を変えるなら問題にならない。確実にするなら store に接続ごとに増える世代番号を足す。
- 2026-09-23 開始儀式: main `872e406`(PR #1 のマージ)の tree は `f4d6644` と同一。`f4d6644` で `./scripts/verify.ps1 -Cpp` が exit 0 だった(fixtures 174、bridge cargo test 183、ctest 7/7、IPC 名 commands 28 / events 11、pnpm test 42/43/624)。`cargo test --manifest-path apps/editor/src-tauri/Cargo.toml` は 139 + 14 件が通過。Windows 11 で実行。
- ループは cmd.exe で `verify:` を実行する(`spawnSync(..., { shell: true })`)。verify に bash の構文を書かない。
- `pnpm lint` は実体が無い(どのパッケージにも lint スクリプトが無い)。緑でも何も検査していない。
- NorvesLib の作業ツリーは `feature/rendering-r4-ddgi` の未コミットの変更を抱えている。今回の主題では NorvesLib に触らない。
