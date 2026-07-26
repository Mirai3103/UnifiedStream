//! Control-channel messages: newline-delimited JSON over TCP.
//!
//! Normative in `openspec/specs/protocol.md` §3. JSON rather than a binary format because
//! control traffic is negligible in volume and being able to read it in a log — or poke it with
//! `nc` — is worth more than the bytes.

use serde::{Deserialize, Serialize};

use crate::error::{NetError, Result};

/// Serializes a `u64` as a decimal string.
///
/// A session id uses the full 64-bit range, which JSON numbers cannot carry safely: values
/// above 2^63 overflow a signed 64-bit reader (Kotlin's `Long`), and values above 2^53 lose
/// precision in any JavaScript reader. Both peers and the desktop UI read this field, so it
/// travels as text.
pub(crate) mod u64_string {
    use serde::{Deserialize as _, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// The same encoding for an optional session id.
pub(crate) mod opt_u64_string {
    use serde::{Deserialize as _, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &Option<u64>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => serializer.serialize_str(&v.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        let text = Option::<String>::deserialize(deserializer)?;
        text.map(|t| t.parse().map_err(serde::de::Error::custom))
            .transpose()
    }
}

/// Capability tokens a peer may advertise.
pub mod caps {
    /// Camera stream, phone to PC.
    pub const CAMERA: &str = "cam";
    /// Microphone stream, phone to PC.
    pub const MICROPHONE: &str = "mic";
    /// Speaker stream, PC to phone.
    pub const SPEAKER: &str = "spk";

    /// Every capability this build knows about.
    pub const ALL: [&str; 3] = [CAMERA, MICROPHONE, SPEAKER];
}

/// Opening handshake message, phone to desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Protocol version the sender speaks.
    pub version: u8,
    /// Stable device UUID, persisted across restarts.
    pub device_id: String,
    /// Human-readable device name.
    pub device_name: String,
    /// Capability tokens. Unknown tokens are ignored, not rejected.
    pub caps: Vec<String>,
    /// Session to resume after a drop. Absent on a first connection.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "opt_u64_string"
    )]
    pub resume_session_id: Option<u64>,
    /// UDP port the phone bound for media.
    ///
    /// The desktop pairs this with the TCP peer's IP to address the phone, so it can send
    /// without waiting to learn the address from an inbound datagram.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_port: Option<u16>,
}

/// Handshake acceptance, desktop to phone. Establishes the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloAck {
    /// Protocol version the desktop speaks.
    pub version: u8,
    /// Desktop's stable device UUID.
    pub device_id: String,
    /// Desktop's human-readable name.
    pub device_name: String,
    /// Desktop's capability tokens.
    pub caps: Vec<String>,
    /// Random 64-bit session identifier, echoed in every media header.
    ///
    /// Carried as a decimal string; see [`u64_string`].
    #[serde(with = "u64_string")]
    pub session_id: u64,
    /// UDP port the desktop bound for media.
    pub media_port: u16,
}

/// Why a control exchange failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorReason {
    /// Peer's protocol version is not supported. Connection closes.
    VersionMismatch,
    /// User declined pairing, or the prompt timed out. Connection closes.
    Rejected,
    /// A session is already active. Connection closes.
    Busy,
    /// Unparseable or `type`-less line. Connection stays open.
    Malformed,
    /// Unexpected local failure. Connection closes.
    Internal,
}

impl ErrorReason {
    /// Whether receiving this reason means the control connection is being closed.
    ///
    /// `Malformed` is the one recoverable case: a single bad line does not end a session.
    #[must_use]
    pub const fn is_fatal(self) -> bool {
        !matches!(self, Self::Malformed)
    }
}

/// Error report, either direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorMessage {
    /// Machine-readable cause.
    pub reason: ErrorReason,
    /// Human-readable detail.
    pub message: String,
    /// Version the sender supports. Present only for [`ErrorReason::VersionMismatch`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_version: Option<u8>,
}

/// Link-quality sample, sent once per second while a session is active.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TelemetryReport {
    /// Smoothed round-trip time. `None` before the first heartbeat completes.
    pub rtt_ms: Option<f64>,
    /// Outbound media throughput over the last interval.
    pub tx_mbps: f64,
    /// Inbound media throughput over the last interval.
    pub rx_mbps: f64,
    /// Packets lost as a percentage of packets expected over the last interval.
    pub loss_pct: f64,
    /// Interarrival jitter, RFC 3550 §6.4.1.
    pub jitter_ms: f64,
}

impl Default for TelemetryReport {
    fn default() -> Self {
        Self {
            rtt_ms: None,
            tx_mbps: 0.0,
            rx_mbps: 0.0,
            loss_pct: 0.0,
            jitter_ms: 0.0,
        }
    }
}

