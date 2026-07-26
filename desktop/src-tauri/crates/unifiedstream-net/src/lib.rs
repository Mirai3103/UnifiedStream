//! UnifiedStream networking: mDNS discovery, the TCP control channel, and the UDP media
//! transport.
//!
//! This crate deliberately has no Tauri dependency so the wire protocol can be unit-tested
//! without a webview, and reused by a headless client later. See `openspec/specs/protocol.md`
//! for the normative wire format.

#![deny(missing_docs)]

pub mod control;
pub mod discovery;
pub mod error;
pub mod protocol;
pub mod session;
pub mod telemetry;
pub mod transport;

pub use error::{NetError, Result};
pub use protocol::{MediaHeader, PROTOCOL_VERSION, StreamId};
pub use session::ConnectionState;

/// Default TCP port for the control channel.
pub const DEFAULT_CONTROL_PORT: u16 = 47810;

/// Default UDP port for the media transport.
pub const DEFAULT_MEDIA_PORT: u16 = 47811;
