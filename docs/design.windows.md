# Windows architecture decisions

Status: accepted; W1 to W3 implemented.

This document records the architecture decided for the Windows port of the UnifiedStream desktop application, and the reasoning behind each choice. It is a living document that spans several OpenSpec changes rather than the design of any one of them: an OpenSpec `design.md` is archived with its change, and these decisions must stay visible while the Windows work is carried out.

The first of those changes, `decouple-platform-integrations`, implements no Windows code. It reshapes the seam between the desktop application layer and its platform integrations so that everything below can be added without rewriting the application layer. Read that change's `design.md` for the seam itself; read this document for what the seam is shaped to accommodate.

## Constraints

The product constraint that drives every decision below:

> The user installs one application. The application does everything else. Requesting administrator rights during installation is acceptable. Requiring the user to install a separate third-party product, or to configure anything after installation, is not.

Two consequences follow immediately.

The constraint is about the user's experience, not about the origin of the code. Bundling a component authored by someone else inside the installer satisfies it; telling the user to go and install that component does not. This distinction matters for the microphone.

The constraint is also stricter than the current Linux behavior, where the user runs `modprobe` for the virtual camera and may switch the default output device by hand. Windows will therefore have a lower-friction first run than Linux does. That asymmetry is intentional and is not a reason to lower the Windows target.

## Platform capability map

The three media features do not have the same shape on Windows as they do on Linux.

| Feature | Direction | Linux | Windows | Kernel-mode? |
| --- | --- | --- | --- | --- |
| Camera | phone to PC | `v4l2loopback` kernel module | DirectShow COM filter | No |
| Microphone | phone to PC | PipeWire virtual source | Bundled VB-CABLE, rendered into over WASAPI | No, not ours |
| Speaker | PC to phone | PipeWire virtual sink plus default-output takeover | WASAPI loopback capture | No |

## Decisions

### 1. Keep the kernel-mode surface minimal

Only functionality that provably cannot be implemented in user mode goes into a driver. Every other integration stays in user mode even when a driver is already being shipped for a different feature and adding to it would appear to cost nothing.

A fault in user-mode code fails one process. A fault in kernel-mode code fails the machine. The asymmetry is large enough that a measurable amount of extra user-mode work is worth it to avoid an equivalent amount of kernel-mode work, and the marginal cost of extending an existing driver is a poor guide to the marginal risk.

Applied below, this rule originally put exactly one feature — the microphone — in the kernel. Decision 4 has since taken it out, by bundling someone else's already-signed driver rather than writing one. The rule now holds in its strongest form: **this project ships no kernel-mode code it wrote.** That is a better outcome than the rule was expected to produce, and it was reached by re-reading the product constraint rather than by relaxing the rule.

### 2. Speaker: WASAPI loopback capture, and no routing concept

The desktop captures the default render endpoint directly using `IAudioClient` with `AUDCLNT_STREAMFLAGS_LOOPBACK`. It creates no virtual device, requires no driver, and never changes the user's selected output device.

This is the point where the Windows design is not merely a different implementation of the Linux one but a better user experience. On Linux the desktop must create a null sink and make it the system default output in order to capture system audio, which means remembering the user's previous device, restoring it on stop, and repairing a stale takeover after an unclean exit. On Windows none of that exists: audio keeps playing on the user's real speakers while it is also captured, and the user makes no choice at all.

This is why the `decouple-platform-integrations` change models system audio routing as an **optional** capability (`Option<Box<dyn AudioRouting>>`) rather than as a trait every platform implements. A Windows implementation returning "unsupported" would leave a routing toggle in the UI that always fails and a `routed: false` field that means nothing. Absence must be representable.

Two implementation details are mandatory rather than optional, and are easy to miss:

