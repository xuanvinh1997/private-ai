//! Audio in, 16 kHz mono float PCM out.
//!
//! `transcribe.cpp` v1 accepts exactly one input shape -- mono `f32` at 16 kHz -- and deliberately
//! links no resampler, so this module is not a convenience: without it there is no way to hand the
//! library a file a user actually owns. Decoding is Symphonia and resampling is rubato, both pure
//! Rust, so a document project reads an `.m4a` on a machine with no `ffmpeg` installed.

use std::path::Path;

use rubato::{FftFixedIn, Resampler};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::AsrError;

/// The only sample rate the recognizer accepts.
pub const SAMPLE_RATE: u32 = 16_000;

/// Longest single recording accepted. The transcript of an hour of speech is a large document
/// already, and the decoded PCM alone is ~230 MB held in memory; past this the honest answer is to
/// ask for the file split rather than to swap the machine to death.
pub const MAX_DURATION_MS: u64 = 60 * 60 * 1_000;

/// Container extensions worth handing to Symphonia. Deliberately a list rather than "try everything":
/// a document folder is full of files that are not audio, and probing each one costs a file open.
/// Every entry here is a container Symphonia is compiled to read in this build; `.opus`, `.aiff`
/// and `.caf` are absent because it cannot, and claiming them would only move the failure later.
const AUDIO: &[&str] = &[
    "wav", "wave", "mp3", "m4a", "m4b", "mp4", "aac", "flac", "ogg", "oga", "mka", "webm",
];

/// Whether this path is one the recognizer will try to read.
pub fn is_audio(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    AUDIO.contains(&extension.as_str())
}

/// Decoded audio, ready to feed to a session.
#[derive(Clone, Debug)]
pub struct Clip {
    /// Mono `f32` in [-1, 1] at [`SAMPLE_RATE`].
    pub samples: Vec<f32>,
    /// Original sample rate, kept because it is the useful half of a "this file is unusual" report.
    pub source_rate: u32,
    pub channels: u16,
}

impl Clip {
    pub fn duration_ms(&self) -> u64 {
        self.samples.len() as u64 * 1_000 / SAMPLE_RATE as u64
    }
}

/// Decode any supported container into one mono 16 kHz buffer. Blocking, CPU-bound work: callers
/// must put it on a blocking thread rather than a Tokio worker.
pub fn decode_file(path: &Path) -> Result<Clip, AsrError> {
    let shown = path.display().to_string();
    let file = std::fs::File::open(path)
        .map_err(|error| AsrError::Decode(format!("không mở được `{shown}`: {error}")))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            // Gapless playback trims encoder padding, which otherwise shows up as a phantom
            // silence the recognizer spends time on.
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|error| AsrError::Decode(format!("`{shown}` không phải tệp âm thanh đọc được: {error}")))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| AsrError::Decode(format!("`{shown}` không có luồng âm thanh nào")))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| AsrError::Decode(format!("không giải mã được `{shown}`: {error}")))?;

    let mut mono: Vec<f32> = Vec::new();
    let mut source_rate = 0_u32;
    let mut channels = 0_u16;
    let mut buffer: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // Symphonia signals a clean end of stream as an EOF io error; anything else is real.
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err(AsrError::Decode(format!(
                    "`{shown}` đổi định dạng giữa chừng; tách tệp rồi thử lại"
                )));
            }
            Err(error) => {
                return Err(AsrError::Decode(format!("lỗi đọc `{shown}`: {error}")));
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged packet in the middle of a long recording loses a moment, not the file.
            Err(SymphoniaError::DecodeError(error)) => {
                tracing::debug!(%error, path = %shown, "bỏ qua gói âm thanh hỏng");
                continue;
            }
            Err(error) => return Err(AsrError::Decode(format!("lỗi giải mã `{shown}`: {error}"))),
        };

        let spec = *decoded.spec();
        source_rate = spec.rate;
        channels = spec.channels.count() as u16;
        let interleaved = buffer.get_or_insert_with(|| {
            SampleBuffer::<f32>::new(decoded.capacity() as u64, spec)
        });
        interleaved.copy_interleaved_ref(decoded);
        downmix(interleaved.samples(), channels, &mut mono);

        // Check the budget against the source rate: the whole point is to stop before the
        // allocation, not to discover afterwards that it was too big.
        if source_rate > 0
            && mono.len() as u64 * 1_000 / source_rate as u64 > MAX_DURATION_MS
        {
            return Err(AsrError::Decode(format!(
                "`{shown}` dài quá {} phút; tách nhỏ rồi nạp lại",
                MAX_DURATION_MS / 60_000
            )));
        }
    }

    if mono.is_empty() || source_rate == 0 {
        return Err(AsrError::Decode(format!("`{shown}` không có mẫu âm thanh nào")));
    }

    let samples = resample(mono, source_rate)?;
    Ok(Clip {
        samples,
        source_rate,
        channels,
    })
}

/// Average the channels of an interleaved frame into one, appending to `out`.
/// Averaging rather than taking the left channel: a recording with one speaker on each channel
/// loses half its speech to the simpler rule.
fn downmix(interleaved: &[f32], channels: u16, out: &mut Vec<f32>) {
    let channels = channels.max(1) as usize;
    if channels == 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    out.reserve(interleaved.len() / channels);
    for frame in interleaved.chunks_exact(channels) {
        out.push(frame.iter().sum::<f32>() / channels as f32);
    }
}

