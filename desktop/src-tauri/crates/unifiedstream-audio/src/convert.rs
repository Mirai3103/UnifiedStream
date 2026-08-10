//! Conversion from whatever format a platform's audio source delivers into the negotiated wire
//! format.
//!
//! An audio system that lets a process capture the existing output device hands over that
//! device's format, not the one the wire wants: commonly 32-bit float, at whatever rate the user
//! picked, with as many channels as the endpoint has. The wire wants interleaved S16LE at the
//! negotiated rate and channel count, and it wants that regardless of the user's hardware.
//!
//! Conversion runs in one fixed order, chosen so the expensive step sees the fewest samples:
//!
//! ```text
//!   source samples @ source rate, N ch
//!         │  downmix / upmix to the target channel count
//!         ▼
//!   f32 @ source rate, target ch
//!         │  resample to the target rate      ← bypassed when the rates already match
//!         ▼
//!   f32 @ target rate, target ch
//!         │  scale, clamp, convert
//!         ▼
//!   i16 @ target rate, target ch  ──▶ FrameChunker
//! ```
//!
//! Portable, and free of any platform type, because this is the part of a system-audio capture
//! most worth testing and least able to be tested where it runs: a CI runner for the platform
//! that needs it has no audio endpoint at all. An off-by-one in the chunk accounting produces
//! audio that plays, sounds nearly right, and drifts.

use crate::sample::{self, Resampling, SampleType};
use crate::{AudioError, AudioFormat};

/// The format a platform's audio source actually delivers.
///
/// The mirror of [`AudioFormat`], which describes what the wire carries. The two are independent
/// by design: [`AudioConverter`] exists so that the second does not vary with the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFormat {
    /// Samples per second the source runs at.
    pub sample_rate: u32,
    /// Channels the source interleaves, in WAVE order.
    pub channels: u16,
    /// How the source encodes each sample.
    pub sample_type: SampleType,
}

/// Attenuation applied to a channel folded into another: −3 dB, the conventional coefficient for
/// mixing one channel into two.
const FOLD: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Converts a platform audio source's interleaved samples into the negotiated wire format.
///
/// Stateful: a resampler carries overlap between passes and holds back input that does not fill
/// a whole pass, so one instance serves one capture from start to finish. A capture that
/// re-opens on a different endpoint builds a new converter rather than reusing this one, because
/// the new endpoint's format is not the old one's.
pub struct AudioConverter {
    source: SourceFormat,
    target_channels: usize,
    /// Channel-mapped samples at the source rate, one buffer per target channel. Unused when no
    /// resampling is needed, since nothing then has to be held back.
    planar: Vec<Vec<f32>>,
    /// Absent when the source already runs at the target rate — the bypass, and the common case,
    /// since 48 kHz is the usual endpoint default.
    resampler: Option<Resampling>,
    /// Reused across calls so a steady capture stops allocating.
    out: Vec<i16>,
}

impl AudioConverter {
    /// A converter from `source` to `target`.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::UnsupportedFormat`] when either format is degenerate, when the
    /// target asks for more than two channels, or when a resampler cannot be built for the rate
    /// pair.
    pub fn new(source: SourceFormat, target: AudioFormat) -> Result<Self, AudioError> {
        if source.sample_rate == 0 || source.channels == 0 {
            return Err(AudioError::UnsupportedFormat(format!(
                "source is {}ch @ {}Hz",
                source.channels, source.sample_rate
            )));
        }
        // Two is the widest the channel map produces, and all the wire asks for.
        if target.sample_rate == 0 || target.channels == 0 || target.channels > 2 {
            return Err(AudioError::UnsupportedFormat(format!(
                "target is {}ch @ {}Hz",
                target.channels, target.sample_rate
            )));
        }

        let target_channels = usize::from(target.channels);
        let resampler = if source.sample_rate == target.sample_rate {
            None
        } else {
            Some(Resampling::new(
                source.sample_rate,
                target.sample_rate,
                target_channels,
            )?)
        };

        Ok(Self {
            source,
            target_channels,
            planar: vec![Vec::new(); target_channels],
            resampler,
            out: Vec::new(),
        })
    }

    /// Whether a resampler is in the path, or the rates already matched and it was bypassed.
    #[must_use]
    pub fn is_resampling(&self) -> bool {
        self.resampler.is_some()
    }

