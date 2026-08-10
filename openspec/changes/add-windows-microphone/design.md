## Context

`unifiedstream-audio` already contains everything this change needs except the sink itself. `platform/windows.rs` holds a working WASAPI implementation — the loopback capture behind the speaker — so COM threading, MMCSS registration, mix-format reading, endpoint invalidation, and the `Session`-open-report-pump-teardown shape are all established in the same file the sink lands in. `AudioConverter` runs endpoint format to wire format. `JitterBuffer` is what the sink drains. The `AudioSink` trait, its lifecycle, and its error type are unchanged since W1.

Two things about the destination make this not simply the capture read backwards.

The endpoint is **not the default**, and must not be. Every WASAPI path in this repository so far resolves `GetDefaultAudioEndpoint(eRender, eConsole)` and follows it when the user changes it. This one resolves a specific device and must keep rendering into it when the default changes underneath.

The endpoint is **not ours**, and exists whether or not a stream does. VB-CABLE's driver creates a paired render and capture endpoint at install time. There is no device to create on start or destroy on stop, which is the assumption `microphone-stream` was written under and which this change's delta removes.

## Goals / Non-Goals

**Goals:**

- A `Box<dyn AudioSink>` for Windows that renders the jitter buffer's decoded PCM into VB-CABLE's render endpoint, with the same observable lifecycle the PipeWire sink presents.
- Resolution of that endpoint by identity, such that no arrangement of the user's default-output setting can cause received microphone audio to be rendered anywhere else.
- A refusal with actionable guidance when the endpoint cannot be resolved, distinguishable from the feature being unimplemented on the platform.
- An attribution surface that satisfies VB-CABLE's grant, and an automated check that keeps satisfying it.

**Non-Goals:**

- Bundling VB-CABLE, choosing the installer bundler, Authenticode signing, and the release workflow. All W5.
- Any change to the wire protocol, the Android application, `AudioSink`, or the application layer.
- Hiding VB-CABLE's extra playback endpoint from the user's output list. Not possible — the driver is not ours (decision 4 of `docs/design.windows.md`).
- Supporting virtual audio cables other than VB-CABLE. The grant covers standard VB-CABLE specifically; a second vendor is a different licence question and a different change.

## Decisions

### Two converters, not one bidirectional converter

`AudioConverter` stays as it is — endpoint format to wire format, for the speaker — and the sink gets its own converter for wire format to endpoint format.

The tempting alternative is one type that runs both ways, since the transformations look symmetric: resample, change channel count, change sample type. They are symmetric; the code around them is not. The capture side converts a packet WASAPI hands it, of a size WASAPI chose, and pushes the result into a `FrameChunker` that cuts it into wire frames. The sink side is asked for a specific number of frames by the audio engine, must produce exactly that many, and must decide what to emit when the jitter buffer cannot supply them. Buffer padding, render cadence, and underrun policy have no counterpart on the capture side, and folding both directions into one type means a type whose behaviour depends on which direction it was constructed for — the shape that reads as one abstraction and tests as two.

The concrete asymmetries the sink owns alone:

- **Underrun.** `microphone-stream` requires silence on underrun and continuous playback afterwards. The capture has no such state; a packet either exists or the loop moves on.
- **Cadence.** The renderer must supply the engine on time, every period, indefinitely. The capture drains whatever has accumulated and sleeps.
- **Upmix.** The microphone wire format is mono (`microphone-stream`: 48 kHz, mono, 16-bit). The endpoint is almost always stereo. Mono to N channels is a distribution decision the capture's N-to-stereo downmix does not answer.

Shared numeric helpers — resampling, sample-type conversion — may be factored out and used by both. That is code reuse below the two types, not one type with a direction flag.

### Resolve the endpoint by identity, and never fall back

The sink enumerates render endpoints and selects VB-CABLE's by matching **two** properties — one identifying the driver, one identifying which of that driver's render endpoints is the cable's input — then holds that device for the life of the stream.

Two, because **VB-CABLE exposes more than one render endpoint.** Verified against an installed VB-CABLE 3.3.1.7 on the development machine, 2026-08-10:

