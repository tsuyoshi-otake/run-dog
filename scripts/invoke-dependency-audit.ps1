[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$ToolRoot,

    [Parameter(Mandatory)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$auditExe = Join-Path $ToolRoot 'bin/cargo-audit.exe'
$restored = Test-Path -LiteralPath $auditExe -PathType Leaf
$preparation = [Diagnostics.Stopwatch]::StartNew()

if (-not $restored) {
    cargo install cargo-audit --locked --version $Version --root $ToolRoot
    if ($LASTEXITCODE -ne 0) {
        throw "cargo-audit installation failed with exit code $LASTEXITCODE."
    }
}

$actualVersion = (& $auditExe --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $actualVersion -ne "cargo-audit $Version") {
    throw "Expected cargo-audit $Version; cached/installed executable reported '$actualVersion'."
}
$preparation.Stop()

# Cache only the exact executable. Never cache an audit result or the advisory
# database: each verification must fetch current advisories and run the audit.
$audit = [Diagnostics.Stopwatch]::StartNew()
& $auditExe audit
if ($LASTEXITCODE -ne 0) {
    throw "Dependency audit failed with exit code $LASTEXITCODE."
}
$audit.Stop()

$summary = 'cargo-audit {0}: restored={1}; preparation={2:F2}s; audit={3:F2}s' -f `
    $Version, $restored, $preparation.Elapsed.TotalSeconds, $audit.Elapsed.TotalSeconds
Write-Output $summary
if ($env:GITHUB_STEP_SUMMARY) {
    Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value $summary
}
