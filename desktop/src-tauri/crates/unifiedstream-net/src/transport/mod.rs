//! UDP media transport: fragmentation, sequencing, reassembly, and the reorder buffer.
//!
//! Codec-agnostic by design — it moves opaque timestamped payloads and does not know what
//! H.264 or Opus are. Camera, microphone, and speaker all multiplex over one socket,
//! discriminated by [`StreamId`].

mod reassembly;
mod sender;
mod synthetic;

pub use reassembly::{Frame, StreamReceiver, StreamStats, TimestampUnwrapper, REORDER_WINDOW};
pub use sender::{Datagram, MediaSender};
pub use synthetic::{
    build_frame, IntegrityError, TestStreamConfig, TestStreamGenerator, TestStreamReport,
    TestStreamVerifier,
};

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::protocol::{MediaHeader, StreamId, HEADER_LEN, MAX_PAYLOAD, PROTOCOL_VERSION};
use crate::DEFAULT_MEDIA_PORT;

/// Largest datagram we will ever receive, so the receive buffer never truncates a valid packet.
pub const MAX_DATAGRAM: usize = HEADER_LEN + MAX_PAYLOAD;

/// Why a received datagram was discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DropReason {
    /// Shorter than a header, or an unparseable one.
    Malformed,
    /// Protocol version this build does not implement.
    UnsupportedVersion,
    /// Belongs to a different session — a stale sender, or another app on the LAN.
    ForeignSession,
    /// No receiver is registered for this stream id.
    UnknownStream,
}

/// Counters for datagrams that never reached a stream.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DropStats {
    /// Malformed or truncated datagrams.
    pub malformed: u64,
    /// Datagrams carrying an unsupported protocol version.
    pub unsupported_version: u64,
    /// Datagrams for a session other than the active one.
    pub foreign_session: u64,
    /// Datagrams for a stream nobody is listening to.
    pub unknown_stream: u64,
}

impl DropStats {
    fn record(&mut self, reason: DropReason) {
        match reason {
            DropReason::Malformed => self.malformed += 1,
            DropReason::UnsupportedVersion => self.unsupported_version += 1,
            DropReason::ForeignSession => self.foreign_session += 1,
            DropReason::UnknownStream => self.unknown_stream += 1,
        }
    }

    /// Total datagrams discarded.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.malformed + self.unsupported_version + self.foreign_session + self.unknown_stream
    }
}

/// Demultiplexes datagrams into per-stream receivers.
///
/// Separate from the socket so the whole receive path can be tested by feeding it byte slices.
#[derive(Debug)]
pub struct MediaDemux {
    session_id: u64,
    streams: HashMap<u8, StreamReceiver>,
    drops: DropStats,
}

