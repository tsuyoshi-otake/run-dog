param(
    [ValidateSet('pass', 'mutations', 'all')]
    [string]$Mode = 'all',
    [ValidateSet('ingest', 'scheduling', 'all')]
    [string]$Model = 'all'
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$formal = Join-Path $repo 'formal'
$jar = $env:RUN_DOG_TLA2TOOLS
if ([string]::IsNullOrWhiteSpace($jar)) {
    $jar = Join-Path $repo 'verification\tools\tla2tools.jar'
}
$java = $env:RUN_DOG_JAVA
if ([string]::IsNullOrWhiteSpace($java)) {
    $java = 'java'
}

if (-not (Test-Path -LiteralPath $jar)) {
    throw "tla2tools.jar not found. Set RUN_DOG_TLA2TOOLS or place the jar at verification/tools/tla2tools.jar"
}

function Invoke-Tlc([string]$module, [string]$config, [string]$expect, [string]$violation = '') {
    $spec = Join-Path $formal "$module.tla"
    $cfg = Join-Path $formal $config
    $metadata = Join-Path ([IO.Path]::GetTempPath()) ('rundog-usage-tlc-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $metadata | Out-Null
    try {
        $output = & $java '-XX:+UseParallelGC' -cp $jar tlc2.TLC -workers 4 -metadir $metadata -config $cfg $spec 2>&1 | Out-String
    }
    finally {
        [IO.Directory]::Delete($metadata, $true)
    }
    Write-Output $output
    $failed = $output -match 'Error: Invariant' -or $output -match 'Error: Temporal properties' -or $LASTEXITCODE -ne 0
    if ($expect -eq 'pass' -and $failed) {
        throw "$config expected PASS but TLC reported a violation."
    }
    if ($expect -eq 'fail' -and -not ($output -match 'Error: Invariant' -or $output -match 'Error: Temporal properties')) {
        throw "$config expected FAIL (model too weak if this stays green)."
    }
    if ($expect -eq 'fail' -and $violation -ne '' -and $output -notmatch [regex]::Escape($violation)) {
        throw "$config did not violate the expected property $violation."
    }
}

if (($Mode -eq 'pass' -or $Mode -eq 'all') -and ($Model -eq 'ingest' -or $Model -eq 'all')) {
    Invoke-Tlc 'RunDogUsageIngest' 'RunDogUsageIngest.cfg' 'pass'
    Invoke-Tlc 'RunDogUsageIngest' 'RunDogUsageCatchUp.cfg' 'pass'
}
if (($Mode -eq 'pass' -or $Mode -eq 'all') -and ($Model -eq 'scheduling' -or $Model -eq 'all')) {
    Invoke-Tlc 'RunDogUsageScheduling' 'RunDogUsageScheduling.cfg' 'pass'
}
if (($Mode -eq 'mutations' -or $Mode -eq 'all') -and ($Model -eq 'ingest' -or $Model -eq 'all')) {
    Invoke-Tlc 'RunDogUsageIngest' 'RunDogUsageIngestNoDedupe.cfg' 'fail'
    Invoke-Tlc 'RunDogUsageIngest' 'RunDogUsageIngestCursorAhead.cfg' 'fail'
    Invoke-Tlc 'RunDogUsageIngest' 'RunDogUsageIngestIncompleteCommit.cfg' 'fail'
    Invoke-Tlc 'RunDogUsageIngest' 'RunDogUsageIngestStaleOverwrite.cfg' 'fail'
}
if (($Mode -eq 'mutations' -or $Mode -eq 'all') -and ($Model -eq 'scheduling' -or $Model -eq 'all')) {
    Invoke-Tlc 'RunDogUsageScheduling' 'RunDogUsageSchedulingPrefixFault.cfg' 'fail' 'TodayWithinOneTick'
    Invoke-Tlc 'RunDogUsageScheduling' 'RunDogUsageSchedulingNoReserve.cfg' 'fail' 'TodayWithinOneTick'
}
