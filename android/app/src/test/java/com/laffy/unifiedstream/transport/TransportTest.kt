package com.laffy.unifiedstream.transport

import com.laffy.unifiedstream.protocol.HEADER_LEN
import com.laffy.unifiedstream.protocol.MAX_PAYLOAD
import com.laffy.unifiedstream.protocol.MediaHeader
import com.laffy.unifiedstream.protocol.StreamId
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MediaSenderTest {

    private fun parse(datagram: ByteArray): Pair<MediaHeader, ByteArray> =
        MediaHeader.decode(datagram) to MediaHeader.payloadOf(datagram)

    @Test
    fun aSmallPayloadShouldProduceOneUnfragmentedPacket() {
        val tx = MediaSender(7)
        val datagrams = tx.frameAt(StreamId.TEST, "hello".toByteArray(), 1_000)

        assertEquals(1, datagrams.size)
        val (header, payload) = parse(datagrams[0])
        assertTrue("a single packet is not a fragment", !header.fragment)
        assertTrue("a single packet ends its frame", header.marker)
        assertEquals("hello", String(payload))
    }

    @Test
    fun aPayloadAtExactlyTheLimitShouldNotBeFragmented() {
        val tx = MediaSender(7)
        val datagrams = tx.frameAt(StreamId.TEST, ByteArray(MAX_PAYLOAD), 1_000)
        assertEquals(1, datagrams.size)
        assertTrue(!parse(datagrams[0]).first.fragment)
    }

    @Test
    fun aPayloadOneByteOverTheLimitShouldBeFragmented() {
        val tx = MediaSender(7)
        val datagrams = tx.frameAt(StreamId.TEST, ByteArray(MAX_PAYLOAD + 1), 1_000)
        assertEquals(2, datagrams.size)
        assertTrue(parse(datagrams[0]).first.fragment)
    }

    @Test
    fun onlyTheLastFragmentShouldCarryTheMarker() {
        val tx = MediaSender(7)
        val datagrams = tx.frameAt(StreamId.TEST, ByteArray(MAX_PAYLOAD * 3), 1_000)

        assertEquals(3, datagrams.size)
        assertTrue(!parse(datagrams[0]).first.marker)
        assertTrue(!parse(datagrams[1]).first.marker)
        assertTrue(parse(datagrams[2]).first.marker)
    }

    @Test
    fun everyFragmentShouldShareTheFramesTimestamp() {
        val tx = MediaSender(7)
        for (datagram in tx.frameAt(StreamId.TEST, ByteArray(MAX_PAYLOAD * 3), 0xDEADBEEFL)) {
            assertEquals(0xDEADBEEFL, parse(datagram).first.timestampUs)
        }
    }

    @Test
    fun fragmentsShouldOccupyConsecutiveSequenceNumbers() {
        val tx = MediaSender(7)
        val sequences = tx.frameAt(StreamId.TEST, ByteArray(MAX_PAYLOAD * 3), 1_000)
            .map { parse(it).first.sequence }
        assertEquals(listOf(0, 1, 2), sequences)
    }

    @Test
    fun noDatagramShouldExceedThePathMtu() {
        val tx = MediaSender(7)
        for (datagram in tx.frameAt(StreamId.TEST, ByteArray(MAX_PAYLOAD * 5 + 17), 1_000)) {
            assertTrue(
                "datagram of ${datagram.size} bytes exceeds the MTU budget",
                datagram.size <= HEADER_LEN + MAX_PAYLOAD,
            )
        }
    }

    @Test
    fun reassemblingTheFragmentsShouldReproduceThePayload() {
        val tx = MediaSender(7)
        val payload = ByteArray(MAX_PAYLOAD * 3 + 42) { (it % 251).toByte() }

        val rebuilt = tx.frameAt(StreamId.TEST, payload, 1_000)
            .fold(ByteArray(0)) { acc, d -> acc + parse(d).second }

        assertArrayEquals(payload, rebuilt)
    }

    @Test
    fun sequenceNumbersShouldAdvanceAcrossFrames() {
        val tx = MediaSender(7)
        val first = parse(tx.frameAt(StreamId.TEST, byteArrayOf(1), 1)[0]).first.sequence
        val second = parse(tx.frameAt(StreamId.TEST, byteArrayOf(2), 2)[0]).first.sequence
        assertEquals(first + 1, second)
    }

    @Test
    fun streamsShouldBeSequencedIndependently() {
        val tx = MediaSender(7)
        tx.frameAt(StreamId.TEST, byteArrayOf(1), 1)
        tx.frameAt(StreamId.TEST, byteArrayOf(2), 2)

        val camera = parse(tx.frameAt(StreamId.CAMERA, byteArrayOf(3), 3)[0]).first.sequence
        assertEquals("each stream starts its own sequence at zero", 0, camera)
    }

    @Test
    fun everyPacketShouldCarryTheSessionId() {
        val tx = MediaSender(0x0123456789ABCDEFL)
        for (datagram in tx.frameAt(StreamId.TEST, ByteArray(MAX_PAYLOAD * 2), 1_000)) {
            assertEquals(0x0123456789ABCDEFL, parse(datagram).first.sessionId)
        }
    }

    @Test
    fun sequenceNumbersShouldWrapRatherThanOverflow() {
        val tx = MediaSender(7)
        repeat(65_535) { tx.frameAt(StreamId.TEST, byteArrayOf(0), 1) }
        val last = parse(tx.frameAt(StreamId.TEST, byteArrayOf(0), 1)[0]).first.sequence
        val wrapped = parse(tx.frameAt(StreamId.TEST, byteArrayOf(0), 1)[0]).first.sequence

        assertEquals(65_535, last)
        assertEquals("the counter must wrap, not overflow", 0, wrapped)
    }

    @Test
    fun anEmptyPayloadShouldStillProduceAPacket() {
        val tx = MediaSender(7)
        val datagrams = tx.frameAt(StreamId.TEST, ByteArray(0), 1_000)
        assertEquals(1, datagrams.size)
        assertEquals(HEADER_LEN, datagrams[0].size)
    }
}

