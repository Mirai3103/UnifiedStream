package com.laffy.unifiedstream.transport

import com.laffy.unifiedstream.protocol.MAX_PAYLOAD
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * The synthetic test stream.
 *
 * Exercises the whole transport — framing, fragmentation, sequencing, reassembly — end to end
 * without any media codec, so the link can be proven before camera or audio work exists.
 *
 * Each frame carries an 8-byte preamble (a 32-bit frame counter and a 32-bit length) followed
 * by a deterministic pattern derived from the counter, so a frame assembled from the wrong
 * fragments is detected rather than accepted. Mirrors
 * `unifiedstream_net::transport::synthetic`.
 */
private const val PREAMBLE = 8

/** How a synthetic frame should be generated. */
data class TestStreamConfig(
    /** Frames per second. */
    val rateHz: Int = 60,
    /** Total frame size in bytes, including the preamble. */
    val frameBytes: Int = 4096,
) {
    /** Interval between frames, in milliseconds. */
    val intervalMs: Long get() = 1_000L / rateHz.coerceAtLeast(1)

    /** Nominal bitrate in megabits per second. */
    val nominalMbps: Double get() = frameBytes * 8.0 * rateHz / 1_000_000.0

    /** Whether this configuration forces the fragmentation path. */
    val fragments: Boolean get() = frameBytes > MAX_PAYLOAD
}

/** The byte a correct frame carries at [offset]. */
private fun patternByte(index: Int, offset: Int): Byte =
    ((index * 31 + offset * 7).mod(251)).toByte()

/** Build the frame at [index] with [frameBytes] total size. */
fun buildTestFrame(index: Int, frameBytes: Int): ByteArray {
    val size = frameBytes.coerceAtLeast(PREAMBLE)
    val frame = ByteArray(size)
    ByteBuffer.wrap(frame).order(ByteOrder.BIG_ENDIAN).apply {
        putInt(index)
        putInt(size)
    }
    for (offset in PREAMBLE until size) {
        frame[offset] = patternByte(index, offset)
    }
    return frame
}

/** Produces verifiable synthetic frames. */
class TestStreamGenerator(val config: TestStreamConfig = TestStreamConfig()) {

    var produced: Int = 0
        private set

    /** Produce the next frame. */
    fun nextFrame(): ByteArray {
        val index = produced
        produced += 1
        return buildTestFrame(index, config.frameBytes)
    }
}

/** Why a received synthetic frame failed verification. */
enum class IntegrityError {
    /** Shorter than the preamble. */
    TOO_SHORT,

    /** Length field disagrees with the bytes actually present. */
    LENGTH_MISMATCH,

    /** Payload bytes do not match the pattern for the declared frame index. */
    PAYLOAD_CORRUPT,
}

/** Verification counters for a synthetic stream. */
data class TestStreamReport(
    val verified: Long = 0,
    val corrupt: Long = 0,
    val missing: Long = 0,
    val outOfOrder: Long = 0,
    val bytes: Long = 0,
) {
    /** Frames expected so far. */
    val expected: Long get() = verified + corrupt + missing

    /** Whether every frame that was expected arrived intact. */
    val isClean: Boolean get() = corrupt == 0L && missing == 0L
}

/** Result of verifying a received synthetic frame. */
sealed interface VerifyResult {
    data class Ok(val index: Int) : VerifyResult
    data class Failed(val error: IntegrityError) : VerifyResult
}

/** Verifies frames produced by [TestStreamGenerator]. */
class TestStreamVerifier {

    var report: TestStreamReport = TestStreamReport()
        private set

    private var highestIndex: Int? = null

    /** Check one received frame, updating the counters. */
    fun verify(frame: ByteArray): VerifyResult {
        if (frame.size < PREAMBLE) {
            report = report.copy(corrupt = report.corrupt + 1)
            return VerifyResult.Failed(IntegrityError.TOO_SHORT)
        }

        val buffer = ByteBuffer.wrap(frame).order(ByteOrder.BIG_ENDIAN)
        val index = buffer.int
        val declared = buffer.int

        if (declared != frame.size) {
            report = report.copy(corrupt = report.corrupt + 1)
            return VerifyResult.Failed(IntegrityError.LENGTH_MISMATCH)
        }

        for (offset in PREAMBLE until frame.size) {
            if (frame[offset] != patternByte(index, offset)) {
                report = report.copy(corrupt = report.corrupt + 1)
                return VerifyResult.Failed(IntegrityError.PAYLOAD_CORRUPT)
            }
        }

        val highest = highestIndex
        when {
            highest != null && index <= highest ->
                report = report.copy(outOfOrder = report.outOfOrder + 1)

            highest != null -> {
                report = report.copy(
                    missing = report.missing + (index - highest - 1),
                    verified = report.verified + 1,
                )
                highestIndex = index
            }

            else -> {
                // Frames before the first one seen were sent before we started listening, not
                // lost, so they are not counted as missing.
                highestIndex = index
                report = report.copy(verified = report.verified + 1)
            }
        }

        report = report.copy(bytes = report.bytes + frame.size)
        return VerifyResult.Ok(index)
    }
}
