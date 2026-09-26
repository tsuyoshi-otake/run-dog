#requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$CurrentInstaller,
    [Parameter(Mandatory)] [ValidatePattern('^\d+\.\d+\.\d+$')] [string]$CurrentVersion,
    [Parameter(Mandatory)] [string]$BaselineInstaller,
    [Parameter(Mandatory)] [ValidatePattern('^\d+\.\d+\.\d+$')] [string]$BaselineVersion,
    [Parameter(Mandatory)] [string]$EvidenceDirectory
)

$ErrorActionPreference = 'Stop'

# This script intentionally operates on HKCU and the current user's app data.
# Check the host and the entire ownership boundary before creating even evidence.
function Assert-HostedRunner {
    if ($env:CI -ne 'true' -or $env:GITHUB_ACTIONS -ne 'true' -or
        $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:RUNNER_OS -ne 'Windows') {
        throw 'Installer lifecycle tests require a GitHub-hosted Windows Actions runner.'
    }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    if ($env:USERNAME -ine 'runneradmin' -or $identity -notmatch '(?i)\\runneradmin$' -or
        $env:USERPROFILE -notmatch '(?i)[\\/]runneradmin$') {
        throw "Refusing to run under account '$identity'; only the disposable runneradmin account is allowed."
    }
    if (-not [IO.Path]::IsPathFullyQualified($env:USERPROFILE)) {
        throw 'USERPROFILE must be an absolute runner-owned directory.'
    }
}

function Assert-EqualPath([string]$Actual, [string]$Expected, [string]$What) {
    if ([IO.Path]::GetFullPath($Actual).TrimEnd('\') -ine
        [IO.Path]::GetFullPath($Expected).TrimEnd('\')) {
        throw "$What mismatch: '$Actual' != '$Expected'."
    }
}

function Get-RunDogProcesses {
    @(Get-CimInstance Win32_Process -Filter "Name='RunDog.exe'" -ErrorAction Stop)
}

function Get-UninstallEntries {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey(
        'Software\Microsoft\Windows\CurrentVersion\Uninstall')
    if ($null -eq $key) { return @() }
    try {
        $matches = @()
        foreach ($name in $key.GetSubKeyNames()) {
            $entry = $key.OpenSubKey($name)
            if ($null -ne $entry) {
                try {
                    if ($entry.GetValue('DisplayName') -eq 'RunDog') {
                        $matches += [pscustomobject]@{
                            Key = $name
                            Version = [string]$entry.GetValue('DisplayVersion')
                            Location = [string]$entry.GetValue('InstallLocation')
                        }
                    }
                } finally { $entry.Dispose() }
            }
        }
        return $matches
    } finally { $key.Dispose() }
}

function Assert-RegistryAbsent {
    foreach ($name in @('Software\SystemExe\RunDog')) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($name)
        if ($null -ne $key) {
            $key.Dispose()
            throw "Existing RunDog registry key: HKCU\$name."
        }
    }
    $run = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey(
        'Software\Microsoft\Windows\CurrentVersion\Run')
    if ($null -ne $run) {
        try {
            if ($null -ne $run.GetValue('RunDog')) {
                throw 'Existing HKCU RunDog startup value.'
            }
        } finally { $run.Dispose() }
    }
}

function Assert-CleanState {
    foreach ($path in @($script:appDir, $script:ownedData, $script:legacyData,
            $script:startShortcut, $script:desktopShortcut)) {
        if (Test-Path -LiteralPath $path) { throw "Existing RunDog path: $path" }
    }
    Assert-RegistryAbsent
    if (@(Get-UninstallEntries).Count -ne 0) { throw 'Existing RunDog uninstall entry.' }
    if (@(Get-RunDogProcesses).Count -ne 0) { throw 'An existing RunDog process is running.' }
}

function Seed-DisabledAutoUpdate {
    # src/core/settings.rs SettingsRecord::encode v4, using default settings.
    # A malformed record would fall back to auto_update=1, so verify the value.
    $record = "rundog-settings-4`ngeneration=0`noperation_id=0`ntheme=system`nfps=40`nstartup=0`ndisplay=dog`nauto_update=0`n"
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\SystemExe\RunDog')
    try {
        $key.SetValue('SettingsRecord', $record, [Microsoft.Win32.RegistryValueKind]::String)
        if ($key.GetValue('SettingsRecord') -cne $record) {
            throw 'The auto-update settings record did not persist.'
        }
    } finally { $key.Dispose() }
}

function Add-Evidence([string]$Step, [string]$Detail) {
    $script:summary.steps += [pscustomobject]@{
        step = $Step; detail = $Detail; utc = [DateTime]::UtcNow.ToString('o')
    }
    Add-Content -LiteralPath $script:logPath -Value "$([DateTime]::UtcNow.ToString('o')) $Step $Detail"
    Write-Host "$Step : $Detail"
}

function Invoke-BoundedProcess([string]$Exe, [string[]]$Arguments,
                               [string]$Step, [int]$TimeoutSeconds) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Exe
    $start.UseShellExecute = $false
    $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $start.WorkingDirectory = [IO.Path]::GetDirectoryName($Exe)
    foreach ($argument in $Arguments) { [void]$start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw "$Step did not start." }
        $script:summary.launched += [pscustomobject]@{ step = $Step; pid = $process.Id }
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            try { $process.Kill($true) } catch { }
            [void]$process.WaitForExit(10000)
            throw "$Step timed out after $TimeoutSeconds seconds."
        }
        if ($process.ExitCode -ne 0) {
            throw "$Step exited with code $($process.ExitCode)."
        }
        Add-Evidence $Step "exit=0 pid=$($process.Id)"
    } finally { $process.Dispose() }
}

