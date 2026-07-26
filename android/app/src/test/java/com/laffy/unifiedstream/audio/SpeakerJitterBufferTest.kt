package com.laffy.unifiedstream.audio

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test

/** Mirrors the Rust `unifiedstream_audio::jitter` tests, so both ports share one policy. */
class SpeakerJitterBufferTest {

    private fun frame(value: Short, len: Int) = ShortArray(len) { value }

    @Test
    fun `pop returns pushed samples in order`() {
        val buffer = SpeakerJitterBuffer()
        buffer.push(shortArrayOf(1, 2, 3))
        buffer.push(shortArrayOf(4, 5, 6))

        val out = ShortArray(6)
        buffer.popInto(out)

        assertArrayEquals(shortArrayOf(1, 2, 3, 4, 5, 6), out)
    }

    @Test
    fun `an empty buffer yields silence not a stall`() {
        val buffer = SpeakerJitterBuffer()
        val out = ShortArray(4) { 7 }
        buffer.popInto(out)

        assertArrayEquals(shortArrayOf(0, 0, 0, 0), out)
        assertEquals(1, buffer.stats().underruns)
    }

    @Test
    fun `a partial fill zeroes the remainder and counts an underrun`() {
        val buffer = SpeakerJitterBuffer()
        buffer.push(shortArrayOf(9, 9))

        val out = ShortArray(4) { 1 }
        buffer.popInto(out)

        assertArrayEquals(shortArrayOf(9, 9, 0, 0), out)
        assertEquals(1, buffer.stats().underruns)
    }

    @Test
    fun `a pull smaller than a frame resumes mid frame`() {
        // The AudioTrack write size rarely equals a wire frame, so partial consumption is
        // the norm.
        val buffer = SpeakerJitterBuffer()
        buffer.push(shortArrayOf(1, 2, 3, 4, 5))

        val first = ShortArray(2)
        buffer.popInto(first)
        val second = ShortArray(3)
        buffer.popInto(second)

        assertArrayEquals(shortArrayOf(1, 2), first)
        assertArrayEquals(shortArrayOf(3, 4, 5), second)
    }

    @Test
    fun `a full buffer drops the oldest frame`() {
        val buffer = SpeakerJitterBuffer(cap = 2)
        buffer.push(frame(1, 4))
        buffer.push(frame(2, 4))
        buffer.push(frame(3, 4)) // evicts the frame of 1s

        val out = ShortArray(8)
        buffer.popInto(out)

        assertArrayEquals(shortArrayOf(2, 2, 2, 2, 3, 3, 3, 3), out)
        assertEquals(1, buffer.stats().overruns)
    }

    @Test
    fun `depth never exceeds the cap`() {
        val buffer = SpeakerJitterBuffer()
        repeat(50) { buffer.push(frame(1, 960)) }
        assertEquals(SPEAKER_JITTER_CAP_FRAMES, buffer.depth())
    }

    @Test
    fun `playback resumes after an underrun`() {
        val buffer = SpeakerJitterBuffer()
        val out = ShortArray(4) { 5 }
        buffer.popInto(out) // underrun

        buffer.push(shortArrayOf(8, 8, 8, 8))
        buffer.popInto(out)

        assertArrayEquals(shortArrayOf(8, 8, 8, 8), out)
    }

    @Test
    fun `an eviction mid frame does not leave a stale read offset`() {
        val buffer = SpeakerJitterBuffer(cap = 2)
        buffer.push(shortArrayOf(1, 2, 3, 4))
        buffer.push(shortArrayOf(5, 6, 7, 8))

        // Consume half of the front frame, so the read offset is non-zero.
        val half = ShortArray(2)
        buffer.popInto(half)
        assertArrayEquals(shortArrayOf(1, 2), half)

        // This push evicts the half-read front frame; the offset must reset with it.
        buffer.push(shortArrayOf(9, 10, 11, 12))

        val out = ShortArray(8)
        buffer.popInto(out)
        assertArrayEquals(shortArrayOf(5, 6, 7, 8, 9, 10, 11, 12), out)
    }

    @Test
    fun `an empty frame is ignored`() {
        val buffer = SpeakerJitterBuffer()
        buffer.push(ShortArray(0))
        assertEquals(0, buffer.depth())
        assertEquals(0, buffer.stats().pushed)
    }

    @Test
    fun `clear drops audio and resets counters`() {
        val buffer = SpeakerJitterBuffer()
        buffer.push(shortArrayOf(1, 2))
        val out = ShortArray(8)
        buffer.popInto(out)

        buffer.clear()

        assertEquals(0, buffer.depth())
        assertEquals(SpeakerJitterStats(), buffer.stats())
    }

    @Test
    fun `stats count pushes`() {
        val buffer = SpeakerJitterBuffer()
        buffer.push(shortArrayOf(1))
        buffer.push(shortArrayOf(2))
        assertEquals(2, buffer.stats().pushed)
    }

    @Test
    fun `s16le decoding is the inverse of encoding`() {
        val samples = shortArrayOf(0, 1, -1, Short.MAX_VALUE, Short.MIN_VALUE, 1234, -1234)
        assertArrayEquals(samples, decodeS16le(encodeS16le(samples)))
    }

    @Test
    fun `s16le decoding is little endian`() {
        // 0x0102 little-endian is bytes 02 01.
        val decoded = decodeS16le(byteArrayOf(0x02, 0x01))
        assertArrayEquals(shortArrayOf(0x0102), decoded)
    }
}
