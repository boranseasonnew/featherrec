# Architecture and dependency decisions

The workspace separates platform-neutral settings and timing (`recorder-core`),
Windows capture/encoding/inspection (`recording-windows`), and UI/CLI (`featherrec`).
Audio capture/mixing and MP4 muxing are separate Windows modules.

## Capture and frame ownership

`windows-capture` 2.0.1 (MIT) wraps Windows Graphics Capture and D3D11. Its current
source and maintenance activity were inspected before selection. Full-monitor WGC
provides monitor/window capture with system-relative timestamps and source closure
notifications. Desktop Duplication is a possible fallback, not a second untested
backend included just for the checklist.

The upstream crate's convenience `VideoEncoder` uses an unbounded channel, so this
application does not use it. FeatherRec owns four source-sized BGRA GPU
textures. A callback tries to acquire one, performs a GPU copy, and tries to enqueue
it. If none are free, it counts a dropped frame. It never waits for encoding or disk.
Oversupply skipped by the FPS pacer is counted separately from backpressure drops.

One worker owns a synchronous Media Foundation SinkWriter. It can block on the
encoder/disk without blocking the UI or capture. D3D11 immediate-context protection
is enabled before sharing the device. Each submitted texture is held by an
`IMFTrackedSample`; its release callback returns the texture to the pool only when
Media Foundation releases it. GPU copies avoid application CPU pixel readbacks.
There is no claim that drivers or transforms implement an entirely zero-copy path.

Application-owned source texture storage is `4 × width × height × 4` bytes (about
31.6 MiB at 1080p by default; advanced settings allow 3–8 textures), plus the OS capture pool, encoder buffers, UI, and driver memory.
The pool limits application frame ownership, not all driver or OS allocations.
The high-water in-flight count is reported so validation can check the invariant.

Source-size changes no longer invalidate the encoder. WGC recreates its capture
pool; FeatherRec fits subsequent textures into the original encoder canvas using
the D3D11 video processor. The resize path holds one additional source-sized scratch
texture, recreated only when the source size changes. Normal matching frames use
the original GPU copy path. Input/output views and COM references are released each
frame; Media Foundation retains its bounded output textures until tracked release.
Aspect-ratio changes use a black background. Video timestamps and audio's origin
are preserved across changes, with a source-size-change counter in diagnostics.

## Media pipeline

The sink writer receives BGRA D3D textures with the actual monitor dimensions and
negotiates H.264 at the requested output dimensions/bitrate. Windows inserts the
needed conversion/scaling transform. The DXGI device manager and hardware transform
flag are passed to the writer. Conversion behavior depends on installed transforms
and drivers; hardware initialization can fail.

Hardware H.264 MFTs are enumerated by output subtype, not GPU vendor guesses.
The transform actually selected by SinkWriter is inspected separately. Hardware
policy requires a hardware URL attribute on that encoder, failing conservatively
when Windows cannot identify it. `Allow software` is explicit consent to a potentially
more CPU-intensive encoder. No direct NVENC, AMF or Quick Sync SDK integration is
claimed: vendor support is through installed Media Foundation transforms.

Frame timestamps use WGC's monotonic clock, rebased to zero. The FPS setting is a
ceiling: the pacer drops surplus callbacks. Output uses variable frame durations;
the worker holds one packet until the next timestamp arrives. Static content is
not duplicated at 60 FPS merely to report a larger number. The final packet extends
to stop time. Frame ordering is preserved and repeated/backward timestamps rejected.

## Stop and failure handling

Normal stop first joins capture, closes the bounded packet sender, drains encoding,
and calls `Finalize`. A source closure stops capture and attempts clean finalization. Encoder/disk failures are
surfaced and partial files retained.
Output names contain a nanosecond timestamp and process ID, with exclusive creation.

On a clean stop, SourceReader reopens the container, verifies dimensions, and decodes
the first frame. Only then is `.partial.mp4` renamed to `.mp4`. This check does not
prove every frame is valid: the external validation script decodes the whole file.
No output is called successful solely because the encoder returned without panic.

Closing the UI requests stop and waits asynchronously for finalization. A destructor
also stops/joins for normal unwinding. Process termination or a hung GPU driver
cannot be made safe by a destructor; MP4 recovery is not guaranteed in those cases.

## Audio

`wasapi` 0.26.0 (MIT) provides event-driven shared-mode capture at 48 kHz float stereo,
with Windows automatic format conversion. System loopback uses a render endpoint;
process loopback includes the selected window's process tree (build 20348+). There is
no fallback from process audio to system audio. The microphone is independently selected.

WASAPI packet QPC timestamps are rebased against the first WGC system-relative timestamp.
Each source has a fixed one-second tagged sample ring; a 100 ms jitter window permits
packet arrival before clocked mixing. Late packets/discontinuities are counted, timing
overflow fails explicitly, and missing samples become silence. Live gains and mic mute
use atomics. Routing clips to PCM16 and writes bounded chunks to one WAV per track.
Memory does not grow with recording duration. WAV storage is about 11.5 MB/minute
per track, with a 4 GiB limit per WAV.

