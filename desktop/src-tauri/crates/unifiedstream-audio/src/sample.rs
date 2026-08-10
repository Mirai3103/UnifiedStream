//! The numeric core the two directional converters share: sample decoding, sample encoding with
//! clamping, and the `rubato` wrapper that resamples between two fixed rates.
//!
//! Factored out rather than duplicated because the arithmetic really is identical in both
//! directions, and it is the part of audio conversion that fails silently: a wrong scale factor
//! is a quiet recording, a missing clamp is a burst of noise at the opposite polarity, and a
//! resampler whose chunk accounting is off drifts rather than breaks.
//!
//! This is reuse *below* [`crate::AudioConverter`] and [`crate::RenderConverter`], not one type
//! with a direction flag. The two converters differ in what surrounds the arithmetic — the
//! capture is handed a packet of a size the system chose, the renderer is asked for a frame count
//! it must produce exactly, and only the renderer owns an underrun policy — and folding them
//! together would produce a type whose behaviour depends on which way it was constructed.

use rubato::{FftFixedInOut, Resampler};

use crate::AudioError;

/// How an endpoint encodes one sample in its interleaved byte stream.
///
/// Little-endian throughout: these are the formats a shared-mode mixer reports, and every
/// platform this crate targets is little-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleType {
    /// 32-bit IEEE float, nominally within `-1.0..=1.0`. What a shared-mode mixer usually
    /// reports, and the only format that can arrive out of range.
    Float32,
    /// 16-bit signed integer.
    Int16,
    /// 32-bit signed integer.
    Int32,
}

impl SampleType {
    /// Bytes one sample of this type occupies.
    #[must_use]
    pub const fn width(self) -> usize {
        match self {
            Self::Int16 => 2,
            Self::Float32 | Self::Int32 => 4,
        }
    }
}

/// Full-scale magnitude for 16-bit samples. Used in both directions so an integer source
/// round-trips exactly: only a float sample outside `-1.0..=1.0` is altered, and then by the
/// clamp rather than by wrapping.
pub const FULL_SCALE: f32 = 32_768.0;

/// Full-scale magnitude for 32-bit samples, `2^31`. Exactly representable in `f32`, so the rails
/// are hit rather than approached.
const FULL_SCALE_32: f32 = 2_147_483_648.0;

/// Input frames gathered before one resampler pass, as a fraction of a second.
///
/// The resampler rounds this up to a whole number of its own periods, so the value only sets the
/// scale: a hundredth of a second keeps added latency near 10 ms at the rates endpoints actually
/// run at, and keeps one pass small enough to stay in cache.
const RESAMPLER_CHUNK_DIVISOR: u32 = 100;

/// Decode one channel of one interleaved frame to a float in `-1.0..=1.0`, except that a float
/// stream may already be outside that range — clamping belongs to the encode step, at the end.
pub fn decode(frame: &[u8], channel: usize, sample_type: SampleType) -> f32 {
    let width = sample_type.width();
    let start = channel * width;
    let Some(bytes) = frame.get(start..start + width) else {
        return 0.0;
    };
    match (sample_type, bytes) {
        (SampleType::Float32, [a, b, c, d]) => f32::from_le_bytes([*a, *b, *c, *d]),
        (SampleType::Int16, [a, b]) => f32::from(i16::from_le_bytes([*a, *b])) / FULL_SCALE,
        (SampleType::Int32, [a, b, c, d]) => {
            i32::from_le_bytes([*a, *b, *c, *d]) as f32 / FULL_SCALE_32
        }
        _ => 0.0,
    }
}

/// Append one sample to `out` in the endpoint's encoding, little-endian.
///
/// Every arm clamps. That is the point of this function rather than an incidental safety net: a
/// resampler overshoots on transients, so a signal that was inside full scale on the wire is not
/// guaranteed to still be inside it after the rate conversion, and an unclamped narrowing
/// conversion of an out-of-range sample is what turns a loud passage into noise at the opposite
/// polarity. No representable input can wrap, in either direction, at any width.
pub fn encode(sample: f32, sample_type: SampleType, out: &mut Vec<u8>) {
    match sample_type {
        SampleType::Float32 => out.extend_from_slice(&sample.clamp(-1.0, 1.0).to_le_bytes()),
        SampleType::Int16 => out.extend_from_slice(&to_i16(sample).to_le_bytes()),
        SampleType::Int32 => out.extend_from_slice(&to_i32(sample).to_le_bytes()),
    }
}

/// Scale, clamp, and convert one float sample to 16-bit.
pub fn to_i16(sample: f32) -> i16 {
    let scaled = (sample * FULL_SCALE).round();
    if scaled >= f32::from(i16::MAX) {
        i16::MAX
    } else if scaled <= f32::from(i16::MIN) {
        i16::MIN
    } else {
        scaled as i16
    }
}

