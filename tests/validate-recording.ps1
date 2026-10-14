param(
    [string]$Cli = "$PSScriptRoot\..\target\release\featherrec-cli.exe",
    [string]$FFmpeg = 'ffmpeg.exe',
    [int]$Seconds = 10,
    [ValidateSet(720,1080)][int]$Resolution = 720,
    [ValidateSet(30,60)][int]$Fps = 30,
    [int]$Monitor = 1,
    [switch]$AllowSoftware
)
$ErrorActionPreference = 'Stop'
$folder = Join-Path $PSScriptRoot '..\validation'
New-Item -ItemType Directory -Force -Path $folder | Out-Null
$arguments = @('--record', '--seconds', "$Seconds", '--output', $folder,
    '--monitor', "$Monitor", '--resolution', "$Resolution", '--fps', "$Fps")
if ($AllowSoftware) { $arguments += '--allow-software' }
$raw = & $Cli @arguments
if ($LASTEXITCODE -ne 0) { throw 'Recording failed. See the diagnostic above and retained partial file.' }
$report = ($raw -join "`n") | ConvertFrom-Json
if (-not $report.finalized) { throw 'The recorder did not validate its MP4.' }
if ($report.metrics.max_in_flight -gt 4) { throw 'GPU texture ownership exceeded its bound.' }
# Preserve the MP4 timebase when decoding VFR; a guessed 1/FPS output timebase
# can round distinct timestamps together and produce false DTS warnings.
$decode = & $FFmpeg -v error -xerror -threads 1 -i $report.file -map 0:v:0 -fps_mode passthrough -enc_time_base demux -f null NUL -progress pipe:1 2>&1
if ($LASTEXITCODE -ne 0) { throw "Full-file decoding failed: $decode" }
$frames = ($decode | Select-String '^frame=([0-9]+)' | Select-Object -Last 1).Matches.Groups[1].Value
$timeUs = ($decode | Select-String '^out_time_us=([0-9]+)' | Select-Object -Last 1).Matches.Groups[1].Value
if (-not $frames -or [int]$frames -lt 1) { throw 'No video frames decoded.' }
if ([int]$frames -ne $report.metrics.submitted) { throw 'Decoded frame count does not match submitted frame count.' }
$duration = [double]$timeUs / 1000000
if ([Math]::Abs($duration - $report.metrics.elapsed_seconds) -gt 0.75) {
    throw "Decoded duration $duration differs from capture duration $($report.metrics.elapsed_seconds)."
}
[pscustomobject]@{
    File = $report.file
    DecodedFrames = [int]$frames
    DecodedDuration = $duration
    CaptureDuration = $report.metrics.elapsed_seconds
    Dimensions = "$($report.settings.width)x$($report.settings.height)"
    RequestedFps = $report.settings.fps
    SubmittedFps = $report.metrics.submitted / [Math]::Max(0.001, $report.metrics.elapsed_seconds)
    Dropped = $report.metrics.dropped
    PoolHighWater = $report.metrics.max_in_flight
    Encoder = $report.metrics.encoder_name
    HardwareVerified = $report.metrics.hardware_verified
}
