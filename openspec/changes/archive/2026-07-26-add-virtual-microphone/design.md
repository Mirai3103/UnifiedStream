# Virtual Microphone — Design

## Context

The transport and control planes are implemented and verified: mDNS discovery, TCP control channel with handshake/pairing/heartbeat/reconnect, and a UDP media transport with fragmentation, reorder buffering, loss accounting, and a synthetic test stream on stream ID 0. Stream ID 2 (Microphone, phone → PC) is reserved in `openspec/specs/protocol.md` §2.2 but unimplemented, and the control protocol has no way to start or stop a media stream.

The dev machine is CachyOS Linux running PipeWire; the MVP targets Linux only. `unifiedstream-net` is deliberately free of system dependencies (no Tauri, no audio libraries) — that boundary should survive this change.

## Goals / Non-Goals

**Goals:**

- Phone mic audio playing through a virtual source on the Linux desktop that any application can select as its input device, with end-to-end latency low enough for calls (target < 150 ms, ideally < 80 ms).
- Generic stream lifecycle control messages that camera and speaker can adopt unchanged.
- Mic controls: mute, software gain, Android noise suppression toggle; live level meters in both UIs.
- Codec negotiation with PCM S16LE as the always-available baseline and Opus when both ends support it.

**Non-Goals:**

- Windows audio output (later change; the receive path is structured so only the sink module is platform-specific).
- Camera and speaker streams (they only get the lifecycle messages designed here, not implementations).
- Acoustic echo cancellation, adaptive jitter buffering, clock-drift resampling — MVP uses a fixed-depth jitter buffer with drop/underrun behavior.
- Multiple simultaneous phones (single-session invariant already enforced by session control).

## Decisions

### D1: Generic stream lifecycle messages, not mic-specific ones

The control channel gains four message types, all JSON like the existing ones:

- `stream_start` — sent by the media **source** (phone, for mic). Carries `stream` (numeric stream ID), and `params`: `{ "codec": "pcm_s16le" | "opus", "sample_rate": 48000, "channels": 1, "frame_ms": 20 }`.
- `stream_ack` — sent by the sink in response. `{ "stream": 2, "accepted": true }` or `accepted: false` with a `reason` (`"unsupported_codec"`, `"not_negotiated"`, `"busy"`, `"internal"`). Media packets for the stream may flow only after an accepting ack.
- `stream_stop` — sent by either side; the stream is closed and its transport state (sequence counters, reassembly, jitter buffer) is released.
- `stream_request` — sent by the **sink** to ask the source to start or stop (`{ "stream": 2, "active": true }`). This is what makes the desktop's mic toggle work, and later the phone's speaker toggle. The source responds by sending (or stopping) `stream_start` as if the user had toggled locally.

Rationale: the source owns the format decision because it knows its capture hardware; the sink retains a veto via `stream_ack`. A single request message covering both directions of intent avoids a per-feature message zoo. Alternative considered: mic-only messages (`mic_start` etc.) — rejected because cam/spk would immediately duplicate them.

Streams may only be started when the corresponding capability token (`mic`) is in the negotiated intersection; otherwise the sink refuses with `not_negotiated`.

### D2: PCM baseline, Opus negotiated — PCM ships first

- **PCM S16LE, 48 kHz, mono, 20 ms frames**: 1920-byte frames (~0.79 Mbps). Trivially correct, zero codec dependencies, and the transport already handles >1200-byte frames via fragmentation (each PCM frame = 2 fragments). On a local Wi-Fi link this bandwidth is fine — the synthetic stream already validated higher rates.
- **Opus** (~32 kbps, one packet per datagram) is negotiated: the phone offers it in `stream_start` only when an encoder is actually available. Android's MediaCodec Opus encoder requires API 29+, and the app's minSdk is 24, so Opus capability is probed at runtime. Desktop decodes via the `opus` crate (libopus).
- Opus is a **stretch task within this change**: the wire format, negotiation, and desktop decode are specified now, but the change is complete and archivable with PCM only if Opus turns out to be painful.

Alternative considered: Opus-only (matches the specs.md ambition) — rejected for MVP because it puts a codec dependency on the critical path of the first real media feature, and minSdk 24 devices would have no path at all.

### D3: Payload framing — one audio frame per media frame

Each transport frame (the unit the existing sender/reassembler deals in) carries exactly one 20 ms audio frame. The media header timestamp is the capture timestamp of the frame's first sample, in microseconds since session start — same clock the synthetic stream uses. No additional payload header for PCM. For Opus, the payload is a single raw Opus packet (self-describing). Packet loss therefore costs exactly one 20 ms frame, and the existing incomplete-frame discard logic does the right thing for a lost PCM fragment.

