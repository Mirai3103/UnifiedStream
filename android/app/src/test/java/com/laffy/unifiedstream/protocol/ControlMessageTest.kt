package com.laffy.unifiedstream.protocol

import java.io.File
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ControlMessageTest {

    private fun hello() = ControlMessage.Hello(
        version = PROTOCOL_VERSION,
        deviceId = "8f14e45f-ceea-467a-9a3f-1b2c3d4e5f60",
        deviceName = "Pixel 8",
        caps = listOf("cam", "mic", "spk"),
    )

    @Test
    fun helloShouldRoundTripThroughALine() {
        val line = ControlCodec.toLine(hello())
        assertEquals(hello(), ControlCodec.fromLine(line))
    }

    @Test
    fun toLineShouldTerminateWithANewline() {
        assertTrue(ControlCodec.toLine(ControlMessage.Bye).endsWith("\n"))
    }

    @Test
    fun toLineShouldEmitExactlyOneLine() {
        assertEquals(1, ControlCodec.toLine(hello()).count { it == '\n' })
    }

    @Test
    fun byeShouldSerializeAsATaggedObject() {
        assertEquals("""{"type":"bye"}""", ControlCodec.toLine(ControlMessage.Bye).trim())
    }

    @Test
    fun helloShouldUseSnakeCaseFieldNamesOnTheWire() {
        val line = ControlCodec.toLine(hello())
        assertTrue("device_id must be snake_case", line.contains("\"device_id\""))
        assertTrue("device_name must be snake_case", line.contains("\"device_name\""))
        assertTrue("type tag must be present", line.contains("\"type\":\"hello\""))
    }

    @Test
    fun helloShouldOmitResumeSessionIdWhenAbsent() {
        assertTrue(!ControlCodec.toLine(hello()).contains("resume_session_id"))
    }

    @Test
    fun helloShouldCarryResumeSessionIdWhenPresent() {
        val resuming = hello().copy(resumeSessionId = 0x0123456789ABCDEFL)
        assertEquals(resuming, ControlCodec.fromLine(ControlCodec.toLine(resuming)))
    }

    @Test
    fun helloAckShouldRoundTrip() {
        val ack = ControlMessage.HelloAck(
            version = PROTOCOL_VERSION,
            deviceId = "desktop-1",
            deviceName = "cachy-desktop",
            caps = listOf("cam", "mic", "spk"),
            sessionId = -1L, // all bits set: a u64 above the signed range
            mediaPort = 47811,
        )
        assertEquals(ack, ControlCodec.fromLine(ControlCodec.toLine(ack)))
    }

    @Test
    fun pingShouldRoundTripItsTimestampExactly() {
        val ping = ControlMessage.Ping(1_721_990_400_123_456)
        assertEquals(ping, ControlCodec.fromLine(ControlCodec.toLine(ping)))
    }

    @Test
    fun telemetryShouldRoundTripANullRtt() {
        val report = ControlMessage.Telemetry()
        val parsed = ControlCodec.fromLine(ControlCodec.toLine(report))
        assertEquals(report, parsed)
        assertNull((parsed as ControlMessage.Telemetry).rttMs)
    }

    @Test
    fun telemetryShouldEmitItsNumericFieldsEvenWhenZero() {
        val line = ControlCodec.toLine(ControlMessage.Telemetry())
        assertTrue("tx_mbps must be present", line.contains("\"tx_mbps\""))
        assertTrue("loss_pct must be present", line.contains("\"loss_pct\""))
    }

    @Test
    fun errorReasonsShouldUseTheirWireTokens() {
        val error = ControlMessage.Error(ErrorReason.VERSION_MISMATCH, "nope", 1)
        val line = ControlCodec.toLine(error)
        assertTrue(line.contains("\"version_mismatch\""))
        assertEquals(error, ControlCodec.fromLine(line))
    }

    @Test
    fun fromLineShouldRejectInvalidJson() {
        assertThrows(MalformedControlException::class.java) {
            ControlCodec.fromLine("{not json")
        }
    }

    @Test
    fun fromLineShouldRejectAMessageWithoutAType() {
        assertThrows(MalformedControlException::class.java) {
            ControlCodec.fromLine("""{"version":1}""")
        }
    }

    @Test
    fun fromLineShouldRejectAnUnknownType() {
        assertThrows(MalformedControlException::class.java) {
            ControlCodec.fromLine("""{"type":"teleport"}""")
        }
    }

    @Test
    fun fromLineShouldTolerateCrlfTermination() {
        assertEquals(ControlMessage.Bye, ControlCodec.fromLine("{\"type\":\"bye\"}\r\n"))
    }

    @Test
    fun fromLineShouldIgnoreUnknownFieldsFromANewerPeer() {
        val line = """{"type":"bye","future_field":42}"""
        assertEquals(ControlMessage.Bye, ControlCodec.fromLine(line))
    }

    @Test
    fun fromLineOrNullShouldNeverThrowOnArbitraryText() {
        for (line in listOf("", " ", "null", "[]", "0", "\"bye\"", "{}", """{"type":null}""")) {
            assertNull(ControlCodec.fromLineOrNull(line))
        }
    }

    @Test
    fun malformedShouldBeTheOnlyNonFatalReason() {
        assertTrue(!ErrorReason.MALFORMED.isFatal)
        for (reason in listOf(
            ErrorReason.VERSION_MISMATCH,
            ErrorReason.REJECTED,
            ErrorReason.BUSY,
            ErrorReason.INTERNAL,
        )) {
            assertTrue("$reason must close the connection", reason.isFatal)
        }
    }

    // --- Stream lifecycle --------------------------------------------------------------------

    @Test
    fun streamStartShouldRoundTripThroughALine() {
        val start = ControlMessage.StreamStart(stream = 2, params = AudioParams())
        assertEquals(start, ControlCodec.fromLine(ControlCodec.toLine(start)))
    }

    @Test
    fun streamStartShouldSerializeTheDocumentedShape() {
        val line = ControlCodec.toLine(ControlMessage.StreamStart(stream = 2, params = AudioParams()))
        assertEquals(
            """{"type":"stream_start","stream":2,"params":{"codec":"pcm_s16le","sample_rate":48000,"channels":1,"frame_ms":20}}""",
            line.trim(),
        )
    }

    @Test
    fun anAcceptingStreamAckShouldOmitTheReason() {
        val line = ControlCodec.toLine(ControlMessage.StreamAck(stream = 2, accepted = true))
        assertEquals("""{"type":"stream_ack","stream":2,"accepted":true}""", line.trim())
    }

    @Test
    fun aRefusingStreamAckShouldCarryItsReason() {
        val refusal = ControlMessage.StreamAck(
            stream = 2,
            accepted = false,
            reason = StreamRefusal.UNSUPPORTED_CODEC,
        )
        val line = ControlCodec.toLine(refusal)
        assertTrue(line.contains("\"unsupported_codec\""))
        assertEquals(refusal, ControlCodec.fromLine(line))
    }

    @Test
    fun streamStopAndRequestShouldRoundTrip() {
        for (message in listOf(
            ControlMessage.StreamStop(stream = 2),
            ControlMessage.StreamRequest(stream = 2, active = true),
            ControlMessage.StreamRequest(stream = 2, active = false),
        )) {
            assertEquals(message, ControlCodec.fromLine(ControlCodec.toLine(message)))
        }
    }

    @Test
    fun anUnknownCodecShouldParseRatherThanRejectTheLine() {
        val line =
            """{"type":"stream_start","stream":2,"params":{"codec":"flac","sample_rate":48000,"channels":1,"frame_ms":20}}"""
        val parsed = ControlCodec.fromLine(line) as ControlMessage.StreamStart
        assertEquals(AudioCodec.UNKNOWN, parsed.params.codec)
    }

    @Test
    fun anUnknownRefusalReasonShouldParseRatherThanRejectTheLine() {
        // A newer desktop may add reasons; the phone must still see the refusal.
        val line = """{"type":"stream_ack","stream":2,"accepted":false,"reason":"quota_exceeded"}"""
        val parsed = ControlCodec.fromLine(line) as ControlMessage.StreamAck
        assertEquals(StreamRefusal.UNKNOWN, parsed.reason)
    }

    @Test
    fun streamParamsShouldTolerateUnknownFields() {
        val line =
            """{"type":"stream_start","stream":2,"params":{"codec":"opus","sample_rate":48000,"channels":1,"frame_ms":20,"bitrate":32000}}"""
        val parsed = ControlCodec.fromLine(line) as ControlMessage.StreamStart
        assertEquals(AudioCodec.OPUS, parsed.params.codec)
    }

    // --- Interop with real desktop output ---------------------------------------------------

    @Serializable
    private data class ControlLines(
        val hello_ack_high_session_id: String,
        val expected: Expected,
        val stream_start_mic_pcm: String,
        val stream_start_speaker_pcm: String,
        val stream_request_speaker_start: String,
        val stream_ack_accepted: String,
        val stream_ack_refused: String,
        val stream_stop: String,
        val stream_request_start: String,
    ) {
        @Serializable
        data class Expected(
            val session_id_decimal: String,
            val session_id_as_signed_long_bits: String,
            val media_port: Int,
        )
    }

    private fun loadControlLines(): ControlLines {
        var dir: File? = File(System.getProperty("user.dir") ?: ".").absoluteFile
        while (dir != null) {
            val candidate = File(dir, "testdata/control-lines.json")
            if (candidate.isFile) {
                return Json { ignoreUnknownKeys = true }.decodeFromString(candidate.readText())
            }
            dir = dir.parentFile
        }
        throw AssertionError("testdata/control-lines.json not found")
    }

    @Test
    fun aRealDesktopHelloAckWithAHighSessionIdShouldParse() {
        // Roughly half of all session ids exceed Long.MAX_VALUE. Parsing those as JSON numbers
        // threw, so the phone never saw the ack and retried forever.
        val fixture = loadControlLines()
        val parsed = ControlCodec.fromLine(fixture.hello_ack_high_session_id)

        assertTrue(parsed is ControlMessage.HelloAck)
        val ack = parsed as ControlMessage.HelloAck
        assertEquals(fixture.expected.media_port, ack.mediaPort)
        assertEquals(
            fixture.expected.session_id_as_signed_long_bits.toLong(),
            ack.sessionId,
        )
        assertEquals(
            fixture.expected.session_id_decimal,
            ack.sessionId.toULong().toString(),
        )
    }

    @Test
    fun aHighSessionIdShouldReEncodeAsTheSameUnsignedDecimal() {
        val fixture = loadControlLines()
        val parsed = ControlCodec.fromLine(fixture.hello_ack_high_session_id)
        assertEquals(fixture.hello_ack_high_session_id, ControlCodec.toLine(parsed).trim())
    }

    @Test
    fun everySessionIdBitPatternShouldSurviveARoundTrip() {
        for (sessionId in listOf(0L, 1L, -1L, Long.MAX_VALUE, Long.MIN_VALUE, 13597845101738855385uL.toLong())) {
            val ack = ControlMessage.HelloAck(
                version = PROTOCOL_VERSION,
                deviceId = "d",
                deviceName = "n",
                caps = listOf("cam"),
                sessionId = sessionId,
                mediaPort = 47811,
            )
            val parsed = ControlCodec.fromLine(ControlCodec.toLine(ack))
            assertEquals("session id ${sessionId.toULong()}", ack, parsed)
        }
    }

    @Test
    fun aResumeSessionIdAboveTheSignedRangeShouldRoundTrip() {
        val resuming = hello().copy(resumeSessionId = 13597845101738855385uL.toLong())
        val parsed = ControlCodec.fromLine(ControlCodec.toLine(resuming))
        assertEquals(ControlMessage.Hello::class, parsed::class)
        assertEquals(resuming.resumeSessionId, (parsed as ControlMessage.Hello).resumeSessionId)
    }

    @Test
    fun realDesktopStreamLifecycleLinesShouldParseAndReEncodeIdentically() {
        val fixture = loadControlLines()
        val expected = mapOf(
            fixture.stream_start_mic_pcm to
                ControlMessage.StreamStart(stream = 2, params = AudioParams()),
            fixture.stream_start_speaker_pcm to
                ControlMessage.StreamStart(stream = 3, params = AudioParams(channels = 2)),
            fixture.stream_request_speaker_start to
                ControlMessage.StreamRequest(stream = 3, active = true),
            fixture.stream_ack_accepted to
                ControlMessage.StreamAck(stream = 2, accepted = true),
            fixture.stream_ack_refused to
                ControlMessage.StreamAck(
                    stream = 2,
                    accepted = false,
                    reason = StreamRefusal.UNSUPPORTED_CODEC,
                ),
            fixture.stream_stop to ControlMessage.StreamStop(stream = 2),
            fixture.stream_request_start to
                ControlMessage.StreamRequest(stream = 2, active = true),
        )
        for ((line, message) in expected) {
            assertEquals(message, ControlCodec.fromLine(line))
            assertEquals(line, ControlCodec.toLine(message).trim())
        }
    }

    @Test
    fun intersectCapsShouldKeepOnlySharedTokens() {
        assertEquals(
            listOf("mic", "spk"),
            intersectCaps(listOf("cam", "mic", "spk"), listOf("mic", "spk")),
        )
    }

    @Test
    fun intersectCapsShouldPreserveUnknownTokensBothSidesShare() {
        assertEquals(
            listOf("hologram"),
            intersectCaps(listOf("cam", "hologram"), listOf("hologram")),
        )
    }

    @Test
    fun intersectCapsShouldBeEmptyWhenNothingIsShared() {
        assertTrue(intersectCaps(listOf("cam"), listOf("spk")).isEmpty())
    }
}
