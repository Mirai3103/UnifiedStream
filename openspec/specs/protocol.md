# UnifiedStream Wire Protocol v1

Normative reference for the UnifiedStream protocol. The Rust (`unifiedstream-net`) and Kotlin
(`com.laffy.unifiedstream.protocol`) implementations MUST both conform to this document. Where an
implementation and this document disagree, this document is correct.

## 1. Transport overview

| Plane   | Transport | Default port | Payload format               |
| ------- | --------- | ------------ | ---------------------------- |
| Control | TCP       | 47810        | Newline-delimited JSON (UTF-8) |
| Media   | UDP       | 47811        | Binary: 16-byte header + payload |

The desktop listens on the control port and advertises it via mDNS. The phone connects. Both ports
are defaults: the control port is published in the mDNS SRV record, and the media port is negotiated
during the handshake, so either side may bind an ephemeral port when the default is occupied.

All multi-byte integers in the media protocol are **big-endian** (network byte order).

Protocol version for this document is **1**.

## 2. Media packet format

Every UDP media datagram begins with a fixed 16-byte header, followed by the payload. Total datagram
size MUST NOT exceed 16 + 1200 = 1216 bytes.

```
 0               1               2               3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|Ver|F|M| Res |   Stream ID   |        Sequence Number          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    Timestamp (microseconds)                   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                     Session ID (64 bits)                      +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 2.1 Field definitions

Byte 0 is a bitfield, most-significant bit first:

| Bits | Name  | Width | Description                                                            |
| ---- | ----- | ----- | ---------------------------------------------------------------------- |
| 7-6  | `Ver` | 2     | Protocol version. MUST be `1` for this document.                        |
| 5    | `F`   | 1     | Fragment flag. Set when this packet is one fragment of a larger frame.  |
| 4    | `M`   | 1     | Marker. Set on the final packet of a frame.                             |
| 3-0  | `Res` | 4     | Reserved. MUST be written as `0`, MUST be ignored on receipt.           |

Reserved bits are ignored rather than validated so that a future version can claim them without
older receivers rejecting the traffic outright.

| Offset | Size | Name              | Type   | Description                                                     |
| ------ | ---- | ----------------- | ------ | --------------------------------------------------------------- |
| 0      | 1    | flags             | u8     | Bitfield above.                                                  |
| 1      | 1    | `Stream ID`       | u8     | Logical stream. See §2.2.                                        |
| 2      | 2    | `Sequence Number` | u16 BE | Per-stream counter starting at 0, wraps at 65536.                |
| 4      | 4    | `Timestamp`       | u32 BE | Microseconds since session start, sender's clock. Wraps at 2^32. |
| 8      | 8    | `Session ID`      | u64 BE | Random session identifier assigned in `hello_ack`.               |

### 2.2 Stream identifiers

| ID     | Stream            | Direction     | Status                            |
| ------ | ----------------- | ------------- | --------------------------------- |
| 0      | Synthetic test    | Either        | Implemented                       |
| 1      | Camera            | Phone → PC    | Implemented — see §7              |
| 2      | Microphone        | Phone → PC    | Implemented — see §5              |
| 3      | Speaker           | PC → Phone    | Implemented — see §6              |
| 4-255  | —                 | —             | Reserved                          |

Sequence numbers, reassembly buffers, and loss accounting are tracked **per stream**. Loss on one
stream MUST NOT affect delivery accounting on another.

### 2.3 Receiver validation

A receiver MUST discard a datagram, without error to the peer, when any of the following hold:

1. The datagram is shorter than 16 bytes.
2. The `Ver` field is not the supported version.
3. The `Session ID` does not match the currently active session.

A datagram whose `Stream ID` has no registered receiver MUST be discarded and counted, without
disturbing other streams.

### 2.4 Fragmentation

Payloads of 1200 bytes or fewer are sent as a single packet with `F` clear and `M` set.

Payloads larger than 1200 bytes are split into fragments of at most 1200 payload bytes each:

- Every fragment of a frame carries the **same timestamp**.
- Every fragment has `F` set.
- Fragments occupy **consecutive sequence numbers**.
- Only the final fragment has `M` set.

The receiver reassembles fragments in sequence order. If fragments of a newer frame (a higher
timestamp) begin arriving while an earlier frame is still incomplete, the incomplete frame MUST be
discarded, its buffer released, and the loss counted. Partial frames MUST NOT be delivered.

**Frame-start synchronisation.** A receiver that joins mid-frame — because the first packet it saw
was reordered, or because it attached to a stream already in progress — cannot distinguish a frame's
first fragment from its third. Concatenating whatever arrives would deliver a frame with a missing
head, which is silent corruption and strictly worse than dropping it. A receiver therefore MUST
discard fragmented packets until it has established a frame boundary. A boundary is established by
any of:

1. A packet with `F` clear, which is a complete frame by itself.
2. A packet with `M` set, after which the next packet begins a new frame.
3. Sequence number 0 as the first packet released on a stream, since senders start every stream's
   counter at zero.

### 2.5 Sequence and timestamp wrap

Sequence numbers are 16-bit and wrap from 65535 to 0. A receiver MUST treat the wrap as contiguous,
not as a 65535-packet loss. The comparison uses RFC 1982 serial-number arithmetic: sequence `a` is
newer than `b` when `((a - b) mod 65536)` lies in `1..=32767`.

Timestamps are 32-bit microseconds and wrap approximately every 71.6 minutes. A receiver MUST track
the wrap count and expose a monotonically increasing 64-bit frame time. A backwards jump of more than
half the 32-bit range is a wrap; a smaller backwards jump is an out-of-order packet.

### 2.6 Reorder buffer

A receiver holds at most **3 packets per stream** in a reorder buffer, releasing in sequence order.
The buffer MUST NOT stall waiting for a missing packet: when it is full, the oldest held packet is
released regardless of the gap. A packet arriving after its slot has passed is counted as **late**,
not lost, and is discarded rather than delivered out of order.

## 3. Control messages

Each control message is one UTF-8 JSON object on a single line, terminated by `\n`. Every message has
a `type` field. A peer receiving a line that is not valid JSON, or that lacks a `type`, responds with
an `error` and keeps the connection open.

Closing the TCP control connection ends the session: both sides leave `Connected` and stop sending
media.

### 3.1 `hello` — phone → desktop

Opens the handshake.

```json
{
  "type": "hello",
  "version": 1,
  "device_id": "8f14e45f-ceea-467a-9a3f-1b2c3d4e5f60",
  "device_name": "Pixel 8",
  "caps": ["cam", "mic", "spk"]
}
```

| Field         | Type      | Description                                     |
| ------------- | --------- | ----------------------------------------------- |
| `version`     | integer   | Protocol version the phone speaks.               |
| `device_id`   | string    | Stable UUID, persisted across restarts.          |
| `device_name` | string    | Human-readable name.                             |
| `caps`        | string[]  | Capability tokens. See §3.8.                     |
| `media_port`  | u16?      | UDP port the phone bound for media. Optional.    |

The desktop pairs `media_port` with the phone's TCP source address to know where to send media,
rather than waiting to learn the address from an inbound datagram. When it is absent, the desktop
can only send after the phone has sent first.

### 3.2 `hello_ack` — desktop → phone

Completes the handshake and establishes the session.

```json
{
  "type": "hello_ack",
  "version": 1,
  "device_id": "3c6e0b8a-9c15-4f8d-b0a1-2d3e4f506172",
  "device_name": "cachy-desktop",
  "caps": ["cam", "mic", "spk"],
  "session_id": "10873402398471029384",
  "media_port": 47811
}
```

| Field        | Type    | Description                                                     |
| ------------ | ------- | --------------------------------------------------------------- |
| `session_id` | string  | Random 64-bit session identifier, as a decimal string. Used in every media header. |
| `media_port` | u16     | UDP port the desktop bound for media.                            |

**`session_id` is a string, not a JSON number.** The value uses the full unsigned 64-bit range:
roughly half of all ids exceed 2^63 and overflow a signed 64-bit reader, and anything above 2^53
loses precision in a JavaScript reader. Both apply here — the phone parses into a Kotlin `Long`
and the desktop UI into an IEEE double — so the id travels as text and is converted at the edges.
`resume_session_id` uses the same encoding.

Both sides record the **intersection** of the two `caps` lists as the negotiated capabilities.

### 3.3 `error` — either direction

```json
{ "type": "error", "reason": "version_mismatch", "message": "server speaks version 1", "supported_version": 1 }
```

| `reason`            | Meaning                                            | Connection |
| ------------------- | -------------------------------------------------- | ---------- |
| `version_mismatch`  | Unsupported protocol version in `hello`.            | Closed     |
| `rejected`          | User rejected pairing, or the prompt timed out.     | Closed     |
| `busy`              | A session is already active.                        | Closed     |
| `malformed`         | Unparseable or `type`-less line.                    | Kept open  |
| `internal`          | Unexpected local failure.                           | Closed     |

`message` is a human-readable string. `supported_version` is present only for `version_mismatch`.

### 3.4 `ping` / `pong` — heartbeat

The phone sends `ping` every 1 second while the session is active; the desktop echoes it as `pong`
with the identical `timestamp`.

```json
{ "type": "ping", "timestamp": 1721990400123456 }
{ "type": "pong", "timestamp": 1721990400123456 }
```

`timestamp` is the sender's monotonic clock in microseconds. It is opaque to the responder — echoed
verbatim. The sender computes RTT as `now - echoed`.

Three consecutive pings unanswered within 1 second each mean the peer is unreachable, and the sender
transitions to `Reconnecting`.

### 3.5 `telemetry` — either direction, 1 Hz

```json
{
  "type": "telemetry",
  "rtt_ms": 4.2,
  "tx_mbps": 1.83,
  "rx_mbps": 0.0,
  "loss_pct": 0.4,
  "jitter_ms": 0.9
}
```

All fields are floating point. `rtt_ms` MAY be `null` before the first heartbeat sample is available.
Reports stop when the session ends.

### 3.6 `bye` — either direction

```json
{ "type": "bye" }
```

The sender then stops media, closes its UDP socket, and closes the control connection. The receiver
transitions to `Idle` and MUST NOT attempt to reconnect.

### 3.7 Reconnection

The phone retries a dropped session with backoff `500ms, 1s, 2s, 4s, 8s` — five attempts. Retries
reuse the **existing session ID**: the `hello` carries an optional `resume_session_id` field, and a
desktop that still holds that session replies with a `hello_ack` bearing the same `session_id`.

```json
{ "type": "hello", "version": 1, "device_id": "...", "device_name": "...", "caps": ["..."], "resume_session_id": "10873402398471029384" }
```

If the desktop no longer holds the session, it issues a fresh `session_id` as normal.

A resume MUST be allowed to displace an existing session when — and only when — both the
`resume_session_id` and the `device_id` match the session currently held. TCP may not yet have
noticed that the previous connection died, so without this rule a phone reconnecting after a Wi-Fi
blip is refused `busy` by its own stale socket, and can never clear the condition. A `hello` from a
different `device_id` is refused `busy` regardless of what `resume_session_id` it names, so a session
cannot be stolen by guessing an identifier.

### 3.8 Capability tokens

| Token | Meaning                              |
| ----- | ------------------------------------ |
| `cam` | Camera stream (phone → PC)           |
| `mic` | Microphone stream (phone → PC)       |
| `spk` | Speaker stream (PC → phone)          |

Unknown tokens MUST be ignored, not treated as an error — this is the forward-compatibility hinge for
later capabilities.

### 3.9 Stream lifecycle

Media streams are opened and closed over the control channel with four generic messages. The
**source** of a stream is the peer that sends its media packets (the phone for camera and
microphone, the desktop for speaker); the **sink** is the peer that receives them.

A stream MUST NOT be started unless its capability token (§3.8) is in the negotiated intersection
from the handshake. Media packets for a stream MUST NOT be sent before the source has received an
accepting `stream_ack`. When a session ends for any reason, every active stream is implicitly
stopped and its transport state (sequence tracking, reassembly buffers, playback buffers) released —
no explicit `stream_stop` messages are required.

#### 3.9.1 `stream_start` — source → sink

Announces that the source wants to send a stream, and with what format.

```json
{
  "type": "stream_start",
  "stream": 2,
  "params": { "codec": "pcm_s16le", "sample_rate": 48000, "channels": 1, "frame_ms": 20 }
}
```

| Field    | Type    | Description                                    |
| -------- | ------- | ---------------------------------------------- |
| `stream` | u8      | Stream identifier from §2.2.                    |
| `params` | object  | Format parameters. Audio fields below; video fields in §7.1. |

Audio `params` fields:

| Field         | Type   | Description                                        |
| ------------- | ------ | -------------------------------------------------- |
| `codec`       | string | `"pcm_s16le"` or `"opus"`. PCM S16LE is the mandatory baseline every peer supports. |
| `sample_rate` | u32    | Samples per second. `48000` for the microphone and speaker. |
| `channels`    | u8     | Channel count. `1` for the microphone, `2` for the speaker. |
| `frame_ms`    | u32    | Frame duration in milliseconds. `20` for the microphone and speaker. |

Unknown `params` fields MUST be ignored. A `stream_start` for a stream that is already active
replaces its parameters: the sink re-acks and resets that stream's receive state.

#### 3.9.2 `stream_ack` — sink → source

Accepts or refuses a `stream_start`.

```json
{ "type": "stream_ack", "stream": 2, "accepted": true }
{ "type": "stream_ack", "stream": 2, "accepted": false, "reason": "unsupported_codec" }
```

| Field      | Type    | Description                                       |
| ---------- | ------- | ------------------------------------------------- |
| `stream`   | u8      | Stream identifier being answered.                  |
| `accepted` | bool    | Whether media may flow.                            |
| `reason`   | string? | Present when `accepted` is false. Values below.    |

| `reason`             | Meaning                                                        |
| -------------------- | -------------------------------------------------------------- |
| `not_negotiated`     | The stream's capability token is not in the negotiated set.     |
| `unsupported_codec`  | The sink cannot decode the offered codec.                       |
| `unsupported_stream` | The sink does not recognise the stream identifier.              |
| `busy`               | The sink cannot take another stream right now.                  |
| `internal`           | The sink failed locally (e.g. its audio system is unavailable). |

A refusal closes nothing: the control connection and session continue.

#### 3.9.3 `stream_stop` — either direction

```json
{ "type": "stream_stop", "stream": 2 }
```

Ends the stream. Both sides release the stream's transport state. Media packets that arrive for a
stopped stream are discarded and counted (§2.3). A `stream_stop` for a stream that is not active is
ignored.

#### 3.9.4 `stream_request` — sink → source

Asks the source to start or stop a stream, so the receiving side's UI can drive the toggle (the
desktop's microphone switch, the phone's speaker switch).

```json
{ "type": "stream_request", "stream": 2, "active": true }
```

| Field    | Type | Description                                  |
| -------- | ---- | -------------------------------------------- |
| `stream` | u8   | Stream identifier.                            |
| `active` | bool | `true` to request a start, `false` a stop.    |

The source responds by running the normal `stream_start` flow (honouring local preconditions such
as permission checks — a request is not a command) or by sending `stream_stop`. A `stream_request`
naming a stream the source cannot provide is answered with `stream_ack` `accepted: false` and the
appropriate reason.

Any lifecycle message naming a stream identifier the receiver does not recognise is answered with
`stream_ack` `accepted: false`, reason `unsupported_stream`, and the connection stays open.

## 4. Discovery

### 4.1 Service record

The **desktop advertises**; the **phone browses**.

| Property     | Value                        |
| ------------ | ---------------------------- |
| Service type | `_unifiedstream._udp.local.` |
| Instance name| Machine hostname (default)   |
| SRV port     | The TCP **control** port (default 47810) |

The service type uses `_udp` to describe the media transport the service exists to set up, while the
SRV record's port addresses the TCP control channel that clients actually connect to first.

### 4.2 TXT record keys

| Key    | Type   | Required | Description                                            |
| ------ | ------ | -------- | ------------------------------------------------------ |
| `ver`  | string | Yes      | Protocol version, decimal. `"1"` for this document.     |
| `name` | string | Yes      | Human-readable device name.                             |
| `id`   | string | Yes      | Stable device UUID. Used to deduplicate multi-interface advertisements. |
| `caps` | string | Yes      | Comma-separated capability tokens, e.g. `"cam,mic,spk"`. |

Example: `ver=1`, `name=cachy-desktop`, `id=3c6e0b8a-9c15-4f8d-b0a1-2d3e4f506172`, `caps=cam,mic,spk`.

A browser MUST deduplicate discovered services by TXT `id`, so one desktop reachable on several
interfaces yields exactly one entry. A record missing any required key is ignored.

On shutdown the advertiser sends an mDNS goodbye so browsers drop the entry within 5 seconds.

### 4.3 Fallback

Where multicast is unavailable — AP client isolation, multicast filtering — the phone offers manual
entry of an IP address and control port. A manually entered peer is treated identically to a
discovered one from the handshake onward.

## 5. Microphone stream (stream ID 2)

Phone → PC audio, carried on stream ID 2 after a `stream_start`/`stream_ack` exchange (§3.9).

Each transport frame carries exactly **one audio frame** of the negotiated duration (20 ms for the
microphone). The media header timestamp (§2.1) is the capture time of the frame's **first sample**,
in microseconds since session start, on the sender's clock. Packet loss therefore costs exactly one
audio frame, and the incomplete-frame rules of §2.4 apply unchanged.

The sender MAY simply stop transmitting frames without ending the stream — that is how mute is
implemented. A receiver MUST tolerate an arbitrary gap in frames and resume playback when frames
reappear; it fills the gap with silence.

### 5.1 `pcm_s16le` payload

Raw audio samples, **signed 16-bit little-endian**, interleaved if more than one channel. No
additional payload header. At 48 kHz mono, a 20 ms frame is 960 samples = 1920 bytes, which
fragments into two packets per §2.4.

PCM samples are little-endian — unlike the header fields — because every current CPU on both ends
is little-endian and the payload is copied, not parsed field-by-field.

### 5.2 `opus` payload

One self-delimited [Opus](https://datatracker.ietf.org/doc/html/rfc6716) packet per frame, encoding
20 ms at 48 kHz. Opus is offered in `stream_start` only when the source actually has a working
encoder; PCM S16LE remains the baseline every peer MUST accept.

## 6. Speaker stream (stream ID 3)

PC → phone audio, carried on stream ID 3 after a `stream_start`/`stream_ack` exchange (§3.9). The
**desktop** is the source; the phone, as sink, drives its toggle with `stream_request` (§3.9.4).

The framing rules are those of the microphone stream (§5), unchanged: each transport frame carries
exactly **one audio frame** of the negotiated duration (20 ms for the speaker), the media header
timestamp is the capture time of the frame's **first sample** in microseconds since session start on
the sender's clock, packet loss costs exactly one audio frame, and the sender MAY stop transmitting
without ending the stream — that is how desktop-side mute is implemented. A receiver MUST tolerate
an arbitrary gap in frames, filling it with silence, and resume playback when frames reappear.

The default speaker format is PCM S16LE, 48 kHz, **stereo** (`channels: 2`), 20 ms frames. The sink
MUST accept PCM S16LE at both one and two channels; the source SHOULD offer stereo, since system
audio is stereo.

### 6.1 `pcm_s16le` payload

Raw audio samples, **signed 16-bit little-endian**, interleaved left/right when stereo (§5.1's
encoding with two channels). At 48 kHz stereo, a 20 ms frame is 960 samples per channel =
3840 bytes, which fragments into four packets per §2.4.

### 6.2 `opus` payload

One self-delimited Opus packet per frame, encoding 20 ms of stereo at 48 kHz. As for the
microphone, Opus is offered only when the source has a working encoder and PCM S16LE remains the
baseline every peer MUST accept.

## 7. Camera stream (stream ID 1)

Phone → PC video, carried on stream ID 1 after a `stream_start`/`stream_ack` exchange (§3.9). The
**phone** is the source; the desktop, as sink, drives its toggle with `stream_request` (§3.9.4).

Each transport frame carries exactly **one video frame**. The media header timestamp (§2.1) is the
frame's capture time, in microseconds since session start, on the sender's clock. Video frames are
larger than one datagram and fragment per §2.4; a 720p MJPEG frame typically spans 30–90 packets.
The incomplete-frame rules of §2.4 apply unchanged: **any lost or late fragment costs exactly that
one video frame** — the frame is discarded, counted, and decoding continues with the next complete
frame. Nothing is retransmitted and no back-channel exists to request recovery, which is why the
baseline codec must produce independently decodable frames.

The source MAY stop transmitting frames without ending the stream, and MAY skip frames freely to
respect `max_fps` or shed load. A receiver MUST tolerate an arbitrary gap in frames — its virtual
camera simply holds the last delivered frame — and resume normally when frames reappear.

### 7.1 Video `params` fields

```json
{
  "type": "stream_start",
  "stream": 1,
  "params": { "codec": "mjpeg", "width": 1280, "height": 720, "max_fps": 30 }
}
```

| Field     | Type   | Description                                                        |
| --------- | ------ | ------------------------------------------------------------------ |
| `codec`   | string | `"mjpeg"`. MJPEG is the mandatory baseline every peer supports; inter-frame codecs may be added as negotiated options in a later version. |
| `width`   | u32    | Frame width in pixels. `1280` is the default offering.              |
| `height`  | u32    | Frame height in pixels. `720` is the default offering.              |
| `max_fps` | u32    | Upper bound on the source's frame rate. The source MAY deliver fewer frames; it MUST NOT exceed this rate. `30` is the default. |

As for audio, unknown `params` fields MUST be ignored, and a `stream_start` for an already-active
stream replaces its parameters — the sink re-acks and resets the stream's receive state. That
replacement flow is how a mid-stream resolution change is performed. A sink MAY refuse dimensions
it cannot handle (e.g. frames too large for its reassembly budget) with reason `internal`.

### 7.2 `mjpeg` payload

One complete [JFIF/JPEG](https://www.w3.org/Graphics/JPEG/) image per transport frame, baseline
DCT, encoding the full frame at the negotiated dimensions. Every frame is independently decodable:
no state is shared between frames, so frame loss never corrupts subsequent frames. A payload that
fails JPEG decoding MUST be dropped and counted by the sink without ending the stream.
