# Tasks — Windows DirectShow Camera Filter

## 1. Licence and repository groundwork

- [x] 1.1 Add `LICENSE` at the repository root: MIT, covering the repository as a whole (decision 6). This lands first because decision 8 of `docs/design.windows.md` forbids reading a third-party source before its licence is known to be compatible, and "compatible with what" has had no answer until now
- [x] 1.2 Record in `README.md` that the project is MIT licensed
- [x] 1.3 Create `windows/third_party/strmbase/` with the `strmbase` sources from `microsoft/Windows-classic-samples`, and a `README.md` beside them recording the upstream repository, the exact commit taken, and the MIT licence text — vendored rather than fetched at build time, so what was licence-checked is what gets compiled (decision 1)
- [x] 1.4 Do not read, copy, or consult `obs-virtualcam` at any point. It is GPLv2 and this repository is now MIT (decision 8). Noted as a task because it constrains how the work is done, not what it produces, and is therefore invisible in review of the result

## 2. The transport header in C++

- [x] 2.1 Add `windows/dshow-camera/transport.h`, mirroring `RingHeader` and `SlotHeader` from `unifiedstream-video/src/transport.rs` with explicitly sized integers and no pointer-width field, so the layout is identical in the x86 and x64 builds
- [x] 2.2 Mirror every constant the contract pins: `MAGIC` `USVC`, `TRANSPORT_VERSION` 1, `FOURCC_I420`, `SLOT_COUNT` 4, `HEADER_BYTES` 4096, `SLOT_BYTES` 3112960, `MAX_WIDTH` 1920, `MAX_HEIGHT` 1080, `HEARTBEAT_STALE_MS` 2000, and the section name `Local\UnifiedStream.Camera`. Write the literals, and reference the Rust name each one comes from in a comment
- [x] 2.3 Add `static_assert` on `sizeof`, `alignof`, and every `offsetof` of both structures, using the same numbers as the `const _: () = assert!` lines in `transport.rs` (`RingHeader` 64 bytes, `SlotHeader` 32 bytes, and each field offset). A field reordered on either side must fail a build rather than produce wrong pixels
- [x] 2.4 Implement the consumer half as a transliteration of `RingConsumer::attach`, `sync`, `read`, and `liveness` — **including the memory orderings, which are the part that has no layout and therefore no static assertion**: acquire on `generation`, acquire on `latest`, acquire on the slot sequence before the copy, and an acquire fence between the copy and the second sequence read. Name in a comment what each one pairs with on the producer side
- [x] 2.5 Reject an odd slot sequence before copying, and compare the sequence again afterwards, discarding the copy when it moved — a torn read is a frame that is part one picture and part another, and parity alone does not catch a slot rewritten completely during the copy
- [x] 2.6 Clamp the slot's `bytes` field to the slot payload capacity before it reaches any copy length. It lives inside the seqlock-protected region and may be arbitrary while a write is in flight; the version comparison decides whether the copy counts, but the clamp is what keeps an in-flight value from being a buffer overrun first
- [x] 2.7 Return the three attach failures distinguishably — not ours, version we do not speak, internally inconsistent — matching `AttachError`, since the `virtual-camera-component` spec requires they not be collapsed
- [x] 2.8 Implement the liveness state machine over `flags`, `heartbeat_ms`, and `latest`, returning the four states of `Liveness`

## 3. The filter

