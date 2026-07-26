//! End-to-end camera pipeline: JPEG frames through the sender, the transport's fragmentation
//! and reassembly, and the MJPEG decoder — everything except the kernel device.
//!
//! This is the video counterpart of the audio loopback tests: it proves protocol §7's
//! promises (byte-identical delivery, one-frame loss cost, decode failures stay per-frame)
//! against the real transport code, not a mock.

use unifiedstream_net::protocol::{StreamId, MAX_PAYLOAD};
use unifiedstream_net::transport::{MediaDemux, MediaSender};
use unifiedstream_video::decode_jpeg_to_i420;

const SESSION: u64 = 0xCAFE_F00D;
const WIDTH: u16 = 640;
const HEIGHT: u16 = 480;

/// Encode a deterministic noisy frame — noise defeats JPEG compression enough to guarantee a
/// realistically fragmented payload (tens of KB), unlike a solid color (~2 KB).
fn noisy_jpeg(seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    let mut rgb = Vec::with_capacity(usize::from(WIDTH) * usize::from(HEIGHT) * 3);
    for _ in 0..usize::from(WIDTH) * usize::from(HEIGHT) * 3 {
        // xorshift32: deterministic, no RNG dependency.
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        rgb.push((state & 0xFF) as u8);
    }

    let mut jpeg = Vec::new();
    let encoder = jpeg_encoder::Encoder::new(&mut jpeg, 70);
    encoder
        .encode(&rgb, WIDTH, HEIGHT, jpeg_encoder::ColorType::Rgb)
        .expect("test JPEG must encode");
    jpeg
}

fn demux() -> MediaDemux {
    let mut demux = MediaDemux::new(SESSION);
    demux.register(StreamId::CAMERA);
    demux
}

#[test]
fn a_video_frame_should_survive_the_transport_byte_identical_and_decode() {
    let jpeg = noisy_jpeg(1);
    assert!(
        jpeg.len() > MAX_PAYLOAD * 10,
        "fixture must be realistically large, got {} bytes",
        jpeg.len()
    );

    let mut sender = MediaSender::new(SESSION);
    let datagrams = sender.frame_at(StreamId::CAMERA, &jpeg, 33_000);
    let expected_fragments = jpeg.len().div_ceil(MAX_PAYLOAD);
    assert_eq!(datagrams.len(), expected_fragments);

    let mut rx = demux();
    let mut delivered = Vec::new();
    for datagram in &datagrams {
        delivered.extend(rx.accept(datagram).expect("valid datagrams must be accepted"));
    }

    assert_eq!(delivered.len(), 1, "exactly one frame must come out");
    assert_eq!(delivered[0].payload, jpeg, "payload must be byte-identical");
    assert_eq!(delivered[0].timestamp_us, 33_000);

    let i420 = decode_jpeg_to_i420(&delivered[0].payload, u32::from(WIDTH), u32::from(HEIGHT))
        .expect("the delivered frame must decode");
    assert_eq!(i420.len(), usize::from(WIDTH) * usize::from(HEIGHT) * 3 / 2);
}

#[test]
fn losing_one_fragment_should_cost_exactly_that_frame() {
    let frame_a = noisy_jpeg(2);
    let frame_b = noisy_jpeg(3);

    let mut sender = MediaSender::new(SESSION);
    let datagrams_a = sender.frame_at(StreamId::CAMERA, &frame_a, 33_000);
    let datagrams_b = sender.frame_at(StreamId::CAMERA, &frame_b, 66_000);

    let mut rx = demux();
    let mut delivered = Vec::new();
    for (index, datagram) in datagrams_a.iter().enumerate() {
        if index == datagrams_a.len() / 2 {
            continue; // one fragment of frame A is lost on the network
        }
        delivered.extend(rx.accept(datagram).expect("accept"));
    }
    for datagram in &datagrams_b {
        delivered.extend(rx.accept(datagram).expect("accept"));
    }

    assert_eq!(delivered.len(), 1, "only the complete frame may be delivered");
    assert_eq!(delivered[0].payload, frame_b);
    let stats = rx.stats(StreamId::CAMERA).expect("stream is registered");
    assert_eq!(stats.incomplete_frames, 1);
    assert_eq!(stats.lost, 1);

    // The surviving frame still decodes: loss never corrupts later frames.
    assert!(decode_jpeg_to_i420(&delivered[0].payload, u32::from(WIDTH), u32::from(HEIGHT)).is_ok());
}

#[test]
fn an_undecodable_frame_should_fail_alone_without_poisoning_the_stream() {
    let garbage = vec![0x5A_u8; 20_000];
    let good = noisy_jpeg(4);

    let mut sender = MediaSender::new(SESSION);
    let mut rx = demux();

    let mut delivered = Vec::new();
    for datagram in sender.frame_at(StreamId::CAMERA, &garbage, 33_000) {
        delivered.extend(rx.accept(&datagram).expect("accept"));
    }
    for datagram in sender.frame_at(StreamId::CAMERA, &good, 66_000) {
        delivered.extend(rx.accept(&datagram).expect("accept"));
    }

    // The transport delivers both frames whole; only the decoder tells them apart.
    assert_eq!(delivered.len(), 2);
    assert!(
        decode_jpeg_to_i420(&delivered[0].payload, u32::from(WIDTH), u32::from(HEIGHT)).is_err(),
        "garbage must fail decode, not panic"
    );
    assert!(
        decode_jpeg_to_i420(&delivered[1].payload, u32::from(WIDTH), u32::from(HEIGHT)).is_ok(),
        "the next frame must decode normally"
    );
}
