//! Send-side framing: per-stream sequencing, timestamping, and fragmentation.

use std::collections::HashMap;
use std::time::Instant;

use crate::protocol::{MediaHeader, StreamId, HEADER_LEN, MAX_PAYLOAD};

/// One datagram ready to go on the wire.
pub type Datagram = Vec<u8>;

/// Builds outbound datagrams for a session.
///
/// Fragmentation is ours rather than IP's: a single lost IP fragment silently destroys the
/// whole datagram, and consumer APs handle fragmented UDP poorly.
#[derive(Debug)]
pub struct MediaSender {
    session_id: u64,
    started: Instant,
    sequences: HashMap<u8, u16>,
}

impl MediaSender {
    /// A sender for `session_id`, timestamping from now.
    #[must_use]
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            started: Instant::now(),
            sequences: HashMap::new(),
        }
    }

    /// The session this sender stamps into every packet.
    #[must_use]
    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    /// Microseconds since this sender started, wrapping at 2^32 as the wire format requires.
    #[must_use]
    pub fn timestamp_now(&self) -> u32 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "wrapping at 2^32 is the wire format"
        )]
        let micros = self.started.elapsed().as_micros() as u32;
        micros
    }

    /// Next sequence number for a stream, advancing its counter.
    fn next_sequence(&mut self, stream: StreamId, count: u16) -> u16 {
        let counter = self.sequences.entry(stream.get()).or_insert(0);
        let first = *counter;
        *counter = counter.wrapping_add(count);
        first
    }

    /// Frame a payload into one or more datagrams.
    ///
    /// Payloads of [`MAX_PAYLOAD`] bytes or fewer produce a single packet with the fragment
    /// flag clear and the marker set. Larger payloads are split, every fragment carrying the
    /// same timestamp, with the marker only on the last.
    pub fn frame(&mut self, stream: StreamId, payload: &[u8]) -> Vec<Datagram> {
        self.frame_at(stream, payload, self.timestamp_now())
    }

    /// Frame a payload with an explicit timestamp. Lets tests pin the clock.
    pub fn frame_at(
        &mut self,
        stream: StreamId,
        payload: &[u8],
        timestamp_us: u32,
    ) -> Vec<Datagram> {
        let chunks: Vec<&[u8]> = if payload.len() <= MAX_PAYLOAD {
            vec![payload]
        } else {
            payload.chunks(MAX_PAYLOAD).collect()
        };

        let fragmented = chunks.len() > 1;
        let count = u16::try_from(chunks.len()).unwrap_or(u16::MAX);
        let first_sequence = self.next_sequence(stream, count);

        chunks
            .into_iter()
            .enumerate()
            .map(|(index, chunk)| {
                let is_last = index + 1 == usize::from(count);
                let sequence = first_sequence.wrapping_add(u16::try_from(index).unwrap_or(0));
                let header = MediaHeader {
                    stream,
                    sequence,
                    timestamp_us,
                    session_id: self.session_id,
                    fragment: fragmented,
                    marker: is_last,
                };

                let mut datagram = Vec::with_capacity(HEADER_LEN + chunk.len());
                datagram.extend_from_slice(&header.encode());
                datagram.extend_from_slice(chunk);
                datagram
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(datagram: &[u8]) -> (MediaHeader, Vec<u8>) {
        let (header, payload) =
            MediaHeader::split(datagram).expect("sender must emit valid packets");
        (header, payload.to_vec())
    }

    #[test]
    fn a_small_payload_should_produce_one_unfragmented_packet() {
        let mut tx = MediaSender::new(7);
        let datagrams = tx.frame_at(StreamId::TEST, b"hello", 1_000);

        assert_eq!(datagrams.len(), 1);
        let (header, payload) = parse(&datagrams[0]);
        assert!(!header.fragment, "a single packet is not a fragment");
        assert!(header.marker, "a single packet ends its frame");
        assert_eq!(payload, b"hello");
    }

    #[test]
    fn a_payload_at_exactly_the_limit_should_not_be_fragmented() {
        let mut tx = MediaSender::new(7);
        let payload = vec![0xAA_u8; MAX_PAYLOAD];
        let datagrams = tx.frame_at(StreamId::TEST, &payload, 1_000);

        assert_eq!(datagrams.len(), 1);
        assert!(!parse(&datagrams[0]).0.fragment);
    }

    #[test]
    fn a_payload_one_byte_over_the_limit_should_be_fragmented() {
        let mut tx = MediaSender::new(7);
        let payload = vec![0xAA_u8; MAX_PAYLOAD + 1];
        let datagrams = tx.frame_at(StreamId::TEST, &payload, 1_000);

        assert_eq!(datagrams.len(), 2);
        assert!(parse(&datagrams[0]).0.fragment);
        assert!(parse(&datagrams[1]).0.fragment);
    }

    #[test]
    fn only_the_last_fragment_should_carry_the_marker() {
        let mut tx = MediaSender::new(7);
        let payload = vec![0xAA_u8; MAX_PAYLOAD * 3];
        let datagrams = tx.frame_at(StreamId::TEST, &payload, 1_000);

        assert_eq!(datagrams.len(), 3);
        assert!(!parse(&datagrams[0]).0.marker);
        assert!(!parse(&datagrams[1]).0.marker);
        assert!(parse(&datagrams[2]).0.marker);
    }

    #[test]
    fn every_fragment_should_share_the_frames_timestamp() {
        let mut tx = MediaSender::new(7);
        let payload = vec![0xAA_u8; MAX_PAYLOAD * 3];
        let datagrams = tx.frame_at(StreamId::TEST, &payload, 0xDEAD_BEEF);

        for datagram in &datagrams {
            assert_eq!(parse(datagram).0.timestamp_us, 0xDEAD_BEEF);
        }
    }

    #[test]
    fn fragments_should_occupy_consecutive_sequence_numbers() {
        let mut tx = MediaSender::new(7);
        let payload = vec![0xAA_u8; MAX_PAYLOAD * 3];
        let datagrams = tx.frame_at(StreamId::TEST, &payload, 1_000);

        let sequences: Vec<u16> = datagrams.iter().map(|d| parse(d).0.sequence).collect();
        assert_eq!(sequences, vec![0, 1, 2]);
    }

    #[test]
    fn no_datagram_should_exceed_the_path_mtu() {
        let mut tx = MediaSender::new(7);
        let payload = vec![0xAA_u8; MAX_PAYLOAD * 5 + 17];
        for datagram in tx.frame_at(StreamId::TEST, &payload, 1_000) {
            assert!(
                datagram.len() <= HEADER_LEN + MAX_PAYLOAD,
                "datagram of {} bytes exceeds the MTU budget",
                datagram.len()
            );
        }
    }

    #[test]
    fn reassembling_the_fragments_should_reproduce_the_payload() {
        let mut tx = MediaSender::new(7);
        let payload: Vec<u8> = (0..(MAX_PAYLOAD * 3 + 42))
            .map(|i| (i % 251) as u8)
            .collect();

        let rebuilt: Vec<u8> = tx
            .frame_at(StreamId::TEST, &payload, 1_000)
            .iter()
            .flat_map(|d| parse(d).1)
            .collect();

        assert_eq!(rebuilt, payload);
    }

    #[test]
    fn sequence_numbers_should_advance_across_frames() {
        let mut tx = MediaSender::new(7);
        let first = parse(&tx.frame_at(StreamId::TEST, b"a", 1)[0]).0.sequence;
        let second = parse(&tx.frame_at(StreamId::TEST, b"b", 2)[0]).0.sequence;
        assert_eq!(second, first + 1);
    }

    #[test]
    fn streams_should_be_sequenced_independently() {
        let mut tx = MediaSender::new(7);
        tx.frame_at(StreamId::TEST, b"a", 1);
        tx.frame_at(StreamId::TEST, b"b", 2);

        let camera = parse(&tx.frame_at(StreamId::CAMERA, b"c", 3)[0]).0.sequence;
        assert_eq!(camera, 0, "each stream starts its own sequence at zero");
    }

    #[test]
    fn every_packet_should_carry_the_session_id() {
        let mut tx = MediaSender::new(0x0123_4567_89AB_CDEF);
        let payload = vec![0xAA_u8; MAX_PAYLOAD * 2];
        for datagram in tx.frame_at(StreamId::TEST, &payload, 1_000) {
            assert_eq!(parse(&datagram).0.session_id, 0x0123_4567_89AB_CDEF);
        }
    }

    #[test]
    fn sequence_numbers_should_wrap_rather_than_overflow() {
        let mut tx = MediaSender::new(7);
        // Wind the counter to the top of the u16 range.
        for _ in 0..65_535 {
            tx.frame_at(StreamId::TEST, b"x", 1);
        }
        let last = parse(&tx.frame_at(StreamId::TEST, b"x", 1)[0]).0.sequence;
        let wrapped = parse(&tx.frame_at(StreamId::TEST, b"x", 1)[0]).0.sequence;

        assert_eq!(last, 65_535);
        assert_eq!(wrapped, 0, "the counter must wrap, not panic");
    }

    #[test]
    fn an_empty_payload_should_still_produce_a_packet() {
        let mut tx = MediaSender::new(7);
        let datagrams = tx.frame_at(StreamId::TEST, b"", 1_000);
        assert_eq!(datagrams.len(), 1);
        assert_eq!(datagrams[0].len(), HEADER_LEN);
    }

    #[test]
    fn the_timestamp_should_advance_with_the_clock() {
        let tx = MediaSender::new(7);
        let first = tx.timestamp_now();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(tx.timestamp_now() > first);
    }
}
