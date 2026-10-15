# Validation record

The historical measurements below concern v0.1.0. The v0.1.1 release notes record
its resize, persistent-settings, icon and installer regression checks. The reported
Minecraft fullscreen failure in v0.1.0 was traced to its explicit source-size-change
abort; this condition is replaced by fixed-canvas GPU scaling in v0.1.1.

Validation date: 15 October 2026 (Europe/Istanbul). These are local functional
checks of the initial implementation, not a cross-hardware certification or an OBS
benchmark. Desktop recordings and reports are excluded from the public repository.

## Environment

- Windows 11 Home, build 26300, x64.
- AMD Ryzen 7 5700U, 16 logical processors; AMD Radeon integrated graphics.
- AMD graphics driver 31.0.21925.1001.
- One physical 1920×1080, 60 Hz display.
- Registered hardware H.264 MFT: `AMDh264Encoder`.
- The selected SinkWriter encoder exposed a hardware URL, satisfying hardware
  policy, but did not expose a friendly name. Reports disclose that distinction.
- Rust 1.99.0, Windows MSVC x64 target, release profile, statically linked C++ runtime.
  This host's incomplete Visual Studio registration required Rust's bundled linker
  against the installed MSVC/Windows SDK libraries; no system installation was modified.

## Build and unit checks

