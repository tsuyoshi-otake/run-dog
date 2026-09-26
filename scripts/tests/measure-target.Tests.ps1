$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$measure = Join-Path $repo 'scripts\measure.ps1'
$scenarios = Join-Path $repo 'scripts\perf-scenarios.ps1'
$manifestVersion = (Select-String -LiteralPath (Join-Path $repo 'Cargo.toml') -Pattern '^version = "([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$builtPath = Join-Path $repo 'target\debug\RunDog.exe'
$installedPath = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\RunDog\RunDog.exe'

function New-FakeProcess([string]$Path, [bool]$HasExited = $false) {
    $process = [PSCustomObject]@{
        Path = $Path
        StartTime = [DateTime]::Now.AddMinutes(-1)
        HasExited = $HasExited
        TotalProcessorTime = [TimeSpan]::Zero
        RefreshCount = 0
        Disposed = $false
    }
    $process | Add-Member -MemberType ScriptMethod -Name Refresh -Value {
        $this.RefreshCount++
        if ($global:RunDogMeasureTestExitOnSecondRefresh -and $this.RefreshCount -eq 2) {
            $this.HasExited = $true
        }
    }
    $process | Add-Member -MemberType ScriptMethod -Name Dispose -Value {
        $this.Disposed = $true
        $global:RunDogMeasureTestDisposeCount++
    }
    return $process
}

# Child scripts inherit these command fakes. No executable is launched and no
# installed or built RunDog file is altered by these identity tests.
function Get-Process {
    param([int]$Id, [string]$ErrorAction)
    if ($global:RunDogMeasureTestMissing) { throw 'No process with this PID' }
    if ($null -ne $global:RunDogMeasureTestProcessQueue -and
        $global:RunDogMeasureTestProcessQueue.Count -gt 0) {
        return $global:RunDogMeasureTestProcessQueue.Dequeue()
    }
    return $global:RunDogMeasureTestProcess
}

function Get-Item {
    param([string]$LiteralPath, [string]$ErrorAction)
    return [PSCustomObject]@{
        VersionInfo = [PSCustomObject]@{
            ProductName = $global:RunDogMeasureTestProduct
            InternalName = 'RunDog'
            OriginalFilename = 'RunDog.exe'
            FileVersion = $global:RunDogMeasureTestVersion
            ProductVersion = $global:RunDogMeasureTestVersion
        }
    }
}

function cargo { throw 'Cargo was reached before target validation.' }

function Assert-Throws([scriptblock]$Action, [string]$ExpectedText) {
    try {
        & $Action | Out-Null
    } catch {
        if ($_.Exception.Message -notmatch [regex]::Escape($ExpectedText)) {
            throw "Expected error containing '$ExpectedText'; got '$($_.Exception.Message)'."
        }
        return
    }
    throw "Expected error containing '$ExpectedText', but command succeeded."
}

function Assert-MeasurementAborts([string]$ExpectedText, [int]$ExpectedDisposals) {
    $output = [System.Collections.Generic.List[object]]::new()
    try {
        & $measure -ProcessId 14567 -DurationSeconds 2 -IntervalMilliseconds 100 |
            ForEach-Object { $output.Add($_) }
        throw "Expected measurement to abort with '$ExpectedText'."
    } catch {
        if ($_.Exception.Message -notmatch [regex]::Escape($ExpectedText)) {
            throw "Expected error containing '$ExpectedText'; got '$($_.Exception.Message)'."
        }
    }
    if ($output.Count -ne 0) {
        throw 'Measurement emitted a partial success report after process identity changed.'
    }
    if ($global:RunDogMeasureTestDisposeCount -ne $ExpectedDisposals) {
        throw "Expected $ExpectedDisposals process handles to be disposed; got $global:RunDogMeasureTestDisposeCount."
    }
}

try {
    $global:RunDogMeasureTestMissing = $false
    $global:RunDogMeasureTestProduct = 'RunDog'
    $global:RunDogMeasureTestVersion = $manifestVersion
    $global:RunDogMeasureTestDisposeCount = 0
    $global:RunDogMeasureTestExitOnSecondRefresh = $false
    $global:RunDogMeasureTestProcessQueue = $null

    # Listing scenarios is useful without a running application PID.
    $list = & $scenarios -Mode list
    if (-not ($list -match 'Phase K performance scenarios')) {
        throw 'Scenario list did not render without a PID.'
    }
    Assert-Throws { & $scenarios -Mode smoke } 'requires -ProcessId'

    $global:RunDogMeasureTestMissing = $true
    Assert-Throws { & $measure -ProcessId 14567 -ValidateTargetOnly } 'missing, exited'
    $global:RunDogMeasureTestMissing = $false

    $global:RunDogMeasureTestProcess = New-FakeProcess -Path $builtPath -HasExited $true
    Assert-Throws { & $measure -ProcessId 14567 -ValidateTargetOnly } 'missing, exited'

    $global:RunDogMeasureTestProcess = New-FakeProcess -Path (Join-Path $PSHOME 'pwsh.exe')
    Assert-Throws { & $measure -ProcessId 14567 -ValidateTargetOnly } 'expected a RunDog executable'
    Assert-Throws { & $scenarios -Mode smoke -ProcessId 14567 } 'expected a RunDog executable'

    $global:RunDogMeasureTestProcess = New-FakeProcess -Path $builtPath
    $global:RunDogMeasureTestProduct = 'Not RunDog'
    Assert-Throws { & $measure -ProcessId 14567 -ValidateTargetOnly } 'expected RunDog'

    $global:RunDogMeasureTestProduct = 'RunDog'
    $global:RunDogMeasureTestVersion = '0.0.0'
    Assert-Throws { & $measure -ProcessId 14567 -ValidateTargetOnly } 'expected RunDog'

    $global:RunDogMeasureTestVersion = $manifestVersion
    $identity = & $measure -ProcessId 14567 -ValidateTargetOnly
    if ($identity.ProcessId -ne 14567 -or
        $identity.ExecutablePath -ne [System.IO.Path]::GetFullPath($builtPath) -or
        $identity.FileVersion -ne $manifestVersion) {
        throw 'Valid RunDog identity did not return its exact PID, executable, and version.'
    }

    $global:RunDogMeasureTestProcess = New-FakeProcess -Path $installedPath
    $identity = & $measure -ProcessId 14567 -ValidateTargetOnly
    if ($identity.ExecutablePath -ne [System.IO.Path]::GetFullPath($installedPath)) {
        throw 'Valid installed RunDog identity was rejected.'
    }

    $global:RunDogMeasureTestDisposeCount = 0
    $global:RunDogMeasureTestExitOnSecondRefresh = $true
    $global:RunDogMeasureTestProcess = New-FakeProcess -Path $builtPath
    Assert-MeasurementAborts 'exited during measurement' 1

    $global:RunDogMeasureTestDisposeCount = 0
    $global:RunDogMeasureTestExitOnSecondRefresh = $false
    $original = New-FakeProcess -Path $builtPath
    $reused = New-FakeProcess -Path $builtPath
    $reused.StartTime = $original.StartTime.AddSeconds(1)
    $global:RunDogMeasureTestProcessQueue = [System.Collections.Queue]::new()
    $global:RunDogMeasureTestProcessQueue.Enqueue($original)
    $global:RunDogMeasureTestProcessQueue.Enqueue($reused)
    Assert-MeasurementAborts 'exited or was reused during measurement' 2

    Write-Output 'RunDog measurement target identity tests passed.'
} finally {
    Remove-Variable -Scope Global -Name RunDogMeasureTestMissing, RunDogMeasureTestProcess, RunDogMeasureTestProduct, RunDogMeasureTestVersion, RunDogMeasureTestDisposeCount, RunDogMeasureTestExitOnSecondRefresh, RunDogMeasureTestProcessQueue -ErrorAction SilentlyContinue
}
