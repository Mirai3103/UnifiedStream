## Context

See `proposal.md` — Why, and `docs/design.windows.md` decision 3 for the architectural choice this change serves.

Four facts about the current code shape the design.

The `VideoSink` trait is already platform-neutral and was written anticipating this: `device_label` is documented as "a device node path on Linux, a registered filter name elsewhere", and `push_frame` takes raw JPEG bytes with the sink responsible for decoding on its own thread. Nothing in the trait needs to change.

`unifiedstream-video` has never had a second implementation. Its `platform/mod.rs` is a plain `linux` / `not(linux)` split with no per-integration table, and `unsupported` is compiled only off Linux — unlike `unifiedstream-audio`, which W1 and W2 left with a documented table and a fallback that compiles everywhere.

`SetupHint { message, command }` already crosses the Tauri boundary and the UI already renders a copyable command box when, and only when, the platform supplied one. A Windows sink that needs to say "install the component, like this" has a first-class channel for it and needs no frontend work.

The application layer already reopens the sink on a resolution change: `camera-stream` requires that a new resolution "re-acks, resets receive state, and reopens the virtual camera device at the new format", which `app.rs` implements as `stop` followed by `start`. The transport therefore never has to resize itself while consumers hold it — a constraint that turns out to be load-bearing in decision 5.

## Goals / Non-Goals

**Goals:**

- A frame transport whose failure modes are detectable rather than silent, specified precisely enough that a C++ consumer can be written against this document.
- Every property of the transport that can be tested without Windows, tested on both CI legs; every property that needs Windows kernel objects, tested on the Windows leg. Neither needs a camera or any other device.
- A Windows build that names what is missing and how to fix it, instead of reporting the platform as unimplemented.

**Non-Goals:**

- The DirectShow filter, the `windows/` tree, MSBuild, native CI, and registration tooling. Those are `add-windows-directshow-camera`.
- Any change to the wire protocol, the Android application, the application layer, or the frontend components. Only one frontend test changes, because it pins a string this change makes untrue.
- Supporting more than one producer. There is one desktop application; a second instance is an error to detect, not a case to serve.

## Decisions

### 1. The ring carries decoded I420, and decoding stays in Rust

The producer decodes JPEG with the existing portable `decode_jpeg_to_i420` and publishes raw planar frames. The alternative — publish JPEG and let the filter decode — halves the bytes crossing the boundary and was rejected on two grounds.

Decision 1 of `docs/design.windows.md` says only what provably cannot be done in user mode goes into the riskier component. The filter is not kernel-mode, but it is the component that runs *inside other people's processes*: a fault in it is a fault in Zoom or Chrome, and a JPEG decoder fed by network-originated data is the single largest attack and crash surface in this feature. It stays on our side of the boundary, in the pure-Rust decoder the Linux sink already uses.

The trait settles the rest. `decode_failures()` is on `VideoSink` precisely so that a stream of undecodable frames reads as 0 fps with a rising failure count rather than as a healthy stream. If the filter decoded, the producer could not count failures and that contract would be unenforceable on Windows.

The cost is bandwidth: 1280×720 I420 is 1,382,400 bytes, so 30 fps is roughly 40 MB/s of memory copy. That is a `memcpy` between pages already resident, not I/O, and DirectShow consumers want an uncompressed subtype anyway.

### 2. A fixed header and page-aligned slots, laid out identically for x86 and x64

One producer serves consumers of both architectures, so every offset must be the same in a 32-bit and a 64-bit process. That rules out pointers, `usize`, and any field whose size or alignment varies. The layout is `#[repr(C)]` with explicitly sized integers, each naturally aligned, and it is asserted at compile time rather than trusted.

```text
  mapping base
  ┌──────────────────────────────────────────── 4096 ─┐
  │ RingHeader (64 bytes) + padding to a page          │
  ├────────────────────────────────────────────────────┤
  │ slot 0   SlotHeader (32 bytes) + I420 payload      │
  ├────────────────────────────────────────────────────┤
  │ slot 1   …                                          │
  ├────────────────────────────────────────────────────┤
  │ slot 2   …                                          │
  ├────────────────────────────────────────────────────┤
  │ slot 3   …                                          │
  └────────────────────────────────────────────────────┘
```

