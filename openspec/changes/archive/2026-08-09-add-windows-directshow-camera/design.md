# Design — Windows DirectShow Camera Filter

## Context

See `proposal.md` for motivation, and `docs/design.windows.md` decisions 3, 7, and 8 for the architecture this change implements a part of.

The constraint that shapes everything below is that the producer already exists and is not negotiable. `unifiedstream-video/src/transport.rs` is the normative statement of the protocol — its own module documentation says so — and `platform/windows.rs` already creates the section, refuses the stream when no filter is registered, and names the exact `regsvr32` command that would fix it. Every constant this change must agree with is pinned, asserted at compile time, and covered by tests that run on both CI legs today.

So this is not a design problem in the usual sense. The wire format is settled. What has to be decided is everything the filter does that the ring does not describe: how it presents itself to a graph, what it shows when there is nothing to show, and what geometry it claims when it has no idea what geometry the phone will send.

Three facts about the environment drive the decisions:

- **The filter runs in someone else's process.** Zoom, Chrome, Discord, and OBS load this DLL into their own address space. A fault is their crash. A hang is their hang.
- **The filter is usually loaded before the producer exists.** The user opens the video-conferencing application, then picks the camera, then reaches for their phone. At the moment the graph connects and fixes the media type, `Local\UnifiedStream.Camera` very often does not exist at all.
- **Neither compiler sees both halves.** The Rust build asserts the layout; the C++ build asserts the layout; nothing asserts they are the same layout. That gap is the whole reason W3 was split into two changes, and closing it is this change's central testing problem, not an afterthought.

## Goals / Non-Goals

**Goals:**

- A DirectShow source filter that satisfies `virtual-camera-component` and turns the desktop's existing refusal into a working camera.
- A C++ mirror of the transport header whose divergence from the Rust original is caught mechanically rather than by review.
- A cross-toolchain conformance test: the real C++ consumer against the real Rust producer, run in CI.
- A `windows/` tree and MSBuild pipeline that W4 can extend without rework.

**Non-Goals:**

- The WiX installer (decision 6, W4). Registration here is `regsvr32`, which is what the desktop's hint already tells the user to run.
- Obtaining a code-signing certificate. Producing binaries that *can* be signed is in scope; the certificate is a release concern.
- Any change to the Rust workspace. If one proves necessary, that is a defect in `add-windows-camera-frame-transport` and should be recorded as one rather than absorbed silently here.
- Media Foundation, UWP, or Store application support. Decision 3 accepted that limitation explicitly.

## Decisions

### 1. Build on `strmbase` from `Windows-classic-samples`, vendored into the tree

The filter derives from `CSource` / `CSourceStream`. These base classes are not in the Windows SDK — they shipped with the old DirectShow SDK and now live in `microsoft/Windows-classic-samples` under MIT.

Vendored into `windows/third_party/strmbase/` with its origin, commit, and licence recorded beside it, rather than fetched at build time. A build that reaches the network is a build that breaks when someone else's repository moves, and the whole point of pinning a licence is that the thing you pinned is the thing you compiled.

*Alternative considered:* implementing `IBaseFilter`, `IPin`, `IAMStreamConfig`, and `IMediaSource` directly. Roughly two thousand lines of COM boilerplate that is famously easy to get subtly wrong, in exchange for avoiding a vendored dependency that Microsoft published under MIT for exactly this purpose. Rejected.

*Not considered, deliberately:* `obs-virtualcam`. Decision 8 — it is GPLv2, and this repository now publishes under MIT (decision 6 below), so reading it is not permitted regardless of how tempting the reference is.

### 2. The filter offers a fixed set of resolutions and scales into whichever the application picks

The filter advertises 640×480, 1280×720, and 1920×1080 I420 at 30 fps. Whatever geometry the ring is publishing at, the filter scales it — preserving aspect ratio, letterboxing the remainder — into the geometry the graph connected at.

This looks like unnecessary work, and the obvious alternative is to connect at whatever geometry the ring currently advertises. That alternative does not survive contact with the second fact in the context above: **at connect time there is usually no producer**. `OpenFileMappingW` fails, there is no header, there is no geometry, and the filter still has to answer `GetMediaType` before the graph will connect it. A filter that cannot answer until the phone is streaming is a filter that fails to connect in the ordinary case, and the ordinary case is the user opening Zoom first.

Once the geometry is decoupled from the ring, the `virtual-camera-component` requirement about following geometry changes becomes almost free: a resolution change on the phone changes the scaler's input, and the graph never learns anything happened. The alternative would have required a dynamic format change mid-connection — `QueryAccept`, `ReceiveConnection`, renegotiation — which a large share of consuming applications handle badly or not at all.

