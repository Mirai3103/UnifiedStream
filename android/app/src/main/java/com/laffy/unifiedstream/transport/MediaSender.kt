package com.laffy.unifiedstream.transport

import com.laffy.unifiedstream.protocol.HEADER_LEN
import com.laffy.unifiedstream.protocol.MAX_PAYLOAD
import com.laffy.unifiedstream.protocol.MediaHeader
import com.laffy.unifiedstream.protocol.StreamId

/**
 * Builds outbound datagrams for a session.
 *
 * Mirrors `unifiedstream_net::transport::MediaSender`. Fragmentation is ours rather than IP's:
 * a single lost IP fragment silently destroys the whole datagram, and consumer APs handle
 * fragmented UDP poorly.
 */
class MediaSender(val sessionId: Long) {

    private val startedNanos: Long = System.nanoTime()
    private val sequences = HashMap<Int, Int>()

    /** Microseconds since this sender started, wrapping at 2^32 as the wire format requires. */
    fun timestampNow(): Long = ((System.nanoTime() - startedNanos) / 1_000) and 0xFFFFFFFFL

    private fun nextSequence(stream: StreamId, count: Int): Int {
        val first = sequences[stream.value] ?: 0
        sequences[stream.value] = (first + count) and 0xFFFF
        return first
    }

    /**
     * Frame a payload into one or more datagrams.
     *
     * Payloads of [MAX_PAYLOAD] bytes or fewer produce a single packet with the fragment flag
     * clear and the marker set. Larger payloads are split, every fragment carrying the same
     * timestamp, with the marker only on the last.
     */
    fun frame(stream: StreamId, payload: ByteArray): List<ByteArray> =
        frameAt(stream, payload, timestampNow())

    /** Frame a payload with an explicit timestamp. Lets tests pin the clock. */
    fun frameAt(stream: StreamId, payload: ByteArray, timestampUs: Long): List<ByteArray> {
        val chunks: List<ByteArray> = if (payload.size <= MAX_PAYLOAD) {
            listOf(payload)
        } else {
            payload.asIterable().chunked(MAX_PAYLOAD) { it.toByteArray() }
        }

        val fragmented = chunks.size > 1
        val firstSequence = nextSequence(stream, chunks.size)

        return chunks.mapIndexed { index, chunk ->
            val header = MediaHeader(
                stream = stream,
                sequence = (firstSequence + index) and 0xFFFF,
                timestampUs = timestampUs,
                sessionId = sessionId,
                fragment = fragmented,
                marker = index == chunks.size - 1,
            )
            ByteArray(HEADER_LEN + chunk.size).also { out ->
                header.encode().copyInto(out)
                chunk.copyInto(out, HEADER_LEN)
            }
        }
    }
}
