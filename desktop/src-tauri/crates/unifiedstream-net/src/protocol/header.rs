//! The 16-byte media packet header.
//!
//! Layout is normative in `openspec/specs/protocol.md` §2. All integers are big-endian.
//!
//! ```text
//!  0               1               2               3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |Ver|F|M| Res |   Stream ID   |        Sequence Number          |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                    Timestamp (microseconds)                   |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                                                               |
//! +                     Session ID (64 bits)                      +
//! |                                                               |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```

use crate::error::{NetError, Result};

/// Protocol version implemented by this build.
pub const PROTOCOL_VERSION: u8 = 1;

/// Serialized size of a media header, in bytes.
pub const HEADER_LEN: usize = 16;

/// Maximum payload bytes per datagram. Frames larger than this are fragmented.
///
/// Conservative for Wi-Fi: leaves room for IPv6 and any tunneling without hitting IP-level
/// fragmentation, where a single lost IP fragment silently destroys the whole datagram.
pub const MAX_PAYLOAD: usize = 1200;

/// Logical stream carried over the shared media socket.
///
/// One UDP socket serves camera, microphone, and speaker; this discriminates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId(pub u8);

impl StreamId {
    /// Synthetic verification stream. Carries no real media.
    pub const TEST: Self = Self(0);
    /// Camera video, phone to PC. Reserved for a later change.
    pub const CAMERA: Self = Self(1);
    /// Microphone audio, phone to PC. Reserved for a later change.
    pub const MICROPHONE: Self = Self(2);
    /// System audio, PC to phone. Reserved for a later change.
    pub const SPEAKER: Self = Self(3);

    /// The raw wire value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl std::fmt::Display for StreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::TEST => f.write_str("test"),
            Self::CAMERA => f.write_str("camera"),
            Self::MICROPHONE => f.write_str("microphone"),
            Self::SPEAKER => f.write_str("speaker"),
            Self(other) => write!(f, "stream({other})"),
        }
    }
}

/// Parsed media packet header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaHeader {
    /// Which logical stream this packet belongs to.
    pub stream: StreamId,
    /// Per-stream sequence number. Wraps at 65536.
    pub sequence: u16,
    /// Microseconds since session start, sender's clock. Wraps at 2^32.
    pub timestamp_us: u32,
    /// Session this packet belongs to. Mismatches are dropped.
    pub session_id: u64,
    /// Set when this packet is one fragment of a larger frame.
    pub fragment: bool,
    /// Set on the final packet of a frame.
    pub marker: bool,
}

impl MediaHeader {
    /// Serialize to wire format.
    #[must_use]
    pub const fn encode(&self) -> [u8; HEADER_LEN] {
        let mut flags = PROTOCOL_VERSION << 6;
        if self.fragment {
            flags |= 1 << 5;
        }
        if self.marker {
            flags |= 1 << 4;
        }
        // Bits 3-0 stay zero: reserved for a future version to claim.

        let seq = self.sequence.to_be_bytes();
        let ts = self.timestamp_us.to_be_bytes();
        let sid = self.session_id.to_be_bytes();

        [
            flags,
            self.stream.0,
            seq[0],
            seq[1],
            ts[0],
            ts[1],
            ts[2],
            ts[3],
            sid[0],
            sid[1],
            sid[2],
            sid[3],
            sid[4],
            sid[5],
            sid[6],
            sid[7],
        ]
    }

    /// Parse a header from the front of a datagram.
    ///
    /// Returns [`NetError::MalformedPacket`] for a short buffer or an unsupported version.
    /// This parses untrusted network input, so it must never panic.
    ///
    /// # Errors
    ///
    /// Fails when `buf` is shorter than [`HEADER_LEN`] or the version field is not
    /// [`PROTOCOL_VERSION`].
    pub fn decode(buf: &[u8]) -> Result<Self> {
        let head = buf
            .get(..HEADER_LEN)
            .ok_or(NetError::MalformedPacket("datagram shorter than header"))?;
        let bytes: [u8; HEADER_LEN] = head
            .try_into()
            .map_err(|_| NetError::MalformedPacket("header slice wrong length"))?;

        // Destructured rather than indexed so the parser cannot panic on a bad offset.
        let [flags, stream, s0, s1, t0, t1, t2, t3, i0, i1, i2, i3, i4, i5, i6, i7] = bytes;

        let version = flags >> 6;
        if version != PROTOCOL_VERSION {
            return Err(NetError::MalformedPacket("unsupported protocol version"));
        }

        Ok(Self {
            stream: StreamId(stream),
            sequence: u16::from_be_bytes([s0, s1]),
            timestamp_us: u32::from_be_bytes([t0, t1, t2, t3]),
            session_id: u64::from_be_bytes([i0, i1, i2, i3, i4, i5, i6, i7]),
            fragment: flags & (1 << 5) != 0,
            marker: flags & (1 << 4) != 0,
        })
    }

    /// Parse a datagram into its header and payload.
    ///
    /// # Errors
    ///
    /// Same conditions as [`MediaHeader::decode`].
    pub fn split(buf: &[u8]) -> Result<(Self, &[u8])> {
        let header = Self::decode(buf)?;
        let payload = buf
            .get(HEADER_LEN..)
            .ok_or(NetError::MalformedPacket("datagram shorter than header"))?;
        Ok((header, payload))
    }
}

/// True when sequence `a` is newer than `b` under RFC 1982 serial-number arithmetic.
///
/// This is what keeps a wrap from 65535 to 0 from being read as a 65535-packet loss.
#[must_use]
pub const fn seq_is_newer(a: u16, b: u16) -> bool {
    let delta = a.wrapping_sub(b);
    delta != 0 && delta < 0x8000
}

