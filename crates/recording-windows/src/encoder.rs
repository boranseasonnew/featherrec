use anyhow::{Context, Result, bail, ensure};
use recorder_core::{EncoderPolicy, Settings};
use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::SyncSender,
    },
};
use windows::{
    Win32::{
        Foundation::E_NOTIMPL,
        Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D},
        Media::MediaFoundation::*,
    },
    core::{GUID, Interface, PCWSTR, implement},
};

/// D3D11 resources are free-threaded. The shared immediate context is protected by
/// ID3D11Multithread before any resource crosses threads. No CPU maps are used.
#[derive(Clone)]
pub(crate) struct Texture(pub ID3D11Texture2D);
unsafe impl Send for Texture {}
unsafe impl Sync for Texture {}

pub(crate) struct Device(pub ID3D11Device);
unsafe impl Send for Device {}

impl Device {
    pub fn get(&self) -> &ID3D11Device {
        &self.0
    }
}

/// Return a texture only after Media Foundation releases its sample. Returning
/// immediately after WriteSample would race the asynchronous encoder.
#[implement(IMFAsyncCallback)]
struct Recycle {
    texture: Texture,
    available: SyncSender<Texture>,
    in_flight: Arc<AtomicUsize>,
}

impl IMFAsyncCallback_Impl for Recycle_Impl {
    fn GetParameters(&self, _: *mut u32, _: *mut u32) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Invoke(&self, _: windows::core::Ref<IMFAsyncResult>) -> windows::core::Result<()> {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
        let _ = self.available.try_send(self.texture.clone());
        Ok(())
    }
}

pub(crate) fn attr_string(attrs: &IMFAttributes, key: &GUID) -> Option<String> {
    unsafe {
        let len = attrs.GetStringLength(key).ok()?;
        let mut text = vec![0u16; len as usize + 1];
        attrs.GetString(key, &mut text, None).ok()?;
        Some(String::from_utf16_lossy(&text[..len as usize]))
    }
}

fn dimensions(w: u32, h: u32) -> u64 {
    (u64::from(w) << 32) | u64::from(h)
}

pub(crate) fn media_path(path: &Path) -> Vec<u16> {
    // SourceReader interprets Rust's canonical \\?\ prefix as a network URL.
    // Preserve Unicode while converting extended drive/UNC paths to Win32 paths.
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let drive_prefix: Vec<u16> = "\\\\?\\".encode_utf16().collect();
    let unc_prefix: Vec<u16> = "\\\\?\\UNC\\".encode_utf16().collect();
    let mut normal = if wide.starts_with(&unc_prefix) {
        let mut value = vec![92, 92];
        value.extend_from_slice(&wide[8..]);
        value
    } else if wide.starts_with(&drive_prefix) {
        wide[4..].to_vec()
    } else {
        wide
    };
    normal.push(0);
    normal
}

pub(crate) struct Encoder {
    writer: IMFSinkWriter,
    stream: u32,
    _manager: IMFDXGIDeviceManager,
    pub name: String,
    pub hardware: bool,
}