```
ROOT\MEDIA\0005   "VB-Audio Virtual Cable"   VBAudioVACWDM / VB-Audio Software
  ├── CABLE Input     render    …vbaudiovacwdm2022_out1_topo_…/00010001   ← the destination
  ├── CABLE In 16ch   render    …vbaudiovacwdm2022_out1_topo_…/00010002   ← same driver, wrong endpoint
  └── CABLE Output    capture                                             ← what applications select
```

`CABLE In 16ch` carries the same adapter name, the same INF, the same driver version, and the same topology filter as `CABLE Input`; only the trailing pin index and the device description separate them. Anything resolving "the VB-Audio render endpoint" matches both, and MMDevice specifies no enumeration order. Picking the wrong one **fails silently**: no API returns an error, `CABLE Output` simply stays quiet, and what reaches a user is "the microphone connects and nobody can hear me". It is also version-dependent — the 16ch endpoint is not present in every VB-CABLE release — so a resolver that picks correctly on one machine can pick wrongly on another with no code change in between. That is the failure mode this decision exists to prevent, and it is not the one the earlier draft was guarding against.

What is actually available to match on, measured rather than assumed:

| Property | `CABLE Input` on 3.3.1.7 | `CABLE In 16ch` on 3.3.1.7 | Stable across installs? | User-editable? |
| --- | --- | --- | --- | --- |
| Endpoint ID | `{0.0.0.00000000}.{B5E55CC9-…}` | `…{2BA58530-…}` | **No** — GUID minted at install time | No |
| `PKEY_Device_FriendlyName` | `CABLE Input (VB-Audio Virtual Cable)` | `CABLE In 16ch (…)` | Yes | **Yes** |
| `PKEY_Device_DeviceDesc` | `CABLE Input` | `CABLE In 16ch` | Yes | **Yes** — rename writes here |
| `PKEY_DeviceInterface_FriendlyName` | `VB-Audio Virtual Cable` | **identical** | Yes | No — comes from the INF |
| Driver identity (`{83da6326-…},3`) | `oem13.inf:…:VBCableInst.NTamd64:3.3.1.7:VBAudioVACWDM` | **identical** | Partly — `oem13` is machine-local | No |
| `PKEY_AudioEngine_DeviceFormat` channels | 2 | **2** | Yes | Yes — its control panel sets it |
| `GetMixFormat` | 2ch 48 kHz float | **2ch 48 kHz float** | Yes | Yes — follows the above |
| Jack subtype (`{1da5d803-…},8`) | `KSNODETYPE_SPEAKER` | `KSNODETYPE_LINE_CONNECTOR` | Yes | No — from the driver's topology |
| KS filter name (`{840b8171-b0ad-…},0`) | `…\vbaudiovacwdm2022_out1_topo_…/00010001` | `…/00010002` | Yes | No |

**The endpoint ID is not an identity.** An earlier draft of this decision asserted that the endpoint ID "carries the driver's hardware identifier, which changes only when VB-Audio ships a different driver", and matched on it. That is false. The GUID is generated when the endpoint is first enumerated on a given machine; the hardware identifier `VBAudioVACWDM` lives in an unrelated property. An endpoint ID remains a valid handle for re-opening a device *within one installation* — it is what `IMMDeviceEnumerator::GetDevice` takes, and it is what the feedback-loop guard compares two of — but it can never be a constant in this repository, and no task may be written as though it could.

**Friendly names are unusable for a different reason than the earlier draft gave.** Renaming a device in the Sound control panel writes the new text into `PKEY_Device_DeviceDesc`; `PKEY_Device_FriendlyName` is composed as `DeviceDesc (adapter name)`. So a rename moves both, and neither is a match key. `PKEY_DeviceInterface_FriendlyName` — the adapter name — is the part the rename does not touch, because it comes from the INF rather than from the endpoint.

**The channel count does not discriminate, and an earlier draft of this decision was wrong to say it did.** Measured on 3.3.1.7 rather than assumed: `CABLE In 16ch` declares **two** channels in `PKEY_AudioEngine_DeviceFormat` and reports **two** from `GetMixFormat`, exactly as `CABLE Input` does. The "16ch" in its name describes what its filter can be configured for, not what the endpoint currently declares, and a rule that rejects candidates wider than stereo therefore rejects neither of them. That draft's rule did not pick the wrong endpoint — the refusal rule below caught it — but it made the feature unresolvable on every machine carrying this release, which is a different failure and an equally real one.

