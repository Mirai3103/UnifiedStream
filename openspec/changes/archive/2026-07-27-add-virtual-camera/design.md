# Virtual Camera — Design

## Context

The microphone and speaker changes shipped everything a media stream needs on the wire: the fragmenting UDP transport with per-stream reassembly and loss accounting, generic stream lifecycle messages with capability gating, sink-initiated toggles via `stream_request`, and 1 Hz telemetry. Stream ID 1 (Camera, phone → PC) and the `cam` capability token are reserved in `openspec/specs/protocol.md`, advertised in every handshake, and hard-refused today in `app.rs` (`UnsupportedStream` — "The camera capability is advertised but its sink is a later change").

The roles match the microphone exactly: the phone is the source, the desktop is the sink, and the desktop drives the toggle with `stream_request` (§3.9.4). The phone's `MediaSender` and the desktop's receiver demux (`MediaCmd::Register`) are already in place. What is new is everything video-specific: camera capture and JPEG encoding on the phone, JPEG decoding and a v4l2loopback virtual webcam on the desktop, and frames that are 30–100 KB instead of 2–4 KB.

The dev machine is CachyOS Linux; the MVP targets Linux only. `unifiedstream-net` stays free of system dependencies; system video APIs get their own crate, mirroring how `unifiedstream-audio` is the only crate linking libpipewire.

## Goals / Non-Goals

**Goals:**

- The phone's camera visible as a normal webcam ("UnifiedStream Camera") in Linux video-call apps and browsers, with latency low enough for live conversation (target < 250 ms glass-to-glass on LAN).
- 1280×720 at up to 30 fps as the default, with phone-side resolution (480p/720p/1080p) and front/back facing selection, and a local preview on the phone.
- Camera toggle from either UI (desktop drives it via `stream_request`), reusing the lifecycle machinery unchanged.
- Graceful degradation when v4l2loopback is missing: a clear, actionable message, not a silent failure.
- No protocol additions beyond documenting stream ID 1's video `params` and payload — the "video streams will define their own `params`" hook in §3.9.1 gets filled in.

**Non-Goals:**

- Windows virtual camera (later change; the v4l2loopback sink module is the only platform-specific desktop piece).
- H.264 or any inter-frame codec — specified as a future negotiated option, not implemented here (stretch at most). MJPEG is the baseline this change ships.
- Audio/video synchronization with the mic stream; each stream has independent latency targets.
- Screen mirroring, phone-as-sink video, simultaneous front+back capture, zoom/torch/exposure controls.
- Adaptive bitrate control. The quality knob is static per stream; telemetry makes loss visible so the user can drop resolution manually.

## Decisions

### D1: MJPEG baseline, one JPEG per transport frame

`stream_start` params: `{ "codec": "mjpeg", "width": 1280, "height": 720, "max_fps": 30 }`. Each transport frame carries exactly one complete JPEG image; the media header timestamp is the capture time of the frame.

This is the video analogue of the PCM decision, for the same reason: the transport already discards incomplete frames and delivers complete ones, so a codec where **every frame is independently decodable** makes packet loss cost exactly one video frame and nothing else. An inter-frame codec (H.264) would smear one lost packet into visible corruption until the next keyframe, or force a keyframe-request back-channel the protocol does not have. MJPEG needs no decoder state, no parameter-set exchange, and no hardware encoder negotiation across the zoo of Android devices.

Alternatives considered:

- **H.264 via MediaCodec** — 5–10× better compression, but inter-frame dependency fights the transport's frame-drop semantics, hardware encoder behaviour varies per device, and the desktop would need a real decoder dependency (ffmpeg/openh264). Deferred to a later change as a negotiated option, exactly as Opus relates to PCM.
- **Raw YUV** — 720p30 is ~660 Mbps; not even worth prototyping.

Bandwidth: a 720p JPEG at quality ~70 is roughly 40–80 KB, so 30 fps is ~10–20 Mbps — comfortable on modern Wi-Fi LAN, and the resolution/fps knobs are the escape hatch on weak links.

### D2: Phone capture via CameraX ImageAnalysis with keep-latest backpressure

