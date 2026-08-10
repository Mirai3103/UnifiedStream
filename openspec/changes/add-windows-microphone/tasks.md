# Tasks — Windows Microphone over a Bundled VB-CABLE

## 1. Render-direction conversion

- [x] 1.1 Add `unifiedstream-audio/src/render_convert.rs` beside `convert.rs`, exporting a converter that takes the negotiated `AudioFormat` as its input and a target endpoint format (sample rate, channel count, float or integer samples) and emits interleaved samples in the endpoint's format
- [x] 1.2 Factor the numeric helpers `convert.rs` and the new module share — resampling over `rubato`, sample-type conversion with clamping — into a private module used by both, keeping two directional types rather than one with a direction flag (design decision 1)
- [x] 1.3 Implement mono-to-N channel distribution, since the microphone wire format is mono and the endpoint is almost always stereo; state in a comment which channels receive the signal and why
- [x] 1.4 Implement resampling from 48 kHz to the endpoint's rate, bypassing the resampler entirely when they are equal
- [x] 1.5 Implement integer-to-float conversion for float endpoints, and integer width conversion for integer endpoints, scaling so no representable input can wrap
- [x] 1.6 Unit-test the converter on both CI legs: exact output sample counts over a long 44.1 kHz run, the 48 kHz bypass producing sample-exact output, mono-to-stereo distribution, mono-to-5.1 distribution, and clamping at both rails
- [x] 1.7 Unit-test that a request for N endpoint frames always yields exactly N, including across a resampling ratio that does not divide evenly

## 2. Endpoint resolution

