use crate::{capture::SharedMetrics, inspect::MediaRuntime};
use anyhow::{Context, Result, anyhow, bail, ensure};
use recorder_core::{AudioSettings, Settings, SystemAudio};
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Instant,
};
use wasapi::{
    AudioCaptureClient, AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode,
    WaveFormat,
};
use windows::Win32::{Foundation::WAIT_FAILED, System::Threading::WaitForMultipleObjects};

const RATE: usize = 48_000;
const CHUNK: usize = 4_800;
const RING: usize = RATE;

#[derive(Clone, Debug, serde::Serialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct AudioDevices {
    pub outputs: Vec<AudioDevice>,
    pub microphones: Vec<AudioDevice>,
    pub default_output: Option<AudioDevice>,
    pub default_microphone: Option<AudioDevice>,
}

pub fn enumerate_audio_devices() -> Result<AudioDevices> {
    let _runtime = MediaRuntime::new()?;
    let enumerator = DeviceEnumerator::new()?;
    let list = |direction| -> Result<Vec<AudioDevice>> {
        let collection = enumerator.get_device_collection(&direction)?;
        let mut devices = Vec::new();
        for index in 0..collection.get_nbr_devices()? {
            let device = collection.get_device_at_index(index)?;
            devices.push(AudioDevice {
                id: device.get_id()?,
                name: device.get_friendlyname()?,
            });
        }
        Ok(devices)
    };
    Ok(AudioDevices {
        outputs: list(Direction::Render)?,
        microphones: list(Direction::Capture)?,
        default_output: enumerator
            .get_default_device(&Direction::Render)
            .ok()
            .and_then(|d| {
                Some(AudioDevice {
                    id: d.get_id().ok()?,
                    name: d.get_friendlyname().ok()?,
                })
            }),
        default_microphone: enumerator
            .get_default_device(&Direction::Capture)
            .ok()
            .and_then(|d| {
                Some(AudioDevice {
                    id: d.get_id().ok()?,
                    name: d.get_friendlyname().ok()?,
                })
            }),
    })
}

/// Live gain/mute values are the only recording settings changed mid-session.
pub struct AudioControls {
    pub microphone_muted: AtomicBool,
    system_gain: AtomicU32,
    microphone_gain: AtomicU32,
}

impl AudioControls {
    pub fn new(settings: &AudioSettings) -> Self {
        Self {
            microphone_muted: AtomicBool::new(settings.microphone_muted),
            system_gain: AtomicU32::new(settings.system_gain.to_bits()),
            microphone_gain: AtomicU32::new(settings.microphone_gain.to_bits()),
        }
    }
    pub fn update(&self, settings: &AudioSettings) {
        self.microphone_muted
            .store(settings.microphone_muted, Ordering::Relaxed);
        self.system_gain.store(
            settings.system_gain.clamp(0.0, 2.0).to_bits(),
            Ordering::Relaxed,
        );
        self.microphone_gain.store(
            settings.microphone_gain.clamp(0.0, 2.0).to_bits(),
            Ordering::Relaxed,
        );
    }
}

struct Source {
    client: AudioClient,
    capture: AudioCaptureClient,
    event: wasapi::Handle,
    bytes: Vec<u8>,
    samples: Vec<[f32; 2]>,
    tags: Vec<i64>,
    microphone: bool,
    anchor: Option<(u64, i64)>,
    device_id: String,
    label: String,
}