### D4: Desktop virtual source via a PipeWire stream node, in a new crate

A new workspace crate `unifiedstream-audio` (inside `desktop/src-tauri/crates/`) owns all audio system integration, keeping `unifiedstream-net` dependency-free. It uses the `pipewire` crate to create a stream with `media.class = "Audio/Source"`, node name **"UnifiedStream Microphone"**, format F32 or S16 48 kHz mono. PipeWire pulls samples from our process; applications see an ordinary microphone node in any device picker (pavucontrol, browser, Discord).

The PipeWire loop runs on its own thread. The network receive path pushes decoded frames into a ring-buffer **jitter buffer** (target depth ~3 frames / 60 ms, capped at 6): the PipeWire process callback pops samples; an empty buffer yields silence (which also makes phone-side mute free — see D6); a full buffer drops the oldest frame. Clock drift between the phone's capture clock and PipeWire's graph clock is absorbed by those two behaviors for MVP; no resampler.

Alternatives considered: `pactl load-module module-pipe-source` (FIFO) — no Rust bindings needed, but fragile lifecycle, fixed format, and orphaned modules on crash; ALSA loopback — pre-dates PipeWire, poor UX. The `pipewire` crate needs libpipewire headers at build time, which is acceptable for a Linux-first project.

The node is created when the mic stream is accepted and destroyed on `stream_stop`/session end, so a dead phone never leaves a zombie microphone behind.

### D5: Android capture pipeline

`AudioRecord` (source `MIC`), 48 kHz mono PCM16, read in 20 ms chunks on a dedicated thread owned by the existing foreground `SessionService`. New `audio/` package: `MicCapture` (AudioRecord lifecycle), `MicController` (gain, mute, NS state, level), codec wrapper for the Opus stretch task. Android's `NoiseSuppressor` effect is attached to the AudioRecord session when available and enabled (`NoiseSuppressor.isAvailable()` gates the UI toggle). Software gain (0.5×–4.0×, default 1×) is applied to samples with clipping guard before encoding; the level meter is RMS + peak computed on the same pass, throttled to ~15 Hz for the UI.

Manifest changes: `RECORD_AUDIO` permission (runtime-requested from the mic toggle) and `foregroundServiceType="microphone"` added to the existing service (required on API 30+/34+ for background mic access).

### D6: Mute stops transmission

Mute (from either UI) stops sending frames entirely rather than sending silence — with PCM, silence would still cost 0.79 Mbps. The desktop jitter buffer underruns and the virtual source emits silence naturally. The stream stays *started* (no `stream_stop`), so unmute is instant. The desktop's toggle drives the same behavior remotely via `stream_request`.

### D7: Telemetry and status reuse existing channels

Mic bandwidth/loss ride the existing per-stream transport accounting and the 1 Hz `telemetry` message — no new telemetry fields. The desktop UI additionally shows virtual-source state (node created / streaming / underrunning) from the audio crate's status events, delivered over the existing Tauri event channel used for connection state.

## Risks / Trade-offs

- [`pipewire` crate is low-level and callback-heavy; misuse can deadlock the graph loop] → Isolate it in `unifiedstream-audio` behind a small trait (`AudioSink`: `start(format)`, `push(frame)`, `stop()`); unit-test the jitter buffer separately from PipeWire; integration smoke test via `pw-cli ls` in a dev script.
- [Clock drift with no resampler causes a drop or a silence gap every few minutes] → Acceptable for MVP; buffer depth counters are logged so the drift rate is measurable before deciding whether a resampler is worth it.
- [PCM at 0.79 Mbps amplifies Wi-Fi loss vs Opus] → Loss costs one 20 ms frame per event (D3); the Opus stretch task is the real fix and the wire format already negotiates it.
- [`RECORD_AUDIO` denial or `foregroundServiceType` misconfiguration breaks capture silently on newer Android] → Explicit UI state for "permission needed"; capture start failures surface as a visible mic error, never a silent dead stream.
- [PipeWire absent (pure PulseAudio distro)] → Out of MVP scope; the sink trait keeps the door open for a PulseAudio implementation, and the UI reports "virtual source unavailable" instead of crashing.

## Open Questions

- None blocking. Deferred deliberately: adaptive jitter/resampling, Windows sink, echo cancellation, and whether Opus uses MediaCodec or a bundled libopus if MediaCodec proves unreliable across devices (resolved within the stretch task if reached).
