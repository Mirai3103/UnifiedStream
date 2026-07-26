//! Send-side frame assembly between the audio graph and the network.
//!
//! The graph delivers whatever its quantum happens to be — 256 samples, 1024, anything — while
//! the wire wants exact 20 ms frames. The chunker absorbs that mismatch; it is plain sample
//! arithmetic, kept free of PipeWire so every branch runs under `cargo test`.

/// Accumulates arbitrarily sized sample chunks into fixed-length frames.
///
/// Interleaving is preserved: the chunker deals in raw samples and neither knows nor cares how
/// many channels they carry, as long as `frame_len` is a whole number of frames' worth.
#[derive(Debug)]
pub struct FrameChunker {
    frame_len: usize,
    pending: Vec<i16>,
}

impl FrameChunker {
    /// A chunker emitting frames of exactly `frame_len` samples (all channels included).
    #[must_use]
    pub fn new(frame_len: usize) -> Self {
        Self {
            frame_len: frame_len.max(1),
            pending: Vec::new(),
        }
    }

    /// Feed captured samples, invoking `emit` once per completed frame, in order.
    pub fn push(&mut self, samples: &[i16], mut emit: impl FnMut(Vec<i16>)) {
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= self.frame_len {
            let rest = self.pending.split_off(self.frame_len);
            let frame = std::mem::replace(&mut self.pending, rest);
            emit(frame);
        }
    }

    /// Samples buffered toward the next frame.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Discard any partial frame, e.g. on stream restart.
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(chunker: &mut FrameChunker, samples: &[i16]) -> Vec<Vec<i16>> {
        let mut frames = Vec::new();
        chunker.push(samples, |frame| frames.push(frame));
        frames
    }

    #[test]
    fn an_exact_frame_should_be_emitted_whole() {
        let mut chunker = FrameChunker::new(4);
        let frames = collect(&mut chunker, &[1, 2, 3, 4]);
        assert_eq!(frames, vec![vec![1, 2, 3, 4]]);
        assert_eq!(chunker.pending_len(), 0);
    }

    #[test]
    fn a_short_push_should_emit_nothing_and_buffer() {
        let mut chunker = FrameChunker::new(4);
        assert!(collect(&mut chunker, &[1, 2]).is_empty());
        assert_eq!(chunker.pending_len(), 2);
    }

    #[test]
    fn frames_should_assemble_across_pushes() {
        let mut chunker = FrameChunker::new(4);
        assert!(collect(&mut chunker, &[1, 2]).is_empty());
        let frames = collect(&mut chunker, &[3, 4, 5]);
        assert_eq!(frames, vec![vec![1, 2, 3, 4]]);
        assert_eq!(chunker.pending_len(), 1);
    }

    #[test]
    fn a_large_push_should_emit_every_complete_frame_in_order() {
        let mut chunker = FrameChunker::new(2);
        let frames = collect(&mut chunker, &[1, 2, 3, 4, 5]);
        assert_eq!(frames, vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(chunker.pending_len(), 1);
    }

    #[test]
    fn clear_should_discard_a_partial_frame() {
        let mut chunker = FrameChunker::new(4);
        chunker.push(&[1, 2, 3], |_| {});
        chunker.clear();
        let frames = collect(&mut chunker, &[7, 8, 9, 10]);
        assert_eq!(frames, vec![vec![7, 8, 9, 10]], "old samples must not leak in");
    }

    #[test]
    fn samples_should_never_be_reordered_or_dropped() {
        // Interleaved stereo survives arbitrary chunk boundaries.
        let mut chunker = FrameChunker::new(6);
        let mut received = Vec::new();
        for chunk in [[1_i16, 2].as_slice(), &[3, 4, 5], &[6, 7, 8, 9, 10, 11, 12]] {
            chunker.push(chunk, |frame| received.extend(frame));
        }
        assert_eq!(received, (1..=12).collect::<Vec<i16>>());
    }
}
