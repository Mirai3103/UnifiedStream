# Speaker Stream Specification

## Purpose

The end-to-end wireless speaker: desktop-side system audio capture via a PipeWire virtual sink, the speaker payload format on stream ID 3, phone-side decoding, jitter buffering, and playback, speaker controls (toggle, mute, volume, system routing), and level indication in both UIs.

See `openspec/specs/protocol.md` §3.9 and §6 for the normative wire format these requirements are implemented against.

## Requirements

### Requirement: Desktop virtual audio sink

The desktop SHALL expose a PipeWire virtual sink named "UnifiedStream Speaker" that ordinary applications can select as an output device, and SHALL capture the audio played into it for transmission. The sink SHALL exist only while a speaker stream is active.

#### Scenario: Applications see a normal output device

- **WHEN** the speaker stream is accepted by the phone
- **THEN** a PipeWire node named "UnifiedStream Speaker" with media class Audio/Sink appears
- **AND** audio an application plays into it is transmitted to the phone

#### Scenario: The sink is removed when the stream ends

- **WHEN** the speaker stream is stopped or the session ends for any reason
- **THEN** the virtual sink node is destroyed
- **AND** no orphaned node remains after the desktop app exits

#### Scenario: Audio system unavailability is reported

- **WHEN** the desktop cannot create the virtual sink (e.g., PipeWire is not running)
- **THEN** it does not start the speaker stream
- **AND** the desktop UI shows that the virtual sink is unavailable

### Requirement: System audio routing

The desktop SHALL offer a control that makes the virtual sink the system default output while the speaker stream is active, and SHALL restore the previously selected default output when the stream stops, the session ends, or on application startup after an unclean exit.

#### Scenario: Routing sends system audio to the phone

- **WHEN** the user enables the route-system-audio control while the speaker stream is active
- **THEN** the virtual sink becomes the system default output
- **AND** system audio plays on the phone

#### Scenario: The previous output is restored on stop

- **WHEN** the stream stops or the session ends while routing is enabled
- **THEN** the system default output is restored to the device that was selected before routing was enabled

#### Scenario: A stale takeover is repaired on startup

- **WHEN** the desktop application starts and finds the remembered default output was left pointing at its own virtual sink by a previous unclean exit
- **THEN** it restores the persisted previous default output

### Requirement: Desktop-side audio capture

The desktop SHALL capture audio from the virtual sink at 48 kHz, 16-bit PCM, in 20 ms frames, only while a speaker stream is active.

#### Scenario: Capture starts with the stream

- **WHEN** the speaker stream is accepted by the phone
- **THEN** the desktop begins capturing 48 kHz 16-bit PCM audio from the virtual sink in 20 ms frames
- **AND** capture stops when the stream is stopped or the session ends

#### Scenario: Capture failure is visible

- **WHEN** capture fails to start or aborts while the stream is active
- **THEN** the stream is stopped and the desktop UI shows a speaker error state rather than a silently dead stream

### Requirement: Audio format negotiation

The speaker stream SHALL be started with an explicit audio format, where PCM S16LE 48 kHz stereo is the default offering, PCM S16LE is the mandatory baseline every peer supports at both mono and stereo channel counts, and Opus MAY be used only when both peers support it.

#### Scenario: Baseline format is always accepted

- **WHEN** the desktop starts the speaker stream offering PCM S16LE 48 kHz stereo
- **THEN** the phone accepts the format

#### Scenario: Opus is offered only when an encoder exists

- **WHEN** the desktop's runtime has no working Opus encoder
- **THEN** the desktop offers only PCM and the stream operates in PCM

#### Scenario: Unsupported codec is refused

- **WHEN** the phone receives a speaker stream start naming a codec it cannot decode
- **THEN** it refuses the stream with reason `unsupported_codec`
- **AND** no media flows for that stream

### Requirement: Speaker payload format

Speaker audio SHALL be carried on stream ID 3, one 20 ms audio frame per transport frame, with the media header timestamp set to the capture time of the frame's first sample in microseconds since session start. A PCM payload is raw little-endian signed 16-bit samples, interleaved left/right when stereo; an Opus payload is a single Opus packet.

#### Scenario: Stereo PCM frame round-trips

