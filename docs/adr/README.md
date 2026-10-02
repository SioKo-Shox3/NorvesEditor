# アーキテクチャ決定記録（ADR）

ADR には、公開 API の形、プロトコル互換性、プロセスとセキュリティ権限、メモリ所有権、スレッド親和性、ビューポート方針、長期的なリポジトリ構成に影響する決定を記録する。

プロジェクト全体の計画は `docs/alpha-project-plan.md` に置き、ADR には将来の変更で不用意に覆さない個別の決定を記録する。

## 初期 ADR

```text
0001-editor-owned-bridge-subsystem.md
0002-tauri-rust-editor-backend.md
0003-websocket-json-bridge-control-channel.md
0004-cpp-engine-sdk.md
0005-external-engine-viewport-for-alpha.md
0006-bridge-error-model-and-versioning.md
0007-cpp-websocket-library-libwebsockets.md
0008-cpp23-and-norveslib-style-alignment.md
0009-cpp-bridge-namespace-pascalcase.md
0010-backend-edit-service-and-history.md
0011-local-mcp-interface.md
```

各 ADR には次を記録する。

```text
- 状態
- 背景
- 決定
- 帰結
- 影響する作業範囲
- 必要に応じて検証または移行の注記
```
