//! The one place in this crate where a platform-specific video type is named.
//!
//! Selection is by `cfg(target_os = …)` here and nowhere above: the application layer calls
//! [`video_sink`] and holds the returned `Box<dyn VideoSink>`, so adding a virtual camera for
//! another platform changes this module and its sibling implementation only.
//!
//! Every target compiles. A target with no implementation resolves to [`unsupported`], whose
//! sink refuses the stream with a message naming the platform rather than failing the build.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod unsupported;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(not(target_os = "linux"))]
use unsupported as imp;

use crate::VideoSink;

/// The virtual camera for the platform this binary was built for.
///
/// Returned stopped: the caller starts it with the negotiated geometry, and a platform with no
/// implementation reports that from `start` rather than from here, so the failure carries the
/// format context and reaches the UI through the existing refusal path.
#[must_use]
pub fn video_sink() -> Box<dyn VideoSink> {
    imp::video_sink()
}
