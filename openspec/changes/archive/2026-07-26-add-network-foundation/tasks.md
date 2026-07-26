## 1. Protocol definition

- [x] 1.1 Write `openspec/specs/protocol.md` as the normative wire-format reference: 16-byte media header field layout, big-endian ordering, stream ID assignments (0=test, 1=cam, 2=mic, 3=speaker), and default ports (TCP 47810 control, UDP 47811 media)
- [x] 1.2 Document every control message type in the same file with its JSON shape: `hello`, `hello_ack`, `error`, `ping`, `pong`, `telemetry`, `bye`
- [x] 1.3 Define the mDNS service record contract: service type `_unifiedstream._udp.local.` and TXT keys `ver`, `name`, `id`, `caps`

## 2. Desktop scaffolding

- [x] 2.1 Convert `desktop/src-tauri` into a Cargo workspace with members `desktop` (the Tauri binary) and a new `crates/unifiedstream-net` library crate
- [x] 2.2 Add Rust dependencies to `unifiedstream-net`: `tokio` (rt-multi-thread, net, sync, time, macros), `mdns-sd`, `serde`/`serde_json`, `bytes`, `thiserror`, `tracing`; add `tracing-subscriber` and the net crate to the Tauri binary
- [x] 2.3 Create the module skeleton in `unifiedstream-net`: `protocol`, `discovery`, `control`, `transport`, `telemetry`, `session`, with a public `lib.rs` surface and a `NetError` type
- [x] 2.4 Initialize `tracing-subscriber` in the Tauri binary and confirm `cargo build --workspace` and `cargo test --workspace` both succeed

## 3. Android scaffolding

- [x] 3.1 Add dependencies to `android/gradle/libs.versions.toml` and `app/build.gradle.kts`: `kotlinx-coroutines-android`, `kotlinx-serialization-json` (plus the serialization plugin), `androidx.lifecycle` viewmodel-compose + runtime-compose, `androidx.datastore-preferences`
- [x] 3.2 Add manifest permissions `INTERNET`, `ACCESS_NETWORK_STATE`, `ACCESS_WIFI_STATE`, `CHANGE_WIFI_MULTICAST_STATE`, `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_CONNECTED_DEVICE`, and `POST_NOTIFICATIONS`
- [x] 3.3 Create the package skeleton under `com.laffy.unifiedstream`: `protocol/`, `discovery/`, `control/`, `transport/`, `telemetry/`, `session/`, `ui/`
- [x] 3.4 Confirm `./gradlew assembleDebug` and `./gradlew testDebugUnitTest` both succeed against the new skeleton

## 4. Packet header (both platforms)

- [x] 4.1 Implement `MediaHeader` encode/decode in Rust: version, fragment flag, marker, 4-bit reserved, stream ID, sequence, 32-bit microsecond timestamp, 64-bit session ID, big-endian, exactly 16 bytes
- [x] 4.2 Write Rust unit tests for header round-trip, unknown version rejection, datagram shorter than 16 bytes, and mismatched session ID
- [x] 4.3 Add a fuzz or property test over the header parser asserting no panic on arbitrary byte input
- [x] 4.4 Implement the equivalent `MediaHeader` in Kotlin using `ByteBuffer` with `BIG_ENDIAN`, with the same unit test cases
- [x] 4.5 Add a cross-implementation fixture: a committed set of encoded header bytes that both the Rust and Kotlin test suites parse and assert identical field values

## 5. Device identity and discovery

- [x] 5.1 Implement stable device identity on desktop: generate a UUID on first run, persist to the Tauri app config directory, load on subsequent runs
- [x] 5.2 Implement stable device identity on Android using DataStore with the same generate-once-then-load semantics
- [x] 5.3 Implement mDNS advertisement in `unifiedstream-net::discovery` using `mdns-sd`: register `_unifiedstream._udp.local.` with the TXT keys from task 1.3, unregister with a goodbye on shutdown
- [x] 5.4 Implement mDNS browsing on Android via `NsdManager`, exposing discovered devices as a `StateFlow<List<DiscoveredDevice>>`
- [x] 5.5 Acquire and release the Wi-Fi `MulticastLock` around the browse lifecycle
- [x] 5.6 Deduplicate discovered devices by TXT `id` so one desktop on multiple interfaces yields one list entry, and remove entries on service-lost
- [x] 5.7 Implement the manual IP/port entry path with validation, surfaced after a 10-second discovery timeout
- [x] 5.8 Detect and surface the no-network state on Android instead of showing an empty device list

## 6. Control channel

- [x] 6.1 Implement the newline-delimited JSON codec on both sides, rejecting malformed lines with an `error` response while keeping the connection open
- [x] 6.2 Implement the Tokio TCP control listener on desktop, accepting one connection and dispatching parsed messages
- [x] 6.3 Implement the Kotlin TCP control client with a coroutine read loop and a serialized write channel
- [x] 6.4 Implement the `hello` / `hello_ack` handshake including protocol version check, capability intersection, random 64-bit session ID generation, and UDP media port assignment
- [x] 6.5 Reject incompatible protocol versions with an `error` naming the supported version, then close the connection
- [x] 6.6 Implement pairing: prompt the desktop user for unknown device IDs, persist accepted IDs as trusted, skip the prompt for trusted devices, treat a 30-second no-answer as rejection, and reject with reason `rejected`
- [x] 6.7 Enforce single active session — reply `error` with reason `busy` to a second `hello` and leave the existing session untouched
- [x] 6.8 Implement the 1 Hz `ping`/`pong` heartbeat on both sides, deriving RTT samples from echoed timestamps
- [x] 6.9 Declare the peer unreachable after three consecutive unanswered pings and transition to Reconnecting
- [x] 6.10 Implement `bye` teardown on both sides: stop media, close the UDP socket, close the control connection, return to Idle without reconnecting