- [x] 3.1 Add `windows/dshow-camera/` as an MSBuild project producing `UnifiedStreamCamera64.dll` and `UnifiedStreamCamera32.dll` — the exact names `FILTER_DLL_X64` and `FILTER_DLL_X86` pin, because the desktop's setup hint already names them in a command it tells the user to paste
- [x] 3.2 Implement the filter as a `CSource` subclass with one `CSourceStream` output pin, carrying the CLSID `{6D8DD393-D871-4498-A24F-4AFFEACFC106}` and the friendly name `UnifiedStream Camera` (`FILTER_FRIENDLY_NAME`, which is `CAMERA_NODE_LABEL` — the camera has one name across the product)
- [x] 3.3 Implement `GetMediaType` / `CheckMediaType` offering I420 at 640×480, 1280×720, and 1920×1080, 30 fps, **without consulting the ring** (decision 2): at connect time there is usually no producer, and a filter that cannot answer until the phone is streaming fails to connect in the ordinary case where the user opens their conferencing application first
- [x] 3.4 Implement `DecideBufferSize` for the largest offered geometry, and allocate every buffer the streaming path uses at connection time — no allocation on the streaming path once connected, because this code runs inside another application's process
- [x] 3.5 Implement `FillBuffer` on the graph's streaming thread with no worker thread of its own (decision 3): read the ring, scale into the connected geometry, pace to the media type's `AvgTimePerFrame`, and set the sample time from the slot's `timestamp_us`
- [x] 3.6 **`FillBuffer` must have no path that waits and no path that returns without a buffer.** Map the producer states as decision 3's table specifies: a frame is delivered scaled; `NoFrame`, `BeingWritten`, `Torn`, and `Holding` hold the previous frame; `Stopped`, `ProducerGone`, and not-attached deliver the placeholder. A filter that stops delivering does not freeze the picture, it hangs the application it lives in
- [x] 3.7 Implement the scaler: aspect-preserving, letterboxing the remainder, plane-wise over I420, and **bypassed entirely when the ring geometry already equals the connected geometry**, which is the default configuration on both ends
- [x] 3.8 Implement the placeholder as a generated flat I420 field requiring no asset, no font, and no file access
- [x] 3.9 Implement attachment: try `OpenFileMappingW` with `FILE_MAP_READ` on start and, while detached, roughly twice a second from the streaming thread — the section grants consumers read access only, so a read-only view is the contract and not a precaution
- [x] 3.10 Treat a `NotOurs` result as a retry rather than an error: the producer clears `magic` first during initialisation and writes it last, so a filter attaching mid-initialisation sees exactly this and must not conclude the camera is broken
- [x] 3.11 Re-read cached geometry whenever `generation` moves, covering both a resolution change and a producer restarting against a section this filter still holds alive
- [x] 3.12 Treat a version disagreement as terminal for that attachment — detach, show the placeholder, do not retry in a loop that can only fail identically. The user-visible resolution comes from the desktop, which reads the declared version from the registry before a stream starts
- [x] 3.13 Confirm no decoder of any kind is linked into the filter. Decision 3 of `docs/design.windows.md` keeps JPEG on the desktop's side of the boundary specifically so the component loaded into other applications' processes never parses network-originated bytes

## 4. Registration

- [x] 4.1 Implement `DllRegisterServer` writing `InprocServer32` under `HKCR\CLSID\{6D8DD393-D871-4498-A24F-4AFFEACFC106}`, with the DLL path resolved at run time from the module handle rather than assumed — the user may have installed anywhere, and the desktop's probe confirms the named file exists on disk
- [x] 4.2 Register with `IFilterMapper2` under `CLSID_VideoInputDeviceCategory` with the friendly name, which is what makes applications enumerate the camera at all
- [x] 4.3 Write `TransportVersion` as a `REG_DWORD` on the CLSID key, equal to the transport version compiled in. **Easy to omit and invisible when omitted**: `platform/windows.rs` treats a missing value as version 0 and reports `VersionMismatch`, so a filter that skips this step is a filter the desktop refuses to talk to
- [x] 4.4 Implement `DllUnregisterServer` reversing all three, and verify nothing is left behind — a leftover CLSID leaves a camera in every application's device list that cannot be loaded
- [x] 4.5 Do not attempt to register both architectures from one invocation. The desktop's `FilterAvailability::OneArchitecture` walks the user through one command at a time, and each must land in its own registry view: the 32-bit `regsvr32` in `SysWOW64` writes `WOW6432Node`, which is where `probe_view(KEY_WOW64_32KEY)` looks
- [x] 4.6 Add `.def` files or `__declspec(dllexport)` so `DllGetClassObject`, `DllCanUnloadNow`, `DllRegisterServer`, and `DllUnregisterServer` are exported from both architectures

## 5. Cross-toolchain conformance

- [x] 5.1 Add a publish-only mode to `unifiedstream-video/examples/ring_smoke.rs` — publish a verifiable pattern for a bounded run without attaching its own Rust consumer, so the C++ tool can be the consumer. **This is a harness addition, not a Rust defect**: the proposal's "no Rust changes expected" is about the production path, which stays untouched
- [x] 5.2 Add `windows/dshow-camera/ring_conform.exe` as a second MSBuild target, built from **the filter's own transport translation unit rather than a copy of it** — a conformance test against a duplicate proves the duplicate conforms
- [x] 5.3 Have it attach to the real named section, read until the producer stops, and verify every accepted frame byte-for-byte against the published pattern, reporting frames accepted, rejected as torn, and lost to being lapped
- [x] 5.4 Make it exit non-zero on any frame accepted with wrong contents, and on zero frames accepted — a test that accepted nothing proves nothing
- [x] 5.5 Wire it into CI against `cargo run -p unifiedstream-video --example ring_smoke` in publish-only mode. **This is the only thing in either build that observes both halves at once**: a wrong memory ordering in the C++ consumer compiles cleanly, passes every static assertion in task 2.3, and tears rarely enough to reach users
- [x] 5.6 Implement `IAMStreamConfig` on the output pin — the capability list, `SetFormat`, and `GetFormat`. Found by manual verification, not by any check here: Chromium's `VideoCaptureDeviceWin` queries it before anything else and abandons the device when the query fails, so the camera enumerated in Discord and Chrome and opened in neither, showing a black picture and `NotReadableError`. `IKsPropertySet` alone is what a *graph builder* needs; this is what an *application* needs
- [x] 5.7 Add `windows/dshow-camera/filter_conform.exe`, checking the COM surface from outside through COM alone — no strmbase, no shared header with the filter, the DLL loaded by path so it needs no administrator. A check that reached into the filter's own constants could not have caught 5.6, because the fault was an interface the filter never claimed to implement. Verified to fail against the pre-fix binary and pass against the fixed one
- [x] 5.8 Wire `filter_conform` into CI ahead of the frame-level test: if the interfaces are wrong, whether the pixels are right does not matter yet