- **WHEN** a 20 ms stereo PCM frame (3840 bytes) is sent over the transport
- **THEN** it is fragmented, reassembled, and delivered byte-identical with its capture timestamp

#### Scenario: Loss costs exactly one frame

- **WHEN** one fragment of a PCM audio frame is lost
- **THEN** only that 20 ms frame is discarded and playback continues with the next complete frame

### Requirement: Phone-side audio playback

The Android application SHALL play received speaker audio through the device's active audio output at the negotiated format, and SHALL continue playback while the app is not in the foreground under the existing foreground service, with the service declaring media-playback foreground use.

#### Scenario: Received audio is audible

- **WHEN** the speaker stream is active and frames are arriving
- **THEN** the phone plays the decoded audio through its speaker or connected headphones

#### Scenario: Playback continues in the background

- **WHEN** the speaker stream is active and the app leaves the foreground
- **THEN** reception and playback continue under the foreground service

#### Scenario: Playback failure is visible

- **WHEN** audio playback fails to start or aborts while the stream is active
- **THEN** the stream is stopped and the phone UI shows a speaker error state rather than a silently dead stream

### Requirement: Phone-side jitter buffering

The phone SHALL buffer decoded audio in a bounded jitter buffer, emitting silence on underrun and dropping the oldest audio on overrun, so playback never stalls and latency never grows without bound.

#### Scenario: Underrun produces silence

- **WHEN** the playback path needs samples and the jitter buffer is empty
- **THEN** silence is played and playback resumes seamlessly when frames arrive again

#### Scenario: Overrun drops the oldest audio

- **WHEN** the jitter buffer reaches its maximum depth (6 frames)
- **THEN** the oldest frame is dropped so added latency stays bounded

### Requirement: Speaker toggle from either device

Both applications SHALL offer a speaker on/off toggle. The desktop toggle SHALL start or stop the stream directly as its source; the phone toggle SHALL drive the same outcome through a sink-initiated stream request.

#### Scenario: Phone toggle starts the stream

- **WHEN** the phone user enables the speaker toggle while a session is Connected and `spk` is negotiated
- **THEN** the phone sends a stream request for the speaker stream
- **AND** the desktop initiates the stream start flow and media flows after acceptance

#### Scenario: Phone toggle stops the stream

- **WHEN** the phone user disables the speaker toggle while the stream is active
- **THEN** the stream is stopped and the desktop releases the virtual sink

#### Scenario: Desktop toggle starts the stream

- **WHEN** the desktop user enables the speaker toggle while a session is Connected and `spk` is negotiated
- **THEN** the desktop initiates the stream start flow directly

### Requirement: Desktop mute stops transmission

The desktop SHALL offer a speaker mute that stops frame transmission while keeping the stream started, so unmute resumes instantly without renegotiation.

#### Scenario: Mute stops transmission

- **WHEN** the user mutes the speaker from the desktop
- **THEN** the desktop stops sending audio frames without stopping the stream
- **AND** the phone's playback falls silent via jitter-buffer underrun

#### Scenario: Unmute resumes instantly

- **WHEN** the user unmutes
- **THEN** frame transmission resumes without renegotiation

### Requirement: Phone-side mute and volume

The phone SHALL offer a local mute and a volume control from 0 to 100 percent applied to playback, taking effect immediately without affecting the stream or transmission.

#### Scenario: Volume changes take effect immediately

- **WHEN** the user adjusts the volume slider while the stream is active
- **THEN** subsequent playback reflects the new volume without restarting the stream

#### Scenario: Phone mute silences playback locally

- **WHEN** the phone user mutes the speaker locally
- **THEN** playback is silenced immediately
- **AND** the stream remains active and frames continue to arrive

### Requirement: Audio level indication

Both applications SHALL display a live output level while the speaker stream is active — computed from captured samples on the desktop and from decoded samples on the phone — updated at least 10 times per second.

#### Scenario: Levels move with the audio

- **WHEN** audio plays into the virtual sink while streaming
- **THEN** both the desktop and phone level indicators rise and fall with the audio within one second of each other

#### Scenario: Transmission mute reads zero

- **WHEN** the desktop speaker mute is engaged
- **THEN** the phone's level indicator shows silence
