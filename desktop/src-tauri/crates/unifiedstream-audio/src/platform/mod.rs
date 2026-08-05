//! The one place in this crate where a platform-specific audio type is named.
//!
//! Selection is by `cfg(target_os = …)` here and nowhere above. The factories take only
//! portable arguments — a jitter buffer, a frame callback, a config directory — because
//! construction is where the coupling would otherwise hide: a trait describes a lifecycle, but
//! the application layer still has to build the thing, and it must do that without naming a
//! concrete constructor.
//!
//! Every target compiles. A target with no implementation resolves to [`unsupported`], whose
//! sink and capture refuse to start with a message naming the platform, and whose
//! [`audio_routing`] is `None` — absent, not present and broken.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod unsupported;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(not(target_os = "linux"))]
use unsupported as imp;

use std::path::Path;
use std::sync::Arc;

use crate::{AudioCapture, AudioRouting, AudioSink, FrameCallback, JitterBuffer};

/// The virtual microphone for the platform this binary was built for.
///
/// Returned stopped, draining `buffer` once started: the receive path pushes decoded frames in
/// while the implementation pulls them out on its own thread.
#[must_use]
pub fn audio_sink(buffer: Arc<JitterBuffer>) -> Box<dyn AudioSink> {
    imp::audio_sink(buffer)
}

/// The system audio capture for the platform this binary was built for.
///
/// Returned stopped, delivering complete frames to `on_frame` from its own thread once started.
#[must_use]
pub fn audio_capture(on_frame: FrameCallback) -> Box<dyn AudioCapture> {
    imp::audio_capture(on_frame)
}

/// Default-output routing, where this platform's audio system requires it to capture system
/// audio, and `None` where it does not.
///
/// `None` is the interesting case and is not a failure: a platform that captures the existing
/// output device directly has no routing concept, so the desktop offers no control rather than
/// one that always fails. `config_dir` is where an implementation that needs to remember the
/// user's previous device keeps its memo.
#[must_use]
pub fn audio_routing(config_dir: &Path) -> Option<Box<dyn AudioRouting>> {
    imp::audio_routing(config_dir)
}
