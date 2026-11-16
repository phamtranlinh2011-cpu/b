[CmdletBinding()]
param(
    [ValidateRange(0, 2147483647)]
    [int]$ProcessId = 0,
    [ValidateRange(10, 3600)]
    [int]$DurationSeconds = 60,
    [ValidateRange(1, 10)]
    [int]$IntervalSeconds = 1,
    [string]$OutputPath = ''
)

$ErrorActionPreference = 'Stop'
if (-not $IsWindows -and $env:OS -ne 'Windows_NT') {
    throw 'NOVA performance sampling requires Windows.'
}

if ($ProcessId -gt 0) {
    $process = Get-Process -Id $ProcessId -ErrorAction Stop
} else {
    $candidates = @(Get-Process -Name 'NOVA' -ErrorAction SilentlyContinue)
    if ($candidates.Count -eq 0) { throw 'NOVA.exe is not running. Start NOVA first or pass -ProcessId.' }
    if ($candidates.Count -gt 1) {
        Write-Host 'Multiple NOVA processes found:'
        $candidates | Select-Object Id, StartTime, Path | Format-Table -AutoSize
        throw 'Pass -ProcessId to select the process to measure.'
    }
    $process = $candidates[0]
}

if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $stamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')
    $OutputPath = Join-Path $PSScriptRoot "..\perf-results\NOVA-$($process.Id)-$stamp.csv"
}
$outputFullPath = [IO.Path]::GetFullPath($OutputPath)
$outputDirectory = Split-Path -Parent $outputFullPath
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null

$logicalCpuCount = [Math]::Max(1, [Environment]::ProcessorCount)
$rows = [Collections.Generic.List[object]]::new()
$stopwatch = [Diagnostics.Stopwatch]::StartNew()
$previousSampleAt = $stopwatch.Elapsed.TotalSeconds
$previousCpuSeconds = $process.TotalProcessorTime.TotalSeconds
$endAt = $DurationSeconds

Write-Host "Sampling NOVA PID $($process.Id) for up to $DurationSeconds seconds."
Write-Host 'CPU is normalized to total logical processor capacity (0–100% of the whole machine).'

while ($stopwatch.Elapsed.TotalSeconds -lt $endAt) {
    Start-Sleep -Seconds $IntervalSeconds
    try {
        $process.Refresh()
        if ($process.HasExited) { break }
        $now = $stopwatch.Elapsed.TotalSeconds
        $cpuSeconds = $process.TotalProcessorTime.TotalSeconds
        $elapsed = [Math]::Max(0.001, $now - $previousSampleAt)
        $cpuPercent = (($cpuSeconds - $previousCpuSeconds) / $elapsed / $logicalCpuCount) * 100.0
        $rows.Add([pscustomobject]@{
            ProcessId = $process.Id
            TimestampUtc = [DateTime]::UtcNow.ToString('o')
            ElapsedSeconds = [Math]::Round($now, 2)
            CpuPercentTotal = [Math]::Round([Math]::Max(0, $cpuPercent), 2)
            WorkingSetMB = [Math]::Round($process.WorkingSet64 / 1MB, 2)
            PrivateMemoryMB = [Math]::Round($process.PrivateMemorySize64 / 1MB, 2)
            Handles = $process.HandleCount
            Threads = $process.Threads.Count
        })
        $previousSampleAt = $now
        $previousCpuSeconds = $cpuSeconds
    } catch {
        Write-Warning "Could not sample NOVA any further: $($_.Exception.Message)"
        break
    }
}
$stopwatch.Stop()

if ($rows.Count -eq 0) { throw 'No samples were collected. NOVA may have exited before the first sample.' }
$rows | Export-Csv -LiteralPath $outputFullPath -NoTypeInformation -Encoding UTF8
$avgCpu = ($rows | Measure-Object -Property CpuPercentTotal -Average).Average
$peakCpu = ($rows | Measure-Object -Property CpuPercentTotal -Maximum).Maximum
$peakWorkingSet = ($rows | Measure-Object -Property WorkingSetMB -Maximum).Maximum
$peakPrivate = ($rows | Measure-Object -Property PrivateMemoryMB -Maximum).Maximum
$peakHandles = ($rows | Measure-Object -Property Handles -Maximum).Maximum
$peakThreads = ($rows | Measure-Object -Property Threads -Maximum).Maximum

Write-Host ''
Write-Host "Samples: $($rows.Count) | Duration: $([Math]::Round($stopwatch.Elapsed.TotalSeconds, 1)) s"
Write-Host "Average CPU: $([Math]::Round($avgCpu, 2))% | Peak CPU: $([Math]::Round($peakCpu, 2))%"
Write-Host "Peak working set: $peakWorkingSet MB | Peak private memory: $peakPrivate MB"
Write-Host "Peak handles: $peakHandles | Peak threads: $peakThreads"
Write-Host "CSV: $outputFullPath" -ForegroundColor Green
