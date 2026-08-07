## Why

`decouple-platform-integrations` left Windows compiling, running, and reporting all three media features as unavailable. This change fills in the first of them. It is phase W2 of the plan in `docs/design.windows.md`, and it is the phase that turns a Windows build from a proof that the seam holds into something a user would run: a wireless speaker, with the PC's audio playing on the phone.

The speaker is first because it is the only feature that needs nothing bought, signed, or written in C++. Decision 2 of the Windows design captures system audio with `IAudioClient` loopback on the existing default render endpoint — no driver, no virtual device, and no change to the device the user selected. Everything here is Rust in one platform module, so it can ship while the driver-signing question in decision 5 is still open.

## What Changes

- A Windows `AudioCapture` implementation capturing the default render endpoint through `IAudioClient` with `AUDCLNT_STREAMFLAGS_LOOPBACK`, on its own COM-initialized thread, feeding the existing portable `FrameChunker`.
- A silent keep-alive render client on the same endpoint, held open for the lifetime of the capture. Loopback on an idle endpoint delivers no packets at all rather than silence, so without this the phone's jitter buffer starves whenever the PC is quiet.
- Endpoint changes are followed rather than survived: `IMMNotificationClient` reports a new default output and `AUDCLNT_E_DEVICE_INVALIDATED` reports a disappearing one, and both re-open the capture and the keep-alive on the current endpoint through one recovery path.
- Mix-format conversion on the desktop. `GetMixFormat` decides the endpoint's format, commonly 32-bit float and not always 48 kHz; the wire format stays PCM S16LE 48 kHz. Sample conversion and resampling therefore happen in the desktop capture, and neither the protocol nor the Android application changes.
- **The platform factory becomes per-integration rather than per-platform.** Windows gets a real `audio_capture`, keeps `audio_routing` at `None`, and takes `audio_sink` from the unsupported fallback until W4 supplies the driver. Today a platform is wholly implemented or wholly unimplemented; Windows will be partial through W3 and this is the first change to need it.
- The Windows CI leg runs the tests that are meaningful on it. Its current test exemption is written against a workspace whose only integration tests need PipeWire and v4l2, and this change invalidates that reasoning by adding Windows code worth testing.

Not in scope, and deliberately: packaging. `tauri.conf.json` still bundles `appimage` and `deb` only, and there is no Windows release workflow. This change is verified by running the built binary on a Windows machine, and producing an installer for anyone else is separate work — decision 6 puts WiX at W4 for the driver's sake, and settling how Windows artifacts are produced is its own change with its own toolchain and its own review.

## Capabilities

### New Capabilities

None. Every behavior here belongs to a capability that already exists.

### Modified Capabilities

- `speaker-stream`: the desktop capture source is specified as a PipeWire virtual sink, which describes one platform's mechanism rather than the behavior. Generalize it to the platform's system audio source, and add the three behaviors a loopback-style capture makes user-visible: audio continues to reach the phone while the PC is silent, capture follows the system output device when it changes, and the stream carries the negotiated format regardless of the rate the endpoint runs at.
- `platform-integration`: the portability contract assumes a platform either has media implementations or has none. State that support is per integration, so a platform may implement one and report another as unavailable, and that the unavailable one behaves exactly as it does on a platform with no implementations at all.
- `repository-quality-gates`: the non-Linux gate is specified as compilation and lints, with tests excluded because they needed a Linux subsystem. Require the non-Linux leg to also run the tests that platform can run.

## Impact

- `desktop/src-tauri/crates/unifiedstream-audio`: new `platform/windows.rs`; `platform/mod.rs` gains a third branch and stops gating `unsupported` on `not(target_os = "linux")`; `Cargo.toml` gains target-gated `windows` and resampling dependencies. `FrameChunker`, `JitterBuffer`, and the three traits are unchanged.
- Application layer, frontend, protocol, and Android: no change. `SpeakerStatus.routed` stays `None` on Windows, so the routing control stays hidden exactly as it is today.
- `.github/workflows/quality.yml`: the Windows leg gains a test step.
- `docs/design.windows.md`: W2 marked implemented; decision 2 gains the mix-format consequence, which the document does not currently mention.
