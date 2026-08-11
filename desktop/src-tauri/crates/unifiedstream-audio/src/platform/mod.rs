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
//!
//! Support is per integration rather than per platform, and this is the table:
//!
//! ```text
//!               audio_capture      audio_sink        audio_routing
//!   linux       linux::…           linux::…          Some(linux::…)
//!   windows     windows::…         windows::…        None
//!   other       unsupported::…     unsupported::…    None
//! ```
//!
//! A partially supported platform names its own gaps in its own module — a module re-exports
//! whatever it has not written yet — so the table above stays in one place and each gap is
//! explicit at the point where a later phase closes it. Gating each factory's body separately here
//! would put three independent tables in the module that exists to have one.
//!
//! Windows' `None` in the last column is not a gap: capturing system audio there means reading the
//! endpoint the user already chose, so there is no default output to take over and the capability
//! does not exist rather than existing and failing.

#[cfg(target_os = "linux")]
mod linux;
// Compiled on every target, not only where nothing is implemented, because a platform with
// partial support borrows from it. That leaves the rest of the module unreferenced wherever a
// real implementation exists — hence an allowance carrying its reason here, rather than
// silencing the lint across the workspace.
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
use unsupported as imp;
#[cfg(target_os = "windows")]
use windows as imp;

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
