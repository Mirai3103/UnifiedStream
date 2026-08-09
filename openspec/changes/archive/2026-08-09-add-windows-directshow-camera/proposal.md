## Why

`add-windows-camera-frame-transport` built the desktop's half of the Windows virtual camera and proved it: a versioned shared-memory ring, a seqlock tested under a racing reader, a security descriptor a sandboxed consumer can open, and an availability probe that refuses the stream and names the command that would fix it. Nothing consumes it. The probe finds no filter registered on any machine, so the camera refuses on every Windows install today, and the refusal is correct — the component it names does not exist yet.

This change writes that component. It is the second half of W3 in `docs/design.windows.md`, deliberately split from the first so the ring was specified and tested before the first C++ in this repository was written against it.

## What Changes

- Add a `windows/` tree with its own MSBuild toolchain, per decision 7 of `docs/design.windows.md`. This is the first non-Cargo, non-Gradle build area in the repository.
- Implement a DirectShow source filter in C++ that registers under `CLSID_VideoInputDeviceCategory` as "UnifiedStream Camera", opens `Local\UnifiedStream.Camera` read-only, and delivers I420 frames to the graph. The filter is a transliteration of `RingConsumer::read` and `RingConsumer::liveness` in `unifiedstream-video/src/transport.rs`, which is the normative statement of the protocol.
- Build and register both architectures. A 32-bit application can only load a 32-bit filter, and the desktop's probe already reports a half-registration as its own distinct state with its own remedy.
- Have `DllRegisterServer` write the `TransportVersion` value the probe reads, so a filter and a desktop that disagree are caught before a stream starts rather than after frames are misread. `DllUnregisterServer` removes everything `DllRegisterServer` created — a leftover CLSID leaves a broken camera in every application's device list.
- Add a native CI area building both architectures on every pull request, reported as its own required check. The Rust workspace check cannot see this code.
- Add a `LICENSE` file: MIT, covering the repository as a whole. The repository has none, and this change vendors third-party C++ for the first time; decision 8 warns that reading GPLv2 sources would otherwise make the licensing decision by accident and irreversibly. MIT matches every third-party source the Windows work depends on — `strmbase` here, `sysvad` in W4 — and leaves every route in decision 5 open, including bundling a proprietary signed driver under a redistribution licence.

Explicitly **not** in scope, and flagged rather than assumed:

- **The WiX installer** (decision 6). W3b registers the filter with `regsvr32`, which is what the desktop's setup hint already tells the user to run. The installer arrives with W4, which is the phase that actually needs it — it has a driver package to install under one elevation prompt. Building it here would mean building it before the thing it exists to install.
- **Authenticode signing.** The filter loads into other applications' processes and should be signed before it is distributed, but signing is a release concern and the phase table marks W3 "Authenticode only" against a build that is not yet released. Producing signable binaries is in scope; obtaining the certificate is not.
- **The `add-windows-camera-frame-transport` Linux regression (task 6.6)**, which was left unchecked when that change was archived. It is a gap in the previous change, not this one, and is noted here so it is not lost.

## Capabilities

### New Capabilities

- `virtual-camera-component`: the behavioural contract of a virtual camera component that consuming applications load into their own processes — what it must do as a guest in someone else's address space, how it declares the frame format version it speaks, how it behaves when the producer is idle, stopped, or gone, and what installing and uninstalling it must leave behind. `camera-stream` states the desktop's obligations as the producer; nothing yet states the consumer's, and the consumer is a separately built, separately installed artifact that can fail entirely on its own.

### Modified Capabilities

- `repository-quality-gates`: two requirement changes. The pull request validation requirement currently enumerates four areas (OpenSpec, Rust, Android, frontend) and assumes every buildable artifact is reachable from one of them; a native component built by a different toolchain needs its own required check. Separately, this is the first change to vendor third-party source, so the provenance rule that currently lives only in decision 8 of a design document becomes a checkable repository requirement.

`camera-stream` is deliberately **not** modified. `add-windows-camera-frame-transport` already generalised it — "How the camera is presented is a platform concern", plus the requirements *Virtual camera delivery tolerates its consumers* and *Virtual camera component compatibility* — precisely so this change would satisfy existing requirements rather than rewrite them. If implementing the filter turns out to need a `camera-stream` change, that is a signal the earlier generalisation was wrong and should be examined rather than patched.

## Impact

**New build surface.** `windows/dshow-camera/` (C++, MSBuild, x86 + x64), consuming `strmbase` from `microsoft/Windows-classic-samples` (MIT). A new CI job on `windows-latest` that does not use Cargo. `.github/workflows/quality.yml` gains an area; `CONTRIBUTING.md` gains the toolchain prerequisite (Visual Studio C++ workload, Windows SDK).

**Frozen contract, consumed for the first time.** Every value the filter must agree with is already pinned and asserted in `unifiedstream-video/src/transport.rs`: `FILTER_CLSID` `{6D8DD393-D871-4498-A24F-4AFFEACFC106}`, `FILTER_VERSION_VALUE` `TransportVersion`, `FILTER_INSTALL_DIR`, `FILTER_DLL_X64` / `FILTER_DLL_X86`, `FILTER_FRIENDLY_NAME`, `MAGIC` `USVC`, `TRANSPORT_VERSION` 1, `FOURCC_I420`, `SLOT_COUNT` 4, `HEADER_BYTES` 4096, `SLOT_BYTES` 3,112,960, and every field offset in `RingHeader` and `SlotHeader`. The Rust build asserts this layout; the C++ build must assert the same offsets independently, because no compiler sees both.

**No Rust changes expected on the production path.** `platform/windows.rs` already probes, refuses, produces, and publishes. Once a filter is registered, the probe returns `Usable` and the existing `start` path proceeds — the desktop was written against this component's eventual existence. Any change to the Rust library or binary this change turns out to need is a defect in W3a and should be recorded as one.

The one deliberate exception is the test harness: `examples/ring_smoke.rs` gains a publish-only mode so the C++ conformance tool can be the consumer instead of the Rust one. That is a new test, not a correction to a shipped path.

**Licensing.** Adding `LICENSE` sets the terms for the whole repository, including code already written. `obs-virtualcam` is GPLv2 and must not be read — decision 8, and a constraint on how the work is done, not only on what ships.

**Risk carried from the design.** Coverage is broad but not universal: Zoom, Discord, OBS, Skype, and Chromium-based browsers enumerate DirectShow devices; UWP, Store, and Media Foundation-only applications do not. This is documented as a known limitation in the same way the `v4l2loopback` prerequisite is on Linux.
