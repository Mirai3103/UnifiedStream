//! Receive-side frame reassembly, loss accounting, and the reorder buffer.
//!
//! Kept free of sockets so every edge — wrap-around, lost fragments, late arrivals — is
//! exercised by ordinary unit tests rather than by timing-dependent network tests.

use std::collections::BTreeMap;

use crate::protocol::{seq_distance, seq_is_newer, MediaHeader};

/// Packets held per stream while waiting for an out-of-order predecessor.
///
/// Deliberately small and fixed. An adaptive buffer that grows under loss is the standard
/// choice and the wrong one here: it trades away exactly the latency this project exists to
/// minimize. Three packets covers LAN reordering, which is rare.
pub const REORDER_WINDOW: usize = 3;

/// Upper bound on one reassembled frame's payload.
///
/// Audio frames are a few KB and even a 1080p MJPEG frame tops out well under 300 KB, so
/// anything larger is a broken or hostile sender. Without a cap, a stream of fragments that
/// never delivers its marker would grow the fragment buffer without bound.
pub const MAX_FRAME_BYTES: usize = 512 * 1024;

/// A fully reassembled frame ready for a consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Sender's timestamp, widened past the 32-bit wire wrap.
    pub timestamp_us: u64,
    /// Sequence number of the frame's first packet.
    pub first_sequence: u16,
    /// Reassembled payload.
    pub payload: Vec<u8>,
}

/// Running counts for one stream.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StreamStats {
    /// Packets accepted and accounted for.
    pub received: u64,
    /// Packets never seen, inferred from sequence gaps.
    pub lost: u64,
    /// Packets that arrived after their slot had already passed.
    pub late: u64,
    /// Frames dropped because fragments were missing.
    pub incomplete_frames: u64,
    /// Frames delivered whole.
    pub delivered_frames: u64,
    /// Payload bytes accepted.
    pub bytes: u64,
}

impl StreamStats {
    /// Packets expected over the lifetime of the stream.
    #[must_use]
    pub const fn expected(&self) -> u64 {
        self.received + self.lost
    }

    /// Loss as a percentage of expected packets. Zero when nothing was expected.
    #[must_use]
    pub fn loss_pct(&self) -> f64 {
        let expected = self.expected();
        if expected == 0 {
            return 0.0;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "counter magnitudes are far below 2^53"
        )]
        let pct = (self.lost as f64 / expected as f64) * 100.0;
        pct
    }
}

/// Widens a 32-bit microsecond timestamp past its ~71.6-minute wrap.
///
/// Called out explicitly because a wrap bug works perfectly in every short test and then fails
/// an hour into a real session.
#[derive(Debug, Default, Clone, Copy)]
pub struct TimestampUnwrapper {
    last_raw: Option<u32>,
    epoch: u64,
}

impl TimestampUnwrapper {
    /// Widen `raw` to a monotonically increasing 64-bit microsecond value.
    pub fn unwrap_ts(&mut self, raw: u32) -> u64 {
        let Some(last) = self.last_raw else {
            self.last_raw = Some(raw);
            return u64::from(raw);
        };

        // A backwards jump of more than half the range is a wrap; a smaller one is just an
        // out-of-order packet and must not advance the epoch.
        if raw < last && last.wrapping_sub(raw) > u32::MAX / 2 {
            self.epoch = self.epoch.wrapping_add(u64::from(u32::MAX) + 1);
        }

        if seq32_is_newer(raw, last) {
            self.last_raw = Some(raw);
        }

        self.epoch + u64::from(raw)
    }
}

fn seq32_is_newer(a: u32, b: u32) -> bool {
    let delta = a.wrapping_sub(b);
    delta != 0 && delta < 0x8000_0000
}

