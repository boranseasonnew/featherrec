param(
    [Parameter(Mandatory=$true)][string]$ProcessName,
    [int]$Seconds = 120,
    [string]$Output = "$PSScriptRoot\..\validation\process-metrics.csv"
)
$ErrorActionPreference = 'Stop'
$process = Get-Process -Name $ProcessName | Select-Object -First 1
if (-not $process) { throw "Start $ProcessName before measuring." }
$samples = [Collections.Generic.List[object]]::new()
$timer = [Diagnostics.Stopwatch]::StartNew()
$lastCpu = $process.TotalProcessorTime.TotalSeconds
$lastTime = 0.0
$invariant = [Globalization.CultureInfo]::InvariantCulture
while ($timer.Elapsed.TotalSeconds -lt $Seconds -and -not $process.HasExited) {
    Start-Sleep -Seconds 1
    $process.Refresh()
    if ($process.HasExited) { break }
    $time = $timer.Elapsed.TotalSeconds
    $cpu = $process.TotalProcessorTime.TotalSeconds
    $samples.Add([pscustomobject]@{
        # Explicit invariant text keeps Turkish decimal commas from being read
        # as thousands separators by tools expecting standard CSV numeric data.
        ElapsedSeconds = $time.ToString('R', $invariant)
        ProcessId = $process.Id
        CpuPercent = (100 * ($cpu - $lastCpu) / ($time - $lastTime) / [Environment]::ProcessorCount).ToString('R', $invariant)
        WorkingSetBytes = $process.WorkingSet64
        PrivateBytes = $process.PrivateMemorySize64
    })
    $lastCpu = $cpu
    $lastTime = $time
}
$parent = Split-Path ([IO.Path]::GetFullPath($Output)) -Parent
New-Item -ItemType Directory -Force -Path $parent | Out-Null
$samples | Export-Csv -LiteralPath $Output -NoTypeInformation
Write-Output "Measured $($samples.Count) samples: $Output"
