package com.laffy.unifiedstream.audio

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MicControllerTest {

    private fun controller(
        noiseSuppressionAvailable: Boolean = true,
        levelEveryNFrames: Int = 1,
    ) = MicController(noiseSuppressionAvailable, levelEveryNFrames)

    @Test
    fun processShouldEncodeSamplesAsLittleEndian() {
        val payload = controller().process(shortArrayOf(0x1234, -2))

        // -2 is 0xFFFE: low byte first.
        assertArrayEquals(
            byteArrayOf(0x34, 0x12, 0xFE.toByte(), 0xFF.toByte()),
            payload,
        )
    }

    @Test
    fun aMutedFrameShouldNotProduceAPayload() {
        val mic = controller()
        mic.setMuted(true)
        assertNull(mic.process(shortArrayOf(1000, 2000)))
    }

    @Test
    fun unmutingShouldResumePayloads() {
        val mic = controller()
        mic.setMuted(true)
        mic.setMuted(false)
        assertNotNull(mic.process(shortArrayOf(1000)))
    }

    @Test
    fun mutingShouldZeroTheLevelImmediately() {
        val mic = controller()
        mic.process(ShortArray(960) { 20_000 })
        assertTrue("meter must move before the mute", mic.level.value.peak > 0f)

        mic.setMuted(true)

        assertEquals(AudioLevel(), mic.level.value)
    }

    @Test
    fun gainShouldScaleSamples() {
        val mic = controller()
        mic.setGain(2.0f)

        val samples = shortArrayOf(1_000, -1_000)
        mic.process(samples)

        assertArrayEquals(shortArrayOf(2_000, -2_000), samples)
    }

    @Test
    fun gainShouldSaturateInsteadOfWrapping() {
        val mic = controller()
        mic.setGain(4.0f)

        val samples = shortArrayOf(20_000, -20_000)
        mic.process(samples)

        assertArrayEquals(shortArrayOf(Short.MAX_VALUE, Short.MIN_VALUE), samples)
    }

    @Test
    fun gainShouldBeClampedToTheAllowedRange() {
        val mic = controller()
        mic.setGain(10f)
        assertEquals(GAIN_MAX, mic.gain.value)
        mic.setGain(0.01f)
        assertEquals(GAIN_MIN, mic.gain.value)
    }

    @Test
    fun aFullScaleFrameShouldMeterNearOne() {
        val mic = controller()
        mic.process(ShortArray(960) { Short.MAX_VALUE })

        assertEquals(1f, mic.level.value.peak, 0.001f)
        assertEquals(1f, mic.level.value.rms, 0.01f)
    }

    @Test
    fun aSilentFrameShouldMeterZero() {
        val mic = controller()
        mic.process(ShortArray(960))

        assertEquals(AudioLevel(), mic.level.value)
    }

    @Test
    fun theMeterShouldReflectPostGainLevels() {
        val mic = controller()
        mic.setGain(2.0f)
        mic.process(ShortArray(960) { 8_192 })

        // 8192 x 2 = 16384, half of full scale.
        assertEquals(0.5f, mic.level.value.peak, 0.01f)
    }

    @Test
    fun theLevelShouldUpdateOnlyEveryNthFrame() {
        val mic = controller(levelEveryNFrames = 3)
        val loud = { ShortArray(960) { 20_000 } }

        mic.process(loud())
        mic.process(loud())
        assertEquals("two frames must not move a 3-frame meter", AudioLevel(), mic.level.value)

        mic.process(loud())
        assertTrue("the third frame must", mic.level.value.peak > 0f)
    }

    @Test
    fun noiseSuppressionShouldBeRejectedWhenUnavailable() {
        val mic = controller(noiseSuppressionAvailable = false)
        mic.setNoiseSuppression(true)
        assertFalse(mic.noiseSuppression.value)
    }

    @Test
    fun noiseSuppressionShouldBeRecordedWhenAvailable() {
        val mic = controller(noiseSuppressionAvailable = true)
        mic.setNoiseSuppression(true)
        assertTrue(mic.noiseSuppression.value)
    }

    @Test
    fun aTwentyMillisecondFrameShouldEncodeToTheDocumentedSize() {
        // Protocol §5.1: 960 samples = 1920 bytes, which fragments into two packets.
        val payload = controller().process(ShortArray(MIC_FRAME_SAMPLES))
        assertEquals(1920, payload?.size)
    }

    @Test
    fun resetLevelShouldZeroTheMeter() {
        val mic = controller()
        mic.process(ShortArray(960) { 20_000 })
        mic.resetLevel()
        assertEquals(AudioLevel(), mic.level.value)
    }
}
