# Connection Telemetry Specification

## Purpose

Measurement and presentation of link quality — round-trip latency, throughput, packet loss, and jitter — and the dashboards that make a degraded link diagnosable without attaching a debugger.

See `openspec/specs/protocol.md` for the normative wire format these requirements are implemented against.

## Requirements

### Requirement: Round-trip latency measurement

The system SHALL measure and report round-trip latency between phone and desktop, smoothed to avoid single-sample noise.

#### Scenario: Latency is reported once a session is connected

- **WHEN** a session has been Connected for at least 2 seconds
- **THEN** both apps display a round-trip latency value in milliseconds

#### Scenario: Latency is smoothed

- **WHEN** consecutive round-trip samples vary
- **THEN** the reported value is an exponentially weighted moving average over the most recent samples rather than the raw latest sample

#### Scenario: Latency is unavailable before connection

- **WHEN** no session is established
- **THEN** the latency display shows an explicit unavailable state rather than a stale or zero value

### Requirement: Throughput measurement

The system SHALL measure and report media throughput in megabits per second, separately for sent and received directions.

#### Scenario: Throughput reflects active streaming

- **WHEN** the synthetic test stream is running at a known rate
- **THEN** the reported throughput is within 10% of the actual byte rate over the sampling interval

#### Scenario: Throughput falls to zero when idle

- **WHEN** no media packets have been sent or received for one full sampling interval
- **THEN** the reported throughput for that direction is zero

### Requirement: Packet loss measurement

The system SHALL compute and report packet loss as a percentage of expected packets over the sampling interval, derived from sequence gaps.

#### Scenario: Loss is reported from sequence gaps

- **WHEN** 100 packets are expected in an interval and 5 sequence numbers are missing
- **THEN** the reported packet loss for that interval is 5%

#### Scenario: Late packets are counted separately from loss

- **WHEN** a packet arrives after its reorder slot has passed
- **THEN** it is counted as late and not double-counted as lost

#### Scenario: No loss on a clean link

- **WHEN** every expected packet arrives within its reorder window
- **THEN** the reported packet loss is 0%

### Requirement: Jitter measurement

The system SHALL compute and report interarrival jitter for received media streams.

#### Scenario: Jitter is computed from interarrival timing

- **WHEN** media packets are being received
- **THEN** jitter is computed as the smoothed mean deviation of packet interarrival time relative to sender timestamps, and reported in milliseconds

### Requirement: Telemetry reporting interval

Each peer SHALL send a telemetry report over the control channel once per second while a session is active.

#### Scenario: Reports arrive at 1 Hz

- **WHEN** a session is Connected
- **THEN** each peer sends a `telemetry` control message containing latency, sent and received throughput, packet loss, and jitter once per second

#### Scenario: Reporting stops with the session

- **WHEN** a session ends
- **THEN** telemetry reports stop being sent

### Requirement: Status dashboard

Both applications SHALL present a status dashboard showing live connection quality metrics and the current connection state.

#### Scenario: Dashboard shows live metrics

- **WHEN** a session is Connected
- **THEN** the dashboard displays current latency in milliseconds, throughput in Mbps, packet loss as a percentage, and jitter in milliseconds
- **AND** the values update at least once per second

#### Scenario: Dashboard shows connection state

- **WHEN** the connection state changes
- **THEN** the dashboard reflects the new state, showing the peer device name while Connected and the reason while Failed

#### Scenario: Link quality is summarized visually

- **WHEN** the dashboard is displaying metrics
- **THEN** it shows a qualitative link-quality indicator derived from latency and loss, distinguishing good, degraded, and poor conditions

#### Scenario: Metrics are cleared on disconnect

- **WHEN** the session ends
- **THEN** the dashboard clears live metric values rather than leaving the last reading displayed as if current

### Requirement: Feature toggles reflect unavailable capabilities

Both applications SHALL render Camera, Microphone, and Speaker toggles in the main UI, disabled until the corresponding media capability is implemented.

#### Scenario: Toggles are visible but inert

- **WHEN** the user views the main screen with a session Connected
- **THEN** Camera, Microphone, and Speaker toggles are visible in a disabled state indicating the feature is not yet available
- **AND** interacting with them does not change any session state