A new Android `video/` package: `CameraCapture` wraps CameraX with an `ImageAnalysis` use case (`YUV_420_888`, `STRATEGY_KEEP_ONLY_LATEST`) plus a `Preview` use case bound to the UI, and `FrameEncoder` converts YUV_420_888 → NV21 → JPEG via `YuvImage.compressToJpeg`. Encoding happens on the analysis executor thread; if it cannot keep up at the requested fps, CameraX's keep-latest strategy drops camera frames upstream, so the pipeline degrades to a lower frame rate instead of building latency — the video equivalent of the mic's bounded frame queue.

`FrameEncoder` (quality selection, fps pacing, byte assembly) is Android-camera-free and unit-testable, following the `MicController` pattern.

Alternatives considered:

- **Camera2 directly** — more control, much more lifecycle code, and CameraX's keep-latest backpressure is precisely the behaviour we want. Rejected.
- **CameraX `ImageCapture` (JPEG) use case** — hardware JPEG, but designed for stills; it cannot sustain 30 fps. Rejected.
- **MediaCodec MJPEG encoder** — rarely present on real devices. Rejected.

### D3: Desktop sink is a new `unifiedstream-video` crate writing decoded frames to v4l2loopback

A new crate `unifiedstream-video` (the only crate touching system video APIs, mirroring `unifiedstream-audio`'s charter) with:

- A `VideoSink` trait (`start(format)`, `push_frame(jpeg_bytes, timestamp)`, `stop()`) so lifecycle wiring in `app.rs` is testable without a kernel module.
- A Linux `V4l2LoopbackSink`: decodes each JPEG to planar YUV420 and writes it to a v4l2loopback output device via the `v4l` crate, negotiating the format (`VIDIOC_S_FMT`, `YU12`) when the stream starts. Decoding uses a pure-Rust JPEG decoder (`zune-jpeg`) — no C toolchain dependency; ~2–6 ms per 720p frame is well inside a 33 ms budget.

Frames are decoded rather than passing MJPEG through to the loopback device because consumer support for MJPEG loopback output is inconsistent (some apps only probe raw formats), while every consumer accepts YUV420. Decode also gives us the place to validate frames before a kernel device sees them.

Device selection: the sink enumerates `/dev/video*` for v4l2loopback devices (driver name `v4l2 loopback`), preferring one whose card label is "UnifiedStream Camera", else the first loopback device with no active writer. If none exists, the `stream_start` is refused with reason `internal` and the UI shows setup guidance including the exact command:
`sudo modprobe v4l2loopback card_label="UnifiedStream Camera" exclusive_caps=1`. The app never loads kernel modules itself — that requires root and is the user's call.

Alternatives considered:

- **Ship/install the module for the user** — needs root, distro-specific, fragile. Rejected; guidance instead.
- **PipeWire camera node** — PipeWire video is the future, but application support (browsers especially) still overwhelmingly probes v4l2. v4l2loopback is what OBS's virtual camera uses today. Rejected for MVP.

### D4: Toggle symmetry copied from the microphone

The phone is the source, so its toggle runs the `stream_start` flow directly (after the `CAMERA` permission check — a `stream_request` is a request, not a command, per §3.9.4). The desktop is the sink, so its toggle sends `stream_request` `{ "stream": 1, "active": true|false }`, exactly like its mic switch. Gating on `cam` in the negotiated intersection is the existing generic rule. No new control messages.

The refusal surface also mirrors the mic: permission denied on the phone → the desktop sees `stream_ack` `accepted: false` (reason `internal`) and shows a "camera unavailable on phone" state; missing v4l2loopback on the desktop → the phone's `stream_start` is refused with `internal` and the phone shows the refusal.

### D5: Facing switch keeps the stream; resolution change replaces it

Switching front/back camera restarts CameraX capture but changes nothing on the wire — same resolution, same stream, at worst a few dropped frames while the camera reopens. The desktop notices nothing.

Changing resolution changes the negotiated `params`, so the phone sends a fresh `stream_start` with the new `width`/`height`. Protocol §3.9.1 already defines this: a `stream_start` for an active stream **replaces its parameters — the sink re-acks and resets receive state**. The desktop sink handles the re-ack by closing and reopening the loopback device with the new format (v4l2 format is fixed while a writer holds the device). Consumers attached to the virtual camera may glitch on a mid-call resolution change; that is inherent to v4l2 and acceptable.

The same replacement flow absorbs device reality: CameraX's resolution selection is aspect-ratio-first (its default 4:3 strategy outranks the size request, so 1280×720 must be requested together with a 16:9 aspect strategy), and a device that genuinely lacks the requested size delivers its closest supported one. When delivered frames differ from the negotiated geometry, the phone renegotiates once to the delivered size (even dimensions required) rather than failing — the desktop reopens its device at what the camera can actually produce.

### D6: Frame pacing — drop, never block; a stalled stream freezes the virtual camera

The phone's send queue is bounded (small — 2 frames) and drops the newest frame when full, because with video the freshest complete frame is the next one; queuing stale frames only adds latency. `max_fps` caps the pace; the encoder skips camera frames that arrive faster.

On the desktop, a gap in arriving frames simply means the loopback device is not written — consumers keep displaying the last frame. This is the video analogue of the audio gap-tolerance rule and needs no keepalive. The desktop UI shows the delivered frame rate so a frozen stream is visible as "0 fps", distinct from "stream stopped".

### D7: Fragmentation load is accepted and measured, not redesigned around

A 720p frame at ~60 KB fragments into ~50 packets, against a per-stream reorder window of 3 and incomplete-frame-discard on newer-timestamp arrival — the rules already written into §2.4/§2.6 and exercised by tests up to 4 fragments. The design treats those rules as correct and complete for video (any lost or late fragment ⇒ that frame is dropped, count it, move on) and adds no transport features. Two practical accommodations:

- The reassembly buffer's size assumptions are revisited: it must happily hold ~100 KB frames and release them promptly (bounded per-stream memory, e.g. 512 KB cap ⇒ frames larger than the cap are refused at `stream_start` time via resolution limits).
- The sender paces fragments of one frame across a few milliseconds rather than blasting 50 datagrams back-to-back, which measurably reduces burst loss on consumer Wi-Fi and costs nothing at these rates.

Loss economics are worse than audio (1% packet loss ⇒ ~40% of 50-fragment frames lost), which is fine at LAN loss rates (≪ 0.1%) and visible in telemetry when it is not. The resolution knob and the future H.264 option are the mitigations; adaptive behaviour is a non-goal.

### D8: Privacy posture rides the platform

Camera capture runs only while the stream is active, under the existing foreground service extended with `foregroundServiceType="camera"` — Android then enforces the system camera indicator and background-capture restrictions. The phone UI always reflects capture state, and stopping the stream releases the camera immediately (CameraX unbind). No frames are ever captured outside an accepted stream, which the spec pins as a requirement rather than an implementation nicety.

## Risks / Trade-offs

- [v4l2loopback is not installed on most systems out of the box] → Detection with exact, copyable setup guidance in the desktop UI; the stream refuses cleanly (`internal`) instead of half-starting. Documented as a system requirement.
- [YUV→JPEG in software at 720p30 may saturate a weak phone core] → CameraX keep-latest drops frames upstream so latency stays flat while fps sags; quality constant chosen with headroom; 1080p is offered but explicitly best-effort. Measured on a real device in the E2E task before tuning further.
- [~50-fragment frames amplify Wi-Fi burst loss into whole-frame loss] → Intra-frame fragment pacing on the sender; loss and delivered-fps are visible in telemetry/UI; resolution knob as the manual fallback. Accepted for LAN MVP.
- [Consumers cache v4l2 formats; a resolution change mid-call can glitch attached apps] → Resolution changes reopen the device (D5); the UI notes that changing resolution while apps are attached may require re-selecting the camera. Inherent to v4l2loopback.
- [`exclusive_caps=1` module option changes device visibility semantics (device hidden until a writer attaches); users without it see a black "camera" in app pickers even when idle] → Setup guidance recommends `exclusive_caps=1`; the desktop tolerates either configuration.
- [A second `!Send` system-integration stack (v4l2 fd + decoder on a worker thread) beside PipeWire's doubles the lifecycle surface in `app.rs`] → Same pattern that worked twice already: dedicated worker thread owning the device, communication via bounded channels, `Drop` guarantees the device is released, creation under `spawn_blocking`.
- [JPEG frames are parsed by a Rust decoder fed from the network] → `zune-jpeg` is memory-safe Rust; decode errors drop the frame and count it, never the stream. The workspace's deny-`unwrap` lints apply to the new crate.

## Open Questions

- None blocking. Deferred deliberately: Windows virtual camera backend (likely a DirectShow/MediaFoundation filter — a separate change), H.264 as a negotiated codec (stretch at most), phone torch/zoom controls, and whether the desktop should expose a "mirror horizontally" convenience toggle (pure sink-side pixel transform; can ride any later change).
