package com.laffy.unifiedstream.control

import android.util.Log
import com.laffy.unifiedstream.protocol.AudioParams
import com.laffy.unifiedstream.protocol.ControlCodec
import com.laffy.unifiedstream.protocol.ControlMessage
import com.laffy.unifiedstream.protocol.ErrorReason
import com.laffy.unifiedstream.protocol.MalformedControlException
import com.laffy.unifiedstream.protocol.PROTOCOL_VERSION
import com.laffy.unifiedstream.protocol.StreamRefusal
import java.io.BufferedReader
import java.io.BufferedWriter
import java.io.IOException
import java.net.InetSocketAddress
import java.net.Socket
import java.net.SocketTimeoutException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private const val TAG = "ControlClient"

/** Heartbeat interval, matching the desktop. */
const val HEARTBEAT_INTERVAL_MS: Long = 1_000

/** Unanswered heartbeats tolerated before the peer is declared unreachable. */
const val MAX_MISSED_HEARTBEATS: Int = 3

/** How long to wait for the TCP connection itself. */
const val CONNECT_TIMEOUT_MS: Int = 5_000

/**
 * How long to wait for the handshake reply.
 *
 * Must exceed the desktop's 30-second pairing prompt: the reply does not arrive until the user
 * taps Allow. Reusing the 5-second connect timeout here meant any user slower than five seconds
 * could never pair — the phone gave up, retried, and restarted the prompt on every attempt.
 */
const val HANDSHAKE_TIMEOUT_MS: Int = 35_000

/** Something that happened on the control channel. */
sealed interface ControlClientEvent {
    /** Handshake completed; the session is live. */
    data class Established(val ack: ControlMessage.HelloAck) : ControlClientEvent

    /** The desktop refused us. */
    data class Refused(val reason: ErrorReason, val message: String) : ControlClientEvent

    /** A round-trip sample, in milliseconds. */
    data class RttSample(val ms: Double) : ControlClientEvent

    /** The desktop reported its link quality. */
    data class TelemetryReceived(val report: ControlMessage.Telemetry) : ControlClientEvent

    /** The desktop answered one of our `stream_start`s, protocol §3.9.2. */
    data class StreamAckReceived(
        val stream: Int,
        val accepted: Boolean,
        val reason: StreamRefusal?,
    ) : ControlClientEvent

    /** The desktop asked us to start or stop a stream we source, protocol §3.9.4. */
    data class StreamRequested(val stream: Int, val active: Boolean) : ControlClientEvent

    /** The desktop announced a stream it wants to send us, protocol §3.9.1. */
    data class StreamStartReceived(
        val stream: Int,
        val params: AudioParams,
    ) : ControlClientEvent

    /** The desktop ended a stream, protocol §3.9.3. */
    data class StreamStopReceived(val stream: Int) : ControlClientEvent

    /** The desktop said bye. */
    data object PeerLeft : ControlClientEvent

    /** The connection ended. [cause] is null for a clean local disconnect. */
    data class Closed(val cause: String?) : ControlClientEvent

    /** The peer stopped answering heartbeats. */
    data object Unreachable : ControlClientEvent
}

/** Failure to establish the control channel. */
class ControlConnectException(message: String, cause: Throwable? = null) : Exception(message, cause)

/**
 * The phone-side control channel client.
 *
 * Blocking sockets on [Dispatchers.IO] rather than NIO: the traffic is a handful of small
 * messages per second, and the straightforward code is easier to get right than a selector
 * loop.
 */
