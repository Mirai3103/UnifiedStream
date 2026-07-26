package com.laffy.unifiedstream.audio

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.util.Log
import kotlin.concurrent.thread

private const val TAG = "SpeakerPlayback"

/** Playback sample rate. The one rate the speaker stream uses, protocol §6. */
const val SPEAKER_SAMPLE_RATE = 48_000

/** Frame duration in milliseconds. */
const val SPEAKER_FRAME_MS = 20

/**
 * Owns the AudioTrack session: pulls samples from a [SpeakerJitterBuffer] on a dedicated
 * thread and writes them to the device's active audio output.
 *
 * The pull size is one 20 ms frame, matching the wire cadence; the jitter buffer's
 * silence-on-underrun rule means a lossy or muted desktop produces quiet, never a stall.
 *
 * @param buffer the jitter buffer the network path fills with decoded frames.
 * @param onFailure called once, on the playback thread, if playback cannot start or dies.
 */
class SpeakerPlayback(
    private val buffer: SpeakerJitterBuffer,
    private val onFailure: (String) -> Unit,
) {
    private var track: AudioTrack? = null
    private var playbackThread: Thread? = null

    @Volatile
    private var running = false

    @Volatile
    private var volume: Float = 1f

    @Volatile
    private var muted: Boolean = false

    /** Whether playback is currently running. */
    val isRunning: Boolean get() = running

    /**
     * Start playing. Returns false — after reporting through `onFailure` — when the audio
     * output cannot be opened.
     */
    fun start(channels: Int): Boolean {
        if (running) return true

        val channelMask = if (channels >= 2) {
            AudioFormat.CHANNEL_OUT_STEREO
        } else {
            AudioFormat.CHANNEL_OUT_MONO
        }
        val frameSamples = SPEAKER_SAMPLE_RATE * SPEAKER_FRAME_MS / 1000 * channels
        val frameBytes = frameSamples * 2

        val minBytes = AudioTrack.getMinBufferSize(
            SPEAKER_SAMPLE_RATE,
            channelMask,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        if (minBytes <= 0) {
            onFailure("This device cannot play 48 kHz audio")
            return false
        }

        // At least two wire frames of track buffer: writes are 20 ms at a time, and the track
        // must not underrun between them. The jitter buffer upstream carries the real slack.
        val bufferBytes = maxOf(minBytes, frameBytes * 2)

        val built = try {
            AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build(),
                )
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                        .setSampleRate(SPEAKER_SAMPLE_RATE)
                        .setChannelMask(channelMask)
                        .build(),
                )
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                .setBufferSizeInBytes(bufferBytes)
                .build()
        } catch (e: RuntimeException) {
            onFailure("Could not open the audio output: ${e.message}")
            return false
        }

        if (built.state != AudioTrack.STATE_INITIALIZED) {
            built.release()
            onFailure("Audio output is unavailable")
            return false
        }

        track = built
        applyVolume()
        built.play()
        if (built.playState != AudioTrack.PLAYSTATE_PLAYING) {
            stop()
            onFailure("Audio output did not start playing")
            return false
        }

        running = true
        playbackThread = thread(name = "speaker-playback") { playbackLoop(built, frameSamples) }
        return true
    }

    /**
     * Set the playback volume, 0.0–1.0. Composes with the hardware media volume; applied
     * immediately, without touching the stream.
     */
    fun setVolume(value: Float) {
        volume = value.coerceIn(0f, 1f)
        applyVolume()
    }

    /** Mute or unmute playback locally. The stream and reception continue untouched. */
    fun setMuted(value: Boolean) {
        muted = value
        applyVolume()
    }

    private fun applyVolume() {
        track?.setVolume(if (muted) 0f else volume)
    }

    /** Stop playback and release the output. Idempotent. */
    fun stop() {
        running = false
        playbackThread?.let { t ->
            if (t !== Thread.currentThread()) t.join(500)
        }
        playbackThread = null

        track?.let { t ->
            runCatching { t.pause() }
            runCatching { t.flush() }
            t.release()
        }
        track = null
    }

    private fun playbackLoop(track: AudioTrack, frameSamples: Int) {
        val frame = ShortArray(frameSamples)
        while (running) {
            // Silence on underrun comes from the buffer, so this write always has samples
            // and the track clock stays fed at a steady cadence.
            buffer.popInto(frame)
            val written = track.write(frame, 0, frame.size, AudioTrack.WRITE_BLOCKING)
            if (written < 0) {
                if (running) {
                    Log.w(TAG, "AudioTrack.write returned $written")
                    running = false
                    onFailure("Audio output stopped accepting audio")
                }
                return
            }
        }
    }

    companion object {
        /** Volume slider default: full scale, letting the hardware volume govern. */
        const val VOLUME_DEFAULT: Float = 1f
    }
}