class StreamReceiverTest {

    private fun header(sequence: Int, timestampUs: Long, fragment: Boolean, marker: Boolean) =
        MediaHeader(StreamId.TEST, sequence, timestampUs, 7, fragment, marker)

    private fun whole(sequence: Int, timestampUs: Long) = header(sequence, timestampUs, false, true)

    @Test
    fun aSinglePacketFrameShouldBeDeliveredImmediately() {
        val rx = StreamReceiver()
        val frames = rx.accept(whole(0, 1_000), "hello".toByteArray())

        assertEquals(1, frames.size)
        assertEquals("hello", String(frames[0].payload))
        assertEquals(1_000L, frames[0].timestampUs)
    }

    @Test
    fun consecutiveFramesShouldEachBeDelivered() {
        val rx = StreamReceiver()
        assertEquals(1, rx.accept(whole(0, 1_000), byteArrayOf(1)).size)
        assertEquals(1, rx.accept(whole(1, 2_000), byteArrayOf(2)).size)
        assertEquals(1, rx.accept(whole(2, 3_000), byteArrayOf(3)).size)
        assertEquals(3, rx.stats.deliveredFrames)
        assertEquals(0, rx.stats.lost)
    }

    @Test
    fun aFragmentedFrameShouldBeReassembledInOrder() {
        val rx = StreamReceiver()
        assertTrue(rx.accept(header(0, 500, true, false), "abc".toByteArray()).isEmpty())
        assertTrue(rx.accept(header(1, 500, true, false), "def".toByteArray()).isEmpty())

        val frames = rx.accept(header(2, 500, true, true), "ghi".toByteArray())
        assertEquals(1, frames.size)
        assertEquals("abcdefghi", String(frames[0].payload))
    }

    @Test
    fun outOfOrderFragmentsShouldStillReassemble() {
        val rx = StreamReceiver()
        assertTrue(rx.accept(header(0, 500, true, false), "abc".toByteArray()).isEmpty())
        assertTrue(rx.accept(header(2, 500, true, true), "ghi".toByteArray()).isEmpty())

        val frames = rx.accept(header(1, 500, true, false), "def".toByteArray())
        assertEquals("the frame must complete once the gap fills", 1, frames.size)
        assertEquals("abcdefghi", String(frames[0].payload))
    }

    @Test
    fun reorderedPacketsShouldBeReleasedInSequenceOrder() {
        val rx = StreamReceiver()
        val delivered = mutableListOf<Frame>()

        delivered += rx.accept(whole(0, 100), "0".toByteArray())
        delivered += rx.accept(whole(2, 300), "2".toByteArray())
        delivered += rx.accept(whole(1, 200), "1".toByteArray())

        assertEquals(listOf("0", "1", "2"), delivered.map { String(it.payload) })
    }