function Wait-ForInstance([int]$TimeoutSeconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $processes = @(Get-RunDogProcesses)
        if ($processes.Count -eq 1 -and $processes[0].ExecutablePath) {
            Assert-EqualPath $processes[0].ExecutablePath $script:appExe 'RunDog process image'
            return $processes[0]
        }
        if ($processes.Count -gt 1) { throw "Expected one RunDog process; found $($processes.Count)." }
        Start-Sleep -Milliseconds 500
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Exactly one installed RunDog instance did not appear within $TimeoutSeconds seconds."
}

function Assert-Shortcuts([bool]$RequireDesktop) {
    $shell = New-Object -ComObject WScript.Shell
    try {
        $paths = @($script:startShortcut)
        if ($RequireDesktop) { $paths += $script:desktopShortcut }
        foreach ($path in $paths) {
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                throw "Missing current-user shortcut: $path"
            }
            $shortcut = $null
            try {
                $shortcut = $shell.CreateShortcut($path)
                Assert-EqualPath $shortcut.TargetPath $script:appExe "Shortcut target $path"
                Assert-EqualPath $shortcut.WorkingDirectory $script:appDir "Shortcut working directory $path"
            } finally {
                if ($null -ne $shortcut) {
                    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcut)
                }
            }
        }
        if (-not $RequireDesktop -and (Test-Path -LiteralPath $script:desktopShortcut)) {
            throw 'Baseline installer created a desktop shortcut despite /TASKS=.'
        }
    } finally {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
    }
    Add-Evidence 'shortcuts' "Required current-user shortcuts verified; desktop_required=$RequireDesktop."
}

function Assert-Installed([string]$Version, [bool]$RequireDesktop = $true) {
    if (-not (Test-Path -LiteralPath $script:appExe -PathType Leaf)) {
        throw "Installed executable is missing: $script:appExe"
    }
    $entries = @(Get-UninstallEntries)
    if ($entries.Count -ne 1) { throw "Expected one RunDog uninstall entry; found $($entries.Count)." }
    if ($entries[0].Version -ne $Version) {
        throw "Installed version '$($entries[0].Version)' is not '$Version'."
    }
    $fileVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($script:appExe).FileVersion
    if ($fileVersion -ne $Version) {
        throw "RunDog.exe FileVersion '$fileVersion' is not '$Version'."
    }
    if ($entries[0].Location) {
        Assert-EqualPath $entries[0].Location $script:appDir 'Install location'
    }
    Assert-Shortcuts $RequireDesktop
    $instance = Wait-ForInstance 20
    Add-Evidence 'installed' "version=$Version resident_pid=$($instance.ProcessId)"
}

