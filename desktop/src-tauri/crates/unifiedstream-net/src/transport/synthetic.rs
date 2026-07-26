//! The synthetic test stream.
//!
//! Exercises the whole transport — framing, fragmentation, sequencing, reassembly — end to end
//! without any media codec, so the link can be proven before camera or audio work exists.
//!
//! Each frame carries an 8-byte header (a 32-bit frame counter and a 32-bit length) followed by
//! a deterministic pattern derived from the counter. The receiver recomputes the pattern, so a
//! frame that arrives with reordered or corrupted fragments is detected rather than accepted.

/// Bytes of bookkeeping at the front of every synthetic frame.
const PREAMBLE: usize = 8;

/// How a synthetic frame should be generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestStreamConfig {
    /// Frames per second.
    pub rate_hz: u32,
    /// Total frame size in bytes, including the preamble.
    pub frame_bytes: usize,
}

impl Default for TestStreamConfig {
    fn default() -> Self {
        // 60 frames of 4 KiB is ~2 Mbps and forces fragmentation, which is the point.
        Self {
            rate_hz: 60,
            frame_bytes: 4096,
        }
    }
}

impl TestStreamConfig {
    /// Interval between frames.
    #[must_use]
    pub fn interval(&self) -> std::time::Duration {
        let hz = self.rate_hz.max(1);
        std::time::Duration::from_nanos(1_000_000_000 / u64::from(hz))
    }

    /// Nominal bitrate in megabits per second.
    #[must_use]
    pub fn nominal_mbps(&self) -> f64 {
        #[allow(clippy::cast_precision_loss, reason = "configuration magnitudes are small")]
        let bits = (self.frame_bytes as f64) * 8.0 * f64::from(self.rate_hz);
        bits / 1_000_000.0
    }
}

/// Produces verifiable synthetic frames.
#[derive(Debug)]
pub struct TestStreamGenerator {
    config: TestStreamConfig,
    counter: u32,
}

impl TestStreamGenerator {
    /// A generator starting at frame zero.
    #[must_use]
    pub const fn new(config: TestStreamConfig) -> Self {
        Self { config, counter: 0 }
    }

    /// The configuration this generator was built with.
    #[must_use]
    pub const fn config(&self) -> TestStreamConfig {
        self.config
    }

    /// Frames produced so far.
    #[must_use]
    pub const fn produced(&self) -> u32 {
        self.counter
    }

    /// Produce the next frame.
    pub fn next_frame(&mut self) -> Vec<u8> {
        let index = self.counter;
        self.counter = self.counter.wrapping_add(1);
        build_frame(index, self.config.frame_bytes)
    }
}

/// Build the frame at `index` with `frame_bytes` total size.
#[must_use]
pub fn build_frame(index: u32, frame_bytes: usize) -> Vec<u8> {
    let size = frame_bytes.max(PREAMBLE);
    let mut frame = Vec::with_capacity(size);
    frame.extend_from_slice(&index.to_be_bytes());
    #[allow(clippy::cast_possible_truncation, reason = "frame sizes are far below 4 GiB")]
    let len = size as u32;
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend((PREAMBLE..size).map(|offset| pattern_byte(index, offset)));
    frame
}

/// The byte a correct frame carries at `offset`.
///
/// A cheap mix of the frame index and the offset: distinct across both axes, so a frame
/// assembled from the wrong fragments fails verification instead of looking plausible.
#[must_use]
const fn pattern_byte(index: u32, offset: usize) -> u8 {
    let mixed = (index as usize).wrapping_mul(31).wrapping_add(offset.wrapping_mul(7));
    (mixed % 251) as u8
}

/// Why a received synthetic frame failed verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IntegrityError {
    /// Shorter than the preamble.
    TooShort,
    /// Length field disagrees with the bytes actually present.
    LengthMismatch,
    /// Payload bytes do not match the pattern for the declared frame index.
    PayloadCorrupt,
}

/// Verification counters for a synthetic stream.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TestStreamReport {
    /// Frames that verified clean.
    pub verified: u64,
    /// Frames that arrived but failed verification.
    pub corrupt: u64,
    /// Frames never seen, inferred from gaps in the frame counter.
    pub missing: u64,
    /// Frames whose index was older than one already accepted.
    pub out_of_order: u64,
    /// Payload bytes verified.
    pub bytes: u64,
}

impl TestStreamReport {
    /// Frames expected so far.
    #[must_use]
    pub const fn expected(&self) -> u64 {
        self.verified + self.corrupt + self.missing
    }

