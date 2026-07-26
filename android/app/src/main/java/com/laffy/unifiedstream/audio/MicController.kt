package com.laffy.unifiedstream.audio

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Softest allowed software gain. */
const val GAIN_MIN: Float = 0.5f

/** Loudest allowed software gain. */
const val GAIN_MAX: Float = 4.0f

/** Default software gain: unity. */
const val GAIN_DEFAULT: Float = 1.0f

/** Live input level. Both fields are 0..1, computed after gain. */
data class AudioLevel(val rms: Float = 0f, val peak: Float = 0f)

/**
 * The processing stage between capture and transport: software gain with saturation, mute,
 * level metering, and the PCM S16LE wire encoding.
 *
 * Deliberately free of Android types so every branch runs under plain JUnit. The Android-bound
 * halves — AudioRecord and NoiseSuppressor — live in [MicCapture]; this class only records the
 * user's noise-suppression *preference* and hands it to whoever owns the capture session.
 *
 * @param noiseSuppressionAvailable whether the platform offers a suppressor at all, gating
 *   both the stored preference and the UI toggle.
 * @param levelEveryNFrames how often the level flow updates, in frames. Three 20 ms frames
 *   is ~17 Hz — smooth enough for a meter without hammering the UI with state.
 */
class MicController(
    val noiseSuppressionAvailable: Boolean,
    private val levelEveryNFrames: Int = 3,
) {
    private val _gain = MutableStateFlow(GAIN_DEFAULT)

    /** Software gain, clamped to [GAIN_MIN]..[GAIN_MAX]. */
    val gain: StateFlow<Float> = _gain.asStateFlow()

    private val _muted = MutableStateFlow(false)

    /** Whether frames are being withheld. Capture keeps running; submission stops. */
    val muted: StateFlow<Boolean> = _muted.asStateFlow()

    private val _noiseSuppression = MutableStateFlow(false)

    /** The user's noise-suppression preference. Always false when unavailable. */
    val noiseSuppression: StateFlow<Boolean> = _noiseSuppression.asStateFlow()

    private val _level = MutableStateFlow(AudioLevel())

    /** Live input level for the UI meter. Zero while muted. */
    val level: StateFlow<AudioLevel> = _level.asStateFlow()

    private var framesSinceLevel = 0

    /** Set the software gain, clamping into the allowed range. Takes effect on the next frame. */
    fun setGain(value: Float) {
        _gain.value = value.coerceIn(GAIN_MIN, GAIN_MAX)
    }

    /** Mute or unmute. Muting zeroes the level immediately so the meter does not freeze. */
    fun setMuted(muted: Boolean) {
        _muted.value = muted
        if (muted) {
            _level.value = AudioLevel()
            framesSinceLevel = 0
        }
    }

    /** Record the noise-suppression preference. Ignored when the platform has no suppressor. */
    fun setNoiseSuppression(enabled: Boolean) {
        if (!noiseSuppressionAvailable) return
        _noiseSuppression.value = enabled
    }

    /**
     * Apply gain and metering to one captured frame, returning the PCM S16LE wire payload —
     * or null when muted, which is how mute stops transmission without stopping capture.
     *
     * [samples] is modified in place (gain is applied to it).
     */
    fun process(samples: ShortArray): ByteArray? {
        if (_muted.value) return null

        val gainNow = _gain.value
        var sumSquares = 0.0
        var peak = 0

        for (i in samples.indices) {
            // Saturate rather than wrap: a loud input at 4x gain must clip, not glitch.
            val amplified = (samples[i] * gainNow).toInt()
                .coerceIn(Short.MIN_VALUE.toInt(), Short.MAX_VALUE.toInt())
            samples[i] = amplified.toShort()

            val magnitude = if (amplified < 0) -amplified else amplified
            if (magnitude > peak) peak = magnitude
            sumSquares += amplified.toDouble() * amplified
        }

        if (++framesSinceLevel >= levelEveryNFrames) {
            framesSinceLevel = 0
            val rms = kotlin.math.sqrt(sumSquares / samples.size) / Short.MAX_VALUE
            _level.value = AudioLevel(
                rms = rms.toFloat().coerceIn(0f, 1f),
                peak = (peak.toFloat() / Short.MAX_VALUE).coerceIn(0f, 1f),
            )
        }

        return encodeS16le(samples)
    }

    /** Reset the meter, e.g. when the stream stops. */
    fun resetLevel() {
        _level.value = AudioLevel()
        framesSinceLevel = 0
    }
}

/**
 * Encode samples as the wire format: signed 16-bit little-endian, protocol §5.1.
 *
 * Little-endian unlike the header fields, because every current CPU on both ends is
 * little-endian and the payload is copied, not parsed field-by-field.
 */
fun encodeS16le(samples: ShortArray): ByteArray {
    val bytes = ByteArray(samples.size * 2)
    for (i in samples.indices) {
        val v = samples[i].toInt()
        bytes[2 * i] = (v and 0xFF).toByte()
        bytes[2 * i + 1] = ((v shr 8) and 0xFF).toByte()
    }
    return bytes
}
