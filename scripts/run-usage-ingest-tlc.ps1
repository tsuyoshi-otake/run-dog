param(
    [ValidateSet('pass', 'mutations', 'all')]
    [string]$Mode = 'all'
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

function Invoke-Tlc([string]$config, [string]$expect) {
    $spec = Join-Path $formal 'RunDogUsageIngest.tla'
    $cfg = Join-Path $formal $config
    $metadata = Join-Path ([IO.Path]::GetTempPath()) ('rundog-ingest-tlc-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $metadata | Out-Null
    $output = & $java -XX:+UseParallelGC -cp $jar tlc2.TLC -workers 4 -metadir $metadata -config $cfg $spec 2>&1 | Out-String
    Write-Output $output
    $failed = $output -match 'Error: Invariant' -or $output -match 'Error: Temporal properties' -or $LASTEXITCODE -ne 0
    if ($expect -eq 'pass' -and $failed) {
        throw "$config expected PASS but TLC reported a violation."
    }
    if ($expect -eq 'fail' -and -not ($output -match 'Error: Invariant' -or $output -match 'Error: Temporal properties')) {
        throw "$config expected FAIL (model too weak if this stays green)."
    }
}

if ($Mode -eq 'pass' -or $Mode -eq 'all') {
    Invoke-Tlc 'RunDogUsageIngest.cfg' 'pass'
}
if ($Mode -eq 'mutations' -or $Mode -eq 'all') {
    Invoke-Tlc 'RunDogUsageIngestNoDedupe.cfg' 'fail'
    Invoke-Tlc 'RunDogUsageIngestCursorAhead.cfg' 'fail'
    Invoke-Tlc 'RunDogUsageIngestIncompleteCommit.cfg' 'fail'
    Invoke-Tlc 'RunDogUsageIngestStaleOverwrite.cfg' 'fail'
}
