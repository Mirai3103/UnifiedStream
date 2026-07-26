# Add Virtual Camera

## Why

The camera is the last of the three headline features (cam/mic/spk) and the one the project is named for. The microphone and speaker changes built everything a media stream needs — the UDP transport with fragmentation and reassembly, the generic stream lifecycle messages, sink-driven toggles via `stream_request`, and the pattern of a system-integration module per platform — and stream ID 1 plus the `cam` capability token have been reserved and advertised since the first handshake. What remains is the video-specific work: capturing camera frames on the phone, compressing them, and presenting them on the desktop as a virtual webcam that any video-call application can select.

## What Changes

- The phone captures camera frames and sends them over the existing UDP media transport on stream ID 1 (already reserved), using the existing `stream_start`/`stream_ack`/`stream_stop`/`stream_request` lifecycle. The phone is the source; the desktop is the sink and can drive the toggle with `stream_request`, exactly as it does for the microphone.
- Video codec: **MJPEG is the mandatory baseline** — each transport frame carries one independently decodable JPEG image, so packet loss costs exactly one video frame and matches the transport's incomplete-frame-discard semantics (the same reasoning that made PCM the audio baseline). H.264 becomes a negotiated option in a later change, mirroring how Opus relates to PCM.
- On Linux (MVP target), the desktop decodes the incoming frames and writes them to a **v4l2loopback** device named "UnifiedStream Camera", so browsers and call apps see a normal webcam. The desktop detects a missing v4l2loopback module and reports actionable guidance instead of failing silently. Windows comes in a later change.
- Default format 1280×720 at up to 30 fps; the phone offers a resolution choice (480p/720p/1080p) and front/back camera switching. Camera switching restarts capture but not the stream.
- Camera controls: an on/off toggle in both UIs (the desktop drives it as the sink via `stream_request`), camera-facing and resolution selection on the phone, and a local preview on the phone.
- The phone requests the `CAMERA` permission on first use and runs capture under the existing foreground service with the `camera` service type; capture continues in the background where the platform allows it.
- Both UIs show the camera stream state (resolution, fps, and delivered-frame rate on the desktop; capture state on the phone).
- `openspec/specs/protocol.md` is extended: stream ID 1 becomes Implemented and a new section defines the camera stream payload — video `params` (`codec`, `width`, `height`, `max_fps`) and the MJPEG frame encoding.

## Capabilities

### New Capabilities

- `camera-stream`: End-to-end virtual camera — phone-side camera capture, MJPEG encoding, and transmission on stream ID 1; desktop-side reassembly, decoding, and presentation as a v4l2loopback virtual webcam; camera controls (toggle, facing, resolution) and stream-state indication in both UIs.

### Modified Capabilities

<!-- The stream lifecycle, capability gating, and sink-initiated stream_request in
     session-control were specified generically and cover the camera unchanged.
     media-transport already fragments and reassembles arbitrary-size frames; video
     frames exercise the existing rules (per-frame timestamps, incomplete-frame
     discard) without adding requirements. -->

None.

### Modified Specs (non-capability)

- `openspec/specs/protocol.md`: stream ID 1 status becomes Implemented; a new camera stream section defines direction, video `params`, framing (one JPEG per transport frame), and loss behaviour.

## Impact

- **Desktop Rust**: a new `unifiedstream-video` crate (mirroring `unifiedstream-audio`'s role as the only crate touching system video APIs) with a Linux v4l2loopback sink module and JPEG decoding; `unifiedstream-net` stays free of system dependencies. The `app.rs` refusal of stream 1 (`UnsupportedStream`) is replaced with real sink wiring alongside the existing mic path.
- **Desktop app** (`src-tauri` + React UI): camera stream lifecycle wiring as a second sink stream; camera toggle, stream state, and v4l2loopback availability status in the UI, replacing the "camera arrives in a later change" placeholder.
- **Android** (`com.laffy.unifiedstream`): a new `video/` package (CameraX capture, YUV→JPEG encoding, frame pacing); `SessionManager`/`SessionService` wiring for a second phone-sourced stream reusing the `MicStreamState` lifecycle pattern; `CAMERA` permission and `camera` foreground-service type; camera card with preview, toggle, facing, and resolution controls in Compose.
- **Dependencies**: desktop adds a JPEG decoder and v4l2 output crate (Linux-only, gated like the `pipewire` dependency); Android adds CameraX (`androidx.camera`). No new protocol-level dependencies.
- **System requirement**: the Linux desktop needs the v4l2loopback kernel module for the virtual webcam to appear; the feature degrades to a clear error message when it is absent.
- **Protocol**: additive only — stream 1 and the `cam` token were reserved from the start, and peers that do not negotiate `cam` never see the stream.
- **Transport pressure**: a 720p MJPEG frame is roughly 30–100 KB and fragments into dozens of packets, exercising reassembly far harder than the four-fragment speaker frames. The existing rules (incomplete-frame discard, reorder window) already define correct behaviour; the risk is throughput, addressed in design.
