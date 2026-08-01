# Tasks — Add Virtual Microphone

## 1. Protocol & stream lifecycle (both sides)

- [x] 1.1 Update `openspec/specs/protocol.md`: mark stream ID 2 Implemented, add §3 definitions for `stream_start`, `stream_ack`, `stream_stop`, `stream_request` with field tables and refusal reasons, and define the microphone payload format (PCM S16LE / Opus, 20 ms frames, timestamp semantics)
- [x] 1.2 Rust: add the four stream lifecycle messages to `unifiedstream-net/src/protocol/messages.rs` (serde round-trip tests included)
- [x] 1.3 Kotlin: add the same messages to `protocol/ControlMessage.kt` with round-trip tests, and add matching cross-implementation test vectors to `testdata/` + `cross_impl_vectors.rs` + `ControlMessageTest.kt`
- [x] 1.4 Rust: implement stream lifecycle state in the session/control layer — track per-stream started/accepted state, enforce media-only-after-ack, refuse with `not_negotiated`/`unsupported_codec`/`internal`, release transport state on `stream_stop` and session end
- [x] 1.5 Kotlin: implement the source-side lifecycle in `SessionManager` — send `stream_start`, await ack, handle refusal reasons, honor incoming `stream_request`, send `stream_stop` on toggle-off and teardown

## 2. Android capture pipeline

- [x] 2.1 Create `audio/MicCapture.kt`: AudioRecord at 48 kHz mono PCM16, 20 ms read loop on a dedicated thread, clean start/stop, capture-failure callback
- [x] 2.2 Create `audio/MicController.kt`: mute (stops frame submission), software gain 0.5×–4.0× with saturation, RMS/peak level metering throttled to ~15 Hz, NoiseSuppressor attach/detach gated on `NoiseSuppressor.isAvailable()`
- [x] 2.3 Wire capture into the media path: submit each 20 ms frame to `MediaSender` on stream ID 2 with capture timestamps; unit-test gain saturation, mute, and level computation
- [x] 2.4 Manifest & service: add `RECORD_AUDIO` permission, `foregroundServiceType="microphone"` on the existing service, runtime permission request flow from the mic toggle with a visible "permission required" state

## 3. Desktop audio sink (Linux/PipeWire)

- [x] 3.1 Create workspace crate `unifiedstream-audio` with an `AudioSink` trait (`start(format)`, `push(frame)`, `stop()`) and a jitter buffer (target 3 frames, cap 6, silence on underrun, drop-oldest on overrun) with unit tests
- [x] 3.2 Implement `PipeWireSource`: `Audio/Source` stream node named "UnifiedStream Microphone" on its own loop thread, fed from the jitter buffer; node created on stream accept, destroyed on stop/session end/app exit
- [x] 3.3 Handle PipeWire unavailability: sink creation failure refuses the stream with `internal` and surfaces "virtual source unavailable" status
- [x] 3.4 Desktop receive path: register a stream-2 receiver on the transport, validate/decode PCM frames, push to the sink, compute RMS level from decoded samples, emit status + level events to the UI layer

## 4. UI (both apps)

- [x] 4.1 Android: mic toggle, mute button, gain slider, noise-suppression switch (hidden when unsupported), live level meter, and error states (permission, capture failure, refusal reason) in the Compose screens + ViewModel
- [x] 4.2 Desktop: mic toggle (sends `stream_request`), level meter, virtual-source status indicator, and refusal/unavailable states in the React app + Tauri commands/events

## 5. End-to-end verification

- [x] 5.1 Loopback integration test in Rust: synthetic PCM frames through sender → transport → reassembly → jitter buffer, asserting byte-identical audio, one-frame loss cost, and underrun/overrun behavior
- [x] 5.2 Manual E2E on CachyOS: phone mic → "UnifiedStream Microphone" visible in pavucontrol → record in a real app; verify mute, gain, remote toggle from desktop, reconnect mid-stream, and no orphaned node after exit; note observed latency in the change notes