The following local checks passed:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
```

Nine unit tests passed (settings, presets, monotonic pacing, pressure hysteresis,
Unicode/extended path normalization, legacy preferences, hotkey validation and
Windows hotkey-thread dispatch/shutdown). The desktop integration test is ignored
by default because hosted CI does not prove physical capture. It was also invoked
explicitly on this machine and passed: **two recordings in one process**, each
successfully finalized, reopened, dimension-checked and first-frame-decoded.

Local Cargo sometimes warned that incremental cache hard links were unavailable and
copied cache files instead. These were environment warnings; no Clippy code warnings
were suppressed to make the checks pass.

PE dependency inspection confirmed that the GUI executable imports Windows system
DLLs and does not require `VCRUNTIME140.dll` or a bundled FFmpeg runtime.

## Moving desktop recording matrix

The local WinForms motion/text workload was visible on the physical desktop. Each
recording used hardware policy, 4 Mbps, no audio, and a 10-second requested duration.
Resolution and FPS were custom overrides of the default Potato settings. FFmpeg
decoded each complete MP4, with the original video timebase preserved. Decoded frame
counts matched submitted samples; dimensions and duration checks passed.

| Output | Requested FPS ceiling | Decoded frames | Decoded duration | Pool drops | Maximum in-flight textures |
|---|---:|---:|---:|---:|---:|
| 1280×720 | 30 | 301 | 10.03 s | 0 | 3 |
| 1280×720 | 60 | 510 | 10.03 s | 0 | 3 |
| 1920×1080 | 30 | 301 | 10.03 s | 0 | 2 |
| 1920×1080 | 60 | 506 | 10.03 s | 0 | 3 |

The 60 FPS targets produced approximately 50–51 submitted FPS, matching the callback
count delivered by WGC in those runs. They do **not** establish sustained 60 FPS.
The 30 FPS paths intentionally paced out surplus callbacks. The application did
not invent missing frames to claim the target was reached.

These runs occurred during development, with compilation and other desktop activity.
One short GUI smoke recording overlapped part of this matrix. Consequently these
numbers establish recording correctness under those conditions, not uncontaminated
performance measurements. The benchmark procedure requires isolated trials.

## Lifecycle, static content and error paths

- A static 720p30 recording produced a single coded frame with about 5.5 seconds of
  duration, and passed full-file decoding. The initial extra startup duration was
  then removed by initializing the encoder before starting capture and timing the
  CLI stop against capture time. The final matrix durations are approximately 10 s.
- A preliminary moving 1080p60 run decoded all 996 submitted frames over 20.7 s,
  with zero application pool drops. This was a development check, not a benchmark.
- GUI launch and screenshot capture succeeded. The screenshot in `assets/` contains
  only the application UI, with no personal paths or desktop content.
- GUI smoke test started real capture and exercised the close-while-recording path.
  It exited normally and produced a finalized MP4; all 119 submitted frames decoded.
- Two backend recordings in one process passed the opt-in integration test.
- Invalid FPS, nonexistent monitor index, and an output path that is an existing
  file each produced an actionable error and CLI exit code 1.
- Rust's canonical extended path prefix initially broke SourceReader URL handling.
  Media paths are now normalized while preserving Unicode; regression tests cover
  drive and UNC syntax. Actual network-share recording was not tested.
- Windows' video processor initially expanded long-duration static samples into
  repeated frames. `MF_XVP_DISABLE_FRC` now disables that conversion before streaming;
  the single-frame static output and matched decode counts verify the fix here.

## 60-second resource sanity check

A subsequent release-CLI run recorded the visible motion/text workload at 720p30,
4 Mbps, hardware H.264 and no audio for 60.049 seconds. All **1,802** submitted
frames decoded; FFmpeg reported 60.077 seconds of output, zero decode errors, and
no frame duplication. Application pool drops were zero and texture high-water was 3.
Output size was 30,093,913 bytes.

The process sampler collected 60 one-second samples. Excluding samples earlier than
5 seconds left 56 observations: mean working set **53.61 MiB**, peak **53.83 MiB**,
and mean normalized process CPU **0.90%** (maximum sampled interval **1.65%**).
The first five retained memory samples averaged 53.69 MiB and the last five 53.18 MiB.
No monotonic memory growth was observed during this minute; this is not proof that
all longer sessions or drivers are leak-free.

These are CLI-only observations on this machine and synthetic workload. The GUI
uses additional resources (about 126 MiB working set was sampled in the short GUI
smoke run). A separate test build ran during part of the resource check. CPU
percentages are recorder process time normalized over 16 logical processors, not
total system workload. No GPU utilization or game FPS conclusion follows from them.

The initial CSV used the host's Turkish decimal formatting. Its values were parsed
with that explicit locale for analysis; the sampling script now exports invariant
decimal points for reproducible downstream numeric parsing.

## Settings, window capture and audio update

- Release GUI launch and screenshots passed for the redesigned dashboard, Audio
  settings and locked Advanced settings. Advanced controls are disabled until the
  explicit unlock; no placeholder audio controls remain.
- Real global Ctrl+Shift+F9/F10 input started/stopped the GUI recorder while in the
  background, with minimize-on-start enabled. The application closed normally, and
  the resulting 2.21-second, 66-frame MP4 passed full decoding with zero pool drops.
- Windows enumerated actual render and microphone endpoint IDs/names. Both device
  selectors support the Windows default and specific endpoint IDs.
- System loopback and process-tree loopback recorded simultaneous reference sources
  at 440 and 880 Hz. Each complete H.264/AAC MP4 decoded successfully: 149 frames
  over 5.01 seconds for system capture; 151 frames over 5.03 seconds for the selected
  window/process. Audio used 48 kHz stereo AAC. No drops or audio discontinuities
  were reported in these short runs.
- Offline analysis of the middle two seconds found both frequencies in system audio
  at comparable levels. In process-only audio, the other process's 880 Hz component
  was approximately 115 dB below the selected 440 Hz component. This validates process
  isolation for this controlled source, rather than all games or audio middleware.
- Microphone-only capture with mute enabled produced a valid 3.09-second MP4. Full
  decode and audio statistics confirmed every decoded audio sample was zero. This
  verifies mute, not microphone speech quality.
- A subsequent 60.044-second system-audio plus muted-microphone run finalized with
  1,573 video samples, zero pool drops, texture high-water 4, and 2,882,125 PCM audio
  frames (60.0443 seconds). Both streams passed complete decoding. Eleven device
  discontinuity flags were reported; no late-packet warning or timing-buffer overflow
  occurred. This is a bounded short-session check, not proof of uninterrupted audio
  or long-session perceptual A/V synchronization.
- All new recordings used capture-border suppression through Windows' supported API
  without initialization failure. No claim is made about unsupported older builds.

Desktop audio tests are local and excluded from Git. Reference recordings are never
release assets. The tiny black, silent video fixture committed for file-only CI tests
is synthetic and contains no desktop capture or user audio.

## Not yet validated

Long-session perceptual A/V sync, audio-device disconnect and drift across different
drivers remain unverified. The historical tests above used one mixed track;
v0.1.2 adds 1–5 routed AAC streams with a separate file-only CI test. New release
validation results are recorded in the release notes. Software-only fallback, NVIDIA/Intel hardware,
Windows 10, other GPU drivers, multi-monitor/cross-adapter capture, HDR, RDP, screen
lock, physical disconnects, display-mode changes, full disks, forced termination,
and power loss remain outside the completed test matrix.

There are no game FPS/1% low measurements, GPU utilization traces, allocation counts,
hardware encode-latency measurements, SSIM/VMAF scores or OBS comparison results.
No official production release is published from these checks alone.
