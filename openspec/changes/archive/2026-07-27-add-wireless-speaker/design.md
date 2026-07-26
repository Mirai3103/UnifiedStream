# Wireless Speaker — Design

## Context

The virtual microphone change shipped everything the speaker needs on the wire: generic stream lifecycle messages (`stream_start`/`stream_ack`/`stream_stop`/`stream_request`) with capability gating, an audio payload convention (one audio frame per transport frame, PCM S16LE baseline, Opus negotiated), and per-stream transport accounting. Stream ID 3 (Speaker, PC → phone) and the `spk` capability token are reserved in `openspec/specs/protocol.md` but unimplemented.

The pieces are asymmetrically placed, though. The desktop has only ever been a media *sink* (it receives mic audio into a PipeWire `Audio/Source` via the `unifiedstream-audio` crate, which owns a tested `JitterBuffer` and an `AudioSink` trait); the phone has only ever been a *source* (`MicCapture` → `MediaSender`). The speaker inverts both roles: the desktop must capture and send, and the phone must receive, buffer, and play. The phone's `transport/StreamReceiver` (reassembly, loss accounting) already exists for the synthetic test stream, so the receive plumbing is in place.

The dev machine is CachyOS Linux running PipeWire; the MVP targets Linux only. `unifiedstream-net` stays free of system dependencies.

## Goals / Non-Goals

**Goals:**

- PC system audio playing on the phone's speaker or headphones with latency low enough for video watching to feel synchronized (target < 150 ms end to end).
- A PipeWire virtual sink on the desktop that any application — or the whole system, when the user makes it the default output — plays into.
- Stereo audio as the default (system audio is stereo), with the existing PCM baseline / Opus stretch split.
- Speaker controls: toggle from either UI (phone drives it via `stream_request`), mute on both sides, phone-side volume; live level meters in both UIs.
- No protocol additions beyond documenting stream ID 3's payload — the lifecycle machinery is reused as designed.

**Non-Goals:**

- Windows audio capture (later change; the capture module is the only platform-specific piece).
- Acoustic echo cancellation. Running mic and speaker simultaneously can feed the phone's speaker into the phone's mic; that is a known limitation for MVP, not a blocker.
- Adaptive jitter buffering or clock-drift resampling — same fixed-depth buffer policy as the mic, now on the phone.
- A/V sync with a future camera stream; audio-only latency is the target here.
- Multiple phones (single-session invariant unchanged).

## Decisions

### D1: Desktop capture via a PipeWire virtual sink node, not monitor capture

The `unifiedstream-audio` crate gains a `PipeWireSpeakerSink` module: a PipeWire stream with `media.class = "Audio/Sink"`, node name **"UnifiedStream Speaker"**, S16LE 48 kHz stereo. Applications (or the system default route) play into the node; PipeWire delivers those samples to our process callback, which pushes 20 ms frames into the send path.

Alternatives considered:

- **Capture the default sink's monitor** — hears everything with zero user routing, but the audio *also* keeps playing on the PC's speakers (usually unwanted for a "wireless speaker"), and following the user's default-sink changes mid-stream is racy. Rejected.
- **`pactl load-module module-null-sink`** — same fragile-lifecycle and orphaned-module problems that ruled out `module-pipe-source` for the mic. Rejected.

Routing *all* system audio is a one-step user action (select "UnifiedStream Speaker" as the output device). The desktop UI additionally offers a **"route system audio" toggle** that sets the virtual sink as the default output via WirePlumber and restores the previous default when the stream stops or the app exits — restoration is mandatory, a hijacked default output that persists after a crash is unacceptable, so the previous default is persisted before switching and restored defensively on startup.

### D2: Stereo 48 kHz S16LE, 20 ms frames — same framing rules as the mic

`stream_start` params: `{ "codec": "pcm_s16le", "sample_rate": 48000, "channels": 2, "frame_ms": 20 }`. A 20 ms stereo frame is 3840 bytes → 4 fragments (~1.62 Mbps), well within what the synthetic stream validated. Payload is interleaved L/R little-endian samples, exactly as protocol §5.1 already specifies for multi-channel PCM. One audio frame per transport frame, timestamp = capture time of the first sample; loss costs one 20 ms frame.

Mono was right for voice; system audio is music and video, so stereo is the default. The phone accepts mono too (the format is negotiated, and a mono fallback halves bandwidth if ever needed). Opus stereo (~64–96 kbps) is the same stretch-task shape as the mic's: specified and negotiated now, shipped only if reached.

### D3: Phone playback via AudioTrack with a Kotlin port of the desktop jitter buffer

