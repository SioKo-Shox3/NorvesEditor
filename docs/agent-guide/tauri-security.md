# Tauri / Security 規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## Tauri / Security 規約

- Engine process launch は Rust backend が所有する。
- UI から任意コマンドを直接実行させない。
- Tauri shell/sidecar permissions は最小限にする。
- capabilities/default.json の permission 追加は計画レビュー必須。
- External binary / sidecar 設定を追加する場合は platform target triple と packaging impact を文書化する。
- Remote connection は alpha では無効。Default は localhost のみ。
- Secret / token / user-specific path はリポジトリにコミットしない。
