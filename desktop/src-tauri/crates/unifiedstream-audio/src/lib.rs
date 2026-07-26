//! UnifiedStream audio output: the jitter buffer and the PipeWire virtual source.
//!
//! Kept separate from `unifiedstream-net` so the wire protocol stays free of system
//! dependencies — this crate is the only place that links libpipewire.

#![deny(missing_docs)]

mod jitter;
#[cfg(target_os = "linux")]
mod pipewire_source;

pub use jitter::{JitterBuffer, JitterStats, JITTER_CAP_FRAMES, JITTER_TARGET_FRAMES};
#[cfg(target_os = "linux")]
pub use pipewire_source::PipeWireSource;

/// Sample format the sink consumes: what protocol §5.1 carries, decoded to host order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    /// Samples per second.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u8,
    /// Samples per frame per channel — 960 for the microphone's 20 ms at 48 kHz.
    pub frame_samples: usize,
}

impl AudioFormat {
    /// The microphone baseline: 48 kHz mono, 20 ms frames.
    pub const MICROPHONE: Self = Self {
        sample_rate: 48_000,
        channels: 1,
        frame_samples: 960,
    };
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
