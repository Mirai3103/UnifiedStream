package com.laffy.unifiedstream.session

import android.util.Log
import com.laffy.unifiedstream.audio.AudioLevel
import com.laffy.unifiedstream.audio.SpeakerJitterBuffer
import com.laffy.unifiedstream.audio.decodeS16le
import com.laffy.unifiedstream.control.ControlClient
import com.laffy.unifiedstream.control.ControlClientEvent
import com.laffy.unifiedstream.control.ControlConnectException
import com.laffy.unifiedstream.discovery.DiscoveredDevice
import com.laffy.unifiedstream.protocol.AudioCodec
import com.laffy.unifiedstream.protocol.AudioParams
import com.laffy.unifiedstream.protocol.MediaHeader
import com.laffy.unifiedstream.protocol.Caps
import com.laffy.unifiedstream.protocol.ControlMessage
import com.laffy.unifiedstream.protocol.ErrorReason
import com.laffy.unifiedstream.protocol.DEFAULT_MEDIA_PORT
import com.laffy.unifiedstream.protocol.StreamId
import com.laffy.unifiedstream.protocol.StreamParams
import com.laffy.unifiedstream.protocol.StreamRefusal
import com.laffy.unifiedstream.protocol.VideoParams
import com.laffy.unifiedstream.protocol.intersectCaps
import com.laffy.unifiedstream.telemetry.TelemetryCollector
import com.laffy.unifiedstream.transport.DemuxResult
import com.laffy.unifiedstream.transport.MAX_DATAGRAM
import com.laffy.unifiedstream.transport.MediaDemux
import com.laffy.unifiedstream.transport.MediaSender
import com.laffy.unifiedstream.transport.MediaSocket
import com.laffy.unifiedstream.transport.TestStreamConfig
import com.laffy.unifiedstream.transport.TestStreamGenerator
import com.laffy.unifiedstream.transport.TestStreamReport
import com.laffy.unifiedstream.transport.TestStreamVerifier
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

private const val TAG = "SessionManager"

/** How long to wait for a `stream_ack` before declaring the start failed. */
const val STREAM_ACK_TIMEOUT_MS: Long = 5_000

/**
 * Captured frames queued for sending. Small and lossy on purpose: audio is only useful fresh,
 * so when the sender falls behind, dropping the oldest 20 ms beats growing a latency debt.
 */
private const val MIC_FRAME_QUEUE_CAPACITY = 8

/** How many speaker frames between level updates: 3 x 20 ms ≈ 15 Hz. */
private const val SPEAKER_LEVEL_EVERY_FRAMES = 3

/**
 * Captured video frames queued for sending. Two frames only: with video, the freshest complete
 * frame is worth more than any backlog, so a stalled sender drops the newest rather than
 * growing a latency debt — the next capture is fresher than anything it would displace.
 */
private const val CAMERA_FRAME_QUEUE_CAPACITY = 2

/**
 * Datagrams sent back-to-back before yielding within one video frame.
 *
 * A 720p frame is ~50 fragments; blasting them as one burst measurably raises loss on
 * consumer Wi-Fi, and spreading the frame across a few milliseconds costs nothing at 30 fps.
 */
private const val CAMERA_FRAGMENT_BURST = 10

/**
 * Owns the connection lifecycle: connect, reconnect with backoff, disconnect.
 *
 * Every UI surface renders from [state] rather than inspecting sockets, so the phone and the
 * desktop agree on what "connected" means.
 */