- **Silent keep-alive stream.** Loopback capture on an idle render endpoint delivers no packets — not silence, nothing. Without mitigation the phone would receive a stalled stream whenever the PC is quiet, and the jitter buffer would have no frames to consume. The desktop must therefore open an additional render client on the same endpoint and play silence continuously for the lifetime of the speaker stream, keeping the audio engine running.
- **Default-device change notifications.** The user may change the output device while the stream is live. The capture must follow via `IMMNotificationClient` and re-open on the new endpoint rather than continuing to capture a device nothing plays to.
- **Mix-format conversion is the desktop's job.** `GetMixFormat` decides the format, not the application: shared mode gives whatever the endpoint's mixer runs at, commonly 32-bit float, at whatever rate the user set in the Sound control panel, with as many channels as the endpoint has. The negotiated wire format is fixed at PCM S16LE 48 kHz stereo, so downmixing, resampling, and float-to-integer conversion all happen on the desktop. The alternative — renegotiating the stream at the endpoint's rate — was rejected: it pushes rate handling into the protocol and the Android application to avoid a resampler on one platform's desktop.

### 3. Camera: a user-mode DirectShow filter, not an AVStream driver

The virtual camera is a DirectShow source filter shipped as a COM DLL, registered by the installer.

The alternative considered was extending the audio driver of decision 4 into a full AVStream (kernel streaming) capture driver covering both audio and video. That option is genuinely attractive on paper: it gives universal application support including UWP and Media Foundation-only applications, it uses a single INF-based install mechanism for everything, and the code-signing cost of decision 5 is being paid anyway, so the video half appears to be free.

It is rejected under decision 1. An AVStream capture driver is among the harder things to write correctly on Windows, its failure mode is a bug check rather than an application crash, and it would be carrying the project's highest-risk component for a feature that has a working user-mode answer. Trading roughly ten per cent of application coverage for not writing a kernel-mode video capture driver is the right side of that bargain, and OBS's virtual camera demonstrates that the DirectShow approach is sufficient in practice.

`MFCreateVirtualCamera` was also considered and rejected: it is the modern, officially supported path and it works everywhere, but it requires Windows 11 build 22000 or later and an MSIX-packaged application. Dropping Windows 10 is not acceptable, and the primary development machine for this project runs Windows 10. The re-check this decision called for before the filter was written has now been made, and the answer is unchanged — see the open questions.

Five consequences must be planned for, not discovered:

- **The filter runs inside the consuming application's process.** This is the largest structural difference from `v4l2loopback`, where the desktop simply writes to a device node. When a user selects the virtual camera in a video-conferencing application, that application loads the DLL into its own address space. Frames must therefore cross a process boundary: a shared-memory ring buffer with a named event for signalling and a named mutex for the header. The desktop application is the producer; each filter instance is a consumer.
- **The desktop keeps the decoder, and the boundary carries decoded frames.** `add-windows-camera-frame-transport` settles the shape of that crossing: a versioned shared-memory ring carrying decoded I420, four page-aligned slots behind a per-slot seqlock, no event and no steady-state lock. The desktop decodes JPEG with the existing portable decoder and publishes raw planar frames, so the component that runs inside other applications' processes never contains a decoder fed by network-originated bytes — the largest crash and attack surface in this feature stays on our side of the boundary, under decision 1. It also keeps `decode_failures()` countable by the producer, which is what makes the trait's 0-fps contract enforceable on this platform. The cost is bandwidth: 1280x720 I420 at 30 fps is roughly 40 MB/s of `memcpy` between resident pages, and DirectShow consumers want an uncompressed subtype anyway.
- **Both architectures are required.** A 32-bit application can only load a 32-bit filter. The installer registers an x86 DLL and an x64 DLL.
- **Registration needs administrator rights**, and unregistration must be equally complete. `regsvr32` at install, `regsvr32 /u` at uninstall; a leftover CLSID leaves a broken camera in every application's device list.
- **The filter offers a fixed set of resolutions and scales the ring into whichever one the application picked.** 640x480, 1280x720, and 1920x1080 I420 at 30 fps, answered without consulting the ring. This looks like unnecessary work, and the obvious alternative — connect at whatever geometry the ring currently advertises — does not survive the ordinary case: at connect time there is usually no producer at all. The user opens their conferencing application, picks the camera, and only then reaches for their phone, so `OpenFileMappingW` fails, there is no header, there is no geometry, and the filter still has to answer `GetMediaType` before the graph will connect it. Decoupling the two also makes following a resolution change nearly free — the scaler's input changes and the graph never learns anything happened — where the alternative would have needed a dynamic format change mid-connection that many consuming applications handle badly or not at all. The cost is bounded, and zero in the common case where the phone's geometry already matches the connected one and the scaler is bypassed entirely.
- **Coverage is broad but not universal.** Zoom, Discord, OBS, Skype, and Chromium-based browsers enumerate DirectShow devices. UWP and Store applications, and applications that use Media Foundation exclusively, do not — the camera will not appear in their device lists at all, and there is no setting that makes it appear. This limitation is documented for users in the same way the `v4l2loopback` prerequisite is documented on Linux: see [Using camera, microphone, and speaker](usage.md) and [Troubleshooting](troubleshooting.md).

