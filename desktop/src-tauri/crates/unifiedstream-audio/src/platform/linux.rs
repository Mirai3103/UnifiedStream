//! Linux resolution of the audio platform factories: the PipeWire virtual source and sink, and
//! `pactl` default-output routing.

use std::path::Path;
use std::sync::Arc;

use crate::routing::PactlRouting;
use crate::{
    AudioCapture, AudioRouting, AudioSink, FrameCallback, JitterBuffer, PipeWireSource,
    PipeWireSpeakerSink,
};

/// A PipeWire `Audio/Source` node draining the given buffer.
pub fn audio_sink(buffer: Arc<JitterBuffer>) -> Box<dyn AudioSink> {
    Box::new(PipeWireSource::new(buffer))
}

/// A PipeWire `Audio/Sink` node delivering captured frames to the callback.
pub fn audio_capture(on_frame: FrameCallback) -> Box<dyn AudioCapture> {
    Box::new(PipeWireSpeakerSink::new(on_frame))
}

/// Always present on this platform: capturing system audio here means becoming the default
/// output, so the control is real and the user must be offered it.
pub fn audio_routing(config_dir: &Path) -> Option<Box<dyn AudioRouting>> {
    Some(Box::new(PactlRouting::new(config_dir)))
}
