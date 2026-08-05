//! Fallback resolution of the audio platform factories for targets with no audio integration.
//!
//! Deliberately inert rather than absent, for the same reason as the video fallback: the
//! workspace — including the desktop binary — compiles for these targets, and the running
//! application discovers, pairs, connects, and reports telemetry with the microphone and
//! speaker toggles failing honestly rather than appearing to succeed.

use std::path::Path;
use std::sync::Arc;

use crate::{
    AudioCapture, AudioError, AudioFormat, AudioRouting, AudioSink, FrameCallback, JitterBuffer,
};

/// The message both implementations refuse with, naming the platform so a build run outside CI
/// is unambiguous about why nothing works.
fn unavailable(what: &str) -> AudioError {
    AudioError::Unavailable(format!(
        "{} has no {what} implementation in this build",
        std::env::consts::OS
    ))
}

/// A microphone sink that refuses to start.
struct UnsupportedAudioSink;

impl AudioSink for UnsupportedAudioSink {
    fn start(&mut self, _format: AudioFormat) -> Result<(), AudioError> {
        Err(unavailable("virtual microphone"))
    }

    fn push(&mut self, _frame: &[i16]) {
        // Unreachable while start refuses; discarding beats buffering frames nothing plays.
    }

    fn stop(&mut self) {}
}

/// A system audio capture that refuses to start.
///
/// Holds the callback so that the same construction path runs on every platform — a callback
/// dropped here rather than at a different point would change when the desktop's frame channel
/// closes.
struct UnsupportedAudioCapture {
    _on_frame: FrameCallback,
}

impl AudioCapture for UnsupportedAudioCapture {
    fn start(&mut self, _format: AudioFormat) -> Result<(), AudioError> {
        Err(unavailable("system audio capture"))
    }

    fn stop(&mut self) {}
}

/// A sink that reports the virtual microphone as unavailable on this platform.
pub fn audio_sink(_buffer: Arc<JitterBuffer>) -> Box<dyn AudioSink> {
    Box::new(UnsupportedAudioSink)
}

/// A capture that reports system audio capture as unavailable on this platform.
pub fn audio_capture(on_frame: FrameCallback) -> Box<dyn AudioCapture> {
    Box::new(UnsupportedAudioCapture {
        _on_frame: on_frame,
    })
}

/// Absent, not unsupported: a platform reaching this fallback has made no claim either way about
/// needing to take over the default output, and reporting `Some` here would put a control in the
/// UI that could only fail.
pub fn audio_routing(_config_dir: &Path) -> Option<Box<dyn AudioRouting>> {
    None
}
