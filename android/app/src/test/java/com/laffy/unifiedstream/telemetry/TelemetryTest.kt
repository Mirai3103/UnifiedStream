package com.laffy.unifiedstream.telemetry

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RttTrackerTest {

    @Test
    fun rttShouldBeUnavailableBeforeTheFirstSample() {
        assertNull(RttTracker().smoothedMs)
    }

    @Test
    fun theFirstSampleShouldBeTakenVerbatim() {
        val tracker = RttTracker()
        tracker.record(12.0)
        assertEquals(12.0, tracker.smoothedMs!!, 1e-9)
    }

    @Test
    fun rttShouldBeSmoothedRatherThanTrackTheLatestSample() {
        val tracker = RttTracker()
        tracker.record(10.0)
        tracker.record(50.0)

        // 0.2 * 50 + 0.8 * 10 = 18
        assertEquals(18.0, tracker.smoothedMs!!, 1e-9)
    }

    @Test
    fun rttShouldConvergeTowardsASteadyValue() {
        val tracker = RttTracker()
        tracker.record(100.0)
        repeat(50) { tracker.record(5.0) }
        assertEquals(5.0, tracker.smoothedMs!!, 0.1)
    }

    @Test
    fun rttShouldIgnoreNonsensicalSamples() {
        val tracker = RttTracker()
        tracker.record(-5.0)
        tracker.record(Double.NaN)
        assertNull(tracker.smoothedMs)
        assertEquals(0, tracker.sampleCount)
    }

    @Test
    fun resettingShouldMakeRttUnavailableAgain() {
        val tracker = RttTracker()
        tracker.record(10.0)
        tracker.reset()
        assertNull(tracker.smoothedMs)
    }
}

class ThroughputMeterTest {

    /** A controllable clock keeps throughput assertions exact rather than timing-dependent. */
    private class FakeClock {
        var nanos: Long = 0
        fun read(): Long = nanos
        fun advanceMillis(ms: Long) {
            nanos += ms * 1_000_000
        }
    }

    @Test
    fun throughputShouldMatchAKnownRate() {
        val clock = FakeClock()
        val meter = ThroughputMeter(clock::read)

        meter.add(125_000) // 1 megabit
        clock.advanceMillis(1_000)

        assertEquals("1 megabit over 1 second is 1 Mbps", 1.0, meter.sample(), 1e-6)
    }

    @Test
    fun throughputShouldBeWithinTenPercentOfAKnownRate() {
        val clock = FakeClock()
        val meter = ThroughputMeter(clock::read)

        meter.add(1_250_000) // 10 megabits
        clock.advanceMillis(1_000)

        val mbps = meter.sample()
        assertTrue("expected ~10 Mbps, got $mbps", kotlin.math.abs(mbps - 10.0) / 10.0 < 0.10)
    }

    @Test
    fun throughputShouldFallToZeroOverAnIdleInterval() {
        val clock = FakeClock()
        val meter = ThroughputMeter(clock::read)

        meter.add(125_000)
        clock.advanceMillis(1_000)
        meter.sample()

        clock.advanceMillis(1_000)
        assertEquals(
            "an idle interval must read zero, not the previous rate",
            0.0,
            meter.sample(),
            1e-9,
        )
    }

    @Test
    fun throughputShouldResetItsCounterEachInterval() {
        val clock = FakeClock()
        val meter = ThroughputMeter(clock::read)

        meter.add(125_000)
        clock.advanceMillis(1_000)
        assertEquals(1.0, meter.sample(), 1e-6)

        meter.add(125_000)
        clock.advanceMillis(1_000)
        assertEquals("the second interval counts only its own bytes", 1.0, meter.sample(), 1e-6)
    }
}

class LossMeterTest {

    @Test
    fun lossShouldBeTheRatioOfMissingToExpected() {
        val meter = LossMeter()
        meter.record(95, 5)
        assertEquals(5.0, meter.sample(), 1e-9)
    }

