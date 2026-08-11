//! Conversion from the negotiated wire format into whatever format a platform's audio endpoint
//! renders.
//!
//! The mirror image of [`crate::AudioConverter`], and deliberately a second type rather than the
//! same one run backwards. The transformations are symmetric; the code around them is not. The
//! capture converts a packet the system handed it, of a size the system chose, and pushes the
//! result into a chunker. This side is *asked* for a specific number of endpoint frames by the
//! audio engine and must produce exactly that many, every period, indefinitely — a shortfall is
//! not a smaller buffer, it is a glitch. The shared arithmetic lives in [`crate::sample`]; the
//! asymmetry lives here.
//!
//! Conversion runs in one fixed order, chosen so the expensive step sees the fewest samples —
//! which puts the channel step *after* the resampler here, exactly because the wire is the narrow
//! side in this direction and the endpoint is the wide one:
//!
//! ```text
//!   i16 @ wire rate, wire ch      ◀── JitterBuffer
//!         │  scale to float
//!         ▼
//!   f32 @ wire rate, wire ch
//!         │  resample to the endpoint rate   ← bypassed when the rates already match
//!         ▼
//!   f32 @ endpoint rate, wire ch
//!         │  distribute across the endpoint's channels
//!         ▼
//!   f32 @ endpoint rate, endpoint ch
//!         │  scale, clamp, encode
//!         ▼
//!   endpoint samples, interleaved ──▶ IAudioRenderClient
//! ```
//!
//! Portable and free of any platform type, for the same reason its mirror is: the CI runner for
//! the platform that needs this has no audio endpoint at all, and the failure mode of the
//! arithmetic is audio that plays and is subtly wrong rather than audio that does not play.

use crate::sample::{self, Resampling, SampleType, FULL_SCALE};
use crate::{AudioError, AudioFormat};

/// The format a platform's audio endpoint renders.
///
/// The mirror of [`crate::SourceFormat`], which describes what a platform's audio source
/// delivers. Two types rather than one shared "device format" because they are read in opposite
/// directions and a single name could only be right about one of them: the same fields describe
/// where samples come from there and where they are going here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointFormat {
    /// Samples per second the endpoint runs at. User-configurable on a virtual cable, so this is
    /// not reliably 48 kHz and the resampler is an ordinary path rather than an exceptional one.
    pub sample_rate: u32,
    /// Channels the endpoint interleaves, in WAVE order.
    pub channels: u16,
    /// How the endpoint encodes each sample.
    pub sample_type: SampleType,
}

impl EndpointFormat {
    /// Bytes one interleaved endpoint frame occupies, across all channels.
    #[must_use]
    pub const fn block_align(&self) -> usize {
        self.sample_type.width() * self.channels as usize
    }
}

/// Converts decoded wire frames into an endpoint's interleaved samples.
///
/// Stateful: a resampler carries overlap between passes, and output frames that a pass produced
/// beyond what the engine asked for are held until the next request. One instance serves one
/// stream from start to finish; an endpoint that changes format needs a new converter, because
/// the new format is not the old one.
pub struct RenderConverter {
    endpoint: EndpointFormat,
    wire_channels: usize,
    /// Absent when the wire already runs at the endpoint's rate — the bypass, and the common case,
    /// since 48 kHz is the usual endpoint default and the wire's only rate.
    resampler: Option<Resampling>,
    /// Interleaved wire samples pulled from the caller, reused so a steady stream stops
    /// allocating.
    wire: Vec<i16>,
    /// Deinterleaved wire samples at the wire rate, one buffer per wire channel: the resampler's
    /// input view. Unused on the bypass.
    planar: Vec<Vec<f32>>,
    /// Resampled frames at the endpoint rate that no request has claimed yet, one buffer per wire
    /// channel. Bounded by one pass: the loop stops as soon as a request can be served.
    pending: Vec<Vec<f32>>,
    /// The interleaved endpoint bytes of the current request.
    out: Vec<u8>,
}

