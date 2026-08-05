//! Linux resolution of the video platform factory: the v4l2loopback virtual camera.

use crate::{V4l2LoopbackSink, VideoSink};

/// A v4l2loopback writer, not yet attached to a device.
pub fn video_sink() -> Box<dyn VideoSink> {
    Box::new(V4l2LoopbackSink::new())
}
