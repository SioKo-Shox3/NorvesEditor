# scripts/verify.ps1 — NorvesEditor の品質ゲートランナー
#
# 使用例:
#   ./scripts/verify.ps1              # fixtures + Rust を実行。C++ は -Cpp 時のみ、フロントエンドはパッケージがあれば実行
#   ./scripts/verify.ps1 -Cpp         # 未構成なら構成してから C++ のビルドと ctest を実行
#   ./scripts/verify.ps1 -Cpp -CppBuildDir $env:TEMP\norves-cpp  # C++ のビルドディレクトリを指定
#   ./scripts/verify.ps1 -SkipFrontend  # pnpm のフロントエンドゲートを省略
#
# 注意:
#   - C++ 段は -Cpp のときだけ実行する。未構成なら構成ゲートを先に実行し、構成失敗は全体の失敗とする。
#   - フロントエンド段は "pnpm -r --if-present <script>" を使うため、ワークスペースのパッケージが 0 件でもエラーにならない。
#   - ルートワークスペースは bridge クレートのみ。apps/editor/src-tauri は独立した Cargo ワークスペースで対象外。
#   - 最初の失敗で停止し、非 0 で終了する。

param(
    [switch]$Cpp,
    [switch]$SkipFrontend,
    [string]$CppBuildDir = 'build/cpp'
)

$ErrorActionPreference = 'Stop'

# Resolve repo root relative to this script's location
$RepoRoot = Split-Path -Parent $PSScriptRoot

function Invoke-Gate {
    param([string]$Description, [scriptblock]$Block)
    Write-Host ""
    Write-Host ">>> $Description" -ForegroundColor Cyan
    & $Block
    if ($LASTEXITCODE -ne 0) {
        Write-Host ""
        Write-Host "[FAIL] $Description exited with code $LASTEXITCODE" -ForegroundColor Red
        exit $LASTEXITCODE
    }
    Write-Host "[OK]   $Description" -ForegroundColor Green
}

Push-Location $RepoRoot
try {

    # -------------------------------------------------------------------------
    # 1. Protocol fixtures
    # -------------------------------------------------------------------------
    Write-Host ""
    Write-Host "=== [1/4] Protocol fixtures ===" -ForegroundColor Yellow
    Invoke-Gate "validate-bridge-fixtures" {
        Write-Host "    python scripts/validate-bridge-fixtures.py" -ForegroundColor DarkGray
        python scripts/validate-bridge-fixtures.py
    }

    # -------------------------------------------------------------------------
    # 2. Rust workspace (bridge crates only; apps/editor/src-tauri is excluded)
    # -------------------------------------------------------------------------
    Write-Host ""
    Write-Host "=== [2/4] Rust (bridge workspace) ===" -ForegroundColor Yellow
    Invoke-Gate "cargo fmt check" {
        Write-Host "    cargo fmt --all -- --check" -ForegroundColor DarkGray
        cargo fmt --all -- --check
    }
    Invoke-Gate "cargo clippy" {
        Write-Host "    cargo clippy --workspace --all-targets -- -D warnings" -ForegroundColor DarkGray
        cargo clippy --workspace --all-targets -- -D warnings
    }
    Invoke-Gate "cargo test" {
        Write-Host "    cargo test --workspace" -ForegroundColor DarkGray
        cargo test --workspace
    }

    # -------------------------------------------------------------------------
    # 3. C++ エンジン SDK / mock engine（-Cpp で有効化、未構成なら先に構成）
    # -------------------------------------------------------------------------
    Write-Host ""
    Write-Host "=== [3/4] C++ ===" -ForegroundColor Yellow
    if ([System.IO.Path]::IsPathRooted($CppBuildDir)) {
        $CppBuildPath = $CppBuildDir
    } else {
        $CppBuildPath = Join-Path $RepoRoot $CppBuildDir
    }
    if ($Cpp) {
        if (-not (Test-Path -LiteralPath (Join-Path $CppBuildPath 'CMakeCache.txt') -PathType Leaf)) {
            Invoke-Gate "cmake configure" {
                Write-Host "    cmake -S bridge/cpp -B `"$CppBuildPath`" -G `"Visual Studio 17 2022`"" -ForegroundColor DarkGray
                cmake -S bridge/cpp -B $CppBuildPath -G "Visual Studio 17 2022"
            }
        } else {
            Write-Host "構成済みのため CMake の構成を省略します。" -ForegroundColor DarkGray
        }
        Invoke-Gate "cmake build Debug" {
            Write-Host "    cmake --build `"$CppBuildPath`" --config Debug" -ForegroundColor DarkGray
            cmake --build $CppBuildPath --config Debug
        }
        Invoke-Gate "ctest Debug" {
            Write-Host "    ctest --test-dir `"$CppBuildPath`" -C Debug --output-on-failure" -ForegroundColor DarkGray
            ctest --test-dir $CppBuildPath -C Debug --output-on-failure
        }
    } else {
        Write-Host "[SKIP] C++ gate skipped (pass -Cpp to enable)" -ForegroundColor DarkYellow
    }

    # -------------------------------------------------------------------------
    # 3b. Protocol name cross-language equality guard (P3)
    # -------------------------------------------------------------------------
    Write-Host ""
    Write-Host "=== [3b] Protocol name cross-language check ===" -ForegroundColor Yellow
    Invoke-Gate "check-protocol-names" {
        Write-Host "    node scripts/check-protocol-names.mjs" -ForegroundColor DarkGray
        node scripts/check-protocol-names.mjs
    }

    # -------------------------------------------------------------------------
    # 4. Frontend (pnpm workspace; -r --if-present tolerates zero packages)
    # -------------------------------------------------------------------------
    Write-Host ""
    Write-Host "=== [4/4] Frontend (pnpm) ===" -ForegroundColor Yellow
    $PnpmWorkspace = Join-Path $RepoRoot "pnpm-workspace.yaml"
    if ($SkipFrontend) {
        Write-Host "[SKIP] Frontend gate skipped (-SkipFrontend)" -ForegroundColor DarkYellow
    } elseif (-not (Test-Path $PnpmWorkspace)) {
        Write-Host "[SKIP] pnpm-workspace.yaml not found; skipping frontend gates" -ForegroundColor DarkYellow
    } else {
        Invoke-Gate "pnpm typecheck" {
            Write-Host "    pnpm -r --if-present typecheck" -ForegroundColor DarkGray
            pnpm -r --if-present typecheck
        }
        Invoke-Gate "pnpm lint" {
            Write-Host "    pnpm -r --if-present lint" -ForegroundColor DarkGray
            pnpm -r --if-present lint
        }
        Invoke-Gate "pnpm build" {
            Write-Host "    pnpm -r --if-present build" -ForegroundColor DarkGray
            pnpm -r --if-present build
        }
        Invoke-Gate "pnpm test" {
            Write-Host "    pnpm -r --if-present test" -ForegroundColor DarkGray
            pnpm -r --if-present test
        }
    }

    Write-Host ""
    Write-Host "=== All requested gates passed ===" -ForegroundColor Green

} finally {
    Pop-Location
}