/// Audio codec named in stream parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AudioCodec {
    /// Raw signed 16-bit little-endian samples. The mandatory baseline every peer decodes.
    PcmS16le,
    /// One Opus packet per frame. Offered only when the source has a working encoder.
    Opus,
    /// A codec this build does not know.
    ///
    /// Deserialized rather than rejected so the sink can answer with a `stream_ack` refusal
    /// (`unsupported_codec`) instead of treating the whole line as malformed.
    #[serde(other)]
    Unknown,
}

/// Format parameters for an audio stream, protocol §3.9.1.
///
/// Unknown fields are ignored on receipt; video streams will define their own parameter set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioParams {
    /// Payload encoding.
    pub codec: AudioCodec,
    /// Samples per second.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u8,
    /// Frame duration in milliseconds.
    pub frame_ms: u32,
}

impl AudioParams {
    /// The microphone baseline: PCM S16LE, 48 kHz, mono, 20 ms frames.
    pub const MICROPHONE_PCM: Self = Self {
        codec: AudioCodec::PcmS16le,
        sample_rate: 48_000,
        channels: 1,
        frame_ms: 20,
    };

    /// The speaker default: PCM S16LE, 48 kHz, stereo, 20 ms frames. Stereo because system
    /// audio is stereo; voice was the reason the microphone is mono.
    pub const SPEAKER_PCM: Self = Self {
        codec: AudioCodec::PcmS16le,
        sample_rate: 48_000,
        channels: 2,
        frame_ms: 20,
    };
}

/// Why a `stream_start` or `stream_request` was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StreamRefusal {
    /// The stream's capability token is not in the negotiated set.
    NotNegotiated,
    /// The sink cannot decode the offered codec.
    UnsupportedCodec,
    /// The sink does not recognise the stream identifier.
    UnsupportedStream,
    /// The sink cannot take another stream right now.
    Busy,
    /// The sink failed locally — e.g. its audio system is unavailable.
    Internal,
}

/// A single control-channel message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ControlMessage {
    /// Opens the handshake.
    Hello(Hello),
    /// Accepts the handshake and establishes a session.
    HelloAck(HelloAck),
    /// Reports a failure.
    Error(ErrorMessage),
    /// Heartbeat probe. `timestamp` is the sender's monotonic microseconds.
    Ping {
        /// Sender's monotonic clock, echoed verbatim in the reply.
        timestamp: u64,
    },
    /// Heartbeat reply, echoing the probe's timestamp.
    Pong {
        /// The probe's timestamp, unmodified.
        timestamp: u64,
    },
    /// Periodic link-quality report.
    Telemetry(TelemetryReport),
    /// Source announces a stream it wants to send, protocol §3.9.1.
    StreamStart {
        /// Stream identifier, protocol §2.2.
        stream: u8,
        /// Format the source will send.
        params: AudioParams,
    },
    /// Sink accepts or refuses a `stream_start`, protocol §3.9.2.
    StreamAck {
        /// Stream identifier being answered.
        stream: u8,
        /// Whether media may flow.
        accepted: bool,
        /// Why not. Present exactly when `accepted` is false.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<StreamRefusal>,
    },
    /// Either side ends a stream, protocol §3.9.3.
    StreamStop {
        /// Stream identifier to close.
        stream: u8,
    },
    /// Sink asks the source to start or stop a stream, protocol §3.9.4.
    StreamRequest {
        /// Stream identifier.
        stream: u8,
        /// `true` to request a start, `false` a stop.
        active: bool,
    },
    /// Clean session teardown.
    Bye,
}

impl ControlMessage {
    /// Build an error message.
    #[must_use]
    pub fn error(reason: ErrorReason, message: impl Into<String>) -> Self {
        Self::Error(ErrorMessage {
            reason,
            message: message.into(),
            supported_version: None,
        })
    }

    /// Build an accepting `stream_ack`.
    #[must_use]
    pub const fn stream_accept(stream: u8) -> Self {
        Self::StreamAck {
            stream,
            accepted: true,
            reason: None,
        }
    }

    /// Build a refusing `stream_ack`.
    #[must_use]
    pub const fn stream_refuse(stream: u8, reason: StreamRefusal) -> Self {
        Self::StreamAck {
            stream,
            accepted: false,
            reason: Some(reason),
        }
    }

    /// Build a version-mismatch error carrying the version this build supports.
    #[must_use]
    pub fn version_mismatch(ours: u8) -> Self {
        Self::Error(ErrorMessage {
            reason: ErrorReason::VersionMismatch,
            message: format!("server speaks version {ours}"),
            supported_version: Some(ours),
        })
    }