The `VideoSink` trait absorbs this without change: `push_frame` becomes a write into the shared-memory ring rather than a v4l2 `write`, and `device_label` returns the filter's registered friendly name rather than a device path. This is why the trait's device identifier is specified as an opaque platform-supplied label and not as a path.

### 4. Microphone: bundle VB-CABLE rather than write a driver

A driver is unavoidable; **writing one is not.**

WASAPI — and therefore every modern application that captures audio — enumerates only audio endpoints created by a driver. No user-mode API creates an audio input endpoint. That much has not changed.

A DirectShow audio capture filter is the tempting shortcut and does not work. Registering under `CLSID_AudioInputDeviceCategory` makes the device visible to whatever still enumerates audio through DirectShow, which in practice is nothing that matters: Zoom, Discord, browsers, and Teams all go through the MMDevice API. DirectShow remains viable for video and is effectively dead for audio. This asymmetry is the reason the camera and the microphone get different answers despite superficially being the same problem.

What changed is the reading of the product constraint. Asking the user to go and install VB-Cable is what most comparable products do and is what the constraint rules out. **Shipping VB-CABLE inside our own installer is not the same act**, and the Constraints section above says so explicitly: bundling a component authored by someone else satisfies the constraint; telling the user to fetch it does not. The user still installs one application.

So the microphone is VB-Audio's standard VB-CABLE, embedded in the UnifiedStream installer and installed silently. The desktop renders received phone audio into `CABLE Input` through ordinary WASAPI; applications capture `CABLE Output` as an ordinary microphone. No driver is written, no driver is signed, and no kernel-mode code in this product is ours.

#### What the licence requires

Confirmed against <https://vb-audio.com/Services/licensing.htm> on 2026-08-09, before any of it was relied on — decision 8's ordering rule applies to dependencies as much as to references.

Redistribution of the free donationware VB-CABLE is permitted, including silent installation from inside another installer, subject to conditions that are **product requirements, not footnotes**:

- **Standard VB-CABLE only.** VB-CABLE A+B and C+D "cannot be distributed or bundled with other product". Only the single standard cable may be shipped. If the design ever needs a second independent cable, this route does not extend to it and the decision reopens.
- **The end user must be able to see and identify VB-CABLE as a VB-Audio application.**
- **The end user must be in a position to donate.** The licence's words: the user "must be in a position to donate/pay license if finding it useful". A credit line the user cannot act on does not satisfy this.
- **The origin and the donationware nature must both be communicated**, naming `www.vb-cable.com`.

A bare "powered by VB-CABLE" credit meets the second condition and misses the third and fourth. The attribution shown in About/Settings must therefore name VB-Audio, say the word *donationware*, and carry a reachable link — for example:

