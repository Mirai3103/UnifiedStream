## ADDED Requirements

### Requirement: Control channel transport

The system SHALL establish a TCP control channel between phone and desktop carrying newline-delimited JSON messages, separate from the UDP media path.

#### Scenario: Control channel is accepted

- **WHEN** the phone opens a TCP connection to the desktop's advertised control port
- **THEN** the desktop accepts the connection and begins reading newline-delimited JSON messages

#### Scenario: Malformed control message is rejected

- **WHEN** a peer receives a line that is not valid JSON or lacks a `type` field
- **THEN** it responds with an `error` message identifying the failure
- **AND** the connection remains open

#### Scenario: Control channel loss ends the session

- **WHEN** the TCP control connection closes for any reason while a session is active
- **THEN** both sides transition the session out of the Connected state and stop sending media packets

### Requirement: Session handshake and capability negotiation

The phone and desktop SHALL perform a handshake that exchanges protocol versions and capabilities and assigns a session identifier before any media may be sent.

#### Scenario: Successful handshake

- **WHEN** the phone sends a `hello` message containing its protocol version, device identifier, device name, and capability list
- **THEN** the desktop replies with a `hello_ack` containing its own version, identifier, name, capability list, a randomly generated 64-bit session identifier, and the UDP media port to use
- **AND** both sides record the intersection of their capability lists as the negotiated capabilities

#### Scenario: Incompatible protocol version

- **WHEN** the desktop receives a `hello` whose protocol version it does not support
- **THEN** it replies with an `error` message stating the version mismatch and its own supported version
- **AND** closes the control connection without creating a session

#### Scenario: Media before handshake is ignored

- **WHEN** a peer receives a media packet for a session identifier that has not been established
- **THEN** it discards the packet without processing

### Requirement: Session identifiers are carried as decimal strings

The session identifier SHALL be encoded as a decimal string wherever it crosses a serialization boundary, never as a JSON number.

The identifier uses the full unsigned 64-bit range. Roughly half of all values exceed the signed 64-bit maximum and overflow a signed reader, and any value above 2^53 loses precision in a reader whose numbers are IEEE doubles. Both readers exist in this system — the phone parses into a signed 64-bit integer and the desktop UI into a double.

#### Scenario: An identifier above the signed 64-bit range survives the handshake

- **WHEN** the desktop issues a session identifier greater than 2^63 and sends it in `hello_ack`
- **THEN** the phone parses the message successfully and recovers the identifier exactly
- **AND** the session is established

#### Scenario: A resumed identifier uses the same encoding

- **WHEN** the phone sends a `hello` carrying `resume_session_id`
- **THEN** the value is encoded as a decimal string

#### Scenario: The identifier reaches the desktop UI without rounding

- **WHEN** the connection state carrying a session identifier is delivered to the desktop UI
- **THEN** the identifier is encoded as a decimal string rather than a number

### Requirement: Media address exchange

The `hello` message SHALL be able to carry the UDP port the phone bound for media, so the desktop can address the phone without waiting for an inbound datagram.

#### Scenario: The desktop learns where to send media from the handshake

- **WHEN** the phone sends a `hello` containing its media port
- **THEN** the desktop combines that port with the phone's control-channel address to form the media destination
- **AND** may send media before receiving any datagram from the phone

#### Scenario: The media port is optional

- **WHEN** a `hello` arrives without a media port
- **THEN** the handshake still succeeds
- **AND** the desktop sends media only after learning the phone's address from an inbound datagram

### Requirement: Explicit pairing confirmation

The desktop SHALL require explicit user confirmation before a newly connecting phone is granted a session.

#### Scenario: User accepts a pairing request

- **WHEN** an unrecognized phone completes a `hello` and the desktop prompts the user
- **AND** the user accepts
- **THEN** the desktop sends `hello_ack`, records the phone's device identifier as trusted, and the session becomes established

#### Scenario: User rejects a pairing request

- **WHEN** the desktop prompts the user for a pairing request and the user rejects it
- **THEN** the desktop sends an `error` message with reason `rejected` and closes the control connection
- **AND** no session identifier is issued

#### Scenario: Previously trusted phone reconnects without a prompt

- **WHEN** a phone whose device identifier is already recorded as trusted sends a `hello`
- **THEN** the desktop completes the handshake without prompting the user

#### Scenario: Pairing prompt times out

- **WHEN** a pairing prompt is displayed and the user takes no action for 30 seconds
- **THEN** the desktop treats it as a rejection and closes the control connection