New `audio/` members on Android: `SpeakerPlayback` (an `AudioTrack` in low-latency mode, `USAGE_MEDIA`, stereo 48 kHz) and `SpeakerJitterBuffer`, a direct port of the desktop `JitterBuffer` policy — target depth 3 frames (60 ms), cap 6, silence on underrun, drop-oldest on overrun. The playback thread pops frames and writes to the `AudioTrack`; decoded frames arrive from the existing `StreamReceiver` reassembly path registered on stream ID 3.

The same two behaviors (underrun → silence, overrun → drop) absorb clock drift between the desktop's capture clock and the phone's audio clock, as they do for the mic in the other direction. Buffer depth is logged so drift is measurable before deciding whether a resampler is ever warranted.

`SessionService` gains `foregroundServiceType="mediaPlayback"` (alongside `microphone`) so playback continues in the background.

### D4: The phone toggle is a `stream_request`; the desktop toggle starts the stream directly

The desktop is the stream source, so its toggle runs the `stream_start` flow directly. The phone is the sink, so its toggle sends `stream_request` `{ "stream": 3, "active": true|false }` — the exact flow §3.9.4 was written for ("later the phone's speaker switch"). No new control messages, no session-control spec changes. The stream is gated on `spk` being in the negotiated capability intersection, enforced by the existing generic rule.

### D5: Mute stops transmission on the desktop; phone mute is local

Desktop mute stops sending frames without ending the stream (identical to mic mute — the phone's jitter buffer underruns into silence, unmute resumes instantly with no renegotiation). Phone-side mute simply zeroes its `AudioTrack` volume locally — instant, no round trip, and the user's expectation for a "mute this device" control. Turning the feature *off* from either side, by contrast, stops the stream (`stream_stop` or `stream_request` active:false) and releases the virtual sink.

### D6: Volume is phone-side only

A volume slider on the phone drives `AudioTrack.setVolume()` (0–100 %), composing with the hardware media volume keys naturally. The desktop does not scale samples before sending: attenuating before PCM transmission would just throw away bits, and the phone already has the control. Level meters are computed where the samples are: on the desktop from captured frames before encoding, on the phone from decoded frames before the volume is applied (so the meter shows signal presence even when muted locally — matching the mic spec's "muted reads zero" only for the *transmission* mute, which is the desktop's).

### D7: Desktop send path lives beside the receive path, reusing the net crate's sender

`unifiedstream-net` already has the fragmenting sender (used by the synthetic stream and the desktop's outbound test traffic). The speaker adds a `SpeakerStream` task in the app crate: pops frames from the capture callback's ring buffer, stamps capture timestamps, and hands them to the existing sender on stream ID 3. Bandwidth and loss ride the existing per-stream accounting and 1 Hz `telemetry` — no new fields. Virtual-sink status (created / streaming / clients-connected) flows over the same Tauri event channel as the mic's source status.

## Risks / Trade-offs

- [Feedback loop when mic and speaker run simultaneously (phone speaker → phone mic → PC → phone speaker)] → Documented limitation; headphones on the phone break the loop. No AEC in MVP; the UIs do not artificially forbid the combination since headphone use is legitimate.
- [Changing the default sink via WirePlumber and crashing leaves the user's audio routed to a dead node] → Previous default is persisted before switching; restored on clean stop, session end, *and* app startup (defensive sweep for a stale takeover). The virtual sink itself dies with the process, so PipeWire re-routes to a real device automatically — the sweep only fixes the remembered default.
- [~1.6 Mbps PCM stereo amplifies Wi-Fi loss vs the mic's 0.79 Mbps] → Loss still costs exactly one 20 ms frame; Opus stretch task is the real fix and the negotiation already carries it.
- [`AudioTrack` low-latency mode varies wildly across devices; some add 100 ms+ of output latency] → Use `PERFORMANCE_MODE_LOW_LATENCY` and the minimum buffer size the device reports; measured latency is out of our control beyond that and the jitter buffer target is not inflated to compensate.
- [PipeWire `Audio/Sink` stream nodes and `Audio/Source` nodes share the crate's callback-heavy patterns; a second module doubles the deadlock surface] → Same mitigation that worked for the mic: keep the PipeWire loop on its own thread, communicate only through lock-free ring buffers, and reuse the `AudioSink`-style trait boundary (a mirrored `AudioCapture` trait) so the buffer logic is unit-tested without PipeWire.

## Open Questions

- None blocking. Deferred deliberately: Windows capture (WASAPI loopback is the likely shape), AEC, Opus (stretch task), and whether the "route system audio" toggle should also duck or mute the PC's local output when monitor-style routing is ever added.