/// Per-stream receive state: ordering, loss accounting, and fragment reassembly.
#[derive(Debug, Default)]
pub struct StreamReceiver {
    stats: StreamStats,
    /// Highest sequence released to the consumer.
    last_released: Option<u16>,
    /// Packets held awaiting an out-of-order predecessor.
    pending: BTreeMap<u16, HeldPacket>,
    /// Fragments accumulated for the frame currently being reassembled.
    fragments: Vec<HeldPacket>,
    /// Payload bytes held in `fragments`, kept alongside so the [`MAX_FRAME_BYTES`] check is
    /// O(1) per packet.
    fragment_bytes: usize,
    /// Whether we know where a frame boundary is.
    ///
    /// A receiver that joins mid-frame — first packet reordered, or a stream picked up in
    /// progress — has no way to tell a frame's first fragment from its third. Concatenating
    /// what arrives would deliver a frame with a missing head, which is silent corruption:
    /// strictly worse than dropping it. So fragmented frames are discarded until a marker
    /// establishes a boundary. An unfragmented packet is self-contained and always syncs.
    synced: bool,
    /// Whether any packet has been released yet, used to recognise stream start.
    released_any: bool,
    unwrapper: TimestampUnwrapper,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HeldPacket {
    sequence: u16,
    timestamp_us: u32,
    marker: bool,
    fragment: bool,
    payload: Vec<u8>,
}

impl StreamReceiver {
    /// A receiver with no history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Counters for this stream.
    #[must_use]
    pub const fn stats(&self) -> StreamStats {
        self.stats
    }

    /// Accept a packet, returning any frames that became deliverable.
    ///
    /// Packets are released in sequence order. A packet older than what has already been
    /// released is counted as late and discarded rather than delivered out of order.
    pub fn accept(&mut self, header: &MediaHeader, payload: &[u8]) -> Vec<Frame> {
        // Too old to matter: its slot has passed and the consumer has moved on.
        if let Some(last) = self.last_released {
            if !seq_is_newer(header.sequence, last) {
                self.stats.late += 1;
                return Vec::new();
            }
        }

        self.stats.received += 1;
        self.stats.bytes += payload.len() as u64;

        self.pending.insert(
            header.sequence,
            HeldPacket {
                sequence: header.sequence,
                timestamp_us: header.timestamp_us,
                marker: header.marker,
                fragment: header.fragment,
                payload: payload.to_vec(),
            },
        );

        self.drain()
    }

    /// Release everything currently releasable.
    fn drain(&mut self) -> Vec<Frame> {
        let mut frames = Vec::new();

        while let Some((&next_seq, _)) = self.pending.iter().next() {
            let in_order = match self.last_released {
                None => true,
                Some(last) => seq_distance(last, next_seq) == 1,
            };

            // The buffer must not stall waiting for a packet that may never come: once it is
            // full, release the oldest and account for the gap as loss.
            let must_flush = self.pending.len() > REORDER_WINDOW;

            if !in_order && !must_flush {
                break;
            }

            if !in_order {
                if let Some(last) = self.last_released {
                    let gap = u64::from(seq_distance(last, next_seq).saturating_sub(1));
                    self.stats.lost += gap;
                }
            }

            let Some(packet) = self.pending.remove(&next_seq) else {
                break;
            };
            self.last_released = Some(next_seq);

            if let Some(frame) = self.absorb(packet) {
                frames.push(frame);
            }
        }

        frames
    }

