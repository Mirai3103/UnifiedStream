## Why

UnifiedStream turns an Android phone into a webcam, microphone, and wireless speaker for a Linux/Windows PC, but today both sides are empty scaffolds (a stock Jetpack Compose template and a stock Tauri template). Every planned media feature — virtual camera, virtual microphone, wireless speaker — needs the same substrate first: find the peer on the LAN without typing an IP, negotiate a session, move timestamped media frames over UDP with sub-50ms latency, and show the user whether the link is healthy. Building that substrate once, before any codec work, avoids three incompatible ad-hoc transports.

## What Changes

- Add mDNS/DNS-SD service advertisement on the desktop and service browsing on Android, so the phone lists reachable PCs on the same Wi-Fi network with zero manual configuration.
- Add a pairing + session handshake over a TCP control channel: capability exchange (which media streams each side supports), session ID assignment, UDP port negotiation, and explicit accept/reject on the desktop.
- Add a shared UDP media transport: an RTP-like packet header (session ID, stream ID, sequence number, timestamp, marker bit), fragmentation/reassembly for payloads larger than the path MTU, and per-stream jitter/reorder handling. No media codecs are wired in yet — the transport is exercised with a synthetic test stream.
- Add a keepalive/heartbeat loop with RTT probing, and connection lifecycle states (Discovering → Connecting → Connected → Reconnecting → Disconnected) with automatic reconnect on transient loss.
- Add a real-time telemetry channel and status dashboard on both apps: round-trip ping (ms), throughput (Mbps), packet loss (%), and jitter, sampled at 1 Hz.
- Replace both template UIs with the real app shells: a dark-mode Material 3 device-list + status screen on Android, and a dark/glassmorphism device panel + status dashboard in Tauri. Per-feature Cam/Mic/Speaker toggles are rendered but disabled with a "coming soon" state, since no media capability exists yet.
- Establish the Rust workspace layout under `desktop/src-tauri` and the Kotlin module/package layout under `android/app`, plus the dependency additions both need.

Non-goals for this change: camera capture, audio capture, Opus/H.264 encoding, `v4l2loopback`, PipeWire/PulseAudio sinks, and any Windows-specific driver work. Those land as follow-up changes on top of this transport.

## Capabilities

### New Capabilities

- `device-discovery`: mDNS/DNS-SD advertisement and browsing over the local network, service record contents, device identity and persistence, and manual-IP fallback when multicast is blocked.
- `session-control`: the TCP control channel — pairing, capability negotiation, session establishment and teardown, heartbeat, connection state machine, and reconnect behavior.
- `media-transport`: the UDP datagram transport — packet header format, fragmentation and reassembly, sequencing, timestamping, loss detection, and the receive-side jitter buffer.
- `connection-telemetry`: measurement and reporting of ping, throughput, packet loss, and jitter, and the status dashboard surfaces that display them on both platforms.

### Modified Capabilities

None — `openspec/specs/` is empty; this is the first change in the project.

## Impact

- **Android** (`android/app`): new packages for discovery, control, transport, and UI; `MainActivity.kt` and the Compose theme are rewritten. New dependencies: `androidx.lifecycle` (ViewModel/Compose), `kotlinx-coroutines`, `kotlinx-serialization`, `androidx.datastore` for device identity. Uses the platform `NsdManager` for mDNS and `java.nio` channels for sockets. New manifest permissions: `INTERNET`, `ACCESS_NETWORK_STATE`, `ACCESS_WIFI_STATE`, `CHANGE_WIFI_MULTICAST_STATE`, and `FOREGROUND_SERVICE` (the session must survive the app being backgrounded).
- **Desktop** (`desktop/`): `src-tauri` becomes a Cargo workspace with the Tauri binary plus a `unifiedstream-net` library crate holding discovery/control/transport so it stays unit-testable without Tauri. New Rust dependencies: `tokio`, `mdns-sd`, `serde`, `bytes`, `thiserror`, `tracing`. The React front end gains a device panel, connection state, and telemetry dashboard; telemetry flows to the UI via Tauri events.
- **Wire protocol**: introduces the versioned control and media packet formats that all later media changes must build on. Getting the header and negotiation right here is what keeps camera/mic/speaker from each inventing their own.
- **Platform scope**: Linux (CachyOS) is the development and MVP target. Nothing in this change is Linux-specific, but only Linux is verified.
- **Risk**: mDNS is unreliable on networks with AP client isolation or multicast filtering — mitigated by the manual-IP fallback listed under `device-discovery`.
