package com.laffy.unifiedstream.telemetry

import com.laffy.unifiedstream.protocol.ControlMessage

/**
 * Link-quality measurement: RTT, throughput, loss, and jitter.
 *
 * Mirrors `unifiedstream_net::telemetry`. Each side computes locally and reports once per
 * second; 1 Hz keeps the overhead negligible and matches what a human can read on a dashboard.
 */

/** How often telemetry is reported, in milliseconds. */
const val REPORT_INTERVAL_MS: Long = 1_000

/** Smoothing factor for the round-trip average. */
const val RTT_ALPHA: Double = 0.2

/**
 * Exponentially weighted round-trip average.
 *
 * Smoothed rather than raw because a single sample bounces enough to make the dashboard
 * unreadable, and the user is trying to answer "is the link good".
 */
class RttTracker {
    var smoothedMs: Double? = null
        private set

    var sampleCount: Int = 0
        private set

    /** Record a round-trip sample in milliseconds. Nonsensical samples are ignored. */
    fun record(sampleMs: Double) {
        if (!sampleMs.isFinite() || sampleMs < 0.0) return
        sampleCount += 1
        val current = smoothedMs
        smoothedMs = if (current == null) sampleMs else RTT_ALPHA * sampleMs + (1 - RTT_ALPHA) * current
    }

    fun reset() {
        smoothedMs = null
        sampleCount = 0
    }
}

/** Byte counter that converts to megabits per second over an interval. */
class ThroughputMeter(private val clock: () -> Long = System::nanoTime) {
    private var bytes: Long = 0
    private var windowStart: Long = clock()

    var lastMbps: Double = 0.0
        private set

    /** Count bytes moved. */
    fun add(count: Long) {
        bytes += count
    }

    /**
     * Close the current interval and return its rate in megabits per second.
     *
     * An interval with no traffic yields zero rather than the previous reading, so an idle link
     * reads as idle instead of appearing frozen at its last value.
     */
    fun sample(): Double {
        val now = clock()
        val elapsedSeconds = (now - windowStart) / 1_000_000_000.0
        lastMbps = if (elapsedSeconds <= 0.0) 0.0 else bytes * 8.0 / elapsedSeconds / 1_000_000.0
        bytes = 0
        windowStart = now
        return lastMbps
    }

    fun reset() {
        bytes = 0
        windowStart = clock()
        lastMbps = 0.0
    }
}

/**
 * Interarrival jitter, per RFC 3550 section 6.4.1.
 *
 * `J += (|D(i-1,i)| - J) / 16`, where `D` is the difference between the transit times of
 * consecutive packets.
 */
class JitterEstimator {
    private var jitterUs: Double = 0.0
    private var lastTransitUs: Double? = null

    /** Record a packet's sender timestamp and local arrival time, both in microseconds. */
    fun record(sentUs: Long, arrivedUs: Long) {
        val transit = (arrivedUs - sentUs).toDouble()
        val previous = lastTransitUs
        if (previous != null) {
            val d = kotlin.math.abs(transit - previous)
            jitterUs += (d - jitterUs) / 16.0
        }
        lastTransitUs = transit
    }

    /** Current jitter estimate in milliseconds. */
    val jitterMs: Double get() = jitterUs / 1000.0

    fun reset() {
        jitterUs = 0.0
        lastTransitUs = null
    }
}

/** Packet loss over the reporting interval, from sequence gaps. */
class LossMeter {
    private var received: Long = 0
    private var lost: Long = 0

    var lastPct: Double = 0.0
        private set

    /**
     * Record counts observed since the last sample.
     *
     * Late packets are deliberately not passed here: they arrived, so counting them as lost
     * would double-count a single degradation.
     */
    fun record(receivedCount: Long, lostCount: Long) {
        received += receivedCount
        lost += lostCount
    }

    /** Close the interval and return loss as a percentage of expected packets. */
    fun sample(): Double {
        val expected = received + lost
        lastPct = if (expected == 0L) 0.0 else lost.toDouble() / expected * 100.0
        received = 0
        lost = 0
        return lastPct
    }

    fun reset() {
        received = 0
        lost = 0
        lastPct = 0.0
    }
}

/** Qualitative link health, for a dashboard indicator. */
enum class LinkQuality {
    /** Comfortably within the latency budget. */
    GOOD,

    /** Usable, but the user would notice. */
    DEGRADED,

    /** Not usable for live media. */
    POOR,

    /** Not enough information yet. */
    UNKNOWN;

    companion object {
        /**
         * Classify from round-trip time and loss.
         *
         * Thresholds come from the sub-50ms end-to-end target: transport latency has to stay in
         * single-digit milliseconds for the codec and display pipeline to fit in the rest.
         */
        fun classify(rttMs: Double?, lossPct: Double): LinkQuality = when {
            rttMs == null -> UNKNOWN
            rttMs > 50.0 || lossPct > 5.0 -> POOR
            rttMs > 20.0 || lossPct > 1.0 -> DEGRADED
            else -> GOOD
        }
    }
}

/** Aggregates every measurement into the 1 Hz report. */
class TelemetryCollector(clock: () -> Long = System::nanoTime) {
    private val rtt = RttTracker()
    private val tx = ThroughputMeter(clock)
    private val rx = ThroughputMeter(clock)
    private val loss = LossMeter()
    private val jitter = JitterEstimator()

    fun recordRtt(sampleMs: Double) = rtt.record(sampleMs)
    fun recordSent(bytes: Long) = tx.add(bytes)
    fun recordReceived(bytes: Long) = rx.add(bytes)
    fun recordPackets(received: Long, lost: Long) = this.loss.record(received, lost)
    fun recordArrival(sentUs: Long, arrivedUs: Long) = jitter.record(sentUs, arrivedUs)

    /** Close the interval and produce the report to send. */
    fun sample(): ControlMessage.Telemetry = ControlMessage.Telemetry(
        rttMs = rtt.smoothedMs,
        txMbps = tx.sample(),
        rxMbps = rx.sample(),
        lossPct = loss.sample(),
        jitterMs = jitter.jitterMs,
    )

    /** Link quality implied by the most recent sample. */
    fun quality(): LinkQuality = LinkQuality.classify(rtt.smoothedMs, loss.lastPct)

    /**
     * Forget everything, for when a session ends.
     *
     * The dashboard must clear rather than keep showing a dead session's last reading as if it
     * were current.
     */
    fun reset() {
        rtt.reset()
        tx.reset()
        rx.reset()
        loss.reset()
        jitter.reset()
    }
}
