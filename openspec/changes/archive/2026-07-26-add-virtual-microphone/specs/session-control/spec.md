# Session Control Specification (Delta)

## ADDED Requirements

### Requirement: Stream lifecycle messages

The control channel SHALL provide generic stream lifecycle messages: the media source announces a stream with `stream_start` carrying the stream identifier and format parameters (codec, sample rate, channels, frame duration); the sink replies with `stream_ack` accepting or refusing; and either peer ends a stream with `stream_stop`. Media packets for a stream MUST NOT be sent before an accepting `stream_ack` is received.

#### Scenario: Stream starts after acknowledgement

- **WHEN** the source sends `stream_start` for a stream and the sink replies with an accepting `stream_ack`
- **THEN** the source begins sending media packets on that stream identifier

#### Scenario: Refused stream sends no media

- **WHEN** the sink replies to `stream_start` with `stream_ack` where `accepted` is false and a machine-readable `reason`
- **THEN** the source does not send media packets on that stream
- **AND** the source UI reflects the refusal reason

#### Scenario: Stream stop releases state

- **WHEN** either peer sends `stream_stop` for an active stream
- **THEN** both sides release that stream's transport state (sequence tracking, reassembly buffers, playback buffers)
- **AND** subsequent media packets on that stream identifier are discarded and counted

#### Scenario: Session end implies stream stop

- **WHEN** a session ends for any reason while streams are active
- **THEN** every active stream is treated as stopped and its resources are released without requiring explicit `stream_stop` messages

### Requirement: Streams are gated by negotiated capabilities

A stream SHALL only be started when the capability token corresponding to that stream is present in the negotiated capability intersection from the handshake.

#### Scenario: Non-negotiated stream is refused

- **WHEN** a `stream_start` arrives for a stream whose capability token was not in the negotiated intersection
- **THEN** the sink refuses with `stream_ack` reason `not_negotiated`

### Requirement: Sink-initiated stream requests

The sink of a stream SHALL be able to request that the source start or stop it, via a `stream_request` message carrying the stream identifier and desired active state. The source responds by initiating the normal `stream_start` flow or by sending `stream_stop`.

#### Scenario: Desktop toggle starts the microphone

- **WHEN** the desktop user enables the microphone toggle and the desktop sends `stream_request` with active true for the microphone stream
- **THEN** the phone initiates `stream_start` for the microphone stream as if toggled locally, honoring permission checks

#### Scenario: Sink request to stop is honored

- **WHEN** the sink sends `stream_request` with active false for an active stream
- **THEN** the source stops sending and issues `stream_stop`

#### Scenario: Unknown stream identifiers in control messages are refused gracefully

- **WHEN** a peer receives a stream lifecycle message naming a stream identifier it does not recognize
- **THEN** it replies with a refusal (`stream_ack` with `accepted` false for `stream_start`, or an `error` with reason `malformed` otherwise)
- **AND** the control connection remains open
