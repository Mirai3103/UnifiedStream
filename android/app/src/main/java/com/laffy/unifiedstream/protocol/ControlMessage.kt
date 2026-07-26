package com.laffy.unifiedstream.protocol

import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonContentPolymorphicSerializer
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

/**
 * Reads and writes a 64-bit session id carried as a decimal string.
 *
 * The id uses the full unsigned 64-bit range, so roughly half of all values exceed
 * [Long.MAX_VALUE]. Parsed as a JSON number those overflow and the message is rejected — which
 * silently broke every second handshake. As text the value survives both this reader and the
 * desktop UI's IEEE-double numbers.
 */
object SessionIdSerializer : KSerializer<Long> {
    override val descriptor: SerialDescriptor =
        PrimitiveSerialDescriptor("SessionId", PrimitiveKind.STRING)

    override fun serialize(encoder: Encoder, value: Long) {
        encoder.encodeString(value.toULong().toString())
    }

    override fun deserialize(decoder: Decoder): Long =
        decoder.decodeString().toULong().toLong()
}

/**
 * Control-channel messages: newline-delimited JSON over TCP.
 *
 * Mirrors `unifiedstream_net::protocol::messages`. Normative in
 * `openspec/specs/protocol.md` section 3.
 */
@Serializable
sealed interface ControlMessage {

    /** Opens the handshake, phone to desktop. */
    @Serializable
    @SerialName("hello")
    data class Hello(
        val version: Int = PROTOCOL_VERSION,
        @SerialName("device_id") val deviceId: String,
        @SerialName("device_name") val deviceName: String,
        val caps: List<String>,
        @SerialName("resume_session_id")
        @Serializable(with = SessionIdSerializer::class)
        val resumeSessionId: Long? = null,
        /**
         * UDP port this phone bound for media.
         *
         * The desktop pairs this with our TCP address so it can send without waiting to learn
         * the address from an inbound datagram.
         */
        @SerialName("media_port") val mediaPort: Int? = null,
    ) : ControlMessage

    /** Accepts the handshake and establishes a session, desktop to phone. */
    @Serializable
    @SerialName("hello_ack")
    data class HelloAck(
        val version: Int,
        @SerialName("device_id") val deviceId: String,
        @SerialName("device_name") val deviceName: String,
        val caps: List<String>,
        @SerialName("session_id")
        @Serializable(with = SessionIdSerializer::class)
        val sessionId: Long,
        @SerialName("media_port") val mediaPort: Int,
    ) : ControlMessage

    /** Reports a failure. */
    @Serializable
    @SerialName("error")
    data class Error(
        val reason: ErrorReason,
        val message: String = "",
        @SerialName("supported_version") val supportedVersion: Int? = null,
    ) : ControlMessage

    /** Heartbeat probe. [timestamp] is the sender's monotonic microseconds. */
    @Serializable
    @SerialName("ping")
    data class Ping(val timestamp: Long) : ControlMessage

    /** Heartbeat reply, echoing the probe's timestamp verbatim. */
    @Serializable
    @SerialName("pong")
    data class Pong(val timestamp: Long) : ControlMessage

    /** Periodic link-quality report. */
    @Serializable
    @SerialName("telemetry")
    data class Telemetry(
        @SerialName("rtt_ms") val rttMs: Double? = null,
        @SerialName("tx_mbps") val txMbps: Double = 0.0,
        @SerialName("rx_mbps") val rxMbps: Double = 0.0,
        @SerialName("loss_pct") val lossPct: Double = 0.0,
        @SerialName("jitter_ms") val jitterMs: Double = 0.0,
    ) : ControlMessage

    /** Source announces a stream it wants to send, protocol §3.9.1. */
    @Serializable
    @SerialName("stream_start")
    data class StreamStart(
        val stream: Int,
        val params: StreamParams,
    ) : ControlMessage

    /** Sink accepts or refuses a [StreamStart], protocol §3.9.2. */
    @Serializable
    @SerialName("stream_ack")
    data class StreamAck(
        val stream: Int,
        val accepted: Boolean,
        /** Why not. Present exactly when [accepted] is false. */
        val reason: StreamRefusal? = null,
    ) : ControlMessage

    /** Either side ends a stream, protocol §3.9.3. */
    @Serializable
    @SerialName("stream_stop")
    data class StreamStop(val stream: Int) : ControlMessage