The cost is CPU in the host's process. It is bounded and small: a plane-wise box scale of I420 at 30 fps, on the same order as the `memcpy` the read already performs, and zero in the common case where the phone's 1280×720 matches the connected 1280×720 and the scaler is bypassed entirely.

*Alternative considered:* advertise one fixed resolution, 1280×720. Simpler still, and rejected because applications legitimately want to pick 480p on a constrained uplink, and the phone already offers the choice.

### 3. `FillBuffer` reads the ring directly on the streaming thread, and always returns a frame

`CSourceStream::FillBuffer` is called in a loop on the graph's own streaming thread. The filter does its ring read there, with no worker thread of its own.

A worker thread would buy nothing. The read is a bounded `memcpy` against a page-resident mapping with no lock held — the seqlock is precisely the mechanism that makes it non-blocking — so there is nothing to move off the streaming thread. Adding one would add a queue, a hand-off, and a second place for the filter's lifetime to be wrong inside a host process.

`FillBuffer` returns a frame every time it is called, pacing itself to the media type's `AvgTimePerFrame`. The five producer states map onto five sources of pixels:

| `RingConsumer` state | What `FillBuffer` delivers |
| --- | --- |
| `Frame` | the frame, scaled |
| `NoFrame` / `BeingWritten` / `Torn` | the previous frame, held |
| `Liveness::Holding` | the previous frame, held |
| `Liveness::Stopped` / `ProducerGone` | the placeholder |
| not attached at all | the placeholder |

This is the direct expression of *the component never stalls the application hosting it*. There is no path on which `FillBuffer` waits for the desktop, and no path on which it returns without a buffer. A DirectShow filter that stops delivering does not merely show a frozen picture — it hangs the application it lives in.

### 4. Attachment is retried on a timer, and the header is re-validated on every geometry change

The filter attempts `OpenFileMappingW` when it starts and, while detached, roughly twice a second on the streaming thread. Retrying every frame would hammer the object manager for no benefit; a fixed back-off means the camera comes alive within half a second of the user starting the stream, which is indistinguishable from instant.

Two transient conditions look like failure and must not be treated as it. The producer clears `magic` first during initialisation and writes it last, so a filter attaching mid-initialisation sees `NotOurs` — a retry, not an error. And a producer restarting against a section a surviving consumer still holds re-initialises the same section, bumping `generation`; the filter must re-read its cached geometry rather than assume it still knows the layout. Both are already covered by `RingConsumer`; the C++ must reproduce the behaviour, not just the struct.

A version disagreement is different and is terminal for that attachment: the filter detaches, shows the placeholder, and does not retry in a loop that would only fail identically. The user-visible resolution comes from the desktop side, which reads the declared version from the registry and refuses the stream with a hint naming the fix.

### 5. The layout is asserted in C++, and then proven against Rust by a conformance test

Two mechanisms, because they catch different failures.

`windows/dshow-camera/transport.h` mirrors `RingHeader` and `SlotHeader` with `static_assert` on `sizeof`, `alignof`, and every `offsetof`, using the same literal numbers the Rust `const _: () = assert!` lines use. A field reordered on one side fails a build. This catches layout drift and nothing else.

It does not catch the failure that actually matters. Memory ordering has no layout: a C++ consumer that reads `latest` with `memory_order_relaxed`, or skips the acquire fence between the copy and the second sequence read, compiles cleanly, passes every static assertion, and produces a torn frame perhaps once an hour on one machine in ten. That is the failure mode the whole two-change split exists to prevent, and no amount of asserting `offsetof` touches it.

So the change ships `windows/dshow-camera/ring_conform.exe`: a standalone consumer built from the filter's own transport code — the same translation unit, not a copy — which attaches to the real named section, reads frames, and verifies each against the pattern the producer wrote. CI runs it against `cargo run --example ring_smoke`, which already exists and already publishes a verifiable pattern. Every frame accepted must be byte-identical to one published; a spliced frame fails the job.

This is the single highest-value test in the change. It is the only thing in either build that observes both halves at once.

*Alternative considered:* transliterating the Rust tests into a C++ unit test over a private buffer. Useful, and it would not have caught the ordering bug either, because a single-threaded test cannot tear. The race has to be real.

### 6. The repository publishes under MIT

`LICENSE` at the root, MIT, applying to the repository as a whole.

