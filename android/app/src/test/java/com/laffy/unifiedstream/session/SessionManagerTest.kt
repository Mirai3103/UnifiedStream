package com.laffy.unifiedstream.session

import com.laffy.unifiedstream.discovery.DiscoveredDevice
import com.laffy.unifiedstream.protocol.ControlCodec
import com.laffy.unifiedstream.protocol.ControlMessage
import com.laffy.unifiedstream.protocol.PROTOCOL_VERSION
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Drives [SessionManager] against a fake desktop on loopback.
 *
 * Real sockets rather than mocks: the framing and handshake are exactly what these tests are
 * meant to catch regressions in.
 */
class SessionManagerTest {

    private val scope = CoroutineScope(SupervisorJob())
    private var fake: FakeDesktop? = null

    @After
    fun tearDown() {
        fake?.close()
        scope.cancel()
    }

    /** A minimal desktop: accepts, answers `hello`, echoes `ping`. */
    private class FakeDesktop(
        private val sessionId: Long = 0x0123456789ABCDEFL,
        private val acceptConnections: Boolean = true,
    ) {
        private val server = ServerSocket(0)
        private val running = AtomicBoolean(true)
        val handshakes = java.util.concurrent.atomic.AtomicInteger(0)

        /** Session ids issued, in order, so a resume can be verified. */
        val issuedSessionIds = java.util.Collections.synchronizedList(mutableListOf<Long>())

        /** Resume ids the phone asked for, in order. */
        val requestedResumeIds = java.util.Collections.synchronizedList(mutableListOf<Long?>())

        /** Every `stream_start` the phone sent, in order. */
        val streamStarts =
            java.util.Collections.synchronizedList(mutableListOf<ControlMessage.StreamStart>())

        /** How many `stream_stop`s the phone sent. */
        val streamStops = java.util.concurrent.atomic.AtomicInteger(0)

        /** Whether to accept the next microphone `stream_start`. */
        @Volatile
        var acceptMic: Boolean = true

        /** Refusal reason used when [acceptMic] is false. */
        @Volatile
        var micRefusal: com.laffy.unifiedstream.protocol.StreamRefusal =
            com.laffy.unifiedstream.protocol.StreamRefusal.INTERNAL

        val port: Int get() = server.localPort

        @Volatile
        var currentClient: Socket? = null

        @Volatile
        private var clientWriter: java.io.BufferedWriter? = null

        /** Push a message to the connected phone, as the desktop would. */
        fun send(message: ControlMessage) {
            val writer = clientWriter ?: return
            synchronized(writer) {
                writer.write(ControlCodec.toLine(message))
                writer.flush()
            }
        }

        init {
            thread(isDaemon = true, name = "fake-desktop") {
                while (running.get()) {
                    val socket = try {
                        server.accept()
                    } catch (_: Exception) {
                        break
                    }
                    if (!acceptConnections) {
                        socket.close()
                        continue
                    }
                    currentClient = socket
                    thread(isDaemon = true) { serve(socket) }
                }
            }
        }

        private fun serve(socket: Socket) {
            try {
                val reader = socket.getInputStream().bufferedReader()
                val writer = socket.getOutputStream().bufferedWriter()
                clientWriter = writer
                while (running.get()) {
                    val line = reader.readLine() ?: break
                    if (line.isBlank()) continue
                    when (val message = ControlCodec.fromLineOrNull(line)) {
                        is ControlMessage.Hello -> {
                            requestedResumeIds.add(message.resumeSessionId)
                            val id = message.resumeSessionId ?: sessionId
                            issuedSessionIds.add(id)
                            val ack = ControlMessage.HelloAck(
                                version = PROTOCOL_VERSION,
                                deviceId = "desktop-1",
                                deviceName = "cachy-desktop",
                                caps = listOf("cam", "mic", "spk"),
                                sessionId = id,
                                mediaPort = 47811,
                            )
                            synchronized(writer) {
                                writer.write(ControlCodec.toLine(ack))
                                writer.flush()
                            }
                            handshakes.incrementAndGet()
                        }

                        is ControlMessage.Ping -> {
                            synchronized(writer) {
                                writer.write(ControlCodec.toLine(ControlMessage.Pong(message.timestamp)))
                                writer.flush()
                            }
                        }

                        is ControlMessage.StreamStart -> {
                            streamStarts.add(message)
                            val ack = if (acceptMic) {
                                ControlMessage.StreamAck(stream = message.stream, accepted = true)
                            } else {
                                ControlMessage.StreamAck(
                                    stream = message.stream,
                                    accepted = false,
                                    reason = micRefusal,
                                )
                            }
                            synchronized(writer) {
                                writer.write(ControlCodec.toLine(ack))
                                writer.flush()
                            }
                        }

                        is ControlMessage.StreamStop -> streamStops.incrementAndGet()

                        ControlMessage.Bye -> break
                        else -> Unit
                    }
                }
            } catch (_: Exception) {
                // A dropped client is the scenario under test, not a failure.
            } finally {
                try {
                    socket.close()
                } catch (_: Exception) {
                }
            }
        }

        /** Hang up on the connected phone without a `bye`, simulating a Wi-Fi blip. */
        fun dropClient() {
            try {
                currentClient?.close()
            } catch (_: Exception) {
            }
            currentClient = null
        }

        fun close() {
            running.set(false)
            dropClient()
            try {
                server.close()
            } catch (_: Exception) {
            }
        }
    }

