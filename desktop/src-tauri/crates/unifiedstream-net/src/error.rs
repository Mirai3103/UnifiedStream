//! Error types for the networking layer.

use std::net::SocketAddr;

/// Errors produced by discovery, control, and transport.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NetError {
    /// An underlying socket or file operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A control message could not be encoded or decoded.
    #[error("malformed control message: {0}")]
    MalformedControl(String),

    /// The peer speaks a protocol version this build does not support.
    #[error("protocol version mismatch: peer speaks {peer}, we speak {ours}")]
    VersionMismatch {
        /// Version advertised by the peer.
        peer: u8,
        /// Version this build implements.
        ours: u8,
    },

    /// A media datagram could not be parsed.
    #[error("malformed media packet: {0}")]
    MalformedPacket(&'static str),

    /// mDNS registration or browsing failed.
    #[error("discovery error: {0}")]
    Discovery(String),

    /// The operation requires an established session.
    #[error("no active session")]
    NoSession,

    /// A second peer tried to connect while a session was already active.
    #[error("already paired with another device")]
    Busy,

    /// The desktop user declined the pairing request, or it timed out.
    #[error("pairing rejected")]
    PairingRejected,

    /// The peer stopped responding to heartbeats.
    #[error("peer {0} is unreachable")]
    PeerUnreachable(SocketAddr),

    /// A manually entered address could not be parsed.
    #[error("invalid address: {0}")]
    InvalidAddress(String),
}

/// Convenience alias for fallible networking operations.
pub type Result<T> = std::result::Result<T, NetError>;
