//! Cross-implementation header vectors.
//!
//! Reads the same `testdata/media-header-vectors.json` the Kotlin suite reads. If these two
//! ever disagree, the phone and the PC have silently stopped speaking the same protocol.

use std::path::PathBuf;

use serde::Deserialize;
use unifiedstream_net::protocol::{MediaHeader, StreamId, HEADER_LEN, PROTOCOL_VERSION};

#[derive(Debug, Deserialize)]
struct Vector {
    name: String,
    hex: String,
    stream: u8,
    sequence: u16,
    timestamp_us: u32,
    /// Decimal string: a u64 exceeds JSON's safe integer range.
    session_id: String,
    fragment: bool,
    marker: bool,
}

#[derive(Debug, Deserialize)]
struct VectorFile {
    protocol_version: u8,
    header_len: usize,
    vectors: Vec<Vector>,
}

/// Walk up from the crate directory so the fixture has one home in the repo rather than a copy
/// per platform.
fn load() -> VectorFile {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("testdata/media-header-vectors.json");
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).expect("fixture must be readable");
            return serde_json::from_str(&text).expect("fixture must be valid JSON");
        }
        if !dir.pop() {
            panic!(
                "testdata/media-header-vectors.json not found above {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    assert!(hex.len() % 2 == 0, "hex string must have even length");
    hex.as_bytes()
        .chunks(2)
        .map(|pair| {
            let s = std::str::from_utf8(pair).expect("hex is ascii");
            u8::from_str_radix(s, 16).expect("hex digit")
        })
        .collect()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn fixture_should_match_this_builds_constants() {
    let file = load();
    assert_eq!(file.protocol_version, PROTOCOL_VERSION);
    assert_eq!(file.header_len, HEADER_LEN);
    assert!(!file.vectors.is_empty(), "fixture must not be empty");
}

#[test]
fn every_fixture_vector_should_decode_to_its_declared_fields() {
    for v in load().vectors {
        let bytes = hex_to_bytes(&v.hex);
        let decoded = MediaHeader::decode(&bytes)
            .unwrap_or_else(|e| panic!("{}: must decode, got {e}", v.name));
        let session_id: u64 = v
            .session_id
            .parse()
            .unwrap_or_else(|e| panic!("{}: session id must parse, got {e}", v.name));

        assert_eq!(decoded.stream, StreamId(v.stream), "{}: stream", v.name);
        assert_eq!(decoded.sequence, v.sequence, "{}: sequence", v.name);
        assert_eq!(
            decoded.timestamp_us, v.timestamp_us,
            "{}: timestamp",
            v.name
        );
        assert_eq!(decoded.session_id, session_id, "{}: session id", v.name);
        assert_eq!(decoded.fragment, v.fragment, "{}: fragment", v.name);
        assert_eq!(decoded.marker, v.marker, "{}: marker", v.name);
    }
}

#[test]
fn every_fixture_vector_should_re_encode_to_the_same_bytes() {
    for v in load().vectors {
        let bytes = hex_to_bytes(&v.hex);
        let decoded = MediaHeader::decode(&bytes)
            .unwrap_or_else(|e| panic!("{}: must decode, got {e}", v.name));
        assert_eq!(
            bytes_to_hex(&decoded.encode()),
            v.hex,
            "{}: re-encoded bytes",
            v.name
        );
    }
}

// --- Synthetic test-stream frames ------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FrameVector {
    index: u32,
    size: usize,
    hex: String,
}

#[derive(Debug, Deserialize)]
struct FrameFile {
    preamble_bytes: usize,
    frames: Vec<FrameVector>,
}

fn load_frames() -> FrameFile {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("testdata/test-stream-frames.json");
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).expect("fixture must be readable");
            return serde_json::from_str(&text).expect("fixture must be valid JSON");
        }
        if !dir.pop() {
            panic!("testdata/test-stream-frames.json not found");
        }
    }
}

#[test]
fn the_generator_should_reproduce_every_fixture_frame_byte_for_byte() {
    use unifiedstream_net::transport::build_frame;

    let file = load_frames();
    assert_eq!(file.preamble_bytes, 8);

    for vector in file.frames {
        let built = build_frame(vector.index, vector.size);
        assert_eq!(
            bytes_to_hex(&built),
            vector.hex,
            "frame index {} size {}",
            vector.index,
            vector.size
        );
    }
}

