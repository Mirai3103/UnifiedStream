package com.laffy.unifiedstream.protocol

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.Json

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

    /** Clean session teardown. */
    @Serializable
    @SerialName("bye")
    data object Bye : ControlMessage
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
