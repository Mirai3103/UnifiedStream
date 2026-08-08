# Speaker Stream Specification

## Purpose

The end-to-end wireless speaker: desktop-side system audio capture, the speaker payload format on stream ID 3, phone-side decoding, jitter buffering, and playback, speaker controls (toggle, mute, volume, system routing), and level indication in both UIs. How the audio is captured is a platform concern — a virtual sink the desktop creates and makes the default output, as on Linux, or a direct capture of the existing default output, as on Windows.

See `openspec/specs/protocol.md` §3.9 and §6 for the normative wire format these requirements are implemented against.
## Requirements
### Requirement: System audio routing

System audio routing is an optional platform capability. On a platform whose audio system requires the desktop to become the system default output in order to capture system audio, the desktop SHALL offer a control that makes the virtual sink the system default output while the speaker stream is active, and SHALL restore the previously selected default output when the stream stops, the session ends, or on application startup after an unclean exit.

On a platform that captures system audio without taking over the default output, the desktop SHALL report the routing capability as absent and SHALL NOT offer the control. Absence MUST be distinguishable from the control being present and switched off.

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

#### Scenario: A platform that needs no routing

- **WHEN** the desktop runs on a platform that captures system audio without taking over the default output
- **THEN** the speaker status reports routing as absent rather than as off
- **AND** the user interface omits the route-system-audio control
- **AND** the user's selected output device is never changed

### Requirement: Desktop-side audio capture

The desktop SHALL capture audio from the platform's system audio source at the negotiated stream format — 48 kHz, 16-bit PCM, in 20 ms frames by default — only while a speaker stream is active. Where the platform's audio source runs at a different sample rate or sample format, the desktop SHALL convert captured audio to the negotiated format before transmission, so that the wire format does not vary with the user's audio hardware.

#### Scenario: Capture starts with the stream

- **WHEN** the speaker stream is accepted by the phone
- **THEN** the desktop begins capturing 48 kHz 16-bit PCM audio from the platform's system audio source in 20 ms frames
- **AND** capture stops when the stream is stopped or the session ends

#### Scenario: The audio source runs at a different format

- **WHEN** the platform's system audio source delivers samples at a sample rate or sample format other than the negotiated one
- **THEN** the desktop converts them to the negotiated format
- **AND** the phone receives 20 ms frames at the negotiated rate, byte-identical in layout to frames from a source that needed no conversion
- **AND** no renegotiation with the phone is required

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

### Requirement: Desktop system audio source

The desktop SHALL capture the audio the user hears and transmit it to the phone while a speaker stream is active. How the audio is obtained is a platform concern: a platform MAY create a virtual output device and capture what is played into it, or MAY capture the existing system output device directly. The chosen mechanism SHALL exist only while a speaker stream is active, and SHALL be reported to the user when it cannot be established.

A platform that captures the existing output device directly SHALL NOT create an output device and SHALL NOT change the device the user selected.

#### Scenario: A platform that captures through a virtual output device

- **WHEN** the speaker stream is accepted by the phone on a platform whose audio system requires a virtual output device
- **THEN** an output device named "UnifiedStream Speaker" appears and ordinary applications can select it
- **AND** audio an application plays into it is transmitted to the phone
- **AND** the device is destroyed when the stream stops or the session ends, leaving nothing behind after the desktop application exits

#### Scenario: A platform that captures the existing output device

- **WHEN** the speaker stream is accepted by the phone on a platform that can capture the existing system output directly
- **THEN** no additional output device appears in the user's device list
- **AND** the user's selected output device is unchanged
- **AND** audio continues to play on the user's own speakers while it is transmitted to the phone

#### Scenario: The audio source cannot be established

- **WHEN** the desktop cannot establish the system audio source
- **THEN** it does not start the speaker stream
- **AND** the desktop UI shows that system audio capture is unavailable, naming what failed

### Requirement: Capture continues while the system is silent

While a speaker stream is active, the desktop SHALL continue to deliver audio frames at the negotiated rate even when no application is playing audio. Silence SHALL be transmitted as silence; the stream MUST NOT stall.

This is normative because a capture that observes an idle output device may receive nothing at all rather than receiving silent samples, which would leave the phone's jitter buffer with nothing to consume and the user with a stream that appears dead.

#### Scenario: Nothing is playing on the PC

- **WHEN** the speaker stream is active and no application on the PC is playing audio
- **THEN** the desktop continues to send 20 ms frames at the negotiated rate
- **AND** the phone's jitter buffer does not underrun
- **AND** the desktop and phone continue to report the stream as active

#### Scenario: Playback resumes after silence

- **WHEN** an application begins playing audio after a period of PC silence
- **THEN** that audio reaches the phone without the stream being restarted
- **AND** no additional latency remains from the silent period

### Requirement: Capture follows the system output device

Where the desktop captures a specific system output device, it SHALL follow that device for the lifetime of the stream. When the user changes the system default output, or the captured device becomes invalid, the desktop SHALL re-establish capture on the current default output without stopping the stream.

#### Scenario: The user changes the output device mid-stream

- **WHEN** the user selects a different system default output while the speaker stream is active
- **THEN** the desktop re-establishes capture on the newly selected device
- **AND** audio played to the new device reaches the phone
- **AND** the stream is not stopped and the session is not interrupted

#### Scenario: The captured device disappears

- **WHEN** the device being captured becomes invalid, such as a headset being unplugged
- **THEN** the desktop re-establishes capture on the current default output
- **AND** the stream continues

#### Scenario: Recovery is impossible

- **WHEN** the desktop cannot re-establish capture on any output device
- **THEN** the stream is stopped
- **AND** the desktop UI shows a speaker error state rather than a silently dead stream

