# ビルドと検証

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## 標準ゲートランナー

`scripts/verify.ps1` が公式の集約ランナー。**ゲートを手動で並べるより先にこれを使うこと。**

```powershell
# 標準: fixtures + Rust のみ（C++ 未構成でも通る）
./scripts/verify.ps1

# C++ も含めて実行（build/cpp が cmake configure 済みの場合のみ有効）
./scripts/verify.ps1 -Cpp

# pnpm フロントエンドゲートをスキップ
./scripts/verify.ps1 -SkipFrontend
```

スクリプトは失敗したゲートで即座に停止し、非ゼロで終了する。
各セクションは「ゲートなし = SKIP」ではなく「ゲートあり = 実行」で設計されている。

## Cargo ワークスペース分割

- **ルートワークスペース** (`Cargo.toml`) は bridge クレート 4 本のみを管理する:
  - `bridge/crates/norves-bridge-core`
  - `bridge/crates/norves-bridge-editor-client`
  - `bridge/crates/norves-bridge-tools`
  - `bridge/crates/norves-bridge-dump`
- **`apps/editor/src-tauri`** は P1 以降に作成される独立した Cargo ワークスペースであり、ルートに `exclude` 済み。ルートワークスペースが自動吸収しないようにするための設定:
  ```toml
  [workspace]
  exclude = ["apps/editor/src-tauri"]
  ```
- `cargo fmt`/`cargo clippy`/`cargo test` の `--workspace` フラグはルートワークスペース（bridge クレートのみ）に作用する。

## pnpm ワークスペース

- ルート `package.json` は `"private": true` のワークスペースルート。アプリコードを持たない。
- `pnpm-workspace.yaml` が管理するパッケージグロブ: `apps/*`, `bridge/ts/packages/*`。
- パッケージが存在しない段階では `pnpm -r --if-present <script>` は "No projects matched" を出力してエラーにならない（正常）。
- `pnpm-lock.yaml` はコミット対象。`node_modules/` はコミット禁止。

## 個別コマンド（手動実行時の参考）

```powershell
# Protocol fixtures
python scripts/validate-bridge-fixtures.py

# Rust（bridge ワークスペースのみ）
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# C++ engine SDK / mock engine（build/cpp を cmake configure した後に実行）
cmake -S bridge/cpp -B build/cpp -G "Visual Studio 17 2022"
cmake --build build/cpp --config Debug
ctest --test-dir build/cpp -C Debug --output-on-failure

# TypeScript / フロントエンド（packages に package.json が存在する段階で有効）
pnpm -r --if-present typecheck
pnpm -r --if-present lint
pnpm -r --if-present build
pnpm -r --if-present test
```

## コミット禁止の生成物

生成物、ビルドディレクトリ、Tauri generated schema、node_modules、Cargo target、CMake build 出力はコミットしない。
具体的には `.gitignore` で管理済み: `node_modules/`, `.pnpm-store/`, `target/`, `build/`, `apps/editor/src-tauri/target/`, `apps/editor/src-tauri/gen/`。
`pnpm-lock.yaml` と `Cargo.lock` はコミット対象。
