## Why

The microphone is the last of the three media features missing on Windows, and it was the one blocked on money. `platform/windows.rs` says so in the code: `pub use super::unsupported::audio_sink`, with a comment naming W4 as the phase that replaces the line. Today a Windows user who enables the microphone gets a refusal naming the platform, which is correct and has been correct since W1.

Decision 5 of `docs/design.windows.md` is now closed. It priced option 2 as "VB-Audio *sells* redistribution licenses … in exchange for a licence fee", which made bundling look like trading a certificate bill for a licensing bill. VB-Audio's published terms grant redistribution of the standard donationware VB-CABLE — silent installation inside another installer explicitly included — against attribution obligations rather than a fee. No EV certificate, no Partner Center account, no `sysvad` driver to write, sign, or own the bug checks of.

So W4 is no longer "build a WDM audio driver". It is "render into an endpoint somebody else's signed driver already created", which is the same sentence the Linux sink satisfies with PipeWire, and which decision 4 recorded as identical work whether the endpoint is ours or a third party's. That foresight is what makes this a dependency change rather than an architectural one.

## What Changes

- **Implement `audio_sink` for Windows.** Render decoded phone audio into VB-CABLE's render endpoint (`CABLE Input`) through ordinary WASAPI shared mode, draining the existing `JitterBuffer` on a dedicated COM-initialized thread. Applications capture `CABLE Output` as an ordinary microphone. This replaces the one-line re-export from `unsupported` and changes nothing above the trait.

- **Resolve the endpoint by identity, not by position.** The sink targets a specific named render endpoint, not the default output — rendering the phone's voice into the user's speakers is the failure mode to design against, and it is what "just use the default" would do.

- **Convert the wire format to the endpoint's mix format.** `GetMixFormat` decides, as it does for the speaker: whatever the shared mixer runs at, commonly 32-bit float, at whatever rate the endpoint is configured for, in that endpoint's channel count. The wire format is PCM S16LE 48 kHz mono. This is the existing `AudioConverter` problem in the opposite direction — `SourceFormat` → `AudioFormat` today, `AudioFormat` → endpoint tomorrow — and whether that is one converter or two is a design question, not a scope question.

- **Refuse with guidance when VB-CABLE is absent**, through the existing platform-supplied setup guidance mechanism. No new mechanism, but a failure mode worth stating plainly: after W5 this should be unreachable, because the installer put it there. It is reachable in exactly two situations — an unpackaged build, and a user who uninstalled VB-CABLE by hand — and both deserve a message that says which.

- **Add the attribution VB-CABLE's licence requires.** In About/Settings: name VB-Audio, say the word *donationware*, and carry a reachable link to `vb-cable.com`. This is the consideration for the licence, not a courtesy credit — the grant is conditioned on the end user being able to identify the component and being "in a position to donate/pay license if finding it useful". A bare "powered by VB-CABLE" line does not satisfy it.

- **Generalise `microphone-stream` away from PipeWire**, the same move `add-windows-camera-frame-transport` made on `camera-stream`.

Explicitly **not** in scope, and flagged rather than assumed:

- **Bundling VB-CABLE in an installer.** That is W5, and the split is deliberate: it is exactly the shape of W3, where `add-windows-camera-frame-transport` shipped a desktop half that refused and named `regsvr32` until `add-windows-directshow-camera` supplied the thing to register. This change makes the microphone work on a machine where VB-CABLE is present; W5 makes it present, and only then is the product constraint — one application, no post-install configuration — actually satisfied on Windows. Until W5 this feature is reachable by developers and not by users, which is the same state the camera was in between W3a and W3b.
- **The installer bundler choice** (decision 6, reopened). There is no INF to install any more, so the argument that Tauri's NSIS bundler cannot do the job no longer holds. W5 settles it.
- **Authenticode signing and the release workflow.** W5.
- **Pinning and archiving the redistributed VB-CABLE installer** alongside releases, so an upstream change cannot retroactively alter what a shipped installer contains. A W5 concern because it is about what a release contains, recorded here because this is the change that creates the dependency.
- **Any change to the wire protocol.** The phone is unaware of any of this. If this change needs a protocol change, that is a signal something is wrong with the analysis above.

## Capabilities

### Modified Capabilities

