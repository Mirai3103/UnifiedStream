# Media Transport Specification

## Purpose

The UDP media transport that camera, microphone, and speaker all multiplex over: packet framing, fragmentation and reassembly, sequencing and loss accounting, the bounded reorder buffer, and the synthetic stream used to verify the path without a codec.

See `openspec/specs/protocol.md` for the normative wire format these requirements are implemented against.

## Requirements

### Requirement: Media packet header format

The system SHALL prefix every media datagram with a 16-byte big-endian header containing a 2-bit protocol version, a fragment flag, a marker bit, a 4-bit reserved field, an 8-bit stream identifier, a 16-bit sequence number, a 32-bit microsecond timestamp, and a 64-bit session identifier.

#### Scenario: Header round-trips

- **WHEN** a header is serialized and then parsed
- **THEN** every field parses back to the value it was written with

#### Scenario: Unknown protocol version is dropped

- **WHEN** a packet arrives whose version field does not match the supported version
- **THEN** the receiver discards the packet without further processing

#### Scenario: Truncated packet is dropped

- **WHEN** a datagram shorter than 16 bytes arrives
- **THEN** the receiver discards it without attempting to parse a header and without panicking or throwing

#### Scenario: Foreign session identifier is dropped

- **WHEN** a packet arrives whose session identifier does not match the active session
- **THEN** the receiver discards the packet

### Requirement: Multiplexed logical streams

The transport SHALL carry multiple independent logical streams over a single UDP socket, distinguished by stream identifier, with sequencing and reassembly tracked per stream.

#### Scenario: Streams are sequenced independently

- **WHEN** packets are sent on two different stream identifiers
- **THEN** each stream carries its own sequence number series starting at zero
- **AND** loss on one stream does not affect delivery accounting on the other

#### Scenario: Unknown stream identifier is ignored

- **WHEN** a packet arrives with a stream identifier that has no registered receiver
- **THEN** it is discarded and counted, without disrupting other streams

### Requirement: Frame fragmentation and reassembly

The transport SHALL fragment payloads exceeding 1200 bytes into multiple packets and reassemble them on the receiving side.

#### Scenario: Large frame is fragmented

- **WHEN** a frame larger than 1200 bytes is submitted for sending
- **THEN** it is split into fragments of at most 1200 payload bytes each, all sharing the frame's timestamp, with the fragment flag set on every fragment and the marker bit set only on the last

#### Scenario: Fragments are reassembled in order

- **WHEN** all fragments of a frame arrive
- **THEN** the receiver reassembles them in sequence order and delivers a single frame byte-identical to the original

#### Scenario: Out-of-order fragments are reassembled

- **WHEN** the fragments of a frame arrive in a different order than sent, within the reorder window
- **THEN** the receiver still delivers the correct reassembled frame

#### Scenario: Incomplete frame is discarded

- **WHEN** one or more fragments of a frame are lost and the next frame's fragments begin arriving
- **THEN** the incomplete frame is discarded, its buffer released, and the loss counted
- **AND** no partial frame is delivered to the consumer

#### Scenario: Small payload is not fragmented

- **WHEN** a frame of 1200 bytes or fewer is sent
- **THEN** it is transmitted as a single packet with the fragment flag clear and the marker bit set

### Requirement: Frame-start synchronisation

A receiver SHALL discard fragmented packets until it has established a frame boundary, so that a frame missing its leading fragments is never delivered as if it were whole.

A receiver that joins mid-frame — because the first packet it saw was reordered, or because it attached to a stream already in progress — cannot distinguish a frame's first fragment from its third. Concatenating whatever arrives produces a frame with a missing head, which is silent corruption and strictly worse than dropping the frame.

#### Scenario: Joining mid-frame does not deliver a truncated frame

- **WHEN** a receiver's first packets on a stream are the second and third fragments of a frame, and the first fragment is never seen
- **THEN** no frame is delivered for that timestamp
- **AND** the frame is counted as incomplete

#### Scenario: An unfragmented packet establishes a boundary