> Virtual microphone provided by **VB-CABLE**, a donationware application from VB-Audio. If you find it useful, please donate at [vb-cable.com](https://www.vb-cable.com).

VB-Audio additionally "encourage[s] distributors (if they are a significant company) to make a significant donation as licensing fee (e.g 500, 1000, 2000 USD ...)". This is an expectation on established companies rather than a condition of the grant, and it is the right thing to honour once the project has revenue. It is recorded here so it is a deliberate deferral and not an oversight.

#### What this costs

Two things, and neither is money.

**The device names are VB-Audio's, not ours.** The user's recording list shows `CABLE Output (VB-Audio Virtual Cable)`, not "UnifiedStream Microphone", and their playback list gains VB-Audio's entries. With our own driver all of them would have carried the product's name. The `AudioSink` trait already specifies its device identifier as an opaque platform-supplied label, so the UI can tell the user exactly which device to select without knowing why it is named that — the same seam that let `device_label` return a DirectShow friendly name instead of a device path.

**The extra playback endpoints are now permanent, and there is more than one.** A cable-style driver makes an extra playback device appear in the user's output list; that was already the one visible wart, and the plan was to replace it later with a capture-only variant. That escape route is gone — the driver is not ours to change.

The wart is also larger than this decision first recorded. Measured on VB-CABLE 3.3.1.7, the driver exposes **two** render endpoints, not one:

```
ROOT\MEDIA\0005   "VB-Audio Virtual Cable"
  ├── CABLE Input     render    ← the one the desktop renders into
  ├── CABLE In 16ch   render    ← a 16-channel variant, unused by this product
  └── CABLE Output    capture   ← the one applications select as a microphone
```

So the user's playback list gains two entries they did not ask for, one of which does nothing for them. The count is version-dependent — `CABLE In 16ch` is not in every VB-CABLE release — which means it is not a number this document can state once and rely on.

That has a consequence beyond cosmetics, and it is the reason this paragraph exists rather than a footnote: the two render endpoints share adapter name, INF, driver version, and topology filter, so **resolving "VB-Audio's render endpoint" is ambiguous**, and rendering into the wrong one fails silently — no error from any API, and applications capturing `CABLE Output` simply hear nothing. Borrowing a third party's driver means inheriting whatever endpoints it chooses to expose, in whatever release the user happens to have. Any implementation must resolve its destination unambiguously and refuse rather than guess; `add-windows-microphone` carries the specifics.

The wart, and the ambiguity that comes with it, are the price of not writing, signing, or maintaining a kernel driver. It is still a good trade.

**One thing this does not cost: the code above the seam.** The `AudioSink` implementation for Windows is "render PCM into a named endpoint over WASAPI", and that code is identical whether the endpoint belongs to a driver we wrote or to a third-party cable. This decision said so in its original form, in order to keep the funding question from becoming an architectural fork; that foresight is why taking option 2 is a dependency change and nothing more.

The desktop must detect that the endpoint exists and refuse the stream with guidance when it does not — a portable build, or a user who removed VB-CABLE by hand. That is the existing platform-supplied setup guidance requirement, and needs no new mechanism.

### 5. Driver signing: resolved by taking option 2 at no cost

**Status: closed.** This was the single blocking risk in the Windows plan. It is not one any more, and nothing was paid to clear it.

The problem, recorded because it is why decision 4 came out the way it did: installing a kernel-mode driver on Windows 10 1607 or later, with Secure Boot enabled, requires a driver package signed by Microsoft through attestation signing, which in turn requires an EV code-signing certificate and a Partner Center hardware account. Cross-signing with an ordinary certificate has not been viable for new drivers for years. Test signing works for development but requires the user to disable Secure Boot and reboot, which is exactly the post-installation configuration the product constraint forbids.

Three ways to clear it were identified, all satisfying the product constraint:

1. **Obtain an EV certificate and sign the project's own driver.** Roughly USD 150–300 per year including the required hardware token, plus a one-time Partner Center registration fee. Full control, and the preferred outcome when this was written.
2. **Redistribute an existing signed driver.** ← **taken**
3. **Redistribute an open-source attestation-signed driver under a permissive licence.** Free if it exists, and it may not. Never needed.

Option 2 is taken, and the reason it costs nothing is that this decision mispriced it. It was written as "VB-Audio *sells* redistribution licenses … in exchange for a licence fee", which made it look like trading a certificate bill for a licensing bill. VB-Audio's published terms grant redistribution of the standard donationware VB-CABLE — silent installation inside another installer included — against attribution obligations rather than a fee. The obligations are listed in decision 4 and are cheap to meet.

What this buys, beyond the certificate:

- **The driver-development work disappears too.** Not just the signature on `sysvad`, but building it, maintaining it, and owning its bug checks.
- **Windows is no longer feature-gated behind a purchase.** All three media features can ship.
- **Decision 1 reaches its strongest form.** No kernel-mode code in this product is ours.

What is taken on in exchange: a dependency on another vendor's release cadence and continued goodwill, the naming and extra-endpoint costs in decision 4, and an attribution requirement that must survive every future redesign of the About screen. That last one is a requirement, not a nicety — it is the consideration for the licence.

The residual signing requirement is Authenticode on our own two filter DLLs, which is ordinary code signing, not attestation, and is a W5 concern.

### 6. WiX for the installer, not the default NSIS bundler

**Reopened by decision 4; to be settled in W5.** The premise below no longer holds and the conclusion may not either.

The original reasoning: Tauri's default Windows bundler produces an NSIS installer that cannot install a driver package, so the installer becomes a WiX project performing the whole first-run setup under a single elevation prompt — install the driver via its INF, register both DirectShow filter DLLs, install the application and the WebView2 runtime, and reverse all of it on uninstall.

There is no INF to install now. VB-CABLE ships its own signed setup executable with a silent mode, so the installer's job is to *invoke* an installer rather than to *be* a driver installer. What remains is: run VB-CABLE's setup silently, `regsvr32` two DLLs, install the application and WebView2, and reverse all of it on uninstall — all under one elevation prompt. NSIS with install and uninstall hooks can plausibly do that, which would keep the Tauri bundler and delete a whole toolchain from the project.

The standing argument for deciding early — that the choice determines how release artifacts are produced and reworking it later touches every Windows change — now argues for settling it *in* W5 rather than before it, because W5 is the first phase that produces an artifact at all. Uninstall completeness is the property to judge the options against: a leftover CLSID leaves a broken camera in every application's device list, and VB-CABLE's own uninstall must be sequenced correctly rather than orphaned.

### 7. Repository layout: a `windows/` tree with its own toolchain

```text
UnifiedStream/
├── desktop/          Rust, Tauri, React
├── android/          Kotlin, Gradle
└── windows/          C++, MSBuild
    ├── dshow-camera/   COM filter, x86 and x64, Authenticode-signed
    └── installer/      bundler chosen in W5 (decision 6)
```

`audio-driver/` was planned here and will not exist: decision 4 bundles VB-CABLE instead of building a WDM miniport, so the only C++ this repository contains is the DirectShow filter. That is a real reduction in the size of this tree, not a relabelling.

The filter is a C++ project because Rust is not a realistic option for it: a DirectShow filter is built on the C++ `strmbase` base classes. (The same was true of a WDM audio miniport, which is portclass COM in C++ — `windows-drivers-rs` exists but is preview-stage and oriented toward KMDF rather than audio. Moot now.)

Cargo cannot build it. It is a separate MSBuild artifact consumed by the installer, and CI needs a Windows job that builds it independently of the Rust workspace check introduced in `decouple-platform-integrations` — the `windows-native` job, which now exists.

### 8. Licence hygiene: do not read `obs-virtualcam`

`obs-virtualcam` is the obvious reference for a DirectShow virtual camera and is licensed GPLv2. This repository had no licence file when this decision was written, so incorporating GPLv2 code would have made that decision by accident and irreversibly.

Work from `microsoft/Windows-classic-samples` for the DirectShow base classes and `microsoft/Windows-driver-samples` for `sysvad`, both MIT. Confirm the licence of any reference implementation before reading its source, not after.

The `sysvad` half of that instruction is moot after decision 4 — no driver is written, so none is read. The rule itself is not, and decision 4 is where it was applied next: VB-Audio's redistribution terms were read before the dependency was chosen, not after it was shipped. The rule generalises from *source you read* to *anything you take*.

**Resolved by decision 9: the repository publishes under MIT.** `obs-virtualcam` remains unreadable, which was true before that decision and stays true after it.

### 9. The repository publishes under MIT

`LICENSE` at the root, MIT, applying to the repository as a whole. Added by `add-windows-directshow-camera`, which is the change that first vendored third-party source and therefore the first one that had to answer "compatible with what".

Every third-party **source** the Windows work depends on is MIT — and after decision 4 that is just `strmbase` from `Windows-classic-samples` in W3, since `sysvad` is no longer vendored.

The clause that mattered was the other one. This decision recorded that MIT "leaves every route in decision 5 open: bundling VB-Audio's proprietary signed driver under a redistribution licence is a live option for the microphone, and a copyleft licence here would have foreclosed it while the funding decision is still unmade." That live option is now the chosen one. A copyleft licence chosen here would have cost the project the free route to its third media feature, and the cost would have been discovered a phase later, when it was unpayable.

Note what the dependency actually is, because it is a new category for this repository: VB-CABLE is redistributed as a **signed binary under a grant**, not vendored as source under a permissive licence. Nothing of it enters this repository's source tree, MIT does not extend to it, and it carries obligations of its own — enumerated in decision 4 — that no other dependency here imposes.

The ordering is the part worth keeping: a licence checked after the fact cannot be acted on, because the knowledge cannot be given back. `repository-quality-gates` states this as a checkable requirement rather than leaving it in this document alone.

## Risks

- ~~**[Driver signing is never funded]**~~ → Retired by decision 5. No certificate is needed and no driver is written.
- **[VB-Audio withdraws or changes the redistribution terms]** → The microphone stops shipping on Windows; the camera and the speaker are unaffected, exactly as under the retired risk above. The fallback is the original plan — write and sign a `sysvad`-derived driver — which is why decisions 4 and 5 keep the reasoning for it rather than deleting it. `AudioSink` does not change either way. Pin and archive the exact redistributed VB-CABLE installer alongside the release rather than fetching it at build time, so a change upstream cannot retroactively alter what a shipped installer contains.
- **[The attribution requirement is lost in a later redesign]** → The licence is granted against attribution, so dropping it from About/Settings converts a compliant product into an infringing one silently, with no build failure to catch it. It belongs in a spec as a checkable requirement, not only in this document.
- **[The DirectShow filter is invisible in an application users care about]** → Documented as a known limitation, as the `v4l2loopback` prerequisite is on Linux. The upgrade path to Media Foundation or AVStream exists and is contained behind `VideoSink`.
- **[Shared-memory frame transport across a process boundary is subtle]** → Single producer, multiple consumers, no consumer trusted to be alive. Sequence-numbered slots, no blocking wait on a consumer, and drop rather than stall — the same lossy-by-design policy the existing transport and jitter buffer already use.
- **[A kernel-mode fault reaches users]** → Decision 4 removed the last kernel-mode component this project would have written. The remaining kernel code is VB-Audio's, already signed and already deployed on a large installed base.
- ~~**[Windows development requires Secure Boot to be disabled on the development machine]**~~ → Retired with the driver. No phase test-signs anything.
- **[The C++ subprojects diverge from the Rust workspace]** → CI builds them on every pull request once they exist, and the shared-memory protocol between the filter and the desktop is versioned with an explicit header field so a stale filter refuses rather than misreads.

## Phase plan

| Phase | Scope | Kernel? | Needs signing? |
| --- | --- | --- | --- |
| W1 | Decouple platform integrations: traits, factories, unsupported fallbacks, Windows CI leg — **implemented** | No | No |
| W2 | Speaker over WASAPI loopback, with silent keep-alive and device-change following — **implemented** | No | No |
| W3 | Camera over the DirectShow filter, with the shared-memory frame transport and manual `regsvr32` registration — **implemented** | No | No |
| W4 | Microphone by rendering into VB-CABLE's endpoint over WASAPI, with the attribution it requires — **implemented**; bundling it is W5 | No, not ours | No |
| W5 | Windows release: installer, Authenticode on the filter DLLs, release workflow, versioned artifacts, user documentation, and a Windows smoke test | No | Authenticode only |

No phase needs a certificate this project must buy, and no phase writes kernel-mode code. That was not true when this plan was first written; decision 4 made it true.

**W5 is not dependent on W4 — it is only sequenced after it.** Packaging the camera and the speaker requires nothing from the microphone, and while decision 5 was open the ordering mattered a great deal: with W5 behind a phase blocked on a purchase, "the decision can be made against a working product" would have been false, because no product would ever have reached a user. Decision 5 is closed and W4 is cheap, so the ordering now costs little. The independence is recorded because it is the escape hatch if W4 stalls for any reason — a terms change, an integration surprise — and because a reader should not have to re-derive it.

Two things follow for W5 regardless of ordering:

- **The installer bundler is chosen there**, not before (decision 6, reopened).
- **The Windows smoke test is written there.** CI cannot cover this: the runners have no audio or video device, which the `windows-native` job states as a standing constraint on what may be added to the suite. WASAPI loopback against a real endpoint, the silent keep-alive, default-device-change following, the filter opening in Zoom, Discord, and Chrome, and VB-CABLE appearing as a microphone are all verified by hand or not at all. The Linux MVP has `docs/release-smoke-test.md`; Windows has no equivalent, and shipping without one would make the first release the first test.

W1, W2, and W3 have OpenSpec changes (`decouple-platform-integrations`, `add-windows-speaker-capture`, then `add-windows-camera-frame-transport` and `add-windows-directshow-camera`). W4 and W5 are named here so the sequencing is on the record; each needs its own proposal.

Earlier proposals defer packaging to "W4" — see the non-goals of `add-windows-speaker-capture`, `add-windows-camera-frame-transport`, and `add-windows-directshow-camera`. Those are archived and correct as written; the phase they meant is now W5.

W3 is delivered as two changes, because the camera is the only feature whose frames leave the desktop's address space and the crossing is code that fails silently — a torn read is a corrupt frame, not a crash:

- `add-windows-camera-frame-transport` builds and proves the transport alone, in Rust, where the existing test suite already runs on both CI legs. It ships the versioned shared-memory contract, the Windows section and its security descriptor, the availability probe, and a `VideoSink` that refuses the stream while no filter is registered and names the command that installs one.
- `add-windows-directshow-camera` writes the C++ filter that consumes it, against a contract that is already specified and already tested, and brings with it the `windows/` tree, MSBuild, the native CI area, and the registration tooling. It is the second half, and with it W3 is complete.

Writing the ring at the same time as the first C++ in this repository, a new toolchain, and a new CI area would have meant debugging all of them against each other.

## Open questions

- Which installer bundler W5 uses, now that there is no INF to install (decision 6, reopened). Judge the options on uninstall completeness.
- How VB-CABLE's presence is detected, and what the desktop shows when it is absent. The mechanism exists — platform-supplied setup guidance — but the failure is unusual: the component is normally installed by our own installer, so its absence means someone removed it or is running an unpackaged build.
- Which endpoint property distinguishes `CABLE Input` from the driver's other render endpoints across VB-CABLE releases. Only 3.3.1.7 has been examined, and the rule `add-windows-microphone` adopts is provisional. It will stay provisional: VB-Audio publishes the current release only, so there is no supported way to measure the rule against the versions users actually have. The answer is to make a wrong rule refuse loudly rather than to make the rule certain, and a report from a real machine is what reopens this.
- At what point the project makes the "significant donation as licensing fee" VB-Audio encourages. Not a condition of the grant, deferred until there is revenue, and recorded so the deferral stays deliberate.

Closed:

- ~~Which of the three signing options in decision 5 is taken, and by when.~~ Option 2, at no cost. See decision 5.
- ~~Whether the cable-style driver's extra playback endpoint can be hidden from the user's output device list, or whether the capture-only variant is required to achieve that.~~ Moot, and not in the way it was expected to be: the driver is no longer ours, so neither hiding the endpoint nor switching to a capture-only variant is available. The extra endpoint is permanent, and decision 4 accepts it as the price.

- ~~Whether Windows 10 support is still required at the time W3 is implemented.~~ Re-checked when `add-windows-camera-frame-transport` was written, and decision 3 stands unchanged. The development machine for this project runs Windows 10, so `MFCreateVirtualCamera` is not merely a platform floor to raise but a path that cannot be exercised at all here; it would additionally require repackaging the application as MSIX, which decision 6 does not plan for. The DirectShow filter is the path.