### Requirement: Single active session

The desktop SHALL maintain at most one active session at a time.

#### Scenario: Second phone is refused while a session is active

- **WHEN** a phone sends a `hello` while another session is already established
- **THEN** the desktop replies with an `error` message with reason `busy` and closes the new connection
- **AND** the existing session is unaffected

### Requirement: A phone may resume its own session

The desktop SHALL allow a `hello` to displace the active session when, and only when, both the resumed session identifier and the device identifier match the session currently held.

TCP may not yet have noticed that the previous connection died, so without this a phone reconnecting after a network drop is refused `busy` by its own stale socket — a condition it can never clear, which would make the reconnect path useless in exactly the case it exists for.

#### Scenario: The same phone resumes after a network drop

- **WHEN** a phone sends a `hello` whose `resume_session_id` and device identifier both match the active session
- **THEN** the desktop accepts it, replaces the stale connection, and replies with a `hello_ack` carrying the same session identifier
- **AND** the phone is not prompted for pairing again

#### Scenario: A different device may not steal a session by naming its identifier

- **WHEN** a `hello` carries a `resume_session_id` matching the active session but a different device identifier
- **THEN** the desktop refuses it with reason `busy`

#### Scenario: An unrecognised session identifier is refused

- **WHEN** a `hello` carries a `resume_session_id` that does not match the active session
- **THEN** the desktop refuses it with reason `busy`

### Requirement: Heartbeat and round-trip measurement

Both peers SHALL exchange heartbeat messages that detect a dead peer and provide round-trip time samples.

#### Scenario: Heartbeat exchange

- **WHEN** a session is established
- **THEN** the phone sends a `ping` control message containing a monotonic timestamp every 1 second
- **AND** the desktop replies with a `pong` echoing that timestamp

#### Scenario: Round-trip time is derived from heartbeats

- **WHEN** a `pong` is received
- **THEN** the sender computes round-trip time as the difference between the current time and the echoed timestamp
- **AND** records it as a latency sample

#### Scenario: Missed heartbeats declare the peer dead

- **WHEN** three consecutive `ping` messages receive no `pong` within 1 second each
- **THEN** the sender treats the peer as unreachable and transitions the session to Reconnecting

### Requirement: Connection state machine

Each application SHALL model the connection as an explicit state machine with states Idle, Discovering, Connecting, Connected, Reconnecting, and Failed, and SHALL drive all connection UI from that state.

#### Scenario: State advances through a successful connection

- **WHEN** the user selects a discovered device and confirms connection
- **THEN** the state moves from Discovering to Connecting, and to Connected once the handshake completes

#### Scenario: Handshake failure moves to Failed

- **WHEN** the control connection cannot be established or the handshake returns an error
- **THEN** the state moves to Failed and the UI shows the failure reason

#### Scenario: State is observable by the UI

- **WHEN** the connection state changes for any reason
- **THEN** the change is published to the user interface layer within 100 milliseconds

### Requirement: Automatic reconnection

The phone SHALL attempt to re-establish a dropped session automatically using exponential backoff, reusing the existing session identifier.

#### Scenario: Transient loss reconnects successfully

- **WHEN** the control connection drops while the session is Connected and the desktop is still reachable
- **THEN** the phone enters Reconnecting and retries with delays of 500ms, 1s, 2s, 4s, and 8s
- **AND** on a successful retry the session resumes with the same session identifier and the state returns to Connected

#### Scenario: Reconnection attempts are exhausted

- **WHEN** all five reconnection attempts fail
- **THEN** the state moves to Failed and the UI offers a manual retry

#### Scenario: User cancels reconnection

- **WHEN** the user cancels while the state is Reconnecting
- **THEN** retries stop immediately and the state moves to Idle

### Requirement: Session teardown

Either peer SHALL be able to end a session cleanly, releasing all associated resources.

#### Scenario: Clean disconnect

- **WHEN** the user disconnects from either app
- **THEN** that peer sends a `bye` control message, stops sending media packets, closes the UDP socket, and closes the control connection
- **AND** the other peer transitions to Idle without attempting to reconnect

### Requirement: Session survives app backgrounding on Android

The Android application SHALL keep an active session running while the app is not in the foreground.

#### Scenario: Session continues in the background

- **WHEN** a session is Connected and the user switches to another app
- **THEN** the session is maintained by a foreground service with a persistent notification
- **AND** the control channel and media transport continue operating
