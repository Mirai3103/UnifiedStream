# Add Virtual Microphone

## Why

The network foundation (discovery, session control, UDP media transport) is complete and verified end-to-end with the synthetic test stream, but no real media flows yet. The virtual microphone is the first of the three media features (cam/mic/spk) and the best one to build first: audio has the smallest data rates, exercises the whole capture → encode → transport → render pipeline, and establishes the stream-lifecycle control messages that camera and speaker will reuse.

## What Changes

- The phone captures microphone audio, encodes it, and sends it over the existing UDP media transport on stream ID 2 (already reserved in the protocol).
- The desktop receives, decodes, and feeds the audio into a virtual audio source so Linux applications (browsers, Discord, OBS, …) see it as a normal hardware microphone. Linux/PipeWire is the MVP target; Windows comes in a later change.
- New generic stream-lifecycle control messages (`stream_start`, `stream_stop`, `stream_ack`) are added to the control channel, carrying per-stream parameters (codec, sample rate, channels). Camera and speaker will reuse these.
- Audio format negotiation: PCM S16LE 48 kHz mono is the mandatory baseline; Opus is negotiated when both ends support it.
- Mic controls: mute/unmute toggle on both apps, gain adjustment, and Android's built-in noise suppression toggle.
- Both UIs get a microphone toggle plus a live audio-level indicator; the desktop shows the state of the virtual source.
- `openspec/specs/protocol.md` is extended: stream ID 2 becomes Implemented, new §3 control messages, and the mic payload format is defined.

## Capabilities

### New Capabilities

- `microphone-stream`: End-to-end virtual microphone — phone-side audio capture and encoding, mic payload format, desktop-side decoding and playback into a PipeWire virtual source, mic controls (mute, gain, noise suppression), and level indication in both UIs.

### Modified Capabilities

- `session-control`: The control channel gains generic stream lifecycle messages — a peer requests a stream with `stream_start` (codec/format parameters included), the other side acknowledges or refuses with `stream_ack`, and either side ends it with `stream_stop`. Negotiated capabilities gate which streams may be started.

### Modified Specs (non-capability)

- `openspec/specs/protocol.md`: normative wire format for the new control messages, stream ID 2 status, and the microphone payload encoding.

## Impact

- **Android** (`com.laffy.unifiedstream`): new `audio/` package (AudioRecord capture, encoder, level metering); `SessionManager`/`SessionService` wiring for stream lifecycle; `RECORD_AUDIO` permission + `microphone` foreground-service type; mic toggle and level UI in Compose screens.
- **Desktop Rust** (`unifiedstream-net` + `desktop` crates): stream lifecycle handling in the control layer; new audio receive path (jitter handling on top of the existing reorder buffer, decode); a new PipeWire output module (via `pipewire` crate) that publishes an `Audio/Source` node — kept in the Tauri app crate or a new `unifiedstream-audio` crate so `unifiedstream-net` stays free of system dependencies.
- **Desktop UI** (React/Tauri): mic toggle, level meter, virtual-source status.
- **Dependencies**: `pipewire` (Rust bindings, needs libpipewire headers on the build machine); Opus libraries on both platforms only if the Opus task is reached (`opus` crate on desktop, Android MediaCodec Opus encoder).
- **Protocol**: additive only — old peers ignore unknown capability tokens by design, and stream messages are only sent when negotiated.
