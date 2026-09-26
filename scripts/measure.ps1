[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateRange(1, 2147483647)]
    [int]$ProcessId,

    [ValidateRange(2, 3600)]
    [int]$DurationSeconds = 60,

    [ValidateRange(100, 10000)]
    [int]$IntervalMilliseconds = 1000,

    # Used by perf-scenarios.ps1 before it starts the Cargo smoke tests.
    [switch]$ValidateTargetOnly
)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class RunDogGuiResources {
    [DllImport("user32.dll")]
    public static extern uint GetGuiResources(IntPtr process, uint flags);
}
"@ -ErrorAction SilentlyContinue

function Get-NearestRankP95([double[]]$values) {
    if ($values.Count -eq 0) { return 0 }
    $ordered = $values | Sort-Object
    $index = [Math]::Max(0, [Math]::Ceiling(0.95 * $ordered.Count) - 1)
    return [Math]::Round([double]$ordered[$index], 4)
}

function Get-Summary([double[]]$values) {
    if ($values.Count -eq 0) {
        return [PSCustomObject]@{ Avg = 0; P95 = 0; Max = 0 }
    }
    return [PSCustomObject]@{
        Avg = [Math]::Round((($values | Measure-Object -Average).Average), 4)
        P95 = Get-NearestRankP95 $values
        Max = [Math]::Round((($values | Measure-Object -Maximum).Maximum), 4)
    }
}

function Assert-RunDogTarget([int]$TargetId) {
    $repo = Split-Path -Parent $PSScriptRoot
    $manifest = Join-Path $repo 'Cargo.toml'
    $versionMatch = Select-String -LiteralPath $manifest -Pattern '^\s*version\s*=\s*"(\d+\.\d+\.\d+)"' |
        Select-Object -First 1
    if ($null -eq $versionMatch) {
        throw "Could not read the RunDog version from $manifest."
    }
    $expectedVersion = $versionMatch.Matches[0].Groups[1].Value

    $targetRoot = if ($env:CARGO_TARGET_DIR) {
        $configuredTarget = if ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
            $env:CARGO_TARGET_DIR
        } else {
            Join-Path $repo $env:CARGO_TARGET_DIR
        }
        [System.IO.Path]::GetFullPath($configuredTarget)
    } else {
        Join-Path $repo 'target'
    }
    $expectedPaths = @(
        Join-Path $targetRoot 'debug\RunDog.exe'
        Join-Path $targetRoot 'release\RunDog.exe'
    )
    $localAppData = [Environment]::GetFolderPath('LocalApplicationData')
    if ($localAppData) {
        $expectedPaths += Join-Path $localAppData 'Programs\RunDog\RunDog.exe'
    }

    $process = $null
    $validated = $false
    try {
        try {
            $process = Get-Process -Id $TargetId -ErrorAction Stop
            if ($process.HasExited) {
                throw 'The process has exited.'
            }
            $startTime = $process.StartTime
            $actualPath = [System.IO.Path]::GetFullPath($process.Path)
        } catch {
            throw "RunDog PID $TargetId is missing, exited, or its executable path is unavailable: $_"
        }

        $pathMatches = $false
        foreach ($expectedPath in $expectedPaths) {
            if ([string]::Equals($actualPath, [System.IO.Path]::GetFullPath($expectedPath),
                    [System.StringComparison]::OrdinalIgnoreCase)) {
                $pathMatches = $true
                break
            }
        }
        if (-not $pathMatches) {
            throw "PID $TargetId runs '$actualPath'; expected a RunDog executable at one of: $($expectedPaths -join ', ')."
        }

        $fileVersion = (Get-Item -LiteralPath $actualPath -ErrorAction Stop).VersionInfo
        if ($fileVersion.ProductName -ne 'RunDog' -or
            $fileVersion.InternalName -ne 'RunDog' -or
            $fileVersion.OriginalFilename -ne 'RunDog.exe' -or
            $fileVersion.FileVersion -ne $expectedVersion -or
            $fileVersion.ProductVersion -ne $expectedVersion) {
            throw "PID $TargetId runs '$actualPath' with product '$($fileVersion.ProductName)', file version '$($fileVersion.FileVersion)', product version '$($fileVersion.ProductVersion)'; expected RunDog $expectedVersion."
        }

        $process.Refresh()
        if ($process.HasExited -or $process.StartTime -ne $startTime) {
            throw "RunDog PID $TargetId exited during identity validation."
        }

        $validated = $true
        return [PSCustomObject]@{
            Process = $process
            ProcessId = $TargetId
            StartTime = $startTime
            ExecutablePath = $actualPath
            FileVersion = $fileVersion.FileVersion
            ProductVersion = $fileVersion.ProductVersion
        }
    } finally {
        if (-not $validated -and $null -ne $process) {
            $process.Dispose()
        }
    }
}

function Assert-SameRunDogProcess([int]$TargetId, [datetime]$StartTime) {
    $probe = $null
    try {
        $probe = Get-Process -Id $TargetId -ErrorAction Stop
        if ($probe.HasExited -or $probe.StartTime -ne $StartTime) {
            throw 'PID was reused or process exited.'
        }
    } catch {
        throw "RunDog PID $TargetId exited or was reused during measurement: $_"
    } finally {
        if ($null -ne $probe) {
            $probe.Dispose()
        }
    }
}

