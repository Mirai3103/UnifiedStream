package com.laffy.unifiedstream.transport

import com.laffy.unifiedstream.protocol.MediaHeader
import com.laffy.unifiedstream.protocol.seqDistance
import com.laffy.unifiedstream.protocol.seqIsNewer
import java.util.TreeMap

/**
 * Packets held per stream while waiting for an out-of-order predecessor.
 *
 * Deliberately small and fixed. An adaptive buffer that grows under loss is the standard choice
 * and the wrong one here: it trades away exactly the latency this project exists to minimize.
 */
const val REORDER_WINDOW: Int = 3

/** A fully reassembled frame ready for a consumer. */
data class Frame(
    /** Sender's timestamp, widened past the 32-bit wire wrap. */
    val timestampUs: Long,
    /** Sequence number of the frame's first packet. */
    val firstSequence: Int,
    /** Reassembled payload. */
    val payload: ByteArray,
) {
    // ByteArray uses identity equality by default, which would make frame comparison useless.
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is Frame) return false
        return timestampUs == other.timestampUs &&
            firstSequence == other.firstSequence &&
            payload.contentEquals(other.payload)
    }

    override fun hashCode(): Int {
        var result = timestampUs.hashCode()
        result = 31 * result + firstSequence
        result = 31 * result + payload.contentHashCode()
        return result
    }
}

/** Running counts for one stream. */
data class StreamStats(
    val received: Long = 0,
    val lost: Long = 0,
    val late: Long = 0,
    val incompleteFrames: Long = 0,
    val deliveredFrames: Long = 0,
    val bytes: Long = 0,
) {
    /** Packets expected over the lifetime of the stream. */
    val expected: Long get() = received + lost

    /** Loss as a percentage of expected packets. Zero when nothing was expected. */
    val lossPct: Double get() = if (expected == 0L) 0.0 else (lost.toDouble() / expected) * 100.0
}

/**
 * Widens a 32-bit microsecond timestamp past its ~71.6-minute wrap.
 *
 * Called out explicitly because a wrap bug works perfectly in every short test and then fails
 * an hour into a real session.
 */
class TimestampUnwrapper {
    private var lastRaw: Long? = null
    private var epoch: Long = 0

    fun unwrap(raw: Long): Long {
        val last = lastRaw
        if (last == null) {
            lastRaw = raw
            return raw
        }

        // A backwards jump of more than half the range is a wrap; a smaller one is just an
        // out-of-order packet and must not advance the epoch.
        if (raw < last && (last - raw) > 0xFFFFFFFFL / 2) {
            epoch += 0x100000000L
        }
        if (seq32IsNewer(raw, last)) {
            lastRaw = raw
        }
        return epoch + raw
    }

    private fun seq32IsNewer(a: Long, b: Long): Boolean {
        val delta = (a - b) and 0xFFFFFFFFL
        return delta != 0L && delta < 0x80000000L
    }
}

private data class HeldPacket(
    val sequence: Int,
    val timestampUs: Long,
    val marker: Boolean,
    val fragment: Boolean,
    val payload: ByteArray,
) {
    override fun equals(other: Any?): Boolean =
        other is HeldPacket && sequence == other.sequence && payload.contentEquals(other.payload)

    override fun hashCode(): Int = 31 * sequence + payload.contentHashCode()
}

/**
 * Per-stream receive state: ordering, loss accounting, and fragment reassembly.
 *
 * Mirrors `unifiedstream_net::transport::StreamReceiver`.
 */
class StreamReceiver {

    var stats: StreamStats = StreamStats()
        private set

    private var lastReleased: Int? = null
    private val pending = TreeMap<Int, HeldPacket>()
    private val fragments = mutableListOf<HeldPacket>()
    private val unwrapper = TimestampUnwrapper()

    /**
     * Whether we know where a frame boundary is.
     *
     * A receiver that joins mid-frame cannot tell a frame's first fragment from its third.
     * Concatenating what arrives would deliver a frame with a missing head — silent corruption,
     * strictly worse than dropping it.
     */
    private var synced = false
    private var releasedAny = false

