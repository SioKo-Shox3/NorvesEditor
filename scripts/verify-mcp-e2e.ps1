# 実mock必須のMCPサービス受入。未実行・SKIPを成功として扱わない。
param([string]$EnginePath = $env:NORVES_ENGINE_PATH)
$ErrorActionPreference = 'Stop'
# PowerShell 5.1の出力も、ランナーが読むUTF-8へ揃える。
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
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
    $arguments = @('test', '--manifest-path', ('"' + $manifest + '"'), '--features', 'mcp-e2e', '--lib', '--test', 'mcp_e2e', '--test', 'mcp_reads', '--', '--nocapture')
    $process = Start-Process -FilePath (Get-Command cargo -ErrorAction Stop).Source -ArgumentList $arguments -Wait -PassThru -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError $errorLog -ErrorAction Stop
    $code = $process.ExitCode
    $output = (Get-Content -LiteralPath $errorLog -Encoding UTF8 -Raw) + (Get-Content -LiteralPath $log -Encoding UTF8 -Raw)
    Write-Output $output
    if ($code -ne 0) { throw "MCP受入コマンドが失敗しました (exit $code)。" }
    if ($output -match '(?i)\bSKIP\b|[1-9][0-9]* ignored') { throw '未実行の試験があります。' }
    foreach ($marker in @(
        'MCP_ENTRYPOINT_OK Discover', 'MCP_ENTRYPOINT_OK Initialize', 'MCP_ENTRYPOINT_OK shutdown',
        'MCP_ACCEPTANCE_OK current-listen-notification', 'MCP_ACCEPTANCE_OK legacy-peer-notification',
        'MCP_ACCEPTANCE_OK authentication-origin-host', 'MCP_ACCEPTANCE_OK disabled',
        'MCP_ACCEPTANCE_OK reads-paging-logs-image', 'MCP_ACCEPTANCE_OK scope-group-remap-ui-undo',
        'MCP_ACCEPTANCE_OK mandatory-confirmation-delete-history', 'MCP_ACCEPTANCE_OK reconnect-cursor-group',
        'MCP_ACCEPTANCE_OK token-regeneration'
    )) {
        if (-not $output.Contains($marker)) { throw "受入証拠がありません: $marker" }
    }
    # 一方の版だけの成功や試験の削除を完了として扱わない。
    foreach ($scenario in @('reads-paging-logs-image', 'scope-group-remap-ui-undo', 'mandatory-confirmation-delete-history', 'reconnect-cursor-group', 'token-regeneration')) {
        if ([regex]::Matches($output, "MCP_ACCEPTANCE_OK $scenario").Count -ne 2) {
            throw "両版の受入証拠が揃っていません: $scenario"
        }
    }
    foreach ($test in @(
        'http_lifetime_both_protocols_enforce_30_and_125_seconds_with_paused_time',
        'http_lifetime_cancellation_is_scoped_by_typed_id_and_legacy_session',
        'both_http_versions_dispatch_writes_and_return_structured_outcomes',
        'rejected_single_mcp_undo_is_pending_until_ui_discards_it',
        'redo_remaps_created_ids_across_children_and_resumes_after_known_rejection',
        'real_mock_subscription_burst_is_retained_without_a_ui_or_mcp_client'
    )) {
        if ($output -notmatch ([regex]::Escape($test) + ' \.\.\. ok')) { throw "境界試験の成功証拠がありません: $test" }
    }
    Write-Output 'MCP受入が成功しました（実mock、Discover / Initialize、両版の通知、Rust境界試験）。'
} finally {
    $env:NORVES_ENGINE_PATH = $previous
    Remove-Item -LiteralPath $log, $errorLog -ErrorAction Stop
}