class SessionManager(
    private val scope: CoroutineScope,
    private val deviceId: String,
    private val deviceName: String,
    private val caps: List<String>,
    /**
     * Backoff delay before a given 1-based attempt, or null once attempts are exhausted.
     *
     * Injectable so tests can exercise the exhausted-retry path without waiting out the real
     * 15.5-second schedule.
     */
    private val backoff: (Int) -> Long? = ::backoffForAttempt,
) {
    private val _state = MutableStateFlow<ConnectionState>(ConnectionState.Idle)

    /** Current lifecycle state. The single source of truth for connection UI. */
    val state: StateFlow<ConnectionState> = _state.asStateFlow()

    private val _telemetry = MutableStateFlow(ControlMessage.Telemetry())

    /** Most recent link-quality report from the desktop. */
    val peerTelemetry: StateFlow<ControlMessage.Telemetry> = _telemetry.asStateFlow()

    private val _rttMs = MutableStateFlow<Double?>(null)

    /** Most recent locally measured round-trip sample, in milliseconds. */
    val rttMs: StateFlow<Double?> = _rttMs.asStateFlow()

    private val _testReport = MutableStateFlow<TestStreamReport?>(null)

    /** Synthetic test-stream counters, or null when it has never run this session. */
    val testReport: StateFlow<TestStreamReport?> = _testReport.asStateFlow()

    private val _testRunning = MutableStateFlow(false)

    /** Whether the synthetic test stream is sending. */
    val testRunning: StateFlow<Boolean> = _testRunning.asStateFlow()

    private val _micState = MutableStateFlow<MicStreamState>(MicStreamState.Inactive)

    /** Microphone stream lifecycle. The single source of truth for mic UI and capture. */
    val micState: StateFlow<MicStreamState> = _micState.asStateFlow()

    /**
     * Desktop-initiated requests to start the microphone.
     *
     * Not handled here because starting capture needs a permission check only an Android
     * component can perform; the observer either calls [startMicStream] or [refuseMicRequest].
     */
    private val _micStartRequests = MutableSharedFlow<Unit>(extraBufferCapacity = 4)
    val micStartRequests: SharedFlow<Unit> = _micStartRequests.asSharedFlow()

    private val _cameraState = MutableStateFlow<CameraStreamState>(CameraStreamState.Inactive)

    /** Camera stream lifecycle. The single source of truth for camera UI and capture. */
    val cameraState: StateFlow<CameraStreamState> = _cameraState.asStateFlow()

    /**
     * Desktop-initiated requests to start the camera.
     *
     * Not handled here because starting capture needs a permission check only an Android
     * component can perform; the observer either calls [startCameraStream] or
     * [refuseCameraRequest] — the mic's pattern exactly.
     */
    private val _cameraStartRequests = MutableSharedFlow<Unit>(extraBufferCapacity = 4)
    val cameraStartRequests: SharedFlow<Unit> = _cameraStartRequests.asSharedFlow()

    private val _speakerState = MutableStateFlow<SpeakerStreamState>(SpeakerStreamState.Inactive)

    /** Speaker stream lifecycle. The single source of truth for speaker UI and playback. */
    val speakerState: StateFlow<SpeakerStreamState> = _speakerState.asStateFlow()

    private val _speakerLevel = MutableStateFlow(AudioLevel())

    /** Live output level from decoded speaker frames, ~15 Hz while audio flows. */
    val speakerLevel: StateFlow<AudioLevel> = _speakerLevel.asStateFlow()

    /**
     * Decoded speaker audio waiting for the playback thread.
     *
     * Owned here rather than by the audio layer so frames arriving between the accepting
     * `stream_ack` and the AudioTrack spinning up are buffered, not lost.
     */
    val speakerBuffer = SpeakerJitterBuffer()

    private var client: ControlClient? = null
    private var eventJob: Job? = null
    private var connectJob: Job? = null

    private var media: MediaSocket? = null
    private var mediaReceiveJob: Job? = null
    private var testStreamJob: Job? = null
    private val telemetry = TelemetryCollector()

    /** The live receiver's demultiplexer, so stream lifecycle can register and unregister. */
    private var demux: MediaDemux? = null

    private var speakerRequestTimeoutJob: Job? = null
    private var speakerFramesSinceLevel = 0

    /** Whether the user wants the speaker on, so a reconnect can re-request the stream. */
    private var speakerDesired: Boolean = false

    private var micSendJob: Job? = null
    private var micAckTimeoutJob: Job? = null
    private var micParams = AudioParams()

    /** Whether the user wants the mic on, so a reconnect can re-announce the stream. */
    private var micDesired: Boolean = false

    private val micFrames = Channel<ByteArray>(
        capacity = MIC_FRAME_QUEUE_CAPACITY,
        onBufferOverflow = BufferOverflow.DROP_OLDEST,
    )

    private var cameraSendJob: Job? = null
    private var cameraAckTimeoutJob: Job? = null
    private var cameraParams = VideoParams()

    /** Whether the user wants the camera on, so a reconnect can re-announce the stream. */
    private var cameraDesired: Boolean = false

    private val cameraFrames = Channel<ByteArray>(
        capacity = CAMERA_FRAME_QUEUE_CAPACITY,
        onBufferOverflow = BufferOverflow.DROP_LATEST,
    )

    /** The device we are connected to, or trying to reach. */
    private var target: DiscoveredDevice? = null

    /** Session id to reuse on reconnect, so in-flight media stays valid across a blip. */
    private var resumeSessionId: Long? = null

    /** Set while the user explicitly asked to stop, to suppress automatic reconnection. */
    private var userInitiatedStop: Boolean = false

    /** Move to [ConnectionState.Discovering] while the device list is open. */
    fun enterDiscovery() {
        if (_state.value is ConnectionState.Idle || _state.value is ConnectionState.Failed) {
            _state.value = ConnectionState.Discovering
        }
    }

    /** Connect to [device], replacing any current session. */
    fun connect(device: DiscoveredDevice) {
        connectJob?.cancel()
        userInitiatedStop = false
        target = device
        resumeSessionId = null

        connectJob = scope.launch {
            attemptConnect(device, attempt = 0)
        }
    }

    /** End the session cleanly and return to [ConnectionState.Idle]. */
    fun disconnect() {
        userInitiatedStop = true
        micDesired = false
        cameraDesired = false
        speakerDesired = false
        connectJob?.cancel()
        connectJob = null

        scope.launch {
            client?.disconnect()
            teardown()
            _state.value = ConnectionState.Idle
        }
    }

    /**
     * Stop retrying and return to [ConnectionState.Idle].
     *
     * Distinct from [disconnect] only in intent; both stop immediately, which is what the user
     * asked for when they tap cancel.
     */
    fun cancelReconnect() {
        if (_state.value !is ConnectionState.Reconnecting) return
        userInitiatedStop = true
        micDesired = false
        cameraDesired = false
        speakerDesired = false
        connectJob?.cancel()
        connectJob = null
        scope.launch {
            teardown()
            _state.value = ConnectionState.Idle
        }
    }

    /** Send a telemetry report to the desktop. */
    fun sendTelemetry(report: ControlMessage.Telemetry) {
        client?.sendTelemetry(report)
    }

    // --- Microphone stream --------------------------------------------------------------

    /**
     * Announce the microphone stream to the desktop. No media flows until it is accepted.
     *
     * The caller is responsible for having `RECORD_AUDIO` granted; this only runs the
     * protocol side.
     */
    fun startMicStream(params: AudioParams = AudioParams()) {
        val state = _state.value
        if (state !is ConnectionState.Connected) return
        if (_micState.value is MicStreamState.Starting) return
        if (Caps.MICROPHONE !in state.negotiatedCaps) {
            _micState.value = MicStreamState.Refused(StreamRefusal.NOT_NEGOTIATED)
            return
        }

        micDesired = true
        micParams = params
        _micState.value = MicStreamState.Starting
        client?.send(ControlMessage.StreamStart(StreamId.MICROPHONE.value, params))

        micAckTimeoutJob?.cancel()
        micAckTimeoutJob = scope.launch {
            delay(STREAM_ACK_TIMEOUT_MS)
            if (_micState.value is MicStreamState.Starting) {
                Log.w(TAG, "mic stream_start was never answered")
                _micState.value = MicStreamState.Error("Device did not answer")
            }
        }
    }

    /** Stop the microphone stream and tell the desktop. */
    fun stopMicStream() {
        micDesired = false
        val wasLive = _micState.value is MicStreamState.Starting ||
            _micState.value is MicStreamState.Active
        stopMicSending()
        if (wasLive) client?.send(ControlMessage.StreamStop(StreamId.MICROPHONE.value))
        _micState.value = MicStreamState.Inactive
    }

    /**
     * Refuse a desktop-initiated mic request this phone cannot honour — no permission, or
     * capture is broken. A request left unanswered would hang the desktop's toggle.
     */
    fun refuseMicRequest(reason: StreamRefusal = StreamRefusal.INTERNAL) {
        client?.send(
            ControlMessage.StreamAck(StreamId.MICROPHONE.value, accepted = false, reason = reason),
        )
    }

    /** Report a local capture failure: stops the stream and surfaces the error. */
    fun reportMicFailure(message: String) {
        stopMicStream()
        _micState.value = MicStreamState.Error(message)
    }

    /**
     * Queue one captured audio frame for sending.
     *
     * Returns false when the mic stream is not active — a muted or stopped mic simply stops
     * submitting frames; the desktop fills the gap with silence.
     */
    fun sendMicFrame(payload: ByteArray): Boolean {
        if (!_micState.value.isActive) return false
        return micFrames.trySend(payload).isSuccess
    }

    private fun onStreamAck(stream: Int, accepted: Boolean, reason: StreamRefusal?) {
        if (stream == StreamId.SPEAKER.value) {
            // The desktop refusing our stream_request, protocol §3.9.4. An accepting ack for
            // the speaker never targets the phone — the desktop is that stream's source.
            if (!accepted && _speakerState.value is SpeakerStreamState.Requesting) {
                speakerRequestTimeoutJob?.cancel()
                speakerRequestTimeoutJob = null
                speakerDesired = false
                _speakerState.value =
                    SpeakerStreamState.Refused(reason ?: StreamRefusal.UNKNOWN)
            }
            return
        }
        if (stream == StreamId.CAMERA.value) {
            if (_cameraState.value !is CameraStreamState.Starting) return
            cameraAckTimeoutJob?.cancel()
            cameraAckTimeoutJob = null

            if (accepted) {
                val sessionId = _state.value.activeSessionId ?: return
                _cameraState.value = CameraStreamState.Active(cameraParams)
                startCameraSending(sessionId)
            } else {
                cameraDesired = false
                _cameraState.value = CameraStreamState.Refused(reason ?: StreamRefusal.UNKNOWN)
            }
            return
        }
        if (stream != StreamId.MICROPHONE.value) return
        if (_micState.value !is MicStreamState.Starting) return
        micAckTimeoutJob?.cancel()
        micAckTimeoutJob = null

        if (accepted) {
            val sessionId = _state.value.activeSessionId ?: return
            _micState.value = MicStreamState.Active(micParams)
            startMicSending(sessionId)
        } else {
            micDesired = false
            _micState.value = MicStreamState.Refused(reason ?: StreamRefusal.UNKNOWN)
        }
    }

    private fun startMicSending(sessionId: Long) {
        micSendJob?.cancel()
        // Frames queued before this activation belong to a previous stream generation.
        while (micFrames.tryReceive().isSuccess) Unit

        micSendJob = scope.launch {
            val socket = media ?: return@launch
            val sender = MediaSender(sessionId)
            for (payload in micFrames) {
                val datagrams = sender.frame(StreamId.MICROPHONE, payload)
                if (!socket.sendAll(datagrams)) break
                telemetry.recordSent(datagrams.sumOf { it.size }.toLong())
            }
        }
    }

    private fun stopMicSending() {
        micAckTimeoutJob?.cancel()
        micAckTimeoutJob = null
        micSendJob?.cancel()
        micSendJob = null
    }

    // --- Camera stream ------------------------------------------------------------------

    /**
     * Announce the camera stream to the desktop. No media flows until it is accepted.
     *
     * The caller is responsible for having `CAMERA` granted; this only runs the protocol
     * side. Calling it while the stream is active with different [params] performs the
     * §3.9.1 replacement flow — the desktop re-acks and reopens its device at the new format,
     * which is how a resolution change works.
     */
    fun startCameraStream(params: VideoParams = VideoParams()) {
        val state = _state.value
        if (state !is ConnectionState.Connected) return
        if (_cameraState.value is CameraStreamState.Starting) return
        if (Caps.CAMERA !in state.negotiatedCaps) {
            _cameraState.value = CameraStreamState.Refused(StreamRefusal.NOT_NEGOTIATED)
            return
        }

        cameraDesired = true
        cameraParams = params
        _cameraState.value = CameraStreamState.Starting
        client?.send(ControlMessage.StreamStart(StreamId.CAMERA.value, params))

        cameraAckTimeoutJob?.cancel()
        cameraAckTimeoutJob = scope.launch {
            delay(STREAM_ACK_TIMEOUT_MS)
            if (_cameraState.value is CameraStreamState.Starting) {
                Log.w(TAG, "camera stream_start was never answered")
                _cameraState.value = CameraStreamState.Error("Device did not answer")
            }
        }
    }

    /** Stop the camera stream and tell the desktop. */
    fun stopCameraStream() {
        cameraDesired = false
        val wasLive = _cameraState.value is CameraStreamState.Starting ||
            _cameraState.value is CameraStreamState.Active
        stopCameraSending()
        if (wasLive) client?.send(ControlMessage.StreamStop(StreamId.CAMERA.value))
        _cameraState.value = CameraStreamState.Inactive
    }

    /**
     * Refuse a desktop-initiated camera request this phone cannot honour — no permission, or
     * capture is broken. A request left unanswered would hang the desktop's toggle.
     */
    fun refuseCameraRequest(reason: StreamRefusal = StreamRefusal.INTERNAL) {
        client?.send(
            ControlMessage.StreamAck(StreamId.CAMERA.value, accepted = false, reason = reason),
        )
    }

    /** Report a local capture failure: stops the stream and surfaces the error. */
    fun reportCameraFailure(message: String) {
        stopCameraStream()
        _cameraState.value = CameraStreamState.Error(message)
    }

    /**
     * Queue one encoded JPEG frame for sending.
     *
     * Returns false when the camera stream is not active. A full queue drops this frame —
     * the next capture is fresher than anything waiting would be.
     */
    fun sendCameraFrame(jpeg: ByteArray): Boolean {
        if (!_cameraState.value.isActive) return false
        return cameraFrames.trySend(jpeg).isSuccess
    }

    private fun startCameraSending(sessionId: Long) {
        cameraSendJob?.cancel()
        // Frames queued before this activation belong to a previous stream generation.
        while (cameraFrames.tryReceive().isSuccess) Unit

        cameraSendJob = scope.launch {
            val socket = media ?: return@launch
            val sender = MediaSender(sessionId)
            frames@ for (payload in cameraFrames) {
                // One video frame is dozens of datagrams; pace them in small bursts so a
                // frame does not arrive as a single loss-prone blast, protocol §7.
                val datagrams = sender.frame(StreamId.CAMERA, payload)
                for (burst in datagrams.chunked(CAMERA_FRAGMENT_BURST)) {
                    if (!socket.sendAll(burst)) break@frames
                    telemetry.recordSent(burst.sumOf { it.size }.toLong())
                    if (burst.size == CAMERA_FRAGMENT_BURST) delay(1)
                }
            }
        }
    }

    private fun stopCameraSending() {
        cameraAckTimeoutJob?.cancel()
        cameraAckTimeoutJob = null
        cameraSendJob?.cancel()
        cameraSendJob = null
    }

    // --- Speaker stream -----------------------------------------------------------------

    /**
     * Ask the desktop to start the speaker stream, protocol §3.9.4.
     *
     * The phone is the sink, so its toggle is a `stream_request`; the desktop answers by
     * running the normal `stream_start` flow, which [onStreamStart] accepts.
     */
    fun startSpeaker() {
        val state = _state.value
        if (state !is ConnectionState.Connected) return
        if (_speakerState.value is SpeakerStreamState.Requesting) return
        if (Caps.SPEAKER !in state.negotiatedCaps) {
            _speakerState.value = SpeakerStreamState.Refused(StreamRefusal.NOT_NEGOTIATED)
            return
        }

        speakerDesired = true
        _speakerState.value = SpeakerStreamState.Requesting
        client?.send(ControlMessage.StreamRequest(StreamId.SPEAKER.value, active = true))

        speakerRequestTimeoutJob?.cancel()
        speakerRequestTimeoutJob = scope.launch {
            delay(STREAM_ACK_TIMEOUT_MS)
            if (_speakerState.value is SpeakerStreamState.Requesting) {
                Log.w(TAG, "speaker stream_request was never answered")
                _speakerState.value = SpeakerStreamState.Error("PC did not answer")
            }
        }
    }

    /** Stop the speaker stream and ask the desktop to stop sending. */
    fun stopSpeaker() {
        speakerDesired = false
        val wasLive = _speakerState.value is SpeakerStreamState.Requesting ||
            _speakerState.value is SpeakerStreamState.Active
        // The desktop honours the stop by ceasing to send and issuing `stream_stop`;
        // reception is released locally right away rather than waiting for it.
        if (wasLive) {
            client?.send(ControlMessage.StreamRequest(StreamId.SPEAKER.value, active = false))
        }
        releaseSpeakerReception()
        _speakerState.value = SpeakerStreamState.Inactive
    }

    /** Report a local playback failure: stops the stream and surfaces the error. */
    fun reportSpeakerFailure(message: String) {
        stopSpeaker()
        _speakerState.value = SpeakerStreamState.Error(message)
    }

    /**
     * Answer the desktop's `stream_start`. Only the speaker stream is sinkable; everything
     * else is refused so the desktop's ack timeout never fires blind.
     */
    private fun onStreamStart(stream: Int, params: StreamParams) {
        if (stream != StreamId.SPEAKER.value) {
            client?.send(
                ControlMessage.StreamAck(stream, accepted = false, reason = StreamRefusal.UNSUPPORTED_STREAM),
            )
            return
        }

        val state = _state.value as? ConnectionState.Connected
        if (state == null || Caps.SPEAKER !in state.negotiatedCaps) {
            client?.send(
                ControlMessage.StreamAck(stream, accepted = false, reason = StreamRefusal.NOT_NEGOTIATED),
            )
            return
        }

        // PCM S16LE at 48 kHz, mono or stereo, is what this phone plays; Opus is the desktop's
        // stretch task — and video parameters on an audio stream — are refused the same way.
        val audio = params as? AudioParams
        val playable = audio != null &&
            audio.codec == AudioCodec.PCM_S16LE &&
            audio.sampleRate == 48_000 &&
            audio.channels in 1..2
        if (audio == null || !playable) {
            client?.send(
                ControlMessage.StreamAck(stream, accepted = false, reason = StreamRefusal.UNSUPPORTED_CODEC),
            )
            return
        }

        speakerRequestTimeoutJob?.cancel()
        speakerRequestTimeoutJob = null

        // A restart replaces the stream: reset receive state before the ack, per §3.9.1.
        // Reception must be ready before the accepting ack goes out — media may follow it
        // immediately, and the jitter buffer is what catches frames until playback spins up.
        demux?.unregister(StreamId.SPEAKER)
        demux?.register(StreamId.SPEAKER)
        speakerBuffer.clear()
        speakerFramesSinceLevel = 0

        client?.send(ControlMessage.StreamAck(stream, accepted = true))
        _speakerState.value = SpeakerStreamState.Active(audio)
    }

    /** Route one reassembled speaker frame into the jitter buffer, metering as it passes. */
    private fun onSpeakerFrame(payload: ByteArray) {
        if (!_speakerState.value.isActive) return
        val samples = decodeS16le(payload)
        if (samples.isEmpty()) return

        if (++speakerFramesSinceLevel >= SPEAKER_LEVEL_EVERY_FRAMES) {
            speakerFramesSinceLevel = 0
            _speakerLevel.value = levelOf(samples)
        }
        speakerBuffer.push(samples)
    }

    private fun levelOf(samples: ShortArray): AudioLevel {
        var sumSquares = 0.0
        var peak = 0
        for (sample in samples) {
            val value = sample.toInt()
            val magnitude = if (value < 0) -value else value
            if (magnitude > peak) peak = magnitude
            sumSquares += value.toDouble() * value
        }
        val rms = kotlin.math.sqrt(sumSquares / samples.size) / Short.MAX_VALUE
        return AudioLevel(
            rms = rms.toFloat().coerceIn(0f, 1f),
            peak = (peak.toFloat() / Short.MAX_VALUE).coerceIn(0f, 1f),
        )
    }

    /** Stop accepting and holding speaker audio. Playback teardown belongs to the observer. */
    private fun releaseSpeakerReception() {
        speakerRequestTimeoutJob?.cancel()
        speakerRequestTimeoutJob = null
        demux?.unregister(StreamId.SPEAKER)
        speakerBuffer.clear()
        _speakerLevel.value = AudioLevel()
        speakerFramesSinceLevel = 0
    }

    // --- Connection attempts ------------------------------------------------------------

    private suspend fun attemptConnect(device: DiscoveredDevice, attempt: Int) {
        _state.value = if (attempt == 0) {
            ConnectionState.Connecting(device.name)
        } else {
            ConnectionState.Reconnecting(device.name, attempt, MAX_RECONNECT_ATTEMPTS)
        }

        val fresh = ControlClient(scope, deviceId, deviceName, caps)
        client = fresh
        observe(fresh)

        // Bound before the handshake so its port can be advertised in `hello`; the desktop
        // pairs that with our TCP address rather than waiting to learn it from a datagram.
        val socket = media ?: MediaSocket.bind(DEFAULT_MEDIA_PORT).also { media = it }

        try {
            val ack = fresh.connect(device.host, device.port, resumeSessionId, socket.localPort)
            resumeSessionId = ack.sessionId
            socket.setPeer(device.host, ack.mediaPort)
            startMediaReceive(socket, ack.sessionId)

            _state.value = ConnectionState.Connected(
                peerName = ack.deviceName,
                sessionId = ack.sessionId,
                negotiatedCaps = intersectCaps(caps, ack.caps),
                mediaPort = ack.mediaPort,
            )
            Log.i(TAG, "connected to ${ack.deviceName} session=${ack.sessionId}")

            // A resumed session does not resurrect streams by itself: the desktop tore its
            // sinks down with the old connection, so anything the user still wants on is
            // re-announced with a fresh stream_start — or, for the speaker, re-requested.
            if (micDesired) startMicStream(micParams)
            if (cameraDesired) startCameraStream(cameraParams)
            if (speakerDesired) startSpeaker()
        } catch (e: ControlConnectException) {
            Log.w(TAG, "connect attempt ${attempt + 1} failed", e)
            teardown()
            handleConnectFailure(device, attempt, e)
        }
    }

    private suspend fun handleConnectFailure(
        device: DiscoveredDevice,
        attempt: Int,
        error: ControlConnectException,
    ) {
        val message = error.message.orEmpty()

        // A refusal is a decision, not a glitch: retrying would just re-annoy the user or
        // hammer a desktop that is deliberately busy.
        val terminal = when {
            message.contains("declined") -> FailureReason.Rejected
            message.contains("already paired") -> FailureReason.Busy
            message.contains("incompatible protocol") -> FailureReason.VersionMismatch(message)
            else -> null
        }
        if (terminal != null) {
            _state.value = ConnectionState.Failed(terminal)
            return
        }

        scheduleRetry(device, attempt, FailureReason.Unreachable(message))
    }

    private suspend fun scheduleRetry(
        device: DiscoveredDevice,
        attempt: Int,
        reasonIfExhausted: FailureReason,
    ) {
        if (userInitiatedStop) return

        val nextAttempt = attempt + 1
        val delayMs = backoff(nextAttempt)
        if (delayMs == null) {
            Log.w(TAG, "reconnect attempts exhausted for ${device.name}")
            _state.value = ConnectionState.Failed(
                if (attempt == 0) reasonIfExhausted else FailureReason.ReconnectExhausted,
            )
            return
        }

        _state.value = ConnectionState.Reconnecting(device.name, nextAttempt, MAX_RECONNECT_ATTEMPTS)
        delay(delayMs)
        if (!scope.isActive || userInitiatedStop) return

        attemptConnect(device, nextAttempt)
    }

    private fun observe(source: ControlClient) {
        eventJob?.cancel()
        eventJob = scope.launch {
            source.events.collect { event ->
                when (event) {
                    is ControlClientEvent.RttSample -> _rttMs.value = event.ms

                    is ControlClientEvent.TelemetryReceived -> _telemetry.value = event.report

                    is ControlClientEvent.Refused -> {
                        _state.value = ConnectionState.Failed(
                            when (event.reason) {
                                ErrorReason.REJECTED -> FailureReason.Rejected
                                ErrorReason.BUSY -> FailureReason.Busy
                                ErrorReason.VERSION_MISMATCH ->
                                    FailureReason.VersionMismatch(event.message)
                                else -> FailureReason.Other(event.message)
                            },
                        )
                        teardown()
                    }

                    // The desktop hung up deliberately; reconnecting would fight the user.
                    ControlClientEvent.PeerLeft -> {
                        teardown()
                        _state.value = ConnectionState.Idle
                    }

                    ControlClientEvent.Unreachable -> onConnectionLost("stopped responding")

                    is ControlClientEvent.Closed -> {
                        if (event.cause != null) onConnectionLost(event.cause)
                    }

                    is ControlClientEvent.StreamAckReceived ->
                        onStreamAck(event.stream, event.accepted, event.reason)

                    is ControlClientEvent.StreamRequested -> when {
                        event.stream == StreamId.MICROPHONE.value && event.active ->
                            // Not started here: capture needs a permission check only an
                            // Android component can perform.
                            _micStartRequests.tryEmit(Unit)

                        event.stream == StreamId.MICROPHONE.value -> stopMicStream()

                        event.stream == StreamId.CAMERA.value && event.active ->
                            // Same shape as the mic: the CAMERA permission check belongs to
                            // an Android component, so the request is surfaced, not obeyed.
                            _cameraStartRequests.tryEmit(Unit)

                        event.stream == StreamId.CAMERA.value -> stopCameraStream()

                        event.active ->
                            // A stream this phone cannot source; answer rather than hang the
                            // desktop's ack timeout.
                            client?.send(
                                ControlMessage.StreamAck(
                                    stream = event.stream,
                                    accepted = false,
                                    reason = StreamRefusal.UNSUPPORTED_STREAM,
                                ),
                            )

                        else -> Unit // a stop request for a stream we are not sending
                    }

                    is ControlClientEvent.StreamStartReceived ->
                        onStreamStart(event.stream, event.params)

                    is ControlClientEvent.StreamStopReceived -> {
                        if (event.stream == StreamId.MICROPHONE.value &&
                            _micState.value !is MicStreamState.Inactive
                        ) {
                            micDesired = false
                            stopMicSending()
                            _micState.value = MicStreamState.Inactive
                        }
                        if (event.stream == StreamId.CAMERA.value &&
                            _cameraState.value !is CameraStreamState.Inactive &&
                            _cameraState.value !is CameraStreamState.Starting
                        ) {
                            // A stop while Starting is the §3.9.1 replacement flow tearing the
                            // old generation down; the fresh ack is still on its way, so only
                            // an established stream is ended here.
                            cameraDesired = false
                            stopCameraSending()
                            _cameraState.value = CameraStreamState.Inactive
                        }
                        if (event.stream == StreamId.SPEAKER.value &&
                            _speakerState.value !is SpeakerStreamState.Inactive
                        ) {
                            // The desktop ended it deliberately; do not resurrect on reconnect.
                            speakerDesired = false
                            releaseSpeakerReception()
                            _speakerState.value = SpeakerStreamState.Inactive
                        }
                    }

                    is ControlClientEvent.Established -> Unit // handled by attemptConnect
                }
            }
        }
    }

    private fun onConnectionLost(cause: String) {
        if (userInitiatedStop) return
        val device = target ?: return
        if (_state.value !is ConnectionState.Connected) return

        Log.w(TAG, "connection to ${device.name} lost: $cause")
        _rttMs.value = null

        connectJob?.cancel()
        connectJob = scope.launch {
            teardown()
            scheduleRetry(device, attempt = 0, reasonIfExhausted = FailureReason.Unreachable(cause))
        }
    }

    // --- Media ----------------------------------------------------------------------------

    /**
     * Start the synthetic test stream toward the desktop.
     *
     * No codecs exist yet, so this is what proves the transport end to end. A frame size above
     * [com.laffy.unifiedstream.protocol.MAX_PAYLOAD] exercises fragmentation and reassembly.
     */
    fun startTestStream(config: TestStreamConfig) {
        val sessionId = _state.value.activeSessionId ?: return
        val socket = media ?: return

        stopTestStream()
        _testReport.value = TestStreamReport()
        _testRunning.value = true

        testStreamJob = scope.launch {
            val generator = TestStreamGenerator(config)
            val sender = MediaSender(sessionId)
            while (isActive) {
                delay(config.intervalMs)
                val frame = generator.nextFrame()
                val datagrams = sender.frame(StreamId.TEST, frame)
                if (!socket.sendAll(datagrams)) break
                telemetry.recordSent(datagrams.sumOf { it.size }.toLong())
                _testReport.value = _testReport.value?.copy(
                    verified = generator.produced.toLong(),
                )
            }
            _testRunning.value = false
        }
    }

    /** Stop the synthetic test stream. */
    fun stopTestStream() {
        testStreamJob?.cancel()
        testStreamJob = null
        _testRunning.value = false
    }

    private fun startMediaReceive(socket: MediaSocket, sessionId: Long) {
        mediaReceiveJob?.cancel()
        val fresh = MediaDemux(sessionId).apply { register(StreamId.TEST) }
        demux = fresh
        mediaReceiveJob = scope.launch {
            val verifier = TestStreamVerifier()
            val buffer = ByteArray(MAX_DATAGRAM)

            while (isActive) {
                val len = socket.receive(buffer) ?: break
                telemetry.recordReceived(len.toLong())

                // The demux returns frames without their stream id; read it from the header
                // before handing the datagram over.
                val stream = MediaHeader.decodeOrNull(buffer, len)?.stream

                when (val result = fresh.accept(buffer, len)) {
                    is DemuxResult.Frames -> {
                        if (stream == StreamId.SPEAKER) {
                            result.frames.forEach { onSpeakerFrame(it.payload) }
                        } else {
                            result.frames.forEach { verifier.verify(it.payload) }
                            if (result.frames.isNotEmpty()) _testReport.value = verifier.report
                        }
                        val stats = fresh.totalStats()
                        telemetry.recordPackets(stats.received, stats.lost)
                    }

                    is DemuxResult.Dropped -> Unit // routine; counted inside the demux
                }
            }
        }
    }

    private fun teardown() {
        eventJob?.cancel()
        eventJob = null
        client = null

        stopTestStream()
        // The streams die with the session, but the `*Desired` flags survive so a successful
        // reconnect can re-announce them.
        stopMicSending()
        _micState.value = MicStreamState.Inactive
        stopCameraSending()
        _cameraState.value = CameraStreamState.Inactive
        releaseSpeakerReception()
        _speakerState.value = SpeakerStreamState.Inactive
        mediaReceiveJob?.cancel()
        mediaReceiveJob = null
        demux = null
        media?.close()
        media = null

        telemetry.reset()
        _testReport.value = null
        _rttMs.value = null
        _telemetry.value = ControlMessage.Telemetry()
    }
}