impl RenderConverter {
    /// A converter from the negotiated wire format to `endpoint`.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::UnsupportedFormat`] when either format is degenerate, when the wire
    /// carries more than two channels — the wire is mono for the microphone and stereo at the
    /// widest the protocol negotiates — or when a resampler cannot be built for the rate pair.
    pub fn new(wire: AudioFormat, endpoint: EndpointFormat) -> Result<Self, AudioError> {
        if wire.sample_rate == 0 || wire.channels == 0 || wire.channels > 2 {
            return Err(AudioError::UnsupportedFormat(format!(
                "wire is {}ch @ {}Hz",
                wire.channels, wire.sample_rate
            )));
        }
        if endpoint.sample_rate == 0 || endpoint.channels == 0 {
            return Err(AudioError::UnsupportedFormat(format!(
                "endpoint is {}ch @ {}Hz",
                endpoint.channels, endpoint.sample_rate
            )));
        }

        let wire_channels = usize::from(wire.channels);
        let resampler = if wire.sample_rate == endpoint.sample_rate {
            None
        } else {
            Some(Resampling::new(
                wire.sample_rate,
                endpoint.sample_rate,
                wire_channels,
            )?)
        };

        // Sized up front from what one pass can leave behind, so a steady stream never reallocates
        // on the audio thread.
        let surplus = resampler.as_ref().map_or(0, Resampling::chunk_out);
        Ok(Self {
            endpoint,
            wire_channels,
            resampler,
            wire: Vec::new(),
            planar: vec![Vec::new(); wire_channels],
            pending: vec![Vec::with_capacity(surplus); wire_channels],
            out: Vec::new(),
        })
    }

    /// Whether a resampler is in the path, or the rates already matched and it was bypassed.
    #[must_use]
    pub fn is_resampling(&self) -> bool {
        self.resampler.is_some()
    }

    /// The endpoint format this converter emits.
    #[must_use]
    pub fn endpoint(&self) -> EndpointFormat {
        self.endpoint
    }

    /// Produce exactly `frames` interleaved endpoint frames, drawing wire samples from `fill`.
    ///
    /// `fill` is handed a slice of interleaved wire samples to populate — [`crate::JitterBuffer`]'s
    /// `pop_into` fits it directly — and must fill the whole slice, zero-padding whatever it
    /// cannot supply. The returned slice is always `frames * block_align` bytes: the engine asked
    /// for a buffer of that size and a short write is a glitch, so the count is a contract rather
    /// than a best effort.
    ///
    /// How many wire frames one call consumes is *not* fixed when a resampler is in the path: the
    /// converter pulls whole passes until it can serve the request and keeps the surplus. Over a
    /// long run the consumption converges on the exact rate ratio, which is what keeps the stream
    /// from drifting.
    pub fn render<F: FnMut(&mut [i16])>(&mut self, frames: usize, mut fill: F) -> &[u8] {
        self.out.clear();
        self.out.reserve(frames * self.endpoint.block_align());

        if self.resampler.is_none() {
            // Nothing is held back without a resampler, so one request is exactly one pull.
            self.wire.clear();
            self.wire.resize(frames * self.wire_channels, 0);
            fill(&mut self.wire);
            for frame in self.wire.chunks_exact(self.wire_channels) {
                let pair = spread(frame);
                emit(pair, self.endpoint, &mut self.out);
            }
            return &self.out;
        }

        self.fill_pending(frames, &mut fill);

        let Self {
            endpoint,
            wire_channels,
            pending,
            out,
            ..
        } = self;
        for frame in 0..frames {
            let mut pair = [0.0_f32; 2];
            for (channel, buffer) in pending.iter().enumerate() {
                if let Some(slot) = pair.get_mut(channel) {
                    *slot = buffer.get(frame).copied().unwrap_or(0.0);
                }
            }
            emit(widen(pair, *wire_channels), *endpoint, out);
        }
        for buffer in pending.iter_mut() {
            let taken = frames.min(buffer.len());
            buffer.drain(..taken);
        }

        &self.out
    }