    /** Sink asks the source to start or stop a stream, protocol §3.9.4. */
    @Serializable
    @SerialName("stream_request")
    data class StreamRequest(
        val stream: Int,
        /** `true` to request a start, `false` a stop. */
        val active: Boolean,
    ) : ControlMessage

    /** Clean session teardown. */
    @Serializable
    @SerialName("bye")
    data object Bye : ControlMessage
}

/**
 * Audio codec named in stream parameters.
 *
 * Unknown wire tokens parse as [UNKNOWN] rather than failing the line, so the sink can answer
 * with an `unsupported_codec` refusal instead of treating the message as malformed.
 */
@Serializable(with = AudioCodecSerializer::class)
enum class AudioCodec(val wire: String) {
    /** Raw signed 16-bit little-endian samples. The mandatory baseline every peer decodes. */
    PCM_S16LE("pcm_s16le"),

    /** One Opus packet per frame. Offered only when the source has a working encoder. */
    OPUS("opus"),

    /** A codec this build does not know. */
    UNKNOWN("unknown");

    companion object {
        fun fromWire(token: String): AudioCodec =
            entries.firstOrNull { it.wire == token } ?: UNKNOWN
    }
}

/** Maps [AudioCodec] to its wire token, tolerating tokens from a newer peer. */
object AudioCodecSerializer : KSerializer<AudioCodec> {
    override val descriptor: SerialDescriptor =
        PrimitiveSerialDescriptor("AudioCodec", PrimitiveKind.STRING)

    override fun serialize(encoder: Encoder, value: AudioCodec) {
        encoder.encodeString(value.wire)
    }

    override fun deserialize(decoder: Decoder): AudioCodec =
        AudioCodec.fromWire(decoder.decodeString())
}

/**
 * Parameters carried by a `stream_start`: audio fields (§3.9.1) or video fields (§7.1).
 *
 * The wire carries a bare object either way — the kinds are untagged — so deserialization
 * keys on the shape: a `width` field means video. Mirrors `StreamParams` on the Rust side.
 */
@Serializable(with = StreamParamsSerializer::class)
sealed interface StreamParams

/** Picks the concrete parameter type from the JSON shape. */
object StreamParamsSerializer : JsonContentPolymorphicSerializer<StreamParams>(StreamParams::class) {
    override fun selectDeserializer(element: JsonElement): DeserializationStrategy<StreamParams> =
        if (element is JsonObject && "width" in element) {
            VideoParams.serializer()
        } else {
            AudioParams.serializer()
        }
}

/**
 * Format parameters for an audio stream, protocol §3.9.1.
 *
 * Defaults are the microphone baseline: PCM S16LE, 48 kHz, mono, 20 ms frames.
 */
@Serializable
data class AudioParams(
    val codec: AudioCodec = AudioCodec.PCM_S16LE,
    @SerialName("sample_rate") val sampleRate: Int = 48_000,
    val channels: Int = 1,
    @SerialName("frame_ms") val frameMs: Int = 20,
) : StreamParams

/**
 * Video codec named in stream parameters.
 *
 * Unknown wire tokens parse as [UNKNOWN] rather than failing the line, so the sink can answer
 * with an `unsupported_codec` refusal instead of treating the message as malformed.
 */
@Serializable(with = VideoCodecSerializer::class)
enum class VideoCodec(val wire: String) {
    /**
     * One complete JPEG image per frame. The mandatory baseline every peer decodes: every
     * frame is independently decodable, so loss never corrupts later frames.
     */
    MJPEG("mjpeg"),

    /** A codec this build does not know. */
    UNKNOWN("unknown");

    companion object {
        fun fromWire(token: String): VideoCodec =
            entries.firstOrNull { it.wire == token } ?: UNKNOWN
    }
}

/** Maps [VideoCodec] to its wire token, tolerating tokens from a newer peer. */
object VideoCodecSerializer : KSerializer<VideoCodec> {
    override val descriptor: SerialDescriptor =
        PrimitiveSerialDescriptor("VideoCodec", PrimitiveKind.STRING)

    override fun serialize(encoder: Encoder, value: VideoCodec) {
        encoder.encodeString(value.wire)
    }

    override fun deserialize(decoder: Decoder): VideoCodec =
        VideoCodec.fromWire(decoder.decodeString())
}

/**
 * Format parameters for a video stream, protocol §7.1.
 *
 * Defaults are the camera baseline: MJPEG at 1280x720, up to 30 fps.
 */
