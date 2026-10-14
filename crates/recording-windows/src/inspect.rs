use crate::encoder::attr_string;
use anyhow::{Context, Result};
use windows::Win32::{
    Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1},
    Media::MediaFoundation::*,
    System::{
        Com::*,
        ProcessStatus::*,
        Threading::*,
        WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    },
};
use windows_capture::monitor::Monitor;

pub(crate) struct MediaRuntime;
impl MediaRuntime {
    pub fn new() -> Result<Self> {
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
            if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                RoUninitialize();
                return Err(error).context(
                    "Media Foundation unavailable. Install Media Feature Pack on Windows N",
                );
            }
        }
        Ok(Self)
    }
}
impl Drop for MediaRuntime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            RoUninitialize();
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Display {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct HardwareInfo {
    pub displays: Vec<Display>,
    pub gpu_adapters: Vec<String>,
    pub cpu: String,
    /// Enumeration is not proof that every resolution/FPS combination works.
    pub h264_hardware_encoders: Vec<String>,
    pub logical_processors: usize,
    pub windows: Vec<recorder_core::WindowSelection>,
    pub audio_devices: crate::audio::AudioDevices,
    pub audio_device_error: Option<String>,
}

pub fn enumerate_windows() -> Result<Vec<recorder_core::WindowSelection>> {
    let mut windows = Vec::new();
    for w in windows_capture::window::Window::enumerate()? {
        let title = w.title().unwrap_or_default();
        let process = w.process_id().unwrap_or(0);
        if !title.trim().is_empty() && process != std::process::id() && process > 0 {
            windows.push(recorder_core::WindowSelection {
                handle: w.as_raw_hwnd() as u64,
                process,
                title,
            });
        }
    }
    windows.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(windows)
}

pub fn inspect() -> Result<HardwareInfo> {
    let _runtime = MediaRuntime::new()?;
    let displays = Monitor::enumerate()?
        .iter()
        .enumerate()
        .map(|(i, m)| {
            Ok(Display {
                index: i + 1,
                name: m
                    .name()
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| format!("Display {}", i + 1)),
                width: m.width()?,
                height: m.height()?,
                refresh_hz: m.refresh_rate().unwrap_or(0),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut names = Vec::new();
    let mut gpu_adapters = Vec::new();
    unsafe {
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            let mut index = 0;
            while let Ok(adapter) = factory.EnumAdapters1(index) {
                if let Ok(desc) = adapter.GetDesc1() {
                    let end = desc
                        .Description
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(desc.Description.len());
                    gpu_adapters.push(format!(
                        "{} (vendor 0x{:04X})",
                        String::from_utf16_lossy(&desc.Description[..end]),
                        desc.VendorId
                    ));
                }
                index += 1;
            }
        }
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let mut array = std::ptr::null_mut();
        let mut count = 0;
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            None,
            Some(&output),
            &mut array,
            &mut count,
        )?;
        if !array.is_null() {
            let entries = std::slice::from_raw_parts_mut(array, count as usize);
            for entry in entries {
                if let Some(activate) = entry.take() {
                    names.push(
                        attr_string(&activate, &MFT_FRIENDLY_NAME_Attribute)
                            .unwrap_or_else(|| "Unnamed hardware H.264 encoder".into()),
                    );
                }
            }
            CoTaskMemFree(Some(array.cast()));
        }
    }
    names.sort();
    names.dedup();
    let (audio_devices, audio_device_error) = match crate::audio::enumerate_audio_devices() {
        Ok(devices) => (devices, None),
        Err(error) => (
            crate::audio::AudioDevices::default(),
            Some(format!("{error:#}")),
        ),
    };
    Ok(HardwareInfo {
        displays,
        gpu_adapters,
        cpu: std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "Unknown CPU".into()),
        windows: enumerate_windows()?,
        audio_devices,
        audio_device_error,
        h264_hardware_encoders: names,
        logical_processors: std::thread::available_parallelism().map_or(1, usize::from),
    })
}

pub(crate) fn process_resources() -> (u64, u64) {
    unsafe {
        let process = GetCurrentProcess();
        let mut mem = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        let _ = GetProcessMemoryInfo(process, &mut mem, std::mem::size_of_val(&mem) as u32);
        let mut creation = Default::default();
        let mut exit = Default::default();
        let mut kernel = Default::default();
        let mut user = Default::default();
        let _ = GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user);
        let ticks = ((u64::from(kernel.dwHighDateTime) << 32) | u64::from(kernel.dwLowDateTime))
            + ((u64::from(user.dwHighDateTime) << 32) | u64::from(user.dwLowDateTime));
        (mem.WorkingSetSize as u64, ticks)
    }
}