impl Source {
    fn new(settings: &AudioSettings, microphone: bool, process: Option<u32>) -> Result<Self> {
        let mut device_id = String::new();
        let mut label = String::from("Uygulama sesi");
        let mut client = if !microphone && settings.source == SystemAudio::Application {
            AudioClient::new_application_loopback_client(process.context("Select the game's window first")?, true)
                .context("Application-only audio needs Windows build 20348 or later and a running target process; it will not fall back to system audio")?
        } else {
            let direction = if microphone {
                Direction::Capture
            } else {
                Direction::Render
            };
            let id = if microphone {
                &settings.microphone_device
            } else {
                &settings.output_device
            };
            let enumerator = DeviceEnumerator::new()?;
            let device = if id.is_empty() {
                enumerator.get_default_device(&direction)?
            } else {
                enumerator.get_device(id)?
            };
            device_id = device.get_id()?;
            label = device.get_friendlyname()?;
            device.get_iaudioclient()?
        };
        let format = WaveFormat::new(32, 32, &SampleType::Float, RATE, 2, None);
        client.initialize_client(&format, &Direction::Capture, &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 200_000 })
            .context("Cannot open the selected audio source at 48 kHz stereo. Check the device and Windows microphone permissions")?;
        let event = client.set_get_eventhandle()?;
        let capture = client.get_audiocaptureclient()?;
        client.start_stream()?;
        Ok(Self {
            client,
            capture,
            event,
            bytes: vec![0; RATE * 8],
            samples: vec![[0.0; 2]; RING],
            tags: vec![-1; RING],
            microphone,
            anchor: None,
            device_id,
            label,
        })
    }

    fn follow_default(&mut self, settings: &AudioSettings, process: Option<u32>) -> Result<bool> {
        let id = if self.microphone {
            &settings.microphone_device
        } else {
            &settings.output_device
        };
        if !id.is_empty() || (!self.microphone && settings.source == SystemAudio::Application) {
            return Ok(false);
        }
        let direction = if self.microphone {
            Direction::Capture
        } else {
            Direction::Render
        };
        let default = DeviceEnumerator::new()?
            .get_default_device(&direction)?
            .get_id()?;
        if default == self.device_id {
            return Ok(false);
        }
        let mut next = Self::new(settings, self.microphone, process)?;
        std::mem::swap(&mut next.samples, &mut self.samples);
        std::mem::swap(&mut next.tags, &mut self.tags);
        *self = next;
        Ok(true)
    }

    fn peak(&mut self) -> Result<f32> {
        let mut peak = 0.0f32;
        while let Some(count) = self.capture.get_next_packet_size()? {
            if count == 0 {
                break;
            }
            ensure!(
                count as usize <= RATE,
                "Audio packet exceeds the preview buffer"
            );
            let (frames, info) = self.capture.read_from_device(&mut self.bytes)?;
            if !info.flags.silent {
                for value in self.bytes[..frames as usize * 8].chunks_exact(4) {
                    let value = f32::from_le_bytes(value.try_into().unwrap());
                    if value.is_finite() {
                        peak = peak.max(value.abs());
                    }
                }
            }
        }
        Ok(peak)
    }

    fn drain(&mut self, base: i64, now: i64, written: i64, metrics: &SharedMetrics) -> Result<()> {
        while let Some(count) = self.capture.get_next_packet_size()? {
            if count == 0 {
                break;
            }
            ensure!(
                count as usize <= RATE,
                "Audio device exceeded the bounded one-second packet buffer"
            );
            let (frames, info) = self.capture.read_from_device(&mut self.bytes)?;
            if base == 0 {
                continue;
            }
            let timestamp = if info.flags.timestamp_error || info.timestamp == 0 {
                self.anchor.map_or(
                    now - i64::from(frames) * 10_000_000 / RATE as i64,
                    |(index, time)| {
                        time + (info.index as i64 - index as i64) * 10_000_000 / RATE as i64
                    },
                )
            } else {
                info.timestamp as i64
            };
            self.anchor = Some((info.index, timestamp));
            if info.flags.data_discontinuity || info.flags.timestamp_error {
                metrics.lock().unwrap().audio_discontinuities += 1;
            }
            let start = ((i128::from(timestamp - base) * RATE as i128) / 10_000_000) as i64;
            let mut late = false;
            for index in 0..frames as usize {
                let frame = start + index as i64;
                if frame < 0 {
                    continue;
                }
                if frame < written {
                    late = true;
                    continue;
                }
                ensure!(
                    frame < written + RING as i64,
                    "Audio timing exceeded the bounded one-second jitter buffer"
                );
                let offset = index * 8;
                let sample = if info.flags.silent {
                    [0.0; 2]
                } else {
                    [
                        f32::from_le_bytes(self.bytes[offset..offset + 4].try_into().unwrap()),
                        f32::from_le_bytes(self.bytes[offset + 4..offset + 8].try_into().unwrap()),
                    ]
                };
                let slot = frame as usize % RING;
                self.samples[slot] = sample.map(|s| if s.is_finite() { s } else { 0.0 });
                self.tags[slot] = frame;
            }
            if late {
                let mut m = metrics.lock().unwrap();
                m.audio_discontinuities += 1;
                m.warning = Some("Audio packets arrived after the jitter window; some audio may be missing. Check the selected device.".into());
            }
        }
        Ok(())
    }

    fn sample(&self, frame: i64) -> [f32; 2] {
        let slot = frame as usize % RING;
        if self.tags[slot] == frame {
            self.samples[slot]
        } else {
            [0.0; 2]
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        let _ = self.client.stop_stream();
    }
}