#[test]
fn the_verifier_should_accept_every_fixture_frame() {
    use unifiedstream_net::transport::TestStreamVerifier;

    for vector in load_frames().frames {
        let mut verifier = TestStreamVerifier::new();
        let bytes = hex_to_bytes(&vector.hex);
        assert_eq!(
            verifier.verify(&bytes),
            Ok(vector.index),
            "frame index {} must verify",
            vector.index
        );
    }
}

// --- Control-message wire fixtures ------------------------------------------------------------

/// Writes the control lines this build produces, for the Kotlin suite to parse.
///
/// Guards the interop bug where a session id above `i64::MAX` was emitted as a JSON number and
/// overflowed Kotlin's `Long`, silently breaking roughly every second handshake.
#[test]
fn control_lines_should_match_the_committed_fixture() {
    use unifiedstream_net::protocol::{ControlMessage, HelloAck};

    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = loop {
        let candidate = dir.join("testdata/control-lines.json");
        if candidate.is_file() {
            break candidate;
        }
        assert!(dir.pop(), "testdata/control-lines.json not found");
    };

    #[derive(Debug, Deserialize)]
    struct Lines {
        hello_ack_high_session_id: String,
        stream_start_mic_pcm: String,
        stream_start_speaker_pcm: String,
        stream_start_camera_mjpeg: String,
        stream_request_speaker_start: String,
        stream_request_camera_start: String,
        stream_ack_accepted: String,
        stream_ack_refused: String,
        stream_stop: String,
        stream_request_start: String,
    }
    let text = std::fs::read_to_string(&path).expect("fixture must be readable");
    let expected: Lines = serde_json::from_str(&text).expect("fixture must be valid JSON");

    // A session id with the top bit set: above i64::MAX.
    let ack = ControlMessage::HelloAck(HelloAck {
        version: 1,
        device_id: "desktop-1".to_owned(),
        device_name: "cachyos-x8664".to_owned(),
        caps: vec!["cam".to_owned(), "mic".to_owned(), "spk".to_owned()],
        session_id: 13_597_845_101_738_855_385,
        media_port: 47811,
    });

    let line = ack.to_line().expect("encode");
    assert_eq!(line.trim_end(), expected.hello_ack_high_session_id);

    // And it must survive a round trip through our own parser.
    let parsed = ControlMessage::from_line(&line).expect("decode");
    assert_eq!(parsed, ack);

    // Stream lifecycle lines, protocol §3.9. The Kotlin suite parses and re-emits these.
    use unifiedstream_net::protocol::{AudioParams, StreamRefusal, VideoParams};

    let cases = [
        (
            ControlMessage::StreamStart {
                stream: 2,
                params: AudioParams::MICROPHONE_PCM.into(),
            },
            &expected.stream_start_mic_pcm,
        ),
        (
            ControlMessage::StreamStart {
                stream: 3,
                params: AudioParams::SPEAKER_PCM.into(),
            },
            &expected.stream_start_speaker_pcm,
        ),
        (
            ControlMessage::StreamStart {
                stream: 1,
                params: VideoParams::CAMERA_MJPEG_720P.into(),
            },
            &expected.stream_start_camera_mjpeg,
        ),
        (
            ControlMessage::StreamRequest {
                stream: 3,
                active: true,
            },
            &expected.stream_request_speaker_start,
        ),
        (
            ControlMessage::StreamRequest {
                stream: 1,
                active: true,
            },
            &expected.stream_request_camera_start,
        ),
        (
            ControlMessage::stream_accept(2),
            &expected.stream_ack_accepted,
        ),
        (
            ControlMessage::stream_refuse(2, StreamRefusal::UnsupportedCodec),
            &expected.stream_ack_refused,
        ),
        (
            ControlMessage::StreamStop { stream: 2 },
            &expected.stream_stop,
        ),
        (
            ControlMessage::StreamRequest {
                stream: 2,
                active: true,
            },
            &expected.stream_request_start,
        ),
    ];
    for (message, fixture) in cases {
        let line = message.to_line().expect("encode");
        assert_eq!(line.trim_end(), fixture.as_str());
        let parsed = ControlMessage::from_line(&line).expect("decode");
        assert_eq!(parsed, message);
    }
}
