//! UnifiedStream video integration: MJPEG decoding and the v4l2loopback virtual camera.
//!
//! Kept separate from `unifiedstream-net` so the wire protocol stays free of system
//! dependencies — this crate is the only place that touches v4l2. The decode path is pure
//! Rust and platform-independent; only the loopback device module is Linux-only.

#![deny(missing_docs)]

mod decode;
#[cfg(target_os = "linux")]
mod v4l2;

pub use decode::decode_jpeg_to_i420;
#[cfg(target_os = "linux")]
pub use v4l2::{find_loopback_device, LoopbackDevice, V4l2LoopbackSink};

/// Card label the setup guidance asks the user to give the loopback device, and the label the
/// discovery pass prefers.
pub const CAMERA_NODE_LABEL: &str = "UnifiedStream Camera";

/// The exact command shown to the user when no v4l2loopback device exists.
///
/// `exclusive_caps=1` keeps the idle device out of application camera pickers until a writer
/// attaches, so users do not select a black camera.
pub const MODPROBE_HINT: &str =
    "sudo modprobe v4l2loopback card_label=\"UnifiedStream Camera\" exclusive_caps=1";

/// Frame geometry crossing the video boundary: what protocol §7.1 negotiates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoFormat {
    /// Frame width in pixels. Must be even: the output is 4:2:0 subsampled.
    pub width: u32,
    /// Frame height in pixels. Must be even, as for the width.
    pub height: u32,
    /// Upper bound on the source's frame rate.
    pub max_fps: u32,
}

impl VideoFormat {
    /// The camera default: 1280x720 at up to 30 fps.
    pub const CAMERA_720P: Self = Self {
        width: 1280,
        height: 720,
        max_fps: 30,
    };

    /// Bytes in one I420 (planar YUV 4:2:0) frame at this geometry.
    #[must_use]
    pub const fn i420_frame_bytes(&self) -> usize {
        let pixels = self.width as usize * self.height as usize;
        pixels + pixels / 2
    }
}

/// Why a video sink could not start, or a frame could not be presented.
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    /// No usable loopback device — the kernel module is missing or every device is taken.
    /// The message carries the user-facing guidance, including [`MODPROBE_HINT`].
    #[error("virtual camera unavailable: {0}")]
    Unavailable(String),
    /// The device refused this geometry, or the format is out of contract (odd dimensions).
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    /// A frame that is not a decodable JPEG at the negotiated dimensions.
    #[error("frame decode failed: {0}")]
    Decode(String),
}

/// Something that turns received camera frames into a webcam the OS can offer to applications.
///
/// One implementation exists today — [`V4l2LoopbackSink`] — but the trait is what keeps a
/// future Windows virtual camera from touching the receive path, mirroring `AudioSink` in
/// `unifiedstream-audio`.
pub trait VideoSink: Send {
    /// Create the output at the given geometry. Idempotent: starting a started sink is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`VideoError`] when no loopback device is usable or it refuses the format; the
    /// caller reports the stream as refused rather than accepting video it cannot present.
    fn start(&mut self, format: VideoFormat) -> Result<(), VideoError>;

    /// Queue one received JPEG frame for decode and presentation.
    ///
    /// Never blocks: a stalled device drops frames instead of stalling the receive path. An
    /// undecodable frame is dropped and counted, never fatal. Frames arriving while stopped
    /// are dropped.
    fn push_frame(&mut self, jpeg: Vec<u8>);

    /// Frames actually decoded and written to the device since `start`.
    ///
    /// The UI derives its delivered-fps figure from this advancing, so a wedged device or a
    /// stream of undecodable frames reads as 0 fps rather than as a healthy stream.
    fn frames_written(&self) -> u64;

    /// Tear the output down. Idempotent.
    fn stop(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i420_frame_bytes_should_be_one_and_a_half_bytes_per_pixel() {
        assert_eq!(VideoFormat::CAMERA_720P.i420_frame_bytes(), 1280 * 720 * 3 / 2);
        let vga = VideoFormat {
            width: 640,
            height: 480,
            max_fps: 30,
        };
        assert_eq!(vga.i420_frame_bytes(), 460_800);
    }
}
