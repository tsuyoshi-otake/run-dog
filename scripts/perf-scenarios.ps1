[CmdletBinding()]
param(
    [ValidateSet('list', 'smoke')]
    [string]$Mode = 'list',

    [ValidateRange(2, 180)]
    [int]$SmokeSeconds = 15
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

$scenarios = @(
    @{ Name = 'clean-first-launch'; Status = 'DEFINED'; Note = 'empty store; first catch-up' }
    @{ Name = 'warm-restart'; Status = 'SMOKE'; Note = 'unchanged files: usage_parse_bytes=0; integrity/limits-tail measured separately' }
    @{ Name = '1000-jsonl'; Status = 'DEFINED'; Note = 'synthetic 1000 complete records' }
    @{ Name = 'large-corpus'; Status = 'DEFINED'; Note = 'NOT RUN this turn; needs fixture volume' }
    @{ Name = '4kib-append'; Status = 'SMOKE'; Note = 'append-only after cursor' }
    @{ Name = 'month-rollover'; Status = 'DEFINED'; Note = 'cursor must not rewind' }
    @{ Name = 'catch-up-restart'; Status = 'DEFINED'; Note = 'mid catch-up persist then restart' }
    @{ Name = 'truncate'; Status = 'SMOKE'; Note = 'size shrink rebuilds from 0' }
    @{ Name = 'replace'; Status = 'SMOKE'; Note = 'same path new ids' }
    @{ Name = 'rebuild'; Status = 'DEFINED'; Note = 'last good generation stays visible' }
    @{ Name = '8h-soak'; Status = 'NOT RUN'; Note = 'harness only; do not run this turn' }
    @{ Name = 'vendor-api-unavailable'; Status = 'DEFINED'; Note = 'keep last good window' }
    @{ Name = 'no-claude-codex'; Status = 'DEFINED'; Note = 'no provider dirs; no fabricated 0%' }
)

Write-Output 'Phase K performance scenarios'
Write-Output 'Warm restart oracle is NOT "payload bytes = 0".'
Write-Output 'Measure UsageParseBytes, IntegrityProbeBytes, LimitsTailBytes, CheckpointBytes separately.'
Write-Output ''
$scenarios | ForEach-Object {
    '{0,-24} {1,-10} {2}' -f $_.Name, $_.Status, $_.Note
}

if ($Mode -ne 'smoke') {
    return
}

Write-Output ''
Write-Output "Smoke: cargo test usage_perf + measure.ps1 against PID $PID for ${SmokeSeconds}s"
Push-Location $repo
try {
    cargo test --offline --lib usage_perf
    if ($LASTEXITCODE -ne 0) {
        throw 'usage_perf tests failed'
    }
    $measure = Join-Path $repo 'scripts\measure.ps1'
    & $measure -ProcessId $PID -DurationSeconds $SmokeSeconds -IntervalMilliseconds 1000 | Out-Host
} finally {
    Pop-Location
}

Write-Output ''
Write-Output '8h-soak: NOT RUN'