What does separate them, and is neither user-editable nor per-install, is the **jack subtype** the driver's topology gives each endpoint: `CABLE Input` presents itself as a `KSNODETYPE_SPEAKER` and `CABLE In 16ch` as a `KSNODETYPE_LINE_CONNECTOR`. That is the property that says *what the endpoint is for* rather than *where it happened to land in an enumeration*, which is what the channel count was reached for and failed to be.

So the match is:

1. **Family**, from `PKEY_DeviceInterface_FriendlyName` equal to VB-CABLE's adapter name, corroborated where readable by the hardware identifier `VBAudioVACWDM` in the driver-identity property. The adapter name is a documented PKEY; the driver-identity property is not, so it strengthens a match and must not be the only thing a match depends on.
2. **Which endpoint**, by rejecting candidates whose jack subtype is not `KSNODETYPE_SPEAKER`, and additionally rejecting any whose device format declares more than two channels. The second rule discriminates nothing on 3.3.1.7 and is kept anyway: it costs one comparison, and a future release that does expose a genuinely wide endpoint is exactly the case it was reached for.

The KS filter name's trailing pin index (`/00010001` against `/00010002`) also separates them exactly, and was **rejected as a match key**: choosing the lower index is choosing by position within the driver's topology, which is the thing `microphone-stream` forbids and for the same reason it forbids enumeration order — nothing guarantees the destination is the earlier pin in a release nobody has seen.

The resolver builds a candidate list and **refuses on anything other than exactly one survivor** — zero is "not installed", more than one is ambiguity that must be reported rather than guessed through. It logs every candidate it considered and why each was rejected, so a wrong pick on a VB-CABLE version this project has not seen is diagnosable from a user's log instead of invisible. Guessing here is the same class of mistake as falling back to the default output, and gets the same answer.

The friendly name stays as display text only — it is what the UI shows the user so they know which device to select, which is precisely the opaque platform-supplied label `microphone-stream` now requires.

**The discrimination is provisional, and deliberately not blocked on.** Only 3.3.1.7 has been examined, and whether the rules hold across VB-CABLE releases cannot be settled by measurement: VB-Audio publishes the current release only, older ones have no official source, and fetching a kernel-mode driver from a third-party mirror to verify a heuristic is a worse trade than leaving the heuristic unverified.

What makes that acceptable is the refusal rule above, not optimism. A release that breaks the discrimination produces zero candidates or several, and both are refusals with a log naming what was considered. The design's job here is not to be right about every VB-CABLE version — it cannot be — but to fail loudly on the versions it is wrong about. The channel rule proved the point on the very release it was written against, and the failure it produced was a refusal with both candidates named. Measure the current release, state in the resolver's comment which version the rules rest on, and let a real report from a real machine be what reopens this.

**No fallback, on any failure.** If the endpoint cannot be resolved, the stream is refused. This is stated as a requirement rather than left to implementation because the natural defensive reflex — "fall back to the default output so something works" — produces the exact failure the feature must not have: the user's voice played aloud on their own speakers, and fed back into the phone if the speaker stream is also running. The same reasoning governs recovery: `AUDCLNT_E_DEVICE_INVALIDATED` mid-stream fails the stream visibly rather than re-resolving, because the only thing worth re-resolving to is the device that just went away.

This is where the sink deliberately diverges from its sibling in the same module. The loopback capture re-opens on device change and retries twenty times, because following the user's choice is its whole job. The sink must not follow anything. A reader who notices the inconsistency and "fixes" it would introduce the bug this paragraph exists to prevent.

### Event-driven rendering, not polling

The sink initializes with `AUDCLNT_STREAMFLAGS_EVENTCALLBACK` and waits on the event, rather than polling on a timer as the capture does.

The capture polls because it has no choice: loopback capture does not combine with event callbacks, which is recorded in `platform/windows.rs` as one of three load-bearing facts about that path. That constraint does not apply to an ordinary render client. Event-driven is the better pattern for a renderer — it wakes exactly when the engine needs data, rather than at a guessed interval that is either wasteful or late — and choosing it here is not an inconsistency to reconcile with the capture but the absence of a constraint the capture is subject to.

Shared mode, not exclusive: exclusive mode would fail whenever anything else holds the endpoint and would bypass the mixer for no benefit, and the mix format conversion is happening regardless.

