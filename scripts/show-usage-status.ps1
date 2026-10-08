param(
    [string]$StatusPath = (Join-Path $env:LOCALAPPDATA 'RunDog\diagnostics\usage-status.json')
)

$ErrorActionPreference = 'Stop'

function Format-Cents([object]$Cents) {
    if ($null -eq $Cents) { return '—' }
    return '$' + ([decimal]$Cents / 100).ToString('F2', [Globalization.CultureInfo]::InvariantCulture)
}

if (-not (Test-Path -LiteralPath $StatusPath -PathType Leaf)) {
    throw "RunDog has not published a usage status yet: $StatusPath"
}

$status = Get-Content -LiteralPath $StatusPath -Raw | ConvertFrom-Json
if ($status.schema -ne 1 -or $status.pid -le 0 -or $status.captured_at_ms -le 0) {
    throw "Unsupported or incomplete RunDog usage status: $StatusPath"
}

$captured = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$status.captured_at_ms)
$age = [DateTimeOffset]::UtcNow - $captured
$process = Get-Process -Id ([int]$status.pid) -ErrorAction SilentlyContinue
if ($null -eq $process -or $process.ProcessName -ne 'RunDog' -or
    $process.StartTime.ToUniversalTime() -gt $captured.UtcDateTime.AddSeconds(1) -or
    $age.TotalSeconds -lt -5 -or $age.TotalSeconds -gt 120 -or
    [string]$status.day -ne (Get-Date -Format 'yyyyMMdd')) {
    throw "RunDog usage status is stale (PID $($status.pid), captured $($captured.ToLocalTime().ToString('yyyy-MM-dd HH:mm:ss')))."
}

foreach ($name in @('claude', 'codex')) {
    $provider = $status.$name
    if ($null -eq $provider -or $null -eq $provider.displayed_today_cents) {
        throw "RunDog usage status is missing $name Today."
    }
    $expected = [int64]$provider.local_today_cents
    if ($null -ne $provider.cache_today_cents) {
        $expected = [Math]::Max($expected, [int64]$provider.cache_today_cents)
    }
    if ([int64]$provider.displayed_today_cents -ne $expected) {
        throw "RunDog $name Today disagrees with its local/cache sources."
    }
}

$captureLabel = $captured.ToLocalTime().ToString('yyyy-MM-dd HH:mm:ss')
"RunDog PID $($status.pid) | captured $captureLabel | age $([math]::Floor($age.TotalSeconds))s"
foreach ($name in @('claude', 'codex')) {
    $provider = $status.$name
    $label = (Get-Culture).TextInfo.ToTitleCase($name)
    "$label Today: $(Format-Cents $provider.displayed_today_cents) (JSONL $(Format-Cents $provider.local_today_cents), otak-usage $(Format-Cents $provider.cache_today_cents))"
}
if ($null -ne $status.cache_snapshot_at_ms) {
    $cacheTime = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$status.cache_snapshot_at_ms)
    "otak-usage snapshot: $($cacheTime.ToLocalTime().ToString('yyyy-MM-dd HH:mm:ss')) | pending files: $($status.pending_files)"
} else {
    "otak-usage snapshot: unavailable | pending files: $($status.pending_files)"
}
