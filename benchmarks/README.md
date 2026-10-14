# Reproducible comparison with OBS

No comparative results have been produced. FeatherRec does not claim to beat OBS.
This procedure produces measurements; it is not a synthetic marketing score.

1. Record CPU, GPU, RAM, Windows build, graphics driver, display refresh, OBS version,
   FeatherRec commit, power plan, temperatures, and whether the laptop is plugged in.
2. Use one display, 1280×720 output, 30 FPS ceiling, H.264, 4 Mbps, no audio, no
   preview, and the same workload for the first comparison. Repeat for 1080p60/8 Mbps.
   Record the exact selected encoder; Media Foundation and OBS vendor SDK paths may
   have different defaults. Match keyframe interval, profile and rate control when
   the interfaces expose them. Document every unmatched setting.
3. Warm up the workload for 60 seconds. Capture a 120-second repeatable game replay
   or scripted workload. Disable OBS preview. Keep the UI visibility of both tools
   identical. Do not run both recorders at once.
4. Run at least three trials each, alternating order. Add a no-recorder baseline.
   Use PresentMon or another documented game frame-time collector for average and
   1% low FPS. Define 1% low as 1000 divided by the mean of the slowest 1% of frame
   times in milliseconds; do not mix percentile definitions across tools.
5. Use `measure-process.ps1` for each recorder's normalized CPU, working set and
   private bytes. For FeatherRec CLI use `-ProcessName featherrec-cli`; for the UI
   use `featherrec`; for OBS use `obs64`. Add GPU utilization counters and a GPU
   trace when available; leave unavailable measurements blank, not zero.
6. Save OBS logs and FeatherRec's local recording report. Record actual frames,
   dropped frames, queue high-water, submission latency, output bytes and elapsed
   duration. FeatherRec's pacer skips and backpressure drops are distinct; OBS's
   counters may have different meanings. Do not merge unlike counters.
7. Decode every output fully with FFmpeg. Verify dimensions, duration, frame PTS and
   first/last visible workload frames. FeatherRec is VFR and may emit fewer frames
   for static content. Compare a moving workload, and report actual encoded FPS.
8. Inspect identical frames for text legibility, motion artifacts and scaling.
   Use SSIM/PSNR/VMAF only if you have a synchronized lossless reference. Never
   treat smaller output alone as better quality or report an invented quality score.

Example resource measurements (run in a second terminal while the trial records):

```powershell
./benchmarks/measure-process.ps1 -ProcessName featherrec-cli -Seconds 120 -Output validation/featherrec.csv
./benchmarks/measure-process.ps1 -ProcessName obs64 -Seconds 120 -Output validation/obs.csv
```

Create a results table containing trial, recorder commit/version, configuration,
game average FPS, 1% low FPS, CPU mean/p95, RAM mean/peak, GPU utilization, dropped
frames, actual recording FPS, file size and quality observations. Attach raw data
after removing private paths/screen content. Report variance and limitations.
If OBS wins, publish that result and investigate the bottleneck.
