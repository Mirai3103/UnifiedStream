# Tasks — Windows Camera Frame Transport

## 1. Portable frame-transport module

- [x] 1.1 Add `unifiedstream-video/src/transport.rs` beside `decode.rs`, holding the whole contract over a plain byte slice with no Win32 in it (decision 9: a seqlock fails silently, so it must be covered by the test leg that runs on every platform)
- [x] 1.2 Define `#[repr(C)] RingHeader` and `#[repr(C)] SlotHeader` exactly as decision 2 lays them out, with explicitly sized integers and no pointer-width field, and document beside each why the field exists rather than what it holds
- [x] 1.3 Add compile-time assertions on `size_of`, `align_of`, and every field offset of both structures, so a layout change that would break the C++ consumer fails the Rust build first
- [x] 1.4 Fix the transport version at `1`, the magic at `b"USVC"`, the slot count at 4, and the allocation at the application's existing `1920 × 1080` cap, with the arithmetic for `slot_bytes` page-aligned as decision 5 requires
- [x] 1.5 **Declare the filter's CLSID `{6D8DD393-D871-4498-A24F-4AFFEACFC106}` in this module**, with a comment stating that it is the contract's identity, that `add-windows-directshow-camera` registers this exact value, and that a mismatch is caught by neither build (decision 7)
- [x] 1.6 Implement the producer half of the sequence protocol: claim the slot, mark it odd, copy, mark it even, then publish `latest`, with the `Release` orderings decision 3 specifies and a comment naming what each one pairs with
- [x] 1.7 Implement the consumer half — read `latest`, reject an odd sequence, copy, re-read and compare — as ordinary library code rather than test-only code, since it is the normative statement of what the C++ filter must do
- [x] 1.8 Implement geometry and lifetime signalling: `generation` bumped on geometry change and on stream start and stop, `flags` bit 0 for a running stream, and `heartbeat_ms` advanced on publish and at least every 250 ms while idle (decision 6)
- [x] 1.9 Implement the version and magic checks a consumer performs, returning distinguishable outcomes for "not our mapping", "ours but a version I do not understand", and "usable", because decision 2 requires the three to produce different messages
- [x] 1.10 Unit-test the protocol: a published frame round-trips byte-identically; an odd sequence is refused; a slot overwritten mid-copy is detected and discarded; being lapped costs exactly one frame and not the stream; a geometry change moves `generation`; a mismatched version is refused rather than read
- [x] 1.11 Test the protocol under a reader thread deliberately racing a writer thread, asserting every frame the consumer accepts is byte-identical to one the producer published (the risk in decision 3 is a wrong frame, not a missing one)
- [x] 1.12 Test the liveness state machine of decision 6 directly: running with frames, running while idle, cleanly stopped, and a heartbeat stale beyond two seconds

## 2. Windows mapping and availability probe

- [x] 2.1 Add the target-gated `windows` dependency to `unifiedstream-video/Cargo.toml`, pinned to the same `0.61` line the audio crate uses so the build carries one copy of the Windows metadata, with features for the file mapping, named synchronisation objects, security descriptors, and the registry, each non-obvious one carrying the reason it is present
- [x] 2.2 Create the section as `Local\UnifiedStream.Camera` with the exact descriptor decision 4 specifies, `D:(A;;GA;;;OW)(A;;GR;;;AU)(A;;GR;;;AC)S:(ML;;NW;;;LW)`, converted with `ConvertStringSecurityDescriptorToSecurityDescriptorW`; the mandatory label is what lets a sandboxed consuming process open a mapping a Medium-integrity desktop created, and omitting it fails only under a sandbox. The owner ACE is `OW` and not the `CO` originally written down: `CREATOR OWNER` resolves only on inheritance, so applied directly it grants nobody map-write and the reclaim path in 2.3 fails with `ACCESS_DENIED` — cover it with a test that opens the section twice, since one open cannot see it
- [x] 2.3 Take `Local\UnifiedStream.Camera.Init` around header creation and re-initialisation only, and handle `CreateFileMappingW` returning an existing section a surviving consumer still holds: validate, reclaim, and re-initialise rather than assume the mapping is new (decision 4)
- [x] 2.4 Wrap the mapping, the view, and the mutex in RAII guards, each with a doc comment stating why it is a guard rather than a bare call, matching the discipline of `unifiedstream-audio/src/platform/windows.rs`
- [x] 2.5 Implement the registry probe: read the filter CLSID's `InprocServer32` under `HKEY_CLASSES_ROOT` in **both** the 64-bit and the `WOW6432Node` views and confirm each named DLL exists on disk (decision 7)
- [x] 2.6 Map the probe's outcomes to distinct `SetupHint`s — absent, present for one architecture only, and present but a version this desktop cannot talk to — each naming the command that resolves it, so the UI's copyable box is populated exactly as the `v4l2loopback` module-load hint populates it today. Each command must be a single invocation with no shell syntax: expand `%ProgramFiles%` and `%SystemRoot%` in the desktop rather than leaving them for a shell, and never join two registrations with `&&` — `%VAR%` is literal in PowerShell and `&&` is a parse error in Windows PowerShell 5.1, so either one turns the guidance into an error that looks like the guidance being wrong. `Absent` names the 64-bit registration alone and the probe's `OneArchitecture` state names the other
- [x] 2.7 Test what can be tested on the Windows leg without a device: a section created and opened by name from an independent mapping in the same process, the init mutex serialising two initialisers, and the probe returning absent on a runner where nothing is registered
- [x] 2.8 Add `unifiedstream-video/examples/ring_smoke.rs`, following the precedent of `unifiedstream-audio/examples/pw_smoke.rs`: drive the producer against the consumer half over the real named section, reporting frames published, accepted, and rejected as torn. Without it the producer path has no manual exercise at all, because `start` refuses on every machine until change B installs a filter

