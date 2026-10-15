use crate::encoder::media_path;
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};
use windows::{Win32::Media::MediaFoundation::*, core::PCWSTR};

/// Copy H.264 packets without decoding/re-encoding video; encode the local mixed
/// PCM tracks as AAC. This keeps audio I/O off the GPU capture callback and bounds
/// memory, at the cost of temporary PCM storage and a final mux pass.
pub(crate) fn mux_audio(
    video: &Path,
    waves: &[PathBuf],
    output: &Path,
    bitrate: u32,
) -> Result<()> {
    ensure!(
        (1..=5).contains(&waves.len()),
        "Audio track count must be between one and five"
    );
    unsafe {
        let video_path = media_path(video);
        let output_path = media_path(output);
        let video_reader = MFCreateSourceReaderFromURL(PCWSTR(video_path.as_ptr()), None)?;
        let vi = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let ai = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        let video_type = video_reader.GetNativeMediaType(vi, 0)?;
        video_reader.SetCurrentMediaType(vi, None, &video_type)?;
        let mut audio_readers = Vec::with_capacity(waves.len());
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
        let mut audio_streams = Vec::with_capacity(waves.len());
        for wave in waves {
            let audio_path = media_path(wave);
            let audio_reader = MFCreateSourceReaderFromURL(PCWSTR(audio_path.as_ptr()), None)?;
            let audio_type = audio_reader.GetNativeMediaType(ai, 0)?;
            audio_reader.SetCurrentMediaType(ai, None, &audio_type)?;
            let audio_stream = writer.AddStream(&aac)?;
            writer.SetInputMediaType(audio_stream, &audio_type, None)?;
            audio_streams.push(audio_stream);
            audio_readers.push(audio_reader);
        }
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
        let mut samples: Vec<_> = audio_readers
            .iter()
            .map(|r| next(r, ai))
            .collect::<Result<_>>()?;
        while v.is_some() || samples.iter().any(Option::is_some) {
            let mut earliest = v
                .as_ref()
                .map(|s| s.GetSampleTime())
                .transpose()?
                .map(|t| (t, None));
            for (index, sample) in samples.iter().enumerate() {
                if let Some(sample) = sample {
                    let time = sample.GetSampleTime()?;
                    if earliest.is_none_or(|(previous, _)| time < previous) {
                        earliest = Some((time, Some(index)));
                    }
                }
            }
            if let Some((_, Some(index))) = earliest {
                writer.WriteSample(audio_streams[index], samples[index].as_ref().unwrap())?;
                samples[index] = next(&audio_readers[index], ai)?;
            } else {
                writer.WriteSample(video_stream, v.as_ref().unwrap())?;
                v = next(&video_reader, vi)?;
            }
        }
        writer
            .Finalize()
            .context("Audio/video MP4 finalization failed; raw video and PCM are retained")?;
        drop(writer);
        // Confirm that the muxed audio can actually be decoded before promoting it.
        let output_reader = MFCreateSourceReaderFromURL(PCWSTR(output_path.as_ptr()), None)?;
        let mut count = 0;
        for index in 0..32 {
            let Ok(media_type) = output_reader.GetNativeMediaType(index, 0) else {
                break;
            };
            if media_type.GetGUID(&MF_MT_MAJOR_TYPE)? != MFMediaType_Audio {
                continue;
            }
            // Give every track its own demux position, including short recordings.
            let decoded_reader = MFCreateSourceReaderFromURL(PCWSTR(output_path.as_ptr()), None)?;
            let pcm = MFCreateMediaType()?;
            pcm.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            pcm.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            decoded_reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            decoded_reader.SetStreamSelection(index, true)?;
            decoded_reader.SetCurrentMediaType(index, None, &pcm)?;
            ensure!(
                next(&decoded_reader, index)?.is_some(),
                "Muxed MP4 audio stream {index} has no decodable sample"
            );
            count += 1;
        }
        ensure!(
            count == waves.len(),
            "Final MP4 audio track count differs from the selected count"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::MediaRuntime;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn muxes_one_and_five_independently_decodable_audio_tracks() -> Result<()> {
        let _runtime = MediaRuntime::new()?;
        let folder = Fixture(std::env::temp_dir().join(format!(
            "featherrec-multitrack-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        )));
        fs::create_dir_all(&folder.0)?;
        let video = folder.0.join("video.mp4");
        fs::write(&video, include_bytes!("../tests/fixtures/video.mp4"))?;
        let mut waves = Vec::new();
        for track in 0..5 {
            let path = folder.0.join(format!("audio-{track}.wav"));
            let length = 48_000u32 * 2 * 4;
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&(length + 36).to_le_bytes());
            bytes.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x02\0");
            bytes.extend_from_slice(&48_000u32.to_le_bytes());
            bytes.extend_from_slice(&192_000u32.to_le_bytes());
            bytes.extend_from_slice(b"\x04\0\x10\0data");
            bytes.extend_from_slice(&length.to_le_bytes());
            for frame in 0..96_000 {
                let sample = ((frame as f32 * std::f32::consts::TAU / 256.0).sin()
                    * (track + 1) as f32
                    * 3_000.0) as i16;
                bytes.extend_from_slice(&sample.to_le_bytes());
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            fs::write(&path, bytes)?;
            waves.push(path);
        }
        // Pure file processing: never opens an audio render device or plays a tone.
        mux_audio(&video, &waves[..1], &folder.0.join("one.mp4"), 128_000)
            .context("One-track mux test")?;
        mux_audio(&video, &waves, &folder.0.join("five.mp4"), 128_000)
            .context("Five-track mux test")?;
        Ok(())
    }
}
