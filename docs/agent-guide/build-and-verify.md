# ビルドと検証

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## ビルドと検証

実際のコマンドはリポジトリ初期化時に確定する。確定後はこの節を更新すること。

想定される標準コマンド:

```powershell
# TypeScript / Tauri frontend
pnpm install
pnpm --filter @norves/editor dev
pnpm --filter @norves/editor build
pnpm lint
pnpm test

# Rust workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# C++ engine SDK / mock engine
cmake -S bridge/cpp -B build/cpp -G "Visual Studio 17 2022"
cmake --build build/cpp --config Debug
ctest --test-dir build/cpp -C Debug --output-on-failure
```

ルートに `scripts/verify.ps1` または同等の検証スクリプトを作成したら、標準ゲートはそれを優先する。

```powershell
./scripts/verify.ps1
```

生成物、ビルドディレクトリ、Tauri generated schema、node_modules、Cargo target、CMake build 出力はコミットしない。