## 7. Connection state machine

- [x] 7.1 Implement the state enum (Idle, Discovering, Connecting, Connected, Reconnecting, Failed) with a `Failed` reason payload, on both platforms
- [x] 7.2 Publish state changes to the UI within 100ms — Tauri events on desktop, `StateFlow` on Android
- [x] 7.3 Implement exponential-backoff reconnection (500ms, 1s, 2s, 4s, 8s; five attempts) that reuses the existing session ID, and moves to Failed when exhausted
- [x] 7.4 Support user cancellation during Reconnecting, stopping retries immediately and returning to Idle
- [x] 7.5 Unit-test the state machine transitions on both platforms, including cancel-during-reconnect and exhausted-retries

## 8. UDP media transport

- [x] 8.1 Implement UDP socket binding with default port 47811 and ephemeral fallback when taken, communicating the bound port through the handshake; release on teardown
- [x] 8.2 Implement the sender path: per-stream sequence counters, timestamping in microseconds since session start, and fragmentation at a 1200-byte payload MTU with marker on the final fragment
- [x] 8.3 Implement the receiver path: session ID and version filtering, per-stream demultiplexing, and counting packets for unregistered stream IDs
- [x] 8.4 Implement fragment reassembly with per-frame buffers, discarding and counting incomplete frames when a newer frame starts, and never delivering partial frames
- [x] 8.5 Implement loss and reorder detection over 16-bit sequence numbers with correct wrap-around handling, counting late packets separately from lost ones
- [x] 8.6 Implement the bounded 3-packet reorder buffer that releases in sequence order and does not stall when a packet is lost
- [x] 8.7 Implement 32-bit timestamp wrap tracking so frame times stay monotonic past ~71 minutes
- [x] 8.8 Port the full sender and receiver paths to Kotlin against the same behavior
- [x] 8.9 Unit-test both implementations for: fragmentation round-trip, out-of-order fragment arrival, lost-fragment discard, sequence wrap, timestamp wrap, and reorder-window release-on-loss

## 9. Synthetic test stream

- [x] 9.1 Implement a generator on stream ID 0 producing verifiable payloads at a configurable rate and frame size, on both platforms
- [x] 9.2 Implement receiver-side integrity verification reporting delivered, lost, and late counts
- [x] 9.3 Expose start/stop of the test stream in both UIs, including a frame size above 1200 bytes to exercise fragmentation

## 10. Telemetry

- [x] 10.1 Implement RTT smoothing as an EWMA (α=0.2) over the last 10 heartbeat samples, with an explicit unavailable state before the first samples arrive
- [x] 10.2 Implement sent and received throughput counters sampled per interval and reported in Mbps, falling to zero after an idle interval
- [x] 10.3 Implement packet loss percentage per interval from sequence gaps, keeping late packets out of the loss count
- [x] 10.4 Implement RFC 3550 interarrival jitter, reported in milliseconds
- [x] 10.5 Send the `telemetry` control message at 1 Hz while Connected and stop when the session ends
- [x] 10.6 Unit-test throughput accuracy within 10% of a known byte rate, and loss percentage against a synthetic gap sequence

## 11. Desktop UI

- [x] 11.1 Replace the Tauri template front end with the app shell: dark theme, glassmorphism surfaces, and a layout holding the device panel, status dashboard, and feature toggles
- [x] 11.2 Add Tauri commands for start/stop advertising, accept/reject pairing, disconnect, and start/stop the test stream
- [x] 11.3 Emit Tauri events for connection state changes, pairing requests, and 1 Hz telemetry ticks; wire them to React state
- [x] 11.4 Build the pairing prompt UI with accept/reject and the 30-second timeout
- [x] 11.5 Build the status dashboard rendering latency, throughput, loss, jitter, connection state, peer name, failure reason, and a good/degraded/poor link-quality indicator
- [x] 11.6 Clear live metric values on disconnect rather than leaving stale readings on screen
- [x] 11.7 Render disabled Camera, Microphone, and Speaker toggles that indicate the features are not yet available

## 12. Android UI

- [x] 12.1 Replace the Compose template with the app shell: Material 3 dark theme and navigation between the device list and the session screen
- [x] 12.2 Build the device list screen showing discovered devices with name and capabilities, plus the no-network state and the manual-entry fallback
- [x] 12.3 Build the session screen with connection state, the status dashboard (latency, throughput, loss, jitter, link quality), and disabled Cam/Mic/Speaker toggles
- [x] 12.4 Implement the ViewModel layer bridging discovery, session, and telemetry flows to Compose state
- [x] 12.5 Implement the foreground service that owns the session, with a persistent notification, `POST_NOTIFICATIONS` runtime permission handling, and a partial wake lock while streaming
- [x] 12.6 Verify the session stays Connected and media keeps flowing while the app is backgrounded

## 13. Verification

- [x] 13.1 End-to-end on the CachyOS dev machine: launch the desktop app, discover it from the phone, pair, and reach Connected
- [x] 13.2 Run the synthetic test stream with fragmented frames and confirm integrity verification passes and telemetry populates on both dashboards
- [x] 13.3 Measure round-trip latency on the LAN and record the observed baseline in the change directory
- [x] 13.4 Test reconnection by toggling phone Wi-Fi mid-session and confirm the session resumes with the same session ID
- [x] 13.5 Test pairing rejection, the busy path with a second phone or a second client instance, and clean `bye` teardown from both sides
- [x] 13.6 Test the manual-IP fallback path end to end
- [x] 13.7 Run `cargo test --workspace`, `cargo clippy --workspace`, and `./gradlew testDebugUnitTest` clean
