# Windows architecture decisions

Status: accepted; W1 and W2 implemented.

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
| Microphone | phone to PC | PipeWire virtual source | Own WDM audio driver | **Yes** |
| Speaker | PC to phone | PipeWire virtual sink plus default-output takeover | WASAPI loopback capture | No |

## Decisions

### 1. Keep the kernel-mode surface minimal

Only functionality that provably cannot be implemented in user mode goes into a driver. Every other integration stays in user mode even when a driver is already being shipped for a different feature and adding to it would appear to cost nothing.

A fault in user-mode code fails one process. A fault in kernel-mode code fails the machine. The asymmetry is large enough that a measurable amount of extra user-mode work is worth it to avoid an equivalent amount of kernel-mode work, and the marginal cost of extending an existing driver is a poor guide to the marginal risk.

Applied below, this rule puts exactly one feature — the microphone — in the kernel, and keeps the camera and speaker out of it.

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

`MFCreateVirtualCamera` was also considered and rejected: it is the modern, officially supported path and it works everywhere, but it requires Windows 11 build 22000 or later and an MSIX-packaged application. Dropping Windows 10 is not acceptable, and the primary development machine for this project runs Windows 10.

Four consequences must be planned for, not discovered:

- **The filter runs inside the consuming application's process.** This is the largest structural difference from `v4l2loopback`, where the desktop simply writes to a device node. When a user selects the virtual camera in a video-conferencing application, that application loads the DLL into its own address space. Frames must therefore cross a process boundary: a shared-memory ring buffer with a named event for signalling and a named mutex for the header. The desktop application is the producer; each filter instance is a consumer.
- **Both architectures are required.** A 32-bit application can only load a 32-bit filter. The installer registers an x86 DLL and an x64 DLL.
- **Registration needs administrator rights**, and unregistration must be equally complete. `regsvr32` at install, `regsvr32 /u` at uninstall; a leftover CLSID leaves a broken camera in every application's device list.
- **Coverage is broad but not universal.** Zoom, Discord, OBS, Skype, and Chromium-based browsers enumerate DirectShow devices. UWP and Store applications, and applications that use Media Foundation exclusively, do not. This limitation is documented for users in the same way the `v4l2loopback` prerequisite is documented on Linux.

The `VideoSink` trait absorbs this without change: `push_frame` becomes a write into the shared-memory ring rather than a v4l2 `write`, and `device_label` returns the filter's registered friendly name rather than a device path. This is why the trait's device identifier is specified as an opaque platform-supplied label and not as a path.

### 4. Microphone: the project's own WDM audio driver

There is no way to avoid this one. WASAPI — and therefore every modern application that captures audio — enumerates only audio endpoints created by a driver. No user-mode API creates an audio input endpoint.

A DirectShow audio capture filter is the tempting shortcut and does not work. Registering under `CLSID_AudioInputDeviceCategory` makes the device visible to whatever still enumerates audio through DirectShow, which in practice is nothing that matters: Zoom, Discord, browsers, and Teams all go through the MMDevice API. DirectShow remains viable for video and is effectively dead for audio. This asymmetry is the reason the camera and the microphone get different answers despite superficially being the same problem.

Depending on VB-Cable, Virtual Audio Cable, or VoiceMeeter and asking the user to install it is what most comparable products do, and it is what the product constraint rules out.

The driver is therefore built from Microsoft's `sysvad` sample (`microsoft/Windows-driver-samples`, MIT), as a **cable-style** driver exposing a paired render and capture endpoint. The desktop renders received phone audio into the render half through ordinary WASAPI; applications capture the capture half as an ordinary microphone.

A capture-only driver fed through a private IOCTL interface was considered and deferred. It is the cleaner result — a cable-style driver makes an extra playback device appear in the user's output list, which is the one visible wart in this design — but it requires more driver-side work and a private control channel, and it can replace the cable-style driver later without changing anything above the `AudioSink` trait. Cable style first; capture-only as polish.

The `AudioSink` implementation for Windows is "render PCM into a named endpoint over WASAPI". That code is identical whether the endpoint belongs to this project's driver or to a third-party cable, which keeps the funding question in decision 5 from being an architectural fork.

### 5. Driver signing is a funding decision, not an architectural one

Installing a kernel-mode driver on Windows 10 1607 or later, with Secure Boot enabled, requires a driver package signed by Microsoft through attestation signing, which in turn requires an EV code-signing certificate and a Partner Center hardware account. Cross-signing with an ordinary certificate has not been viable for new drivers for years. Test signing works for development but requires the user to disable Secure Boot and reboot, which is exactly the post-installation configuration the product constraint forbids.

This is the single blocking risk in the Windows plan, and it is commercial rather than technical. Three ways to clear it, all of which satisfy the product constraint:

