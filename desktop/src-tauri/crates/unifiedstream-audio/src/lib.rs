//! UnifiedStream audio integration: jitter buffering, the PipeWire virtual source
//! (microphone), and the PipeWire virtual sink (speaker).
//!
//! Kept separate from `unifiedstream-net` so the wire protocol stays free of system
//! dependencies — this crate is the only place that links libpipewire.

#![deny(missing_docs)]

mod capture;
mod convert;
mod jitter;
#[cfg(target_os = "linux")]
mod pipewire_sink;
#[cfg(target_os = "linux")]
mod pipewire_source;
pub mod platform;
mod render_convert;
#[cfg(target_os = "linux")]
mod routing;
mod sample;

pub use capture::FrameChunker;
pub use convert::{AudioConverter, SourceFormat};
pub use jitter::{JitterBuffer, JitterStats, JITTER_CAP_FRAMES, JITTER_TARGET_FRAMES};
#[cfg(target_os = "linux")]
pub use pipewire_sink::{PipeWireSpeakerSink, SINK_NODE_ID, SINK_NODE_NAME};
#[cfg(target_os = "linux")]
pub use pipewire_source::PipeWireSource;
pub use render_convert::{EndpointFormat, RenderConverter};
pub use sample::SampleType;

/// Called from the capture implementation's own thread with each complete frame of samples.
///
/// Portable, and defined here rather than beside an implementation, so the application layer
/// can build the callback that [`platform::audio_capture`] takes without naming a platform
/// type.
pub type FrameCallback = Box<dyn FnMut(Vec<i16>) + Send>;

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
/// The whole surface the application layer needs: it holds a `Box<dyn AudioSink>` obtained from
/// [`platform::audio_sink`] and never names a platform type, so a PulseAudio or Windows sink is
/// added in [`platform`] alone.
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
/// The mirror of [`AudioSink`], obtained from [`platform::audio_capture`]. Frames are delivered
/// through the [`FrameCallback`] the implementation was constructed with, so the application
/// layer never names the concrete constructor.
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

/// A future returned by an [`AudioRouting`] method.
///
/// Spelled out rather than written as `async fn` because the application layer holds an
/// `Option<Box<dyn AudioRouting>>`, and a trait with `async fn` is not object-safe.
pub type RoutingFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Taking over the system default output so that system audio can be captured.
///
/// Optional by design, and the reason [`platform::audio_routing`] returns an `Option`. An audio
/// system that lets a process capture the existing output device directly has nothing to route,
/// nothing to remember, and nothing to repair — the capability does not exist there rather than
/// existing and failing, so the desktop offers no control at all.
///
/// The trait covers the whole mechanism, not just the switch: the memo that survives an unclean
/// exit and the startup sweep that repairs one are as platform-specific as the switch itself.
pub trait AudioRouting: Send + Sync {
    /// Make this platform's virtual sink the system default output, remembering the device that
    /// was selected first so [`restore`](Self::restore) can put it back.
    ///
    /// The memo is persisted *before* the switch: a process that dies while routed must still
    /// leave the next launch able to give the user their device back.
    ///
    /// Returns a user-facing message on failure, which the caller surfaces as a state error.
    fn enable(&self) -> RoutingFuture<'_, Result<(), String>>;

    /// Put the remembered default output back and forget the memo. Idempotent, and safe to call
    /// when routing was never enabled.
    fn restore(&self) -> RoutingFuture<'_, ()>;

    /// Repair a routing takeover left behind by an unclean exit, at startup.
    fn sweep_stale(&self) -> RoutingFuture<'_, ()>;
}
