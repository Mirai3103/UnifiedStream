//! The receive-side jitter buffer between the network and the audio clock.
//!
//! Two clocks feed this buffer: the phone's capture clock pushes 20 ms frames, and PipeWire's
//! graph clock pulls whatever the quantum asks for. Neither waits for the other, so the buffer
//! absorbs jitter with two hard rules: an empty buffer yields silence (never a stall), and a
//! full buffer drops its oldest frame (never unbounded latency).

use std::collections::VecDeque;
use std::sync::Mutex;

/// Steady-state depth the buffer aims for, in frames. Three 20 ms frames is 60 ms — enough to
/// ride out Wi-Fi jitter without pushing a call past noticeable latency.
pub const JITTER_TARGET_FRAMES: usize = 3;

/// Hard depth cap, in frames. Beyond this the oldest audio is dropped: 120 ms of buffered
/// audio is already a latency problem, and stale speech is worth less than fresh speech.
pub const JITTER_CAP_FRAMES: usize = 6;

/// Counters exposed for diagnostics and the E2E latency notes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JitterStats {
    /// Frames accepted from the network.
    pub pushed: u64,
    /// Frames discarded because the buffer was full.
    pub overruns: u64,
    /// Pulls that found the buffer empty and emitted silence.
    pub underruns: u64,
}

struct Inner {
    frames: VecDeque<Vec<i16>>,
    /// Read offset into the front frame, in samples: the audio clock's quantum rarely aligns
    /// with frame boundaries, so a frame is consumed across several pulls.
    front_offset: usize,
    stats: JitterStats,
}

/// Bounded frame queue: lossy on both ends, safe to share between the network task and the
/// audio callback thread.
pub struct JitterBuffer {
    inner: Mutex<Inner>,
    cap: usize,
}

impl Default for JitterBuffer {
    fn default() -> Self {
        Self::new(JITTER_CAP_FRAMES)
    }
}