    /// Pull and resample whole passes until `frames` endpoint frames are available.
    fn fill_pending<F: FnMut(&mut [i16])>(&mut self, frames: usize, fill: &mut F) {
        let Self {
            wire_channels,
            resampler,
            wire,
            planar,
            pending,
            ..
        } = self;
        let Some(resampling) = resampler.as_mut() else {
            return;
        };
        let chunk_in = resampling.chunk_in();

        while pending.first().is_none_or(|buffer| buffer.len() < frames) {
            wire.clear();
            wire.resize(chunk_in * *wire_channels, 0);
            fill(wire);

            for (channel, buffer) in planar.iter_mut().enumerate() {
                buffer.clear();
                buffer.extend(wire.chunks_exact(*wire_channels).map(|frame| {
                    frame
                        .get(channel)
                        .map_or(0.0, |s| f32::from(*s) / FULL_SCALE)
                }));
            }

            let views: Vec<&[f32]> = planar.iter().map(Vec::as_slice).collect();
            let produced = resampling.process(&views);
            for (channel, buffer) in pending.iter_mut().enumerate() {
                let scratch = resampling
                    .scratch()
                    .get(channel)
                    .and_then(|channel| channel.get(..produced))
                    .unwrap_or(&[]);
                buffer.extend_from_slice(scratch);
            }

            if produced == 0 {
                // A pass that produced nothing is a resampler failure, already logged. Looping
                // would spin against it and starve the engine of the buffer it is waiting on; the
                // shortfall becomes silence at the tail of this request instead.
                break;
            }
        }
    }
}

/// Widen one interleaved wire frame to the pair the endpoint distribution reads.
fn spread(frame: &[i16]) -> [f32; 2] {
    let left = frame.first().map_or(0.0, |s| f32::from(*s) / FULL_SCALE);
    let right = frame.get(1).map_or(left, |s| f32::from(*s) / FULL_SCALE);
    [left, right]
}

/// The same widening for samples already in float form, where a mono wire leaves one meaningful
/// value in the first slot.
fn widen(pair: [f32; 2], wire_channels: usize) -> [f32; 2] {
    let [left, right] = pair;
    if wire_channels == 1 {
        [left, left]
    } else {
        [left, right]
    }
}