    /// Whether every frame that was expected arrived intact.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.corrupt == 0 && self.missing == 0
    }
}

/// Verifies frames produced by [`TestStreamGenerator`].
#[derive(Debug, Default)]
pub struct TestStreamVerifier {
    report: TestStreamReport,
    highest_index: Option<u32>,
}

impl TestStreamVerifier {
    /// A verifier with no history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Counters so far.
    #[must_use]
    pub const fn report(&self) -> TestStreamReport {
        self.report
    }

    /// Check one received frame, updating the counters.
    ///
    /// # Errors
    ///
    /// Returns the specific way the frame failed to verify.
    pub fn verify(&mut self, frame: &[u8]) -> std::result::Result<u32, IntegrityError> {
        let Some(preamble) = frame.get(..PREAMBLE) else {
            self.report.corrupt += 1;
            return Err(IntegrityError::TooShort);
        };
        let (index_bytes, len_bytes) = preamble.split_at(4);

        let index = u32::from_be_bytes(index_bytes.try_into().unwrap_or([0; 4]));
        let declared = u32::from_be_bytes(len_bytes.try_into().unwrap_or([0; 4]));

        if usize::try_from(declared).unwrap_or(usize::MAX) != frame.len() {
            self.report.corrupt += 1;
            return Err(IntegrityError::LengthMismatch);
        }

        let body = frame.get(PREAMBLE..).unwrap_or_default();
        let matches = body
            .iter()
            .enumerate()
            .all(|(i, byte)| *byte == pattern_byte(index, PREAMBLE + i));
        if !matches {
            self.report.corrupt += 1;
            return Err(IntegrityError::PayloadCorrupt);
        }

        match self.highest_index {
            Some(highest) if index <= highest => {
                self.report.out_of_order += 1;
            }
            Some(highest) => {
                self.report.missing += u64::from(index - highest - 1);
                self.highest_index = Some(index);
                self.report.verified += 1;
            }
            None => {
                // Frames before the first one seen were sent before we started listening, not
                // lost, so they are not counted as missing.
                self.highest_index = Some(index);
                self.report.verified += 1;
            }
        }

        self.report.bytes += frame.len() as u64;
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{StreamId, MAX_PAYLOAD};
    use crate::transport::{MediaDemux, MediaSender};

    #[test]
    fn a_generated_frame_should_verify() {
        let mut generator = TestStreamGenerator::new(TestStreamConfig::default());
        let mut verifier = TestStreamVerifier::new();

        let frame = generator.next_frame();
        assert_eq!(verifier.verify(&frame), Ok(0));
        assert_eq!(verifier.report().verified, 1);
    }

    #[test]
    fn frames_should_carry_an_increasing_index() {
        let mut generator = TestStreamGenerator::new(TestStreamConfig::default());
        let mut verifier = TestStreamVerifier::new();

        for expected in 0..10_u32 {
            let frame = generator.next_frame();
            assert_eq!(verifier.verify(&frame), Ok(expected));
        }
        assert!(verifier.report().is_clean());
    }

    #[test]
    fn a_generated_frame_should_have_the_configured_size() {
        let config = TestStreamConfig {
            rate_hz: 30,
            frame_bytes: 1_500,
        };
        let mut generator = TestStreamGenerator::new(config);
        assert_eq!(generator.next_frame().len(), 1_500);
    }

    #[test]
    fn a_corrupted_byte_should_fail_verification() {
        let mut generator = TestStreamGenerator::new(TestStreamConfig::default());
        let mut verifier = TestStreamVerifier::new();

        let mut frame = generator.next_frame();
        if let Some(byte) = frame.get_mut(100) {
            *byte = byte.wrapping_add(1);
        }

        assert_eq!(verifier.verify(&frame), Err(IntegrityError::PayloadCorrupt));
        assert_eq!(verifier.report().corrupt, 1);
    }

    #[test]
    fn a_truncated_frame_should_fail_verification() {
        let mut generator = TestStreamGenerator::new(TestStreamConfig::default());
        let mut verifier = TestStreamVerifier::new();

        let frame = generator.next_frame();
        let truncated = frame.get(..frame.len() - 1).unwrap_or_default();

        assert_eq!(
            verifier.verify(truncated),
            Err(IntegrityError::LengthMismatch)
        );
    }

    #[test]
    fn a_frame_shorter_than_the_preamble_should_fail_verification() {
        let mut verifier = TestStreamVerifier::new();
        assert_eq!(verifier.verify(&[1, 2, 3]), Err(IntegrityError::TooShort));
    }

    #[test]
    fn swapping_two_frames_payloads_should_be_detected() {
        let mut verifier = TestStreamVerifier::new();
        // Frame 5's bytes labelled as frame 6: exactly what a mis-reassembled frame looks like.
        let mut frame = build_frame(5, 512);
        frame.splice(0..4, 6_u32.to_be_bytes());

        assert_eq!(verifier.verify(&frame), Err(IntegrityError::PayloadCorrupt));
    }

    #[test]
    fn a_missing_frame_should_be_counted() {
        let mut verifier = TestStreamVerifier::new();
        verifier.verify(&build_frame(0, 256)).expect("frame 0");
        verifier.verify(&build_frame(3, 256)).expect("frame 3");

        assert_eq!(verifier.report().missing, 2);
        assert!(!verifier.report().is_clean());
    }

    #[test]
    fn a_replayed_frame_should_be_counted_out_of_order() {
        let mut verifier = TestStreamVerifier::new();
        verifier.verify(&build_frame(0, 256)).expect("frame 0");
        verifier.verify(&build_frame(1, 256)).expect("frame 1");
        verifier.verify(&build_frame(0, 256)).expect("replayed");

        assert_eq!(verifier.report().out_of_order, 1);
        assert_eq!(verifier.report().missing, 0);
    }

    #[test]
    fn joining_a_stream_late_should_not_count_earlier_frames_as_missing() {
        let mut verifier = TestStreamVerifier::new();
        verifier.verify(&build_frame(500, 256)).expect("frame 500");
        assert_eq!(verifier.report().missing, 0);
    }

    #[test]
    fn the_default_config_should_force_fragmentation() {
        let config = TestStreamConfig::default();
        assert!(
            config.frame_bytes > MAX_PAYLOAD,
            "the default test stream must exercise the fragmentation path"
        );
    }

    #[test]
    fn the_interval_should_follow_the_configured_rate() {
        let config = TestStreamConfig {
            rate_hz: 100,
            frame_bytes: 256,
        };
        assert_eq!(config.interval(), std::time::Duration::from_millis(10));
    }

    #[test]
    fn a_zero_rate_should_not_divide_by_zero() {
        let config = TestStreamConfig {
            rate_hz: 0,
            frame_bytes: 256,
        };
        assert_eq!(config.interval(), std::time::Duration::from_secs(1));
    }

    #[test]
    fn the_nominal_bitrate_should_match_the_configuration() {
        let config = TestStreamConfig {
            rate_hz: 100,
            frame_bytes: 1_250,
        };
        // 1250 bytes * 8 bits * 100 Hz = 1 Mbps
        assert!((config.nominal_mbps() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_fragmented_test_frame_should_verify_after_the_full_transport_round_trip() {
        let mut generator = TestStreamGenerator::new(TestStreamConfig::default());
        let mut verifier = TestStreamVerifier::new();
        let mut tx = MediaSender::new(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        for _ in 0..5 {
            let frame = generator.next_frame();
            for datagram in tx.frame(StreamId::TEST, &frame) {
                if let Ok(frames) = demux.accept(&datagram) {
                    for received in frames {
                        verifier.verify(&received.payload).expect("must verify");
                    }
                }
            }
        }

        assert_eq!(verifier.report().verified, 5);
        assert!(verifier.report().is_clean());
    }

    #[test]
    fn a_dropped_fragment_should_surface_as_a_missing_frame_not_a_corrupt_one() {
        let mut generator = TestStreamGenerator::new(TestStreamConfig::default());
        let mut verifier = TestStreamVerifier::new();
        let mut tx = MediaSender::new(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        for index in 0..4 {
            let frame = generator.next_frame();
            let datagrams = tx.frame(StreamId::TEST, &frame);
            for (position, datagram) in datagrams.iter().enumerate() {
                // Lose one fragment of the second frame.
                if index == 1 && position == 1 {
                    continue;
                }
                if let Ok(frames) = demux.accept(datagram) {
                    for received in frames {
                        let _ = verifier.verify(&received.payload);
                    }
                }
            }
        }

        let report = verifier.report();
        assert_eq!(report.corrupt, 0, "a lost fragment must not look like corruption");
        assert!(report.missing >= 1, "the lost frame must be reported missing");
    }
}
