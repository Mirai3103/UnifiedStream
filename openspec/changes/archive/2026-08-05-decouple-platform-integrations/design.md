## Context

The desktop is a Tauri 2 application over a three-crate Rust workspace. `unifiedstream-net` carries discovery, the control channel, and the UDP media transport with no operating-system dependency. `unifiedstream-audio` and `unifiedstream-video` each pair portable logic (jitter buffering, frame chunking, MJPEG decode) with a Linux integration module gated on `cfg(target_os = "linux")` and a target-gated crate dependency. Each already declares a trait — `AudioSink`, `AudioCapture`, `VideoSink` — whose documentation names a future Windows implementation as the reason it exists.

The application layer did not adopt those traits. `app.rs` is 1616 lines and holds every coupling in the workspace: unconditional imports of four Linux-only items, three concrete-typed sink fields, three direct constructor calls, two calls to methods that exist only on concrete types, a `pactl` subprocess helper with no trait at all, and a Linux remediation string forwarded to the UI. The result is that the seam is a comment rather than a compiler-enforced boundary.

This change is a refactor. It adds no media capability and changes no Linux behavior. Its purpose is to make the boundary real and to make CI keep it real, so that the Windows work recorded in `docs/design.windows.md` is additive.

## Goals / Non-Goals

**Goals:**

- The application layer names no platform-specific type. Concrete platform types are referenced in exactly one module per crate.
- Every supported target compiles the full workspace, including the Tauri binary, without a platform implementation being present.
- A target with no implementation degrades to a visible, honest unsupported state — not a build failure, not a panic, and not a silent no-op that looks like success.
- Platform-specific user guidance and platform-specific device identifiers originate in the platform layer.
- Optional platform capabilities — system audio routing being the first — are expressible as absent rather than as present-and-broken.
- CI compiles and lints a non-Linux target on every pull request.

**Non-Goals:**

- Any Windows or macOS media implementation. The fallback implementations in this change deliberately do nothing.
- Any device driver, DirectShow filter, installer, or code-signing work.
- Protocol, wire format, capability token, or transport changes.
- Android changes.
- Restructuring `app.rs` beyond what the seam requires. It stays one file; splitting it is a separate concern.
- Changing what Linux users see, other than the mechanism that produces it.

## Decisions

### 1. Trait objects in the application layer, concrete types in one module per crate

`MicSink.sink`, `CameraSink.sink`, and `SpeakerSource.sink` become `Option<Box<dyn AudioSink>>`, `Option<Box<dyn VideoSink>>`, and `Option<Box<dyn AudioCapture>>`. The traits are already `Send`, which is what the `spawn_blocking` construction and teardown paths require.

Static dispatch through a type alias (`type PlatformAudioSink = …` resolved by `cfg`) was considered. It avoids allocation and keeps monomorphized calls, and the per-frame cost of dynamic dispatch here is irrelevant next to a PipeWire round trip or a JPEG decode. It was rejected because a type alias does not force the application layer through the trait: code can still call an inherent method that only one platform has, which is precisely the failure mode being fixed. Boxing makes the trait the only reachable surface, and the compiler enforces it.

### 2. Construction moves behind platform factory functions

The traits describe a lifecycle (`start`, `push`/`push_frame`, `stop`) but not construction, and construction is where the remaining coupling hides. `PipeWireSource::new` takes an `Arc<JitterBuffer>`, `PipeWireSpeakerSink::new` takes a boxed frame callback, and `V4l2LoopbackSink::new` takes nothing. The application layer cannot call any of them without naming the type.

Each crate therefore exposes factory functions that return boxed trait objects and take only portable arguments:

```
unifiedstream_audio::platform::audio_sink(buffer: Arc<JitterBuffer>) -> Box<dyn AudioSink>
unifiedstream_audio::platform::audio_capture(on_frame: FrameCallback) -> Box<dyn AudioCapture>
unifiedstream_audio::platform::audio_routing() -> Option<Box<dyn AudioRouting>>
unifiedstream_video::platform::video_sink() -> Box<dyn VideoSink>
```

