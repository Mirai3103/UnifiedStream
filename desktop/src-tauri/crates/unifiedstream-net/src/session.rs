//! Connection lifecycle: the state machine both apps render from.
//!
//! The UI never infers status from socket state — it reads this enum. That is what keeps the
//! two platforms showing the same thing for the same condition.

use serde::{Deserialize, Serialize};

/// Why a connection attempt ended in [`ConnectionState::Failed`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
#[non_exhaustive]
pub enum FailureReason {
    /// The control connection could not be opened.
    Unreachable(String),
    /// The peer speaks a protocol version this build does not support.
    VersionMismatch(String),
    /// The desktop user declined pairing, or the prompt timed out.
    Rejected,
    /// The desktop already has an active session with another phone.
    Busy,
    /// Every reconnection attempt failed.
    ReconnectExhausted,
    /// Anything else, carrying its display text.
    Other(String),
}

impl std::fmt::Display for FailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(detail) => write!(f, "could not reach device: {detail}"),
            Self::VersionMismatch(detail) => write!(f, "incompatible version: {detail}"),
            Self::Rejected => f.write_str("pairing was declined"),
            Self::Busy => f.write_str("device is already paired with another phone"),
            Self::ReconnectExhausted => f.write_str("could not reconnect"),
            Self::Other(detail) => f.write_str(detail),
        }
    }
}

/// Lifecycle of a connection to a peer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConnectionState {
    /// Nothing in progress.
    #[default]
    Idle,
    /// Browsing or advertising, no peer selected.
    Discovering,
    /// Control connection opening, or handshake in flight.
    Connecting {
        /// Name of the peer being connected to, for display.
        peer_name: String,
    },
    /// Session established; media may flow.
    Connected {
        /// Name of the connected peer.
        peer_name: String,
        /// Session identifier stamped into every media packet.
        ///
        /// Carried as a decimal string so the webview, whose numbers are IEEE doubles, does
        /// not silently round it.
        #[serde(with = "crate::protocol::u64_string")]
        session_id: u64,
    },
    /// Session dropped; retrying with backoff.
    Reconnecting {
        /// Name of the peer being retried.
        peer_name: String,
        /// 1-based index of the attempt in flight.
        attempt: u32,
        /// Total attempts that will be made before giving up.
        max_attempts: u32,
    },
    /// Gave up. Carries what went wrong so the UI can show it.
    Failed {
        /// Why the connection ended.
        reason: FailureReason,
    },
}

impl ConnectionState {
    /// Whether media may be sent in this state.
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    /// Whether a connection attempt is in flight.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        matches!(self, Self::Connecting { .. } | Self::Reconnecting { .. })
    }

    /// The active session identifier, if any.
    #[must_use]
    pub const fn session_id(&self) -> Option<u64> {
        match self {
            Self::Connected { session_id, .. } => Some(*session_id),
            _ => None,
        }
    }

    /// Peer name to display, if a peer is known in this state.
    #[must_use]
    pub fn peer_name(&self) -> Option<&str> {
        match self {
            Self::Connecting { peer_name }
            | Self::Connected { peer_name, .. }
            | Self::Reconnecting { peer_name, .. } => Some(peer_name),
            Self::Idle | Self::Discovering | Self::Failed { .. } => None,
        }
    }
}

/// Reconnection backoff schedule, in milliseconds.
///
/// Five attempts. Beyond this, a phone that has genuinely left the network is just burning
/// battery, so we stop and let the user decide.
pub const RECONNECT_BACKOFF_MS: [u64; 5] = [500, 1_000, 2_000, 4_000, 8_000];

/// Number of reconnection attempts before giving up.
pub const MAX_RECONNECT_ATTEMPTS: u32 = RECONNECT_BACKOFF_MS.len() as u32;

/// Backoff delay before a given 1-based attempt, in milliseconds.
///
/// Returns `None` once the attempts are exhausted.
#[must_use]
pub fn backoff_for_attempt(attempt: u32) -> Option<u64> {
    let index = usize::try_from(attempt.checked_sub(1)?).ok()?;
    RECONNECT_BACKOFF_MS.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_should_be_idle() {
        assert_eq!(ConnectionState::default(), ConnectionState::Idle);
    }

    #[test]
    fn is_connected_should_be_true_only_when_connected() {
        let connected = ConnectionState::Connected {
            peer_name: "cachy-desktop".to_owned(),
            session_id: 7,
        };
        assert!(connected.is_connected());
        assert!(!ConnectionState::Idle.is_connected());
        assert!(!ConnectionState::Discovering.is_connected());
    }

    #[test]
    fn session_id_should_be_available_only_while_connected() {
        let connected = ConnectionState::Connected {
            peer_name: "cachy-desktop".to_owned(),
            session_id: 42,
        };
        assert_eq!(connected.session_id(), Some(42));
        assert_eq!(ConnectionState::Idle.session_id(), None);
    }

    #[test]
    fn peer_name_should_be_exposed_while_reconnecting() {
        let reconnecting = ConnectionState::Reconnecting {
            peer_name: "cachy-desktop".to_owned(),
            attempt: 2,
            max_attempts: MAX_RECONNECT_ATTEMPTS,
        };
        assert_eq!(reconnecting.peer_name(), Some("cachy-desktop"));
    }

    #[test]
    fn peer_name_should_be_absent_when_failed() {
        let failed = ConnectionState::Failed {
            reason: FailureReason::Busy,
        };
        assert_eq!(failed.peer_name(), None);
    }

    #[test]
    fn backoff_should_follow_the_documented_schedule() {
        assert_eq!(backoff_for_attempt(1), Some(500));
        assert_eq!(backoff_for_attempt(2), Some(1_000));
        assert_eq!(backoff_for_attempt(3), Some(2_000));
        assert_eq!(backoff_for_attempt(4), Some(4_000));
        assert_eq!(backoff_for_attempt(5), Some(8_000));
    }

    #[test]
    fn backoff_should_be_exhausted_past_the_last_attempt() {
        assert_eq!(backoff_for_attempt(MAX_RECONNECT_ATTEMPTS + 1), None);
    }

    #[test]
    fn backoff_should_reject_attempt_zero() {
        assert_eq!(backoff_for_attempt(0), None);
    }

    #[test]
    fn state_should_round_trip_through_json() {
        let original = ConnectionState::Connected {
            peer_name: "cachy-desktop".to_owned(),
            session_id: 0x0123_4567_89AB_CDEF,
        };
        let json = serde_json::to_string(&original).expect("encode");
        let parsed: ConnectionState = serde_json::from_str(&json).expect("decode");
        assert_eq!(parsed, original);
    }

    #[test]
    fn failure_reason_should_render_a_human_readable_message() {
        assert_eq!(
            FailureReason::Busy.to_string(),
            "device is already paired with another phone"
        );
    }
}