/// Forward distance from `from` to `to`, accounting for wrap.
///
/// Used to size loss gaps: `seq_distance(last_seen, arrived) - 1` packets went missing.
#[must_use]
pub const fn seq_distance(from: u16, to: u16) -> u16 {
    to.wrapping_sub(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MediaHeader {
        MediaHeader {
            stream: StreamId::CAMERA,
            sequence: 0x1234,
            timestamp_us: 0xDEAD_BEEF,
            session_id: 0x0123_4567_89AB_CDEF,
            fragment: true,
            marker: false,
        }
    }

    #[test]
    fn encode_should_produce_exactly_sixteen_bytes() {
        assert_eq!(sample().encode().len(), HEADER_LEN);
    }

    #[test]
    fn decode_should_recover_every_field_after_encode() {
        let original = sample();
        let decoded = MediaHeader::decode(&original.encode()).expect("round-trip must parse");
        assert_eq!(decoded, original);
    }

    #[test]
    fn round_trip_should_preserve_marker_without_fragment() {
        let original = MediaHeader {
            fragment: false,
            marker: true,
            ..sample()
        };
        let decoded = MediaHeader::decode(&original.encode()).expect("round-trip must parse");
        assert_eq!(decoded, original);
    }

    #[test]
    fn round_trip_should_preserve_extreme_values() {
        let original = MediaHeader {
            stream: StreamId(255),
            sequence: u16::MAX,
            timestamp_us: u32::MAX,
            session_id: u64::MAX,
            fragment: true,
            marker: true,
        };
        let decoded = MediaHeader::decode(&original.encode()).expect("round-trip must parse");
        assert_eq!(decoded, original);
    }

    #[test]
    fn encode_should_write_version_in_top_two_bits() {
        let encoded = sample().encode();
        let first = encoded.first().copied().unwrap_or_default();
        assert_eq!(first >> 6, PROTOCOL_VERSION);
    }

    #[test]
    fn encode_should_leave_reserved_bits_zero() {
        let encoded = sample().encode();
        let first = encoded.first().copied().unwrap_or_default();
        assert_eq!(first & 0x0F, 0);
    }

    #[test]
    fn decode_should_reject_unknown_version() {
        let mut encoded = sample().encode();
        // Version 2 is not implemented by this build.
        if let Some(first) = encoded.first_mut() {
            *first = (*first & 0x3F) | (2 << 6);
        }
        assert!(MediaHeader::decode(&encoded).is_err());
    }

    #[test]
    fn decode_should_reject_buffer_shorter_than_header() {
        let encoded = sample().encode();
        for len in 0..HEADER_LEN {
            let truncated = encoded.get(..len).expect("slice within bounds");
            assert!(
                MediaHeader::decode(truncated).is_err(),
                "{len}-byte datagram must be rejected"
            );
        }
    }

    #[test]
    fn decode_should_ignore_reserved_bits_set_by_a_future_version() {
        let original = sample();
        let mut encoded = original.encode();
        if let Some(first) = encoded.first_mut() {
            *first |= 0x0F;
        }
        let decoded = MediaHeader::decode(&encoded).expect("reserved bits must not fail parsing");
        assert_eq!(decoded, original);
    }

    #[test]
    fn split_should_separate_header_from_payload() {
        let mut datagram = sample().encode().to_vec();
        datagram.extend_from_slice(b"payload");
        let (header, payload) = MediaHeader::split(&datagram).expect("must parse");
        assert_eq!(header, sample());
        assert_eq!(payload, b"payload");
    }

    #[test]
    fn split_should_yield_empty_payload_for_bare_header() {
        let encoded = sample().encode();
        let (_, payload) = MediaHeader::split(&encoded).expect("must parse");
        assert!(payload.is_empty());
    }

    #[test]
    fn session_id_mismatch_is_visible_to_the_caller() {
        // The transport drops on mismatch; the header layer just reports the value faithfully.
        let decoded = MediaHeader::decode(&sample().encode()).expect("must parse");
        assert_ne!(decoded.session_id, 0xFFFF_FFFF_FFFF_FFFF);
    }

    #[test]
    fn seq_is_newer_should_handle_plain_increment() {
        assert!(seq_is_newer(11, 10));
        assert!(!seq_is_newer(10, 11));
    }

    #[test]
    fn seq_is_newer_should_treat_wrap_as_forward_progress() {
        assert!(seq_is_newer(0, 65535));
        assert!(!seq_is_newer(65535, 0));
    }

    #[test]
    fn seq_is_newer_should_be_false_for_equal_values() {
        assert!(!seq_is_newer(42, 42));
    }

    #[test]
    fn seq_distance_should_count_across_wrap() {
        assert_eq!(seq_distance(65535, 0), 1);
        assert_eq!(seq_distance(65534, 2), 4);
        assert_eq!(seq_distance(10, 14), 4);
    }

    /// The parser sees untrusted network input, so no byte sequence may panic it.
    #[test]
    fn decode_should_never_panic_on_arbitrary_input() {
        // Deterministic xorshift; a seeded PRNG keeps failures reproducible without a dev-dep.
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        for len in 0..64_usize {
            for _ in 0..256 {
                let buf: Vec<u8> = (0..len).map(|_| (next() & 0xFF) as u8).collect();
                // Must return, not unwind.
                let _ = MediaHeader::decode(&buf);
                let _ = MediaHeader::split(&buf);
            }
        }
    }

    #[test]
    fn decode_should_accept_every_valid_flag_combination() {
        for fragment in [false, true] {
            for marker in [false, true] {
                let original = MediaHeader {
                    fragment,
                    marker,
                    ..sample()
                };
                let decoded =
                    MediaHeader::decode(&original.encode()).expect("valid flags must parse");
                assert_eq!(decoded, original, "fragment={fragment} marker={marker}");
            }
        }
    }
}
