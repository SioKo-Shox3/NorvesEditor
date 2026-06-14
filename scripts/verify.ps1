# scripts/verify.ps1 — NorvesEditor quality-gate runner
#
# Usage:
#   ./scripts/verify.ps1              # Run fixtures + Rust gates; C++ skipped unless -Cpp; frontend runs if packages exist
#   ./scripts/verify.ps1 -Cpp         # Also run C++ cmake/ctest (requires build/cpp to be pre-configured)
#   ./scripts/verify.ps1 -SkipFrontend  # Skip pnpm frontend gates
#
# Notes:
#   - C++ section only runs when -Cpp is passed AND build/cpp exists.
#   - Frontend section uses "pnpm -r --if-present <script>", so zero workspace packages is not an error.
#   - Root workspace = bridge crates only. apps/editor/src-tauri is a separate Cargo workspace (excluded).
#   - The script stops on the first failure and exits non-zero.

param(
    [switch]$Cpp,
    [switch]$SkipFrontend
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
    # 3. C++ engine SDK / mock engine (opt-in via -Cpp; guarded on build/cpp)
    # -------------------------------------------------------------------------
    Write-Host ""
    Write-Host "=== [3/4] C++ ===" -ForegroundColor Yellow
    $CppBuildDir = Join-Path $RepoRoot "build/cpp"
    if ($Cpp) {
        if (-not (Test-Path $CppBuildDir)) {
            Write-Host "[SKIP] -Cpp requested but build/cpp does not exist." -ForegroundColor DarkYellow
            Write-Host "       Configure first: cmake -S bridge/cpp -B build/cpp -G 'Visual Studio 17 2022'" -ForegroundColor DarkYellow
        } else {
            Invoke-Gate "cmake build Debug" {
                Write-Host "    cmake --build $CppBuildDir --config Debug" -ForegroundColor DarkGray
                cmake --build $CppBuildDir --config Debug
            }
            Invoke-Gate "ctest Debug" {
                Write-Host "    ctest --test-dir $CppBuildDir -C Debug --output-on-failure" -ForegroundColor DarkGray
                ctest --test-dir $CppBuildDir -C Debug --output-on-failure
            }
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
