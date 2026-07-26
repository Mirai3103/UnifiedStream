//! Wire protocol types shared by both peers.
//!
//! Normative reference: `openspec/specs/protocol.md`. The Kotlin implementation under
//! `com.laffy.unifiedstream.protocol` mirrors this module and is checked against the same
//! byte fixtures in `testdata/`.

mod header;
mod messages;

pub use header::{
    seq_distance, seq_is_newer, MediaHeader, StreamId, HEADER_LEN, MAX_PAYLOAD, PROTOCOL_VERSION,
};
pub use messages::{
    caps, intersect_caps, AudioCodec, AudioParams, ControlMessage, ErrorMessage, ErrorReason,
    Hello, HelloAck, StreamParams, StreamRefusal, TelemetryReport, VideoCodec, VideoParams,
};
pub(crate) use messages::u64_string;

/// mDNS service type the desktop advertises and the phone browses.
pub const SERVICE_TYPE: &str = "_unifiedstream._udp.local.";

/// TXT record keys carried in the service advertisement.
pub mod txt_keys {
    /// Protocol version, decimal.
    pub const VERSION: &str = "ver";
    /// Human-readable device name.
    pub const NAME: &str = "name";
    /// Stable device UUID, used to deduplicate multi-interface advertisements.
    pub const ID: &str = "id";
    /// Comma-separated capability tokens.
    pub const CAPS: &str = "caps";
}
