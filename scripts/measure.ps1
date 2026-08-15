[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateRange(1, 2147483647)]
    [int]$ProcessId,

    [ValidateRange(2, 3600)]
    [int]$DurationSeconds = 60,

    [ValidateRange(100, 10000)]
    [int]$IntervalMilliseconds = 1000
)

$targetProcess = Get-Process -Id $ProcessId -ErrorAction Stop
$logicalProcessors = [Environment]::ProcessorCount
$samples = [System.Collections.Generic.List[object]]::new()
$deadline = (Get-Date).AddSeconds($DurationSeconds)
$previousCpu = $targetProcess.TotalProcessorTime
$previousTimestamp = Get-Date

while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds $IntervalMilliseconds
    $targetProcess.Refresh()
    $timestamp = Get-Date
    $elapsedMilliseconds = ($timestamp - $previousTimestamp).TotalMilliseconds
    $cpuMilliseconds = ($targetProcess.TotalProcessorTime - $previousCpu).TotalMilliseconds
    $machineCpuPercent = if ($elapsedMilliseconds -gt 0) {
        100 * $cpuMilliseconds / ($elapsedMilliseconds * $logicalProcessors)
    } else {
        0
    }

    $samples.Add([PSCustomObject]@{
        Timestamp = $timestamp.ToString('o')
        MachineCpuPercent = [Math]::Round($machineCpuPercent, 4)
        WorkingSetMiB = [Math]::Round($targetProcess.WorkingSet64 / 1MB, 3)
        PrivateMiB = [Math]::Round($targetProcess.PrivateMemorySize64 / 1MB, 3)
        Handles = $targetProcess.HandleCount
        Threads = $targetProcess.Threads.Count
    })

    $previousCpu = $targetProcess.TotalProcessorTime
    $previousTimestamp = $timestamp
}

$samples | ConvertTo-Json -Depth 3
