[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Version
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $repoRoot 'Cargo.toml'
$manifestVersionMatch = Select-String -LiteralPath $manifestPath -Pattern '^\s*version\s*=\s*"([^"]+)"' |
    Select-Object -First 1
if ($null -eq $manifestVersionMatch) {
    throw "Could not read package version from $manifestPath."
}

$manifestVersion = $manifestVersionMatch.Matches[0].Groups[1].Value
$releaseVersion = $Version.Trim()
if ($releaseVersion.StartsWith('v')) {
    $releaseVersion = $releaseVersion.Substring(1)
}
if ($releaseVersion -notmatch '^\d+\.\d+\.\d+$') {
    throw 'Version must be a stable MAJOR.MINOR.PATCH value or a v-prefixed tag.'
}
if ($releaseVersion -ne $manifestVersion) {
    throw "Release version $releaseVersion does not match Cargo.toml version $manifestVersion."
}

$updateRepository = $env:RUN_DOG_UPDATE_REPOSITORY
if ([string]::IsNullOrWhiteSpace($updateRepository)) {
    $updateRepository = 'tsuyoshi-otake/run-dog'
}
if ($updateRepository -notmatch '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$') {
    throw 'RUN_DOG_UPDATE_REPOSITORY must be a GitHub owner/repository slug.'
}

Push-Location $repoRoot
try {
    cargo build --release
}
finally {
    Pop-Location
}

$compilerCandidates = @($env:ISCC)
$localApplicationData = [Environment]::GetFolderPath('LocalApplicationData')
if ($localApplicationData) {
    $compilerCandidates += Join-Path $localApplicationData 'Programs\Inno Setup 6\ISCC.exe'
}
$programFilesX86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
if ($programFilesX86) {
    $compilerCandidates += Join-Path $programFilesX86 'Inno Setup 6\ISCC.exe'
}
if ($env:ProgramFiles) {
    $compilerCandidates += Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe'
}
$iscc = $compilerCandidates |
    Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } |
    Select-Object -First 1
if ($null -eq $iscc) {
    throw 'Inno Setup 6 compiler (ISCC.exe) was not found. Install Inno Setup 6 or set ISCC.'
}

$installerScript = Join-Path $repoRoot 'installer\RunDog.iss'
& $iscc "/DAppVersion=$releaseVersion" "/DUpdateRepository=$updateRepository" $installerScript
if ($LASTEXITCODE -ne 0) {
    throw "ISCC failed with exit code $LASTEXITCODE."
}

$outputDirectory = Join-Path $repoRoot 'dist'
$installerPath = Join-Path $outputDirectory 'RunDog-Setup-x64.exe'
if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) {
    throw "Inno Setup did not create $installerPath."
}

$checksum = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$checksumPath = "$installerPath.sha256"
[System.IO.File]::WriteAllText(
    $checksumPath,
    "$checksum  RunDog-Setup-x64.exe$([Environment]::NewLine)",
    [System.Text.Encoding]::ASCII
)

Write-Output "Installer: $installerPath"
Write-Output "Checksum: $checksumPath"