    /// Fold a released packet into the frame under construction.
    fn absorb(&mut self, packet: HeldPacket) -> Option<Frame> {
        // Sequence 0 as the very first release is a frame start by construction: the sender
        // starts every stream's counter at zero. Restricting this to stream start keeps a
        // frame that happens to span the 16-bit wrap from being re-synced mid-frame.
        let stream_start = !self.released_any;
        self.released_any = true;
        if stream_start && packet.sequence == 0 {
            self.synced = true;
        }

        // An unfragmented packet is a whole frame on its own, and re-establishes the boundary.
        if !packet.fragment {
            self.drop_partial();
            self.synced = true;
            self.stats.delivered_frames += 1;
            return Some(Frame {
                timestamp_us: self.unwrapper.unwrap_ts(packet.timestamp_us),
                first_sequence: packet.sequence,
                payload: packet.payload,
            });
        }

        // Fragmented, and we do not know where this frame began. Discard until a marker gives
        // us a boundary to start counting from.
        if !self.synced {
            if packet.marker {
                self.synced = true;
                self.stats.incomplete_frames += 1;
            }
            return None;
        }

        // A newer frame starting means the previous one will never complete.
        if let Some(first) = self.fragments.first() {
            if first.timestamp_us != packet.timestamp_us {
                self.drop_partial();
            }
        }

        // A frame past the cap can never be delivered; holding more of it would let one
        // stream grow memory without bound. The rest of the frame is unusable, so boundary
        // sync is re-established only by this packet's own marker.
        if self.fragment_bytes + packet.payload.len() > MAX_FRAME_BYTES {
            self.drop_partial();
            self.synced = packet.marker;
            return None;
        }

        let is_last = packet.marker;
        let timestamp_raw = packet.timestamp_us;
        self.fragment_bytes += packet.payload.len();
        self.fragments.push(packet);

        if !is_last {
            return None;
        }

        // A fragmented frame is only whole if its fragments are sequentially contiguous.
        if self.fragments.len() > 1 && !self.fragments_are_contiguous() {
            self.drop_partial();
            return None;
        }

        let first_sequence = self.fragments.first().map_or(0, |p| p.sequence);
        let mut payload = Vec::with_capacity(self.fragment_bytes);
        for fragment in self.fragments.drain(..) {
            payload.extend_from_slice(&fragment.payload);
        }
        self.fragment_bytes = 0;

        self.stats.delivered_frames += 1;
        Some(Frame {
            timestamp_us: self.unwrapper.unwrap_ts(timestamp_raw),
            first_sequence,
            payload,
        })
    }

    /// Discard the partial frame under construction, counting it once if there was one.
    fn drop_partial(&mut self) {
        if !self.fragments.is_empty() {
            self.fragments.clear();
            self.stats.incomplete_frames += 1;
        }
        self.fragment_bytes = 0;
    }

    fn fragments_are_contiguous(&self) -> bool {
        self.fragments.windows(2).all(|pair| match pair {
            [a, b] => seq_distance(a.sequence, b.sequence) == 1,
            _ => true,
        })
    }

    /// Release anything still held, ending any partial frame. Used at teardown.
    pub fn flush(&mut self) -> Vec<Frame> {
        let mut frames = Vec::new();
        while !self.pending.is_empty() {
            let before = self.pending.len();
            frames.extend(self.drain_one_forced());
            if self.pending.len() == before {
                break;
            }
        }
        self.drop_partial();
        frames
    }

    fn drain_one_forced(&mut self) -> Vec<Frame> {
        let Some(&next_seq) = self.pending.keys().next() else {
            return Vec::new();
        };
        if let (Some(last), true) = (self.last_released, true) {
            let gap = u64::from(seq_distance(last, next_seq).saturating_sub(1));
            self.stats.lost += gap;
        }
        let Some(packet) = self.pending.remove(&next_seq) else {
            return Vec::new();
        };
        self.last_released = Some(next_seq);
        self.absorb(packet).into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{StreamId, MAX_PAYLOAD};

    fn header(sequence: u16, timestamp_us: u32, fragment: bool, marker: bool) -> MediaHeader {
        MediaHeader {
            stream: StreamId::TEST,
            sequence,
            timestamp_us,
            session_id: 7,
            fragment,
            marker,
        }
    }

    /// A single unfragmented packet: fragment clear, marker set.
    fn whole(sequence: u16, timestamp_us: u32) -> MediaHeader {
        header(sequence, timestamp_us, false, true)
    }

    #[test]
    fn a_single_packet_frame_should_be_delivered_immediately() {
        let mut rx = StreamReceiver::new();
        let frames = rx.accept(&whole(0, 1000), b"hello");

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, b"hello");
        assert_eq!(frames[0].timestamp_us, 1000);
    }