## 6. Build and quality gates

- [x] 6.1 Add `windows/UnifiedStreamWindows.sln` tying the filter and the conformance tool together, with `x64` and `Win32` configurations
- [x] 6.2 Add a native job to `.github/workflows/quality.yml` on `windows-latest`, building both architectures with MSBuild and running the conformance test, reported as its own required check — nothing in the existing four areas builds C++, so without this a change that breaks the filter merges green
- [x] 6.3 Confirm the job shares no cache, toolchain, or working directory with the Rust job, which uses `desktop/src-tauri` as its working directory
- [x] 6.4 Update `CONTRIBUTING.md` with the Windows native prerequisites: the Visual Studio C++ workload and the Windows SDK, and how to build the solution
- [x] 6.5 Record the new required check in whatever branch-protection documentation `add-quality-gates-and-pr-workflow` established, so the ruleset and the workflow do not drift

## 7. Documentation

- [x] 7.1 Mark W3 fully implemented in the phase plan of `docs/design.windows.md`, and name this change beside `add-windows-camera-frame-transport` as the second half
- [x] 7.2 Record in decision 3 of that document that the filter offers a fixed set of resolutions and scales into the connected one, and why: at connect time there is usually no producer to ask for a geometry
- [x] 7.3 Record the MIT licence decision in that document beside decision 8, which is the decision it resolves
- [x] 7.4 Document the DirectShow coverage limitation for users — UWP, Store, and Media Foundation-only applications do not enumerate the camera — in the same place and the same way the `v4l2loopback` prerequisite is documented on Linux
- [x] 7.5 Update the camera feature description in `README.md` now that Windows has a working virtual camera rather than a refusal
- [x] 7.6 Confirm `python3 scripts/check-markdown.py` still passes, since it gates every file in `docs/`

## 8. Verification

- [ ] 8.1 `cargo fmt --all -- --check`, `cargo clippy --workspace --lib --bins -- -D warnings`, and `cargo test --workspace` pass on Linux with no new warnings
- [x] 8.2 The same three pass on Windows, and the MSBuild solution builds both architectures with no warnings
- [x] 8.3 The conformance test passes locally and in CI, with a non-zero frame count and zero frames accepted with wrong contents
- [x] 8.4 Manual: register the 64-bit filter only, and confirm the desktop reports `OneArchitecture` and names the 32-bit command — the half-registered state is a real state and this is the only way to see it
- [x] 8.5 Manual: register both, start a session, enable the camera, and confirm the desktop no longer refuses and a 64-bit application (OBS or Chrome) shows the phone's video
- [x] 8.6 Manual: confirm a 32-bit application sees the camera too, which is the entire reason two DLLs are built
- [x] 8.7 Manual: select the camera in an application **before** starting the stream, and confirm it connects, shows the placeholder, and begins showing video when the stream starts without the application reselecting the camera — the ordinary first-run sequence, and the case decision 2 exists for
- [x] 8.8 Manual: change the phone's resolution mid-stream and confirm the picture follows without the application reselecting the camera and without a run of corrupt frames
- [x] 8.9 Manual: kill the desktop while an application is consuming the camera, and confirm the application does not hang, shows the placeholder within two seconds, and recovers when the desktop restarts
- [x] 8.10 Manual: stop the stream cleanly and confirm the placeholder appears immediately rather than after the heartbeat times out
- [x] 8.11 Manual: kill a consuming application abruptly and confirm the stream continues, other consumers are undisturbed, and the camera can be selected again afterwards
- [x] 8.12 Manual: run two consuming applications at once and confirm both receive video
- [x] 8.13 Manual: `regsvr32 /u` both architectures and confirm the camera disappears from every application's device list and the desktop reports it absent rather than present and broken
- [ ] 8.14 Manual Linux regression: the camera behaves exactly as before, including the `v4l2loopback` hint, the device label, the delivered-fps figure, and the device being released on stop. **This also closes `add-windows-camera-frame-transport` task 6.6**, which was left unchecked when that change was archived
- [ ] 8.15 Sync the delta specs into `openspec/specs/`, archive the change, and record the commands run in the pull request template