On stop, capture time is frozen for video and audio. After workers drain, a separate
Media Foundation pass copies compressed H.264 samples and encodes mixed PCM as stereo AAC.
This avoids a second video encode but adds temporary storage and finalization time.
The MP4 contains the selected 1–5 audio streams. Each audio stream is decoded once and
video dimensions/first frame are checked before promoting the output; full-file
decoding remains external QA.
Temporary video/WAV files are removed only after validated promotion. Audio-device
failure is explicit and stops recording. Long-session drift and hot-unplug need more tests.

## Diagnostics and UI

The egui/eframe 0.36.2 UI uses Glow with default features disabled. This was chosen
for a compact, direct Rust UI and event-driven redraws without a browser runtime.
No live video preview is rendered. UI updates are limited to status/timer activity,
input events and framework requests. Slint could reduce or change rendering costs,
but no superiority claim is made without measuring both.

Measured diagnostics include callbacks, submitted samples, pacer skips, pool drops,
in-flight textures, SinkWriter submission duration, working set, process CPU and
output file size. Submission latency is not labeled hardware encoding latency.
CPU is normalized to all logical processors, like Task Manager. FPS can be derived
from counts and elapsed capture time; submitted samples are not a hardware completion
counter. A pressure monitor recommends safer settings after sustained pressure and
never silently changes recording quality.

Reports and per-user preferences are local JSON files. Preferences are written and
flushed to a unique temporary file, then atomically replaced in Local AppData before
Save returns. Global-hotkey registration is independent; a collision preserves other
settings and rolls the shortcut selection back. Legacy portable preferences migrate
only when the per-user file is absent. Runtime code has
no network communication. Dependency downloads and GitHub CI are build-time concerns.

New GUI profiles use Local AppData/FeatherRec/Recordings. A previously chosen output
folder is preserved, and the dashboard exposes its path and a settings shortcut.
General settings can explicitly restore and save the per-user recording folder.
Output-directory creation, resolution and first-file creation report the selected
path on failure. Protected-folder restrictions remain enforced by Windows; FeatherRec
does not change security policies or silently redirect a custom output selection.

The Turkish UI has a recorder dashboard and five settings pages. Engine/source controls
are locked during capture; microphone mute and source gains stay live. A dedicated
blocking Windows message thread uses RegisterHotKey and MOD_NOREPEAT, wakes the GUI even
when minimized, and rolls back registrations on a conflict. Assignment temporarily
suspends active shortcuts; only explicit foreground key assignment reads key events.
There is no global keyboard hook. Window handles are never persisted across restarts.
Advanced controls expose bitrate, encoder fallback, frame-pool size, low latency and
report writing behind an explicit unlock. Output verification cannot be disabled.

Audio output consists of 1–5 bounded per-track PCM writers. System/application and
microphone samples share the video origin, then route to each track independently.
Finalization copies compressed video and encodes each WAV to a separate AAC stream;
every stream must be present and decodable before the file is promoted. Temporary
PCM uses approximately 11 MiB/minute per selected track. More tracks increase disk
I/O and finalization work. A file-only CI fixture checks one and five audio streams.
It never renders sound to the desktop.

An explicit audio-preview worker opens capture endpoints only, reads bounded packets
and publishes decaying peaks without files or playback. It stops when source settings
change or recording begins. Empty endpoint IDs follow the Windows console default
device, checked once per second and on capture failure; explicit IDs stay fixed.
An independent, transient window selection can supply process audio while recording
a monitor. No process identifiers are restored across application restarts.

CQP uses the selected transform's ICodecAPI before BeginWriting: quality mode plus
AVEncVideoEncodeQP. Both values must read back correctly; unsupported or ignored
properties cause initialization failure. No claim is made that CQP improves speed.

## References

- [windows-capture source and MIT license](https://github.com/NiiightmareXD/windows-capture)
- [Media Foundation Sink Writer](https://learn.microsoft.com/en-us/windows/win32/medfound/sink-writer)
- [Tracked sample allocator ownership](https://learn.microsoft.com/en-us/windows/win32/api/mfidl/nf-mfidl-imftrackedsample-setallocator)
- [SinkWriter transform inspection](https://learn.microsoft.com/en-us/windows/win32/api/mfreadwrite/nf-mfreadwrite-imfsinkwriterex-gettransformforstream)
- [Hardware transform enumeration](https://learn.microsoft.com/en-us/windows/win32/api/mfapi/nf-mfapi-mftenumex)
- [egui/eframe](https://github.com/emilk/egui)
- [wasapi-rs](https://github.com/HEnquist/wasapi-rs)
- [Process loopback sample](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/)
- [Capture indicator API](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired)
- [RegisterHotKey](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey)
- [D3D11 video processing](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11videocontext-videoprocessorblt)
