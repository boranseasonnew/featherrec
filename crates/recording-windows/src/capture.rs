use crate::{
    encoder::{Device, Encoder, Texture, validate_video},
    inspect::{MediaRuntime, process_resources},
};
use anyhow::{Context as _, Result, anyhow, bail, ensure};
use recorder_core::{
    FramePacer, Metrics, PressureMonitor, RecordingReport, Settings as RecordingSettings,
    TEXTURE_LIMIT,
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::{
    Win32::Graphics::{Direct3D11::*, Dxgi::Common::*},
    core::Interface,
};
use windows_capture::{
    capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::*,
};

pub type SharedMetrics = Arc<Mutex<Metrics>>;

struct Packet {
    texture: Texture,
    pts: i64,
}

struct Flags {
    settings: RecordingSettings,
    source: (u32, u32),
    partial: PathBuf,
    metrics: SharedMetrics,
    clock: Arc<Mutex<Option<Instant>>>,
    in_flight: Arc<AtomicUsize>,
    stop_at: Arc<AtomicI64>,
}

struct Handler {
    flags: Flags,
    available: Receiver<Texture>,
    return_texture: SyncSender<Texture>,
    sender: Option<SyncSender<Packet>>,
    worker: Option<JoinHandle<Result<()>>>,
    pacer: FramePacer,
    first_frame: bool,
}

// The capture library serializes callbacks with its mutex. D3D11's immediate
// context is protected with ID3D11Multithread before the worker starts.
unsafe impl Send for Handler {}

impl Drop for Handler {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = anyhow::Error;

    fn new(ctx: Context<Flags>) -> Result<Self> {
        let flags = ctx.flags;
        unsafe {
            let protection: ID3D11Multithread = ctx.device_context.cast()?;
            let _ = protection.SetMultithreadProtected(true);
        }
        let (return_texture, available) = mpsc::sync_channel(TEXTURE_LIMIT);
        for _ in 0..TEXTURE_LIMIT {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: flags.source.0,
                Height: flags.source.1,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32 | D3D11_BIND_SHADER_RESOURCE.0 as u32,
                ..Default::default()
            };
            let mut texture = None;
            unsafe {
                ctx.device
                    .CreateTexture2D(&desc, None, Some(&mut texture))?;
            }
            return_texture.send(Texture(texture.context("GPU texture allocation failed")?))?;
        }
        let (sender, receiver) = mpsc::sync_channel(TEXTURE_LIMIT);
        let worker_settings = flags.settings.clone();
        let worker_source = flags.source;
        let worker_path = flags.partial.clone();
        let worker_metrics = flags.metrics.clone();
        let worker_in_flight = flags.in_flight.clone();
        let (ready, ready_receiver) = mpsc::sync_channel::<Result<(), String>>(1);
        let stop_at = flags.stop_at.clone();
        let pool = return_texture.clone();
        let device = Device(ctx.device);
        let worker = thread::Builder::new().name("featherrec-encoder".into()).spawn(move || {
            let result = (|| {
                let _runtime = MediaRuntime::new()?;
                let encoder = Encoder::new(device.get(), &worker_settings, worker_source, &worker_path)?;
                {
                    let mut m = worker_metrics.lock().unwrap();
                    m.encoder_name = encoder.name.clone();
                    m.hardware_verified = encoder.hardware;
                    if !encoder.hardware { m.warning = Some("Software or unidentified encoder: CPU usage may be high.".into()); }
                }
                let _ = ready.send(Ok(()));
                let mut pending: Option<Packet> = None;
                let mut total_ms = 0.0;
                let mut submit = |packet: Packet, duration: i64| -> Result<()> {
                    let started = Instant::now();
                    encoder.write(packet.texture, packet.pts, duration, &pool, &worker_in_flight)?;
                    let ms = started.elapsed().as_secs_f64() * 1000.0;
                    total_ms += ms;
                    let mut m = worker_metrics.lock().unwrap();
                    m.submitted += 1;
                    m.average_submit_ms = total_ms / m.submitted as f64;
                    m.max_submit_ms = m.max_submit_ms.max(ms);
                    Ok(())
                };
                while let Ok(packet) = receiver.recv() {
                    if let Some(previous) = pending.replace(packet) {
                        let duration = pending.as_ref().unwrap().pts - previous.pts;
                        submit(previous, duration)?;
                    }
                }
                if let Some(packet) = pending {
                    let end = stop_at.load(Ordering::Relaxed);
                    let duration = (end - packet.pts).max(10_000_000 / i64::from(worker_settings.fps));
                    submit(packet, duration)?;
                } else { bail!("Windows did not deliver any frames. Check that the display is connected and the session is unlocked."); }
                encoder.finish()?;
                Ok(())
            })();
            if let Err(error) = &result {
                let message = format!("{error:#}");
                worker_metrics.lock().unwrap().error = Some(message.clone());
                let _ = ready.try_send(Err(message));
            }
            result
        })?;
        // This runs before the capture session starts, outside any frame callback.
        // It prevents initialization latency from appearing as recorded frame loss.
        ready_receiver
            .recv_timeout(Duration::from_secs(30))
            .context("Encoder initialization did not complete")?
            .map_err(anyhow::Error::msg)?;
        Ok(Self {
            flags,
            available,
            return_texture,
            sender: Some(sender),
            worker: Some(worker),
            pacer: FramePacer::default(),
            first_frame: true,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<()> {
        if self.flags.metrics.lock().unwrap().error.is_some() {
            control.stop();
            return Ok(());
        }
        ensure!(
            (frame.width(), frame.height()) == self.flags.source,
            "Display size changed. Recording stopped; restart with the new resolution."
        );
        self.flags.metrics.lock().unwrap().captured += 1;
        let Some(pts) = self
            .pacer
            .accept(frame.timestamp()?.Duration, self.flags.settings.fps)
        else {
            self.flags.metrics.lock().unwrap().paced_out += 1;
            return Ok(());
        };
        if self.first_frame {
            *self.flags.clock.lock().unwrap() = Some(Instant::now());
            self.first_frame = false;
        }
        let texture = match self.available.try_recv() {
            Ok(texture) => texture,
            Err(TryRecvError::Empty) => {
                self.flags.metrics.lock().unwrap().dropped += 1;
                return Ok(());
            }
            Err(TryRecvError::Disconnected) => bail!("Encoder texture pool disconnected"),
        };
        let count = self.flags.in_flight.fetch_add(1, Ordering::Relaxed) + 1;
        {
            let mut m = self.flags.metrics.lock().unwrap();
            m.max_in_flight = m.max_in_flight.max(count);
        }
        unsafe {
            frame
                .device_context()
                .CopyResource(&texture.0, frame.as_raw_texture());
            // Publish the GPU commands before the MF worker consumes the resource.
            frame.device_context().Flush();
        }
        if let Some(sender) = &self.sender
            && let Err(error) = sender.try_send(Packet { texture, pts })
        {
            self.flags.in_flight.fetch_sub(1, Ordering::Relaxed);
            match error {
                mpsc::TrySendError::Full(packet) => {
                    let _ = self.return_texture.try_send(packet.texture);
                    self.flags.metrics.lock().unwrap().dropped += 1;
                }
                mpsc::TrySendError::Disconnected(_) => bail!("Encoder stopped unexpectedly"),
            }
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<()> {
        self.flags.metrics.lock().unwrap().error =
            Some("Capture source closed or monitor disconnected.".into());
        Ok(())
    }
}

pub struct Recording {
    control: Option<CaptureControl<Handler, anyhow::Error>>,
    settings: RecordingSettings,
    partial: PathBuf,
    complete: PathBuf,
    metrics: SharedMetrics,
    clock: Arc<Mutex<Option<Instant>>>,
    in_flight: Arc<AtomicUsize>,
    stop_at: Arc<AtomicI64>,
    last_poll: Instant,
    cpu_ticks: u64,
    pressure: PressureMonitor,
}

pub fn start(settings: RecordingSettings, metrics: SharedMetrics) -> Result<Recording> {
    settings.validate()?;
    fs::create_dir_all(&settings.output_dir).context("Cannot create output folder")?;
    let folder = fs::canonicalize(&settings.output_dir)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let stem = format!("FeatherRec-{stamp}-{}", std::process::id());
    let partial = folder.join(format!("{stem}.partial.mp4"));
    let complete = folder.join(format!("{stem}.mp4"));
    // Never overwrite an existing recording, even in the unlikely event of a collision.
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial)?;
    let monitor =
        Monitor::from_index(settings.monitor).context("Selected monitor is no longer available")?;
    let source = (monitor.width()?, monitor.height()?);
    if let Ok(hz) = monitor.refresh_rate()
        && hz > 0
        && settings.fps > hz
    {
        metrics.lock().unwrap().warning = Some(format!(
            "Selected {} FPS exceeds this display's {hz} Hz refresh rate; unique frame delivery will be lower.",
            settings.fps
        ));
    }
    let clock = Arc::new(Mutex::new(None));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let stop_at = Arc::new(AtomicI64::new(0));
    let flags = Flags {
        settings: settings.clone(),
        source,
        partial: partial.clone(),
        metrics: metrics.clone(),
        clock: clock.clone(),
        in_flight: in_flight.clone(),
        stop_at: stop_at.clone(),
    };
    let capture_settings = Settings::new(
        monitor,
        if settings.cursor {
            CursorCaptureSettings::WithCursor
        } else {
            CursorCaptureSettings::WithoutCursor
        },
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        flags,
    );
    let control = Handler::start_free_threaded(capture_settings)
        .map_err(|e| anyhow!("Capture initialization failed: {e}"))?;
    Ok(Recording {
        control: Some(control),
        settings,
        partial,
        complete,
        metrics,
        clock,
        in_flight,
        stop_at,
        last_poll: Instant::now(),
        cpu_ticks: process_resources().1,
        pressure: PressureMonitor::default(),
    })
}

impl Recording {
    pub fn elapsed(&self) -> Duration {
        self.clock
            .lock()
            .unwrap()
            .map_or(Duration::ZERO, |t| t.elapsed())
    }

    pub fn is_finished(&self) -> bool {
        self.control.as_ref().is_none_or(|c| c.is_finished())
            || self.metrics.lock().unwrap().error.is_some()
    }

    pub fn poll(&mut self) {
        let seconds = self.last_poll.elapsed().as_secs_f64();
        if seconds < 1.0 {
            return;
        }
        self.last_poll = Instant::now();
        let (memory, cpu) = process_resources();
        let mut m = self.metrics.lock().unwrap();
        m.elapsed_seconds = self
            .clock
            .lock()
            .unwrap()
            .map_or(0.0, |t| t.elapsed().as_secs_f64());
        m.working_set_bytes = memory;
        let cores = thread::available_parallelism().map_or(1, usize::from) as f64;
        m.process_cpu_percent =
            (cpu.saturating_sub(self.cpu_ticks) as f64 / 10_000_000.0 / seconds / cores * 100.0)
                .clamp(0.0, 100.0);
        self.cpu_ticks = cpu;
        m.in_flight = self.in_flight.load(Ordering::Relaxed);
        m.output_bytes = fs::metadata(&self.partial).map_or(0, |f| f.len());
        if self
            .pressure
            .observe(m.dropped, m.average_submit_ms, self.settings.fps)
        {
            m.warning = Some("Sustained encoder pressure. For the next recording, try Potato mode or 30 FPS. Settings have not been changed.".into());
        }
    }

    fn close_pipeline(&mut self) -> Result<()> {
        if let Some(control) = self.control.take() {
            self.stop_at.store(
                (self.elapsed().as_secs_f64() * 10_000_000.0) as i64,
                Ordering::Relaxed,
            );
            let callback = control.callback();
            let capture_result = control
                .stop()
                .map_err(|e| anyhow!("Capture stopped with an error: {e}"));
            let worker = {
                let mut handler = callback.lock();
                handler.sender.take();
                handler.worker.take()
            };
            let encode_result = if let Some(worker) = worker {
                worker
                    .join()
                    .map_err(|_| anyhow!("Encoder thread panicked"))?
            } else {
                Ok(())
            };
            capture_result?;
            encode_result?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<RecordingReport> {
        let elapsed = self
            .clock
            .lock()
            .unwrap()
            .map_or(0.0, |t| t.elapsed().as_secs_f64());
        if let Err(error) = self.close_pipeline() {
            self.metrics.lock().unwrap().error = Some(format!("{error:#}"));
        }
        let clean = self.metrics.lock().unwrap().error.is_none();
        let validation = if clean {
            (|| {
                let _runtime = MediaRuntime::new()?;
                validate_video(&self.partial, self.settings.width, self.settings.height)
            })()
        } else {
            Err(anyhow!(
                "Capture or encoder failed; inspect the retained partial file"
            ))
        };
        let (file, finalized, validation) = match validation {
            Ok(()) => match fs::rename(&self.partial, &self.complete) {
                Ok(()) => (self.complete.clone(), true, "Media Foundation reopened the MP4, verified dimensions and decoded its first frame.".to_string()),
                Err(error) => (self.partial.clone(), false, format!("MP4 verified, but rename failed: {error}")),
            },
            Err(error) => (self.partial.clone(), false, format!("{error:#}")),
        };
        let mut metrics = self.metrics.lock().unwrap().clone();
        metrics.elapsed_seconds = elapsed;
        metrics.in_flight = 0;
        metrics.output_bytes = fs::metadata(&file).map_or(0, |f| f.len());
        if !finalized && metrics.error.is_none() {
            metrics.error = Some(validation.clone());
        }
        let report = RecordingReport {
            settings: self.settings.clone(),
            metrics,
            file,
            finalized,
            validation,
        };
        let report_path = self.complete.with_extension("recording.json");
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&report_path)
            .with_context(|| {
                format!(
                    "Recording saved but report could not be created: {}",
                    report_path.display()
                )
            })?;
        output.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
        Ok(report)
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        let _ = self.close_pipeline();
    }
}
