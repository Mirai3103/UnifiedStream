# Tasks — Decouple Platform Integrations

## 1. Complete the media traits

- [x] 1.1 Add `SetupHint { message: String, command: Option<String> }` to `unifiedstream-video` and use it as the payload of `VideoError::Unavailable`, so the remediation command originates in the platform implementation instead of being read from `MODPROBE_HINT` by the application layer
- [x] 1.2 Add `decode_failures(&self) -> u64` and `device_label(&self) -> Option<String>` to the `VideoSink` trait; implement both on `V4l2LoopbackSink` by delegating to the existing inherent methods (`device_path` renders to the label)
- [x] 1.3 Add the `AudioRouting` trait to `unifiedstream-audio` (`enable`, `restore`, `sweep_stale`) covering the whole system-default-output mechanism, not just the switch
- [x] 1.4 Confirm `AudioSink`, `AudioCapture`, and `VideoSink` expose everything `app.rs` uses; no inherent method of a Linux type may remain reachable from the application layer

## 2. Platform factory modules

- [x] 2.1 Add `unifiedstream_video::platform` with `video_sink() -> Box<dyn VideoSink>`, resolving to `linux` under `cfg(target_os = "linux")` and to `unsupported` otherwise
- [x] 2.2 Add `unifiedstream_audio::platform` with `audio_sink(Arc<JitterBuffer>) -> Box<dyn AudioSink>`, `audio_capture(FrameCallback) -> Box<dyn AudioCapture>`, and `audio_routing() -> Option<Box<dyn AudioRouting>>`, resolved the same way
- [x] 2.3 Implement the Linux side of both factories over the existing `PipeWireSource`, `PipeWireSpeakerSink`, and `V4l2LoopbackSink`, and over the `pactl` routing logic moved in from `app.rs` (task 3.4)
- [x] 2.4 Implement the unsupported side: `start` returns `Unavailable` with a message naming the platform, `push`/`push_frame` discard, `stop` and the counters are inert, `audio_routing()` returns `None`
- [x] 2.5 Verify the crate-level `cfg` gates and the target-gated `pipewire`/`v4l` dependencies in `Cargo.toml` still exclude every Linux-only item from a non-Linux build

## 3. Application layer

- [x] 3.1 Replace the unconditional `use` of `PipeWireSource`, `PipeWireSpeakerSink`, `SINK_NODE_ID`, `V4l2LoopbackSink`, and `MODPROBE_HINT` in `app.rs` with trait and factory imports only
- [x] 3.2 Change `MicSink.sink`, `CameraSink.sink`, and `SpeakerSource.sink` to boxed trait objects, and route the three `spawn_blocking` construction sites through the factories
- [x] 3.3 Feed `CameraStatus.hint` from the `SetupHint` carried by the video error and `CameraStatus.device` from `device_label()`; delete the `MODPROBE_HINT` special case
- [x] 3.4 Move `RoutingMemo`, `pactl`, `enable_routing`, `restore_routing`, and `sweep_stale_routing` out of `app.rs` into the Linux routing implementation; hold `Option<Box<dyn AudioRouting>>` in `AppState` and keep the memo path and file contents unchanged
- [x] 3.5 Change `SpeakerStatus.routed` to `Option<bool>`, emit `None` when the platform provides no routing, and return a state error from `set_speaker_routing` in that case
- [x] 3.6 Confirm `unifiedstream-net` needed no change and that `app.rs` contains no `cfg(target_os = …)` and no platform-specific string

## 4. Frontend

- [x] 4.1 Update `desktop/src/types.ts` for the `SetupHint` shape of `CameraStatus.hint` and the optional `SpeakerStatus.routed`
- [x] 4.2 Render the camera setup hint from the structured value (message always, command as copyable text only when present) and hide the route-system-audio control entirely when `routed` is absent
- [x] 4.3 Replace the hard-coded `"Streams to a v4l2loopback device"` fallback in `App.tsx` with the platform-supplied description, or with a fallback that names no platform
- [x] 4.4 Update `App.test.tsx` for the new `CameraStatus`/`SpeakerStatus` shapes, and add cases for an absent routing capability and for a setup hint with and without a command
- [x] 4.5 Confirm the rendered Linux strings are byte-identical to what shipped before, including the `sudo modprobe v4l2loopback …` command

## 5. Quality gates

- [x] 5.1 Convert the `rust` job in `.github/workflows/quality.yml` to a matrix over `ubuntu-24.04` and `windows-latest`; keep format, Clippy, and tests on the Linux leg, and run `cargo check --workspace` plus the same Clippy invocation on the Windows leg
- [x] 5.2 Update `CONTRIBUTING.md` with the non-Linux verification command and note which checks are expected to be unrunnable on a Linux workstation
- [x] 5.3 After the workflow has run once on the pull request, add the new Windows check to the required checks in branch protection
- [x] 5.4 Add `docs/design.windows.md` recording the Windows architecture decisions, and link it from the development section of `README.md`

## 6. Verification

- [x] 6.1 `cargo fmt --all -- --check`, `cargo clippy --workspace --lib --bins -- -D warnings`, and `cargo test --workspace` pass on Linux with no new warnings
- [x] 6.2 `cargo check --workspace` and Clippy pass for `x86_64-pc-windows-msvc` on the Windows runner
- [ ] 6.3 Manual Linux regression on CachyOS: camera to a v4l2 device with correct fps and device path shown; the `modprobe` hint appears verbatim with the module unloaded; microphone visible as a PipeWire source with a live level meter; speaker with the routing toggle, including default-output restore after a forced kill and the stale-takeover sweep on the next launch
- [x] 6.4 Manual Windows smoke run of the produced binary: the app starts, advertises, pairs with the phone, connects, and reports telemetry, and each of the three toggles fails with a visible unsupported message rather than crashing or appearing to succeed
- [ ] 6.5 Sync the delta specs into `openspec/specs/`, archive the change, and record the commands run in the pull request template