- [x] 2.0 **Measure what is obtainable, best-effort and not blocking:** for every VB-CABLE release available from VB-Audio — in practice the current one — enumerate its render endpoints and record adapter name, device description, channel count, jack subtype, and KS filter name. Older releases have no official source and are not worth fetching from third-party mirrors when the artifact is a kernel-mode driver, so the discrimination stays provisional rather than verified. Record what was measured and against which version
- [x] 2.1 Add an endpoint resolver to `platform/windows.rs` that enumerates render endpoints and builds a **candidate list**, matching the family on `PKEY_DeviceInterface_FriendlyName` (the adapter name, which the Sound control panel's rename does not touch) and corroborating with the hardware identifier `VBAudioVACWDM` where the driver-identity property is readable
- [x] 2.2 Reject candidates whose jack subtype is not `KSNODETYPE_SPEAKER`, and additionally reject any whose device format declares more than two channels, so `CABLE In 16ch` cannot be selected in place of `CABLE Input`; comment that both endpoints share adapter name, INF, driver version, topology filter, **channel count, and mix format**, that only the jack subtype and the KS pin index separate them, and that rendering into the wrong one is silent
- [x] 2.3 Refuse unless exactly one candidate survives — zero is "not installed", more than one is ambiguity — and never take the first of several
- [x] 2.4 Log every candidate considered and the reason each was rejected, so a wrong or empty result on a VB-CABLE version this project has not seen is diagnosable from a user's log
- [x] 2.5 Comment that the endpoint ID is **not** a stable identity — its GUID is minted per install, and the hardware identifier lives in a different property — so a later reader does not reintroduce a hardcoded endpoint ID; the ID is used only to re-open and to compare within one installation
- [x] 2.6 Return the endpoint's friendly name separately as the platform-supplied display label, so the UI can tell the user which device to select
- [x] 2.7 Distinguish the failure outcomes — no VB-CABLE endpoint present, several candidates and no unambiguous choice, endpoint present but disabled or unplugged, endpoint present but unopenable — each with its own message and, where one exists, its own remedy
- [x] 2.8 Unit-test the outcome classification against synthetic enumeration results, so every case is covered on a runner with no audio device
- [x] 2.9 Unit-test the candidate filter specifically against a synthetic two-render-endpoint driver modelled on the observed `CABLE Input` / `CABLE In 16ch` pair, in **both** enumeration orders, asserting the same endpoint is chosen either way
- [x] 2.10 Confirm the resolver never returns the default render endpoint as a fallback on any path, and add a test that a resolution failure is an error rather than a substitution

## 3. Windows audio sink

- [x] 3.1 Implement `audio_sink` in `platform/windows.rs`, replacing the `pub use super::unsupported::audio_sink` re-export and its W4 comment
- [x] 3.2 Structure the sink as a `JoinHandle` plus an `AtomicBool` stop flag, so it is `Send` with no COM object crossing a thread boundary, matching how `WasapiLoopback` and the PipeWire sink are built
- [x] 3.3 On the render thread: `CoInitializeEx(COINIT_MULTITHREADED)`, resolve the endpoint via task 2.1, read its mix format with `GetMixFormat`, and build the converter from task 1.1
- [x] 3.4 Initialize the render client in `AUDCLNT_SHAREMODE_SHARED` with `AUDCLNT_STREAMFLAGS_EVENTCALLBACK`, set the event handle, and wait on it rather than polling — with a comment stating that the capture polls only because loopback cannot use event callbacks, and that this is not an inconsistency to reconcile (design decision 3)
- [x] 3.5 Register the render thread with MMCSS for the lifetime of the stream, reusing the existing `MmcssThread` guard
- [x] 3.6 Implement the render loop: `GetCurrentPadding`, `GetBuffer` for the available frames, fill from the `JitterBuffer` through the converter, `ReleaseBuffer`
- [x] 3.7 On jitter buffer underrun, release the buffer with `AUDCLNT_BUFFERFLAGS_SILENT` rather than writing zeros by hand, and confirm playback resumes seamlessly when frames arrive again
- [x] 3.8 Report startup success or failure back through a channel with a bounded timeout, so an unresolvable endpoint refuses the stream rather than accepting one that never carries audio, matching `WasapiLoopback::start`
- [x] 3.9 On `AUDCLNT_E_DEVICE_INVALIDATED` mid-stream, fail the stream visibly with guidance; do **not** re-resolve and do **not** retry, and carry the reason in a comment so a later reader does not add the recovery path the capture has
- [x] 3.10 Make `stop` idempotent, joining the thread, releasing the client, and uninitializing COM on the thread that initialized it
- [x] 3.11 Confirm the application layer, `app.rs`, and `platform/mod.rs`'s support table need no change beyond the table's `audio_sink` row for Windows

## 4. Feedback loop guard

- [x] 4.1 Extend the endpoint resolver so the microphone's target device ID can be queried without starting a microphone stream
- [x] 4.2 In `WasapiLoopback::open`, compare the resolved default render endpoint against the microphone's target device ID and refuse the speaker stream when they match, with a message naming the device and stating that the system output must be changed
- [x] 4.3 Apply the same comparison on the capture's existing re-open path, so selecting the cable as the default output mid-stream stops the capture rather than transmitting what it finds there
- [x] 4.4 Confirm the check is inert on platforms whose virtual microphone is not also an output device, and that it costs nothing on Linux
- [x] 4.5 Unit-test the comparison against synthetic device IDs, covering match, non-match, and microphone-endpoint-absent

## 5. Attribution surface and its check

- [x] 5.1 Add the VB-CABLE notice to the About/Settings surface in `desktop/src/`: names VB-Audio, uses the word "donationware", and carries a reachable link to `vb-cable.com`
- [x] 5.2 Show the notice on every platform rather than gating it on Windows, so a platform-conditional render cannot hide it from the check
- [x] 5.3 Add a test in the frontend suite asserting the rendered About surface contains the component name, the author, the word "donationware", and an anchor whose href reaches `vb-cable.com` — asserting on the rendered output rather than on a source constant (design decision 5)
- [x] 5.4 Record VB-CABLE's redistribution obligations in the repository — component, grant, what it requires, and where the check lives — as the artifact the new `repository-quality-gates` requirement refers to
- [x] 5.5 Confirm the frontend CI area runs the new test and that removing the notice fails it

## 6. Documentation

- [x] 6.1 Mark W4 implemented in the phase plan of `docs/design.windows.md`
- [x] 6.2 Update the microphone feature description in `README.md` so it does not describe the PipeWire mechanism as the only one, matching how the camera and speaker entries were generalised
- [x] 6.3 Document in `docs/usage.md` that Windows presents the microphone as `CABLE Output (VB-Audio Virtual Cable)` and that this is the device to select in the consuming application
- [x] 6.4 Document in `docs/troubleshooting.md` the extra VB-Audio playback devices — `CABLE Input`, and `CABLE In 16ch` on releases that expose it — that they are expected and not a defect, and the feedback-loop conflict with the symptom a user would recognise
- [x] 6.5 Add the VB-CABLE prerequisite to `CONTRIBUTING.md` for developers running the microphone before the W5 installer exists

## 7. Verification

- [ ] 7.1 `cargo fmt --all -- --check`, `cargo clippy --workspace --lib --bins -- -D warnings`, and `cargo test --workspace` pass on Linux with no new warnings
- [x] 7.2 The same three commands pass on Windows, including the full test suite
- [x] 7.3 Frontend suite passes, including the new attribution test
- [x] 7.4 Manual Windows run: install VB-CABLE by hand, pair, enable the microphone, select `CABLE Output` in a conferencing application, and confirm the phone's audio is heard
- [x] 7.5 Manual Windows privacy check: confirm with the microphone streaming that nothing is audible on the PC's own speakers, and that changing the default output device mid-stream does not change where the audio goes
- [x] 7.6 Manual Windows underrun test: interrupt the phone's network briefly and confirm silence rather than repeated or stale audio, with playback resuming immediately and no accumulated latency
- [x] 7.7 Manual Windows rate test: set VB-CABLE's endpoint format to a rate other than 48 kHz in its control panel and confirm audio is correct and stays in sync over several minutes
- [ ] 7.8 Manual Windows endpoint-selection test: on a machine where both `CABLE Input` and `CABLE In 16ch` are present, confirm from the reported device label and the candidate log that `CABLE Input` was chosen, then rename `CABLE Input` in the Sound control panel and confirm resolution still succeeds
- [x] 7.9 Manual Windows absence test: uninstall VB-CABLE and confirm the microphone refuses with guidance naming what is missing, while the camera and speaker continue to work
- [x] 7.10 Manual Windows loop test: set the system output to `CABLE Input`, enable the speaker, and confirm it is refused with the conflict named rather than producing feedback
- [ ] 7.11 Manual Linux regression: the microphone path behaves exactly as before, including the virtual source appearing and being destroyed with the stream
- [ ] 7.12 Sync the delta specs into `openspec/specs/`, archive the change, and record the commands run in the pull request template
