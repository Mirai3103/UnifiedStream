## Why

`add-windows-speaker-capture` gave Windows the first of its three media features. The camera is the second, and it is phase W3 of the plan in `docs/design.windows.md`. With the speaker already shipping, a Windows build that also presents a virtual webcam is the point at which the port stops being partial in a way users notice — and decision 5's driver-signing question can then be settled against a working two-feature product rather than against a plan.

W3 is not one change. The camera is the only feature whose frames must leave the desktop's address space: a DirectShow filter runs inside the consuming application's process, so Zoom, Discord, and Chrome each load a DLL that must read frames the desktop produced. That crossing is a lock-free single-producer ring in shared memory, and it is the kind of code that fails silently — a torn read is a corrupt frame, not a crash, and a missed wakeup is a stall nobody attributes to the transport. Writing it at the same time as the first C++ in this repository, a new `windows/` tree, a new MSBuild toolchain, and a new CI area would mean debugging all of them against each other.

This change therefore builds and proves the transport alone, in Rust, where the existing test suite already runs on both CI legs. The C++ filter that consumes it follows in `add-windows-directshow-camera`, against a contract that is already specified and already tested.

Decision 3's own precondition is also settled here. It rejected `MFCreateVirtualCamera` because Windows 10 must stay supported, and the open questions required that be re-checked before the filter was written. It has been: the development machine for this project runs Windows 10, so Media Foundation is not merely a platform floor to raise but a path that cannot be exercised at all, and it would additionally require repackaging the application as MSIX. Decision 3 stands unchanged.

## What Changes

- A versioned frame-transport contract: a shared-memory ring with a fixed header, sequence-numbered slots, and an explicit format version. The desktop is the single producer; each loaded filter is an untrusted consumer that may appear, vanish, or die mid-read. The producer never waits for a consumer and never stalls the receive path, which is the same lossy-by-design policy the transport and jitter buffer already follow.
- **A version disagreement is a refusal, not a degraded read.** A filter built against a different transport version must decline rather than interpret a layout it does not understand, mirroring the `version_mismatch` rule `protocol.md` already applies to the wire protocol.
- Decoding stays on the desktop, in portable Rust. `decode_jpeg_to_i420` already serves the Linux sink; the ring carries decoded frames rather than JPEG so the harder-to-debug component does not also own a decoder. This keeps `decode_failures()` meaningful, since the trait makes the producer responsible for counting them.
- A Windows `VideoSink` that publishes into the ring, and a per-integration platform table for `unifiedstream-video`. The audio crate gained one in W1; the video crate is still a plain Linux/not-Linux split because it had only ever had one implementation.
- **Availability is probed, and its remedy is named.** Until the filter is registered there is no virtual camera, so `start` refuses and supplies the command that registers one — exactly the shape the missing `v4l2loopback` module already has on Linux, through the same `SetupHint` the UI already renders as a copyable box. This is what makes the change honest rather than a stub: the state it produces on a machine without the filter is a real, correct, actionable state.
- A consumer harness in the test suite, standing in for the filter, so the sequence protocol is exercised against a reader that races the writer rather than only against itself.

Not in scope, and deliberately: the DirectShow filter itself, the `windows/` tree, MSBuild, the native CI area, and the registration tooling. Those are `add-windows-directshow-camera`. Packaging and Authenticode signing remain out of scope for the whole of W3 — decision 6 puts WiX at W4, and an unsigned user-mode COM DLL registers and loads regardless, so signing buys nothing until there is an installer to distribute.

Until that follow-up change lands, Windows continues to report the camera as unavailable. What changes is that it now says what is missing and what would fix it, instead of reporting that the platform has no implementation at all.

## Capabilities

### New Capabilities

None. Every behavior here belongs to a capability that already exists.

### Modified Capabilities

- `camera-stream`: the desktop virtual camera requirement is written around one platform's mechanism — its scenarios assert a v4l2 device and a missing `v4l2loopback` module — even though its contract is already platform-neutral. Generalize the scenarios so the Linux specifics become the illustrative case rather than the rule, and add the case of a platform whose virtual-camera component is not yet installed. Add the two behaviors a cross-process camera makes user-visible and which nothing currently covers: that applications holding the virtual camera may come and go without disturbing the stream, and that a component which disagrees with the desktop about the frame format refuses rather than renders garbage.

## Impact

- `desktop/src-tauri/crates/unifiedstream-video`: a new portable ring module and its tests; a new `platform/windows.rs`; `platform/mod.rs` gains a table and stops gating `unsupported` on `not(target_os = "linux")`; `Cargo.toml` gains a target-gated `windows` dependency. `VideoSink`, `VideoFormat`, `SetupHint`, and `decode_jpeg_to_i420` are unchanged.
- `desktop/src/App.test.tsx`: the case pinning `"windows has no virtual camera implementation in this build"` and asserting no copyable command no longer describes Windows. It becomes the platform-with-a-remedy case, and a platform genuinely without an implementation still needs the no-command case covered.
- Application layer, frontend components, protocol, and Android: no change. The camera status already carries a device label and an optional setup hint, and the UI already renders both without knowing the platform.
- `openspec/specs/camera-stream/spec.md` and `openspec/specs/speaker-stream/spec.md`: both `## Purpose` lines name a Linux mechanism as the only one. A delta cannot reach a `Purpose`, so these are corrected directly when the specs are synced. The speaker line is already wrong today — `add-windows-speaker-capture` generalized its requirements and removed the PipeWire-specific one without being able to touch the summary above them.
- `docs/design.windows.md`: W3 recorded as two changes; the Windows 10 open question closed; decision 3 gains the frame-transport consequence.
- `.github/workflows/quality.yml`: no change. The transport is portable and its tests run on both existing legs; the native build area arrives with the C++ that needs it.