    /// Serialize to a single newline-terminated JSON line.
    ///
    /// # Errors
    ///
    /// Fails only if the message contains values serde cannot represent as JSON, which the
    /// concrete field types make unreachable in practice.
    pub fn to_line(&self) -> Result<String> {
        let mut line = serde_json::to_string(self)
            .map_err(|e| NetError::MalformedControl(format!("encode failed: {e}")))?;
        line.push('\n');
        Ok(line)
    }

    /// Parse one line received from the control channel.
    ///
    /// # Errors
    ///
    /// Returns [`NetError::MalformedControl`] when the line is not valid JSON, lacks a `type`
    /// field, or carries a `type` this build does not know.
    pub fn from_line(line: &str) -> Result<Self> {
        serde_json::from_str(line.trim_end_matches(['\r', '\n']))
            .map_err(|e| NetError::MalformedControl(e.to_string()))
    }
}

/// Capability tokens present in both lists, in the order the local side lists them.
///
/// Unknown tokens survive intersection — if both peers name the same future capability, they
/// agree on it without this build needing to know what it is.
#[must_use]
pub fn intersect_caps(ours: &[String], theirs: &[String]) -> Vec<String> {
    ours.iter()
        .filter(|c| theirs.contains(c))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Hello {
        Hello {
            version: 1,
            device_id: "8f14e45f-ceea-467a-9a3f-1b2c3d4e5f60".to_owned(),
            device_name: "Pixel 8".to_owned(),
            caps: vec!["cam".to_owned(), "mic".to_owned(), "spk".to_owned()],
            resume_session_id: None,
            media_port: Some(crate::DEFAULT_MEDIA_PORT),
        }
    }

    #[test]
    fn hello_should_round_trip_through_a_line() {
        let original = ControlMessage::Hello(hello());
        let line = original.to_line().expect("encode");
        let parsed = ControlMessage::from_line(&line).expect("decode");
        assert_eq!(parsed, original);
    }

    #[test]
    fn to_line_should_terminate_with_a_newline() {
        let line = ControlMessage::Bye.to_line().expect("encode");
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn to_line_should_emit_exactly_one_line() {
        let line = ControlMessage::Hello(hello()).to_line().expect("encode");
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[test]
    fn bye_should_serialize_as_a_tagged_object() {
        let line = ControlMessage::Bye.to_line().expect("encode");
        assert_eq!(line.trim_end(), r#"{"type":"bye"}"#);
    }

    #[test]
    fn hello_should_omit_resume_session_id_when_absent() {
        let line = ControlMessage::Hello(hello()).to_line().expect("encode");
        assert!(!line.contains("resume_session_id"));
    }

    #[test]
    fn hello_should_carry_resume_session_id_when_present() {
        let resuming = Hello {
            resume_session_id: Some(0x0123_4567_89AB_CDEF),
            ..hello()
        };
        let line = ControlMessage::Hello(resuming.clone())
            .to_line()
            .expect("encode");
        let parsed = ControlMessage::from_line(&line).expect("decode");
        assert_eq!(parsed, ControlMessage::Hello(resuming));
    }

    #[test]
    fn ping_should_round_trip_its_timestamp_exactly() {
        let original = ControlMessage::Ping {
            timestamp: 1_721_990_400_123_456,
        };
        let parsed = ControlMessage::from_line(&original.to_line().expect("encode")).expect("decode");
        assert_eq!(parsed, original);
    }

    #[test]
    fn telemetry_should_round_trip_a_null_rtt() {
        let original = ControlMessage::Telemetry(TelemetryReport::default());
        let parsed = ControlMessage::from_line(&original.to_line().expect("encode")).expect("decode");
        assert_eq!(parsed, original);
    }

    #[test]
    fn version_mismatch_should_name_the_supported_version() {
        let msg = ControlMessage::version_mismatch(1);
        let ControlMessage::Error(err) = msg else {
            panic!("expected an error message");
        };
        assert_eq!(err.supported_version, Some(1));
    }

    #[test]
    fn from_line_should_reject_invalid_json() {
        assert!(ControlMessage::from_line("{not json").is_err());
    }

    #[test]
    fn from_line_should_reject_a_message_without_a_type() {
        assert!(ControlMessage::from_line(r#"{"version":1}"#).is_err());
    }

    #[test]
    fn from_line_should_reject_an_unknown_type() {
        assert!(ControlMessage::from_line(r#"{"type":"teleport"}"#).is_err());
    }

    #[test]
    fn from_line_should_tolerate_crlf_termination() {
        let parsed = ControlMessage::from_line("{\"type\":\"bye\"}\r\n").expect("decode");
        assert_eq!(parsed, ControlMessage::Bye);
    }

    #[test]
    fn from_line_should_never_panic_on_arbitrary_text() {
        for line in ["", " ", "\n", "null", "[]", "0", "\"bye\"", "{}", "{\"type\":null}"] {
            let _ = ControlMessage::from_line(line);
        }
    }

    #[test]
    fn malformed_should_be_the_only_non_fatal_reason() {
        assert!(!ErrorReason::Malformed.is_fatal());
        for reason in [
            ErrorReason::VersionMismatch,
            ErrorReason::Rejected,
            ErrorReason::Busy,
            ErrorReason::Internal,
        ] {
            assert!(reason.is_fatal(), "{reason:?} must close the connection");
        }
    }

    #[test]
    fn stream_start_should_round_trip_through_a_line() {
        let original = ControlMessage::StreamStart {
            stream: 2,
            params: AudioParams::MICROPHONE_PCM,
        };
        let parsed = ControlMessage::from_line(&original.to_line().expect("encode")).expect("decode");
        assert_eq!(parsed, original);
    }

    #[test]
    fn stream_start_should_serialize_the_documented_shape() {
        let line = ControlMessage::StreamStart {
            stream: 2,
            params: AudioParams::MICROPHONE_PCM,
        }
        .to_line()
        .expect("encode");
        assert_eq!(
            line.trim_end(),
            r#"{"type":"stream_start","stream":2,"params":{"codec":"pcm_s16le","sample_rate":48000,"channels":1,"frame_ms":20}}"#
        );
    }

    #[test]
    fn an_accepting_stream_ack_should_omit_the_reason() {
        let line = ControlMessage::stream_accept(2).to_line().expect("encode");
        assert_eq!(line.trim_end(), r#"{"type":"stream_ack","stream":2,"accepted":true}"#);
    }

    #[test]
    fn a_refusing_stream_ack_should_carry_its_reason() {
        let original = ControlMessage::stream_refuse(2, StreamRefusal::UnsupportedCodec);
        let line = original.to_line().expect("encode");
        assert_eq!(
            line.trim_end(),
            r#"{"type":"stream_ack","stream":2,"accepted":false,"reason":"unsupported_codec"}"#
        );
        let parsed = ControlMessage::from_line(&line).expect("decode");
        assert_eq!(parsed, original);
    }

    #[test]
    fn stream_stop_should_round_trip_through_a_line() {
        let original = ControlMessage::StreamStop { stream: 2 };
        let parsed = ControlMessage::from_line(&original.to_line().expect("encode")).expect("decode");
        assert_eq!(parsed, original);
    }

    #[test]
    fn stream_request_should_round_trip_both_directions_of_intent() {
        for active in [true, false] {
            let original = ControlMessage::StreamRequest { stream: 2, active };
            let parsed =
                ControlMessage::from_line(&original.to_line().expect("encode")).expect("decode");
            assert_eq!(parsed, original);
        }
    }

    #[test]
    fn an_unknown_codec_should_parse_rather_than_reject_the_line() {
        // The sink answers `unsupported_codec`; a parse failure would wrongly read as malformed.
        let line = r#"{"type":"stream_start","stream":2,"params":{"codec":"flac","sample_rate":48000,"channels":1,"frame_ms":20}}"#;
        let parsed = ControlMessage::from_line(line).expect("must parse");
        let ControlMessage::StreamStart { params, .. } = parsed else {
            panic!("expected stream_start");
        };
        assert_eq!(params.codec, AudioCodec::Unknown);
    }

    #[test]
    fn stream_params_should_tolerate_unknown_fields() {
        let line = r#"{"type":"stream_start","stream":2,"params":{"codec":"opus","sample_rate":48000,"channels":1,"frame_ms":20,"bitrate":32000}}"#;
        assert!(ControlMessage::from_line(line).is_ok());
    }

    #[test]
    fn intersect_caps_should_keep_only_shared_tokens() {
        let ours = vec!["cam".to_owned(), "mic".to_owned(), "spk".to_owned()];
        let theirs = vec!["mic".to_owned(), "spk".to_owned()];
        assert_eq!(intersect_caps(&ours, &theirs), vec!["mic", "spk"]);
    }

    #[test]
    fn intersect_caps_should_preserve_unknown_tokens_both_sides_share() {
        let ours = vec!["cam".to_owned(), "hologram".to_owned()];
        let theirs = vec!["hologram".to_owned()];
        assert_eq!(intersect_caps(&ours, &theirs), vec!["hologram"]);
    }

    #[test]
    fn intersect_caps_should_be_empty_when_nothing_is_shared() {
        let ours = vec!["cam".to_owned()];
        let theirs = vec!["spk".to_owned()];
        assert!(intersect_caps(&ours, &theirs).is_empty());
    }
}
