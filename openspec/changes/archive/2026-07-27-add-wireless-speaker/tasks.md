# Tasks — Add Wireless Speaker

## 1. Protocol & stream lifecycle (both sides)

- [x] 1.1 Update `openspec/specs/protocol.md`: mark stream ID 3 Implemented and add a speaker stream section (§6) defining direction, one-frame-per-transport-frame framing, timestamp semantics, and the stereo interleaved PCM S16LE / Opus payload encoding
- [x] 1.2 Rust: implement the source side of the stream lifecycle for the desktop — send `stream_start` for stream 3 with `{pcm_s16le, 48000, 2, 20}`, await `stream_ack`, surface refusal reasons, honor incoming `stream_request` for stream 3 (start/stop as if toggled locally), send `stream_stop` on toggle-off and teardown
- [x] 1.3 Kotlin: implement the sink side in `SessionManager` — accept/refuse `stream_start` for stream 3 (validate codec and channel count, refuse `unsupported_codec`, gate on negotiated `spk` via the existing `not_negotiated` rule), reply `stream_ack`, send `stream_request` from the phone toggle, release playback state on `stream_stop` and session end
- [x] 1.4 Add cross-implementation test vectors to `testdata/` for stream-3 lifecycle messages with stereo params (`cross_impl_vectors.rs` + `ControlMessageTest.kt`)

## 2. Desktop capture pipeline (Linux/PipeWire)

- [x] 2.1 Add an `AudioCapture` trait to `unifiedstream-audio` (mirror of `AudioSink`: `start(format)`, frame callback/pop, `stop()`) so capture logic is testable without PipeWire
- [x] 2.2 Implement `PipeWireSpeakerSink`: `Audio/Sink` stream node named "UnifiedStream Speaker" (S16LE 48 kHz stereo) on its own loop thread, delivering 20 ms frames through a lock-free ring buffer; node created on stream accept, destroyed on stop/session end/app exit; creation failure surfaces "virtual sink unavailable" and does not start the stream
- [x] 2.3 Desktop send path: pop captured frames, stamp capture timestamps, submit to the existing transport sender on stream ID 3; desktop mute stops frame submission without stopping the stream; RMS level computed from captured frames, throttled for the UI
- [x] 2.4 System audio routing: set the virtual sink as default output via WirePlumber when the route toggle is enabled, persist the previous default before switching, restore it on stream stop, session end, and app startup (stale-takeover sweep)

## 3. Android playback pipeline

- [x] 3.1 Create `audio/SpeakerJitterBuffer.kt`: Kotlin port of the desktop jitter buffer policy (target 3 frames, cap 6, silence on underrun, drop-oldest on overrun) with unit tests mirroring the Rust ones
- [x] 3.2 Create `audio/SpeakerPlayback.kt`: `AudioTrack` in low-latency mode, `USAGE_MEDIA`, stereo 48 kHz, playback thread popping the jitter buffer; clean start/stop; playback-failure callback that stops the stream and surfaces a speaker error state
- [x] 3.3 Wire reception: register a stream-3 receiver on `StreamReceiver`, validate/decode PCM frames into the jitter buffer, compute RMS level from decoded samples (~15 Hz); local mute via `AudioTrack` volume 0, volume slider 0–100 % via `setVolume()`
- [x] 3.4 Manifest & service: add `foregroundServiceType="mediaPlayback"` to `SessionService` so playback continues in the background

## 4. UI (both apps)

- [x] 4.1 Desktop: speaker toggle (runs `stream_start` directly), mute button, live level meter, virtual-sink status indicator, route-system-audio toggle, and refusal/unavailable/error states in the React app + Tauri commands/events
- [x] 4.2 Android: speaker toggle (sends `stream_request`), local mute, volume slider, live level meter, and error states (refusal reason, playback failure) in the Compose screens + ViewModel

## 5. End-to-end verification

- [x] 5.1 Loopback integration test in Rust: synthetic stereo PCM frames through capture buffer → sender → transport → reassembly, asserting byte-identical interleaved audio, 4-fragment framing of 3840-byte frames, and one-frame loss cost
- [x] 5.2 Manual E2E on CachyOS: play music into "UnifiedStream Speaker" (visible in pavucontrol) → hear it on the phone; verify phone toggle via `stream_request`, desktop mute → phone silence, volume slider, background playback, route-toggle default restore (including after a forced kill), reconnect mid-stream, and no orphaned node after exit; note observed latency in the change notes

## 6. Opus (stretch — change is complete without it)

- [ ] 6.1 Desktop: encode 20 ms stereo frames via the `opus` crate, offer `opus` in `stream_start` only when the encoder initializes
- [ ] 6.2 Android: runtime-probe MediaCodec Opus decoder, accept `opus` in `stream_start` only when present, decode into the same jitter buffer; extend the loopback test to Opus