@Serializable
data class VideoParams(
    val codec: VideoCodec = VideoCodec.MJPEG,
    val width: Int = 1280,
    val height: Int = 720,
    @SerialName("max_fps") val maxFps: Int = 30,
) : StreamParams

/**
 * Why a `stream_start` or `stream_request` was refused.
 *
 * Unknown wire tokens parse as [UNKNOWN] so a newer desktop can add reasons without breaking
 * this phone.
 */
@Serializable(with = StreamRefusalSerializer::class)
enum class StreamRefusal(val wire: String) {
    /** The stream's capability token is not in the negotiated set. */
    NOT_NEGOTIATED("not_negotiated"),

    /** The sink cannot decode the offered codec. */
    UNSUPPORTED_CODEC("unsupported_codec"),

    /** The sink does not recognise the stream identifier. */
    UNSUPPORTED_STREAM("unsupported_stream"),

    /** The sink cannot take another stream right now. */
    BUSY("busy"),

    /** The sink failed locally — e.g. its audio system is unavailable. */
    INTERNAL("internal"),

    /** A reason this build does not know. */
    UNKNOWN("unknown");

    companion object {
        fun fromWire(token: String): StreamRefusal =
            entries.firstOrNull { it.wire == token } ?: UNKNOWN
    }
}

/** Maps [StreamRefusal] to its wire token, tolerating tokens from a newer peer. */
object StreamRefusalSerializer : KSerializer<StreamRefusal> {
    override val descriptor: SerialDescriptor =
        PrimitiveSerialDescriptor("StreamRefusal", PrimitiveKind.STRING)

    override fun serialize(encoder: Encoder, value: StreamRefusal) {
        encoder.encodeString(value.wire)
    }

    override fun deserialize(decoder: Decoder): StreamRefusal =
        StreamRefusal.fromWire(decoder.decodeString())
}

/** Why a control exchange failed. */
@Serializable
enum class ErrorReason {
    /** Peer's protocol version is not supported. Connection closes. */
    @SerialName("version_mismatch")
    VERSION_MISMATCH,

    /** User declined pairing, or the prompt timed out. Connection closes. */
    @SerialName("rejected")
    REJECTED,

    /** A session is already active. Connection closes. */
    @SerialName("busy")
    BUSY,

    /** Unparseable or type-less line. Connection stays open. */
    @SerialName("malformed")
    MALFORMED,

    /** Unexpected local failure. Connection closes. */
    @SerialName("internal")
    INTERNAL;

    /**
     * Whether receiving this reason means the control connection is being closed.
     *
     * [MALFORMED] is the one recoverable case: a single bad line does not end a session.
     */
    val isFatal: Boolean get() = this != MALFORMED
}

/** Codec for the control channel's newline-delimited JSON framing. */
object ControlCodec {

    /**
     * `explicitNulls = false` so an absent `resume_session_id` is omitted rather than sent as
     * null; `encodeDefaults = true` so a telemetry report of all zeroes still carries its
     * fields. `ignoreUnknownKeys` is the forward-compatibility hinge for later protocol
     * versions.
     */
    val json: Json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
        explicitNulls = false
        classDiscriminator = "type"
    }

    /** Serialize to a single newline-terminated JSON line. */
    fun toLine(message: ControlMessage): String =
        json.encodeToString(ControlMessage.serializer(), message) + "\n"

    /**
     * Parse one line received from the control channel.
     *
     * @throws MalformedControlException when the line is not valid JSON, lacks a `type`, or
     *   carries a `type` this build does not know.
     */
    fun fromLine(line: String): ControlMessage {
        val trimmed = line.trim()
        if (trimmed.isEmpty()) throw MalformedControlException("empty line")
        return try {
            json.decodeFromString(ControlMessage.serializer(), trimmed)
        } catch (e: Exception) {
            throw MalformedControlException(e.message ?: "unparseable control message")
        }
    }

    /** Parse a line, returning null instead of throwing. */
    fun fromLineOrNull(line: String): ControlMessage? =
        try {
            fromLine(line)
        } catch (_: MalformedControlException) {
            null
        }
}

/** Raised when a control line cannot be parsed. */
class MalformedControlException(message: String) : Exception(message)

/**
 * Capability tokens present in both lists, in the order the local side lists them.
 *
 * Unknown tokens survive intersection — if both peers name the same future capability, they
 * agree on it without this build needing to know what it is.
 */
fun intersectCaps(ours: List<String>, theirs: List<String>): List<String> =
    ours.filter { it in theirs }