function Invoke-Installer([string]$Installer, [string]$Label, [bool]$DeselectTasks) {
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
        '/CLOSEAPPLICATIONS', "/LOG=$($script:evidenceDir)\$Label-inno.log")
    if ($DeselectTasks) { $arguments += '/TASKS=' }
    Add-Evidence $Label 'Starting installer with a 120-second bound.'
    Invoke-BoundedProcess $Installer $arguments $Label 120
}

function Invoke-DuplicateShortcut {
    # ShellExecute the .lnk itself, after checking its target and working
    # directory. The mutex owner is already resident, so it must return.
    $original = Wait-ForInstance 5
    $start = [Diagnostics.ProcessStartInfo]::new($script:desktopShortcut)
    $start.UseShellExecute = $true
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw 'Duplicate shortcut target did not start.' }
        $script:summary.launched += [pscustomobject]@{ step = 'duplicate-desktop-shortcut-launch'; pid = $process.Id }
        if (-not $process.WaitForExit(10000)) {
            try { $process.Kill($true) } catch { }
            throw 'Duplicate RunDog launch did not exit within 10 seconds.'
        }
        if ($process.ExitCode -ne 0) {
            throw "Duplicate RunDog launch returned $($process.ExitCode)."
        }
        $instance = Wait-ForInstance 5
        if ($instance.ProcessId -ne $original.ProcessId) {
            throw "Duplicate launch replaced original resident PID $($original.ProcessId) with $($instance.ProcessId)."
        }
        Add-Evidence 'duplicate-desktop-shortcut-launch' "exit=0 original_resident_pid=$($original.ProcessId) survived"
    } finally { $process.Dispose() }
}

function Add-OwnedState {
    $usage = Join-Path $script:ownedData 'usage'
    $legacyUsage = Join-Path $script:legacyData 'usage'
    $updates = Join-Path $script:legacyData 'updates'
    foreach ($path in @($usage, $legacyUsage, $updates)) {
        [void][IO.Directory]::CreateDirectory($path)
        [IO.File]::WriteAllText((Join-Path $path 'lifecycle-sentinel.txt'), 'owned-test-state')
    }
    $settings = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey(
        'Software\SystemExe\RunDog\LifecycleHarness')
    try { $settings.SetValue('Probe', 'owned-test-state') } finally { $settings.Dispose() }
    $run = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey(
        'Software\Microsoft\Windows\CurrentVersion\Run')
    try { $run.SetValue('RunDog', $script:appExe) } finally { $run.Dispose() }
    Add-Evidence 'owned-state' 'Created disposable usage, legacy usage, update cache, settings and startup fixtures.'
}