impl MediaDemux {
    /// A demultiplexer accepting only packets for `session_id`.
    #[must_use]
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            streams: HashMap::new(),
            drops: DropStats::default(),
        }
    }

    /// Begin accepting packets for a stream. Unregistered streams are counted and dropped.
    pub fn register(&mut self, stream: StreamId) {
        self.streams.entry(stream.get()).or_default();
    }

    /// Stop accepting packets for a stream.
    pub fn unregister(&mut self, stream: StreamId) {
        self.streams.remove(&stream.get());
    }

    /// Counters for datagrams that never reached a stream.
    #[must_use]
    pub const fn drops(&self) -> DropStats {
        self.drops
    }

    /// Counters for a registered stream.
    #[must_use]
    pub fn stats(&self, stream: StreamId) -> Option<StreamStats> {
        self.streams.get(&stream.get()).map(StreamReceiver::stats)
    }

    /// Counters summed across every registered stream.
    #[must_use]
    pub fn total_stats(&self) -> StreamStats {
        self.streams
            .values()
            .map(StreamReceiver::stats)
            .fold(StreamStats::default(), |mut acc, s| {
                acc.received += s.received;
                acc.lost += s.lost;
                acc.late += s.late;
                acc.incomplete_frames += s.incomplete_frames;
                acc.delivered_frames += s.delivered_frames;
                acc.bytes += s.bytes;
                acc
            })
    }

    /// Feed one datagram, returning any frames it completed.
    ///
    /// Returns `Err(DropReason)` when the datagram was discarded. Discarding is routine, not
    /// exceptional — a stale sender or an unrelated app on the LAN produces these.
    pub fn accept(&mut self, datagram: &[u8]) -> std::result::Result<Vec<Frame>, DropReason> {
        let (header, payload) = match MediaHeader::split(datagram) {
            Ok(parts) => parts,
            Err(_) => {
                // A short buffer and a bad version both surface as a parse failure; separate
                // them so the counters mean something diagnostically.
                let reason = if datagram.len() < HEADER_LEN {
                    DropReason::Malformed
                } else {
                    DropReason::UnsupportedVersion
                };
                self.drops.record(reason);
                return Err(reason);
            }
        };

        if header.session_id != self.session_id {
            self.drops.record(DropReason::ForeignSession);
            return Err(DropReason::ForeignSession);
        }

        let Some(receiver) = self.streams.get_mut(&header.stream.get()) else {
            self.drops.record(DropReason::UnknownStream);
            return Err(DropReason::UnknownStream);
        };

        Ok(receiver.accept(&header, payload))
    }
}

/// A bound UDP media socket.
#[derive(Debug)]
pub struct MediaSocket {
    socket: Arc<UdpSocket>,
    local_port: u16,
    peer: Mutex<Option<SocketAddr>>,
}

impl MediaSocket {
    /// Bind the media socket, preferring [`DEFAULT_MEDIA_PORT`].
    ///
    /// Falls back to an ephemeral port when the default is taken; the bound port is what the
    /// handshake advertises, so a busy default is a non-event rather than a startup failure.
    ///
    /// # Errors
    ///
    /// Fails if no port can be bound at all.
    pub async fn bind() -> Result<Self> {
        Self::bind_on(DEFAULT_MEDIA_PORT).await
    }

    /// Bind a specific port, falling back to ephemeral if it is unavailable.
    ///
    /// # Errors
    ///
    /// Fails if neither the requested port nor an ephemeral one can be bound.
    pub async fn bind_on(preferred: u16) -> Result<Self> {
        let socket = match UdpSocket::bind(("0.0.0.0", preferred)).await {
            Ok(socket) => socket,
            Err(e) => {
                tracing::warn!(
                    port = preferred,
                    error = %e,
                    "media port unavailable; falling back to an ephemeral port"
                );
                UdpSocket::bind(("0.0.0.0", 0)).await?
            }
        };

        let local_port = socket.local_addr()?.port();
        tracing::info!(port = local_port, "media socket bound");

        Ok(Self {
            socket: Arc::new(socket),
            local_port,
            peer: Mutex::new(None),
        })
    }

    /// The port actually bound, to be advertised in the handshake.
    #[must_use]
    pub const fn local_port(&self) -> u16 {
        self.local_port
    }

    /// Point this socket at a peer for subsequent sends.
    pub async fn set_peer(&self, addr: SocketAddr) {
        *self.peer.lock().await = Some(addr);
    }

    /// The peer this socket sends to, if one has been set.
    pub async fn peer(&self) -> Option<SocketAddr> {
        *self.peer.lock().await
    }

    /// Send one datagram to the configured peer.
    ///
    /// # Errors
    ///
    /// Returns [`crate::NetError::NoSession`] if no peer has been set, or an IO error if the
    /// send fails.
    pub async fn send(&self, datagram: &[u8]) -> Result<usize> {
        let peer = self.peer().await.ok_or(crate::NetError::NoSession)?;
        Ok(self.socket.send_to(datagram, peer).await?)
    }