    #[test]
    fn consecutive_frames_should_each_be_delivered() {
        let mut rx = StreamReceiver::new();
        assert_eq!(rx.accept(&whole(0, 1000), b"a").len(), 1);
        assert_eq!(rx.accept(&whole(1, 2000), b"b").len(), 1);
        assert_eq!(rx.accept(&whole(2, 3000), b"c").len(), 1);
        assert_eq!(rx.stats().delivered_frames, 3);
        assert_eq!(rx.stats().lost, 0);
    }

    #[test]
    fn a_fragmented_frame_should_be_reassembled_in_order() {
        let mut rx = StreamReceiver::new();
        assert!(rx.accept(&header(0, 500, true, false), b"abc").is_empty());
        assert!(rx.accept(&header(1, 500, true, false), b"def").is_empty());

        let frames = rx.accept(&header(2, 500, true, true), b"ghi");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, b"abcdefghi");
    }

    #[test]
    fn out_of_order_fragments_should_still_reassemble() {
        let mut rx = StreamReceiver::new();
        assert!(rx.accept(&header(0, 500, true, false), b"abc").is_empty());
        // Third fragment arrives before the second.
        assert!(rx.accept(&header(2, 500, true, true), b"ghi").is_empty());

        let frames = rx.accept(&header(1, 500, true, false), b"def");
        assert_eq!(
            frames.len(),
            1,
            "the frame must complete once the gap fills"
        );
        assert_eq!(frames[0].payload, b"abcdefghi");
    }

    #[test]
    fn reordered_packets_should_be_released_in_sequence_order() {
        let mut rx = StreamReceiver::new();
        let mut delivered = Vec::new();

        delivered.extend(rx.accept(&whole(0, 100), b"0"));
        delivered.extend(rx.accept(&whole(2, 300), b"2"));
        delivered.extend(rx.accept(&whole(1, 200), b"1"));

        let order: Vec<&[u8]> = delivered.iter().map(|f| f.payload.as_slice()).collect();
        assert_eq!(
            order,
            vec![b"0".as_slice(), b"1".as_slice(), b"2".as_slice()]
        );
    }

    #[test]
    fn the_reorder_buffer_should_not_stall_on_a_lost_packet() {
        let mut rx = StreamReceiver::new();
        assert_eq!(rx.accept(&whole(0, 100), b"0").len(), 1);

        // Sequence 1 never arrives. Fill the window past its capacity.
        let mut delivered = 0;
        for seq in 2..=(2 + REORDER_WINDOW as u16) {
            delivered += rx.accept(&whole(seq, u32::from(seq) * 100), b"x").len();
        }

        assert!(
            delivered > 0,
            "the buffer must release rather than wait forever"
        );
        assert!(rx.stats().lost >= 1, "the gap must be counted as loss");
    }

    #[test]
    fn the_reorder_buffer_should_hold_at_most_the_window() {
        let mut rx = StreamReceiver::new();
        rx.accept(&whole(0, 100), b"0");
        for seq in 2..20_u16 {
            rx.accept(&whole(seq, u32::from(seq) * 100), b"x");
        }
        assert!(
            rx.pending.len() <= REORDER_WINDOW,
            "buffer grew to {} packets",
            rx.pending.len()
        );
    }

    #[test]
    fn a_gap_should_be_counted_as_loss() {
        let mut rx = StreamReceiver::new();
        rx.accept(&whole(10, 100), b"a");
        rx.accept(&whole(11, 200), b"b");
        // 12 and 13 are lost.
        for seq in 14..=17_u16 {
            rx.accept(&whole(seq, u32::from(seq) * 100), b"c");
        }
        assert_eq!(rx.stats().lost, 2, "exactly two packets went missing");
    }

    #[test]
    fn a_packet_older_than_what_was_released_should_be_counted_late_not_lost() {
        let mut rx = StreamReceiver::new();
        rx.accept(&whole(10, 100), b"a");
        rx.accept(&whole(11, 200), b"b");

        let frames = rx.accept(&whole(9, 50), b"stale");

        assert!(frames.is_empty(), "a late packet must not be delivered");
        assert_eq!(rx.stats().late, 1);
        assert_eq!(rx.stats().lost, 0);
    }

