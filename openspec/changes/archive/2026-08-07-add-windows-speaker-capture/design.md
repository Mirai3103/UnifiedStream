## Context

See `proposal.md` — Why, and `docs/design.windows.md` decision 2 for the architectural choice this change implements.

Three facts about the current code shape the design.

`decouple-platform-integrations` left `unifiedstream-audio::platform` resolving the whole platform at once: `linux` under `cfg(target_os = "linux")`, `unsupported` otherwise. Windows needs one of the three factories and not the other two, which no existing branch expresses.

`FrameChunker` already absorbs the mismatch between whatever chunk size an audio system delivers and the exact 20 ms frame the wire wants. It is portable, unit-tested, and needs no change. The Windows capture feeds it exactly as the PipeWire capture does.

The workspace's tests are all portable. Despite the comment in `.github/workflows/quality.yml` claiming they "exercise PipeWire and v4l2 loopback behavior", `mic_loopback.rs`, `speaker_loopback.rs`, and `camera_pipeline.rs` each state in their own header that they run without a PipeWire daemon and without the kernel device — they exercise the transport and the portable pipeline, not the system. Nothing in the suite touches audio or video hardware. This is why enabling tests on the Windows leg is a one-line change rather than a test-infrastructure project, and it is also a constraint: GitHub's Windows runners have no audio endpoint at all, so nothing added here may require one either.

## Goals / Non-Goals

**Goals:**

- The Windows capture path is correct on a machine whose default output is not 48 kHz, is not stereo, and is not 16-bit — the common case, not an edge case.
- The parts of the work that can be tested without an audio endpoint are in code that has no `windows` types in it, so both CI legs cover them.
- One recovery path serves every way the captured endpoint can stop being the right one.
- Adding the camera in W3 and the microphone in W4 requires no further change to how the platform factories resolve.

**Non-Goals:**

- Any change to the protocol, the Android application, or the negotiated wire format. The conversion exists precisely so those stay fixed.
- Exclusive-mode capture, per-process capture, or letting the user choose which endpoint is captured. The default render endpoint, always.
- Packaging, installer, or release workflow. Stated in the proposal and repeated here because it is the most likely thing to creep in: a working `cargo tauri build` producing a runnable `.exe` is in scope, and producing something another person can install is not.

## Decisions

### 1. Platform resolution becomes per integration, with the fallback available everywhere

`platform/mod.rs` gains a third branch, and `unsupported` stops being gated on `not(target_os = "linux")` — it is compiled on every target, because Windows now needs to borrow from it:

```text
              audio_capture      audio_sink        audio_routing
  linux       linux::…           linux::…          Some(linux::…)
  windows     windows::…         unsupported::…    None            ← W2
  other       unsupported::…     unsupported::…    None
```

`platform/windows.rs` re-exports the fallback for what it does not implement:

```rust
pub use super::unsupported::audio_sink;
```

The alternative was to gate each factory function's body separately in `mod.rs`, so that `audio_sink` is chosen by one `cfg` and `audio_capture` by another. Rejected: it puts three independent platform tables in the module that is supposed to have one, and the reader can no longer see what a given platform supports without tracing three functions. A per-platform module that names its own gaps keeps one table and makes the gap explicit at the point where W4 will close it — the `pub use` line is the diff.

Making `unsupported` unconditional costs a dead-code allowance on Linux, where nothing references it. That is cheaper than the alternative and disappears the moment any platform is partial, which is the state Windows is in until W4.

### 2. Capture is polled on a dedicated COM thread, not event-driven

`AUDCLNT_STREAMFLAGS_LOOPBACK` does not combine with `AUDCLNT_STREAMFLAGS_EVENTCALLBACK`; Microsoft's own loopback sample polls. The capture therefore owns a thread that calls `CoInitializeEx(COINIT_MULTITHREADED)`, initializes the client, and loops on `GetNextPacketSize` / `GetBuffer` / `ReleaseBuffer`, sleeping half the device period between passes.

The `AudioCapture` struct itself holds only a `JoinHandle` and an `AtomicBool` stop flag, which is what makes it `Send` without any COM object crossing a thread boundary. This mirrors how the PipeWire implementations are structured, so the application layer sees identical lifecycles.

An event on the keep-alive render stream of decision 3 could drive the loop instead, since that client *can* be event-driven. Rejected as a first implementation: it couples the two clients' lifetimes for a saving of one `Sleep` per period, and it makes the recovery path of decision 4 harder to reason about because the clock would then also need re-establishing.

`AUDCLNT_BUFFERFLAGS_SILENT` is honoured by writing zeros rather than reading the buffer — required by the API, and load-bearing here: when only the keep-alive is playing, every packet is flagged silent, and that is precisely the path that keeps the phone fed while the PC is quiet.

### 3. Keep-alive is a second render client on the same endpoint

An idle render endpoint produces no loopback packets at all. The capture therefore opens a second `IAudioClient` on the same endpoint in ordinary render mode, fills its buffer with silence, starts it, and tops it up on each poll pass, for the lifetime of the capture.

This is decision 2 of `docs/design.windows.md` restated because it is the single easiest thing in this change to omit and the hardest to attribute afterwards: without it the stream stalls only when the PC is silent, which looks like a network fault.

### 4. One recovery path for every way the endpoint stops being right

Three events mean the same thing — the endpoint being captured is no longer the endpoint that should be captured:

```text
  IMMNotificationClient::OnDefaultDeviceChanged  ─┐
  AUDCLNT_E_DEVICE_INVALIDATED from GetBuffer    ─┼─▶ set needs_reopen ──▶ tear down both
  AUDCLNT_E_DEVICE_INVALIDATED from the keep-alive┘                        clients, re-resolve
                                                                           the default endpoint,
                                                                           rebuild the converter,
                                                                           clear the chunker
```