- `microphone-stream`: one requirement changes. **Desktop virtual audio source** currently reads "The desktop SHALL expose received microphone audio as a PipeWire virtual source named 'UnifiedStream Microphone'", with scenarios asserting a PipeWire node with media class `Audio/Source` and naming PipeWire as the thing that might not be running. That is a Linux implementation written into a portable capability, and it is unsatisfiable on Windows by construction — the endpoint is created by a driver, exists whether or not a stream is active, and carries VB-Audio's name rather than ours.

  The generalisation follows `camera-stream`'s precedent exactly: *how* the microphone is presented is a platform concern; *that* applications see a selectable input carrying the phone's audio, that it stops carrying audio when the stream ends, and that an unavailable audio system refuses the stream visibly, are not. Two platform differences must survive the rewrite rather than be flattened away:

  - **Lifetime.** The Linux source exists only while a stream is accepted and is destroyed with it. A driver-created endpoint exists always. The portable requirement is about audio flowing, not about a device object existing.
  - **Naming.** Linux presents "UnifiedStream Microphone"; Windows presents `CABLE Output (VB-Audio Virtual Cable)`. The requirement must state that the user is told which device to select, not what it is called — the trait already returns an opaque platform-supplied label, and this is the same reasoning that let `device_label` return a DirectShow friendly name instead of a device path.

- `speaker-stream`: one new requirement. VB-CABLE is cable-style, so its render endpoint appears in the user's **output** device list, and nothing stops a user selecting it as their system output. If they do while the speaker is streaming, WASAPI loopback captures the cable — which is carrying the phone's own microphone audio — and sends it back to the phone. Every part of that configuration is supported and nothing misbehaves; the result is a feedback loop assembled from correct components. The desktop detects that its capture and its virtual microphone resolve to the same device and refuses the speaker stream, naming the conflict. The refusal falls on the speaker because that is the stream whose source is wrong.

- `repository-quality-gates`: one new requirement. This is the first dependency redistributed as a **signed binary under a grant** rather than vendored as source under a permissive licence, and its grant is conditioned on attribution that lives in the UI. Dropping that attribution in a later redesign converts a compliant product into an infringing one silently — no build fails, no test fails, and the provenance rule added by `add-windows-directshow-camera` does not cover it, because nothing entered the source tree. Decision 9's ordering argument applies with full force: a licence obligation checked after the fact cannot be acted on.

`platform-integration` is deliberately **not** modified. It already specifies per-integration support resolution, platform-supplied setup guidance with a copyable command, and the requirement that a platform's partial support is invisible to the application layer. Windows going from two implemented integrations to three is precisely the case its *Platform support is per integration* requirement was written for — "an integration is added to a partially supported platform", change confined to that integration's crate, no application-layer file changed. If this change needs a `platform-integration` edit, the seam is wrong and that is worth stopping for.

## Impact

**One module, one crate.** `unifiedstream-audio/src/platform/windows.rs` gains a sink and loses its re-export. No application-layer file changes, no `AudioSink` trait change, no frontend change beyond the attribution surface. The lifecycle the sink must present — constructed stopped, draining the buffer from its own thread once started, torn down on drop — is already established twice, by the PipeWire sink and by the loopback capture in this same module.

**A new conversion direction.** `AudioConverter` exists and runs endpoint-format → wire-format for the speaker. The sink needs wire-format → endpoint-format, and the two are not the same code read backwards: the sink also owns buffer padding, the render cadence, and what to write when the jitter buffer underruns. `microphone-stream` already requires silence on underrun, so the answer is specified even though the code is not.

**Testable on CI, mostly.** The `windows-latest` runners have no audio device, which the `rust` job states as a standing constraint on what may be added to the suite. Format conversion and buffer arithmetic are portable and belong in unit tests that run on both legs. Endpoint resolution, WASAPI rendering, and "an application really hears the phone" are not, and land in the Windows smoke test that W5 must write and that does not exist yet — `docs/release-smoke-test.md` has no Windows content at all.

**A vendor dependency, new in kind.** Every third-party thing this repository depends on so far is either a package manager's problem or MIT source vendored with a provenance record. VB-CABLE is neither: a proprietary signed binary, redistributed under a grant that can change, from a vendor whose release cadence is not ours. The fallback if the terms change is the original plan — write and sign a `sysvad`-derived driver — which is why decisions 4 and 5 keep that reasoning rather than deleting it, and `AudioSink` does not change either way.

**A visible wart, now permanent.** VB-CABLE is cable-style, so the user's playback list gains `CABLE Input` alongside their real outputs. The plan had been to replace a cable-style driver of our own with a capture-only variant later; that escape route is gone, because the driver is not ours to change. Decision 4 accepts this as the price of not writing, signing, or maintaining kernel-mode code, and the user documentation should name it before a user finds it and files it as a bug.
