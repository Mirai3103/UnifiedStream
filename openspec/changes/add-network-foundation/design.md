## Context

UnifiedStream pairs an Android app (Kotlin + Jetpack Compose) with a desktop app (Rust + Tauri 2 + React) to stream a phone's camera and microphone to a PC and the PC's system audio back to the phone. Both repositories currently hold unmodified project templates: `android/app` is the Android Studio Compose starter, `desktop/` is the Tauri + React + Vite starter with a single `greet` command.

The product target is sub-50ms glass-to-glass latency on a local Wi-Fi network. That number rules out anything that buffers to guarantee delivery, and it means the transport, not the codec, sets the floor. Three separate media features will sit on this layer, so the packet format and session negotiation have to be decided once, up front, and versioned.

Constraints that shaped this design:
- **LAN-only.** Both peers are on the same subnet. There is no NAT to traverse, no TURN relay, no signaling server.
- **Linux first.** Development is on CachyOS; Windows support is deferred, but nothing here may be Linux-specific.
- **Android is the constrained side.** Doze mode, Wi-Fi multicast locks, background execution limits, and battery all bite on the phone, not the PC.
- **The transport must be codec-agnostic.** It moves opaque timestamped payloads; it does not know what H.264 or Opus are.

## Goals / Non-Goals

**Goals:**

- Zero-configuration discovery: the phone shows a list of PCs without the user knowing an IP address.
- A single versioned wire protocol — one control channel, one media transport — that camera, microphone, and speaker streams all multiplex over.
- Transport overhead under ~2ms end-to-end on a quiet LAN, and a measured, visible latency budget.
- Graceful degradation: Wi-Fi hiccups reconnect automatically instead of requiring the user to re-pair.
- A telemetry path good enough to diagnose "why is it laggy" without attaching a debugger.
- Testable in isolation: the Rust networking crate builds and unit-tests without Tauri; the Kotlin networking layer tests without an Android device.

**Non-Goals:**

- Any media capture, encoding, or decoding. A synthetic pattern stream proves the transport; real codecs come later.
- Virtual device integration (`v4l2loopback`, PipeWire, DirectShow, WASAPI).
- Internet/remote streaming, NAT traversal, or relay servers.
- Encryption of the media path. See "Risks" — this is a deliberate, bounded deferral.
- Multi-phone-to-one-PC or one-phone-to-multi-PC. One session, one peer pair.
- Windows support.

## Decisions

### 1. Custom UDP transport with an RTP-like header, not WebRTC

WebRTC gives congestion control, NACK-based retransmission, and a jitter buffer for free, and it's the obvious answer for internet streaming. It is the wrong answer here. `webrtc-rs` is a large dependency tree implementing ICE, DTLS-SRTP, and SDP negotiation — all of which exist to solve NAT traversal and hostile-network problems that do not exist on a home LAN. The Android side would need Google's WebRTC AAR, which is tens of megabytes and hard to pin. Worse, WebRTC's adaptive jitter buffer is tuned to protect audio continuity over lossy WANs and will happily add 100ms+ of buffering — exactly the latency we are trying to avoid, and not straightforward to override.

The chosen design: a 16-byte fixed header over plain UDP, modeled on RTP's field layout (so RTP-native tooling and intuition transfer) but without RTP's profile machinery.

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

