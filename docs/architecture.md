# Architecture and dependency decisions

The workspace separates platform-neutral settings and timing (`recorder-core`),
Windows capture/encoding/inspection (`recording-windows`), and UI/CLI (`featherrec`).
Audio will be a separate module once its timing and device lifecycle are validated.

## Capture and frame ownership

`windows-capture` 2.0.1 (MIT) wraps Windows Graphics Capture and D3D11. Its current
source and maintenance activity were inspected before selection. Full-monitor WGC
provides event-driven capture with system-relative timestamps and source closure
notifications. Desktop Duplication is a possible fallback, not a second untested
backend included just for the checklist.

The upstream crate's convenience `VideoEncoder` uses an unbounded channel, so this
application does not use it. FeatherRec owns exactly four source-sized BGRA GPU
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
31.6 MiB at 1080p), plus the OS capture pool, encoder buffers, UI, and driver memory.
The pool limits application frame ownership, not all driver or OS allocations.
The high-water in-flight count is reported so validation can check the invariant.

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
and calls `Finalize`. A source-size change or source closure stops the recording
with an error. Encoder/disk failures are surfaced and partial files retained.
Output names contain a nanosecond timestamp and process ID, with exclusive creation.

On a clean stop, SourceReader reopens the container, verifies dimensions, and decodes
the first frame. Only then is `.partial.mp4` renamed to `.mp4`. This check does not
prove every frame is valid: the external validation script decodes the whole file.
No output is called successful solely because the encoder returned without panic.

Closing the UI requests stop and waits asynchronously for finalization. A destructor
also stops/joins for normal unwinding. Process termination or a hung GPU driver
cannot be made safe by a destructor; MP4 recovery is not guaranteed in those cases.

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

Reports and optional portable preferences are local JSON files. Runtime code has
no network communication. Dependency downloads and GitHub CI are build-time concerns.

## References

- [windows-capture source and MIT license](https://github.com/NiiightmareXD/windows-capture)
- [Media Foundation Sink Writer](https://learn.microsoft.com/en-us/windows/win32/medfound/sink-writer)
- [Tracked sample allocator ownership](https://learn.microsoft.com/en-us/windows/win32/api/mfidl/nf-mfidl-imftrackedsample-setallocator)
- [SinkWriter transform inspection](https://learn.microsoft.com/en-us/windows/win32/api/mfreadwrite/nf-mfreadwrite-imfsinkwriterex-gettransformforstream)
- [Hardware transform enumeration](https://learn.microsoft.com/en-us/windows/win32/api/mfapi/nf-mfapi-mftenumex)
- [egui/eframe](https://github.com/emilk/egui)