/// Distribute one stereo pair across the endpoint's channels and append the interleaved frame.
///
/// **Which channels receive the signal, and why.** The wire is mono for the microphone, and the
/// endpoint is almost always stereo; where it is wider, the signal goes to the first two channels
/// — front left and front right in WAVE order — and every remaining channel is silent.
///
/// - *Not all channels.* Copying a mono signal into all six channels of a 5.1 endpoint would put
///   speech into the LFE, where a subwoofer renders it as rumble, and would sum to +8 dB in any
///   consumer that downmixes by adding channels.
/// - *Not the centre channel alone*, which is where a single voice conventionally belongs: an
///   application that takes the endpoint's first two channels and ignores the rest — which is what
///   a capture of a virtual cable's other half usually does — would record silence, and silence is
///   the failure this whole path is built to avoid.
/// - *Front left and right* are in every downmix matrix that exists, so the signal survives
///   whatever the consumer does with it.
///
/// A single-channel endpoint gets the pair folded, which for the mono wire is the sample itself.
fn emit(pair: [f32; 2], endpoint: EndpointFormat, out: &mut Vec<u8>) {
    let [left, right] = pair;
    if endpoint.channels == 1 {
        sample::encode((left + right) * 0.5, endpoint.sample_type, out);
        return;
    }
    for channel in 0..usize::from(endpoint.channels) {
        let value = match channel {
            0 => left,
            1 => right,
            _ => 0.0,
        };
        sample::encode(value, endpoint.sample_type, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIRE: AudioFormat = AudioFormat::MICROPHONE;

    fn endpoint(sample_rate: u32, channels: u16, sample_type: SampleType) -> EndpointFormat {
        EndpointFormat {
            sample_rate,
            channels,
            sample_type,
        }
    }

    /// A `fill` that hands out a fixed sequence and then silence, counting what it was asked for.
    struct Wire {
        samples: std::collections::VecDeque<i16>,
        consumed: usize,
    }

    impl Wire {
        fn new(samples: impl IntoIterator<Item = i16>) -> Self {
            Self {
                samples: samples.into_iter().collect(),
                consumed: 0,
            }
        }

        fn silent() -> Self {
            Self::new([])
        }

        fn fill(&mut self, out: &mut [i16]) {
            self.consumed += out.len();
            for slot in out.iter_mut() {
                *slot = self.samples.pop_front().unwrap_or(0);
            }
        }
    }

    fn i16_from(bytes: &[u8]) -> Vec<i16> {
        bytes
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    fn f32_from(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|quad| f32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
            .collect()
    }

    #[test]
    fn a_matching_rate_should_bypass_the_resampler() {
        let converter = RenderConverter::new(WIRE, endpoint(48_000, 2, SampleType::Float32))
            .expect("48 kHz stereo float is renderable");
        assert!(
            !converter.is_resampling(),
            "48 kHz to 48 kHz must not build a resampler"
        );
    }

    #[test]
    fn the_bypass_should_be_sample_exact_into_a_matching_integer_endpoint() {
        let mut converter = RenderConverter::new(WIRE, endpoint(48_000, 1, SampleType::Int16))
            .expect("48 kHz mono S16 is renderable");
        let input = vec![0_i16, 1, -1, 12_345, -12_345, i16::MAX, i16::MIN, 7];
        let mut wire = Wire::new(input.clone());

        let out = converter.render(input.len(), |slice| wire.fill(slice));
        assert_eq!(i16_from(out), input, "the bypass must not alter a sample");
    }

    #[test]
    fn mono_should_reach_both_channels_of_a_stereo_endpoint() {
        let mut converter =
            RenderConverter::new(WIRE, endpoint(48_000, 2, SampleType::Int16)).expect("renderable");
        let mut wire = Wire::new([1_000, -2_000]);

        let out = i16_from(converter.render(2, |slice| wire.fill(slice)));
        assert_eq!(out, [1_000, 1_000, -2_000, -2_000]);
    }

    #[test]
    fn mono_should_reach_the_front_pair_of_a_five_one_endpoint_and_nothing_else() {
        let mut converter =
            RenderConverter::new(WIRE, endpoint(48_000, 6, SampleType::Int16)).expect("renderable");
        let mut wire = Wire::new([9_000]);

        let out = i16_from(converter.render(1, |slice| wire.fill(slice)));
        // FL FR FC LFE BL BR: the centre stays silent, and so above all does the LFE.
        assert_eq!(out, [9_000, 9_000, 0, 0, 0, 0]);
    }

    #[test]
    fn an_integer_wire_should_reach_a_float_endpoint_at_the_same_level() {
        let mut converter = RenderConverter::new(WIRE, endpoint(48_000, 2, SampleType::Float32))
            .expect("renderable");
        let mut wire = Wire::new([i16::MAX, i16::MIN, 16_384]);

        let out = f32_from(converter.render(3, |slice| wire.fill(slice)));
        assert_eq!(out.len(), 6);
        // Full scale in either direction lands on the rails rather than beyond them, and a half
        // scale sample stays at half scale.
        assert!((out[0] - 0.999_969_5).abs() < 1e-6, "got {}", out[0]);
        assert_eq!(out[2], -1.0);
        assert_eq!(out[4], 0.5);
        for pair in out.chunks_exact(2) {
            assert_eq!(pair[0], pair[1], "both channels carry the same mono sample");
        }
    }

    #[test]
    fn a_thirty_two_bit_endpoint_should_be_scaled_not_truncated() {
        let mut converter =
            RenderConverter::new(WIRE, endpoint(48_000, 1, SampleType::Int32)).expect("renderable");
        let mut wire = Wire::new([16_384, i16::MIN]);

        let out = converter.render(2, |slice| wire.fill(slice)).to_vec();
        let values: Vec<i32> = out
            .chunks_exact(4)
            .map(|q| i32::from_le_bytes([q[0], q[1], q[2], q[3]]))
            .collect();
        assert_eq!(values, [1_073_741_824, i32::MIN]);
    }

    #[test]
    fn every_request_should_be_served_in_full_at_a_ratio_that_does_not_divide_evenly() {
        // 44.1 kHz is the rate a virtual cable is most often reconfigured to, and 48000/44100 is
        // not a whole ratio in either direction: whatever one resampler pass produces, the engine
        // still asked for exactly this many frames.
        let mut converter = RenderConverter::new(WIRE, endpoint(44_100, 2, SampleType::Float32))
            .expect("renderable");
        assert!(converter.is_resampling(), "44.1 kHz needs a resampler");
        let mut wire = Wire::silent();

        for request in [1_usize, 7, 441, 1_024, 4_800, 3] {
            let out = converter.render(request, |slice| wire.fill(slice));
            assert_eq!(
                out.len(),
                request * 2 * 4,
                "a request for {request} frames must yield exactly {request}"
            );
        }
    }

    #[test]
    fn a_long_forty_four_one_run_should_consume_the_wire_at_the_exact_ratio() {
        // The drift case, read from the input side: the endpoint is served exactly by
        // construction, so what a drifting implementation would get wrong is how much wire audio
        // it swallowed to do it. Sixty seconds of 44.1 kHz output must consume sixty seconds of
        // 48 kHz input, to within whatever one pass is holding back — and the discrepancy must
        // not grow with time.
        let mut converter =
            RenderConverter::new(WIRE, endpoint(44_100, 2, SampleType::Int16)).expect("renderable");
        let mut wire = Wire::silent();
        let slack = converter
            .resampler
            .as_ref()
            .map_or(0, |r| r.chunk_in() + r.chunk_out());

        // 441 endpoint frames per pass, 6000 passes: one minute of 44.1 kHz output.
        let mut at_ten_seconds = 0_usize;
        for pass in 1..=6_000 {
            converter.render(441, |slice| wire.fill(slice));
            if pass == 1_000 {
                at_ten_seconds = wire.consumed;
            }
        }

        let expected = 60 * 48_000_usize;
        assert!(
            wire.consumed >= expected && wire.consumed <= expected + slack,
            "60 s of 44.1 kHz output consumed {} wire frames, expected {expected} within {slack}",
            wire.consumed
        );

        let last_fifty = wire.consumed - at_ten_seconds;
        let expected_fifty = 50 * 48_000_usize;
        assert!(
            last_fifty >= expected_fifty.saturating_sub(slack)
                && last_fifty <= expected_fifty + slack,
            "the last 50 s consumed {last_fifty} wire frames, expected {expected_fifty} — \
             a discrepancy that grows with time is drift"
        );
    }

    #[test]
    fn an_endpoint_at_the_wire_rate_should_consume_exactly_what_it_renders() {
        let mut converter = RenderConverter::new(WIRE, endpoint(48_000, 2, SampleType::Float32))
            .expect("renderable");
        let mut wire = Wire::silent();
        for _ in 0..1_000 {
            converter.render(480, |slice| wire.fill(slice));
        }
        assert_eq!(wire.consumed, 480_000, "the bypass must hold nothing back");
    }

    #[test]
    fn resampler_overshoot_should_be_clamped_rather_than_wrapped() {
        // A square wave at full scale is the worst case for resampler ringing, and the reason
        // every encode clamps: the overshoot genuinely exceeds full scale, and an unclamped
        // narrowing conversion of that is a burst of noise at the opposite polarity. A float
        // endpoint is what makes the excess visible — an integer one would already have hidden it
        // in the conversion this test is about.
        let mut converter = RenderConverter::new(WIRE, endpoint(44_100, 1, SampleType::Float32))
            .expect("renderable");
        let square: Vec<i16> = (0..48_000)
            .map(|n| {
                if (n / 24) % 2 == 0 {
                    i16::MAX
                } else {
                    i16::MIN
                }
            })
            .collect();
        let mut wire = Wire::new(square);

        let mut loud = 0_usize;
        for _ in 0..90 {
            for value in f32_from(converter.render(441, |slice| wire.fill(slice))) {
                assert!(
                    (-1.0..=1.0).contains(&value),
                    "{value} left full scale; the clamp is what stops that reaching an endpoint"
                );
                if value.abs() > 0.9 {
                    loud += 1;
                }
            }
        }
        assert!(loud > 0, "the test signal should have reached full scale");
    }

    #[test]
    fn a_degenerate_format_should_be_refused_rather_than_rendered() {
        assert!(RenderConverter::new(WIRE, endpoint(0, 2, SampleType::Int16)).is_err());
        assert!(RenderConverter::new(WIRE, endpoint(48_000, 0, SampleType::Int16)).is_err());
        let wide = AudioFormat {
            channels: 6,
            ..WIRE
        };
        assert!(RenderConverter::new(wide, endpoint(48_000, 2, SampleType::Int16)).is_err());
    }
}