function Close-ResidentAndCaptureDiagnostics([string]$Label) {
    # RunDog's hidden top-level window leaves WM_CLOSE to DefWindowProc, which
    # destroys it. WM_DESTROY ends the message loop and records diagnostics.
    if (-not ('RunDogInstallerHarnessWindow' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class RunDogInstallerHarnessWindow {
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr FindWindow(string className, string windowName);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
}
'@
    }
    $resident = Wait-ForInstance 5
    $residentId = [int]$resident.ProcessId
    $window = [RunDogInstallerHarnessWindow]::FindWindow('SystemExe.RunDog.MessageWindow', $null)
    if ($window -eq [IntPtr]::Zero) { throw 'RunDog message window was not found for graceful exit.' }
    [uint32]$windowProcessId = 0
    [void][RunDogInstallerHarnessWindow]::GetWindowThreadProcessId($window, [ref]$windowProcessId)
    if ($windowProcessId -ne $residentId) {
        throw "RunDog window PID $windowProcessId does not match owned PID $residentId."
    }
    $process = [Diagnostics.Process]::GetProcessById($residentId)
    try {
        if (-not [RunDogInstallerHarnessWindow]::PostMessage(
                $window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
            throw "Could not post WM_CLOSE to owned RunDog PID $residentId."
        }
        if (-not $process.WaitForExit(20000)) {
            throw "RunDog PID $residentId did not exit gracefully within 20 seconds."
        }
        if ($process.ExitCode -ne 0) {
            throw "RunDog PID $residentId returned $($process.ExitCode) on graceful exit."
        }
    } finally { $process.Dispose() }

    $terminationLog = Join-Path $script:ownedData 'diagnostics\termination.log'
    if (-not (Test-Path -LiteralPath $terminationLog -PathType Leaf)) {
        throw "Graceful exit produced no termination log: $terminationLog"
    }
    $contents = Get-Content -LiteralPath $terminationLog -Raw
    if ($contents -notmatch "event=usage_diagnostics\s+run=\S+\s+pid=$residentId\b" -or
        $contents -notmatch "event=clean_exit\s+run=\S+\s+pid=$residentId\b") {
        throw "Graceful exit of PID $residentId has no matching usage_diagnostics and clean_exit records."
    }
    $destination = Join-Path $script:evidenceDir "termination-$Label.log"
    Copy-Item -LiteralPath $terminationLog -Destination $destination
    Add-Evidence 'graceful-exit' "pid=$residentId diagnostics_captured=$destination"
}

function Invoke-Uninstall([string]$Label) {
    $uninstaller = Join-Path $script:appDir 'unins000.exe'
    if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
        throw "Uninstaller missing: $uninstaller"
    }
    Invoke-BoundedProcess $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
        "/LOG=$($script:evidenceDir)\$Label-uninstall.log") $Label 90
}

function Assert-Uninstalled {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ((Test-Path -LiteralPath $script:appDir) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 500
    }
    foreach ($path in @($script:appDir, $script:ownedData, $script:legacyData,
            $script:startShortcut, $script:desktopShortcut)) {
        if (Test-Path -LiteralPath $path) { throw "Uninstall left RunDog path: $path" }
    }
    Assert-RegistryAbsent
    if (@(Get-UninstallEntries).Count -ne 0) { throw 'Uninstall left a RunDog Apps entry.' }
    if (@(Get-RunDogProcesses).Count -ne 0) { throw 'Uninstall left a RunDog process.' }
    foreach ($path in $script:preservedSentinels) {
        if (([IO.File]::ReadAllText($path)) -cne 'preserve-me') {
            throw "Uninstall modified unrelated sentinel: $path"
        }
    }
    Add-Evidence 'uninstalled' 'RunDog paths, registry, shortcuts, Apps entry and process absent; unrelated sentinels preserved.'
}