    @Test
    fun lossShouldBeZeroOnACleanInterval() {
        val meter = LossMeter()
        meter.record(100, 0)
        assertEquals(0.0, meter.sample(), 1e-9)
    }

    @Test
    fun lossShouldBeZeroWhenNothingWasExpected() {
        assertEquals(0.0, LossMeter().sample(), 1e-9)
    }

    @Test
    fun lossShouldResetBetweenIntervals() {
        val meter = LossMeter()
        meter.record(50, 50)
        assertEquals(50.0, meter.sample(), 1e-9)
        assertEquals("the next interval starts clean", 0.0, meter.sample(), 1e-9)
    }
}

class JitterEstimatorTest {

    @Test
    fun jitterShouldBeZeroForPerfectlyPacedArrivals() {
        val estimator = JitterEstimator()
        for (i in 0 until 10) {
            estimator.record(i * 1_000L, i * 1_000L + 500)
        }
        assertTrue(
            "even pacing must not register jitter, got ${estimator.jitterMs}",
            estimator.jitterMs < 0.001,
        )
    }

    @Test
    fun jitterShouldRiseWithUnevenArrivals() {
        val estimator = JitterEstimator()
        estimator.record(0, 500)
        estimator.record(1_000, 6_000)
        estimator.record(2_000, 2_500)

        assertTrue("uneven arrivals must register jitter", estimator.jitterMs > 0.0)
    }

    @Test
    fun resettingShouldClearJitter() {
        val estimator = JitterEstimator()
        estimator.record(0, 500)
        estimator.record(1_000, 6_000)
        estimator.reset()
        assertEquals(0.0, estimator.jitterMs, 1e-9)
    }
}

class LinkQualityTest {

    @Test
    fun qualityShouldBeUnknownWithoutAnRtt() {
        assertEquals(LinkQuality.UNKNOWN, LinkQuality.classify(null, 0.0))
    }

    @Test
    fun aFastCleanLinkShouldBeGood() {
        assertEquals(LinkQuality.GOOD, LinkQuality.classify(4.0, 0.0))
    }

    @Test
    fun aSlowLinkShouldBeDegradedThenPoor() {
        assertEquals(LinkQuality.DEGRADED, LinkQuality.classify(25.0, 0.0))
        assertEquals(LinkQuality.POOR, LinkQuality.classify(80.0, 0.0))
    }

    @Test
    fun lossAloneShouldDegradeTheRating() {
        assertEquals(LinkQuality.DEGRADED, LinkQuality.classify(3.0, 2.0))
        assertEquals(LinkQuality.POOR, LinkQuality.classify(3.0, 10.0))
    }
}

class TelemetryCollectorTest {

    @Test
    fun aReportShouldCarryEveryMeasurement() {
        val collector = TelemetryCollector()
        collector.recordRtt(8.0)
        collector.recordSent(125_000)
        collector.recordReceived(12_500)
        collector.recordPackets(99, 1)
        collector.recordArrival(0, 500)
        collector.recordArrival(1_000, 2_000)

        val report = collector.sample()
        assertEquals(8.0, report.rttMs!!, 1e-9)
        assertEquals(1.0, report.lossPct, 1e-9)
        assertTrue(report.jitterMs >= 0.0)
    }

    @Test
    fun aFreshCollectorShouldReportAnUnavailableRtt() {
        assertNull(TelemetryCollector().sample().rttMs)
    }

    @Test
    fun resettingShouldClearEveryReading() {
        val collector = TelemetryCollector()
        collector.recordRtt(8.0)
        collector.recordPackets(90, 10)
        collector.sample()

        collector.reset()

        val report = collector.sample()
        assertNull("a dead session must not show a stale RTT", report.rttMs)
        assertEquals(0.0, report.lossPct, 1e-9)
        assertEquals(LinkQuality.UNKNOWN, collector.quality())
    }

    @Test
    fun theReportIntervalShouldBeOneSecond() {
        assertEquals(1_000L, REPORT_INTERVAL_MS)
    }
}