### Availability is probed before the stream starts

Resolution runs at `start`, and its three outcomes are distinguished rather than collapsed, because they have different remedies:

- **No VB-CABLE endpoint at all.** The component is not installed. After W5 this means an unpackaged build or a manual uninstall, and the guidance says so.
- **The endpoint exists but is disabled or unplugged.** Installed but not usable; the remedy is in the Sound control panel, not the installer.
- **The endpoint exists and cannot be opened.** Everything else, reported with the underlying failure.

This mirrors the camera's availability probe, which distinguishes absent, half-registered, and version-mismatched for the same reason.

### The attribution check runs in the frontend area

The notice lives in About/Settings, so the surface that ships it is the React application, and `desktop/src/` already has a test suite running in the `frontend` CI area. A test asserting that the rendered About surface contains VB-CABLE, VB-Audio, the word *donationware*, and a reachable `vb-cable.com` link is a check on the thing that ships rather than on a constant that feeds it.

The alternative — a repository script grepping source files — was rejected because it passes on a notice that is present in the source and never rendered, which is the failure mode most likely to arrive from a component refactor.

## Risks / Trade-offs

**The user can point their system output at `CABLE Input` and create a loop.** VB-CABLE is a cable: its render endpoint appears in the user's playback device list, and nothing stops a user from selecting it as their default output — deliberately, or by accident while hunting for a device after an install. If they do, and the speaker stream is also active, WASAPI loopback captures the cable, which is carrying the phone's microphone audio, and sends it back to the phone. The result is a feedback path built entirely from supported configurations, with no component misbehaving. This is a consequence of using a cable-style driver and is not fixable in the sink alone.

**Resolved: detected and refused in this change.** The speaker's capture compares the endpoint it is about to loopback-capture against the one the microphone sink resolves, and refuses the speaker stream with guidance naming the conflict when they are the same device. The refusal falls on the speaker rather than the microphone because the speaker is the stream whose output is wrong — capturing a cable carrying the phone's own voice is not system audio by any reading — and because refusing the microphone would leave the user with a working speaker quietly transmitting their own echo.

It lands here rather than in W5 for a plain reason: the code is in `platform/windows.rs`, which this change is already open in, and W5 is packaging and release work that should not be making Rust changes to the capture path. The alternative of documenting it only was rejected because the symptom — a howl — arrives faster than a user reaches the troubleshooting guide.

**Endpoint matching is a heuristic against another vendor's driver.** A VB-CABLE release that changes its adapter name breaks resolution, and the symptom is the feature reporting "not installed" on a machine where it plainly is. That is the benign direction: it is loud, it is a support question with an obvious answer, and the guidance distinguishing "no endpoint" from "endpoint unusable" makes a confused user's report diagnostic.

The malign direction is a release that adds a render endpoint the discrimination does not exclude. Resolution then succeeds against the wrong endpoint and the feature is silently dead — the case examined above, generalised. This is why the resolver refuses on ambiguity instead of taking the first survivor, and why it logs its rejected candidates: the mitigation is not that the rules cannot break, but that breaking them produces a refusal and a log rather than silence. That was not hypothetical for long — the first rule this design chose was wrong about the release it was written against, and what it produced was a refusal naming both candidates. Only one VB-CABLE version has been examined, and the discrimination is provisional until more have been.

**VB-CABLE's endpoint format is user-configurable.** Its control panel exposes sample rate and bit depth, so the mix format is not merely "whatever the mixer runs at" but whatever the user set it to, including rates far from 48 kHz. The converter handles this by construction — the same problem the speaker already solves in the other direction — but it means the resampling path is ordinary rather than exceptional here, and should be tested at rates that are not simple ratios of 48 kHz.

**Nothing in CI exercises the endpoint.** The Windows runners have no audio device, which the `rust` quality area states as a standing constraint. Converter arithmetic, upmix, and underrun behaviour are portable and unit-testable on both legs. Resolution, rendering, and "an application really hears the phone" are verifiable only by hand, and W5 must write the Windows smoke test that currently does not exist.

**The feature is not user-reachable until W5.** Accepted and recorded in the proposal: this change makes the microphone work where VB-CABLE is present, and only the installer makes it present. The same interval existed between the two halves of W3.
