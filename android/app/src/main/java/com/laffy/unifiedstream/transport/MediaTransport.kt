package com.laffy.unifiedstream.transport

import android.util.Log
import com.laffy.unifiedstream.protocol.HEADER_LEN
import com.laffy.unifiedstream.protocol.MAX_PAYLOAD
import com.laffy.unifiedstream.protocol.MediaHeader
import com.laffy.unifiedstream.protocol.StreamId
import java.io.IOException
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.InetSocketAddress
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

private const val TAG = "MediaTransport"

/** Largest datagram we will ever receive, so the buffer never truncates a valid packet. */
const val MAX_DATAGRAM: Int = HEADER_LEN + MAX_PAYLOAD

/** Why a received datagram was discarded. */
enum class DropReason {
    /** Shorter than a header, or an unparseable one. */
    MALFORMED,

    /** Protocol version this build does not implement. */
    UNSUPPORTED_VERSION,

    /** Belongs to a different session — a stale sender, or another app on the LAN. */
    FOREIGN_SESSION,

    /** No receiver is registered for this stream id. */
    UNKNOWN_STREAM,
}

/** Counters for datagrams that never reached a stream. */
data class DropStats(
    val malformed: Long = 0,
    val unsupportedVersion: Long = 0,
    val foreignSession: Long = 0,
    val unknownStream: Long = 0,
) {
    val total: Long get() = malformed + unsupportedVersion + foreignSession + unknownStream

    fun record(reason: DropReason): DropStats = when (reason) {
        DropReason.MALFORMED -> copy(malformed = malformed + 1)
        DropReason.UNSUPPORTED_VERSION -> copy(unsupportedVersion = unsupportedVersion + 1)
        DropReason.FOREIGN_SESSION -> copy(foreignSession = foreignSession + 1)
        DropReason.UNKNOWN_STREAM -> copy(unknownStream = unknownStream + 1)
    }
}

/** Result of feeding a datagram to [MediaDemux]. */
sealed interface DemuxResult {
    data class Frames(val frames: List<Frame>) : DemuxResult
    data class Dropped(val reason: DropReason) : DemuxResult
}

/**
 * Demultiplexes datagrams into per-stream receivers.
 *
 * Separate from the socket so the whole receive path can be tested by feeding it byte arrays.
 * Mirrors `unifiedstream_net::transport::MediaDemux`.
 */
class MediaDemux(private val sessionId: Long) {

    private val streams = HashMap<Int, StreamReceiver>()

    var drops: DropStats = DropStats()
        private set

    /** Begin accepting packets for a stream. Unregistered streams are counted and dropped. */
    fun register(stream: StreamId) {
        streams.getOrPut(stream.value) { StreamReceiver() }
    }

    /** Stop accepting packets for a stream. */
    fun unregister(stream: StreamId) {
        streams.remove(stream.value)
    }

    /** Counters for a registered stream. */
    fun stats(stream: StreamId): StreamStats? = streams[stream.value]?.stats

    /** Counters summed across every registered stream. */
    fun totalStats(): StreamStats = streams.values.fold(StreamStats()) { acc, receiver ->
        val s = receiver.stats
        StreamStats(
            received = acc.received + s.received,
            lost = acc.lost + s.lost,
            late = acc.late + s.late,
            incompleteFrames = acc.incompleteFrames + s.incompleteFrames,
            deliveredFrames = acc.deliveredFrames + s.deliveredFrames,
            bytes = acc.bytes + s.bytes,
        )
    }

    /**
     * Feed one datagram, returning any frames it completed.
     *
     * Discarding is routine, not exceptional — a stale sender or an unrelated app on the LAN
     * produces these.
     */
    fun accept(datagram: ByteArray, length: Int = datagram.size): DemuxResult {
        val header = MediaHeader.decodeOrNull(datagram, length)
        if (header == null) {
            val reason = if (length < HEADER_LEN) {
                DropReason.MALFORMED
            } else {
                DropReason.UNSUPPORTED_VERSION
            }
            drops = drops.record(reason)
            return DemuxResult.Dropped(reason)
        }

        if (header.sessionId != sessionId) {
            drops = drops.record(DropReason.FOREIGN_SESSION)
            return DemuxResult.Dropped(DropReason.FOREIGN_SESSION)
        }

        val receiver = streams[header.stream.value]
        if (receiver == null) {
            drops = drops.record(DropReason.UNKNOWN_STREAM)
            return DemuxResult.Dropped(DropReason.UNKNOWN_STREAM)
        }

        val payload = datagram.copyOfRange(HEADER_LEN, length)
        return DemuxResult.Frames(receiver.accept(header, payload))
    }
}

/** A bound UDP media socket. */
class MediaSocket private constructor(private val socket: DatagramSocket) {

    /** The port actually bound, to be reported in the handshake. */
    val localPort: Int get() = socket.localPort

    private var peer: InetSocketAddress? = null

    /** Point this socket at a peer for subsequent sends. */
    fun setPeer(host: String, port: Int) {
        peer = InetSocketAddress(InetAddress.getByName(host), port)
    }

    /** Send one datagram to the configured peer. */
    suspend fun send(datagram: ByteArray): Boolean = withContext(Dispatchers.IO) {
        val target = peer ?: return@withContext false
        try {
            socket.send(DatagramPacket(datagram, datagram.size, target))
            true
        } catch (e: IOException) {
            Log.w(TAG, "send failed", e)
            false
        }
    }

    /** Send every datagram of a framed payload. */
    suspend fun sendAll(datagrams: List<ByteArray>): Boolean {
        for (datagram in datagrams) {
            if (!send(datagram)) return false
        }
        return true
    }

    /** Receive one datagram, returning the byte count, or null on failure. */
    suspend fun receive(buffer: ByteArray): Int? = withContext(Dispatchers.IO) {
        try {
            val packet = DatagramPacket(buffer, buffer.size)
            socket.receive(packet)
            packet.length
        } catch (e: IOException) {
            if (!socket.isClosed) Log.w(TAG, "receive failed", e)
            null
        }
    }

    /** Release the port. */
    fun close() {
        socket.close()
    }

    companion object {
        /**
         * Bind the media socket, preferring [preferred].
         *
         * Falls back to an ephemeral port when the default is taken; the bound port is what the
         * handshake reports, so a busy default is a non-event rather than a startup failure.
         */
        suspend fun bind(preferred: Int): MediaSocket = withContext(Dispatchers.IO) {
            val socket = try {
                DatagramSocket(preferred)
            } catch (e: IOException) {
                Log.w(TAG, "media port $preferred unavailable; using an ephemeral port", e)
                DatagramSocket()
            }
            MediaSocket(socket)
        }
    }
}