- `Ver` (2 bits): protocol version, `1` for this change. A peer receiving an unknown version drops the packet.
- `F` (1 bit): fragment flag — set when this packet is part of a fragmented frame.
- `M` (1 bit): marker — set on the final fragment of a frame (matching RTP's "end of frame" convention).
- `Stream ID` (8 bits): which logical stream (`0` = synthetic test, `1` = camera, `2` = microphone, `3` = speaker; the rest reserved). This is why one socket serves all three features.
- `Sequence Number` (16 bits): per-stream, wraps at 65536, used for loss and reorder detection.
- `Timestamp` (32 bits): microseconds since session start, sender's clock. Wraps at ~71 minutes, which the receiver handles by tracking wrap count.
- `Session ID` (64 bits): random, assigned during the handshake. Packets whose session ID doesn't match the active session are dropped — this is what stops a stale sender from a previous session (or another app on the LAN) injecting frames.

Big-endian throughout, matching network byte order and RTP.

Fragmentation is our own, at a 1200-byte payload MTU (conservative enough for Wi-Fi with room for IPv6 and any tunneling). Fragments of one frame share a sequence number range and timestamp; the last carries `M`. Relying on IP-level fragmentation instead would be a mistake: a single lost IP fragment silently destroys the whole datagram, and many consumer APs handle fragmented UDP poorly.

**Trade-off accepted:** we hand-roll loss detection and the jitter buffer, and we get no congestion control in this change. On a dedicated LAN that is acceptable; if it proves otherwise, a sender-side pacing/bitrate-feedback loop can be added over the existing telemetry channel without touching the header.

### 2. Split control plane (TCP) and data plane (UDP)

Control messages — pairing, capability negotiation, stream start/stop, telemetry reports — are infrequent, small, and must not be lost. Media is the opposite: high rate, and a lost packet is better dropped than retransmitted late. Running both over UDP would mean reimplementing reliable delivery for control; running both over TCP would inflict head-of-line blocking on media.

So: TCP on port **47810** for control, UDP on **47811** for media (defaults; both are negotiated and fall back if in use). Control messages are newline-delimited JSON — `{"type": "...", ...}` — chosen over a binary format because control traffic is negligible in volume and JSON keeps the protocol debuggable with `nc` and readable in logs. The media path, where bytes matter, stays binary.

The TCP connection doubles as a liveness signal: if it drops, the session is over, no timeout guessing required.

### 3. Desktop advertises, phone browses

The mDNS service type is `_unifiedstream._udp.local.`, advertised by the **desktop**, browsed by the **phone**. This direction is deliberate: PCs are usually stationary and long-running, phones move between networks and sleep aggressively. Android's `NsdManager` is a much better browser than advertiser, and continuous advertisement from a phone drains battery.

TXT records carry `ver` (protocol version), `name` (human-readable hostname), `caps` (comma-separated capability list, e.g. `cam,mic,spk`), and `id` (stable device UUID). Advertising capabilities in the TXT record lets the phone gray out unsupported features before connecting.

Android requires `MulticastLock` held while browsing, or mDNS responses are filtered by the Wi-Fi driver — a classic silent failure. On the Rust side, `mdns-sd` is chosen over `zeroconf`/`astro-dnssd` because it is a pure-Rust responder with no dependency on a system Avahi daemon, which keeps the eventual Windows port viable.

**Fallback:** manual IP entry, because AP client isolation and multicast filtering are common enough on guest and mesh networks that discovery-only would strand real users.

### 4. `unifiedstream-net` as a separate crate, `:core` as a separate module

`desktop/src-tauri` becomes a Cargo workspace: the Tauri binary plus a `unifiedstream-net` library crate holding discovery, control, and transport with no Tauri dependency. This is not ceremony — it means the protocol can be tested with `cargo test` in milliseconds, and later reused by a headless CLI or a Windows service that has no webview.

Tauri communicates with it in both directions: `#[tauri::command]` for UI-initiated actions (start advertising, accept pairing, disconnect), and the event system (`app.emit`) for pushes to the UI (device found, state changed, telemetry tick).

Android mirrors this with packages under `com.laffy.unifiedstream`: `discovery/`, `control/`, `transport/`, `telemetry/`, `ui/`. The session lives in a **foreground service** with a persistent notification, not in the Activity — otherwise Android kills streaming the moment the user switches apps, which is precisely when they're using the phone as a webcam.

### 5. Explicit connection state machine

```
Idle ──────► Discovering ──────► Connecting ──────► Connected
 ▲                                     │                 │
 │                                     │ (fail)          │ (loss)
 │                                     ▼                 ▼
 └───────────────────────────────── Failed ◄───── Reconnecting
                                        ▲                 │
                                        └─────────────────┘
                                          (retries exhausted)
```

Reconnect uses exponential backoff — 500ms, 1s, 2s, 4s, 8s — capped at five attempts, and reuses the existing session ID so the peer can resume rather than re-pair. State transitions are the single source of truth for what the UI shows; both apps render from this enum rather than inferring status from socket state.

### 6. Fixed 3-packet jitter buffer, not adaptive

The receiver holds a small reorder window of 3 packets and releases in sequence order, dropping anything that arrives after its slot has passed. An adaptive buffer that grows under loss is the standard choice and the wrong one here: it trades exactly the latency this project exists to minimize. On a LAN, reordering is rare and 3 packets (~5ms at typical rates) covers it. If measurement later shows otherwise, the window is a single constant.

### 7. Telemetry as a 1 Hz control-channel report

Each side computes locally — RTT from heartbeat round-trips (EWMA, α=0.2, over the last 10 samples), throughput from a byte counter, loss from sequence gaps, jitter per RFC 3550's interarrival formula — and sends a `telemetry` control message every second. Sampling at 1 Hz rather than per-packet keeps overhead negligible and matches what a human can actually read on a dashboard.

## Risks / Trade-offs

- **No encryption on the media path** → Anyone on the LAN can sniff camera and microphone frames. This is a real privacy exposure and is deferred, not dismissed: the 64-bit random session ID prevents casual injection, but not observation. The mitigation for this change is explicit pairing confirmation on the desktop (nothing streams to an unapproved peer) and the protocol version field, which lets a later change introduce DTLS-SRTP or a pre-shared-key AEAD without breaking the header layout. This should be closed before any public release.

- **mDNS blocked by AP client isolation or multicast filtering** → Manual IP entry is specified as a first-class path, not a debug affordance, and the UI surfaces it after a discovery timeout rather than hiding it in settings.

- **Android background execution limits kill the session** → Foreground service with a persistent notification and a partial wake lock while streaming. Cost: users see a permanent notification, which is the accepted price.

- **No congestion control** → A saturated Wi-Fi link will drop packets with no sender-side backoff, degrading quality unpredictably. Accepted for a LAN MVP; the telemetry channel already carries the loss signal a future bitrate-adaptation loop would need.

- **Hand-rolled protocol means hand-rolled bugs** → Fragmentation, sequence wrap, and reassembly are exactly where custom transports break. Mitigated by keeping the codec-independent core in a pure library crate with unit tests for wrap-around, out-of-order arrival, fragment loss, and truncated/malformed headers — and by fuzzing the header parser, since it parses untrusted network input.

- **32-bit microsecond timestamp wraps every ~71.6 minutes** → Streaming sessions can easily exceed that. The receiver tracks wrap count explicitly; this is called out because it is the kind of thing that works fine in every test and fails an hour into real use.

- **Two independent implementations of one protocol** → Kotlin and Rust can drift. Mitigated by a shared protocol document under `openspec/specs/` treated as normative, and by cross-implementation tests where the Rust test harness talks to a recorded Kotlin packet capture and vice versa.

## Migration Plan

No migration — this is greenfield. Both apps currently contain only template code, which is replaced outright. There are no users, no persisted data, and no prior protocol version to be compatible with. The `Ver` field is set to `1` so future protocol changes have a hinge to swing on.

## Open Questions

- Should the desktop also browse (bidirectional discovery), so the PC can find a phone that is already advertising? Deferred; the phone-browses direction covers every MVP flow.
- Is a 3-packet reorder window right for 1080p60 H.264, where one frame may span many fragments? Revisit with real measurements once the camera stream exists.
- Does `mdns-sd` behave correctly on Windows without Bonjour installed? Must be verified before the Windows port, not before this change ships.
