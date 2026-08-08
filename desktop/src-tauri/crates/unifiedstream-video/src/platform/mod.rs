//! The one place in this crate where a platform-specific video type is named.
//!
//! Selection is by `cfg(target_os = …)` here and nowhere above: the application layer calls
//! [`video_sink`] and holds the returned `Box<dyn VideoSink>`, so adding a virtual camera for
//! another platform changes this module and its sibling implementation only.
//!
//! Every target compiles. A target with no implementation resolves to [`unsupported`], whose
//! sink refuses the stream with a message naming the platform rather than failing the build.
//!
//! Support is per integration rather than per platform, and this is the table:
//!
//! ```text
//!               video_sink
//!   linux       linux::…         a v4l2loopback device the kernel presents
//!   windows     windows::…       a shared-memory ring a DirectShow filter reads
//!   other       unsupported::…   refuses, naming the platform
//! ```
//!
//! One integration is a short table, and it is here anyway rather than left implicit — the audio
//! crate keeps one for three factories, and a reader looking for "what does this platform do about
//! the camera" should find the same shape of answer in both crates.

#[cfg(target_os = "linux")]
mod linux;
// Compiled on every target, not only where nothing is implemented. Keeping it out of the platforms
// that have an implementation would mean the fallback is only ever type-checked on the targets
// nobody builds for, and it is the thing a new platform starts from. That leaves it unreferenced
// wherever a real implementation exists — hence an allowance carrying its reason here, rather than
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

use crate::VideoSink;

/// The named shared-memory section the Windows virtual camera publishes frames through.
///
/// Exported for the manual smoke test, which is the only way to exercise the producer path by hand
/// until a filter exists to read it — the same reason `LoopbackDevice` is public on Linux.
#[cfg(target_os = "windows")]
pub use windows::CameraSection;

/// The virtual camera for the platform this binary was built for.
///
/// Returned stopped: the caller starts it with the negotiated geometry, and a platform that cannot
/// present a camera reports that from `start` rather than from here, so the failure carries the
/// format context and reaches the UI through the existing refusal path.
#[must_use]
pub fn video_sink() -> Box<dyn VideoSink> {
    imp::video_sink()
}
