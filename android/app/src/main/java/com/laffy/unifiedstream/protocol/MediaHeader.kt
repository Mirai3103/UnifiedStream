package com.laffy.unifiedstream.protocol

import java.nio.BufferUnderflowException
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Protocol version implemented by this build. */
const val PROTOCOL_VERSION: Int = 1

/** Serialized size of a media header, in bytes. */
const val HEADER_LEN: Int = 16

/**
 * Maximum payload bytes per datagram. Frames larger than this are fragmented.
 *
 * Conservative for Wi-Fi: leaves room for IPv6 and any tunneling without hitting IP-level
 * fragmentation, where a single lost IP fragment silently destroys the whole datagram.
 */
const val MAX_PAYLOAD: Int = 1200

/** Default TCP port for the control channel. */
const val DEFAULT_CONTROL_PORT: Int = 47810

/** Default UDP port for the media transport. */
const val DEFAULT_MEDIA_PORT: Int = 47811

/** mDNS service type the desktop advertises and the phone browses. */
const val SERVICE_TYPE: String = "_unifiedstream._udp"

/** TXT record keys carried in the service advertisement. */
object TxtKeys {
    const val VERSION: String = "ver"
    const val NAME: String = "name"
    const val ID: String = "id"
    const val CAPS: String = "caps"
}

/** Capability tokens a peer may advertise. */
object Caps {
    const val CAMERA: String = "cam"
    const val MICROPHONE: String = "mic"
    const val SPEAKER: String = "spk"

    val ALL: List<String> = listOf(CAMERA, MICROPHONE, SPEAKER)
}

/**
 * Logical stream carried over the shared media socket.
 *
 * One UDP socket serves camera, microphone, and speaker; this discriminates them.
 */
@JvmInline
value class StreamId(val value: Int) {
    init {
        require(value in 0..255) { "stream id must fit in one byte, got $value" }
    }

    override fun toString(): String = when (this) {
        TEST -> "test"
        CAMERA -> "camera"
        MICROPHONE -> "microphone"
        SPEAKER -> "speaker"
        else -> "stream($value)"
    }

    companion object {
        /** Synthetic verification stream. Carries no real media. */
        val TEST = StreamId(0)

        /** Camera video, phone to PC. Reserved for a later change. */
        val CAMERA = StreamId(1)

        /** Microphone audio, phone to PC. Reserved for a later change. */
        val MICROPHONE = StreamId(2)

        /** System audio, PC to phone. Reserved for a later change. */
        val SPEAKER = StreamId(3)
    }
}

/** Raised when a datagram cannot be parsed. Never escapes the receive loop. */
class MalformedPacketException(message: String) : Exception(message)

/**
 * Parsed media packet header.
 *
 * Mirrors `unifiedstream_net::protocol::MediaHeader`; both are checked against
 * `testdata/media-header-vectors.json`. Layout is normative in `openspec/specs/protocol.md`.
 *
 * Unsigned wire fields are widened because Kotlin's [Int] is signed: [sequence] holds a `u16`
 * in `0..65535`, [timestampUs] holds a `u32` in `0..4294967295`. [sessionId] keeps the `u64`
 * in a [Long]'s raw bits — every use is equality or round-trip, never ordering.
 */
data class MediaHeader(
    val stream: StreamId,
    val sequence: Int,
    val timestampUs: Long,
    val sessionId: Long,
    val fragment: Boolean,
    val marker: Boolean,
) {
    init {
        require(sequence in 0..0xFFFF) { "sequence must fit in u16, got $sequence" }
        require(timestampUs in 0..0xFFFFFFFFL) { "timestamp must fit in u32, got $timestampUs" }
    }

    /** Serialize to wire format: exactly [HEADER_LEN] big-endian bytes. */
    fun encode(): ByteArray {
        var flags = PROTOCOL_VERSION shl 6
        if (fragment) flags = flags or (1 shl 5)
        if (marker) flags = flags or (1 shl 4)
        // Bits 3-0 stay zero: reserved for a future version to claim.

        return ByteBuffer.allocate(HEADER_LEN).order(ByteOrder.BIG_ENDIAN).apply {
            put(flags.toByte())
            put(stream.value.toByte())
            putShort(sequence.toShort())
            putInt(timestampUs.toInt())
            putLong(sessionId)
        }.array()
    }

    /** Write this header followed by [payload] into a single datagram buffer. */
    fun toDatagram(payload: ByteArray): ByteArray =
        encode() + payload

    companion object {
        /**
         * Parse a header from the front of a datagram.
         *
         * @throws MalformedPacketException if [buf] is shorter than [HEADER_LEN] over [length],
         *   or the version field is not [PROTOCOL_VERSION].
         */
        fun decode(buf: ByteArray, length: Int = buf.size): MediaHeader {
            if (length < HEADER_LEN || length > buf.size) {
                throw MalformedPacketException("datagram shorter than header: $length bytes")
            }

            val b = ByteBuffer.wrap(buf, 0, HEADER_LEN).order(ByteOrder.BIG_ENDIAN)
            try {
                val flags = b.get().toInt() and 0xFF
                val version = flags ushr 6
                if (version != PROTOCOL_VERSION) {
                    throw MalformedPacketException("unsupported protocol version: $version")
                }
                val stream = b.get().toInt() and 0xFF
                val sequence = b.short.toInt() and 0xFFFF
                val timestampUs = b.int.toLong() and 0xFFFFFFFFL
                val sessionId = b.long

                return MediaHeader(
                    stream = StreamId(stream),
                    sequence = sequence,
                    timestampUs = timestampUs,
                    sessionId = sessionId,
                    fragment = flags and (1 shl 5) != 0,
                    marker = flags and (1 shl 4) != 0,
                )
            } catch (e: BufferUnderflowException) {
                // Guarded by the length check above; converted rather than propagated so the
                // receive loop only ever has to catch one exception type.
                throw MalformedPacketException("truncated header: ${e.message}")
            }
        }

        /** Parse a header, returning null instead of throwing. Used by the receive loop. */
        fun decodeOrNull(buf: ByteArray, length: Int = buf.size): MediaHeader? =
            try {
                decode(buf, length)
            } catch (_: MalformedPacketException) {
                null
            } catch (_: IllegalArgumentException) {
                // StreamId / field range violations reduce to "this datagram is not for us".
                null
            }

        /** Copy the payload that follows the header in a datagram of [length] bytes. */
        fun payloadOf(buf: ByteArray, length: Int = buf.size): ByteArray {
            if (length < HEADER_LEN || length > buf.size) {
                throw MalformedPacketException("datagram shorter than header: $length bytes")
            }
            return buf.copyOfRange(HEADER_LEN, length)
        }
    }
}

/**
 * True when sequence [a] is newer than [b] under RFC 1982 serial-number arithmetic.
 *
 * This is what keeps a wrap from 65535 to 0 from being read as a 65535-packet loss.
 */
fun seqIsNewer(a: Int, b: Int): Boolean {
    val delta = (a - b) and 0xFFFF
    return delta != 0 && delta < 0x8000
}

/**
 * Forward distance from [from] to [to], accounting for wrap.
 *
 * Used to size loss gaps: `seqDistance(lastSeen, arrived) - 1` packets went missing.
 */
fun seqDistance(from: Int, to: Int): Int = (to - from) and 0xFFFF