    @Test
    fun theReorderBufferShouldNotStallOnALostPacket() {
        val rx = StreamReceiver()
        assertEquals(1, rx.accept(whole(0, 100), byteArrayOf(0)).size)

        var delivered = 0
        for (seq in 2..(2 + REORDER_WINDOW)) {
            delivered += rx.accept(whole(seq, seq * 100L), byteArrayOf(1)).size
        }

        assertTrue("the buffer must release rather than wait forever", delivered > 0)
        assertTrue("the gap must be counted as loss", rx.stats.lost >= 1)
    }

    @Test
    fun theReorderBufferShouldHoldAtMostTheWindow() {
        val rx = StreamReceiver()
        rx.accept(whole(0, 100), byteArrayOf(0))
        for (seq in 2 until 20) {
            rx.accept(whole(seq, seq * 100L), byteArrayOf(1))
        }
        assertTrue("buffer grew to ${rx.pendingCount} packets", rx.pendingCount <= REORDER_WINDOW)
    }

    @Test
    fun aGapShouldBeCountedAsLoss() {
        val rx = StreamReceiver()
        rx.accept(whole(10, 100), byteArrayOf(0))
        rx.accept(whole(11, 200), byteArrayOf(1))
        for (seq in 14..17) {
            rx.accept(whole(seq, seq * 100L), byteArrayOf(2))
        }
        assertEquals("exactly two packets went missing", 2, rx.stats.lost)
    }

    @Test
    fun aPacketOlderThanWhatWasReleasedShouldBeCountedLateNotLost() {
        val rx = StreamReceiver()
        rx.accept(whole(10, 100), byteArrayOf(0))
        rx.accept(whole(11, 200), byteArrayOf(1))

        val frames = rx.accept(whole(9, 50), "stale".toByteArray())

        assertTrue("a late packet must not be delivered", frames.isEmpty())
        assertEquals(1, rx.stats.late)
        assertEquals(0, rx.stats.lost)
    }

    @Test
    fun aSequenceWrapShouldNotBeCountedAsLoss() {
        val rx = StreamReceiver()
        rx.accept(whole(65_534, 100), byteArrayOf(0))
        rx.accept(whole(65_535, 200), byteArrayOf(1))
        rx.accept(whole(0, 300), byteArrayOf(2))
        rx.accept(whole(1, 400), byteArrayOf(3))

        assertEquals("wrapping is forward progress, not loss", 0, rx.stats.lost)
        assertEquals(4, rx.stats.deliveredFrames)
    }

    @Test
    fun anIncompleteFrameShouldBeDiscardedWhenANewerFrameStarts() {
        val rx = StreamReceiver()
        assertTrue(rx.accept(header(0, 500, true, false), "partial".toByteArray()).isEmpty())
        val frames = rx.accept(header(1, 900, false, true), "whole".toByteArray())

        assertEquals(1, frames.size)
        assertEquals("no partial frame may be delivered", "whole", String(frames[0].payload))
        assertEquals(1, rx.stats.incompleteFrames)
    }

    @Test
    fun joiningMidFrameShouldDropThePartialFrameRatherThanCorruptIt() {
        val rx = StreamReceiver()
        // The receiver never sees sequence 0, so it cannot know where this frame began.
        val delivered = mutableListOf<Frame>()
        delivered += rx.accept(header(1, 500, true, false), "bbb".toByteArray())
        delivered += rx.accept(header(2, 500, true, true), "ccc".toByteArray())

        assertTrue(
            "a frame missing its head must be dropped, never delivered with a missing prefix",
            delivered.isEmpty(),
        )
    }

    @Test
    fun aStreamShouldResynchroniseAfterJoiningMidFrame() {
        val rx = StreamReceiver()
        rx.accept(header(1, 500, true, false), "bbb".toByteArray())
        rx.accept(header(2, 500, true, true), "ccc".toByteArray())

        // The next complete frame must come through.
        val frames = rx.accept(header(3, 900, false, true), "next".toByteArray())
        assertEquals("the receiver must recover on the next frame", 1, frames.size)
        assertEquals("next", String(frames[0].payload))
    }

