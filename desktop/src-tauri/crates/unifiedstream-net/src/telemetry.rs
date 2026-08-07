//! Link-quality measurement: RTT, throughput, loss, and jitter.
//!
//! Each side computes locally and reports once per second. Sampling at 1 Hz rather than
//! per-packet keeps the overhead negligible and matches what a human can actually read on a
//! dashboard.

use std::time::{Duration, Instant};

use crate::protocol::TelemetryReport;

/// How often telemetry is reported.
pub const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// Smoothing factor for the round-trip average.
pub const RTT_ALPHA: f64 = 0.2;

/// Exponentially weighted round-trip average.
///
/// Smoothed rather than raw because a single sample bounces enough to make the dashboard
/// unreadable, and the user is trying to answer "is the link good", not "what was packet 4079".
#[derive(Debug, Default, Clone)]
pub struct RttTracker {
    smoothed_ms: Option<f64>,
    samples: usize,
}

impl RttTracker {
    /// A tracker with no samples.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a round-trip sample in milliseconds. Nonsensical samples are ignored.
    pub fn record(&mut self, sample_ms: f64) {
        if !sample_ms.is_finite() || sample_ms < 0.0 {
            return;
        }
        self.samples = self.samples.saturating_add(1);
        self.smoothed_ms = Some(match self.smoothed_ms {
            None => sample_ms,
            Some(current) => RTT_ALPHA.mul_add(sample_ms, (1.0 - RTT_ALPHA) * current),
        });
    }

    /// The smoothed round-trip time, or `None` before the first sample.
    #[must_use]
    pub const fn smoothed_ms(&self) -> Option<f64> {
        self.smoothed_ms
    }

    /// How many samples have been recorded.
    #[must_use]
    pub const fn sample_count(&self) -> usize {
        self.samples
    }

    /// Forget every sample, for when a session ends.
    pub fn reset(&mut self) {
        self.smoothed_ms = None;
        self.samples = 0;
    }
}

/// Byte counter that converts to megabits per second over an interval.
#[derive(Debug, Clone)]
pub struct ThroughputMeter {
    bytes: u64,
    window_start: Instant,
    last_mbps: f64,
}

impl Default for ThroughputMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl ThroughputMeter {
    /// A meter starting now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bytes: 0,
            window_start: Instant::now(),
            last_mbps: 0.0,
        }
    }

    /// Count bytes moved.
    pub fn add(&mut self, bytes: u64) {
        self.bytes = self.bytes.saturating_add(bytes);
    }

    /// Close the current interval and return its rate in megabits per second.
    ///
    /// An interval with no traffic yields zero rather than the previous reading, so an idle
    /// link reads as idle instead of appearing frozen at its last value.
    pub fn sample(&mut self) -> f64 {
        let elapsed = self.window_start.elapsed().as_secs_f64();
        self.last_mbps = if elapsed <= 0.0 {
            0.0
        } else {
            #[allow(
                clippy::cast_precision_loss,
                reason = "byte counts stay far below 2^53"
            )]
            let bits = (self.bytes as f64) * 8.0;
            bits / elapsed / 1_000_000.0
        };
        self.bytes = 0;
        self.window_start = Instant::now();
        self.last_mbps
    }

    /// The most recent sampled rate.
    #[must_use]
    pub const fn last_mbps(&self) -> f64 {
        self.last_mbps
    }

    /// Reset both the counter and the window.
    pub fn reset(&mut self) {
        self.bytes = 0;
        self.window_start = Instant::now();
        self.last_mbps = 0.0;
    }
}

/// Interarrival jitter, per RFC 3550 §6.4.1.
///
/// `J += (|D(i-1,i)| - J) / 16`, where `D` is the difference between the transit times of
/// consecutive packets.
#[derive(Debug, Default, Clone)]
pub struct JitterEstimator {
    jitter_us: f64,
    last_transit_us: Option<f64>,
}

