# 実mock必須のMCPサービス受入。未実行・SKIPを成功として扱わない。
param([string]$EnginePath = $env:NORVES_ENGINE_PATH)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($EnginePath)) {
    $EnginePath = Join-Path $repo 'build/cpp/examples/mock-engine/Debug/norves_mock_engine.exe'
}
$engine = Get-Item -LiteralPath $EnginePath -ErrorAction Stop
if ($engine.PSIsContainer) { throw 'mockの実行ファイルを指定してください。' }
$previous = $env:NORVES_ENGINE_PATH
$log = [System.IO.Path]::GetTempFileName()
$errorLog = [System.IO.Path]::GetTempFileName()
try {
    $env:NORVES_ENGINE_PATH = $engine.FullName
    # ネイティブstderrをPowerShell例外へ変換せず、終了コードと両出力を検査する。
    $manifest = Join-Path $repo 'apps/editor/src-tauri/Cargo.toml'
    $arguments = @('test', '--manifest-path', ('"' + $manifest + '"'), '--features', 'mcp-e2e', '--test', 'mcp_e2e', '--test', 'mcp_reads', '--', '--nocapture')
    $process = Start-Process -FilePath (Get-Command cargo -ErrorAction Stop).Source -ArgumentList $arguments -Wait -PassThru -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError $errorLog -ErrorAction Stop
    $code = $process.ExitCode
    $output = (Get-Content -LiteralPath $errorLog -Encoding UTF8 -Raw) + (Get-Content -LiteralPath $log -Encoding UTF8 -Raw)
    Write-Output $output
    if ($code -ne 0) { throw "MCP受入コマンドが失敗しました (exit $code)。" }
    if ($output -match '(?i)\bSKIP\b|[1-9][0-9]* ignored') { throw '未実行の試験があります。' }
    foreach ($marker in @('MCP_ENTRYPOINT_OK Discover', 'MCP_ENTRYPOINT_OK Initialize', 'MCP_ENTRYPOINT_OK shutdown')) {
        if (-not $output.Contains($marker)) { throw "受入証拠がありません: $marker" }
    }
    Write-Output '共通サービス入口の最小受入が成功しました（Discover / Initialize / 終了処理）。'
} finally {
    $env:NORVES_ENGINE_PATH = $previous
    Remove-Item -LiteralPath $log, $errorLog -ErrorAction Stop
}