#[derive(Clone, Debug, Default)]
pub struct AudioPreviewState {
    pub running: bool,
    pub system_peak: f32,
    pub microphone_peak: f32,
    pub devices: Vec<String>,
    pub error: Option<String>,
}

/// Capture-only meter: no file, playback, monitoring or video encoder is opened.
pub struct AudioPreview {
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<AudioPreviewState>>,
    worker: Option<JoinHandle<()>>,
    pub controls: Arc<AudioControls>,
}
impl AudioPreview {
    pub fn start(settings: &Settings) -> Result<Self> {
        ensure!(
            settings.audio.enabled(),
            "Önce sistem, uygulama veya mikrofon sesini aç."
        );
        let config = settings.audio.clone();
        let process = config
            .application
            .as_ref()
            .or(settings.window.as_ref())
            .map(|w| w.process);
        let controls = Arc::new(AudioControls::new(&config));
        let worker_controls = controls.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let state = Arc::new(Mutex::new(AudioPreviewState::default()));
        let thread_state = state.clone();
        let worker = thread::Builder::new()
            .name("featherrec-audio-preview".into())
            .spawn(move || {
                let result = (|| -> Result<()> {
                    let _runtime = MediaRuntime::new()?;
                    let mut sources = Vec::with_capacity(2);
                    if config.source != SystemAudio::Off {
                        sources.push(Source::new(&config, false, process)?);
                    }
                    if config.microphone {
                        sources.push(Source::new(&config, true, process)?);
                    }
                    let mut device_check = Instant::now();
                    while !thread_stop.load(Ordering::Relaxed) {
                        let handles: Vec<_> = sources.iter().map(|s| s.event.as_handle()).collect();
                        let wait = unsafe { WaitForMultipleObjects(&handles, false, 20) };
                        ensure!(wait != WAIT_FAILED, "Ses aygıtı beklenirken hata oluştu");
                        if device_check.elapsed().as_secs() >= 1 {
                            for source in &mut sources {
                                source.follow_default(&config, process)?;
                            }
                            device_check = Instant::now();
                        }
                        let mut peaks = [0.0f32; 2];
                        for source in &mut sources {
                            let peak = match source.peak() {
                                Ok(peak) => peak,
                                Err(error) => {
                                    if source.follow_default(&config, process)? {
                                        0.0
                                    } else {
                                        return Err(error);
                                    }
                                }
                            };
                            let gain = if source.microphone {
                                if worker_controls.microphone_muted.load(Ordering::Relaxed) {
                                    0.0
                                } else {
                                    f32::from_bits(
                                        worker_controls.microphone_gain.load(Ordering::Relaxed),
                                    )
                                }
                            } else {
                                f32::from_bits(worker_controls.system_gain.load(Ordering::Relaxed))
                            };
                            peaks[usize::from(source.microphone)] = peak * gain;
                        }
                        let mut status = thread_state.lock().unwrap();
                        status.running = true;
                        status.system_peak = peaks[0].max(status.system_peak * 0.85);
                        status.microphone_peak = peaks[1].max(status.microphone_peak * 0.85);
                        status.devices = sources.iter().map(|s| s.label.clone()).collect();
                    }
                    Ok(())
                })();
                let mut status = thread_state.lock().unwrap();
                status.running = false;
                if let Err(error) = result {
                    status.error = Some(format!("Ses denemesi: {error:#}"));
                }
            })?;
        Ok(Self {
            stop,
            state,
            worker: Some(worker),
            controls,
        })
    }
    pub fn snapshot(&self) -> AudioPreviewState {
        self.state.lock().unwrap().clone()
    }
}
impl Drop for AudioPreview {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct WaveWriter {
    file: File,
    bytes: u64,
}
impl WaveWriter {
    fn new(path: &Path) -> Result<Self> {
        let mut file = File::options().write(true).create_new(true).open(path)?;
        file.write_all(&[0; 44])?;
        Ok(Self { file, bytes: 0 })
    }
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            self.bytes + bytes.len() as u64 <= u64::from(u32::MAX) - 36,
            "Temporary WAV reached its 4 GiB limit; stop the recording"
        );
        self.file
            .write_all(bytes)
            .context("Audio write failed; check free storage")?;
        self.bytes += bytes.len() as u64;
        Ok(())
    }
    fn finish(mut self) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(b"RIFF")?;
        self.file
            .write_all(&(self.bytes as u32 + 36).to_le_bytes())?;
        self.file.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x02\0")?;
        self.file.write_all(&(RATE as u32).to_le_bytes())?;
        self.file.write_all(&(RATE as u32 * 4).to_le_bytes())?;
        self.file.write_all(b"\x04\0\x10\0data")?;
        self.file.write_all(&(self.bytes as u32).to_le_bytes())?;
        self.file.flush()?;
        Ok(())
    }
}

