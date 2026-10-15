use anyhow::{Context, Result, bail, ensure};
use recorder_core::{
    AudioTrack, EncoderPolicy, Metrics, Preset, RateControl, Settings, SystemAudio,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};
use windows::core::BOOL;

static STOP: AtomicBool = AtomicBool::new(false);
unsafe extern "system" fn stop_handler(event: u32) -> BOOL {
    if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        STOP.store(true, Ordering::Relaxed);
        return true.into();
    }
    false.into()
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|v| v == "--help" || v == "-h") {
        println!(
            "FeatherRec 0.1.3\n\nfeatherrec-cli --diagnose\nfeatherrec-cli --audio-devices\nfeatherrec-cli --check-audio --seconds 5 [--audio system|application] [--microphone]\nfeatherrec-cli --record --seconds 10 [--output DIR] [--monitor 1]\n    [--window-title UNIQUE_SUBSTRING]\n    [--preset compact|potato|smooth720|balanced30|balanced|quality]\n    [--resolution 720|1080] [--fps 30|60] [--bitrate 4000000] [--cqp 16..51]\n    [--allow-software] [--no-cursor] [--show-border]\n    [--buffers 3..8] [--no-low-latency] [--no-report]\n    [--audio none|system|application] [--output-device ID]\n    [--microphone] [--microphone-device ID] [--mute-microphone]\n    [--audio-tracks mix,system,mic,silent (1..5)]\n\nH.264/MP4 with optional 1..5 independently routed stereo AAC tracks. Microphone defaults off.\nApplication audio requires a selected window; no system-audio fallback.\nFPS is a ceiling; static content uses variable frame rate.\nHardware must be verified by default. Ctrl+C stops and finalizes the MP4."
        );
        return Ok(());
    }
    if args == ["--diagnose"] {
        println!(
            "{}",
            serde_json::to_string_pretty(&recording_windows::inspect()?)?
        );
        return Ok(());
    }
    if args == ["--audio-devices"] {
        println!(
            "{}",
            serde_json::to_string_pretty(&recording_windows::enumerate_audio_devices()?)?
        );
        return Ok(());
    }
    let audio_check = args.first().is_some_and(|s| s == "--check-audio");
    ensure!(
        audio_check || args.first().is_some_and(|s| s == "--record"),
        "Use --help for commands"
    );
    let mut settings = Settings::default();
    if audio_check {
        settings.audio.source = SystemAudio::System;
    }
    let mut seconds: Option<u64> = None;
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--allow-software" {
            settings.encoder = EncoderPolicy::AllowSoftware;
            i += 1;
            continue;
        }
        if arg == "--no-cursor" {
            settings.cursor = false;
            i += 1;
            continue;
        }
        if arg == "--no-low-latency" {
            settings.low_latency = false;
            i += 1;
            continue;
        }
        if arg == "--no-report" {
            settings.write_report = false;
            i += 1;
            continue;
        }
        if arg == "--microphone" {
            settings.audio.microphone = true;
            i += 1;
            continue;
        }
        if arg == "--mute-microphone" {
            settings.audio.microphone_muted = true;
            i += 1;
            continue;
        }
        if arg == "--show-border" {
            settings.show_capture_border = true;
            i += 1;
            continue;
        }
        let value = args
            .get(i + 1)
            .with_context(|| format!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--seconds" => seconds = Some(value.parse()?),
            "--output" => settings.output_dir = value.into(),
            "--monitor" => settings.monitor = value.parse()?,
            "--fps" => settings.fps = value.parse()?,
            "--buffers" => settings.buffer_frames = value.parse()?,
            "--audio" => {
                settings.audio.source = match value.as_str() {
                    "none" => SystemAudio::Off,
                    "system" => SystemAudio::System,
                    "application" => SystemAudio::Application,
                    _ => bail!("Audio must be none, system, or application"),
                }
            }
            "--microphone-device" => settings.audio.microphone_device = value.clone(),
            "--output-device" => settings.audio.output_device = value.clone(),
            "--window-title" => {
                let matches: Vec<_> = recording_windows::enumerate_windows()?
                    .into_iter()
                    .filter(|w| w.title.contains(value))
                    .collect();
                ensure!(
                    matches.len() == 1,
                    "Window title must identify exactly one window (found {})",
                    matches.len()
                );
                settings.window = matches.into_iter().next();
            }
            "--bitrate" => settings.bitrate = value.parse()?,
            "--cqp" => {
                settings.rate_control = RateControl::Cqp;
                settings.qp = value.parse()?;
            }
            "--audio-tracks" => {
                settings.audio.tracks = value
                    .split(',')
                    .map(|route| match route {
                        "mix" => Ok(AudioTrack::default()),
                        "system" => Ok(AudioTrack {
                            system: true,
                            microphone: false,
                        }),
                        "mic" => Ok(AudioTrack {
                            system: false,
                            microphone: true,
                        }),
                        "silent" => Ok(AudioTrack {
                            system: false,
                            microphone: false,
                        }),
                        _ => bail!("Audio track must be mix, system, mic, or silent"),
                    })
                    .collect::<Result<_>>()?;
            }
            "--resolution" => match value.as_str() {
                "720" => (settings.width, settings.height) = (1280, 720),
                "1080" => (settings.width, settings.height) = (1920, 1080),
                _ => bail!("Resolution must be 720 or 1080"),
            },
            "--preset" => settings.apply_preset(match value.as_str() {
                "potato" => Preset::Potato,
                "compact" => Preset::Compact,
                "smooth720" => Preset::Smooth720,
                "balanced30" => Preset::Balanced30,
                "balanced" => Preset::Balanced,
                "quality" => Preset::Quality,
                _ => bail!("Unknown preset: {value}"),
            }),
            _ => bail!("Unknown option: {arg}"),
        }
        i += 2;
    }
    let seconds = seconds.context("Provide --seconds (1 to 86400)")?;
    ensure!(
        (1..=86_400).contains(&seconds),
        "Duration must be 1 to 86400 seconds"
    );
    if audio_check {
        ensure!(seconds <= 60, "Audio checks must be 1 to 60 seconds");
        settings.validate()?;
        let preview = recording_windows::AudioPreview::start(&settings)?;
        let started = Instant::now();
        let mut peak = [0.0f32; 2];
        let mut final_state;
        loop {
            final_state = preview.snapshot();
            if let Some(error) = &final_state.error {
                bail!("{error}");
            }
            peak[0] = peak[0].max(final_state.system_peak);
            peak[1] = peak[1].max(final_state.microphone_peak);
            if started.elapsed() >= Duration::from_secs(seconds) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        ensure!(
            final_state.running,
            "Audio source did not initialize during the check"
        );
        final_state.system_peak = peak[0];
        final_state.microphone_peak = peak[1];
        println!("{}", serde_json::to_string_pretty(&final_state)?);
        return Ok(());
    }
    unsafe {
        SetConsoleCtrlHandler(Some(stop_handler), true)?;
    }
    let metrics = Arc::new(Mutex::new(Metrics::default()));
    let mut recording = recording_windows::start(settings, metrics.clone())?;
    eprintln!("Recording. Press Ctrl+C to stop safely.");
    let started = Instant::now();
    while recording.elapsed() < Duration::from_secs(seconds)
        && started.elapsed() < Duration::from_secs(seconds + 10)
        && !STOP.load(Ordering::Relaxed)
        && !recording.is_finished()
    {
        std::thread::sleep(Duration::from_millis(100));
        recording.poll();
    }
    let report = recording.finish()?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    ensure!(
        report.finalized,
        "Recording did not pass finalization/validation. Partial file retained at {}",
        report.file.display()
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("FeatherRec: {error:#}");
        std::process::exit(1);
    }
}
