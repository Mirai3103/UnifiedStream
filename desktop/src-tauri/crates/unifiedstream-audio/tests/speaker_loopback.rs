//! End-to-end speaker path without a phone or a PipeWire daemon: captured stereo samples
//! through the real frame chunker, sender, fragmentation, demultiplexer, and reassembly —
//! exactly the pipeline the desktop runs between the virtual sink and the UDP socket, plus the
//! phone's receive side in miniature.

use unifiedstream_audio::{AudioFormat, FrameChunker};
use unifiedstream_net::transport::{MediaDemux, MediaSender};
use unifiedstream_net::StreamId;

const SESSION: u64 = 0x5EA5_5EA5_5EA5_5EA5;
/// 20 ms at 48 kHz stereo: 960 samples per channel, interleaved.
const FRAME_SAMPLES: usize = 1920;
const FRAME_BYTES: usize = FRAME_SAMPLES * 2;
const FRAME_INTERVAL_US: u32 = 20_000;

/// A deterministic, non-repeating 20 ms stereo test frame with distinct channels.
fn stereo_frame(index: usize) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|sample| {
            // Even positions are the left channel, odd the right; keep them different so an
            // interleaving mistake cannot cancel out.
            #[allow(clippy::cast_possible_truncation, reason = "wrapping is the point")]
            let value = if sample % 2 == 0 {
                (index * 31 + sample * 7) as i16
            } else {
                (index * 17 + sample * 3 + 9_000) as i16
            };
            value
        })
        .collect()
}

/// Protocol §6.1 wire encoding: little-endian S16, interleaved.
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

fn receiver() -> MediaDemux {
    let mut demux = MediaDemux::new(SESSION);
    demux.register(StreamId::SPEAKER);
    demux
}

#[test]
fn the_speaker_format_should_describe_a_3840_byte_frame() {
    assert_eq!(AudioFormat::SPEAKER.total_samples(), FRAME_SAMPLES);
    assert_eq!(AudioFormat::SPEAKER.total_samples() * 2, FRAME_BYTES);
}

#[test]
fn stereo_frames_should_arrive_byte_identical_through_the_full_path() {
    let mut sender = MediaSender::new(SESSION);
    let mut demux = receiver();

    for index in 0..20 {
        let samples = stereo_frame(index);
        let datagrams = sender.frame_at(
            StreamId::SPEAKER,
            &encode_s16le(&samples),
            index as u32 * FRAME_INTERVAL_US,
        );
        // 3840 bytes must fragment into exactly four packets.
        assert_eq!(
            datagrams.len(),
            4,
            "a 20 ms stereo PCM frame is four fragments"
        );

        let mut delivered = Vec::new();
        for datagram in &datagrams {
            if let Ok(frames) = demux.accept(datagram) {
                delivered.extend(frames);
            }
        }

        assert_eq!(delivered.len(), 1, "frame {index} must reassemble");
        assert_eq!(
            decode_s16le(&delivered[0].payload),
            samples,
            "frame {index} must round-trip byte-identical, interleaving intact"
        );
    }
}

#[test]
fn the_capture_chunker_should_frame_arbitrary_quantum_sizes_for_the_wire() {
    // PipeWire delivers whatever its quantum is — never exactly one wire frame. The chunker
    // must reassemble exact frames regardless, and the wire must carry them intact.
    let mut chunker = FrameChunker::new(FRAME_SAMPLES);
    let mut sender = MediaSender::new(SESSION);
    let mut demux = receiver();

    let total_frames = 5;
    let source: Vec<i16> = (0..total_frames).flat_map(stereo_frame).collect();

    let mut received = Vec::new();
    // Deliver in awkward, non-frame-aligned chunks (a realistic mix of quantum sizes).
    for chunk in source.chunks(441) {
        chunker.push(chunk, |frame| {
            assert_eq!(frame.len(), FRAME_SAMPLES);
            for datagram in sender.frame(StreamId::SPEAKER, &encode_s16le(&frame)) {
                if let Ok(frames) = demux.accept(&datagram) {
                    for delivered in frames {
                        received.extend(decode_s16le(&delivered.payload));
                    }
                }
            }
        });
    }

    assert_eq!(
        received, source,
        "every captured sample must reach the far side, in order"
    );
    assert_eq!(
        chunker.pending_len(),
        0,
        "the source length is a whole number of frames"
    );
}

#[test]
fn a_lost_fragment_should_cost_exactly_one_frame() {
    let mut sender = MediaSender::new(SESSION);
    let mut demux = receiver();

    let total = 10;
    let lost_index = 4;
    let mut delivered = Vec::new();
    for index in 0..total {
        let datagrams = sender.frame_at(
            StreamId::SPEAKER,
            &encode_s16le(&stereo_frame(index)),
            index as u32 * FRAME_INTERVAL_US,
        );
        for (fragment, datagram) in datagrams.iter().enumerate() {
            // Lose the third fragment of one frame.
            if index == lost_index && fragment == 2 {
                continue;
            }
            if let Ok(frames) = demux.accept(datagram) {
                delivered.extend(frames);
            }
        }
    }

    let stats = demux
        .stats(StreamId::SPEAKER)
        .expect("stream is registered");
    assert_eq!(
        stats.delivered_frames,
        (total - 1) as u64,
        "every frame but the damaged one must be delivered"
    );
    assert_eq!(
        stats.incomplete_frames, 1,
        "the damaged frame is discarded whole"
    );
    assert_eq!(stats.lost, 1, "exactly one packet went missing");

    // Nothing that survived is corrupted, and the damaged frame is absent.
    for frame in &delivered {
        let samples = decode_s16le(&frame.payload);
        assert!(
            (0..total).any(|i| samples == stereo_frame(i)),
            "delivered audio must be an uncorrupted frame"
        );
        assert_ne!(
            samples,
            stereo_frame(lost_index),
            "the damaged frame must never be delivered"
        );
    }
    assert_eq!(delivered.len(), total - 1);
}
