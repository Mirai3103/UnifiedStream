# Add Wireless Speaker

## Why

The virtual microphone proved the full audio pipeline (capture → encode → UDP transport → jitter buffer → render) and shipped the generic stream lifecycle messages that were designed for camera and speaker to reuse. The wireless speaker is the natural next media feature: it is the mirror image of the microphone — PC → phone instead of phone → PC — so it reuses the transport, the lifecycle messages, the audio payload format, and the jitter-buffer design almost unchanged, while delivering the second of the three headline features (cam/mic/spk).

## What Changes

- The desktop captures PC system audio and sends it over the existing UDP media transport on stream ID 3 (already reserved in the protocol), using the existing `stream_start`/`stream_ack`/`stream_stop`/`stream_request` lifecycle.
- On Linux (MVP target), the desktop publishes a PipeWire virtual sink named "UnifiedStream Speaker" that applications — or the whole system, when the user routes default output to it — play into; the desktop captures that sink's audio for transmission. Windows comes in a later change.
- The phone receives, decodes, buffers, and plays the audio through its speaker or connected headphones, continuing in the background under the existing foreground service.
- Audio format: PCM S16LE 48 kHz is the mandatory baseline (as for the mic), now with stereo support (`channels: 2`); Opus remains a negotiated option when both ends support it.
- Speaker controls: an on/off toggle in both UIs (the phone drives it as the sink via the existing `stream_request` message), mute on both sides, and a phone-side volume control.
- Both UIs show a live output-level indicator; the desktop shows the virtual sink's state.
- `openspec/specs/protocol.md` is extended: stream ID 3 becomes Implemented and a new section defines the speaker stream payload (a mirror of the microphone section, with stereo interleaving).

## Capabilities

### New Capabilities

- `speaker-stream`: End-to-end wireless speaker — desktop-side system audio capture via a PipeWire virtual sink, the speaker payload format on stream ID 3, phone-side decoding, jitter buffering, and playback, speaker controls (toggle, mute, volume), and level indication in both UIs.

### Modified Capabilities

<!-- No requirement-level changes to existing capabilities. The stream lifecycle,
     capability gating, and sink-initiated stream_request in session-control were
     already specified generically and cover the speaker unchanged. media-transport
     carries stream 3 with no new requirements. -->

None.

### Modified Specs (non-capability)

- `openspec/specs/protocol.md`: stream ID 3 status becomes Implemented; a new speaker stream section defines direction, framing, and the stereo PCM/Opus payload encoding.

## Impact

- **Desktop Rust** (`unifiedstream-audio` crate): a new PipeWire virtual sink module (`media.class = Audio/Sink`, capture side) alongside the existing virtual source; a capture → frame → send path feeding the existing transport sender; desktop-side mute and level metering. `unifiedstream-net` stays free of system dependencies.
- **Desktop app** (`src-tauri` + React UI): speaker stream lifecycle wiring as a stream *source* (the desktop initiated only sink-side flows until now); speaker toggle, level meter, and virtual-sink status in the UI.
- **Android** (`com.laffy.unifiedstream`): a playback path in the `audio/` package (`AudioTrack` output, jitter buffer mirroring the desktop's design, level metering); `SessionManager`/`SessionService` wiring for a stream where the phone is the sink; `mediaPlayback` foreground-service type; speaker toggle, volume, and level UI in Compose.
- **Dependencies**: none new on Android; desktop reuses the `pipewire` crate already in `unifiedstream-audio`. Opus decode on Android (MediaCodec) only if the Opus stretch task is reached.
- **Protocol**: additive only — stream 3 and the `spk` capability token were reserved from the start, and peers that do not negotiate `spk` never see the stream.