- **WHEN** a packet arrives with the fragment flag clear
- **THEN** it is delivered as a complete frame on its own
- **AND** subsequent fragmented packets are eligible for reassembly

#### Scenario: A marker establishes a boundary

- **WHEN** a receiver has not yet established a boundary and a packet arrives with the marker bit set
- **THEN** that frame is discarded as incomplete
- **AND** the next packet is treated as the start of a new frame

#### Scenario: Stream start establishes a boundary

- **WHEN** the first packet released on a stream carries sequence number 0
- **THEN** it is treated as a frame start, because senders begin every stream's counter at zero

#### Scenario: The receiver recovers on the next whole frame

- **WHEN** a receiver has discarded a frame it joined mid-way
- **AND** a subsequent frame arrives complete from its first fragment
- **THEN** that frame is reassembled and delivered

### Requirement: Sequence numbering and wrap handling

The transport SHALL detect packet loss and reordering from per-stream sequence numbers and SHALL handle 16-bit sequence wrap-around correctly.

#### Scenario: Gap is counted as loss

- **WHEN** the receiver observes sequence numbers 10, 11, then 14
- **THEN** it records two lost packets for that stream

#### Scenario: Sequence wrap is not treated as loss

- **WHEN** the receiver observes sequence number 65535 followed by 0
- **THEN** it treats the packets as consecutive and records no loss

#### Scenario: Late packet is dropped, not reordered into the past

- **WHEN** a packet arrives whose sequence number is older than the last packet already delivered
- **THEN** it is discarded and counted as late

### Requirement: Timestamp handling

Each frame SHALL carry a sender-side capture timestamp in microseconds since session start, and the receiver SHALL handle 32-bit timestamp wrap-around.

#### Scenario: Timestamp is preserved across fragmentation

- **WHEN** a frame is fragmented into multiple packets
- **THEN** every fragment carries the same timestamp, and the reassembled frame is delivered with that timestamp

#### Scenario: Timestamp wrap is handled

- **WHEN** the microsecond timestamp wraps from near its 32-bit maximum back to a small value during a session longer than 71 minutes
- **THEN** the receiver detects the wrap and continues producing monotonically increasing frame times

### Requirement: Receive-side reorder buffer

The receiver SHALL apply a bounded reorder buffer that releases packets in sequence order while adding no more than a fixed, small delay.

#### Scenario: Reordered packets are corrected

- **WHEN** packets arrive in the order 10, 12, 11 within the reorder window
- **THEN** they are released to the consumer in the order 10, 11, 12

#### Scenario: Buffer does not stall on loss

- **WHEN** a packet is lost and the reorder window fills with subsequent packets
- **THEN** the buffer releases the held packets rather than waiting indefinitely for the missing one

#### Scenario: Buffer depth is bounded

- **WHEN** the transport is operating at any packet rate
- **THEN** the reorder buffer holds at most 3 packets per stream

### Requirement: Synthetic test stream

The system SHALL provide a synthetic test stream on stream identifier 0 that exercises the transport end to end without any media codec.

#### Scenario: Test stream validates the path

- **WHEN** the user starts the synthetic test stream from either app while a session is Connected
- **THEN** the sender transmits generated payloads at a configurable rate and frame size
- **AND** the receiver verifies payload integrity and reports delivered, lost, and late packet counts

#### Scenario: Test stream exercises fragmentation

- **WHEN** the synthetic test stream is configured with a frame size above 1200 bytes
- **THEN** frames are fragmented and reassembled, and integrity verification still passes

### Requirement: Socket lifecycle and port negotiation

The transport SHALL bind its UDP socket during session establishment and release it on teardown, selecting an alternate port if the default is unavailable.

#### Scenario: Default port is used when free

- **WHEN** a session is established and UDP port 47811 is available
- **THEN** the transport binds that port and advertises it in the handshake

#### Scenario: Alternate port is chosen when the default is taken

- **WHEN** UDP port 47811 is already in use
- **THEN** the transport binds an available ephemeral port and communicates it to the peer in the handshake

#### Scenario: Socket is released on teardown

- **WHEN** a session ends for any reason
- **THEN** the UDP socket is closed and its port released