Each `platform` module is `#[cfg(target_os = "linux")] mod linux;` plus `#[cfg(not(target_os = "linux"))] mod unsupported;`, re-exporting whichever applies. This is the single place per crate where a concrete type is named.

A trait-object factory registry resolved at runtime was considered, so that one binary could carry several backends — relevant later if Linux needs both PipeWire and PulseAudio. It was rejected as premature: it adds indirection now for a second Linux backend nobody has asked for, and moving from compile-time to runtime selection later is a local change inside these `platform` modules.

### 3. Unsupported implementations rather than compile-time absence

For every target that is not Linux, the fallback module provides types whose `start` returns `AudioError::Unavailable` / `VideoError::Unavailable` with a message naming the platform, whose `push`/`push_frame` discard, and whose `stop` is a no-op. `audio_routing()` returns `None`.

This is what lets the whole workspace — including the Tauri binary — compile on Windows in this change, which in turn is what lets CI verify the seam. The alternative, gating the media features out of `app.rs` with `cfg`, would reintroduce exactly the conditional compilation in the application layer that this change removes, and would leave the Windows build path untested until the first Windows implementation lands.

The unsupported path must be visible, not silent. `start` returning an error means the existing refusal handling already reports it: a microphone or camera `stream_start` is refused with `internal`, and the desktop UI shows the sink as unavailable with the platform's message. A running Windows build therefore discovers a phone, pairs, connects, and reports telemetry, with all three media toggles failing honestly. That is a useful integration target for the Windows changes rather than dead code.

### 4. `AudioRouting` is an optional capability, not a required trait

Routing is the one place where the Linux design does not merely need a different implementation — it needs to not exist. On Linux the desktop creates a virtual sink and makes it the system default output, which requires remembering and restoring the user's device and repairing a stale takeover after an unclean exit. On a platform that captures the existing output device directly, there is nothing to route, nothing to remember, and nothing to repair.

Modelling this as a trait whose Windows implementation returns "unsupported" would be wrong: the UI would show a routing toggle that always fails, and `SpeakerStatus.routed` would carry a meaningless `false`. Modelling it as `Option<Box<dyn AudioRouting>>` lets absence propagate: `SpeakerStatus.routed` becomes `Option<bool>`, `None` means the platform has no such concept, and the frontend omits the control entirely.

The trait covers the whole Linux mechanism, not just the switch, because the memo file and the startup sweep are equally Linux-specific:

```
trait AudioRouting: Send {
    async fn enable(&self) -> Result<(), String>;
    async fn restore(&self);
    async fn sweep_stale(&self);
}
```

`RoutingMemo`, `enable_routing`, `restore_routing`, `sweep_stale_routing`, and the `pactl` subprocess helper move out of `app.rs` into the Linux implementation. `app.rs` keeps the `set_speaker_routing` command but resolves it against the `Option`, returning a state error when the platform has none.

### 5. Setup guidance and device identity are platform-supplied values

`MODPROBE_HINT` stops being a constant the application layer reads and becomes part of what the Linux `VideoSink` reports when it fails. A structured value crosses the seam:

```
struct SetupHint {
    message: String,           // what is wrong, in user terms
    command: Option<String>,   // a copyable command that fixes it, when one exists
}
```

`CameraStatus.hint` carries this instead of a bare string, and `CameraStatus.device` is fed by a `device_label()` accessor on `VideoSink` returning an opaque platform string — `/dev/video10` on Linux, whatever identifies the device elsewhere.

The frontend is close to platform-neutral already but not fully: `App.tsx` renders the literal fallback "Streams to a v4l2loopback device" when `camera.device` is absent. That string moves behind the same seam — the platform's idle description becomes part of what the sink reports, or the fallback becomes generic — so the user interface names no platform.

Keeping the Linux hint in the application layer behind a `cfg` was considered and rejected for the same reason as decision 3.

### 6. `decode_failures` joins the `VideoSink` trait

