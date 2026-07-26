package com.laffy.unifiedstream.session

import android.util.Log
import com.laffy.unifiedstream.control.ControlClient
import com.laffy.unifiedstream.control.ControlClientEvent
import com.laffy.unifiedstream.control.ControlConnectException
import com.laffy.unifiedstream.discovery.DiscoveredDevice
import com.laffy.unifiedstream.protocol.AudioParams
import com.laffy.unifiedstream.protocol.Caps
import com.laffy.unifiedstream.protocol.ControlMessage
import com.laffy.unifiedstream.protocol.ErrorReason
import com.laffy.unifiedstream.protocol.DEFAULT_MEDIA_PORT
import com.laffy.unifiedstream.protocol.StreamId
import com.laffy.unifiedstream.protocol.StreamRefusal
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

    private var client: ControlClient? = null
    private var eventJob: Job? = null
    private var connectJob: Job? = null

    private var media: MediaSocket? = null
    private var mediaReceiveJob: Job? = null
    private var testStreamJob: Job? = null
    private val telemetry = TelemetryCollector()

    private var micSendJob: Job? = null
    private var micAckTimeoutJob: Job? = null
    private var micParams = AudioParams()

    /** Whether the user wants the mic on, so a reconnect can re-announce the stream. */
    private var micDesired: Boolean = false

    private val micFrames = Channel<ByteArray>(
        capacity = MIC_FRAME_QUEUE_CAPACITY,
        onBufferOverflow = BufferOverflow.DROP_OLDEST,
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
            // re-announced with a fresh stream_start.
            if (micDesired) startMicStream(micParams)
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

                    is ControlClientEvent.StreamStopReceived -> {
                        if (event.stream == StreamId.MICROPHONE.value &&
                            _micState.value !is MicStreamState.Inactive
                        ) {
                            micDesired = false
                            stopMicSending()
                            _micState.value = MicStreamState.Inactive
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
        mediaReceiveJob = scope.launch {
            val demux = MediaDemux(sessionId).apply { register(StreamId.TEST) }
            val verifier = TestStreamVerifier()
            val buffer = ByteArray(MAX_DATAGRAM)

            while (isActive) {
                val len = socket.receive(buffer) ?: break
                telemetry.recordReceived(len.toLong())

                when (val result = demux.accept(buffer, len)) {
                    is DemuxResult.Frames -> {
                        result.frames.forEach { verifier.verify(it.payload) }
                        val stats = demux.totalStats()
                        telemetry.recordPackets(stats.received, stats.lost)
                        _testReport.value = verifier.report
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
        // The stream dies with the session, but `micDesired` survives so a successful
        // reconnect can re-announce it.
        stopMicSending()
        _micState.value = MicStreamState.Inactive
        mediaReceiveJob?.cancel()
        mediaReceiveJob = null
        media?.close()
        media = null

        telemetry.reset()
        _testReport.value = null
        _rttMs.value = null
        _telemetry.value = ControlMessage.Telemetry()
    }
}
