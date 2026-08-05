# Decouple Platform Integrations

## Why

The library crates are already prepared for a second platform: `unifiedstream-net` has no operating-system dependency at all, `unifiedstream-video`'s MJPEG decoder and `unifiedstream-audio`'s jitter buffer are pure Rust, the PipeWire and v4l2 modules are behind `cfg(target_os = "linux")`, and their crate dependencies are target-gated in `Cargo.toml`. The crates even declare the seam explicitly — `AudioSink`, `AudioCapture`, and `VideoSink` are documented as what "keeps a future Windows sink from touching the receive path".

That seam is not real yet. `desktop/src-tauri/src/app.rs` imports `PipeWireSource`, `PipeWireSpeakerSink`, `SINK_NODE_ID`, and `V4l2LoopbackSink` unconditionally, stores them as concrete field types rather than trait objects, constructs them directly, and calls methods that exist only on the concrete types (`device_path`, `decode_failures`). A non-Linux build therefore fails at the import lines, before any missing implementation is even reached. The traits are declared but nothing depends on them, so nothing prevents the next change from adding more Linux-only calls to the application layer.

Two Linux implementation details have also leaked past the seam into contracts that the UI and the Android peer observe. `CameraStatus.hint` carries the literal `sudo modprobe v4l2loopback …` command and `CameraStatus.device` carries a `/dev/videoN` path, neither of which has a counterpart on another platform. `SpeakerStatus.routed`, the `set_speaker_routing` command, and the persisted `RoutingMemo` encode the PulseAudio/PipeWire approach of hijacking the system default output — a technique that platforms with loopback capture do not need and cannot express.

Fixing this before any Windows feature work means the Windows changes are additive rather than a rewrite of the application layer, and CI proves the seam holds instead of relying on review discipline.

## What Changes

- `app.rs` depends only on the media traits. Sink and capture fields become boxed trait objects, and the concrete Linux types are named in exactly one place.
- A platform factory selects implementations at compile time: `linux` provides the PipeWire and v4l2 types, and a fallback module compiled for every other target provides implementations that report the feature as unsupported. The desktop app therefore **compiles and runs** on Windows after this change, with all three toggles reporting that the platform has no implementation yet, rather than failing to build.
- The traits are completed so the application layer never needs a concrete type: `VideoSink` gains `decode_failures` and a platform-neutral device label, and construction moves behind factory functions so `AudioCapture`'s frame callback no longer forces `app.rs` to know the concrete constructor.
- System audio routing becomes an optional platform capability behind a new `AudioRouting` trait. Platforms that route expose the control; platforms that do not report it as absent, and the UI hides the control rather than showing a dead toggle.
- Linux-specific remediation text stops being a hard-coded constant in the application layer. `CameraStatus` carries a platform-supplied setup hint (message plus an optional copyable command) and a platform-supplied device label, so the same UI renders `sudo modprobe v4l2loopback …` on Linux and something else elsewhere. The one Linux string in the frontend — the fallback "Streams to a v4l2loopback device" in `App.tsx` — moves behind the same seam.
- The Rust quality check runs on a matrix so a non-Linux target is compiled and linted on every pull request. The required-check list and branch protection grow by one entry.
- `docs/design.windows.md` records the Windows architecture decisions this refactor is designed to accommodate, so the later Windows changes inherit the reasoning instead of re-deriving it.

Linux behavior is unchanged. No protocol message, wire format, capability token, or Android source file is touched.

## Capabilities

### New Capabilities

- `platform-integration`: the portability contract for platform-specific media integration — the application layer depends only on traits, every supported target compiles, targets without an implementation degrade to a visible unsupported state instead of a build failure or a silent no-op, and platform-specific user guidance is supplied by the platform layer rather than embedded in the application layer.

### Modified Capabilities

- `camera-stream`: the desktop virtual camera requirement is restated so that setup guidance is platform-supplied rather than specified as the v4l2loopback module command, and the device identifier shown in the UI is a platform-supplied label rather than a `/dev/videoN` path. Linux behavior and the exact command shown on Linux are unchanged.
- `speaker-stream`: system audio routing is restated as an optional platform capability. Where a platform provides it the existing behavior (take over the default output, restore it on stop, repair a stale takeover on startup) is unchanged; where a platform does not, the control is absent rather than present and failing.
- `application-interface`: feature parity is restated so that an action backed by an optional platform capability is retained wherever the platform provides it and omitted — not shown disabled or failing — where the platform does not, and so that the interface names no specific platform in its own text.
- `repository-quality-gates`: the Rust quality area verifies more than one target platform and reports one check per verified platform.

### Modified Specs (non-capability)

<!-- `openspec/specs/protocol.md` is untouched: the wire format, stream identifiers,
     capability tokens, and lifecycle messages are already platform-neutral, and the
     phone never learns which operating system the desktop runs. -->

None.

## Impact

- **`desktop/src-tauri/src/app.rs`**: the only file with substantial change. Imports become trait-only; `MicSink`, `CameraSink`, and `SpeakerSource` hold boxed trait objects; the three `spawn_blocking` construction sites call factory functions; the `pactl` helpers and `RoutingMemo` move out of the application layer and behind `AudioRouting`; `MODPROBE_HINT` and `device_path` are replaced by platform-supplied values.
- **`unifiedstream-audio`**: `AudioRouting` trait added; factory functions added; PipeWire routing implementation moved in from `app.rs`; an unsupported fallback module added for non-Linux targets. The PipeWire modules themselves are unchanged apart from implementing the completed traits.
- **`unifiedstream-video`**: `VideoSink` gains `decode_failures` and a device-label accessor; `MODPROBE_HINT` becomes the Linux implementation's setup hint rather than a constant the application layer reads; an unsupported fallback module added.
- **`unifiedstream-net`**: unchanged. It has no operating-system dependency and needs none.
- **Frontend** (`desktop/src/`): `CameraStatus` and `SpeakerStatus` shapes change in `types.ts`; the camera hint renders from a structured value, the routing control renders conditionally, and the hard-coded "Streams to a v4l2loopback device" fallback in `App.tsx` is replaced by a platform-supplied or generic description. `App.test.tsx` is updated for the new shapes. No Linux-visible behavior change.
- **CI** (`.github/workflows/quality.yml`): the `rust` job becomes a matrix over a Linux runner and a Windows runner. The Windows leg runs `cargo check` and Clippy; the Linux leg keeps the full format, lint, and test set. Branch protection must add the new required check.
- **Docs**: `docs/design.windows.md` added.
- **Dependencies**: none added. The unsupported fallback implementations are plain Rust.
- **Android**: none.
