use anyhow::{Context, Result, ensure};
use std::mem::ManuallyDrop;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{Direct3D11::*, Dxgi::Common::*},
    },
    core::Interface,
};

/// A fixed encoder canvas survives capture-pool resizes. Only one source-sized
/// scratch texture is retained; no CPU readback or encoder reconfiguration occurs.
pub(crate) struct Scaler {
    pub source: (u32, u32),
    device: ID3D11VideoDevice,
    context: ID3D11VideoContext,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    input: ID3D11Texture2D,
    input_view: ID3D11VideoProcessorInputView,
}

fn fit(source: (u32, u32), target: (u32, u32)) -> RECT {
    let (sw, sh) = (u64::from(source.0), u64::from(source.1));
    let (tw, th) = (u64::from(target.0), u64::from(target.1));
    let (w, h) = if sw * th > tw * sh {
        (tw, (sh * tw / sw).max(1))
    } else {
        ((sw * th / sh).max(1), th)
    };
    let x = (tw - w) / 2;
    let y = (th - h) / 2;
    RECT {
        left: x as i32,
        top: y as i32,
        right: (x + w) as i32,
        bottom: (y + h) as i32,
    }
}

impl Scaler {
    pub fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        source: (u32, u32),
        target: (u32, u32),
        fps: u32,
    ) -> Result<Self> {
        ensure!(
            source.0 > 0 && source.1 > 0 && target.0 > 0 && target.1 > 0,
            "Cannot scale an empty capture frame"
        );
        unsafe {
            let video_device: ID3D11VideoDevice = device
                .cast()
                .context("GPU video processing is unavailable for this resolution change")?;
            let video_context: ID3D11VideoContext = context.cast()?;
            let rate = DXGI_RATIONAL {
                Numerator: fps,
                Denominator: 1,
            };
            let enumerator = video_device.CreateVideoProcessorEnumerator(
                &D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                    InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                    InputFrameRate: rate,
                    InputWidth: source.0,
                    InputHeight: source.1,
                    OutputFrameRate: rate,
                    OutputWidth: target.0,
                    OutputHeight: target.1,
                    Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
                },
            )?;
            let support = enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_B8G8R8A8_UNORM)?;
            let required = (D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0
                | D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0) as u32;
            ensure!(
                support & required == required,
                "The GPU cannot resize BGRA capture frames"
            );
            let processor = video_device.CreateVideoProcessor(&enumerator, 0)?;
            video_context.VideoProcessorSetStreamFrameFormat(
                &processor,
                0,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            );
            video_context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
            video_context.VideoProcessorSetStreamDestRect(
                &processor,
                0,
                true,
                Some(&fit(source, target)),
            );
            let rect = RECT {
                left: 0,
                top: 0,
                right: source.0 as i32,
                bottom: source.1 as i32,
            };
            video_context.VideoProcessorSetStreamSourceRect(&processor, 0, true, Some(&rect));
            let target_rect = RECT {
                left: 0,
                top: 0,
                right: target.0 as i32,
                bottom: target.1 as i32,
            };
            video_context.VideoProcessorSetOutputTargetRect(&processor, true, Some(&target_rect));
            let black = D3D11_VIDEO_COLOR {
                Anonymous: D3D11_VIDEO_COLOR_0 {
                    RGBA: D3D11_VIDEO_COLOR_RGBA {
                        R: 0.0,
                        G: 0.0,
                        B: 0.0,
                        A: 1.0,
                    },
                },
            };
            video_context.VideoProcessorSetOutputBackgroundColor(&processor, false, &black);
            video_context.VideoProcessorSetOutputAlphaFillMode(
                &processor,
                D3D11_VIDEO_PROCESSOR_ALPHA_FILL_MODE_OPAQUE,
                0,
            );
            // Explicit RGB/full-range color spaces avoid TV-range conversion.
            let rgb = D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 0 };
            video_context.VideoProcessorSetStreamColorSpace(&processor, 0, &rgb);
            video_context.VideoProcessorSetOutputColorSpace(&processor, &rgb);
            let mut input = None;
            device.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: source.0,
                    Height: source.1,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32
                        | D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut input),
            )?;
            let input = input.context("Resize texture allocation failed")?;
            let mut view = None;
            video_device.CreateVideoProcessorInputView(
                &input,
                &enumerator,
                &D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                    ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                    ..Default::default()
                },
                Some(&mut view),
            )?;
            Ok(Self {
                source,
                device: video_device,
                context: video_context,
                enumerator,
                processor,
                input,
                input_view: view.context("Resize input view unavailable")?,
            })
        }
    }

    pub fn copy(
        &self,
        context: &ID3D11DeviceContext,
        source: &ID3D11Texture2D,
        target: &ID3D11Texture2D,
    ) -> Result<()> {
        unsafe {
            context.CopyResource(&self.input, source);
            let mut output = None;
            self.device.CreateVideoProcessorOutputView(
                target,
                &self.enumerator,
                &D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                    ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                    ..Default::default()
                },
                Some(&mut output),
            )?;
            let output = output.context("Resize output view unavailable")?;
            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: ManuallyDrop::new(Some(self.input_view.clone())),
                ..Default::default()
            };
            let result = self.context.VideoProcessorBlt(
                &self.processor,
                &output,
                0,
                std::slice::from_ref(&stream),
            );
            // Generated COM unions contain ManuallyDrop fields; balance the
            // input-view clone on both success and failure.
            ManuallyDrop::drop(&mut stream.pInputSurface);
            result?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fits_changed_resolutions_without_cropping() {
        let rect = |s, t| {
            let r = fit(s, t);
            (r.left, r.top, r.right, r.bottom)
        };
        assert_eq!(rect((1280, 720), (1920, 1080)), (0, 0, 1920, 1080));
        assert_eq!(rect((800, 600), (1920, 1080)), (240, 0, 1680, 1080));
        assert_eq!(rect((1920, 1080), (800, 600)), (0, 75, 800, 525));
        assert_eq!(rect((1, 16384), (1920, 1080)), (959, 0, 960, 1080));
    }
}