impl JitterEstimator {
    /// An estimator with no history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a packet's sender timestamp and local arrival time, both in microseconds.
    pub fn record(&mut self, sent_us: u64, arrived_us: u64) {
        #[allow(
            clippy::cast_precision_loss,
            reason = "microsecond clocks fit f64 exactly here"
        )]
        let transit = arrived_us as f64 - sent_us as f64;

        if let Some(previous) = self.last_transit_us {
            let d = (transit - previous).abs();
            self.jitter_us += (d - self.jitter_us) / 16.0;
        }
        self.last_transit_us = Some(transit);
    }

    /// Current jitter estimate in milliseconds.
    #[must_use]
    pub fn jitter_ms(&self) -> f64 {
        self.jitter_us / 1000.0
    }

    /// Forget every sample.
    pub fn reset(&mut self) {
        self.jitter_us = 0.0;
        self.last_transit_us = None;
    }
}

/// Packet loss over the reporting interval, from sequence gaps.
#[derive(Debug, Default, Clone, Copy)]
pub struct LossMeter {
    received: u64,
    lost: u64,
    last_pct: f64,
}

impl LossMeter {
    /// A meter with no history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record counts observed since the last sample.
    ///
    /// Late packets are deliberately not passed here: they arrived, so counting them as lost
    /// would double-count a single degradation.
    pub fn record(&mut self, received: u64, lost: u64) {
        self.received = self.received.saturating_add(received);
        self.lost = self.lost.saturating_add(lost);
    }

    /// Close the interval and return loss as a percentage of expected packets.
    pub fn sample(&mut self) -> f64 {
        let expected = self.received + self.lost;
        self.last_pct = if expected == 0 {
            0.0
        } else {
            #[allow(
                clippy::cast_precision_loss,
                reason = "counter magnitudes stay below 2^53"
            )]
            let pct = (self.lost as f64 / expected as f64) * 100.0;
            pct
        };
        self.received = 0;
        self.lost = 0;
        self.last_pct
    }

    /// The most recent sampled percentage.
    #[must_use]
    pub const fn last_pct(&self) -> f64 {
        self.last_pct
    }

    /// Reset the interval counters.
    pub fn reset(&mut self) {
        self.received = 0;
        self.lost = 0;
        self.last_pct = 0.0;
    }
}

/// Qualitative link health, for a dashboard indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkQuality {
    /// Comfortably within the latency budget.
    Good,
    /// Usable, but the user would notice.
    Degraded,
    /// Not usable for live media.
    Poor,
    /// Not enough information yet.
    Unknown,
}

impl LinkQuality {
    /// Classify from round-trip time and loss.
    ///
    /// Thresholds come from the sub-50ms end-to-end target: transport latency has to stay in
    /// single-digit milliseconds for the codec and display pipeline to fit in the rest.
    #[must_use]
    pub fn classify(rtt_ms: Option<f64>, loss_pct: f64) -> Self {
        let Some(rtt) = rtt_ms else {
            return Self::Unknown;
        };
        if rtt > 50.0 || loss_pct > 5.0 {
            Self::Poor
        } else if rtt > 20.0 || loss_pct > 1.0 {
            Self::Degraded
        } else {
            Self::Good
        }
    }
}

/// Aggregates every measurement into the 1 Hz report.
#[derive(Debug, Default)]
pub struct TelemetryCollector {
    rtt: RttTracker,
    tx: ThroughputMeter,
    rx: ThroughputMeter,
    loss: LossMeter,
    jitter: JitterEstimator,
}

impl TelemetryCollector {
    /// A collector with no history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a round-trip sample in milliseconds.
    pub fn record_rtt(&mut self, sample_ms: f64) {
        self.rtt.record(sample_ms);
    }

    /// Count bytes sent.
    pub fn record_sent(&mut self, bytes: u64) {
        self.tx.add(bytes);
    }

    /// Count bytes received.
    pub fn record_received(&mut self, bytes: u64) {
        self.rx.add(bytes);
    }

    /// Record packet counts observed since the last report.
    pub fn record_packets(&mut self, received: u64, lost: u64) {
        self.loss.record(received, lost);
    }

    /// Record a packet's timing for the jitter estimate.
    pub fn record_arrival(&mut self, sent_us: u64, arrived_us: u64) {
        self.jitter.record(sent_us, arrived_us);
    }

    /// Close the interval and produce the report to send.
    pub fn sample(&mut self) -> TelemetryReport {
        TelemetryReport {
            rtt_ms: self.rtt.smoothed_ms(),
            tx_mbps: self.tx.sample(),
            rx_mbps: self.rx.sample(),
            loss_pct: self.loss.sample(),
            jitter_ms: self.jitter.jitter_ms(),
        }
    }