impl Encoder {
    pub fn new(
        device: &ID3D11Device,
        settings: &Settings,
        source: (u32, u32),
        path: &Path,
    ) -> Result<Self> {
        // SAFETY: COM and MF are initialized by this thread's MediaRuntime.
        unsafe {
            let mut token = 0;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
            let manager = manager.context("Windows did not return a DXGI device manager")?;
            manager.ResetDevice(device, token)?;
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 4)?;
            let attrs = attrs.unwrap();
            attrs.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &manager)?;
            attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            attrs.SetUINT32(&MF_LOW_LATENCY, u32::from(settings.low_latency))?;
            // Keep SinkWriter throttling enabled. Only the encoding worker may wait.
            let wide = media_path(path);
            let writer = MFCreateSinkWriterFromURL(PCWSTR(wide.as_ptr()), None, &attrs)
                .context("Cannot create MP4 output; check folder permissions and storage space")?;
            let output = MFCreateMediaType()?;
            output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, settings.bitrate)?;
            output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            output.SetUINT64(
                &MF_MT_FRAME_SIZE,
                dimensions(settings.width, settings.height),
            )?;
            output.SetUINT64(&MF_MT_FRAME_RATE, dimensions(settings.fps, 1))?;
            output.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, dimensions(1, 1))?;
            let stream = writer.AddStream(&output)?;
            let input = MFCreateMediaType()?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_ARGB32)?;
            input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            input.SetUINT64(&MF_MT_FRAME_SIZE, dimensions(source.0, source.1))?;
            input.SetUINT64(&MF_MT_FRAME_RATE, dimensions(settings.fps, 1))?;
            input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, dimensions(1, 1))?;
            writer.SetInputMediaType(stream, &input, None)
                .context("H.264 initialization failed for these dimensions/FPS. Try Potato mode or update the GPU driver")?;
            let ex: IMFSinkWriterEx = writer.cast()?;
            let mut name = String::from("Media Foundation H.264 (unidentified)");
            let mut hardware = false;
            for index in 0..16 {
                let mut category = GUID::zeroed();
                let mut transform = None;
                if ex
                    .GetTransformForStream(stream, index, Some(&mut category), &mut transform)
                    .is_err()
                {
                    break;
                }
                let Some(attrs) = transform.and_then(|t| t.GetAttributes().ok()) else {
                    continue;
                };
                if category == MFT_CATEGORY_VIDEO_ENCODER {
                    name = attr_string(&attrs, &MFT_FRIENDLY_NAME_Attribute).unwrap_or(name);
                    hardware = attr_string(&attrs, &MFT_ENUM_HARDWARE_URL_Attribute)
                        .is_some_and(|url| !url.is_empty());
                    if hardware && name.contains("unidentified") {
                        name =
                            "Hardware H.264 MFT (verified hardware URL; name unavailable)".into();
                    }
                } else {
                    // The video processor otherwise repeats static frames up to
                    // the nominal FPS, wasting encoding work and bitrate.
                    attrs.SetUINT32(&MF_XVP_DISABLE_FRC, 1)?;
                }
            }
            if settings.encoder == EncoderPolicy::Hardware && !hardware {
                bail!(
                    "Windows did not identify the selected H.264 encoder as hardware ({name}). Choose 'Allow software' explicitly to continue; CPU usage may increase."
                );
            }
            writer.BeginWriting()?;
            Ok(Self {
                writer,
                stream,
                _manager: manager,
                name,
                hardware,
            })
        }
    }

    pub fn write(
        &self,
        texture: Texture,
        pts: i64,
        duration: i64,
        available: &SyncSender<Texture>,
        in_flight: &Arc<AtomicUsize>,
    ) -> Result<()> {
        unsafe {
            let tracked = MFCreateTrackedSample()?;
            let sample: IMFSample = tracked.cast()?;
            let buffer: IMFMediaBuffer =
                MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &texture.0, 0, false)?;
            let dxgi: IMF2DBuffer = buffer.cast()?;
            buffer.SetCurrentLength(dxgi.GetContiguousLength()?)?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(pts)?;
            sample.SetSampleDuration(duration.max(1))?;
            let callback: IMFAsyncCallback = Recycle {
                texture,
                available: available.clone(),
                in_flight: in_flight.clone(),
            }
            .into();
            tracked.SetAllocator(&callback, None)?;
            self.writer
                .WriteSample(self.stream, &sample)
                .context("Encoding or disk write failed; check free storage and the GPU driver")?;
            Ok(())
        }
    }

    pub fn finish(self) -> Result<()> {
        unsafe {
            self.writer
                .Finalize()
                .context("MP4 finalization failed. The .partial.mp4 file is retained for recovery")
        }
    }
}

/// Bounded check that Windows can demux and decode the first frame. This does
/// not replace full-file external validation in the release test procedure.
pub(crate) fn validate_video(path: &Path, width: u32, height: u32) -> Result<()> {
    unsafe {
        let wide = media_path(path);
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.unwrap();
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), &attrs)?;
        let video = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(video, true)?;
        let native = reader.GetNativeMediaType(video, 0)?;
        ensure!(
            native.GetUINT64(&MF_MT_FRAME_SIZE)? == dimensions(width, height),
            "Finalized MP4 has incorrect dimensions"
        );
        let decoded = MFCreateMediaType()?;
        decoded.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        decoded.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(video, None, &decoded)?;
        for _ in 0..16 {
            let mut sample = None;
            let mut flags = 0;
            reader.ReadSample(video, 0, None, Some(&mut flags), None, Some(&mut sample))?;
            if let Some(sample) = sample {
                ensure!(sample.GetTotalLength()? > 0, "Decoded frame was empty");
                return Ok(());
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
        }
        bail!("The finalized MP4 did not contain a decodable video frame")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_media_foundation_paths_without_losing_unicode() {
        let normal = |path: &str| {
            let wide = media_path(Path::new(path));
            String::from_utf16(&wide[..wide.len() - 1]).unwrap()
        };
        assert_eq!(
            normal(r"\\?\C:\Kayıtlar\çekim.mp4"),
            r"C:\Kayıtlar\çekim.mp4"
        );
        assert_eq!(
            normal(r"\\?\UNC\server\share\clip.mp4"),
            r"\\server\share\clip.mp4"
        );
        assert_eq!(normal(r"C:\clip.mp4"), r"C:\clip.mp4");
    }
}
