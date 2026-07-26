package com.laffy.unifiedstream.transport

import com.laffy.unifiedstream.protocol.MAX_PAYLOAD
import com.laffy.unifiedstream.protocol.StreamId
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TestStreamTest {

    @Test
    fun aGeneratedFrameShouldVerify() {
        val generator = TestStreamGenerator()
        val verifier = TestStreamVerifier()

        assertEquals(VerifyResult.Ok(0), verifier.verify(generator.nextFrame()))
        assertEquals(1, verifier.report.verified)
    }

    @Test
    fun framesShouldCarryAnIncreasingIndex() {
        val generator = TestStreamGenerator()
        val verifier = TestStreamVerifier()

        for (expected in 0 until 10) {
            assertEquals(VerifyResult.Ok(expected), verifier.verify(generator.nextFrame()))
        }
        assertTrue(verifier.report.isClean)
    }

    @Test
    fun aGeneratedFrameShouldHaveTheConfiguredSize() {
        val generator = TestStreamGenerator(TestStreamConfig(rateHz = 30, frameBytes = 1_500))
        assertEquals(1_500, generator.nextFrame().size)
    }

    @Test
    fun aCorruptedByteShouldFailVerification() {
        val generator = TestStreamGenerator()
        val verifier = TestStreamVerifier()

        val frame = generator.nextFrame()
        frame[100] = (frame[100] + 1).toByte()

        assertEquals(
            VerifyResult.Failed(IntegrityError.PAYLOAD_CORRUPT),
            verifier.verify(frame),
        )
        assertEquals(1, verifier.report.corrupt)
    }

    @Test
    fun aTruncatedFrameShouldFailVerification() {
        val generator = TestStreamGenerator()
        val verifier = TestStreamVerifier()

        val frame = generator.nextFrame()
        assertEquals(
            VerifyResult.Failed(IntegrityError.LENGTH_MISMATCH),
            verifier.verify(frame.copyOf(frame.size - 1)),
        )
    }

    @Test
    fun aFrameShorterThanThePreambleShouldFailVerification() {
        assertEquals(
            VerifyResult.Failed(IntegrityError.TOO_SHORT),
            TestStreamVerifier().verify(byteArrayOf(1, 2, 3)),
        )
    }

    @Test
    fun mislabellingAFramesPayloadShouldBeDetected() {
        val verifier = TestStreamVerifier()
        // Frame 5's bytes labelled as frame 6: what a mis-reassembled frame looks like.
        val frame = buildTestFrame(5, 512)
        ByteBuffer.wrap(frame).order(ByteOrder.BIG_ENDIAN).putInt(6)

        assertEquals(
            VerifyResult.Failed(IntegrityError.PAYLOAD_CORRUPT),
            verifier.verify(frame),
        )
    }

    @Test
    fun aMissingFrameShouldBeCounted() {
        val verifier = TestStreamVerifier()
        verifier.verify(buildTestFrame(0, 256))
        verifier.verify(buildTestFrame(3, 256))

        assertEquals(2, verifier.report.missing)
        assertTrue(!verifier.report.isClean)
    }

    @Test
    fun aReplayedFrameShouldBeCountedOutOfOrder() {
        val verifier = TestStreamVerifier()
        verifier.verify(buildTestFrame(0, 256))
        verifier.verify(buildTestFrame(1, 256))
        verifier.verify(buildTestFrame(0, 256))

        assertEquals(1, verifier.report.outOfOrder)
        assertEquals(0, verifier.report.missing)
    }

    @Test
    fun joiningAStreamLateShouldNotCountEarlierFramesAsMissing() {
        val verifier = TestStreamVerifier()
        verifier.verify(buildTestFrame(500, 256))
        assertEquals(0, verifier.report.missing)
    }

    @Test
    fun theDefaultConfigShouldForceFragmentation() {
        assertTrue(
            "the default test stream must exercise the fragmentation path",
            TestStreamConfig().frameBytes > MAX_PAYLOAD,
        )
        assertTrue(TestStreamConfig().fragments)
    }

    @Test
    fun theIntervalShouldFollowTheConfiguredRate() {
        assertEquals(10L, TestStreamConfig(rateHz = 100, frameBytes = 256).intervalMs)
    }

    @Test
    fun aZeroRateShouldNotDivideByZero() {
        assertEquals(1_000L, TestStreamConfig(rateHz = 0, frameBytes = 256).intervalMs)
    }

    @Test
    fun theNominalBitrateShouldMatchTheConfiguration() {
        // 1250 bytes * 8 bits * 100 Hz = 1 Mbps
        assertEquals(1.0, TestStreamConfig(rateHz = 100, frameBytes = 1_250).nominalMbps, 1e-9)
    }

    @Test
    fun aFragmentedTestFrameShouldVerifyAfterTheFullTransportRoundTrip() {
        val generator = TestStreamGenerator()
        val verifier = TestStreamVerifier()
        val tx = MediaSender(7)
        val demux = MediaDemux(7).apply { register(StreamId.TEST) }

        repeat(5) {
            for (datagram in tx.frame(StreamId.TEST, generator.nextFrame())) {
                val result = demux.accept(datagram)
                if (result is DemuxResult.Frames) {
                    for (received in result.frames) {
                        assertTrue(verifier.verify(received.payload) is VerifyResult.Ok)
                    }
                }
            }
        }

        assertEquals(5, verifier.report.verified)
        assertTrue(verifier.report.isClean)
    }

    @Test
    fun aDroppedFragmentShouldSurfaceAsAMissingFrameNotACorruptOne() {
        val generator = TestStreamGenerator()
        val verifier = TestStreamVerifier()
        val tx = MediaSender(7)
        val demux = MediaDemux(7).apply { register(StreamId.TEST) }

        for (index in 0 until 4) {
            val datagrams = tx.frame(StreamId.TEST, generator.nextFrame())
            datagrams.forEachIndexed { position, datagram ->
                if (index == 1 && position == 1) return@forEachIndexed
                val result = demux.accept(datagram)
                if (result is DemuxResult.Frames) {
                    result.frames.forEach { verifier.verify(it.payload) }
                }
            }
        }

        assertEquals("a lost fragment must not look like corruption", 0, verifier.report.corrupt)
        assertTrue("the lost frame must be reported missing", verifier.report.missing >= 1)
    }

    // --- Cross-implementation fixture ----------------------------------------------------
    //
    // The same file drives the Rust suite. If the two generators drift, a frame built on the
    // desktop fails integrity checks on the phone.

    @Serializable
    private data class FrameVector(val index: Int, val size: Int, val hex: String)

    @Serializable
    private data class FrameFile(val preamble_bytes: Int, val frames: List<FrameVector>)

    private fun loadFrames(): FrameFile {
        var dir: File? = File(System.getProperty("user.dir") ?: ".").absoluteFile
        while (dir != null) {
            val candidate = File(dir, "testdata/test-stream-frames.json")
            if (candidate.isFile) {
                return Json { ignoreUnknownKeys = true }.decodeFromString(candidate.readText())
            }
            dir = dir.parentFile
        }
        throw AssertionError("testdata/test-stream-frames.json not found")
    }

    private fun String.hexToBytes(): ByteArray =
        chunked(2).map { it.toInt(16).toByte() }.toByteArray()

    @Test
    fun theGeneratorShouldReproduceEveryFixtureFrameByteForByte() {
        val file = loadFrames()
        assertEquals(8, file.preamble_bytes)
        assertTrue("fixture must not be empty", file.frames.isNotEmpty())

        for (vector in file.frames) {
            val built = buildTestFrame(vector.index, vector.size)
            assertEquals(
                "frame index ${vector.index} size ${vector.size}",
                vector.hex,
                built.joinToString("") { "%02x".format(it) },
            )
        }
    }

    @Test
    fun theVerifierShouldAcceptEveryFixtureFrame() {
        for (vector in loadFrames().frames) {
            val verifier = TestStreamVerifier()
            assertEquals(
                "frame index ${vector.index} must verify",
                VerifyResult.Ok(vector.index),
                verifier.verify(vector.hex.hexToBytes()),
            )
        }
    }
}