    /// Link quality implied by the most recent sample.
    #[must_use]
    pub fn quality(&self) -> LinkQuality {
        LinkQuality::classify(self.rtt.smoothed_ms(), self.loss.last_pct())
    }

    /// Forget everything, for when a session ends.
    ///
    /// The dashboard must clear rather than keep showing a dead session's last reading as if
    /// it were current.
    pub fn reset(&mut self) {
        self.rtt.reset();
        self.tx.reset();
        self.rx.reset();
        self.loss.reset();
        self.jitter.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtt_should_be_unavailable_before_the_first_sample() {
        assert_eq!(RttTracker::new().smoothed_ms(), None);
    }

    #[test]
    fn the_first_rtt_sample_should_be_taken_verbatim() {
        let mut tracker = RttTracker::new();
        tracker.record(12.0);
        assert_eq!(tracker.smoothed_ms(), Some(12.0));
    }

    #[test]
    fn rtt_should_be_smoothed_rather_than_track_the_latest_sample() {
        let mut tracker = RttTracker::new();
        tracker.record(10.0);
        tracker.record(50.0);

        let smoothed = tracker.smoothed_ms().unwrap_or_default();
        assert!(
            smoothed > 10.0 && smoothed < 50.0,
            "a spike must not become the reported value: {smoothed}"
        );
        // 0.2 * 50 + 0.8 * 10 = 18
        assert!((smoothed - 18.0).abs() < 1e-9);
    }

    #[test]
    fn rtt_should_converge_towards_a_steady_value() {
        let mut tracker = RttTracker::new();
        tracker.record(100.0);
        for _ in 0..50 {
            tracker.record(5.0);
        }
        let smoothed = tracker.smoothed_ms().unwrap_or_default();
        assert!(
            (smoothed - 5.0).abs() < 0.1,
            "should converge, got {smoothed}"
        );
    }

    #[test]
    fn rtt_should_ignore_nonsensical_samples() {
        let mut tracker = RttTracker::new();
        tracker.record(-5.0);
        tracker.record(f64::NAN);
        assert_eq!(tracker.smoothed_ms(), None);
        assert_eq!(tracker.sample_count(), 0);
    }

    #[test]
    fn resetting_should_make_rtt_unavailable_again() {
        let mut tracker = RttTracker::new();
        tracker.record(10.0);
        tracker.reset();
        assert_eq!(tracker.smoothed_ms(), None);
    }

    #[test]
    fn throughput_should_be_within_ten_percent_of_a_known_rate() {
        let mut meter = ThroughputMeter::new();
        // 125_000 bytes = 1 megabit.
        meter.add(125_000);
        std::thread::sleep(Duration::from_millis(100));

        let mbps = meter.sample();
        // 1 Mbit over ~0.1s is ~10 Mbps.
        assert!(
            (mbps - 10.0).abs() / 10.0 < 0.15,
            "expected ~10 Mbps, got {mbps}"
        );
    }

    #[test]
    fn throughput_should_fall_to_zero_over_an_idle_interval() {
        let mut meter = ThroughputMeter::new();
        meter.add(100_000);
        meter.sample();

        std::thread::sleep(Duration::from_millis(10));
        assert!(
            (meter.sample() - 0.0).abs() < f64::EPSILON,
            "an idle interval must read zero, not the previous rate"
        );
    }

    #[test]
    fn throughput_should_reset_its_counter_each_interval() {
        let mut meter = ThroughputMeter::new();
        meter.add(125_000);
        // Both windows have to be measurably long. Sampling an interval the clock reports as
        // zero yields zero by definition, which says nothing about whether the counter reset —
        // and on a platform whose clock ticks coarsely, back-to-back calls do land in the same
        // tick.
        std::thread::sleep(Duration::from_millis(1));
        let first = meter.sample();
        meter.add(125_000);
        std::thread::sleep(Duration::from_millis(1));
        let second = meter.sample();
        assert!(first > 0.0 && second > 0.0);
    }

    #[test]
    fn loss_should_be_the_ratio_of_missing_to_expected() {
        let mut meter = LossMeter::new();
        meter.record(95, 5);
        assert!((meter.sample() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn loss_should_be_zero_on_a_clean_interval() {
        let mut meter = LossMeter::new();
        meter.record(100, 0);
        assert!((meter.sample() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn loss_should_be_zero_when_nothing_was_expected() {
        assert!((LossMeter::new().sample() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn loss_should_reset_between_intervals() {
        let mut meter = LossMeter::new();
        meter.record(50, 50);
        assert!((meter.sample() - 50.0).abs() < 1e-9);
        assert!(
            (meter.sample() - 0.0).abs() < f64::EPSILON,
            "the next interval starts clean"
        );
    }

    #[test]
    fn jitter_should_be_zero_for_perfectly_paced_arrivals() {
        let mut estimator = JitterEstimator::new();
        for i in 0..10_u64 {
            // Constant transit time: sent at i*1000, arrived 500us later.
            estimator.record(i * 1_000, i * 1_000 + 500);
        }
        assert!(
            estimator.jitter_ms() < 0.001,
            "even pacing must not register jitter, got {}",
            estimator.jitter_ms()
        );
    }

    #[test]
    fn jitter_should_rise_with_uneven_arrivals() {
        let mut estimator = JitterEstimator::new();
        estimator.record(0, 500);
        estimator.record(1_000, 6_000); // 5ms late
        estimator.record(2_000, 2_500);

        assert!(
            estimator.jitter_ms() > 0.0,
            "uneven arrivals must register jitter"
        );
    }

    #[test]
    fn resetting_should_clear_jitter() {
        let mut estimator = JitterEstimator::new();
        estimator.record(0, 500);
        estimator.record(1_000, 6_000);
        estimator.reset();
        assert!((estimator.jitter_ms() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn link_quality_should_be_unknown_without_an_rtt() {
        assert_eq!(LinkQuality::classify(None, 0.0), LinkQuality::Unknown);
    }

    #[test]
    fn a_fast_clean_link_should_be_good() {
        assert_eq!(LinkQuality::classify(Some(4.0), 0.0), LinkQuality::Good);
    }

    #[test]
    fn a_slow_link_should_be_degraded_then_poor() {
        assert_eq!(
            LinkQuality::classify(Some(25.0), 0.0),
            LinkQuality::Degraded
        );
        assert_eq!(LinkQuality::classify(Some(80.0), 0.0), LinkQuality::Poor);
    }

    #[test]
    fn loss_alone_should_degrade_the_rating() {
        assert_eq!(LinkQuality::classify(Some(3.0), 2.0), LinkQuality::Degraded);
        assert_eq!(LinkQuality::classify(Some(3.0), 10.0), LinkQuality::Poor);
    }

    #[test]
    fn a_report_should_carry_every_measurement() {
        let mut collector = TelemetryCollector::new();
        collector.record_rtt(8.0);
        collector.record_sent(125_000);
        collector.record_received(12_500);
        collector.record_packets(99, 1);
        collector.record_arrival(0, 500);
        collector.record_arrival(1_000, 2_000);

        let report = collector.sample();
        assert_eq!(report.rtt_ms, Some(8.0));
        assert!(report.tx_mbps > 0.0);
        assert!(report.rx_mbps > 0.0);
        assert!((report.loss_pct - 1.0).abs() < 1e-9);
        assert!(report.jitter_ms >= 0.0);
    }

    #[test]
    fn a_fresh_collector_should_report_an_unavailable_rtt() {
        assert_eq!(TelemetryCollector::new().sample().rtt_ms, None);
    }

    #[test]
    fn resetting_the_collector_should_clear_every_reading() {
        let mut collector = TelemetryCollector::new();
        collector.record_rtt(8.0);
        collector.record_sent(125_000);
        collector.record_packets(90, 10);
        collector.sample();

        collector.reset();

        let report = collector.sample();
        assert_eq!(
            report.rtt_ms, None,
            "a dead session must not show a stale RTT"
        );
        assert!((report.loss_pct - 0.0).abs() < f64::EPSILON);
        assert_eq!(collector.quality(), LinkQuality::Unknown);
    }

    #[test]
    fn the_report_interval_should_be_one_second() {
        assert_eq!(REPORT_INTERVAL, Duration::from_secs(1));
    }
}