Every third-party source the Windows work depends on is MIT — `strmbase` from `Windows-classic-samples` here, `sysvad` from `Windows-driver-samples` in W4 — so there is no compatibility question to answer. MIT also leaves every route in decision 5 of `docs/design.windows.md` open: bundling VB-Audio's proprietary signed driver under a redistribution licence is a live option for the microphone, and a copyleft licence on this repository would have foreclosed it while the funding decision is still unmade.

It also settles decision 8 in the direction it was already pointing. `obs-virtualcam` remains unreadable, which was true before this decision and stays true after it.

### 7. Registration writes what the probe already reads

`DllRegisterServer` does three things, and the third is the one that is easy to forget:

1. Registers the coclass under `HKCR\CLSID\{6D8DD393-D871-4498-A24F-4AFFEACFC106}` with `InprocServer32` naming the DLL's own path, resolved at run time from the module handle rather than assumed.
2. Registers the filter with `IFilterMapper2` under `CLSID_VideoInputDeviceCategory` with the friendly name `UnifiedStream Camera`, which is what makes applications enumerate it.
3. Writes `TransportVersion` as a `REG_DWORD` on the CLSID key, equal to the transport version the DLL was compiled against.

Step 3 exists because the desktop is the producer and never reads anything the filter writes: without a declared version, a filter built against transport v2 and a desktop speaking v1 would agree on nothing and discover it only as corrupt video. `platform/windows.rs` already reads this value and already treats its absence as version 0. The registration merely has to write what the probe is looking for.

`DllUnregisterServer` reverses all three. A leftover CLSID leaves a camera in every application's device list that cannot be loaded.

Each architecture registers into its own registry view naturally: a 32-bit DLL registered by the 32-bit `regsvr32` in `SysWOW64` lands in `WOW6432Node`, which is exactly where `probe_view(KEY_WOW64_32KEY)` looks. Nothing in the code needs to know which view it is in, and nothing should try.

### 8. The native build is its own CI area

MSBuild on `windows-latest`, building `x64` and `Win32` configurations of the filter and the conformance tool, reported as a distinct required check. It shares no cache, no toolchain, and no working directory with the Rust job.

Nothing in the existing four areas builds C++, so without this a change that breaks the filter merges green. That is the requirement change in the `repository-quality-gates` delta, stated generally: an area is defined by what builds it, not by what language it is written in.

## Risks / Trade-offs

- **[The C++ consumer diverges from the Rust producer in memory ordering, not layout]** → Decision 5's conformance test, run in CI against a real race. This is the risk the entire two-change split was designed around; a green `static_assert` is not evidence about it.
- **[The filter crashes a host application]** → It is loaded into Zoom, Chrome, and Discord, so a fault is their fault to the user. Mitigated structurally: no decoder (decision 3 of `docs/design.windows.md`), no allocation on the streaming path after connection, read-only mapping, and every read length clamped to the slot capacity before it reaches a copy — the producer's `bytes` field lives inside the seqlock-protected region and may be arbitrary while a write is in flight.
- **[The filter hangs a host application]** → Decision 3: `FillBuffer` has no waiting path at all. Worth stating as a rule the code is reviewed against, because the natural way to write a media filter is to block until a frame arrives.
- **[Scaling costs CPU in the host process]** → Bypassed entirely when the geometries match, which is the default configuration on both ends. Bounded and measurable when it does run.
- **[Registration succeeds for one architecture and the user believes they are done]** → Already handled on the desktop side: `FilterAvailability::OneArchitecture` is a distinct state with its own message naming the remaining command. This change must not weaken it by, for example, registering both from one command.
- **[The vendored `strmbase` drifts or is patched locally without record]** → Origin, commit, and licence recorded beside it; local modifications, if any prove necessary, kept as separate patches rather than edits in place.
- **[Coverage is broad but not universal]** → Accepted in decision 3 of `docs/design.windows.md`. Documented for users the same way the `v4l2loopback` prerequisite is on Linux.

## Migration Plan

No migration. The desktop currently refuses the camera on every Windows machine and names a command that installs nothing; after this change the same command installs a filter and the same code path proceeds. There is no state to migrate and no released Windows build to be compatible with.

Rollback is unregistration: `regsvr32 /u` for each architecture returns the machine to refusing the stream with the absent-component hint, which is the current behaviour.

## Open Questions

- Whether the placeholder should carry any text ("UnifiedStream — not connected") or be a plain field of colour. Text means a font, a rasteriser, and a localisation question inside a component that runs in other applications' processes; a plain field means the user gets no explanation from the picture itself. Deferred because it changes neither the specs, the transport, nor the task breakdown — the placeholder is generated behind one function either way.
- Whether the conformance tool should also be run on a developer workstation via a `Makefile` target, or left as a CI-only artifact. Cosmetic; decided when the target is written.