```rust
#[repr(C)]
struct RingHeader {
    magic: u32,            //  0  b"USVC"
    version: u32,          //  4  transport version, 1 for this document
    header_bytes: u32,     //  8  offset of slot 0 from the mapping base
    slot_count: u32,       // 12
    slot_bytes: u32,       // 16  stride, header included, page-aligned
    fourcc: u32,           // 20  I420
    width: u32,            // 24  current geometry, not the allocated maximum
    height: u32,           // 28
    generation: AtomicU64, // 32  bumped whenever geometry or lifetime changes
    latest: AtomicU64,     // 40  sequence of the most recently published frame
    heartbeat_ms: AtomicU64, // 48 producer's monotonic tick
    producer_pid: u32,     // 56
    flags: u32,            // 60  bit 0: a stream is running
}                          // 64 bytes total

#[repr(C)]
struct SlotHeader {
    sequence: AtomicU64,   //  0  seqlock: odd while being written
    timestamp_us: u64,     //  8  capture time, from the media header
    bytes: u32,            // 16  payload length actually written
    reserved: [u32; 3],    // 20
}                          // 32 bytes total
```

`magic` and `version` are separate fields rather than a single tagged word so that a consumer can tell "this is not our mapping" from "this is ours and I am too old for it" — the two produce different messages to the user.

`timestamp_us` is the desktop's own publish time rather than the media header's capture time, which the comment above overstates. `VideoSink::push_frame` carries only bytes and this change does not alter the trait, so the capture time never reaches the sink. A consumer paces on the spacing between timestamps, which is unaffected. Two clocks are in play and neither substitutes for the other: `timestamp_us` comes from an `Instant` since the stream started, which is high-resolution but meaningful only inside this process, while `heartbeat_ms` comes from `GetTickCount64`, which is coarse but is the only clock every process in the session agrees on — and the heartbeat is read across the boundary.

### 3. A per-slot seqlock, because no consumer can be trusted

A consumer may be descheduled mid-copy, may be killed mid-copy, and must never be able to hold the producer up. That rules out any protocol where the producer waits for, or reasons about, a reader. The producer therefore writes unconditionally and the reader detects that it lost the race.

```text
  producer, publishing frame n into slot n mod slot_count
  ────────────────────────────────────────────────────────
    seq = slot.sequence            (even, or 0)
    slot.sequence = seq + 1        (odd: writing)   ─┐ Release
    copy payload, set bytes, timestamp               │
    slot.sequence = seq + 2        (even: stable)   ─┘ Release
    header.latest = n                                  Release

  consumer, reading
  ─────────────────
    n  = header.latest                                 Acquire
    s1 = slot.sequence                                 Acquire
    if s1 is odd            -> being written, skip
    copy payload
    s2 = slot.sequence                                 Acquire
    if s1 != s2             -> overwritten mid-copy, discard
```

A discarded read costs exactly one frame, which is what `camera-stream` already requires of every other loss in this pipeline. Four slots means a consumer has roughly three frame intervals — 100 ms at 30 fps — to complete a copy before the producer laps it, and being lapped is detected rather than silently tolerated.

The rejected alternative was a single mutex-protected latest-frame buffer, as the phase plan's phrasing ("a named mutex for the header") suggests. A mutex a consumer holds is a mutex a *dead* consumer holds: Windows reports `WAIT_ABANDONED` and leaves the protected data in an unknown state, and the producer would have to decide whether to trust it. The named mutex survives in this design for initialisation only — see decision 4 — and no steady-state read takes a lock.

### 4. `Local\` naming, an unversioned name, and a mutex that guards only initialisation