function Stop-OnlyOwnedProcesses {
    foreach ($item in @(Get-RunDogProcesses)) {
        if ($item.ExecutablePath -and
            [IO.Path]::GetFullPath($item.ExecutablePath).TrimEnd('\') -ieq
            [IO.Path]::GetFullPath($script:appExe).TrimEnd('\')) {
            $process = Get-Process -Id $item.ProcessId -ErrorAction SilentlyContinue
            if ($null -ne $process) {
                try { $process.Kill($true); [void]$process.WaitForExit(10000) } catch { }
                finally { $process.Dispose() }
            }
        }
    }
}

Assert-HostedRunner
if ([version]$BaselineVersion -ge [version]$CurrentVersion) {
    throw 'BaselineVersion must be older than CurrentVersion.'
}
$currentFile = (Resolve-Path -LiteralPath $CurrentInstaller -ErrorAction Stop).Path
$baselineFile = (Resolve-Path -LiteralPath $BaselineInstaller -ErrorAction Stop).Path
foreach ($path in @($currentFile, $baselineFile)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Installer missing: $path" }
}
$scratchRoot = [IO.Path]::GetFullPath((Join-Path $env:USERPROFILE 'tmp\run-dog-installer')).TrimEnd('\')
$evidenceDir = [IO.Path]::GetFullPath($EvidenceDirectory).TrimEnd('\')
if (-not $evidenceDir.StartsWith("$scratchRoot\", [StringComparison]::OrdinalIgnoreCase)) {
    throw 'EvidenceDirectory must be a child of ~/tmp/run-dog-installer.'
}
if ((Test-Path -LiteralPath $evidenceDir) -and
    @(Get-ChildItem -LiteralPath $evidenceDir -Force).Count -gt 0) {
    throw 'EvidenceDirectory must be new or empty.'
}

$local = [Environment]::GetFolderPath('LocalApplicationData')
$appDir = Join-Path $local 'Programs\RunDog'
$appExe = Join-Path $appDir 'RunDog.exe'
$ownedData = Join-Path $local 'RunDog'
$legacyData = Join-Path $local 'SystemExe\RunDog'
$startShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'RunDog.lnk'
$desktopShortcut = Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) 'RunDog.lnk'
Assert-CleanState

[void][IO.Directory]::CreateDirectory($evidenceDir)
$logPath = Join-Path $evidenceDir 'lifecycle.log'
$summary = [ordered]@{
    status = 'running'; currentVersion = $CurrentVersion; baselineVersion = $BaselineVersion
    steps = @(); launched = @(); error = $null
}
$preservedSentinels = @()
$started = $false
try {
    # Set the provider homes before any app process starts. These contain no real
    # provider data or credentials and must survive both uninstall scenarios.
    $fakeRoot = Join-Path $scratchRoot 'provider-sentinels'
    if (Test-Path -LiteralPath $fakeRoot) { throw "Existing fixture root: $fakeRoot" }
    $env:CLAUDE_CONFIG_DIR = Join-Path $fakeRoot 'claude'
    $env:CODEX_HOME = Join-Path $fakeRoot 'codex'
    $otherProduct = Join-Path $local 'SystemExe\OtherProduct'
    if (Test-Path -LiteralPath $otherProduct) { throw "Existing other-product path: $otherProduct" }
    foreach ($dir in @($env:CLAUDE_CONFIG_DIR, $env:CODEX_HOME, $otherProduct)) {
        [void][IO.Directory]::CreateDirectory($dir)
        $sentinel = Join-Path $dir 'preserve-me.txt'
        [IO.File]::WriteAllText($sentinel, 'preserve-me')
        $preservedSentinels += $sentinel
    }
    Add-Evidence 'preflight' 'Clean disposable hosted profile and isolated provider/product sentinels confirmed.'

    Seed-DisabledAutoUpdate
    $started = $true
    Invoke-Installer $currentFile 'clean-current' $false
    Assert-Installed $CurrentVersion
    Invoke-DuplicateShortcut
    Invoke-Installer $currentFile 'resident-reinstall' $false
    Assert-Installed $CurrentVersion
    Add-OwnedState
    Close-ResidentAndCaptureDiagnostics 'current-uninstall'
    Invoke-Uninstall 'current-uninstall'
    Assert-Uninstalled

    Seed-DisabledAutoUpdate
    Invoke-Installer $baselineFile 'clean-baseline' $true
    Assert-Installed $BaselineVersion $false
    Invoke-Installer $currentFile 'resident-upgrade' $false
    Assert-Installed $CurrentVersion
    Add-OwnedState
    Close-ResidentAndCaptureDiagnostics 'upgrade-uninstall'
    Invoke-Uninstall 'upgrade-uninstall'
    Assert-Uninstalled
    $summary.status = 'passed'
} catch {
    $summary.status = 'failed'
    $summary.error = $_.Exception.Message
    Add-Evidence 'failure' $_.Exception.Message
    throw
} finally {
    try {
        if ($started -and $summary.status -ne 'passed') {
            # The host was clean at entry. These PIDs can only belong to this run.
            Stop-OnlyOwnedProcesses
            $uninstaller = Join-Path $appDir 'unins000.exe'
            if (Test-Path -LiteralPath $uninstaller -PathType Leaf) {
                try { Invoke-Uninstall 'failure-cleanup' } catch {
                    Add-Evidence 'cleanup-failure' $_.Exception.Message
                }
            }
            Stop-OnlyOwnedProcesses
        }
    } catch {
        Add-Evidence 'cleanup-failure' $_.Exception.Message
    } finally {
        $summary.finishedUtc = [DateTime]::UtcNow.ToString('o')
        $summary | ConvertTo-Json -Depth 8 |
            Set-Content -LiteralPath (Join-Path $evidenceDir 'summary.json') -Encoding utf8
    }
}