## 3. Sink implementation and platform resolution

- [x] 3.1 Make `platform/unsupported` compile on every target rather than only under `cfg(not(target_os = "linux"))`, with an explicit dead-code allowance carrying the reason, mirroring what W2 did in the audio crate
- [x] 3.2 Add the `windows` branch and a per-integration table to `platform/mod.rs`, keeping one visible platform table in that module as the audio crate does
- [x] 3.3 Add `platform/windows.rs` implementing `VideoSink` over the transport: a façade holding only a worker handle and shared counters, so it is `Send` with no mapping handle crossing a thread boundary, structurally matching `V4l2LoopbackSink`
- [x] 3.4 Implement `start`: probe first and refuse with the setup hint when no usable filter is registered, then create the section, initialise the header for the negotiated geometry, and start the worker — refusing before creating anything, since a section proves nothing about whether an application could see the camera (decision 7)
- [x] 3.5 Implement the worker: decode each queued JPEG with the existing portable `decode_jpeg_to_i420`, publish it into the ring, and advance the heartbeat on the idle path too (decision 1 keeps the decoder on this side of the process boundary)
- [x] 3.6 Implement `push_frame` as a non-blocking queue whose full case drops the incoming frame, and `frames_written` / `decode_failures` over the same atomic counters the Linux sink uses, so the UI's 0-fps contract holds identically on both platforms
- [x] 3.7 Return `"UnifiedStream Camera"` from `device_label`, and make `stop` idempotent, clear the running flag, bump `generation`, join the worker, and release the mapping — with `Drop` calling `stop`
- [x] 3.8 Confirm the application layer, `app.rs`, and the frontend components need no change, and that a camera refusal on Windows travels through the existing `VideoError::Unavailable` path into `CameraStatus.hint`

## 4. Frontend test

- [x] 4.1 Update `desktop/src/App.test.tsx`: the case pinning `"windows has no virtual camera implementation in this build"` with no copyable command no longer describes Windows, so replace it with the missing-component case that does supply a command
- [x] 4.2 Keep a case covering a platform that supplies a message and no command, since `platform-integration` still requires the UI to render guidance without a command when the platform names no remedy

## 5. Documentation

- [x] 5.1 Record in `docs/design.windows.md` that W3 is delivered as two changes, naming both, following the precedent already set for W1 and W2 in the phase plan
- [x] 5.2 Close the Windows 10 open question in that document: the re-check decision 3 required has been made, the development machine runs Windows 10, and `MFCreateVirtualCamera` is therefore not merely gated on a platform floor but untestable and additionally dependent on MSIX packaging
- [x] 5.3 Add the frame-transport consequence to decision 3 — frames cross a process boundary as decoded I420 through a versioned shared-memory ring, and the desktop keeps the decoder — alongside the four consequences already recorded there
- [x] 5.4 Confirm `python3 scripts/check-markdown.py` still passes, since it gates `docs/design.windows.md` for fence languages and heading levels
- [x] 5.5 Update the camera feature description in `README.md` so it does not describe the Linux mechanism as the only one

## 6. Verification

- [ ] 6.1 `cargo fmt --all -- --check`, `cargo clippy --workspace --lib --bins -- -D warnings`, and `cargo test --workspace` pass on Linux with no new warnings
- [x] 6.2 The same three commands pass on Windows, including the full test suite and the Windows-only mapping tests
- [x] 6.3 Manual Windows run: start a session, enable the camera, and confirm the desktop refuses the stream and shows the message naming the missing component together with a copyable command, rather than reporting that the platform has no implementation
- [x] 6.4 Manual Windows run: confirm the speaker still works unchanged alongside the refused camera, and that the session survives with one media feature unavailable
- [x] 6.5 Manual Windows run of `cargo run --example ring_smoke`: frames published are accepted byte-identically, a consumer that cannot keep up loses whole frames to being lapped and never accepts one spliced from two, and the heartbeat distinguishes idle from stopped. (A *sequential* slowed consumer cannot tear at all — it reads `latest`, which names a slot the producer has already finished — so the example races a reader thread against the producer instead, which is the only arrangement where the seqlock is under test.)
- [ ] 6.6 Manual Linux regression: the camera behaves exactly as before, including the `v4l2loopback` module-load hint, the device label in the UI, the delivered-fps figure, and the device being released when the stream stops
- [x] 6.7 Sync the delta specs into `openspec/specs/`, and in the same pass correct the two `## Purpose` lines a delta cannot reach: `camera-stream`, which calls the presentation a v4l2loopback webcam, and `speaker-stream`, which still calls the capture a PipeWire virtual sink after `add-windows-speaker-capture` removed that requirement
- [ ] 6.8 Archive the change and record the commands run in the pull request template
