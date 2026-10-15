use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application-owned GPU textures, including queued and encoder-retained samples.
pub const TEXTURE_LIMIT: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    Compact,
    Potato,
    Smooth720,
    Balanced30,
    Balanced,
    Quality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncoderPolicy {
    /// Initialization fails unless the selected encoder can be identified as hardware.
    Hardware,
    /// Explicit opt-in: Media Foundation may select a software encoder.
    AllowSoftware,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WindowSelection {
    pub handle: u64,
    pub process: u32,
    pub title: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemAudio {
    #[default]
    Off,
    System,
    Application,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioTrack {
    pub system: bool,
    pub microphone: bool,
}
impl Default for AudioTrack {
    fn default() -> Self {
        Self {
            system: true,
            microphone: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateControl {
    #[default]
    Bitrate,
    Cqp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub source: SystemAudio,
    pub output_device: String,
    pub microphone: bool,
    pub microphone_device: String,
    pub microphone_muted: bool,
    pub system_gain: f32,
    pub microphone_gain: f32,
    pub bitrate: u32,
    pub tracks: Vec<AudioTrack>,
    /// Process handles must be reselected after restart, independently of video.
    #[serde(skip)]
    pub application: Option<WindowSelection>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            source: SystemAudio::Off,
            output_device: String::new(),
            microphone: false,
            microphone_device: String::new(),
            microphone_muted: false,
            system_gain: 1.0,
            microphone_gain: 1.0,
            bitrate: 192_000,
            tracks: vec![AudioTrack::default()],
            application: None,
        }
    }
}

impl AudioSettings {
    pub fn enabled(&self) -> bool {
        self.source != SystemAudio::Off || self.microphone
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub monitor: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    pub rate_control: RateControl,
    pub qp: u32,
    pub encoder: EncoderPolicy,
    pub output_dir: PathBuf,
    pub preset: Preset,
    pub cursor: bool,
    pub buffer_frames: u8,
    pub low_latency: bool,
    pub write_report: bool,
    pub show_capture_border: bool,
    /// Window handles are transient and must never be silently reused after restart.
    #[serde(skip)]
    pub window: Option<WindowSelection>,
    pub audio: AudioSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            monitor: 1,
            width: 1280,
            height: 720,
            fps: 30,
            bitrate: 4_000_000,
            rate_control: RateControl::Bitrate,
            qp: 24,
            encoder: EncoderPolicy::Hardware,
            output_dir: PathBuf::from("recordings"),
            preset: Preset::Potato,
            cursor: true,
            buffer_frames: TEXTURE_LIMIT as u8,
            low_latency: true,
            write_report: true,
            show_capture_border: false,
            window: None,
            audio: AudioSettings::default(),
        }
    }
}

impl Settings {
    pub fn apply_preset(&mut self, preset: Preset) {
        self.preset = preset;
        self.rate_control = RateControl::Bitrate;
        let (w, h, fps, bitrate) = match preset {
            Preset::Compact => (1280, 720, 30, 2_500_000),
            Preset::Potato => (1280, 720, 30, 4_000_000),
            Preset::Smooth720 => (1280, 720, 60, 6_000_000),
            Preset::Balanced30 => (1920, 1080, 30, 6_000_000),
            Preset::Balanced => (1920, 1080, 60, 8_000_000),
            Preset::Quality => (1920, 1080, 60, 16_000_000),
        };
        (self.width, self.height, self.fps, self.bitrate) = (w, h, fps, bitrate);
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.monitor > 0, "Select an available monitor");
        ensure!(
            matches!((self.width, self.height), (1280, 720) | (1920, 1080)),
            "Choose 720p or 1080p"
        );
        ensure!(matches!(self.fps, 30 | 60), "Choose 30 or 60 FPS");
        ensure!(
            self.audio.source != SystemAudio::Application
                || self.audio.application.is_some()
                || self.window.is_some(),
            "Select a window before using application-only audio"
        );
        ensure!(
            (1..=5).contains(&self.audio.tracks.len()),
            "Choose between one and five audio tracks"
        );
        ensure!(
            (16..=51).contains(&self.qp),
            "CQP must be between 16 and 51"
        );
        ensure!(
            [96_000, 128_000, 192_000, 256_000].contains(&self.audio.bitrate),
            "Choose a supported AAC bitrate"
        );
        ensure!(
            (0.0..=2.0).contains(&self.audio.system_gain)
                && (0.0..=2.0).contains(&self.audio.microphone_gain),
            "Audio gain must be finite and between 0 and 200%"
        );
        ensure!(
            (3..=8).contains(&self.buffer_frames),
            "GPU frame pool must contain 3 to 8 textures"
        );
        ensure!(
            (1_000_000..=50_000_000).contains(&self.bitrate),
            "Bitrate must be between 1 and 50 Mbps"
        );
        ensure!(
            !self.output_dir.as_os_str().is_empty(),
            "Choose an output folder"
        );
        Ok(())
    }
}

/// Paces against capture's monotonic 100 ns timestamps without accumulating rounding drift.
#[derive(Default)]
pub struct FramePacer {
    first: Option<i64>,
    next_slot: i64,
    last: Option<i64>,
}

impl FramePacer {
    pub fn accept(&mut self, timestamp: i64, fps: u32) -> Option<i64> {
        if self.last.is_some_and(|last| timestamp <= last) {
            return None;
        }
        self.last = Some(timestamp);
        let first = *self.first.get_or_insert(timestamp);
        let elapsed = timestamp - first;
        let slot = elapsed.saturating_mul(i64::from(fps)) / 10_000_000;
        if slot < self.next_slot {
            return None;
        }
        self.next_slot = slot + 1;
        Some(elapsed)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub elapsed_seconds: f64,
    pub captured: u64,
    /// Samples accepted by SinkWriter, not a claim about hardware completion.
    pub submitted: u64,
    pub paced_out: u64,
    pub dropped: u64,
    pub in_flight: usize,
    pub max_in_flight: usize,
    pub average_submit_ms: f64,
    pub max_submit_ms: f64,
    pub working_set_bytes: u64,
    pub process_cpu_percent: f64,
    pub output_bytes: u64,
    pub encoder_name: String,
    pub hardware_verified: bool,
    pub warning: Option<String>,
    pub error: Option<String>,
    pub audio_frames: u64,
    pub audio_discontinuities: u64,
    pub system_peak: f32,
    pub microphone_peak: f32,
    #[serde(default)]
    pub source_size_changes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecordingReport {
    pub settings: Settings,
    pub metrics: Metrics,
    pub file: PathBuf,
    pub finalized: bool,
    pub validation: String,
}

/// Hysteresis is deliberately one-way: suggest once after sustained pressure; never change settings.
#[derive(Default)]
pub struct PressureMonitor {
    previous_drops: u64,
    pressured_seconds: u32,
    warned: bool,
}

impl PressureMonitor {
    pub fn observe(&mut self, drops: u64, average_ms: f64, fps: u32) -> bool {
        let pressure = drops > self.previous_drops || average_ms > 1000.0 / f64::from(fps);
        self.previous_drops = drops;
        self.pressured_seconds = if pressure {
            self.pressured_seconds + 1
        } else {
            0
        };
        if !self.warned && self.pressured_seconds >= 3 {
            self.warned = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_track_limits_cqp_and_independent_audio_application() {
        let mut settings = Settings::default();
        settings.audio.tracks.clear();
        assert!(settings.validate().is_err());
        settings.audio.tracks = vec![AudioTrack::default(); 5];
        settings.rate_control = RateControl::Cqp;
        settings.qp = 24;
        settings.validate().unwrap();
        settings.audio.tracks.push(AudioTrack::default());
        assert!(settings.validate().is_err());
        settings.audio.tracks.pop();
        settings.qp = 52;
        assert!(settings.validate().is_err());
        settings.qp = 24;
        settings.audio.source = SystemAudio::Application;
        assert!(settings.validate().is_err());
        settings.audio.application = Some(WindowSelection {
            handle: 1,
            process: 2,
            title: "Game".into(),
        });
        settings.validate().unwrap();
        let restored: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(restored.audio.application.is_none());
        assert_eq!(restored.audio.tracks.len(), 5);
        assert_eq!(restored.rate_control, RateControl::Cqp);
    }

    #[test]
    fn presets_are_valid_and_preserve_user_output() {
        let mut s = Settings {
            output_dir: PathBuf::from("custom"),
            ..Settings::default()
        };
        for p in [
            Preset::Compact,
            Preset::Potato,
            Preset::Smooth720,
            Preset::Balanced30,
            Preset::Balanced,
            Preset::Quality,
        ] {
            s.apply_preset(p);
            s.validate().unwrap();
            assert_eq!(s.output_dir, PathBuf::from("custom"));
        }
    }

    #[test]
    fn rejects_invalid_recording_settings() {
        let mut s = Settings {
            fps: 0,
            ..Settings::default()
        };
        assert!(s.validate().is_err());
        s.fps = 30;
        s.width = 1279;
        assert!(s.validate().is_err());
    }

    #[test]
    fn pacing_rejects_duplicates_and_handles_long_gaps() {
        let mut p = FramePacer::default();
        assert_eq!(p.accept(1_000_000, 30), Some(0));
        assert_eq!(p.accept(1_000_000, 30), None);
        assert_eq!(p.accept(1_001_000, 30), None);
        assert_eq!(p.accept(1_333_334, 30), Some(333_334));
        assert_eq!(p.accept(101_000_000, 30), Some(100_000_000));
    }

    #[test]
    fn pressure_requires_sustained_load_and_warns_once() {
        let mut p = PressureMonitor::default();
        assert!(!p.observe(1, 0.0, 60));
        assert!(!p.observe(1, 0.0, 60));
        assert!(!p.observe(2, 0.0, 60));
        assert!(!p.observe(3, 0.0, 60));
        assert!(p.observe(4, 0.0, 60));
        assert!(!p.observe(5, 100.0, 60));
    }

    #[test]
    fn old_preferences_receive_safe_advanced_defaults() {
        let mut old = serde_json::to_value(Settings::default()).unwrap();
        for field in ["buffer_frames", "low_latency", "write_report"] {
            old.as_object_mut().unwrap().remove(field);
        }
        let restored: Settings = serde_json::from_value(old).unwrap();
        assert_eq!(restored.buffer_frames, 4);
        assert!(restored.low_latency && restored.write_report);
        let invalid = Settings {
            buffer_frames: 255,
            ..restored
        };
        assert!(invalid.validate().is_err());
    }
}