    /// Convert one buffer of interleaved source samples.
    ///
    /// Returns interleaved `i16` at the target rate: a whole number of frames, but not
    /// necessarily the number `input` implies, because a resampler holds back what does not fill
    /// a pass. A trailing partial source frame is ignored rather than half-decoded.
    pub fn convert(&mut self, input: &[u8]) -> &[i16] {
        self.out.clear();
        let source = self.source;
        let target_channels = self.target_channels;
        let frame_bytes = source.sample_type.width() * usize::from(source.channels);

        if self.resampler.is_none() {
            // Nothing is held back without a resampler, so map straight into the output and skip
            // the planar buffers entirely.
            for frame in input.chunks_exact(frame_bytes) {
                let mapped = map_frame(frame, source, target_channels);
                for sample in mapped.iter().take(target_channels) {
                    self.out.push(sample::to_i16(*sample));
                }
            }
            return &self.out;
        }

        for frame in input.chunks_exact(frame_bytes) {
            let mapped = map_frame(frame, source, target_channels);
            for (channel, buffer) in self.planar.iter_mut().enumerate() {
                buffer.push(mapped.get(channel).copied().unwrap_or(0.0));
            }
        }
        self.resample_pending();
        &self.out
    }

    /// Convert `frames` of silence, as if the source had delivered that many zero frames.
    ///
    /// Silence goes through the same path rather than being emitted directly, so a resampler's
    /// timing and overlap stay continuous across it. This is the ordinary path, not an edge
    /// case: a capture of an idle output device is silence for as long as the user is quiet.
    pub fn convert_silence(&mut self, frames: usize) -> &[i16] {
        self.out.clear();

        if self.resampler.is_none() {
            self.out.resize(frames * self.target_channels, 0);
            return &self.out;
        }

        for buffer in &mut self.planar {
            buffer.resize(buffer.len() + frames, 0.0);
        }
        self.resample_pending();
        &self.out
    }

    /// Run as many whole resampler passes as the pending input allows, appending to `out`.
    fn resample_pending(&mut self) {
        let Self {
            planar,
            resampler,
            out,
            ..
        } = self;
        let Some(resampling) = resampler.as_mut() else {
            return;
        };

        while planar
            .first()
            .is_some_and(|channel| channel.len() >= resampling.chunk_in())
        {
            let views: Vec<&[f32]> = planar
                .iter()
                .filter_map(|channel| channel.get(..resampling.chunk_in()))
                .collect();
            if views.len() != planar.len() {
                // Channels out of step with each other would be a bug in this module rather than
                // an input condition; refuse the pass instead of resampling a ragged buffer.
                debug_assert!(false, "planar channels have diverged in length");
                return;
            }

            let frames = resampling.process(&views);
            for frame in 0..frames {
                for channel in resampling.scratch() {
                    out.push(sample::to_i16(channel.get(frame).copied().unwrap_or(0.0)));
                }
            }

            for channel in planar.iter_mut() {
                let taken = resampling.chunk_in().min(channel.len());
                channel.drain(..taken);
            }
        }
    }
}

