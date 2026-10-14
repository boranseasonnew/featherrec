use crate::encoder::media_path;
use anyhow::{Context, Result, ensure};
use std::path::Path;
use windows::{Win32::Media::MediaFoundation::*, core::PCWSTR};

/// Copy H.264 packets without decoding/re-encoding video; encode the local mixed
/// PCM track as AAC. This keeps audio I/O off the GPU capture callback and bounds
/// memory, at the cost of temporary PCM storage and a final mux pass.
pub(crate) fn mux_audio(video: &Path, wave: &Path, output: &Path, bitrate: u32) -> Result<()> {
    unsafe {
        let video_path = media_path(video);
        let audio_path = media_path(wave);
        let output_path = media_path(output);
        let video_reader = MFCreateSourceReaderFromURL(PCWSTR(video_path.as_ptr()), None)?;
        let audio_reader = MFCreateSourceReaderFromURL(PCWSTR(audio_path.as_ptr()), None)?;
        let vi = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let ai = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        let video_type = video_reader.GetNativeMediaType(vi, 0)?;
        video_reader.SetCurrentMediaType(vi, None, &video_type)?;
        let audio_type = audio_reader.GetNativeMediaType(ai, 0)?;
        audio_reader.SetCurrentMediaType(ai, None, &audio_type)?;
        let writer = MFCreateSinkWriterFromURL(PCWSTR(output_path.as_ptr()), None, None)?;
        let video_stream = writer.AddStream(&video_type)?;
        writer.SetInputMediaType(video_stream, &video_type, None)?;
        let aac = MFCreateMediaType()?;
        aac.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        aac.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
        aac.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
        aac.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)?;
        aac.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        aac.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, bitrate / 8)?;
        aac.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?;
        aac.SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, 0x29)?;
        let audio_stream = writer.AddStream(&aac)?;
        writer.SetInputMediaType(audio_stream, &audio_type, None)?;
        writer.BeginWriting()?;
        let next = |reader: &IMFSourceReader, stream| -> Result<Option<IMFSample>> {
            for _ in 0..32 {
                let mut sample = None;
                let mut flags = 0;
                reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))?;
                ensure!(
                    flags & MF_SOURCE_READERF_ERROR.0 as u32 == 0,
                    "Source reader failed while muxing audio"
                );
                if sample.is_some() || flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    return Ok(sample);
                }
            }
            anyhow::bail!("Source reader produced too many empty samples while muxing audio")
        };
        let mut v = next(&video_reader, vi)?;
        let mut a = next(&audio_reader, ai)?;
        while v.is_some() || a.is_some() {
            let take_video = match (&v, &a) {
                (Some(v), Some(a)) => v.GetSampleTime()? <= a.GetSampleTime()?,
                (Some(_), None) => true,
                _ => false,
            };
            if take_video {
                writer.WriteSample(video_stream, v.as_ref().unwrap())?;
                v = next(&video_reader, vi)?;
            } else {
                writer.WriteSample(audio_stream, a.as_ref().unwrap())?;
                a = next(&audio_reader, ai)?;
            }
        }
        writer
            .Finalize()
            .context("Audio/video MP4 finalization failed; raw video and PCM are retained")?;
        drop(writer);
        // Confirm that the muxed audio can actually be decoded before promoting it.
        let output_reader = MFCreateSourceReaderFromURL(PCWSTR(output_path.as_ptr()), None)?;
        let pcm = MFCreateMediaType()?;
        pcm.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        pcm.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
        output_reader.SetCurrentMediaType(ai, None, &pcm)?;
        ensure!(
            next(&output_reader, ai)?.is_some(),
            "Muxed MP4 has no decodable audio sample"
        );
        Ok(())
    }
}
