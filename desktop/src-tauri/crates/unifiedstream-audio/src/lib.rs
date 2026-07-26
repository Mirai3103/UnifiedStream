//! UnifiedStream audio integration: jitter buffering, the PipeWire virtual source
//! (microphone), and the PipeWire virtual sink (speaker).
//!
//! Kept separate from `unifiedstream-net` so the wire protocol stays free of system
//! dependencies — this crate is the only place that links libpipewire.

#![deny(missing_docs)]

mod capture;
mod jitter;
#[cfg(target_os = "linux")]
mod pipewire_sink;
#[cfg(target_os = "linux")]
mod pipewire_source;

pub use capture::FrameChunker;
pub use jitter::{JitterBuffer, JitterStats, JITTER_CAP_FRAMES, JITTER_TARGET_FRAMES};
#[cfg(target_os = "linux")]
pub use pipewire_sink::{FrameCallback, PipeWireSpeakerSink, SINK_NODE_ID, SINK_NODE_NAME};
#[cfg(target_os = "linux")]
pub use pipewire_source::PipeWireSource;

/// Sample format crossing the audio boundary: what protocol §5.1/§6.1 carry, decoded to host
/// order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    /// Samples per second.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u8,
    /// Samples per frame *per channel* — 960 for a 20 ms frame at 48 kHz.
    pub frame_samples: usize,
}

impl AudioFormat {
    /// The microphone baseline: 48 kHz mono, 20 ms frames.
    pub const MICROPHONE: Self = Self {
        sample_rate: 48_000,
        channels: 1,
        frame_samples: 960,
    };

    /// The speaker default: 48 kHz stereo, 20 ms frames.
    pub const SPEAKER: Self = Self {
        sample_rate: 48_000,
        channels: 2,
        frame_samples: 960,
    };

    /// Samples per frame across all channels — the length of a decoded wire frame.
    #[must_use]
    pub const fn total_samples(&self) -> usize {
        self.frame_samples * self.channels as usize
    }
}

/// Why an audio sink could not start.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    /// The audio system is unavailable — PipeWire not running, no session daemon.
    #[error("audio system unavailable: {0}")]
    Unavailable(String),
    /// The sink cannot render this format.
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
}

/// Something that turns received audio frames into sound the OS can route.
///
/// One implementation exists today — [`PipeWireSource`] — but the trait is what keeps a future
/// PulseAudio or Windows sink from touching the receive path.
pub trait AudioSink: Send {
    /// Create the output with the given format. Idempotent: starting a started sink is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError`] when the audio system cannot be reached or refuses the format;
    /// the caller reports the stream as refused rather than accepting audio it cannot play.
    fn start(&mut self, format: AudioFormat) -> Result<(), AudioError>;

    /// Queue one decoded frame. Frames arriving while stopped are dropped.
    fn push(&mut self, frame: &[i16]);

    /// Tear the output down. Idempotent.
    fn stop(&mut self);
}

/// Something that captures system audio and emits fixed-duration frames for the wire.
///
/// The mirror of [`AudioSink`]: one implementation exists today — [`PipeWireSpeakerSink`] —
/// and the trait is what keeps a future PulseAudio or Windows capture from touching the send
/// path. Frames are delivered through the callback the implementation was constructed with.
pub trait AudioCapture: Send {
    /// Create the capture with the given format. Idempotent: starting a started capture is a
    /// no-op.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError`] when the audio system cannot be reached or refuses the format;
    /// the caller reports the stream as failed rather than announcing audio it cannot capture.
    fn start(&mut self, format: AudioFormat) -> Result<(), AudioError>;

    /// Tear the capture down. Idempotent.
    fn stop(&mut self);
}