    /** Number of packets currently held awaiting a predecessor. Exposed for tests. */
    val pendingCount: Int get() = pending.size

    /**
     * Accept a packet, returning any frames that became deliverable.
     *
     * Packets are released in sequence order. A packet older than what has already been
     * released is counted as late and discarded rather than delivered out of order.
     */
    fun accept(header: MediaHeader, payload: ByteArray): List<Frame> {
        val last = lastReleased
        if (last != null && !seqIsNewer(header.sequence, last)) {
            stats = stats.copy(late = stats.late + 1)
            return emptyList()
        }

        stats = stats.copy(received = stats.received + 1, bytes = stats.bytes + payload.size)
        pending[header.sequence] = HeldPacket(
            sequence = header.sequence,
            timestampUs = header.timestampUs,
            marker = header.marker,
            fragment = header.fragment,
            payload = payload,
        )

        return drain()
    }

    private fun drain(): List<Frame> {
        val frames = mutableListOf<Frame>()

        while (true) {
            val nextSeq = pending.keys.firstOrNull() ?: break
            val last = lastReleased
            val inOrder = last == null || seqDistance(last, nextSeq) == 1

            // The buffer must not stall waiting for a packet that may never come.
            val mustFlush = pending.size > REORDER_WINDOW
            if (!inOrder && !mustFlush) break

            if (!inOrder && last != null) {
                val gap = (seqDistance(last, nextSeq) - 1).coerceAtLeast(0)
                stats = stats.copy(lost = stats.lost + gap)
            }

            val packet = pending.remove(nextSeq) ?: break
            lastReleased = nextSeq
            absorb(packet)?.let(frames::add)
        }

        return frames
    }

    private fun absorb(packet: HeldPacket): Frame? {
        // Sequence 0 as the very first release is a frame start by construction: senders start
        // every stream's counter at zero.
        val streamStart = !releasedAny
        releasedAny = true
        if (streamStart && packet.sequence == 0) synced = true

        // An unfragmented packet is a whole frame on its own, and re-establishes the boundary.
        if (!packet.fragment) {
            if (fragments.isNotEmpty()) {
                fragments.clear()
                stats = stats.copy(incompleteFrames = stats.incompleteFrames + 1)
            }
            synced = true
            stats = stats.copy(deliveredFrames = stats.deliveredFrames + 1)
            return Frame(unwrapper.unwrap(packet.timestampUs), packet.sequence, packet.payload)
        }

        // Fragmented, and we do not know where this frame began.
        if (!synced) {
            if (packet.marker) {
                synced = true
                stats = stats.copy(incompleteFrames = stats.incompleteFrames + 1)
            }
            return null
        }

        // A newer frame starting means the previous one will never complete.
        val first = fragments.firstOrNull()
        if (first != null && first.timestampUs != packet.timestampUs) {
            fragments.clear()
            stats = stats.copy(incompleteFrames = stats.incompleteFrames + 1)
        }

        val isLast = packet.marker
        val timestampRaw = packet.timestampUs
        fragments.add(packet)

        if (!isLast) return null

        if (fragments.size > 1 && !fragmentsAreContiguous()) {
            fragments.clear()
            stats = stats.copy(incompleteFrames = stats.incompleteFrames + 1)
            return null
        }

        val firstSequence = fragments.first().sequence
        val total = fragments.sumOf { it.payload.size }
        val payload = ByteArray(total)
        var offset = 0
        for (fragment in fragments) {
            fragment.payload.copyInto(payload, offset)
            offset += fragment.payload.size
        }
        fragments.clear()

        stats = stats.copy(deliveredFrames = stats.deliveredFrames + 1)
        return Frame(unwrapper.unwrap(timestampRaw), firstSequence, payload)
    }

    private fun fragmentsAreContiguous(): Boolean =
        fragments.zipWithNext().all { (a, b) -> seqDistance(a.sequence, b.sequence) == 1 }
}
