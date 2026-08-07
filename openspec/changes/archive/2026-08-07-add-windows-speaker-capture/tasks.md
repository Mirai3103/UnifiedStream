# Tasks — Windows Speaker over WASAPI Loopback

## 1. Portable conversion module

- [x] 1.1 Add `rubato` as an unconditional dependency of `unifiedstream-audio`, with a comment stating why it is not target-gated (decision 5: the conversion must be covered by the test leg that runs)
- [x] 1.2 Add `unifiedstream-audio/src/convert.rs` beside `capture.rs`, exporting a converter that takes a source format (sample rate, channel count, float or integer samples) and the target `AudioFormat`, and emits interleaved `i16` at the target rate
- [x] 1.3 Implement channel mapping: mono upmixed by duplication, stereo passed through, more than two channels downmixed by a fixed matrix, applied before resampling so the resampler sees the fewest samples
- [x] 1.4 Implement resampling to the target rate over `rubato`, and bypass the resampler entirely when the source rate already equals the target rate
- [x] 1.5 Implement float-to-integer conversion last, scaling and clamping so out-of-range float samples cannot wrap
- [x] 1.6 Unit-test the converter: exact output sample counts over a long 44.1 kHz run (the drift case), the 48 kHz bypass producing sample-exact output, mono upmix, 5.1 downmix, channel interleaving preserved, and clamping at both rails
- [x] 1.7 Confirm the converter composes with `FrameChunker(1920)` to emit whole 20 ms stereo frames with no accumulating remainder, as a test

## 2. Per-integration platform resolution

- [x] 2.1 Make `platform/unsupported` compile on every target rather than only under `cfg(not(target_os = "linux"))`, with an explicit dead-code allowance on Linux carrying the reason
- [x] 2.2 Add the `windows` branch to `platform/mod.rs` so the three factories resolve per integration, keeping one visible platform table in that module
- [x] 2.3 Add `platform/windows.rs` re-exporting `audio_sink` from `unsupported` and returning `None` from `audio_routing`, with a comment naming W4 as what replaces the sink re-export
- [x] 2.4 Confirm the application layer, `app.rs`, and the frontend need no change, and that `SpeakerStatus.routed` is still `None` on Windows

## 3. Windows capture implementation

- [x] 3.1 Add the target-gated `windows` crate dependency with the feature set for `Win32_Media_Audio`, `Win32_System_Com`, `Win32_Foundation`, and threading
- [x] 3.2 Implement the `AudioCapture` struct holding only a `JoinHandle` and an `AtomicBool` stop flag, so it is `Send` with no COM object crossing a thread boundary
- [x] 3.3 On the capture thread: `CoInitializeEx(COINIT_MULTITHREADED)`, resolve the default render endpoint through `IMMDeviceEnumerator`, and read its mix format with `GetMixFormat`
- [x] 3.4 Initialize the loopback client with `AUDCLNT_SHAREMODE_SHARED | AUDCLNT_STREAMFLAGS_LOOPBACK` and build the converter from the mix format to the negotiated `AudioFormat`
- [x] 3.5 Open the silent keep-alive render client on the same endpoint, fill its buffer with silence, start it, and top it up on each poll pass for the lifetime of the capture
- [x] 3.6 Implement the poll loop: `GetNextPacketSize` / `GetBuffer` / `ReleaseBuffer`, sleeping half the device period, honouring `AUDCLNT_BUFFERFLAGS_SILENT` by writing zeros rather than reading the buffer
- [x] 3.7 Feed converted samples through `FrameChunker` into the `FrameCallback`, matching the lifecycle the PipeWire capture presents to the application layer
- [x] 3.8 Register an `IMMNotificationClient` whose `OnDefaultDeviceChanged` sets one atomic and returns, touching no COM object and taking no lock the capture thread holds
- [x] 3.9 Implement the single recovery path: on the notification flag or `AUDCLNT_E_DEVICE_INVALIDATED` from either client, tear down both clients, re-resolve the default endpoint, rebuild the converter, and clear the chunker
- [x] 3.10 Return `AudioError` from `start` when no endpoint can be resolved, and stop the stream with a speaker error when recovery fails on every endpoint
- [x] 3.11 Make `stop` idempotent and confirm it joins the thread, releases both clients, and uninitializes COM

## 4. Quality gates

- [x] 4.1 Remove the `if: matrix.os == 'ubuntu-24.04'` condition from the Rust test step in `.github/workflows/quality.yml` so the suite runs on both legs
- [x] 4.2 Replace the step's comment, which claims the tests exercise PipeWire and v4l2 behavior; state instead that the suite is portable and that nothing added to it may require an audio endpoint, since the Windows runners have none
- [x] 4.3 Update `CONTRIBUTING.md` for what is now runnable on a Windows workstation

## 5. Documentation

- [x] 5.1 Mark W2 implemented in the phase plan of `docs/design.windows.md`
- [x] 5.2 Add the mix-format consequence to decision 2 of that document — `GetMixFormat` decides the endpoint's format, and conversion to the negotiated wire format is the desktop's job — alongside the keep-alive and device-change consequences already recorded
- [x] 5.3 Update the speaker feature description in `README.md` so it does not describe the Linux mechanism as the only one

## 6. Verification

- [x] 6.1 `cargo fmt --all -- --check`, `cargo clippy --workspace --lib --bins -- -D warnings`, and `cargo test --workspace` pass on Linux with no new warnings
- [x] 6.2 The same three commands pass on Windows, including the full test suite
- [x] 6.3 Manual Windows run: pair with the phone, enable the speaker, and confirm PC audio plays on the phone while it continues to play on the PC's own speakers, with no new device in the output list and the routing control absent from the UI
- [x] 6.4 Manual Windows silence test: leave the PC silent for a minute with the stream active and confirm the phone reports no underrun and audio resumes immediately when playback starts, with no accumulated latency
- [x] 6.5 Manual Windows device-change test: switch the default output device mid-stream and confirm capture follows without the stream stopping; unplug a headset mid-stream and confirm the same
- [x] 6.6 Manual Windows rate test: set the default output to 44.1 kHz in the Sound control panel and confirm audio is correct and stays in sync over several minutes
- [x] 6.7 Manual Linux regression: the speaker path behaves exactly as before, including the routing toggle, the default-output restore after a forced kill, and the stale-takeover sweep on the next launch
- [x] 6.8 Sync the delta specs into `openspec/specs/`, archive the change, and record the commands run in the pull request template