pub(crate) struct AudioRecorder {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<()>>>,
    pub paths: Vec<PathBuf>,
}

impl AudioRecorder {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        settings: &Settings,
        path: PathBuf,
        base: Arc<AtomicI64>,
        clock: Arc<Mutex<Option<Instant>>>,
        stop_at: Arc<AtomicI64>,
        controls: Arc<AudioControls>,
        metrics: SharedMetrics,
    ) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let config = settings.audio.clone();
        let process = settings
            .audio
            .application
            .as_ref()
            .or(settings.window.as_ref())
            .map(|w| w.process);
        let paths: Vec<_> = (0..config.tracks.len())
            .map(|i| path.with_extension(format!("track{}.wav", i + 1)))
            .collect();
        let outputs = paths.clone();
        let (ready, started) = mpsc::sync_channel::<Result<(), String>>(1);
        let worker = thread::Builder::new()
            .name("featherrec-audio".into())
            .spawn(move || {
                let result = (|| {
                    let _runtime = MediaRuntime::new()?;
                    let mut sources = Vec::with_capacity(2);
                    if config.source != SystemAudio::Off {
                        sources.push(Source::new(&config, false, process)?);
                    }
                    if config.microphone {
                        sources.push(Source::new(&config, true, process)?);
                    }
                    let mut writers: Vec<_> = outputs
                        .iter()
                        .map(|p| WaveWriter::new(p))
                        .collect::<Result<_>>()?;
                    let mut bytes = vec![vec![0u8; CHUNK * 4]; config.tracks.len()];
                    let mut written = 0i64;
                    let mut device_check = Instant::now();
                    let _ = ready.send(Ok(()));
                    loop {
                        // Events wake on real audio. The timeout advances silence and
                        // bounds stop latency even when no application is playing sound.
                        let handles: Vec<_> = sources.iter().map(|s| s.event.as_handle()).collect();
                        let wait = unsafe { WaitForMultipleObjects(&handles, false, 20) };
                        ensure!(wait != WAIT_FAILED, "Waiting for the audio device failed");
                        let base_time = base.load(Ordering::Relaxed);
                        let elapsed = clock
                            .lock()
                            .unwrap()
                            .map_or(0, |t| (t.elapsed().as_secs_f64() * 10_000_000.0) as i64);
                        if device_check.elapsed().as_secs() >= 1 {
                            for source in &mut sources {
                                if source.follow_default(&config, process)? {
                                    metrics.lock().unwrap().warning = Some(format!(
                                        "Windows varsayılan ses aygıtına geçildi: {}",
                                        source.label
                                    ));
                                }
                            }
                            device_check = Instant::now();
                        }
                        for source in &mut sources {
                            if let Err(error) =
                                source.drain(base_time, base_time + elapsed, written, &metrics)
                            {
                                if !source.follow_default(&config, process)? {
                                    return Err(error);
                                }
                            }
                        }
                        let stopping = thread_stop.load(Ordering::Relaxed);
                        let end = if stopping {
                            stop_at.load(Ordering::Relaxed)
                        } else {
                            (elapsed - 1_000_000).max(0)
                        };
                        let target = end * RATE as i64 / 10_000_000;
                        while written < target {
                            let frames = (target - written).min(CHUNK as i64) as usize;
                            let system_gain =
                                f32::from_bits(controls.system_gain.load(Ordering::Relaxed));
                            let mic_gain = if controls.microphone_muted.load(Ordering::Relaxed) {
                                0.0
                            } else {
                                f32::from_bits(controls.microphone_gain.load(Ordering::Relaxed))
                            };
                            let mut peaks = [0.0f32; 2];
                            for index in 0..frames {
                                let mut source_samples = [[0.0f32; 2]; 2];
                                for source in &sources {
                                    let gain = if source.microphone {
                                        mic_gain
                                    } else {
                                        system_gain
                                    };
                                    for (channel, value) in
                                        source.sample(written + index as i64).iter().enumerate()
                                    {
                                        let scaled = value * gain;
                                        peaks[usize::from(source.microphone)] =
                                            peaks[usize::from(source.microphone)].max(scaled.abs());
                                        source_samples[usize::from(source.microphone)][channel] +=
                                            scaled;
                                    }
                                }
                                for (track, buffer) in config.tracks.iter().zip(&mut bytes) {
                                    for channel in 0..2 {
                                        let value = if track.system {
                                            source_samples[0][channel]
                                        } else {
                                            0.0
                                        } + if track.microphone {
                                            source_samples[1][channel]
                                        } else {
                                            0.0
                                        };
                                        let pcm = (value.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                                        buffer
                                            [index * 4 + channel * 2..index * 4 + channel * 2 + 2]
                                            .copy_from_slice(&pcm.to_le_bytes());
                                    }
                                }
                            }
                            for (writer, buffer) in writers.iter_mut().zip(&bytes) {
                                writer.write(&buffer[..frames * 4])?;
                            }
                            written += frames as i64;
                            let mut m = metrics.lock().unwrap();
                            m.audio_frames = written as u64;
                            m.system_peak = peaks[0];
                            m.microphone_peak = peaks[1];
                        }
                        if stopping {
                            break;
                        }
                    }
                    drop(sources);
                    for writer in writers {
                        writer.finish()?;
                    }
                    Ok(())
                })();
                if let Err(error) = &result {
                    let message = format!("Audio recording failed: {error:#}");
                    metrics.lock().unwrap().error = Some(message.clone());
                    let _ = ready.try_send(Err(message));
                }
                result
            })?;
        match started.recv_timeout(std::time::Duration::from_secs(15)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                worker: Some(worker),
                paths,
            }),
            other => {
                stop.store(true, Ordering::Relaxed);
                let _ = worker.join();
                bail!(
                    "{}",
                    match other {
                        Ok(Err(e)) => e,
                        _ => "Audio initialization timed out".into(),
                    }
                )
            }
        }
    }
    pub fn stop(&mut self) -> Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| anyhow!("Audio worker panicked"))??;
        }
        Ok(())
    }
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for AudioRecorder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
