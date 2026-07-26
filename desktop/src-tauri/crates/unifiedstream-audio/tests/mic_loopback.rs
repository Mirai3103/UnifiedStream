//! End-to-end microphone path without a phone or a PipeWire daemon: PCM frames through the
//! real sender, fragmentation, demultiplexer, and reassembly into the jitter buffer — exactly
//! the pipeline the desktop runs between the UDP socket and the virtual source.

use unifiedstream_audio::{JitterBuffer, JITTER_CAP_FRAMES};
use unifiedstream_net::transport::{MediaDemux, MediaSender};
use unifiedstream_net::StreamId;

const SESSION: u64 = 0xA5A5_A5A5_A5A5_A5A5;
const FRAME_SAMPLES: usize = 960; // 20 ms at 48 kHz mono
const FRAME_INTERVAL_US: u32 = 20_000;

/// A deterministic, non-repeating 20 ms test frame.
fn pcm_frame(index: usize) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|sample| {
            #[allow(clippy::cast_possible_truncation, reason = "wrapping is the point")]
            let value = (index * 31 + sample * 7) as i16;
            value
        })
        .collect()
}

/// Protocol §5.1 wire encoding: little-endian S16.
fn encode_s16le(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

fn decode_s16le(payload: &[u8]) -> Vec<i16> {
    payload
        .chunks_exact(2)
        .map(|pair| {
            if let [lo, hi] = pair {
                i16::from_le_bytes([*lo, *hi])
            } else {
                0
            }
        })
        .collect()
}

/// The receive side of the desktop's mic path, as one unit.
struct Receiver {
    demux: MediaDemux,
    buffer: JitterBuffer,
}

impl Receiver {
    fn new() -> Self {
        let mut demux = MediaDemux::new(SESSION);
        demux.register(StreamId::MICROPHONE);
        Self {
            demux,
            buffer: JitterBuffer::default(),
        }
    }

    fn accept(&mut self, datagram: &[u8]) {
        if let Ok(frames) = self.demux.accept(datagram) {
            for frame in frames {
                self.buffer.push(decode_s16le(&frame.payload));
            }
        }
    }
}

#[test]
fn pcm_frames_should_arrive_byte_identical_through_the_full_path() {
    let mut sender = MediaSender::new(SESSION);
    let mut rx = Receiver::new();

    for index in 0..20 {
        let samples = pcm_frame(index);
        let datagrams = sender.frame_at(
            StreamId::MICROPHONE,
            &encode_s16le(&samples),
            index as u32 * FRAME_INTERVAL_US,
        );
        // 1920 bytes must fragment into exactly two packets.
        assert_eq!(datagrams.len(), 2, "a 20 ms PCM frame is two fragments");
        for datagram in &datagrams {
            rx.accept(datagram);
        }

        // Drain as we go, like the audio clock would; the output must be bit-exact.
        let mut out = vec![0_i16; FRAME_SAMPLES];
        rx.buffer.pop_into(&mut out);
        assert_eq!(out, samples, "frame {index} must round-trip byte-identical");
    }

    assert_eq!(rx.buffer.stats().underruns, 0, "a lockstep drain never underruns");
}

#[test]
fn a_lost_fragment_should_cost_exactly_one_frame() {
    let mut sender = MediaSender::new(SESSION);
    let mut rx = Receiver::new();

    let total = 10;
    let lost_index = 4;
    for index in 0..total {
        let datagrams = sender.frame_at(
            StreamId::MICROPHONE,
            &encode_s16le(&pcm_frame(index)),
            index as u32 * FRAME_INTERVAL_US,
        );
        for (fragment, datagram) in datagrams.iter().enumerate() {
            // Lose the second fragment of one frame.
            if index == lost_index && fragment == 1 {
                continue;
            }
            rx.accept(datagram);
        }
    }

    let stats = rx
        .demux
        .stats(StreamId::MICROPHONE)
        .expect("stream is registered");
    assert_eq!(
        stats.delivered_frames,
        (total - 1) as u64,
        "every frame but the damaged one must be delivered"
    );
    assert_eq!(stats.incomplete_frames, 1, "the damaged frame is discarded whole");
    assert_eq!(stats.lost, 1, "exactly one packet went missing");

    // And the audio that did arrive is intact: the buffer holds the last frames, undamaged.
    let mut out = vec![0_i16; FRAME_SAMPLES];
    rx.buffer.pop_into(&mut out);
    assert!(
        (0..total).any(|i| out == pcm_frame(i)),
        "buffered audio must be an uncorrupted frame"
    );
}

#[test]
fn a_transmission_gap_should_underrun_to_silence_and_recover() {
    let mut sender = MediaSender::new(SESSION);
    let mut rx = Receiver::new();

    // One frame arrives, then the phone mutes (stops sending).
    let first = pcm_frame(0);
    for datagram in sender.frame_at(StreamId::MICROPHONE, &encode_s16le(&first), 0) {
        rx.accept(&datagram);
    }
    let mut out = vec![0_i16; FRAME_SAMPLES];
    rx.buffer.pop_into(&mut out);
    assert_eq!(out, first);

    // The audio clock keeps pulling during the gap: silence, not a stall.
    rx.buffer.pop_into(&mut out);
    assert!(out.iter().all(|&s| s == 0), "a gap must play silence");
    assert!(rx.buffer.stats().underruns >= 1);

    // Unmute: the next frame plays normally.
    let resumed = pcm_frame(7);
    for datagram in sender.frame_at(
        StreamId::MICROPHONE,
        &encode_s16le(&resumed),
        50 * FRAME_INTERVAL_US,
    ) {
        rx.accept(&datagram);
    }
    rx.buffer.pop_into(&mut out);
    assert_eq!(out, resumed, "playback must resume seamlessly after a gap");
}

#[test]
fn a_stalled_consumer_should_bound_latency_by_dropping_the_oldest_audio() {
    let mut sender = MediaSender::new(SESSION);
    let mut rx = Receiver::new();

    // The network keeps delivering while the audio clock is stalled.
    let total = 10;
    for index in 0..total {
        for datagram in sender.frame_at(
            StreamId::MICROPHONE,
            &encode_s16le(&pcm_frame(index)),
            index as u32 * FRAME_INTERVAL_US,
        ) {
            rx.accept(&datagram);
        }
    }

    assert_eq!(
        rx.buffer.depth(),
        JITTER_CAP_FRAMES,
        "buffered latency must be capped"
    );
    assert_eq!(
        rx.buffer.stats().overruns,
        (total - JITTER_CAP_FRAMES) as u64,
        "everything beyond the cap is dropped"
    );

    // What survives is the newest audio, in order.
    let mut out = vec![0_i16; FRAME_SAMPLES];
    rx.buffer.pop_into(&mut out);
    assert_eq!(
        out,
        pcm_frame(total - JITTER_CAP_FRAMES),
        "the oldest surviving frame is the first one after the drops"
    );
}