The notification callback runs on a system thread and does nothing but set an `AtomicBool`. It resolves no device, touches no COM object owned by the capture thread, and takes no lock the capture thread holds. Anything more is a deadlock waiting for a user who changes their output device while audio is flowing.

The chunker is cleared on re-open because a partial frame from the old endpoint would otherwise be spliced onto the front of the new one at a different rate. The converter is rebuilt because the new endpoint's mix format is not the old one's.

Failure to re-open on any endpoint stops the stream and surfaces a speaker error, per the spec's "Recovery is impossible" scenario. Notably this also covers an application taking the endpoint in exclusive mode.

### 5. The desktop owns the conversion, in a portable module

`GetMixFormat` returns the shared-mode mixer's format: commonly 32-bit float, stereo, at whatever rate the user set in the Sound control panel. The wire wants interleaved S16LE at 48 kHz stereo. Conversion happens in this order:

```text
  f32 @ endpoint rate, N ch
        │  downmix / upmix to 2 ch       (fewest samples resampled)
        ▼
  f32 @ endpoint rate, 2 ch
        │  resample to 48 kHz            ← bypassed entirely when already 48 kHz
        ▼
  f32 @ 48 kHz, 2 ch
        │  scale, clamp, convert
        ▼
  i16 @ 48 kHz, 2 ch  ──▶ FrameChunker(1920) ──▶ FrameCallback
```

All of it lives in a new portable `convert` module beside `capture.rs`, with `rubato` as an unconditional dependency, and `platform/windows.rs` is glue that fetches samples and hands them over.

This is a deliberate exception to the repository's discipline of target-gating anything only one platform uses, and the reason is testability. Sample-rate conversion feeding a fixed-size chunker is arithmetic that fails silently — an off-by-one in the chunk accounting produces audio that plays, sounds nearly right, and drifts. It is the code in this change most worth testing and the code least able to be tested on the machine it runs on, since GitHub's Windows runners have no audio endpoint. Putting it behind `cfg(target_os = "windows")` would leave it verified by ear on one developer's machine. Portable, it is covered by `cargo test --workspace` on both legs. `rubato` is pure Rust and adds no system dependency to the Linux build.

The resampler is bypassed when the endpoint already runs at 48 kHz, which is the Windows default and therefore the common case: no added latency and no added CPU for most users, and the conversion path reduces to float-to-integer.

Two alternatives were considered:

`AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM` with `AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY` lets a shared-mode client request a format other than the mix format and have WASAPI convert. If it composes with `AUDCLNT_STREAMFLAGS_LOOPBACK`, it removes the resampler entirely. That composition is not verified, and correctness would then differ between machines where it is honoured and machines where `Initialize` refuses — a class of bug that appears only on other people's hardware. Owning the conversion is worth more than the code it would save. If a later spike proves the composition works, replacing the resampler is contained to the `convert` module and changes no requirement.

Renegotiating the stream at the endpoint's rate was rejected by the user in exploration and is recorded here for the record: it pushes rate handling into the protocol and the Android application to avoid a resampler on the desktop, which is the wrong side of that trade for a change scoped to one platform.

### 6. The Windows CI leg runs the whole test suite

The current exemption's stated reason does not hold — no test in the workspace touches PipeWire, v4l2, or any device. Removing the `if:` from the test step is the whole change, and it immediately covers the `convert` module on both legs.

Tests are not added for `platform/windows.rs` itself. It needs a real endpoint, the runners have none, and a mock of `IAudioClient` would test the mock. It is verified by the manual smoke run instead, which is the same bargain the PipeWire implementations already make.

## Risks / Trade-offs

- **[Resampling drift: chunk accounting is off and audio slowly desynchronises]** → The most likely defect in this change and the reason for decision 5. `rubato`'s fixed-input resamplers have deterministic input and output frame counts; tests assert exact output sample counts over a long run at 44.1 kHz, not just that audio comes out.
- **[The keep-alive stream is audible, or shows the app as playing audio]** → It plays digital silence, so it is inaudible, but it will make the application appear in the Volume Mixer for the duration of a speaker stream. Accepted: it is visible only while the feature is on, and the alternative is a stalling stream.
- **[An exclusive-mode application seizes the endpoint mid-stream]** → Loopback initialization fails and the recovery path of decision 4 reports a speaker error rather than a silent stall. Not recoverable by design; the user is told.
- **[Deadlock between the notification callback and the capture thread]** → Mitigated structurally by decision 4: the callback sets one atomic and returns.
- **[An endpoint with more than two channels downmixes badly]** → A 5.1 endpoint is downmixed by a fixed matrix rather than by the endpoint's channel mask. Acceptable for a first implementation; the channel mask is available from `WAVEFORMATEXTENSIBLE` if it proves wrong in practice.
- **[Making `unsupported` unconditional produces dead-code warnings on Linux, and CI denies warnings]** → An explicit allowance on the module with the reason stated, rather than silencing the lint workspace-wide.
- **[The Windows path is verified only by one developer's manual run]** → True, and unchanged from how the Linux integrations are verified. The portable half is covered by CI; the endpoint half is covered by the smoke run in the tasks.

## Migration Plan

No data migration, no configuration change, no user-visible change on Linux. `SpeakerStatus.routed` remains `None` on Windows, so the routing control stays hidden exactly as it is today, and the frontend is untouched.

The Linux speaker scenarios must be re-verified even though no Linux code changes, because the spec's "Desktop virtual audio sink" requirement is removed and its guarantees are re-expressed under "Desktop system audio source". The verification is that the behavior is identical, not that it is new.

Rollback is a revert: nothing here is persisted and nothing outside the process is modified.