$identity = Assert-RunDogTarget $ProcessId
try {
if ($ValidateTargetOnly) {
    [PSCustomObject]@{
        ProcessId = $identity.ProcessId
        ExecutablePath = $identity.ExecutablePath
        FileVersion = $identity.FileVersion
        ProductVersion = $identity.ProductVersion
    }
    return
}

$targetProcess = $identity.Process
$logicalProcessors = [Environment]::ProcessorCount
$samples = [System.Collections.Generic.List[object]]::new()
$deadline = (Get-Date).AddSeconds($DurationSeconds)
$previousCpu = $targetProcess.TotalProcessorTime
$previousTimestamp = Get-Date
$started = Get-Date

while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds $IntervalMilliseconds
    $targetProcess.Refresh()
    if ($targetProcess.HasExited) {
        throw "RunDog PID $ProcessId exited during measurement."
    }
    Assert-SameRunDogProcess $ProcessId $identity.StartTime
    $timestamp = Get-Date
    $elapsedMilliseconds = ($timestamp - $previousTimestamp).TotalMilliseconds
    $cpuMilliseconds = ($targetProcess.TotalProcessorTime - $previousCpu).TotalMilliseconds
    $machineCpuPercent = if ($elapsedMilliseconds -gt 0) {
        100 * $cpuMilliseconds / ($elapsedMilliseconds * $logicalProcessors)
    } else {
        0
    }

    $cim = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId=$ProcessId" -ErrorAction SilentlyContinue
    Assert-SameRunDogProcess $ProcessId $identity.StartTime
    $gdi = 0
    $user = 0
    try {
        $gdi = [RunDogGuiResources]::GetGuiResources($targetProcess.Handle, 0)
        $user = [RunDogGuiResources]::GetGuiResources($targetProcess.Handle, 1)
    } catch {
        $gdi = 0
        $user = 0
    }

    $samples.Add([PSCustomObject]@{
        Timestamp = $timestamp.ToString('o')
        MachineCpuPercent = [Math]::Round($machineCpuPercent, 4)
        WorkingSetMiB = [Math]::Round($targetProcess.WorkingSet64 / 1MB, 3)
        PrivateMiB = [Math]::Round($targetProcess.PrivateMemorySize64 / 1MB, 3)
        Handles = $targetProcess.HandleCount
        Threads = $targetProcess.Threads.Count
        Gdi = [int]$gdi
        User = [int]$user
        PageFaults = if ($cim) { [int64]$cim.PageFaults } else { 0 }
        IoReadBytes = if ($cim) { [int64]$cim.ReadTransferCount } else { 0 }
        IoWriteBytes = if ($cim) { [int64]$cim.WriteTransferCount } else { 0 }
        IoReadOps = if ($cim) { [int64]$cim.ReadOperationCount } else { 0 }
        IoWriteOps = if ($cim) { [int64]$cim.WriteOperationCount } else { 0 }
    })

    $previousCpu = $targetProcess.TotalProcessorTime
    $previousTimestamp = $timestamp
}

$report = [PSCustomObject]@{
    ProcessId = $ProcessId
    ProcessName = $targetProcess.ProcessName
    ExecutablePath = $identity.ExecutablePath
    FileVersion = $identity.FileVersion
    ProductVersion = $identity.ProductVersion
    DurationSeconds = $DurationSeconds
    SampleCount = $samples.Count
    ElapsedMs = [int]((Get-Date) - $started).TotalMilliseconds
    MachineCpuPercent = Get-Summary @($samples | ForEach-Object { [double]$_.MachineCpuPercent })
    WorkingSetMiB = Get-Summary @($samples | ForEach-Object { [double]$_.WorkingSetMiB })
    PrivateMiB = Get-Summary @($samples | ForEach-Object { [double]$_.PrivateMiB })
    Handles = Get-Summary @($samples | ForEach-Object { [double]$_.Handles })
    Threads = Get-Summary @($samples | ForEach-Object { [double]$_.Threads })
    Gdi = Get-Summary @($samples | ForEach-Object { [double]$_.Gdi })
    User = Get-Summary @($samples | ForEach-Object { [double]$_.User })
    PageFaults = Get-Summary @($samples | ForEach-Object { [double]$_.PageFaults })
    IoReadBytes = Get-Summary @($samples | ForEach-Object { [double]$_.IoReadBytes })
    IoWriteBytes = Get-Summary @($samples | ForEach-Object { [double]$_.IoWriteBytes })
    IoReadOps = Get-Summary @($samples | ForEach-Object { [double]$_.IoReadOps })
    IoWriteOps = Get-Summary @($samples | ForEach-Object { [double]$_.IoWriteOps })
    UsageWork = [PSCustomObject]@{
        UsageParseBytes = $null
        IntegrityProbeBytes = $null
        LimitsTailBytes = $null
        CheckpointBytes = $null
        StartupDurationMs = $null
        CatchUpDurationMs = $null
        Note = 'Fill from DiagnosticSnapshot. Warm restart must not reaggregate (usage_parse_bytes=0). Integrity/limits-tail are separate.'
    }
    Samples = $samples
}

$report | ConvertTo-Json -Depth 5
} finally {
    $identity.Process.Dispose()
}