    #[test]
    fn a_sequence_wrap_should_not_be_counted_as_loss() {
        let mut rx = StreamReceiver::new();
        rx.accept(&whole(65534, 100), b"a");
        rx.accept(&whole(65535, 200), b"b");
        rx.accept(&whole(0, 300), b"c");
        rx.accept(&whole(1, 400), b"d");

        assert_eq!(rx.stats().lost, 0, "wrapping is forward progress, not loss");
        assert_eq!(rx.stats().delivered_frames, 4);
    }

    #[test]
    fn an_incomplete_frame_should_be_discarded_when_a_newer_frame_starts() {
        let mut rx = StreamReceiver::new();
        // Frame A: two fragments, but the second is lost, so the marker never arrives.
        assert!(rx
            .accept(&header(0, 500, true, false), b"partial")
            .is_empty());
        // Frame B begins with a new timestamp.
        let frames = rx.accept(&header(1, 900, false, true), b"whole");

        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].payload, b"whole",
            "no partial frame may be delivered"
        );
        assert_eq!(rx.stats().incomplete_frames, 1);
    }

    #[test]
    fn a_fragmented_frame_with_a_missing_middle_should_not_be_delivered() {
        let mut rx = StreamReceiver::new();
        rx.accept(&header(0, 500, true, false), b"aaa");
        // Fragment at sequence 1 is lost forever; push past the window so 2 is force-released.
        for seq in 2..=(2 + REORDER_WINDOW as u16) {
            rx.accept(
                &header(seq, 500, true, seq == 2 + REORDER_WINDOW as u16),
                b"bbb",
            );
        }
        assert!(
            rx.stats().incomplete_frames >= 1,
            "a frame missing a fragment must be discarded"
        );
    }

    #[test]
    fn stats_should_report_loss_as_a_percentage_of_expected() {
        let mut rx = StreamReceiver::new();
        rx.accept(&whole(0, 100), b"a");
        for seq in 2..=8_u16 {
            rx.accept(&whole(seq, u32::from(seq) * 100), b"x");
        }
        let stats = rx.stats();
        assert_eq!(stats.expected(), stats.received + stats.lost);
        assert!(stats.loss_pct() > 0.0 && stats.loss_pct() < 100.0);
    }

    #[test]
    fn loss_percentage_should_be_zero_when_nothing_was_expected() {
        assert!((StreamStats::default().loss_pct() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_clean_stream_should_report_no_loss() {
        let mut rx = StreamReceiver::new();
        for seq in 0..100_u16 {
            rx.accept(&whole(seq, u32::from(seq) * 100), b"x");
        }
        assert!((rx.stats().loss_pct() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn timestamps_should_stay_monotonic_across_the_thirty_two_bit_wrap() {
        let mut unwrapper = TimestampUnwrapper::default();

        let before = unwrapper.unwrap_ts(u32::MAX - 1_000);
        let after = unwrapper.unwrap_ts(1_000);

        assert!(
            after > before,
            "a session past 71 minutes must not go backwards: {before} then {after}"
        );
        assert_eq!(after - before, 2_001);
    }

    #[test]
    fn timestamp_unwrapping_should_leave_ordinary_progress_alone() {
        let mut unwrapper = TimestampUnwrapper::default();
        assert_eq!(unwrapper.unwrap_ts(1_000), 1_000);
        assert_eq!(unwrapper.unwrap_ts(2_000), 2_000);
        assert_eq!(unwrapper.unwrap_ts(3_000), 3_000);
    }

    #[test]
    fn a_small_backwards_timestamp_step_should_not_be_read_as_a_wrap() {
        let mut unwrapper = TimestampUnwrapper::default();
        unwrapper.unwrap_ts(10_000);
        // An out-of-order packet, not a wrap.
        assert_eq!(unwrapper.unwrap_ts(9_000), 9_000);
        assert_eq!(unwrapper.unwrap_ts(11_000), 11_000);
    }

    #[test]
    fn a_fifty_fragment_video_frame_should_round_trip_byte_identical() {
        // A ~60 KB MJPEG frame at the wire's 1200-byte fragment size, protocol §7.
        let payload: Vec<u8> = (0..60_000_u32).map(|i| (i % 251) as u8).collect();
        let chunks: Vec<&[u8]> = payload.chunks(MAX_PAYLOAD).collect();
        assert_eq!(chunks.len(), 50);

        let mut rx = StreamReceiver::new();
        let mut delivered = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let seq = u16::try_from(i).expect("fits");
            let marker = i + 1 == chunks.len();
            delivered.extend(rx.accept(&header(seq, 9_000, true, marker), chunk));
        }

        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].payload, payload);
        assert_eq!(rx.stats().delivered_frames, 1);
        assert_eq!(rx.stats().incomplete_frames, 0);
    }

    #[test]
    fn losing_one_fragment_of_a_video_frame_should_cost_only_that_frame() {
        let mut rx = StreamReceiver::new();

        // Frame A: 50 fragments, sequence 25 never arrives.
        let mut delivered = 0;
        for i in 0..50_u16 {
            if i == 25 {
                continue;
            }
            delivered += rx.accept(&header(i, 1_000, true, i == 49), b"x").len();
        }
        // Frame B: complete, and pushes A's gap past the reorder window.
        for i in 50..100_u16 {
            delivered += rx.accept(&header(i, 2_000, true, i == 99), b"y").len();
        }

        assert_eq!(delivered, 1, "only the complete frame may be delivered");
        assert_eq!(rx.stats().incomplete_frames, 1);
        assert_eq!(rx.stats().lost, 1);
    }

    #[test]
    fn a_frame_past_the_size_cap_should_be_dropped_without_unbounded_buffering() {
        let mut rx = StreamReceiver::new();
        let chunk = vec![0xCD_u8; MAX_PAYLOAD];
        // Enough same-timestamp fragments to exceed MAX_FRAME_BYTES, marker never sent.
        let over = u16::try_from(MAX_FRAME_BYTES / MAX_PAYLOAD + 8).expect("fits");
        for i in 0..over {
            let frames = rx.accept(&header(i, 3_000, true, false), &chunk);
            assert!(
                frames.is_empty(),
                "an oversized frame must never be delivered"
            );
        }

        assert!(
            rx.fragment_bytes <= MAX_FRAME_BYTES,
            "held bytes must stay capped, got {}",
            rx.fragment_bytes
        );
        assert!(rx.stats().incomplete_frames >= 1);
    }

    #[test]
    fn the_frame_after_an_oversized_one_should_be_delivered_once_a_boundary_returns() {
        let mut rx = StreamReceiver::new();
        let chunk = vec![0xCD_u8; MAX_PAYLOAD];
        let over = u16::try_from(MAX_FRAME_BYTES / MAX_PAYLOAD + 2).expect("fits");
        for i in 0..over {
            // The final fragment carries the marker, closing the oversized frame.
            rx.accept(&header(i, 3_000, true, i == over - 1), &chunk);
        }

        let frames = rx.accept(&whole(over, 4_000), b"next frame");
        assert_eq!(
            frames.len(),
            1,
            "the stream must recover after the oversized frame"
        );
        assert_eq!(frames[0].payload, b"next frame");
    }

    #[test]
    fn a_frame_at_exactly_the_payload_limit_should_be_delivered_whole() {
        let mut rx = StreamReceiver::new();
        let payload = vec![0xAB_u8; MAX_PAYLOAD];
        let frames = rx.accept(&whole(0, 100), &payload);

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload.len(), MAX_PAYLOAD);
    }

    #[test]
    fn an_empty_payload_should_still_deliver_a_frame() {
        let mut rx = StreamReceiver::new();
        assert_eq!(rx.accept(&whole(0, 100), b"").len(), 1);
    }

    #[test]
    fn byte_counters_should_track_accepted_payload() {
        let mut rx = StreamReceiver::new();
        rx.accept(&whole(0, 100), b"12345");
        rx.accept(&whole(1, 200), b"678");
        assert_eq!(rx.stats().bytes, 8);
    }
}