    @Test
    fun lossPercentageShouldBeZeroWhenNothingWasExpected() {
        assertEquals(0.0, StreamStats().lossPct, 1e-9)
    }

    @Test
    fun aCleanStreamShouldReportNoLoss() {
        val rx = StreamReceiver()
        for (seq in 0 until 100) {
            rx.accept(whole(seq, seq * 100L), byteArrayOf(0))
        }
        assertEquals(0.0, rx.stats.lossPct, 1e-9)
    }

    @Test
    fun statsShouldReportLossAsAPercentageOfExpected() {
        val rx = StreamReceiver()
        rx.accept(whole(0, 100), byteArrayOf(0))
        for (seq in 2..8) {
            rx.accept(whole(seq, seq * 100L), byteArrayOf(1))
        }
        assertEquals(rx.stats.received + rx.stats.lost, rx.stats.expected)
        assertTrue(rx.stats.lossPct > 0.0 && rx.stats.lossPct < 100.0)
    }

    @Test
    fun byteCountersShouldTrackAcceptedPayload() {
        val rx = StreamReceiver()
        rx.accept(whole(0, 100), "12345".toByteArray())
        rx.accept(whole(1, 200), "678".toByteArray())
        assertEquals(8, rx.stats.bytes)
    }

    @Test
    fun anEmptyPayloadShouldStillDeliverAFrame() {
        val rx = StreamReceiver()
        assertEquals(1, rx.accept(whole(0, 100), ByteArray(0)).size)
    }
}

class TimestampUnwrapperTest {

    @Test
    fun timestampsShouldStayMonotonicAcrossTheThirtyTwoBitWrap() {
        val unwrapper = TimestampUnwrapper()

        val before = unwrapper.unwrap(0xFFFFFFFFL - 1_000)
        val after = unwrapper.unwrap(1_000)

        assertTrue(
            "a session past 71 minutes must not go backwards: $before then $after",
            after > before,
        )
        assertEquals(2_001L, after - before)
    }

    @Test
    fun ordinaryProgressShouldBeLeftAlone() {
        val unwrapper = TimestampUnwrapper()
        assertEquals(1_000L, unwrapper.unwrap(1_000))
        assertEquals(2_000L, unwrapper.unwrap(2_000))
        assertEquals(3_000L, unwrapper.unwrap(3_000))
    }

    @Test
    fun aSmallBackwardsStepShouldNotBeReadAsAWrap() {
        val unwrapper = TimestampUnwrapper()
        unwrapper.unwrap(10_000)
        assertEquals(9_000L, unwrapper.unwrap(9_000))
        assertEquals(11_000L, unwrapper.unwrap(11_000))
    }
}

class MediaDemuxTest {

    @Test
    fun aRegisteredStreamShouldReceiveItsFrames() {
        val tx = MediaSender(7)
        val demux = MediaDemux(7).apply { register(StreamId.TEST) }

        val datagrams = tx.frameAt(StreamId.TEST, "hello".toByteArray(), 100)
        val result = demux.accept(datagrams[0])

        assertTrue(result is DemuxResult.Frames)
        val frames = (result as DemuxResult.Frames).frames
        assertEquals(1, frames.size)
        assertEquals("hello", String(frames[0].payload))
    }

    @Test
    fun aPacketForAnotherSessionShouldBeDropped() {
        val tx = MediaSender(999)
        val demux = MediaDemux(7).apply { register(StreamId.TEST) }

        val result = demux.accept(tx.frameAt(StreamId.TEST, "x".toByteArray(), 100)[0])
        assertEquals(DemuxResult.Dropped(DropReason.FOREIGN_SESSION), result)
        assertEquals(1, demux.drops.foreignSession)
    }

    @Test
    fun aPacketForAnUnregisteredStreamShouldBeDroppedAndCounted() {
        val tx = MediaSender(7)
        val demux = MediaDemux(7).apply { register(StreamId.TEST) }

        val result = demux.accept(tx.frameAt(StreamId.CAMERA, "x".toByteArray(), 100)[0])
        assertEquals(DemuxResult.Dropped(DropReason.UNKNOWN_STREAM), result)
        assertEquals(1, demux.drops.unknownStream)
    }