1. **Obtain an EV certificate and sign the project's own driver.** Roughly USD 150–300 per year including the required hardware token, plus a one-time Partner Center registration fee. Certificate authorities that issue to individuals and sole proprietors exist, so a registered company is not necessarily a prerequisite; this should be confirmed for the relevant jurisdiction before the option is priced. Gives full control and is the preferred outcome.
2. **License redistribution rights for an existing signed driver.** VB-Audio sells redistribution licenses for VB-Cable. The driver is bundled inside this project's installer, so the user still installs one application. Removes the certificate requirement entirely and removes the driver-development work with it, in exchange for a licence fee and a dependency on another vendor's release cadence.
3. **Redistribute an open-source virtual audio driver that already ships an attestation-signed binary under a permissive licence.** This would be free, and it may not exist. Treat it as a spike with a definite answer required — licence text and signature both verified — and not as an assumption.

Until one of these is chosen, the microphone cannot ship on Windows. The camera and the speaker can, because neither requires a driver, which is why the phase plan below reaches a usable Windows build before any of this is decided.

### 6. WiX for the installer, not the default NSIS bundler

Tauri's default Windows bundler produces an NSIS installer that cannot install a driver package. The installer becomes a WiX project that performs the whole first-run setup under a single elevation prompt: install the driver via its INF, register both DirectShow filter DLLs, install the application and the WebView2 runtime, and reverse all of it on uninstall.

This is worth deciding early because it also determines how release artifacts are produced and how the release workflow is structured, and reworking it after the fact would touch every Windows change.

### 7. Repository layout: a `windows/` tree with its own toolchain

```text
UnifiedStream/
├── desktop/          Rust, Tauri, React
├── android/          Kotlin, Gradle
└── windows/          C++, MSBuild
    ├── dshow-camera/   COM filter, x86 and x64, Authenticode-signed
    ├── audio-driver/   WDM audio driver, INF, attestation-signed
    └── installer/      WiX
```

The filter and the driver are C++ projects. Rust is not a realistic option for either: a DirectShow filter is built on the C++ `strmbase` base classes, and a WDM audio miniport is portclass COM in C++. `windows-drivers-rs` exists but is preview-stage and oriented toward KMDF rather than audio.

Cargo cannot build these. They are separate MSBuild artifacts consumed by the installer, and CI needs a Windows job that builds them independently of the Rust workspace check introduced in `decouple-platform-integrations`.

### 8. Licence hygiene: do not read `obs-virtualcam`

`obs-virtualcam` is the obvious reference for a DirectShow virtual camera and is licensed GPLv2. This repository has no licence file yet, so incorporating GPLv2 code would make that decision by accident and irreversibly.

Work from `microsoft/Windows-classic-samples` for the DirectShow base classes and `microsoft/Windows-driver-samples` for `sysvad`, both MIT. Confirm the licence of any reference implementation before reading its source, not after.

## Risks

- **[Driver signing is never funded]** → The microphone does not ship on Windows. The camera and the speaker are unaffected, and the phase plan is ordered so that this is discovered with a working two-feature build in hand rather than at the end.
- **[The DirectShow filter is invisible in an application users care about]** → Documented as a known limitation, as the `v4l2loopback` prerequisite is on Linux. The upgrade path to Media Foundation or AVStream exists and is contained behind `VideoSink`.
- **[Shared-memory frame transport across a process boundary is subtle]** → Single producer, multiple consumers, no consumer trusted to be alive. Sequence-numbered slots, no blocking wait on a consumer, and drop rather than stall — the same lossy-by-design policy the existing transport and jitter buffer already use.
- **[A kernel-mode fault reaches users]** → Decision 1 keeps exactly one component in the kernel, and it is built from a Microsoft sample rather than written from scratch.
- **[Windows development requires Secure Boot to be disabled on the development machine]** → True while test signing, and it applies to the driver work only. Phases W1 to W3 need no test signing.
- **[The C++ subprojects diverge from the Rust workspace]** → CI builds them on every pull request once they exist, and the shared-memory protocol between the filter and the desktop is versioned with an explicit header field so a stale filter refuses rather than misreads.

## Phase plan

| Phase | Scope | Kernel? | Needs signing? |
| --- | --- | --- | --- |
| W1 | Decouple platform integrations: traits, factories, unsupported fallbacks, Windows CI leg | No | No |
| W2 | Speaker over WASAPI loopback, with silent keep-alive and device-change following — **implemented** | No | No |
| W3 | Camera over the DirectShow filter, with the shared-memory frame transport and installer registration | No | Authenticode only |
| W4 | Microphone over the project's own WDM audio driver, with the WiX installer | **Yes** | **Yes** |

W1 through W3 produce a Windows build that is genuinely useful — a wireless speaker and a virtual webcam — without spending anything on certificates. The decision in section 5 can therefore be made against a working product rather than against a plan.

W1 and W2 have OpenSpec changes (`decouple-platform-integrations` and `add-windows-speaker-capture`). W3 and W4 are named here so the sequencing is on the record; each needs its own proposal.

## Open questions

- Which of the three signing options in decision 5 is taken, and by when. Nothing in W1 to W3 depends on the answer.
- Whether the cable-style driver's extra playback endpoint can be hidden from the user's output device list, or whether the capture-only variant is required to achieve that. Deferred to W4.
- Whether Windows 10 support is still required at the time W3 is implemented. If it is not, `MFCreateVirtualCamera` becomes available and decision 3 should be revisited before the DirectShow filter is written rather than after.