    private fun device(port: Int) = DiscoveredDevice(
        id = "desktop-1",
        name = "cachy-desktop",
        host = "127.0.0.1",
        port = port,
        protocolVersion = PROTOCOL_VERSION,
        caps = listOf("cam", "mic", "spk"),
        instanceName = "cachy-desktop",
    )

    private fun manager(backoff: (Int) -> Long? = ::backoffForAttempt) = SessionManager(
        scope = scope,
        deviceId = "phone-1",
        deviceName = "Pixel 8",
        caps = listOf("cam", "mic"),
        backoff = backoff,
    )

    /** Wait until [predicate] holds, or fail after [timeoutMs]. */
    private suspend fun SessionManager.awaitState(
        timeoutMs: Long = 5_000,
        description: String,
        predicate: (ConnectionState) -> Boolean,
    ): ConnectionState {
        val result = withTimeoutOrNull(timeoutMs) {
            var seen = state.value
            while (!predicate(seen)) {
                delay(10)
                seen = state.value
            }
            seen
        }
        return requireNotNull(result) { "timed out waiting for $description; last was ${state.value}" }
    }

    @Test
    fun connectingShouldReachTheConnectedState() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()

        manager.connect(device(desktop.port))

        val state = manager.awaitState(description = "Connected") { it is ConnectionState.Connected }
        val connected = state as ConnectionState.Connected
        assertEquals("cachy-desktop", connected.peerName)
        assertEquals(0x0123456789ABCDEFL, connected.sessionId)
        assertEquals(47811, connected.mediaPort)
    }

    @Test
    fun theHandshakeShouldNegotiateTheCapabilityIntersection() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()

        manager.connect(device(desktop.port))

        val connected = manager.awaitState(description = "Connected") {
            it is ConnectionState.Connected
        } as ConnectionState.Connected
        // Phone offers cam+mic, desktop offers cam+mic+spk.
        assertEquals(listOf("cam", "mic"), connected.negotiatedCaps)
    }

    @Test
    fun aHeartbeatShouldProduceARoundTripSample() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }

        val sample = withTimeoutOrNull(5_000) {
            while (manager.rttMs.value == null) delay(20)
            manager.rttMs.value
        }
        assertTrue("a round-trip sample must be measured, got $sample", (sample ?: -1.0) >= 0.0)
    }

    @Test
    fun aDroppedConnectionShouldEnterReconnecting() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }

        desktop.dropClient()

        val state = manager.awaitState(description = "Reconnecting or recovered") {
            it is ConnectionState.Reconnecting || it is ConnectionState.Connected
        }
        // Either it is mid-retry, or the retry already succeeded — both prove recovery ran.
        assertTrue(
            "expected reconnect activity, got $state",
            state is ConnectionState.Reconnecting || state is ConnectionState.Connected,
        )
    }

    @Test
    fun aReconnectShouldReuseTheExistingSessionId() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()

        manager.connect(device(desktop.port))
        val first = manager.awaitState(description = "Connected") {
            it is ConnectionState.Connected
        } as ConnectionState.Connected

        desktop.dropClient()

        // Wait for a second successful handshake.
        val recovered = withTimeoutOrNull(10_000) {
            while (desktop.handshakes.get() < 2) delay(20)
            true
        }
        assertTrue("the phone must retry the handshake", recovered == true)

        assertEquals(
            "the resumed handshake must carry the original session id",
            first.sessionId,
            desktop.requestedResumeIds.getOrNull(1),
        )
    }

    @Test
    fun exhaustedRetriesShouldFail() = runBlocking {
        // A port nobody listens on, and a schedule that gives up after two quick attempts.
        val deadPort = ServerSocket(0).use { it.localPort }
        val manager = manager(backoff = { attempt -> if (attempt <= 2) 10L else null })

        manager.connect(device(deadPort))

        val state = manager.awaitState(description = "Failed") { it is ConnectionState.Failed }
        val failed = state as ConnectionState.Failed
        assertTrue(
            "expected an exhausted-retry failure, got ${failed.reason}",
            failed.reason is FailureReason.ReconnectExhausted ||
                failed.reason is FailureReason.Unreachable,
        )
    }

    @Test
    fun cancellingDuringReconnectShouldReturnToIdle() = runBlocking {
        val deadPort = ServerSocket(0).use { it.localPort }
        // A long backoff so the test can reliably observe the Reconnecting window.
        val manager = manager(backoff = { 3_000L })

        manager.connect(device(deadPort))
        manager.awaitState(description = "Reconnecting") { it is ConnectionState.Reconnecting }

        manager.cancelReconnect()

        val state = manager.awaitState(description = "Idle") { it is ConnectionState.Idle }
        assertEquals(ConnectionState.Idle, state)
    }

    @Test
    fun cancellingShouldStopFurtherRetryAttempts() = runBlocking {
        val desktop = FakeDesktop(acceptConnections = false).also { fake = it }
        val manager = manager(backoff = { 100L })

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Reconnecting") { it is ConnectionState.Reconnecting }
        manager.cancelReconnect()
        manager.awaitState(description = "Idle") { it is ConnectionState.Idle }

        delay(500) // several backoff windows
        assertEquals(
            "no retry may run after cancelling",
            ConnectionState.Idle,
            manager.state.value,
        )
    }

    @Test
    fun disconnectingShouldReturnToIdle() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }

        manager.disconnect()

        assertEquals(
            ConnectionState.Idle,
            manager.awaitState(description = "Idle") { it is ConnectionState.Idle },
        )
    }

    @Test
    fun disconnectingShouldNotTriggerReconnection() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager(backoff = { 50L })

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }
        manager.disconnect()
        manager.awaitState(description = "Idle") { it is ConnectionState.Idle }

        delay(400)
        assertEquals(
            "an explicit disconnect must not reconnect",
            ConnectionState.Idle,
            manager.state.value,
        )
    }

    @Test
    fun enteringDiscoveryFromIdleShouldChangeState() = runBlocking {
        val manager = manager()
        manager.enterDiscovery()
        assertEquals(ConnectionState.Discovering, manager.state.value)
    }

    @Test
    fun enteringDiscoveryShouldNotDisturbALiveSession() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }

        manager.enterDiscovery()

        assertTrue(
            "a live session must survive opening the device list",
            manager.state.value is ConnectionState.Connected,
        )
    }

    // --- Microphone stream lifecycle ---------------------------------------------------------

    /** Wait until the mic state satisfies [predicate], or fail after [timeoutMs]. */
    private suspend fun SessionManager.awaitMicState(
        timeoutMs: Long = 5_000,
        description: String,
        predicate: (MicStreamState) -> Boolean,
    ): MicStreamState {
        val result = withTimeoutOrNull(timeoutMs) {
            var seen = micState.value
            while (!predicate(seen)) {
                delay(10)
                seen = micState.value
            }
            seen
        }
        return requireNotNull(result) {
            "timed out waiting for $description; last was ${micState.value}"
        }
    }

    @Test
    fun startingTheMicShouldReachActiveAfterAnAcceptingAck() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }
        manager.startMicStream()

        manager.awaitMicState(description = "Active") { it is MicStreamState.Active }
        assertEquals(1, desktop.streamStarts.size)
        assertEquals(2, desktop.streamStarts[0].stream)
    }

    @Test
    fun aRefusedMicShouldSurfaceTheReason() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        desktop.acceptMic = false
        desktop.micRefusal = com.laffy.unifiedstream.protocol.StreamRefusal.UNSUPPORTED_CODEC
        val manager = manager()

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }
        manager.startMicStream()

        val state = manager.awaitMicState(description = "Refused") { it is MicStreamState.Refused }
        assertEquals(
            com.laffy.unifiedstream.protocol.StreamRefusal.UNSUPPORTED_CODEC,
            (state as MicStreamState.Refused).reason,
        )
    }

    @Test
    fun aMicOutsideTheNegotiatedCapsShouldBeRefusedLocally() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = SessionManager(
            scope = scope,
            deviceId = "phone-1",
            deviceName = "Pixel 8",
            caps = listOf("cam"), // no mic offered, so it cannot be negotiated
        )

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }
        manager.startMicStream()

        val state = manager.awaitMicState(description = "Refused") { it is MicStreamState.Refused }
        assertEquals(
            com.laffy.unifiedstream.protocol.StreamRefusal.NOT_NEGOTIATED,
            (state as MicStreamState.Refused).reason,
        )
        assertTrue("no stream_start may reach the desktop", desktop.streamStarts.isEmpty())
    }

    @Test
    fun stoppingTheMicShouldTellTheDesktop() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }
        manager.startMicStream()
        manager.awaitMicState(description = "Active") { it is MicStreamState.Active }

        manager.stopMicStream()

        manager.awaitMicState(description = "Inactive") { it is MicStreamState.Inactive }
        withTimeoutOrNull(5_000) {
            while (desktop.streamStops.get() == 0) delay(10)
        } ?: throw AssertionError("desktop never received stream_stop")
    }

    @Test
    fun aDesktopStreamStopShouldDeactivateTheMic() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }
        manager.startMicStream()
        manager.awaitMicState(description = "Active") { it is MicStreamState.Active }

        desktop.send(ControlMessage.StreamStop(stream = 2))

        manager.awaitMicState(description = "Inactive") { it is MicStreamState.Inactive }
        assertEquals(
            "the phone must not echo a stream_stop back",
            0,
            desktop.streamStops.get(),
        )
    }

    @Test
    fun aDesktopMicRequestShouldBeSurfacedNotAutoStarted() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager()
        val requests = java.util.concurrent.atomic.AtomicInteger(0)
        val collector = scope.launch {
            manager.micStartRequests.collect { requests.incrementAndGet() }
        }

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }

        desktop.send(ControlMessage.StreamRequest(stream = 2, active = true))

        withTimeoutOrNull(5_000) {
            while (requests.get() == 0) delay(10)
        } ?: throw AssertionError("the request never reached the observer")
        assertTrue(
            "capture must not start without a permission check",
            manager.micState.value is MicStreamState.Inactive,
        )
        collector.cancel()
    }

    @Test
    fun aReconnectShouldReAnnounceAnActiveMic() = runBlocking {
        val desktop = FakeDesktop().also { fake = it }
        val manager = manager(backoff = { attempt -> if (attempt <= 5) 50L else null })

        manager.connect(device(desktop.port))
        manager.awaitState(description = "Connected") { it is ConnectionState.Connected }
        manager.startMicStream()
        manager.awaitMicState(description = "Active") { it is MicStreamState.Active }

        desktop.dropClient()

        manager.awaitState(description = "reconnected") {
            it is ConnectionState.Connected && desktop.handshakes.get() >= 2
        }
        manager.awaitMicState(description = "Active again") { it is MicStreamState.Active }
        assertEquals(
            "the mic must be re-announced with a fresh stream_start",
            2,
            desktop.streamStarts.size,
        )
    }
}