/// Resample one mono buffer to [`SAMPLE_RATE`]. A file already at 16 kHz is passed through
/// untouched -- running it through the filter would only cost time and a little quality.
fn resample(mono: Vec<f32>, from: u32) -> Result<Vec<f32>, AsrError> {
    if from == SAMPLE_RATE {
        return Ok(mono);
    }
    // One second of input per FFT pass: large enough that the fixed cost per call disappears,
    // small enough that a long recording does not build one enormous intermediate buffer.
    let chunk = from as usize;
    let mut resampler = FftFixedIn::<f32>::new(from as usize, SAMPLE_RATE as usize, chunk, 2, 1)
        .map_err(|error| AsrError::Decode(format!("không lấy mẫu lại được từ {from} Hz: {error}")))?;

    let mut out: Vec<f32> = Vec::with_capacity(mono.len() * SAMPLE_RATE as usize / from as usize + chunk);
    let mut cursor = 0;
    while cursor + chunk <= mono.len() {
        let block = [&mono[cursor..cursor + chunk]];
        let processed = resampler
            .process(&block, None)
            .map_err(|error| AsrError::Decode(format!("lấy mẫu lại thất bại: {error}")))?;
        out.extend_from_slice(&processed[0]);
        cursor += chunk;
    }
    if cursor < mono.len() {
        // The tail is shorter than one pass; `process_partial` zero-pads it rather than dropping it.
        let block = [&mono[cursor..]];
        let processed = resampler
            .process_partial(Some(&block), None)
            .map_err(|error| AsrError::Decode(format!("lấy mẫu lại phần cuối thất bại: {error}")))?;
        out.extend_from_slice(&processed[0]);
    }
    Ok(out)
}

/// Resampling a microphone as it arrives: same filter, but fed whatever the device hands over and
/// asked for whatever is ready. Capture devices deliver 44.1 or 48 kHz in blocks of their own
/// choosing, and none of those numbers is 16 kHz.
pub struct LiveResampler {
    resampler: Option<FftFixedIn<f32>>,
    channels: u16,
    chunk: usize,
    pending: Vec<f32>,
}

impl LiveResampler {
    pub fn new(from: u32, channels: u16) -> Result<Self, AsrError> {
        if from == 0 {
            return Err(AsrError::Unavailable(
                "thiết bị thu không báo tần số lấy mẫu".into(),
            ));
        }
        // 100 ms per pass: the recognizer wants to be fed at roughly this rate anyway, so the
        // resampler's block size stops being a second source of latency.
        let chunk = (from as usize / 10).max(160);
        let resampler = if from == SAMPLE_RATE {
            None
        } else {
            Some(
                FftFixedIn::<f32>::new(from as usize, SAMPLE_RATE as usize, chunk, 2, 1).map_err(
                    |error| {
                        AsrError::Unavailable(format!(
                            "không lấy mẫu lại được micro từ {from} Hz: {error}"
                        ))
                    },
                )?,
            )
        };
        Ok(Self {
            resampler,
            channels: channels.max(1),
            chunk,
            pending: Vec::new(),
        })
    }

    /// Push one interleaved device buffer; returns every 16 kHz sample that became available.
    pub fn push(&mut self, interleaved: &[f32]) -> Result<Vec<f32>, AsrError> {
        downmix(interleaved, self.channels, &mut self.pending);
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(std::mem::take(&mut self.pending));
        };
        let mut out = Vec::new();
        let mut cursor = 0;
        while cursor + self.chunk <= self.pending.len() {
            let block = [&self.pending[cursor..cursor + self.chunk]];
            let processed = resampler
                .process(&block, None)
                .map_err(|error| AsrError::Engine(format!("lấy mẫu lại micro thất bại: {error}")))?;
            out.extend_from_slice(&processed[0]);
            cursor += self.chunk;
        }
        self.pending.drain(..cursor);
        Ok(out)
    }

    /// Flush the partial block held back by the last [`LiveResampler::push`].
    pub fn drain(&mut self) -> Result<Vec<f32>, AsrError> {
        if self.pending.is_empty() {
            return Ok(Vec::new());
        }
        let pending = std::mem::take(&mut self.pending);
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(pending);
        };
        let block = [&pending[..]];
        let processed = resampler
            .process_partial(Some(&block), None)
            .map_err(|error| AsrError::Engine(format!("lấy mẫu lại phần cuối micro thất bại: {error}")))?;
        Ok(processed[0].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_is_averaged_not_halved() {
        let mut out = Vec::new();
        downmix(&[1.0, 0.0, 0.5, 0.5], 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5]);
    }

    #[test]
    fn a_file_already_at_16k_is_left_alone() {
        let samples = vec![0.25_f32; 320];
        let resampled = resample(samples.clone(), SAMPLE_RATE).unwrap();
        assert_eq!(resampled, samples);
    }

    #[test]
    fn resampling_48k_lands_within_a_block_of_a_third_of_the_samples() {
        let seconds = 3;
        let samples = vec![0.0_f32; 48_000 * seconds];
        let resampled = resample(samples, 48_000).unwrap();
        let expected = SAMPLE_RATE as usize * seconds;
        // Not exact: the FFT resampler carries a filter delay, so the count lands near the ideal.
        assert!(resampled.len().abs_diff(expected) < SAMPLE_RATE as usize / 4);
    }

    #[test]
    fn only_known_containers_are_offered_to_the_decoder() {
        assert!(is_audio(Path::new("hop.m4a")));
        assert!(is_audio(Path::new("/tmp/GHI.WAV")));
        assert!(!is_audio(Path::new("bao-cao.pdf")));
    }
}