    @Test
    fun droppingOneStreamShouldNotDisturbAnother() {
        val tx = MediaSender(7)
        val demux = MediaDemux(7).apply { register(StreamId.TEST) }

        demux.accept(tx.frameAt(StreamId.CAMERA, "video".toByteArray(), 100)[0])
        val result = demux.accept(tx.frameAt(StreamId.TEST, "audio".toByteArray(), 200)[0])

        assertTrue(result is DemuxResult.Frames)
        assertEquals(1, (result as DemuxResult.Frames).frames.size)
    }

    @Test
    fun aTruncatedDatagramShouldBeDroppedAsMalformed() {
        val demux = MediaDemux(7).apply { register(StreamId.TEST) }
        for (len in 0 until HEADER_LEN) {
            assertEquals(
                DemuxResult.Dropped(DropReason.MALFORMED),
                demux.accept(ByteArray(len) { 0x40 }, len),
            )
        }
    }

    @Test
    fun anUnsupportedVersionShouldBeDropped() {
        val tx = MediaSender(7)
        val demux = MediaDemux(7).apply { register(StreamId.TEST) }

        val datagram = tx.frameAt(StreamId.TEST, "hello".toByteArray(), 100)[0]
        datagram[0] = ((datagram[0].toInt() and 0x3F) or (2 shl 6)).toByte()

        assertEquals(
            DemuxResult.Dropped(DropReason.UNSUPPORTED_VERSION),
            demux.accept(datagram),
        )
    }

    @Test
    fun aFragmentedFrameShouldSurviveTheFullSendAndReceivePath() {
        val tx = MediaSender(7)
        val demux = MediaDemux(7).apply { register(StreamId.TEST) }

        val payload = ByteArray(MAX_PAYLOAD * 4 + 7) { (it % 253).toByte() }
        val datagrams = tx.frameAt(StreamId.TEST, payload, 5_000)
        assertTrue("this payload must fragment", datagrams.size > 1)

        val delivered = mutableListOf<Frame>()
        for (datagram in datagrams) {
            val result = demux.accept(datagram)
            if (result is DemuxResult.Frames) delivered += result.frames
        }

        assertEquals(1, delivered.size)
        assertArrayEquals(payload, delivered[0].payload)
        assertEquals(5_000L, delivered[0].timestampUs)
    }

    @Test
    fun aLostFragmentShouldPreventDeliveryOfAPartialFrame() {
        val tx = MediaSender(7)
        val demux = MediaDemux(7).apply { register(StreamId.TEST) }

        val datagrams = tx.frameAt(StreamId.TEST, ByteArray(MAX_PAYLOAD * 3), 5_000)

        val delivered = mutableListOf<Frame>()
        datagrams.forEachIndexed { index, datagram ->
            if (index == 1) return@forEachIndexed
            val result = demux.accept(datagram)
            if (result is DemuxResult.Frames) delivered += result.frames
        }

        assertTrue(
            "a frame missing a fragment must never be delivered",
            delivered.isEmpty(),
        )
    }

    @Test
    fun totalStatsShouldSumEveryRegisteredStream() {
        val tx = MediaSender(7)
        val demux = MediaDemux(7).apply {
            register(StreamId.TEST)
            register(StreamId.CAMERA)
        }

        demux.accept(tx.frameAt(StreamId.TEST, "aaa".toByteArray(), 100)[0])
        demux.accept(tx.frameAt(StreamId.CAMERA, "bb".toByteArray(), 200)[0])

        val total = demux.totalStats()
        assertEquals(2, total.received)
        assertEquals(5, total.bytes)
        assertEquals(2, total.deliveredFrames)
    }

    @Test
    fun unregisteringAStreamShouldStopAcceptingIt() {
        val tx = MediaSender(7)
        val demux = MediaDemux(7).apply {
            register(StreamId.TEST)
            unregister(StreamId.TEST)
        }

        assertEquals(
            DemuxResult.Dropped(DropReason.UNKNOWN_STREAM),
            demux.accept(tx.frameAt(StreamId.TEST, "x".toByteArray(), 100)[0]),
        )
    }

    @Test
    fun dropStatsShouldTotalEveryCategory() {
        val demux = MediaDemux(7)
        demux.accept(ByteArray(0), 0)
        demux.accept(ByteArray(HEADER_LEN) { if (it == 0) 0x80.toByte() else 0 })
        assertEquals(2, demux.drops.total)
    }

    @Test
    fun statsForAnUnregisteredStreamShouldBeAbsent() {
        assertNull(MediaDemux(7).stats(StreamId.CAMERA))
    }
}
