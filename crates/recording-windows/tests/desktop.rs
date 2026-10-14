use recorder_core::{Metrics, Settings, TEXTURE_LIMIT};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// An opt-in integration test: CI must not imply it exercised a physical desktop.
#[test]
#[ignore = "Requires an unlocked interactive Windows desktop and a hardware H.264 MFT"]
fn repeated_recordings_finalize_in_one_process() -> anyhow::Result<()> {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../validation/repeated");
    for iteration in 1..=2 {
        let settings = Settings {
            output_dir: folder.clone(),
            ..Settings::default()
        };
        let metrics = Arc::new(Mutex::new(Metrics::default()));
        let mut recording = recording_windows::start(settings, metrics)?;
        std::thread::sleep(Duration::from_secs(2));
        recording.poll();
        let report = recording.finish()?;
        assert!(report.finalized, "{}", report.validation);
        assert!(report.metrics.submitted > 0);
        assert!(report.metrics.max_in_flight <= TEXTURE_LIMIT);
        assert!(report.metrics.hardware_verified);
        println!(
            "Session {iteration}: MP4 reopened and first frame decoded; {} submitted samples, {} drops",
            report.metrics.submitted, report.metrics.dropped
        );
    }
    Ok(())
}
