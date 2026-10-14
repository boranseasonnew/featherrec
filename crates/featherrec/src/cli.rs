use anyhow::{Context, Result, bail, ensure};
use recorder_core::{EncoderPolicy, Metrics, Preset, Settings};
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
            "FeatherRec 0.1.0\n\nfeatherrec-cli --diagnose\nfeatherrec-cli --record --seconds 10 [--output DIR] [--monitor 1]\n    [--preset potato|balanced|quality] [--resolution 720|1080]\n    [--fps 30|60] [--bitrate 4000000] [--allow-software] [--no-cursor]\n\nVideo-only H.264/MP4. FPS is a ceiling; static content uses variable frame rate.\nHardware must be verified by default. Ctrl+C stops and finalizes the MP4."
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
    ensure!(
        args.first().is_some_and(|s| s == "--record"),
        "Use --help for commands"
    );
    let mut settings = Settings::default();
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
        let value = args
            .get(i + 1)
            .with_context(|| format!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--seconds" => seconds = Some(value.parse()?),
            "--output" => settings.output_dir = value.into(),
            "--monitor" => settings.monitor = value.parse()?,
            "--fps" => settings.fps = value.parse()?,
            "--bitrate" => settings.bitrate = value.parse()?,
            "--resolution" => match value.as_str() {
                "720" => (settings.width, settings.height) = (1280, 720),
                "1080" => (settings.width, settings.height) = (1920, 1080),
                _ => bail!("Resolution must be 720 or 1080"),
            },
            "--preset" => settings.apply_preset(match value.as_str() {
                "potato" => Preset::Potato,
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