`app.rs` reads `frames_written()` (on the trait) and `decode_failures()` (not on the trait) from the same concrete sink in the camera stats task. The counter is part of the contract the UI already depends on — a stream of undecodable frames must read as 0 fps with a rising failure count — so it belongs on the trait rather than being an accident of the v4l2 implementation.

### 7. CI verifies a non-Linux target on a Windows runner, not by cross-checking

Two ways to prove the workspace still compiles for Windows were considered.

Cross-checking from the existing Ubuntu runner (`rustup target add x86_64-pc-windows-msvc` then `cargo check --target …`) needs no new runner and no new required check. It was rejected because `tauri-build` does host-side work for Windows targets, including resource generation, and a cross-check that fails for toolchain reasons rather than code reasons would be a permanently noisy gate.

The `rust` job therefore becomes a matrix over `ubuntu-24.04` and `windows-latest`. The Linux leg is unchanged: `cargo fmt --check`, `cargo clippy --workspace --lib --bins -D warnings`, `cargo test --workspace`. The Windows leg runs `cargo check --workspace` and the same Clippy invocation, and does not run tests — the workspace's integration tests exercise PipeWire and v4l2 loopback behavior that has no Windows meaning, and unit tests over the portable code are already covered by the Linux leg.

The consequence is honest and must be handled rather than hidden: a matrix produces two distinct GitHub checks, so branch protection gains a required check. `CONTRIBUTING.md` and the `repository-quality-gates` capability are updated to match.

### 8. Windows architecture decisions are recorded outside this change

The decisions that make this refactor the right shape — minimal kernel-mode surface, a user-mode DirectShow filter for the camera, an own audio driver for the microphone because no user-mode API can create an audio input endpoint, WASAPI loopback for the speaker so no routing concept is needed, and the driver-signing options — span several future changes. An OpenSpec change's `design.md` is archived with its change, so recording them here would bury them.

They go in `docs/design.windows.md`, a living document that the later Windows proposals reference and amend. This change's job is to make the seam accommodate those decisions; that document explains why the seam has the shape it does, particularly why `AudioRouting` is optional and why the camera trait deliberately does not assume a device node.

## Risks / Trade-offs

- **[The refactor silently changes Linux behavior]** → The change adds no media capability, so every existing test is a regression test. Beyond the workspace suite, the manual Linux verification from the earlier media changes is repeated: camera to a v4l2 device, microphone as a PipeWire source, speaker with the routing toggle including a forced-kill restore, and confirmation that the `modprobe` hint still appears verbatim when the module is absent.

- **[Boxing changes the lifetime or threading behavior of the sinks]** → The sinks are already moved into `spawn_blocking` closures and dropped on teardown, and the traits are already `Send`. The boxed forms are constructed and destroyed at the same points. The speaker frame callback keeps crossing from the PipeWire thread through the same bounded channel.

- **[The unsupported fallback reads as a working build and gets shipped]** → No release workflow targets Windows, and `README.md` states that Windows is not part of the release. The fallback's error message names the platform explicitly so a build run outside CI is unambiguous.

- **[The new required check blocks merges before branch protection is updated]** → Branch protection is updated in the same pull request, and the task list orders the workflow change before the protection change so the check has a run history to select from.

- **[The `Option<bool>` routing field breaks the frontend at runtime rather than at build time]** → `desktop/src/types.ts` is updated in the same change and the frontend build is a required check, so a mismatched shape fails CI.

- **[The seam degrades again once Windows work begins]** → This is what the Windows CI leg exists to prevent. A new Linux-only call in `app.rs` fails the Windows leg at compile time, which is a stronger guarantee than the review discipline that allowed the current coupling.

## Migration Plan

Single pull request, no staged rollout, no compatibility window. The change is internal to the desktop application: no persisted format changes except that the routing memo is written and read by the Linux routing implementation at the same path with the same contents, and no peer observes a difference. A Linux user upgrading across this change sees no behavioral difference and needs to do nothing.

## Open Questions

None blocking. The Windows implementation choices are settled in `docs/design.windows.md`; the one genuinely open item recorded there — how driver signing is funded — does not affect this change, because nothing in it depends on a driver existing.