/// Scale, clamp, and convert one float sample to 32-bit.
///
/// The rails are compared in `f32` against `±2^31`, not against `i32::MAX` converted to `f32`:
/// that conversion rounds up to `2^31` anyway, and naming the power of two says why the
/// comparison is exact.
fn to_i32(sample: f32) -> i32 {
    let scaled = sample * FULL_SCALE_32;
    if scaled >= FULL_SCALE_32 {
        i32::MAX
    } else if scaled <= -FULL_SCALE_32 {
        i32::MIN
    } else {
        scaled as i32
    }
}

/// A fixed-ratio resampler and the scratch its interface requires.
///
/// `FftFixedInOut` consumes a fixed number of input frames per pass and produces a fixed number
/// of output frames, which is what makes the ratio exact rather than approximate: a caller that
/// accounts for whole passes cannot drift, however long it runs.
pub struct Resampling {
    inner: FftFixedInOut<f32>,
    /// Input frames one pass consumes, per channel. Fixed for the resampler's lifetime, which is
    /// what makes the output frames per pass fixed too.
    chunk_in: usize,
    /// Output frames one pass produces, per channel. Also fixed.
    chunk_out: usize,
    /// One buffer per channel, each `chunk_out` long.
    scratch: Vec<Vec<f32>>,
}

impl Resampling {
    /// A resampler from `source_rate` to `target_rate` over `channels` planar channels.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::UnsupportedFormat`] when no resampler can be built for the rate pair.
    pub fn new(source_rate: u32, target_rate: u32, channels: usize) -> Result<Self, AudioError> {
        let chunk_hint = (source_rate / RESAMPLER_CHUNK_DIVISOR).max(1) as usize;
        let inner = FftFixedInOut::<f32>::new(
            source_rate as usize,
            target_rate as usize,
            chunk_hint,
            channels,
        )
        .map_err(|e| {
            AudioError::UnsupportedFormat(format!("{source_rate}Hz to {target_rate}Hz: {e}"))
        })?;

        let chunk_in = inner.input_frames_next();
        let chunk_out = inner.output_frames_max();
        Ok(Self {
            inner,
            chunk_in,
            chunk_out,
            scratch: vec![vec![0.0; chunk_out]; channels],
        })
    }

    /// Input frames one pass consumes, per channel.
    pub const fn chunk_in(&self) -> usize {
        self.chunk_in
    }

    /// Output frames one pass produces, per channel.
    pub const fn chunk_out(&self) -> usize {
        self.chunk_out
    }

    /// Run one pass over `views`, each of which must be exactly [`chunk_in`](Self::chunk_in)
    /// frames long, and return how many output frames landed in [`scratch`](Self::scratch).
    ///
    /// A failed pass is reported as zero frames rather than as an error: the caller is a real-time
    /// audio path in both directions, and dropping one chunk keeps the stream running where
    /// propagating the failure would end it.
    pub fn process(&mut self, views: &[&[f32]]) -> usize {
        match self
            .inner
            .process_into_buffer(views, &mut self.scratch, None)
        {
            Ok((_, produced)) => produced.min(self.chunk_out),
            Err(e) => {
                tracing::warn!(error = %e, "resampler pass failed; dropping the input chunk");
                0
            }
        }
    }

    /// The planar output of the most recent [`process`](Self::process), one buffer per channel.
    pub fn scratch(&self) -> &[Vec<f32>] {
        &self.scratch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_should_clamp_at_both_rails_for_every_width() {
        for sample_type in [SampleType::Float32, SampleType::Int16, SampleType::Int32] {
            let mut high = Vec::new();
            let mut low = Vec::new();
            encode(9.0, sample_type, &mut high);
            encode(-9.0, sample_type, &mut low);

            let mut full_high = Vec::new();
            let mut full_low = Vec::new();
            encode(1.0, sample_type, &mut full_high);
            encode(-1.0, sample_type, &mut full_low);

            assert_eq!(high, full_high, "{sample_type:?} must saturate, never wrap");
            assert_eq!(low, full_low, "{sample_type:?} must saturate, never wrap");
        }
    }

    #[test]
    fn the_sixteen_bit_rails_should_be_the_integer_rails() {
        assert_eq!(to_i16(1.0), i16::MAX);
        assert_eq!(to_i16(-1.0), i16::MIN);
    }

    #[test]
    fn the_thirty_two_bit_rails_should_be_the_integer_rails() {
        assert_eq!(to_i32(1.0), i32::MAX);
        assert_eq!(to_i32(-1.0), i32::MIN);
    }

    #[test]
    fn an_integer_sample_should_survive_a_decode_and_encode_round_trip() {
        // What makes the resampler bypass sample-exact in both directions.
        for value in [0_i16, 1, -1, 12_345, -12_345, i16::MAX, i16::MIN] {
            let bytes = value.to_le_bytes();
            let decoded = decode(&bytes, 0, SampleType::Int16);
            assert_eq!(to_i16(decoded), value, "{value} did not round-trip");
        }
    }

    #[test]
    fn a_truncated_frame_should_decode_as_silence_rather_than_reading_past_it() {
        assert_eq!(decode(&[0x7f], 0, SampleType::Int16), 0.0);
        assert_eq!(decode(&[0x01, 0x02], 4, SampleType::Int16), 0.0);
    }
}