The kernel objects are `Local\UnifiedStream.Camera` for the section and `Local\UnifiedStream.Camera.Init` for the mutex. Session-local rather than `Global\`: the desktop and the applications consuming its camera are the same logged-on user's processes, and `Global\` would require `SeCreateGlobalPrivilege`, which a non-elevated desktop does not have.

**The name deliberately carries no version.** Putting one in it would make a mismatched filter fail to find the mapping at all, which is indistinguishable from the filter not being installed and produces the wrong message. Version lives in the header so a disagreement can be reported as a disagreement, which is what the added `camera-stream` requirement demands.

The section's security descriptor is fixed here rather than left to the implementation, because a consumer that cannot open the mapping fails in a way that looks like the feature being broken:

```text
  D:(A;;GA;;;OW)(A;;GR;;;AU)(A;;GR;;;AC)S:(ML;;NW;;;LW)
     │            │            │          └ label the object Low integrity,
     │            │            │            no-write-up: a sandboxed consumer
     │            │            │            may read, may not write
     │            │            └ all application packages: read
     │            └ authenticated users: read
     └ owner rights: full
```

The mandatory label is the part that is easy to omit and hard to diagnose. Windows defaults a new object to the creator's integrity level, so a Medium-integrity desktop produces a section that a Low-integrity consuming process — a browser's media process, for instance — cannot open at all, even though it belongs to the same user. Lowering the label to `LW` is what makes the mapping readable from a sandbox; `NW` keeps it read-only from there, which is all a consumer needs.

Change B must confirm this against a genuinely sandboxed consumer rather than against a console test, since that is the case a same-integrity test cannot reproduce.

**The owner ACE is `OW`, not `CO`, and this was corrected during implementation.** `CREATOR OWNER` is a placeholder SID that is substituted for a real one only when an ACE is *inherited* from a container; applied directly to a kernel object it matches nobody. The creating process never notices, because creating an object hands back a handle without an access check — so a naive test passes. What breaks is the *second* `CreateFileMappingW` against the same name, since the creating user's implicit owner rights are `READ_CONTROL` and `WRITE_DAC` and not map-write. That is precisely the reclaim path this decision requires, and with `CO` it fails with `ACCESS_DENIED`. `OWNER RIGHTS` (S-1-3-4) is the spelling that is evaluated at access-check time against whoever owns the object. A test that opens the section twice pins this, because it is invisible to any test that opens it once.

Consumers therefore map with `FILE_MAP_READ` rather than `FILE_MAP_ALL_ACCESS`: the DACL grants them read alone and the mandatory label forbids write-up, so a consumer asking for write access is refused. `RingConsumer` never writes, which is what makes a read-only mapping sufficient.

The mutex is taken only while the header is created or re-initialised. It exists because a second desktop instance, or a restart while a filter still holds the section open, must not race the header into an inconsistent state — a named section is refcounted, so `CreateFileMappingW` against a name a surviving consumer still holds returns the *existing* section rather than a fresh one, and the producer must recognise and reclaim it rather than assume it is new.

### 5. Allocate for the maximum geometry, and signal changes with `generation`

The mapping is sized once, for the application's existing `1920 × 1080` cap: four slots of `align_up(32 + 3_110_400, 4096)` bytes, about 11.9 MiB. The header's `width`/`height` describe the *current* stream, not the allocation.

Sizing to the negotiated geometry instead, and recreating the mapping on a resolution change, does not work — and this is the trap decision 4 already names. A consumer holding the section keeps it alive; the producer's `CreateFileMappingW` then returns that same, wrongly sized section, and the consumer never learns a new one exists. A single fixed allocation removes the failure entirely at the cost of about twelve megabytes of pagefile-backed commit.

`generation` is bumped under the init mutex whenever geometry changes or a stream starts or stops. A consumer caches geometry and re-reads the header whenever `generation` moves, so a resolution change mid-session is observed rather than misread as a stream of malformed frames.

### 6. A heartbeat, so "idle" and "gone" are different states

`camera-stream` already requires that a gap in arriving frames freezes the virtual camera rather than breaking it. Across a process boundary that requirement splits in two, because a filter cannot otherwise tell a quiet stream from a dead desktop.

The producer advances `heartbeat_ms` on every publish and, while a stream is running, at least every 250 ms even with no frames to publish. On `stop` it clears bit 0 of `flags` and bumps `generation`, so a clean stop is observed immediately rather than after a timeout.

```text
  flags bit 0 set, heartbeat advancing, latest advancing   -> deliver frames
  flags bit 0 set, heartbeat advancing, latest static      -> hold the last frame
  flags bit 0 clear                                        -> no stream; placeholder
  heartbeat stale beyond 2 s                               -> producer gone; placeholder
