package com.laffy.unifiedstream.video

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FrameEncoderTest {

    private fun encoder(maxFps: Int, onCompress: () -> Unit = {}): FrameEncoder =
        FrameEncoder(maxFps = maxFps, quality = 70, compress = { nv21, _, _, _ ->
            onCompress()
            nv21.copyOf(4)
        })

    private val frame = ByteArray(64)

    @Test
    fun `a camera at exactly max fps should have every frame encoded`() {
        val enc = encoder(maxFps = 30)
        val intervalUs = 1_000_000L / 30
        var encoded = 0
        for (i in 0 until 30) {
            if (enc.encode(8, 8, i * intervalUs) { frame } != null) encoded++
        }
        assertEquals(30, encoded)
    }

    @Test
    fun `a camera running faster than max fps should be paced down`() {
        val enc = encoder(maxFps = 15)
        // 60 fps camera: frames every ~16.7 ms against a 66.7 ms budget.
        var encoded = 0
        for (i in 0 until 60) {
            if (enc.encode(8, 8, i * 16_667L) { frame } != null) encoded++
        }
        assertTrue("expected ~15, got $encoded", encoded in 14..16)
    }

    @Test
    fun `jittery camera timestamps should not undershoot the target rate`() {
        val enc = encoder(maxFps = 30)
        val intervalUs = 1_000_000L / 30
        // +-3 ms of deterministic jitter around the nominal schedule.
        var encoded = 0
        for (i in 0 until 90) {
            val jitter = ((i * 37) % 7 - 3) * 1_000L
            if (enc.shouldEncode(i * intervalUs + jitter)) encoded++
        }
        assertTrue("expected ~90, got $encoded", encoded >= 85)
    }

    @Test
    fun `a gap should not bank credit for a later burst`() {
        val enc = encoder(maxFps = 10)
        assertTrue(enc.shouldEncode(0))
        // One second of silence, then a 100-frame burst within 100 ms.
        var encoded = 0
        for (i in 0 until 100) {
            if (enc.shouldEncode(1_000_000L + i * 1_000L)) encoded++
        }
        assertTrue("a burst after a gap must stay near 1-2 frames, got $encoded", encoded <= 2)
    }

    @Test
    fun `a skipped frame should pay neither conversion nor encode cost`() {
        var compressions = 0
        var conversions = 0
        val enc = encoder(maxFps = 30) { compressions++ }
        val supplier = { conversions++; frame }
        assertNotNull(enc.encode(8, 8, 0, supplier))
        // Immediately after: inside the pacing window.
        assertNull(enc.encode(8, 8, 1_000, supplier))
        assertEquals(1, compressions)
        assertEquals(1, conversions)
    }

    @Test
    fun `the first frame should always be encoded`() {
        assertTrue(encoder(maxFps = 1).shouldEncode(123_456_789L))
    }
}
