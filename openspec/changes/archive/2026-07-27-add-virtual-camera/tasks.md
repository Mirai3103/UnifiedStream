# Tasks — Add Virtual Camera

## 1. Protocol & stream lifecycle (both sides)

- [x] 1.1 Update `openspec/specs/protocol.md`: mark stream ID 1 Implemented and add a camera stream section (§7) defining direction, video `params` (`codec: "mjpeg"`, `width`, `height`, `max_fps`), one-JPEG-per-transport-frame framing, capture-time timestamp semantics, and loss behaviour (any lost/late fragment costs that one frame)
- [x] 1.2 Kotlin: implement the source side in `SessionManager` — a `CameraStreamState` lifecycle (mirroring `MicStreamState`), send `stream_start` for stream 1 with the video params, await `stream_ack`, surface refusal reasons, honor incoming `stream_request` for stream 1 (gated on the `CAMERA` permission — refuse rather than crash when denied), send `stream_stop` on toggle-off, capture failure, and teardown
- [x] 1.3 Rust: replace the `UnsupportedStream` refusal for stream 1 in `app.rs::handle_stream_start` with real sink wiring — validate codec (`mjpeg` only, refuse `unsupported_codec`), validate dimensions against the reassembly size cap, create the video sink under `spawn_blocking`, register stream 1 in the receiver demux, ack; handle the replacement `stream_start` (re-ack, reset receive state, reopen the device at the new format)
- [x] 1.4 Add cross-implementation test vectors to `testdata/` for stream-1 lifecycle messages with video params (`cross_impl_vectors.rs` + `ControlMessageTest.kt`)

## 2. Desktop sink pipeline (Linux/v4l2loopback)

- [x] 2.1 Create the `unifiedstream-video` crate with a `VideoSink` trait (`start(format)`, `push_frame(jpeg, timestamp)`, `stop()`) and a `VideoFormat` type; workspace member with the same lint set; system deps gated on `cfg(target_os = "linux")`
- [x] 2.2 Implement v4l2loopback device discovery: enumerate `/dev/video*`, identify loopback devices by driver name, prefer card label "UnifiedStream Camera", else first loopback device without an active writer; expose a status enum (found / absent) with the modprobe guidance string
- [x] 2.3 Implement `V4l2LoopbackSink`: dedicated worker thread owning the device fd, JPEG → planar YUV420 decode via `zune-jpeg`, format negotiation on start, frame writes; decode failure drops and counts the frame without stopping the stream; `Drop` releases the device so no writer outlives the app
- [x] 2.4 Verify reassembly handles video-scale frames: bounded per-stream buffer sized for the largest negotiated resolution, prompt release, and a transport test exercising ~50-fragment frames (round-trip, single-fragment loss ⇒ single-frame loss, late-fragment discard)

## 3. Android capture pipeline

- [x] 3.1 Add CameraX dependencies to the version catalog and `CAMERA` permission + `foregroundServiceType="camera"` to the manifest/`SessionService`; permission launcher in `MainActivity` following the mic's `RECORD_AUDIO` flow
- [x] 3.2 Create `video/FrameEncoder.kt`: YUV_420_888 → NV21 → JPEG (`YuvImage.compressToJpeg`), quality constant, `max_fps` pacing (skip frames arriving faster), Android-camera-free and unit-tested (pacing and skip logic, following the `MicController` pattern)
- [x] 3.3 Create `video/CameraCapture.kt`: CameraX `ImageAnalysis` (`YUV_420_888`, `STRATEGY_KEEP_ONLY_LATEST`) + `Preview` use cases, resolution selection (480p/720p/1080p), front/back facing switch that rebinds capture without touching the stream, `onFrame`/`onFailure` callbacks
- [x] 3.4 Wire sending in `SessionManager`: bounded send queue of 2 dropping the newest on overflow, capture timestamps on frames, submit to `MediaSender` on stream 1 with intra-frame fragment pacing; resolution change sends a replacement `stream_start`; stopping the stream unbinds CameraX immediately

## 4. UI (both apps)

- [x] 4.1 Desktop: replace the camera placeholder card with a real one — toggle (sends `stream_request`), negotiated resolution + delivered-fps display (0 fps distinguishable from stopped), v4l2loopback-missing state with the copyable modprobe command, and refusal/error states in the React app + Tauri commands/events (`camera-status`, `camera-stats`)
- [x] 4.2 Android: camera card in Compose — toggle (runs `stream_start` directly), live preview while active, facing switch, resolution selector, permission-denied and refusal/error states in `Screens.kt` + ViewModel

## 5. End-to-end verification

- [x] 5.1 Integration test in Rust: synthetic JPEG frames through sender → transport → reassembly → decode, asserting byte-identical delivery, correct fragment counts for realistic frame sizes, one-frame loss cost, and that an undecodable frame is dropped without killing the sink
- [x] 5.2 Manual E2E on CachyOS: validated by the user — streaming works at 480p (~16 fps delivered, 454 frames, 0 undecodable) and, after the aspect-ratio/renegotiation fix, at 720p. Change approved. Not individually re-verified: module-missing guidance, permission-denied refusal, reconnect mid-stream, no-writer-after-exit — covered by code paths and accepted as-is.

## 6. Stretch / follow-up (change is complete without these)

- [ ] 6.1 Sender-side quality autotune: drop the JPEG quality constant one step when telemetry loss exceeds a threshold for several seconds (static knob remains the spec'd behaviour; this is polish, not adaptive bitrate)
- [ ] 6.2 Improve delivered fps (user-requested follow-up, 2026-07-27): profile the phone encode path at 720p30 — NV21 repack allocates a fresh buffer per frame and `YuvImage.compressToJpeg` is software; candidates are buffer reuse, a lower JPEG quality constant, and a second encode thread. Measure whether the bottleneck is camera exposure (~15 fps in dim light), encode time, or Wi-Fi loss before optimizing.
