package com.laffy.unifiedstream.audio

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.media.audiofx.NoiseSuppressor
import android.util.Log
import kotlin.concurrent.thread

private const val TAG = "MicCapture"

/** Capture sample rate. The one rate the microphone stream uses, protocol §5. */
const val MIC_SAMPLE_RATE = 48_000

/** Frame duration in milliseconds. */
const val MIC_FRAME_MS = 20

/** Samples per 20 ms mono frame. */
const val MIC_FRAME_SAMPLES = MIC_SAMPLE_RATE * MIC_FRAME_MS / 1000

/**
 * Owns the AudioRecord session: 48 kHz mono 16-bit capture in 20 ms frames on a dedicated
 * thread, plus the platform NoiseSuppressor bound to that session.
 *
 * The caller is responsible for holding `RECORD_AUDIO` before [start]; a denied permission
 * surfaces as a start failure, never a crash.
 *
 * @param onFrame called on the capture thread with each complete 20 ms frame. The array is
 *   reused only after the callback returns, so the callee may process it in place but must
 *   not retain it.
 * @param onFailure called once, on the capture thread, if capture cannot start or dies.
 */
class MicCapture(
    private val onFrame: (ShortArray) -> Unit,
    private val onFailure: (String) -> Unit,
) {
    private var record: AudioRecord? = null
    private var suppressor: NoiseSuppressor? = null
    private var captureThread: Thread? = null

    @Volatile
    private var running = false

    /** Whether capture is currently running. */
    val isRunning: Boolean get() = running

    /**
     * Start capturing. Returns false — after reporting through `onFailure` — when the
     * microphone cannot be opened.
     */
    @SuppressLint("MissingPermission") // the caller checks RECORD_AUDIO before starting
    fun start(noiseSuppression: Boolean): Boolean {
        if (running) return true

        val minBytes = AudioRecord.getMinBufferSize(
            MIC_SAMPLE_RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        if (minBytes <= 0) {
            onFailure("This device cannot record 48 kHz mono audio")
            return false
        }

        // At least four frames of buffer: the read loop drains 20 ms at a time, and a GC
        // pause shorter than 80 ms must not overflow the hardware buffer.
        val bufferBytes = maxOf(minBytes, MIC_FRAME_SAMPLES * 2 * 4)

        val rec = try {
            AudioRecord(
                MediaRecorder.AudioSource.MIC,
                MIC_SAMPLE_RATE,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
                bufferBytes,
            )
        } catch (e: IllegalArgumentException) {
            onFailure("Could not open the microphone: ${e.message}")
            return false
        }

        if (rec.state != AudioRecord.STATE_INITIALIZED) {
            rec.release()
            onFailure("Microphone is unavailable")
            return false
        }

        record = rec
        applyNoiseSuppression(noiseSuppression)

        rec.startRecording()
        if (rec.recordingState != AudioRecord.RECORDSTATE_RECORDING) {
            stop()
            onFailure("Microphone did not start recording")
            return false
        }

        running = true
        captureThread = thread(name = "mic-capture") { captureLoop(rec) }
        return true
    }

    /**
     * Attach or release the platform noise suppressor on the live capture session.
     *
     * Safe to call while capturing; a device without a suppressor ignores the request.
     */
    fun applyNoiseSuppression(enabled: Boolean) {
        val sessionId = record?.audioSessionId ?: return
        if (!NoiseSuppressor.isAvailable()) return

        if (enabled && suppressor == null) {
            suppressor = try {
                NoiseSuppressor.create(sessionId)?.also { it.enabled = true }
            } catch (e: RuntimeException) {
                // Some devices advertise the effect and then fail to create it; audio still
                // flows, just unsuppressed.
                Log.w(TAG, "could not create NoiseSuppressor", e)
                null
            }
        } else if (!enabled) {
            suppressor?.release()
            suppressor = null
        }
    }

    /** Stop capturing and release the microphone. Idempotent. */
    fun stop() {
        running = false
        captureThread?.let { t ->
            if (t !== Thread.currentThread()) t.join(500)
        }
        captureThread = null

        suppressor?.release()
        suppressor = null

        record?.let { rec ->
            runCatching { rec.stop() }
            rec.release()
        }
        record = null
    }

    private fun captureLoop(rec: AudioRecord) {
        val frame = ShortArray(MIC_FRAME_SAMPLES)
        while (running) {
            var offset = 0
            while (offset < MIC_FRAME_SAMPLES && running) {
                val n = rec.read(frame, offset, MIC_FRAME_SAMPLES - offset)
                if (n <= 0) {
                    if (running) {
                        Log.w(TAG, "AudioRecord.read returned $n")
                        running = false
                        onFailure("Microphone stopped delivering audio")
                    }
                    return
                }
                offset += n
            }
            if (running) onFrame(frame)
        }
    }

    companion object {
        /** Whether this device offers a noise suppressor at all. */
        val noiseSuppressionAvailable: Boolean get() = NoiseSuppressor.isAvailable()
    }
}