    /// Send every datagram of a framed payload.
    ///
    /// # Errors
    ///
    /// Same conditions as [`MediaSocket::send`].
    pub async fn send_all(&self, datagrams: &[Datagram]) -> Result<usize> {
        let mut sent = 0;
        for datagram in datagrams {
            sent += self.send(datagram).await?;
        }
        Ok(sent)
    }

    /// Receive one datagram.
    ///
    /// # Errors
    ///
    /// Fails on an underlying socket error.
    pub async fn recv(&self, buf: &mut [u8; MAX_DATAGRAM]) -> Result<(usize, SocketAddr)> {
        Ok(self.socket.recv_from(buf).await?)
    }

    /// Clone the underlying socket handle, for a separate receive task.
    #[must_use]
    pub fn handle(&self) -> Arc<UdpSocket> {
        Arc::clone(&self.socket)
    }
}

/// Whether a datagram claims a protocol version this build implements.
///
/// Cheap pre-filter for a receive loop that wants to reject before allocating.
#[must_use]
pub fn is_supported_version(datagram: &[u8]) -> bool {
    datagram
        .first()
        .is_some_and(|flags| flags >> 6 == PROTOCOL_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sender_for(session: u64) -> MediaSender {
        MediaSender::new(session)
    }

    #[test]
    fn a_registered_stream_should_receive_its_frames() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        let datagrams = tx.frame_at(StreamId::TEST, b"hello", 100);
        let frames = demux.accept(&datagrams[0]).expect("must be accepted");

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, b"hello");
    }

    #[test]
    fn a_packet_for_another_session_should_be_dropped() {
        let mut tx = sender_for(999);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        let datagrams = tx.frame_at(StreamId::TEST, b"intruder", 100);
        assert_eq!(demux.accept(&datagrams[0]), Err(DropReason::ForeignSession));
        assert_eq!(demux.drops().foreign_session, 1);
    }

    #[test]
    fn a_packet_for_an_unregistered_stream_should_be_dropped_and_counted() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        let datagrams = tx.frame_at(StreamId::CAMERA, b"video", 100);
        assert_eq!(demux.accept(&datagrams[0]), Err(DropReason::UnknownStream));
        assert_eq!(demux.drops().unknown_stream, 1);
    }

    #[test]
    fn dropping_one_stream_should_not_disturb_another() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        let unknown = tx.frame_at(StreamId::CAMERA, b"video", 100);
        let _ = demux.accept(&unknown[0]);

        let known = tx.frame_at(StreamId::TEST, b"audio", 200);
        assert_eq!(
            demux.accept(&known[0]).expect("must be accepted").len(),
            1,
            "an unknown stream must not disrupt a registered one"
        );
    }

    #[test]
    fn a_truncated_datagram_should_be_dropped_as_malformed() {
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        for len in 0..HEADER_LEN {
            let short = vec![0x40_u8; len];
            assert_eq!(demux.accept(&short), Err(DropReason::Malformed));
        }
    }

    #[test]
    fn an_unsupported_version_should_be_dropped() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        let mut datagram = tx.frame_at(StreamId::TEST, b"hello", 100).remove(0);
        if let Some(first) = datagram.first_mut() {
            *first = (*first & 0x3F) | (2 << 6);
        }

        assert_eq!(demux.accept(&datagram), Err(DropReason::UnsupportedVersion));
        assert_eq!(demux.drops().unsupported_version, 1);
    }

    #[test]
    fn a_fragmented_frame_should_survive_the_full_send_and_receive_path() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        let payload: Vec<u8> = (0..(MAX_PAYLOAD * 4 + 7)).map(|i| (i % 253) as u8).collect();
        let datagrams = tx.frame_at(StreamId::TEST, &payload, 5_000);
        assert!(datagrams.len() > 1, "this payload must fragment");

        let mut delivered = Vec::new();
        for datagram in &datagrams {
            delivered.extend(demux.accept(datagram).expect("must be accepted"));
        }

        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].payload, payload);
        assert_eq!(delivered[0].timestamp_us, 5_000);
    }

    #[test]
    fn fragments_arriving_out_of_order_should_still_reassemble() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        // Establish a frame boundary first, so this exercises mid-stream reordering rather
        // than the join-mid-frame case.
        let opener = tx.frame_at(StreamId::TEST, b"start", 1_000);
        assert_eq!(demux.accept(&opener[0]).expect("must be accepted").len(), 1);

        let payload: Vec<u8> = (0..(MAX_PAYLOAD * 2 + 5)).map(|i| (i % 251) as u8).collect();
        let mut datagrams = tx.frame_at(StreamId::TEST, &payload, 5_000);
        datagrams.swap(0, 1);

        let mut delivered = Vec::new();
        for datagram in &datagrams {
            delivered.extend(demux.accept(datagram).expect("must be accepted"));
        }

        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].payload, payload);
    }

    #[test]
    fn joining_mid_frame_should_drop_the_partial_frame_rather_than_corrupt_it() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        let payload: Vec<u8> = (0..(MAX_PAYLOAD * 3)).map(|i| (i % 251) as u8).collect();
        let datagrams = tx.frame_at(StreamId::TEST, &payload, 5_000);

        // The receiver never sees the frame's first fragment.
        let mut delivered = Vec::new();
        for datagram in datagrams.iter().skip(1) {
            if let Ok(frames) = demux.accept(datagram) {
                delivered.extend(frames);
            }
        }

        assert!(
            delivered.is_empty(),
            "a frame missing its head must be dropped, never delivered with a missing prefix"
        );
    }

    #[test]
    fn a_stream_should_resynchronise_after_joining_mid_frame() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        let first: Vec<u8> = vec![0x11; MAX_PAYLOAD * 2];
        for datagram in tx.frame_at(StreamId::TEST, &first, 1_000).iter().skip(1) {
            let _ = demux.accept(datagram);
        }

        // The next complete frame must come through.
        let second: Vec<u8> = vec![0x22; MAX_PAYLOAD * 2];
        let mut delivered = Vec::new();
        for datagram in &tx.frame_at(StreamId::TEST, &second, 2_000) {
            delivered.extend(demux.accept(datagram).expect("must be accepted"));
        }

        assert_eq!(delivered.len(), 1, "the receiver must recover on the next frame");
        assert_eq!(delivered[0].payload, second);
    }

    #[test]
    fn a_lost_fragment_should_prevent_delivery_of_a_partial_frame() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        let payload = vec![0xAB_u8; MAX_PAYLOAD * 3];
        let datagrams = tx.frame_at(StreamId::TEST, &payload, 5_000);

        let mut delivered = Vec::new();
        // Drop the middle fragment entirely.
        for (index, datagram) in datagrams.iter().enumerate() {
            if index == 1 {
                continue;
            }
            if let Ok(frames) = demux.accept(datagram) {
                delivered.extend(frames);
            }
        }

        assert!(
            delivered.is_empty(),
            "a frame missing a fragment must never be delivered"
        );
    }

    #[test]
    fn total_stats_should_sum_every_registered_stream() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);
        demux.register(StreamId::CAMERA);

        let _ = demux.accept(&tx.frame_at(StreamId::TEST, b"aaa", 100)[0]);
        let _ = demux.accept(&tx.frame_at(StreamId::CAMERA, b"bb", 200)[0]);

        let total = demux.total_stats();
        assert_eq!(total.received, 2);
        assert_eq!(total.bytes, 5);
        assert_eq!(total.delivered_frames, 2);
    }

    #[test]
    fn unregistering_a_stream_should_stop_accepting_it() {
        let mut tx = sender_for(7);
        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);
        demux.unregister(StreamId::TEST);

        let datagrams = tx.frame_at(StreamId::TEST, b"hello", 100);
        assert_eq!(demux.accept(&datagrams[0]), Err(DropReason::UnknownStream));
    }

    #[test]
    fn drop_stats_should_total_every_category() {
        let mut demux = MediaDemux::new(7);
        let _ = demux.accept(&[]);
        let _ = demux.accept(&[0x80; HEADER_LEN]); // version 2
        assert_eq!(demux.drops().total(), 2);
    }

    #[test]
    fn version_prefilter_should_accept_our_version_and_reject_others() {
        assert!(is_supported_version(&[0x40]));
        assert!(!is_supported_version(&[0x80]));
        assert!(!is_supported_version(&[]));
    }

    #[tokio::test]
    async fn binding_should_report_the_port_it_actually_got() {
        let socket = MediaSocket::bind_on(0).await.expect("ephemeral bind");
        assert_ne!(socket.local_port(), 0, "an ephemeral port must be reported");
    }

    #[tokio::test]
    async fn binding_a_taken_port_should_fall_back_to_an_ephemeral_one() {
        let first = MediaSocket::bind_on(0).await.expect("first bind");
        let taken = first.local_port();

        let second = MediaSocket::bind_on(taken).await.expect("must fall back");
        assert_ne!(
            second.local_port(),
            taken,
            "the fallback must not claim the busy port"
        );
    }

    #[tokio::test]
    async fn sending_without_a_peer_should_fail_rather_than_panic() {
        let socket = MediaSocket::bind_on(0).await.expect("bind");
        assert!(socket.send(b"orphan").await.is_err());
    }

    #[tokio::test]
    async fn a_datagram_should_survive_a_real_loopback_round_trip() {
        let receiver = MediaSocket::bind_on(0).await.expect("bind receiver");
        let sender = MediaSocket::bind_on(0).await.expect("bind sender");

        let receiver_addr: SocketAddr =
            format!("127.0.0.1:{}", receiver.local_port()).parse().expect("addr");
        sender.set_peer(receiver_addr).await;

        let mut tx = sender_for(7);
        let payload = vec![0x5A_u8; 300];
        let datagrams = tx.frame_at(StreamId::TEST, &payload, 1_234);
        sender.send_all(&datagrams).await.expect("send");

        let mut buf = [0_u8; MAX_DATAGRAM];
        let (len, _from) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            receiver.recv(&mut buf),
        )
        .await
        .expect("must arrive")
        .expect("recv");

        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);
        let frames = demux
            .accept(buf.get(..len).expect("length within buffer"))
            .expect("must be accepted");

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, payload);
    }

    #[tokio::test]
    async fn a_fragmented_frame_should_survive_a_real_loopback_round_trip() {
        let receiver = MediaSocket::bind_on(0).await.expect("bind receiver");
        let sender = MediaSocket::bind_on(0).await.expect("bind sender");

        let receiver_addr: SocketAddr =
            format!("127.0.0.1:{}", receiver.local_port()).parse().expect("addr");
        sender.set_peer(receiver_addr).await;

        let mut tx = sender_for(7);
        let payload: Vec<u8> = (0..(MAX_PAYLOAD * 3)).map(|i| (i % 249) as u8).collect();
        let datagrams = tx.frame_at(StreamId::TEST, &payload, 4_321);
        assert_eq!(datagrams.len(), 3);
        sender.send_all(&datagrams).await.expect("send");

        let mut demux = MediaDemux::new(7);
        demux.register(StreamId::TEST);

        let mut delivered = Vec::new();
        let mut buf = [0_u8; MAX_DATAGRAM];
        for _ in 0..3 {
            let (len, _) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                receiver.recv(&mut buf),
            )
            .await
            .expect("must arrive")
            .expect("recv");
            delivered.extend(
                demux
                    .accept(buf.get(..len).expect("length within buffer"))
                    .expect("must be accepted"),
            );
        }

        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].payload, payload);
    }
}
