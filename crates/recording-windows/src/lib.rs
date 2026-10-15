mod audio;
mod capture;
mod encoder;
mod inspect;
mod mux;
mod output;
mod scaler;

pub use audio::{
    AudioControls, AudioDevice, AudioDevices, AudioPreview, AudioPreviewState,
    enumerate_audio_devices,
};
pub use capture::{Recording, SharedMetrics, start, start_with_audio_controls};
pub use inspect::{Display, HardwareInfo, enumerate_windows, inspect};
