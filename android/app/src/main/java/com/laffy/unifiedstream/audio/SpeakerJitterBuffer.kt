package com.laffy.unifiedstream.audio

import java.util.ArrayDeque

/** Steady-state depth the buffer aims for, in frames. Three 20 ms frames is 60 ms. */
const val SPEAKER_JITTER_TARGET_FRAMES: Int = 3

/**
 * Hard depth cap, in frames. Beyond this the oldest audio is dropped: 120 ms of buffered audio
 * is already a latency problem, and stale audio is worth less than fresh audio.
 */
const val SPEAKER_JITTER_CAP_FRAMES: Int = 6

/** Counters exposed for diagnostics and the E2E latency notes. */
data class SpeakerJitterStats(
    /** Frames accepted from the network. */
    val pushed: Long = 0,
    /** Frames discarded because the buffer was full. */
    val overruns: Long = 0,
    /** Pulls that found the buffer empty (or short) and emitted silence. */
    val underruns: Long = 0,
)

/**
 * The receive-side jitter buffer between the network and the audio clock.
 *
 * A direct port of the desktop's `unifiedstream_audio::JitterBuffer`, with the same two hard
 * rules: an empty buffer yields silence (never a stall), and a full buffer drops its oldest
 * frame (never unbounded latency). The network coroutine pushes 20 ms frames; the [AudioTrack]
 * playback thread pulls whatever it needs. Neither waits for the other.
 */
class SpeakerJitterBuffer(private val cap: Int = SPEAKER_JITTER_CAP_FRAMES) {

    private val lock = Any()
    private val frames = ArrayDeque<ShortArray>()

    /**
     * Read offset into the front frame, in samples: the audio clock's pull size rarely aligns
     * with frame boundaries, so a frame is consumed across several pulls.
     */
    private var frontOffset = 0

    private var pushed = 0L
    private var overruns = 0L
    private var underruns = 0L

    /** Queue one decoded frame, dropping the oldest when full. */
    fun push(frame: ShortArray) {
        if (frame.isEmpty()) return
        synchronized(lock) {
            pushed++
            if (frames.size >= cap.coerceAtLeast(1)) {
                overruns++
                frames.pollFirst()
                frontOffset = 0
            }
            frames.addLast(frame)
        }
    }

    /**
     * Fill [out] with queued samples, zero-filling whatever the queue cannot cover.
     *
     * Never blocks on the network: this is what the playback thread calls, and a stalled
     * write would stutter the [android.media.AudioTrack].
     */
    fun popInto(out: ShortArray) {
        synchronized(lock) {
            var filled = 0
            while (filled < out.size) {
                val front = frames.peekFirst() ?: break
                val available = front.size - frontOffset
                val needed = out.size - filled
                val take = minOf(available, needed)

                front.copyInto(out, filled, frontOffset, frontOffset + take)
                filled += take

                if (take == available) {
                    frames.pollFirst()
                    frontOffset = 0
                } else {
                    frontOffset += take
                }
            }

            if (filled < out.size) {
                out.fill(0, filled, out.size)
                // Partial fills count too: they are the audible edge of an underrun.
                underruns++
            }
        }
    }

    /** Frames currently queued. */
    fun depth(): Int = synchronized(lock) { frames.size }

    /** Counters since construction or the last [clear]. */
    fun stats(): SpeakerJitterStats = synchronized(lock) {
        SpeakerJitterStats(pushed = pushed, overruns = overruns, underruns = underruns)
    }

    /** Drop all queued audio and reset the counters, e.g. on stream restart. */
    fun clear() {
        synchronized(lock) {
            frames.clear()
            frontOffset = 0
            pushed = 0
            overruns = 0
            underruns = 0
        }
    }
}

/**
 * Decode a wire payload — protocol §6.1, signed 16-bit little-endian, interleaved — into
 * host-order samples. The inverse of [encodeS16le].
 */
fun decodeS16le(payload: ByteArray): ShortArray {
    val samples = ShortArray(payload.size / 2)
    for (i in samples.indices) {
        val lo = payload[2 * i].toInt() and 0xFF
        val hi = payload[2 * i + 1].toInt()
        samples[i] = ((hi shl 8) or lo).toShort()
    }
    return samples
}
