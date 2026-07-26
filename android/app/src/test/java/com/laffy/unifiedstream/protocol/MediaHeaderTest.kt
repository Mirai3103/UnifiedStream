package com.laffy.unifiedstream.protocol

import java.io.File
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaHeaderTest {

    private fun sample() = MediaHeader(
        stream = StreamId.CAMERA,
        sequence = 0x1234,
        timestampUs = 0xDEADBEEFL,
        sessionId = 0x0123456789ABCDEFL,
        fragment = true,
        marker = false,
    )

    @Test
    fun encodeShouldProduceExactlySixteenBytes() {
        assertEquals(HEADER_LEN, sample().encode().size)
    }

    @Test
    fun decodeShouldRecoverEveryFieldAfterEncode() {
        assertEquals(sample(), MediaHeader.decode(sample().encode()))
    }

    @Test
    fun roundTripShouldPreserveMarkerWithoutFragment() {
        val original = sample().copy(fragment = false, marker = true)
        assertEquals(original, MediaHeader.decode(original.encode()))
    }

    @Test
    fun roundTripShouldPreserveExtremeValues() {
        val original = MediaHeader(
            stream = StreamId(255),
            sequence = 0xFFFF,
            timestampUs = 0xFFFFFFFFL,
            sessionId = -1L, // all bits set: u64 maximum
            fragment = true,
            marker = true,
        )
        assertEquals(original, MediaHeader.decode(original.encode()))
    }

    @Test
    fun encodeShouldWriteVersionInTopTwoBits() {
        val first = sample().encode()[0].toInt() and 0xFF
        assertEquals(PROTOCOL_VERSION, first ushr 6)
    }

    @Test
    fun encodeShouldLeaveReservedBitsZero() {
        val first = sample().encode()[0].toInt() and 0xFF
        assertEquals(0, first and 0x0F)
    }

    @Test
    fun decodeShouldRejectUnknownVersion() {
        val encoded = sample().encode()
        encoded[0] = ((encoded[0].toInt() and 0x3F) or (2 shl 6)).toByte()
        assertNull(MediaHeader.decodeOrNull(encoded))
    }

    @Test
    fun decodeShouldRejectBufferShorterThanHeader() {
        val encoded = sample().encode()
        for (len in 0 until HEADER_LEN) {
            assertNull(
                "$len-byte datagram must be rejected",
                MediaHeader.decodeOrNull(encoded.copyOf(len)),
            )
        }
    }

    @Test
    fun decodeShouldIgnoreReservedBitsSetByAFutureVersion() {
        val original = sample()
        val encoded = original.encode()
        encoded[0] = (encoded[0].toInt() or 0x0F).toByte()
        assertEquals(original, MediaHeader.decode(encoded))
    }

    @Test
    fun payloadOfShouldSeparatePayloadFromHeader() {
        val datagram = sample().toDatagram("payload".toByteArray())
        assertEquals(sample(), MediaHeader.decode(datagram))
        assertEquals("payload", String(MediaHeader.payloadOf(datagram)))
    }

    @Test
    fun payloadOfShouldYieldEmptyPayloadForBareHeader() {
        assertEquals(0, MediaHeader.payloadOf(sample().encode()).size)
    }

    @Test
    fun sessionIdMismatchIsVisibleToTheCaller() {
        assertNotEquals(-1L, MediaHeader.decode(sample().encode()).sessionId)
    }

    @Test
    fun decodeShouldNeverThrowUncheckedOnArbitraryInput() {
        // Deterministic xorshift keeps any failure reproducible.
        var state = 0x2545F4914F6CDD1DUL
        fun next(): Int {
            state = state xor (state shl 13)
            state = state xor (state shr 7)
            state = state xor (state shl 17)
            return (state and 0xFFUL).toInt()
        }
        for (len in 0 until 64) {
            repeat(256) {
                val buf = ByteArray(len) { next().toByte() }
                // Must return, not throw anything decodeOrNull does not handle.
                MediaHeader.decodeOrNull(buf)
            }
        }
    }

    @Test
    fun decodeShouldAcceptEveryValidFlagCombination() {
        for (fragment in listOf(false, true)) {
            for (marker in listOf(false, true)) {
                val original = sample().copy(fragment = fragment, marker = marker)
                assertEquals(
                    "fragment=$fragment marker=$marker",
                    original,
                    MediaHeader.decode(original.encode()),
                )
            }
        }
    }

    @Test
    fun seqIsNewerShouldHandlePlainIncrement() {
        assertTrue(seqIsNewer(11, 10))
        assertTrue(!seqIsNewer(10, 11))
    }

    @Test
    fun seqIsNewerShouldTreatWrapAsForwardProgress() {
        assertTrue(seqIsNewer(0, 65535))
        assertTrue(!seqIsNewer(65535, 0))
    }

    @Test
    fun seqIsNewerShouldBeFalseForEqualValues() {
        assertTrue(!seqIsNewer(42, 42))
    }

    @Test
    fun seqDistanceShouldCountAcrossWrap() {
        assertEquals(1, seqDistance(65535, 0))
        assertEquals(4, seqDistance(65534, 2))
        assertEquals(4, seqDistance(10, 14))
    }

    // --- Cross-implementation vectors -------------------------------------------------------
    //
    // The same file drives the Rust suite. If these two ever disagree, the phone and the PC
    // have silently stopped speaking the same protocol.

    @Serializable
    private data class Vector(
        val name: String,
        val hex: String,
        val stream: Int,
        val sequence: Int,
        val timestamp_us: Long,
        val session_id: String,
        val fragment: Boolean,
        val marker: Boolean,
    )

    @Serializable
    private data class VectorFile(val protocol_version: Int, val header_len: Int, val vectors: List<Vector>)

    private fun loadVectors(): VectorFile {
        // Walk up from the Gradle working directory so the fixture has one home in the repo
        // rather than a copy per platform.
        var dir: File? = File(System.getProperty("user.dir") ?: ".").absoluteFile
        while (dir != null) {
            val candidate = File(dir, "testdata/media-header-vectors.json")
            if (candidate.isFile) {
                return Json { ignoreUnknownKeys = true }.decodeFromString(candidate.readText())
            }
            dir = dir.parentFile
        }
        throw AssertionError("testdata/media-header-vectors.json not found above ${System.getProperty("user.dir")}")
    }

    private fun String.hexToBytes(): ByteArray =
        chunked(2).map { it.toInt(16).toByte() }.toByteArray()

    @Test
    fun fixtureShouldMatchThisBuildsConstants() {
        val file = loadVectors()
        assertEquals(PROTOCOL_VERSION, file.protocol_version)
        assertEquals(HEADER_LEN, file.header_len)
        assertTrue("fixture must not be empty", file.vectors.isNotEmpty())
    }

    @Test
    fun everyFixtureVectorShouldDecodeToItsDeclaredFields() {
        for (v in loadVectors().vectors) {
            val decoded = MediaHeader.decode(v.hex.hexToBytes())
            assertEquals("${v.name}: stream", v.stream, decoded.stream.value)
            assertEquals("${v.name}: sequence", v.sequence, decoded.sequence)
            assertEquals("${v.name}: timestamp", v.timestamp_us, decoded.timestampUs)
            assertEquals(
                "${v.name}: session id",
                java.lang.Long.parseUnsignedLong(v.session_id),
                decoded.sessionId,
            )
            assertEquals("${v.name}: fragment", v.fragment, decoded.fragment)
            assertEquals("${v.name}: marker", v.marker, decoded.marker)
        }
    }

    @Test
    fun everyFixtureVectorShouldReEncodeToTheSameBytes() {
        for (v in loadVectors().vectors) {
            val expected = v.hex.hexToBytes()
            val reEncoded = MediaHeader.decode(expected).encode()
            assertEquals(
                "${v.name}: re-encoded bytes",
                v.hex,
                reEncoded.joinToString("") { "%02x".format(it) },
            )
        }
    }
}