```

A DirectShow source filter must keep handing buffers to its graph regardless — a filter that stops delivering hangs the consuming application — so "placeholder" means a static frame, never an absence of frames. What that frame contains is change B's concern; that one must exist is specified here.

### 7. Availability is a registry probe, in both bitness views

`start` resolves whether a virtual camera exists at all by reading the filter's `InprocServer32` under its CLSID in `HKEY_CLASSES_ROOT` and confirming the named DLL is present on disk. Reading `HKCR` needs no elevation.

**The CLSID is fixed by this change**, not by the one that writes the filter, because it is part of the contract the two share:

```text
  {6D8DD393-D871-4498-A24F-4AFFEACFC106}
```

It is arbitrary, as every GUID is, and it is permanent. `add-windows-directshow-camera` registers this exact value under `CLSID_VideoInputDeviceCategory`; a mismatch between the two changes is not a compile error on either side, which is why the value is written down in the shared document rather than agreed later.

Two further facts belong to the same contract, and were settled here for the same reason — the refusal this change ships has to name a command, and a command needs a path:

- The filter installs as `%ProgramFiles%\UnifiedStream\UnifiedStreamCamera64.dll` and `…\UnifiedStreamCamera32.dll`. The 32-bit half must be registered by the 32-bit `regsvr32` in `SysWOW64`, which reads backwards and is the usual way this is got wrong.

**A hint's command must not contain shell syntax, and this was found by running one.** The obvious spelling — `regsvr32 "%ProgramFiles%\…\…64.dll" && %SystemRoot%\SysWOW64\regsvr32.exe "…"` — works only in `cmd.exe`. `%VAR%` is literal text in PowerShell, and `&&` is a parse error in Windows PowerShell 5.1, which is still what "Run as administrator" gives on Windows 10. Pasted into PowerShell the command fails in a way that reads as the guidance being wrong rather than as the shell being different, which is worse than no command at all. Two consequences:

- The desktop expands `%ProgramFiles%` and `%SystemRoot%` itself. It is building the hint on the machine that will run it, so it can name real paths and leave nothing for a shell to interpret.
- Each hint carries exactly one invocation, never two joined by an operator. `Absent` names the 64-bit registration alone; once that is done the probe reports `OneArchitecture`, which names the other. The probe's existing states sequence the two steps, so no hint ever needs a shell operator to describe them.

A test asserts that no generated command contains `%`, `&&`, or `;`, and that each names an absolute path — the property is invisible to any test that only checks a command is present.
- Registration records the transport version the filter speaks, as a `REG_DWORD` named `TransportVersion` on the CLSID key. **Without it the version-mismatch refusal cannot be implemented at all**: the desktop is the producer, so it only ever writes the header's version and never reads one a filter wrote. The `camera-stream` requirement demands the desktop *report* a disagreement as setup guidance before any stream starts, and the registry is the only place the two components meet before frames flow. A registration carrying no such value predates the contract and is treated as version 0, which this build does not speak.

The probe reads both the 64-bit and the 32-bit registry views. The desktop is a 64-bit process, so the native view tells it whether 64-bit applications can see the camera; the `WOW6432Node` view tells it whether 32-bit ones can. Half-registered is a real state — decision 3 of the Windows design requires both DLLs — and it produces its own message rather than being rounded to "installed".

Probing the registry rather than attempting to create the section is deliberate: the section is ours to create whether or not any filter exists, so its presence proves nothing about whether an application could ever see the camera.

`device_label()` returns the registered friendly name, `"UnifiedStream Camera"` — the same string the Linux sink's device path occupies in the UI, and the reason the trait specifies an opaque platform-supplied label rather than a path.

### 8. Polling, not an event

The phase plan anticipated "a named event for signalling". This design has none, and consumers poll `latest`.

An auto-reset event wakes exactly one waiter, which is wrong the moment two applications hold the camera; a manual-reset event the producer never clears is permanently signalled and therefore carries no information. Getting fan-out right would mean per-consumer events registered in the header, and registration is state the producer would have to maintain about consumers it is not allowed to trust.

Polling costs nothing here because the consumer is already timer-driven: a DirectShow source filter runs its own thread delivering buffers at the negotiated frame rate whether or not new frames have arrived. It reads `latest` when it is about to deliver anyway. This also matches the idiom W2 established for the Windows audio capture, which polls rather than waiting on callbacks.

### 9. The ring is portable code with a thin Windows shell

The header layout, the sequence protocol, the slot arithmetic, and the geometry and version checks live in a module with no Win32 in it, over a plain byte slice. Only mapping creation, the security descriptor, the mutex, and the registry probe are target-gated.

This is decision 5 of `add-windows-speaker-capture` applied again, and for the same reason: the resampler was made portable because arithmetic that fails silently must be covered by the test leg that actually runs. A seqlock is the same kind of code — a missing `Acquire` produces a rare corrupt frame, not a failure — and the Windows CI runners can host the tests either way, since none of this needs a device.

## Risks / Trade-offs

- **[A torn read or a missing memory ordering produces rare corrupt frames rather than a test failure]** → The whole reason for the split and for decision 9. The sequence protocol is tested against a reader thread deliberately racing a writer thread, asserting that every frame a consumer accepts is byte-identical to one the producer published and that lapping is reported rather than absorbed. The failure this guards against is a wrong frame, not a missing one.
- **[The layout drifts between the Rust producer and the C++ consumer]** → Compile-time assertions on the size, alignment, and every field offset of both structures, so a change that would break the C++ side fails the Rust build first. `docs/design.windows.md` already requires the transport be versioned so a stale filter refuses; this makes the version bump impossible to forget by making the layout impossible to change silently.
- **[Any process in the session can read the camera frames]** → Inherent to a shared-memory virtual camera and true of every implementation of this pattern; the descriptor narrows it to the same user rather than the machine. The frames are already being handed to arbitrary applications by the feature's own design — that is what a virtual camera is — so the exposure is the user's own session, and it exists only while a stream is running.
- **[Twelve megabytes of commit are held for a stream that may run at 720p]** → Accepted, and the alternative is worse: a section a consumer keeps alive cannot be resized, so geometry-driven allocation produces a wrongly sized mapping the consumer cannot discover. Decision 5.
- **[Change A lands with no consumer, so the producer path ships unexercised by real use]** → It ships exercised by a consumer harness that implements the same protocol, and gated behind a registry probe that refuses the stream until change B installs a real one. No user reaches the producer path until something can read it.
- **[The 32-bit consumer pays for 64-bit atomics]** → `std::atomic<uint64_t>` on 32-bit x86 compiles to a locked compare-exchange rather than a plain load. It is one such operation per frame per consumer, against a 40 MB/s copy; the alternative, 32-bit sequence numbers, buys nothing and makes the heartbeat wrap in 49 days.

## Migration Plan

Nothing is persisted and nothing is installed, so there is no migration. Before this change Windows reports the camera as having no implementation; after it, Windows reports the virtual camera component as missing and names the command that installs it. Both are refusals with reason `internal` and both leave the session, the speaker, and the microphone untouched.

Rollback is a revert. The kernel objects exist only while a camera stream is running and are destroyed with the last handle.

## Open Questions

None blocking. One is deferred to `add-windows-directshow-camera` by design: what a filter renders when the producer is absent or the stream is stopped — a static placeholder is required here, its content is that change's to choose.
