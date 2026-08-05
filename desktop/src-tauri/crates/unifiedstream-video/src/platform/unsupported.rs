//! Fallback resolution of the video platform factory for targets with no virtual camera.
//!
//! Deliberately inert rather than absent: the workspace — including the desktop binary —
//! compiles for these targets, and the running application discovers, pairs, connects, and
//! reports telemetry with the camera toggle failing honestly. The failure is visible, so this
//! is a starting point for a real implementation rather than something that can be mistaken
//! for one.

use crate::{SetupHint, VideoError, VideoFormat, VideoSink};

/// A camera sink that refuses to start, naming the platform it has no implementation for.
struct UnsupportedVideoSink;

impl VideoSink for UnsupportedVideoSink {
    fn start(&mut self, _format: VideoFormat) -> Result<(), VideoError> {
        // No command resolves this, so the hint carries the message alone and the UI shows no
        // copyable command.
        Err(VideoError::Unavailable(SetupHint::new(format!(
            "{} has no virtual camera implementation in this build",
            std::env::consts::OS
        ))))
    }

    fn push_frame(&mut self, _jpeg: Vec<u8>) {
        // Unreachable while start refuses, but discarding beats buffering frames nothing reads.
    }

    fn frames_written(&self) -> u64 {
        0
    }

    fn decode_failures(&self) -> u64 {
        0
    }

    fn device_label(&self) -> Option<String> {
        None
    }

    fn stop(&mut self) {}
}

/// A sink that reports the virtual camera as unavailable on this platform.
pub fn video_sink() -> Box<dyn VideoSink> {
    Box::new(UnsupportedVideoSink)
}