/// Map one interleaved source frame onto the target channels.
///
/// Mono is upmixed by duplication and stereo passes through. Beyond that the endpoint's own
/// channel mask is not consulted: 5.1 and 7.1 are folded by the fixed WAVE-order matrix below,
/// and any other count falls back to averaging even-indexed channels into the left and
/// odd-indexed into the right, which is right for quadraphonic and defensible for the rest.
/// Every fold is normalised by the sum of its coefficients, so a downmix cannot clip on its own.
///
/// The returned pair carries `target_channels` meaningful values: a mono target gets the stereo
/// result folded once more, in the same place, so both callers read the array the same way.
fn map_frame(frame: &[u8], source: SourceFormat, target_channels: usize) -> [f32; 2] {
    let at = |channel: usize| sample::decode(frame, channel, source.sample_type);

    let (left, right) = match source.channels {
        1 => {
            let mono = at(0);
            (mono, mono)
        }
        2 => (at(0), at(1)),
        // FL FR FC LFE BL BR. The LFE is dropped, as a stereo downmix conventionally does.
        6 => {
            let norm = 1.0 / (1.0 + FOLD + FOLD);
            let centre = at(2) * FOLD;
            (
                (at(0) + centre + at(4) * FOLD) * norm,
                (at(1) + centre + at(5) * FOLD) * norm,
            )
        }
        // FL FR FC LFE BL BR SL SR.
        8 => {
            let norm = 1.0 / (1.0 + 3.0 * FOLD);
            let centre = at(2) * FOLD;
            (
                (at(0) + centre + (at(4) + at(6)) * FOLD) * norm,
                (at(1) + centre + (at(5) + at(7)) * FOLD) * norm,
            )
        }
        channels => {
            let channels = usize::from(channels);
            let (mut left, mut right) = (0.0, 0.0);
            let (mut lefts, mut rights) = (0.0_f32, 0.0_f32);
            for channel in 0..channels {
                if channel % 2 == 0 {
                    left += at(channel);
                    lefts += 1.0;
                } else {
                    right += at(channel);
                    rights += 1.0;
                }
            }
            (left / lefts.max(1.0), right / rights.max(1.0))
        }
    };

    if target_channels == 1 {
        return [(left + right) * 0.5, 0.0];
    }
    [left, right]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FrameChunker;

    fn f32_bytes(samples: &[f32]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    fn i16_bytes(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    fn source(sample_rate: u32, channels: u16, sample_type: SampleType) -> SourceFormat {
        SourceFormat {
            sample_rate,
            channels,
            sample_type,
        }
    }

    fn stereo_f32(sample_rate: u32) -> SourceFormat {
        source(sample_rate, 2, SampleType::Float32)
    }

    /// Output frames one resampler pass produces, so a test can state its tolerance in terms of
    /// what the resampler can be holding back rather than a guessed constant.
    fn pass_output_samples(converter: &AudioConverter) -> usize {
        converter
            .resampler
            .as_ref()
            .map_or(0, |r| r.chunk_out() * converter.target_channels)
    }

    #[test]
    fn a_matching_rate_should_bypass_the_resampler() {
        let converter = AudioConverter::new(stereo_f32(48_000), AudioFormat::SPEAKER)
            .expect("48 kHz stereo float is convertible");
        assert!(
            !converter.is_resampling(),
            "48 kHz to 48 kHz must not build a resampler"
        );
    }

    #[test]
    fn the_bypass_should_be_sample_exact_and_lose_no_frames() {
        let mut converter =
            AudioConverter::new(source(48_000, 2, SampleType::Int16), AudioFormat::SPEAKER)
                .expect("48 kHz stereo S16 is convertible");

        // An integer source round-trips exactly: both directions scale by full scale, and the
        // bypass leaves nothing else in the path.
        let input: Vec<i16> = vec![0, 1, -1, 12_345, -12_345, 32_767, -32_768, 7];
        assert_eq!(converter.convert(&i16_bytes(&input)), input.as_slice());
    }

    #[test]
    fn a_partial_source_frame_should_be_ignored_rather_than_half_decoded() {
        let mut converter =
            AudioConverter::new(stereo_f32(48_000), AudioFormat::SPEAKER).expect("convertible");
        let mut bytes = f32_bytes(&[0.25, -0.25]);
        bytes.push(0x7f); // A frame torn in half by a buffer boundary.
        assert_eq!(converter.convert(&bytes).len(), 2);
    }

    #[test]
    fn mono_should_be_upmixed_by_duplication() {
        let mut converter =
            AudioConverter::new(source(48_000, 1, SampleType::Float32), AudioFormat::SPEAKER)
                .expect("convertible");
        assert_eq!(
            converter.convert(&f32_bytes(&[0.5, -0.25])),
            [16_384, 16_384, -8_192, -8_192]
        );
    }

    #[test]
    fn stereo_interleaving_should_be_preserved() {
        let mut converter =
            AudioConverter::new(stereo_f32(48_000), AudioFormat::SPEAKER).expect("convertible");
        // Left ramps up, right ramps down; the pairing must survive.
        let out = converter
            .convert(&f32_bytes(&[0.1, -0.1, 0.2, -0.2, 0.3, -0.3]))
            .to_vec();
        assert_eq!(out.len(), 6);
        for pair in out.chunks_exact(2) {
            match pair {
                [left, right] => {
                    assert_eq!(*left, -*right, "channels must not be swapped or mixed");
                }
                _ => unreachable!(),
            }
        }
        assert!(
            out.first().copied().unwrap_or(0) < out.get(2).copied().unwrap_or(0),
            "left must still be the rising channel"
        );
    }

    #[test]
    fn five_one_should_be_downmixed_by_the_fixed_matrix() {
        let mut converter =
            AudioConverter::new(source(48_000, 6, SampleType::Float32), AudioFormat::SPEAKER)
                .expect("convertible");

        // FL FR FC LFE BL BR: only the front-left channel carries signal.
        let norm = 1.0 / (1.0 + FOLD + FOLD);
        assert_eq!(
            converter.convert(&f32_bytes(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0])),
            [sample::to_i16(norm), 0]
        );

        // The LFE is dropped, not folded in.
        assert_eq!(
            converter.convert(&f32_bytes(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0])),
            [0, 0]
        );

        // The fold must not add gain: a fully correlated frame below full scale comes out at the
        // level it went in at, rather than summing five channels into the rail.
        let out = converter
            .convert(&f32_bytes(&[0.9, 0.9, 0.9, 0.0, 0.9, 0.9]))
            .to_vec();
        let expected = sample::to_i16(0.9);
        assert!(
            out.iter()
                .all(|s| (i32::from(*s) - i32::from(expected)).abs() <= 1),
            "a correlated 5.1 frame at 0.9 should stay at 0.9, got {out:?}"
        );
    }

    #[test]
    fn out_of_range_floats_should_clamp_at_both_rails() {
        let mut converter =
            AudioConverter::new(stereo_f32(48_000), AudioFormat::SPEAKER).expect("convertible");
        assert_eq!(
            converter.convert(&f32_bytes(&[9.0, -9.0, 1.0, -1.0])),
            [i16::MAX, i16::MIN, i16::MAX, i16::MIN],
            "an out-of-range sample must saturate, never wrap"
        );
    }

    #[test]
    fn silence_should_convert_to_zeros_at_the_target_rate() {
        let mut converter =
            AudioConverter::new(stereo_f32(48_000), AudioFormat::SPEAKER).expect("convertible");
        let out = converter.convert_silence(960);
        assert_eq!(out.len(), 1_920);
        assert!(out.iter().all(|s| *s == 0));
    }

    /// The drift case: a long run at the rate a resampler is actually needed for.
    ///
    /// `FftFixedInOut` consumes a fixed number of input frames per pass and produces a fixed
    /// number of output frames, so the ratio is exact rather than approximate and the only
    /// output not yet emitted is whatever one pass is still accumulating. Sixty seconds of
    /// 44.1 kHz input must therefore produce sixty seconds of 48 kHz output to within that one
    /// pass — and the shortfall must not grow with time, which is what a drifting implementation
    /// would do.
    #[test]
    fn a_long_forty_four_one_run_should_not_drift() {
        let mut converter =
            AudioConverter::new(stereo_f32(44_100), AudioFormat::SPEAKER).expect("convertible");
        assert!(converter.is_resampling(), "44.1 kHz needs a resampler");
        let slack = pass_output_samples(&converter);

        // 10 ms of 44.1 kHz input per push, 6000 pushes: one minute.
        let mut produced = 0_usize;
        let mut at_ten_seconds = 0_usize;
        for push in 1..=6_000 {
            produced += converter.convert_silence(441).len();
            if push == 1_000 {
                at_ten_seconds = produced;
            }
        }

        let per_second = 48_000 * AudioFormat::SPEAKER.channels as usize;
        let expected = 60 * per_second;
        assert!(
            produced <= expected && produced + slack >= expected,
            "60 s of 44.1 kHz produced {produced} samples, expected {expected} within {slack}"
        );

        let last_fifty = produced - at_ten_seconds;
        let expected_fifty = 50 * per_second;
        assert!(
            last_fifty <= expected_fifty && last_fifty + slack >= expected_fifty,
            "the last 50 s produced {last_fifty} samples, expected {expected_fifty} — \
             a shortfall that grows with time is drift"
        );
    }

    /// The composition the arithmetic above exists to serve.
    #[test]
    fn the_converter_should_feed_the_chunker_whole_frames_without_accumulating_a_remainder() {
        let mut converter =
            AudioConverter::new(stereo_f32(44_100), AudioFormat::SPEAKER).expect("convertible");
        let frame_len = AudioFormat::SPEAKER.total_samples();
        assert_eq!(frame_len, 1_920, "20 ms of 48 kHz stereo");
        let mut chunker = FrameChunker::new(frame_len);

        let mut frames = 0_usize;
        let mut produced = 0_usize;
        for _ in 0..3_000 {
            let converted = converter.convert_silence(441);
            produced += converted.len();
            chunker.push(converted, |frame| {
                assert_eq!(
                    frame.len(),
                    frame_len,
                    "only whole 20 ms frames may be emitted"
                );
                frames += 1;
            });
            assert!(
                chunker.pending_len() < frame_len,
                "the remainder must stay below one frame rather than accumulate"
            );
        }

        assert_eq!(
            frames * frame_len + chunker.pending_len(),
            produced,
            "every converted sample must be either emitted or still pending — none lost"
        );
        // 30 s of input is 1500 frames of 20 ms, less whatever one pass is still holding back.
        assert!(
            (1_495..=1_500).contains(&frames),
            "30 s produced {frames} frames of 20 ms, expected about 1500"
        );
    }
}