impl JitterBuffer {
    /// A buffer holding at most `cap` frames.
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                frames: VecDeque::with_capacity(cap),
                front_offset: 0,
                stats: JitterStats::default(),
            }),
            cap: cap.max(1),
        }
    }

    /// Queue one decoded frame, dropping the oldest when full.
    pub fn push(&self, frame: Vec<i16>) {
        if frame.is_empty() {
            return;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return; // a poisoned lock means the audio thread panicked; nothing left to feed
        };
        inner.stats.pushed += 1;
        if inner.frames.len() >= self.cap {
            inner.stats.overruns += 1;
            inner.frames.pop_front();
            inner.front_offset = 0;
        }
        inner.frames.push_back(frame);
    }

    /// Fill `out` with queued samples, zero-filling whatever the queue cannot cover.
    ///
    /// Never blocks on the network: this is what the audio callback calls, and a stalled
    /// callback would stall the whole PipeWire graph.
    pub fn pop_into(&self, out: &mut [i16]) {
        let Ok(mut inner) = self.inner.lock() else {
            out.fill(0);
            return;
        };

        let mut filled = 0;
        while filled < out.len() {
            let offset = inner.front_offset;
            let Some(front) = inner.frames.front() else {
                break;
            };
            let available = front.len() - offset;
            let needed = out.len() - filled;
            let take = available.min(needed);

            if let (Some(dst), Some(src)) = (
                out.get_mut(filled..filled + take),
                front.get(offset..offset + take),
            ) {
                dst.copy_from_slice(src);
            }
            filled += take;

            if take == available {
                inner.frames.pop_front();
                inner.front_offset = 0;
            } else {
                inner.front_offset = offset + take;
            }
        }

        if filled < out.len() {
            if let Some(rest) = out.get_mut(filled..) {
                rest.fill(0);
            }
            // Partial fills count too: they are the audible edge of an underrun.
            inner.stats.underruns += 1;
        }
    }

    /// Frames currently queued.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.inner.lock().map(|i| i.frames.len()).unwrap_or(0)
    }

    /// Counters since construction or the last [`JitterBuffer::clear`].
    #[must_use]
    pub fn stats(&self) -> JitterStats {
        self.inner.lock().map(|i| i.stats).unwrap_or_default()
    }

    /// Drop all queued audio and reset the counters, e.g. on stream restart.
    pub fn clear(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.frames.clear();
            inner.front_offset = 0;
            inner.stats = JitterStats::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(value: i16, len: usize) -> Vec<i16> {
        vec![value; len]
    }

    #[test]
    fn pop_should_return_pushed_samples_in_order() {
        let buffer = JitterBuffer::default();
        buffer.push(vec![1, 2, 3]);
        buffer.push(vec![4, 5, 6]);

        let mut out = [0_i16; 6];
        buffer.pop_into(&mut out);

        assert_eq!(out, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn an_empty_buffer_should_yield_silence_not_stall() {
        let buffer = JitterBuffer::default();
        let mut out = [7_i16; 4];
        buffer.pop_into(&mut out);

        assert_eq!(out, [0, 0, 0, 0]);
        assert_eq!(buffer.stats().underruns, 1);
    }

    #[test]
    fn a_partial_fill_should_zero_the_remainder_and_count_an_underrun() {
        let buffer = JitterBuffer::default();
        buffer.push(vec![9, 9]);

        let mut out = [1_i16; 4];
        buffer.pop_into(&mut out);

        assert_eq!(out, [9, 9, 0, 0]);
        assert_eq!(buffer.stats().underruns, 1);
    }

    #[test]
    fn a_pull_smaller_than_a_frame_should_resume_mid_frame() {
        // PipeWire's quantum rarely equals 960 samples, so partial consumption is the norm.
        let buffer = JitterBuffer::default();
        buffer.push(vec![1, 2, 3, 4, 5]);

        let mut first = [0_i16; 2];
        buffer.pop_into(&mut first);
        let mut second = [0_i16; 3];
        buffer.pop_into(&mut second);

        assert_eq!(first, [1, 2]);
        assert_eq!(second, [3, 4, 5]);
    }

    #[test]
    fn a_full_buffer_should_drop_the_oldest_frame() {
        let buffer = JitterBuffer::new(2);
        buffer.push(frame(1, 4));
        buffer.push(frame(2, 4));
        buffer.push(frame(3, 4)); // evicts the frame of 1s

        let mut out = [0_i16; 8];
        buffer.pop_into(&mut out);

        assert_eq!(out, [2, 2, 2, 2, 3, 3, 3, 3]);
        assert_eq!(buffer.stats().overruns, 1);
    }

    #[test]
    fn depth_should_never_exceed_the_cap() {
        let buffer = JitterBuffer::new(JITTER_CAP_FRAMES);
        for _ in 0..50 {
            buffer.push(frame(1, 960));
        }
        assert_eq!(buffer.depth(), JITTER_CAP_FRAMES);
    }

    #[test]
    fn playback_should_resume_after_an_underrun() {
        let buffer = JitterBuffer::default();
        let mut out = [5_i16; 4];
        buffer.pop_into(&mut out); // underrun

        buffer.push(vec![8, 8, 8, 8]);
        buffer.pop_into(&mut out);

        assert_eq!(out, [8, 8, 8, 8]);
    }

    #[test]
    fn an_eviction_mid_frame_should_not_leave_a_stale_read_offset() {
        let buffer = JitterBuffer::new(2);
        buffer.push(vec![1, 2, 3, 4]);
        buffer.push(vec![5, 6, 7, 8]);

        // Consume half of the front frame, so front_offset is non-zero.
        let mut half = [0_i16; 2];
        buffer.pop_into(&mut half);
        assert_eq!(half, [1, 2]);

        // This push evicts the half-read front frame; the offset must reset with it.
        buffer.push(vec![9, 10, 11, 12]);

        let mut out = [0_i16; 8];
        buffer.pop_into(&mut out);
        assert_eq!(out, [5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn an_empty_frame_should_be_ignored() {
        let buffer = JitterBuffer::default();
        buffer.push(Vec::new());
        assert_eq!(buffer.depth(), 0);
        assert_eq!(buffer.stats().pushed, 0);
    }

    #[test]
    fn clear_should_drop_audio_and_reset_counters() {
        let buffer = JitterBuffer::default();
        buffer.push(vec![1, 2]);
        let mut out = [0_i16; 8];
        buffer.pop_into(&mut out);

        buffer.clear();

        assert_eq!(buffer.depth(), 0);
        assert_eq!(buffer.stats(), JitterStats::default());
    }

    #[test]
    fn stats_should_count_pushes() {
        let buffer = JitterBuffer::default();
        buffer.push(vec![1]);
        buffer.push(vec![2]);
        assert_eq!(buffer.stats().pushed, 2);
    }
}