class ControlClient(
    private val scope: CoroutineScope,
    private val deviceId: String,
    private val deviceName: String,
    private val caps: List<String>,
) {
    private val _events = MutableSharedFlow<ControlClientEvent>(
        replay = 0,
        extraBufferCapacity = 64,
    )

    /** Everything that happens on the channel. */
    val events: SharedFlow<ControlClientEvent> = _events.asSharedFlow()

    private var socket: Socket? = null
    private var writer: BufferedWriter? = null
    private val outbound = Channel<ControlMessage>(Channel.BUFFERED)

    private var readJob: Job? = null
    private var writeJob: Job? = null
    private var heartbeatJob: Job? = null

    @Volatile
    private var missedHeartbeats: Int = 0

    @Volatile
    private var closing: Boolean = false

    /**
     * Open the control channel and complete the handshake.
     *
     * @param resumeSessionId reuse an existing session so in-flight media stays valid across a
     *   reconnect.
     * @return the desktop's acknowledgement.
     * @throws ControlConnectException if the socket, the handshake, or the peer refuses.
     */
    suspend fun connect(
        host: String,
        port: Int,
        resumeSessionId: Long? = null,
        mediaPort: Int? = null,
    ): ControlMessage.HelloAck = withContext(Dispatchers.IO) {
        check(socket == null) { "already connected" }
        closing = false
        missedHeartbeats = 0

        val sock = try {
            Socket().apply {
                // Nagle would coalesce our tiny control messages and add latency to the
                // handshake for no bandwidth gain.
                tcpNoDelay = true
                connect(InetSocketAddress(host, port), CONNECT_TIMEOUT_MS)
                soTimeout = HANDSHAKE_TIMEOUT_MS
            }
        } catch (e: IOException) {
            throw ControlConnectException("could not reach $host:$port", e)
        }

        val reader = sock.getInputStream().bufferedReader()
        val out = sock.getOutputStream().bufferedWriter()

        val hello = ControlMessage.Hello(
            version = PROTOCOL_VERSION,
            deviceId = deviceId,
            deviceName = deviceName,
            caps = caps,
            resumeSessionId = resumeSessionId,
            mediaPort = mediaPort,
        )

        try {
            out.write(ControlCodec.toLine(hello))
            out.flush()
        } catch (e: IOException) {
            sock.closeQuietly()
            throw ControlConnectException("could not send hello", e)
        }

        val ack = readHandshakeReply(reader, sock)

        // The handshake is done; from here reads block indefinitely and liveness is the
        // heartbeat's job rather than the socket timeout's.
        sock.soTimeout = 0

        socket = sock
        writer = out
        startPumps(reader, out)

        _events.emit(ControlClientEvent.Established(ack))
        ack
    }

    private fun readHandshakeReply(reader: BufferedReader, sock: Socket): ControlMessage.HelloAck {
        while (true) {
            val line = try {
                reader.readLine()
            } catch (e: SocketTimeoutException) {
                sock.closeQuietly()
                throw ControlConnectException("timed out waiting for approval", e)
            } catch (e: IOException) {
                sock.closeQuietly()
                throw ControlConnectException("handshake failed", e)
            } ?: run {
                sock.closeQuietly()
                throw ControlConnectException("device closed the connection during handshake")
            }

            if (line.isBlank()) continue

            val message = try {
                ControlCodec.fromLine(line)
            } catch (e: MalformedControlException) {
                // The desktop sent something we cannot read; keep waiting rather than
                // treating one bad line as a failed handshake.
                Log.w(TAG, "unparseable handshake line", e)
                continue
            }

            when (message) {
                is ControlMessage.HelloAck -> return message
                is ControlMessage.Error -> {
                    sock.closeQuietly()
                    throw ControlConnectException(refusalText(message))
                }
                else -> Log.w(TAG, "ignoring ${message::class.simpleName} during handshake")
            }
        }
    }

    private fun refusalText(error: ControlMessage.Error): String = when (error.reason) {
        ErrorReason.VERSION_MISMATCH ->
            "incompatible protocol version (device speaks ${error.supportedVersion ?: "?"})"
        ErrorReason.REJECTED -> "pairing was declined"
        ErrorReason.BUSY -> "device is already paired with another phone"
        ErrorReason.MALFORMED -> "device rejected our handshake: ${error.message}"
        ErrorReason.INTERNAL -> "device reported an internal error: ${error.message}"
    }

    private fun startPumps(reader: BufferedReader, out: BufferedWriter) {
        writeJob = scope.launch(Dispatchers.IO) {
            // A single writer coroutine: BufferedWriter is not safe to share, and interleaved
            // writes would corrupt the newline framing.
            for (message in outbound) {
                try {
                    out.write(ControlCodec.toLine(message))
                    out.flush()
                } catch (e: IOException) {
                    if (!closing) Log.w(TAG, "write failed", e)
                    break
                }
                if (message is ControlMessage.Bye) break
            }
        }

        readJob = scope.launch(Dispatchers.IO) {
            // A peer that vanishes — Wi-Fi drop, desktop crash — closes the stream rather than
            // raising, so EOF must read as a loss. Defaulting this to null would make the
            // reconnect path dead code for the exact case it exists to handle. A genuinely
            // clean local shutdown sets `closing`, and is filtered below.
            var cause: String? = "device closed the connection"
            try {
                while (isActive) {
                    val line = reader.readLine() ?: break
                    if (line.isBlank()) continue
                    dispatch(line)
                }
            } catch (e: IOException) {
                if (!closing) cause = e.message ?: "connection lost"
            } finally {
                if (!closing) {
                    _events.emit(ControlClientEvent.Closed(cause))
                }
                closeInternal()
            }
        }

        heartbeatJob = scope.launch(Dispatchers.IO) {
            while (isActive) {
                delay(HEARTBEAT_INTERVAL_MS)
                if (closing) break

                if (missedHeartbeats >= MAX_MISSED_HEARTBEATS) {
                    Log.w(TAG, "peer missed $missedHeartbeats heartbeats")
                    _events.emit(ControlClientEvent.Unreachable)
                    closeInternal()
                    break
                }
                missedHeartbeats++
                outbound.trySend(ControlMessage.Ping(nowMicros()))
            }
        }
    }

    private suspend fun dispatch(line: String) {
        val message = try {
            ControlCodec.fromLine(line)
        } catch (e: MalformedControlException) {
            Log.w(TAG, "ignoring malformed line", e)
            return
        }

        when (message) {
            is ControlMessage.Pong -> {
                missedHeartbeats = 0
                val rttUs = nowMicros() - message.timestamp
                if (rttUs >= 0) {
                    _events.emit(ControlClientEvent.RttSample(rttUs / 1000.0))
                }
            }

            is ControlMessage.Ping -> {
                // The desktop may probe us too; echo verbatim.
                outbound.trySend(ControlMessage.Pong(message.timestamp))
            }

            is ControlMessage.Telemetry -> _events.emit(
                ControlClientEvent.TelemetryReceived(message),
            )

            is ControlMessage.Error -> {
                _events.emit(ControlClientEvent.Refused(message.reason, refusalText(message)))
                if (message.reason.isFatal) closeInternal()
            }

            ControlMessage.Bye -> {
                _events.emit(ControlClientEvent.PeerLeft)
                closeInternal()
            }

            is ControlMessage.StreamAck -> _events.emit(
                ControlClientEvent.StreamAckReceived(message.stream, message.accepted, message.reason),
            )

            is ControlMessage.StreamRequest -> _events.emit(
                ControlClientEvent.StreamRequested(message.stream, message.active),
            )

            is ControlMessage.StreamStop -> _events.emit(
                ControlClientEvent.StreamStopReceived(message.stream),
            )

            is ControlMessage.StreamStart ->
                // The session layer decides: it knows the negotiated capabilities and owns
                // the playback path that must exist before an accepting ack goes out.
                _events.emit(
                    ControlClientEvent.StreamStartReceived(message.stream, message.params),
                )

            is ControlMessage.Hello, is ControlMessage.HelloAck ->
                Log.d(TAG, "ignoring ${message::class.simpleName} outside the handshake")
        }
    }

    /** Queue a message for the peer. Returns false if the channel is closed. */
    fun send(message: ControlMessage): Boolean = outbound.trySend(message).isSuccess

    /** Report this side's link quality to the peer. */
    fun sendTelemetry(report: ControlMessage.Telemetry): Boolean = send(report)

    /** End the session cleanly: say bye, then tear the socket down. */
    suspend fun disconnect() {
        if (socket == null) return
        closing = true
        outbound.trySend(ControlMessage.Bye)
        // Give the writer a moment to flush the bye before the socket closes under it.
        delay(50)
        closeInternal()
        _events.emit(ControlClientEvent.Closed(null))
    }

    /** Whether the channel is currently open. */
    val isConnected: Boolean get() = socket?.isConnected == true && socket?.isClosed == false

    private fun closeInternal() {
        closing = true
        heartbeatJob?.cancel()
        writeJob?.cancel()
        readJob?.cancel()
        heartbeatJob = null
        writeJob = null
        readJob = null

        writer = null
        socket?.closeQuietly()
        socket = null
    }
}

/** Monotonic microseconds, for heartbeat timestamps. */
fun nowMicros(): Long = System.nanoTime() / 1_000

private fun Socket.closeQuietly() {
    try {
        close()
    } catch (_: IOException) {
        // Closing a socket that is already broken is not an error worth reporting.
    }
}
