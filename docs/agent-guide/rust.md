# Rust 規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## Rust 規約

対象:

```text
apps/editor/src-tauri/src/
bridge/crates/**
```

規則:

```text
- `cargo fmt` と `cargo clippy -- -D warnings` を通す。
- Library code で `unwrap()` / `expect()` を使わない。テスト、例、明確に失敗即 abort する初期化は例外だが理由をコメントする。
- Error type は library では `thiserror` 等で型を定義し、binary/tool では `anyhow` を使用可。
- Logging は `tracing` を使う。`println!` は CLI output、examples、tests 以外では避ける。
- Async task では blocking I/O を直接行わない。必要なら `spawn_blocking` または dedicated task に分離する。
- Tokio channel は原則 bounded。unbounded を使う場合は計画に理由を書く。
- Long-lived task は shutdown path を持つ。
- Tauri command 引数・戻り値・event payload は `serde` serialize/deserialize 可能な DTO にする。
- Frontend-facing DTO は `camelCase` を基本とする。Wire protocol は schema に従う。
- Panic が FFI / Tauri command / task boundary に漏れないようにする。
```

## エンジンプロセスライフサイクル（Workstream J）

`apps/editor/src-tauri/src/process.rs`（純粋ロジック）と `src/process_runtime.rs`（I/O グルー）が担当する。

### 所有権と起動経路

バックエンド（Rust）がエンジンプロセスのライフサイクル全体を所有する。UI はプロセスを直接起動しない。フロントエンドが実行可能ファイルのパスを渡すことは一切ない。

- 実行パスの解決: `NORVES_ENGINE_PATH` 環境変数（優先） → config 値 → デフォルト `norves_mock_engine`。バックエンドが `process::resolve_engine_path` で決定し、`process::validate_engine_path` で `is_file()` を検証してから起動する。
- ポート注入: バックエンドが `std::net::TcpListener::bind("127.0.0.1:0")` で空きポートを確保し、`drop` 後にそのポートを `--bridge-port <port>` としてエンジンに渡す。
- READY ハンドシェイク: バックエンドはエンジンの stdout から `READY <port>` 行を読み、タイムアウト（10 秒）内に受け取れなければエンジンを kill してエラーを返す。`parse_ready_line` が注入ポートと一致するか純粋に検証する。

### 停止・終了シグナル契約

1 つの原因に対して UI に届くシグナルは必ず 1 つだけ。2 つが同時に届くことはない。

- **ユーザー起動の停止**（`stop_engine`）: `connection_state` イベント（reason `"engine stopped"`）を 1 回だけ発行し、`engine.processExited` は**意図的に抑制する**。抑制は構造的: `stop_engine` がホルダーを `None` にしてから kill を要求するため、モニタータスクのジェネレーションガードがホルダー不在を検出して emit しない。
- **非依頼のエンジン死亡**（クラッシュ・外部 kill）: ホルダーがまだ設定されているためモニタータスクがジェネレーションガードを通過し、スペック既存の `engine.processExited` イベントを 1 回だけ合成・発行する。

### オーファンリスク（正直な注記）

`kill_on_drop(true)` と `RunEvent::ExitRequested` / `Exit` フック（`kill_engine_on_exit`）はベストエフォートであり、保証ではない。エディターが強制終了（SIGKILL・電源断）された場合はどちらも実行されず、エンジン子プロセスがオーファンになり得る。Windows Job Object / POSIX プロセスグループによる強化は alpha 後に持ち越し。

### テスト戦略

| 層 | カバレッジ手段 |
|---|---|
| 純粋ロジック（`process.rs`） | J1 ユニットテスト（`resolve_engine_path`, `parse_ready_line`, `pick_free_port`, `monitor_should_emit_exit`, `build_process_exited_params` 等） |
| 外部エンジン起動 / kill / 再起動契約 | 環境変数ゲート付き e2e `apps/editor/src-tauri/tests/process_e2e.rs`（`NORVES_ENGINE_PATH` 設定時のみ実行） |
| Tauri コマンド層（`process_runtime.rs`） | J1 ユニットテスト（`alloc_process_gen`, `exit_code_and_signal` 等の純粋関数）＋ J3 実装レビュー＋ §10 手動 GUI 検収 |

**Tauri コマンド層の自動 e2e が実装されていない理由**: `launch_engine` / `stop_engine` は具体型 `AppHandle<Wry>` を引数に取る（`#[default_runtime]` が裸の `AppHandle` を `AppHandle<Wry>` に解決する）。`MockRuntime` アプリのハンドルは型不一致となり、`tauri::test` を使うには実 Wry をリンクする必要があるが、Windows テストハーネスで WebView2Loader の読み込みに失敗（`STATUS_ENTRYPOINT_NOT_FOUND`）し、全 lib テストが壊れる。コマンドとリレーコードをランタイムジェネリックにする大規模リファクタは alpha には過剰なリスク。
