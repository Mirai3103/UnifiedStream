# Microphone Stream Specification

## Purpose

The end-to-end virtual microphone: phone-side audio capture and encoding, the mic payload format on stream ID 2, desktop-side decoding and playback into a PipeWire virtual source, mic controls (mute, gain, noise suppression), and level indication in both UIs.

See `openspec/specs/protocol.md` §3.9 and §5 for the normative wire format these requirements are implemented against.

## Requirements

### Requirement: Phone-side microphone capture

The Android application SHALL capture microphone audio at 48 kHz, mono, 16-bit PCM, in 20 ms frames, only while a microphone stream is active.

#### Scenario: Capture starts with the stream

- **WHEN** the microphone stream is accepted by the desktop
- **THEN** the phone begins capturing 48 kHz mono 16-bit PCM audio in 20 ms frames
- **AND** capture stops when the stream is stopped or the session ends

#### Scenario: Missing permission is surfaced, not silent

- **WHEN** the user enables the microphone toggle and `RECORD_AUDIO` permission has not been granted
- **THEN** the app requests the permission
- **AND** if the permission is denied, the toggle reverts and the UI states that the microphone permission is required
- **AND** no stream is started

#### Scenario: Capture failure is visible

- **WHEN** audio capture fails to start or aborts while the stream is active
- **THEN** the stream is stopped and the phone UI shows a microphone error state rather than a silently dead stream

#### Scenario: Capture continues in the background

- **WHEN** the microphone stream is active and the app leaves the foreground
- **THEN** capture and transmission continue under the existing foreground service, with the service declaring microphone foreground use

### Requirement: Audio format negotiation

The microphone stream SHALL be started with an explicit audio format, where PCM S16LE 48 kHz mono is the mandatory baseline every peer supports, and Opus MAY be used only when both peers support it.

#### Scenario: Baseline format is always accepted

- **WHEN** the phone starts the microphone stream offering PCM S16LE 48 kHz mono
- **THEN** the desktop accepts the format

#### Scenario: Opus is offered only when an encoder exists

- **WHEN** the phone's runtime has no working Opus encoder
- **THEN** the phone offers only PCM and the stream operates in PCM

#### Scenario: Unsupported codec is refused

- **WHEN** the desktop receives a microphone stream start naming a codec it cannot decode
- **THEN** it refuses the stream with reason `unsupported_codec`
- **AND** no media flows for that stream

### Requirement: Microphone payload format

Microphone audio SHALL be carried on stream ID 2, one 20 ms audio frame per transport frame, with the media header timestamp set to the capture time of the frame's first sample in microseconds since session start. A PCM payload is raw little-endian signed 16-bit samples; an Opus payload is a single Opus packet.

#### Scenario: PCM frame round-trips

- **WHEN** a 20 ms PCM frame (1920 bytes) is sent over the transport
- **THEN** it is fragmented, reassembled, and delivered byte-identical with its capture timestamp

#### Scenario: Loss costs exactly one frame

- **WHEN** one fragment of a PCM audio frame is lost
- **THEN** only that 20 ms frame is discarded and playback continues with the next complete frame

### Requirement: Desktop virtual audio source

The desktop SHALL expose received microphone audio as a PipeWire virtual source named "UnifiedStream Microphone" that ordinary applications can select as an input device. The source SHALL exist only while a microphone stream is accepted.

#### Scenario: Applications see a normal microphone

- **WHEN** the microphone stream is accepted
- **THEN** a PipeWire node named "UnifiedStream Microphone" with media class Audio/Source appears
- **AND** an application recording from it receives the phone's audio

#### Scenario: The source is removed when the stream ends

- **WHEN** the microphone stream is stopped or the session ends for any reason
- **THEN** the virtual source node is destroyed
- **AND** no orphaned node remains after the desktop app exits

#### Scenario: Audio system unavailability is reported

- **WHEN** the desktop cannot create the virtual source (e.g., PipeWire is not running)
- **THEN** it refuses the microphone stream
- **AND** the desktop UI shows that the virtual source is unavailable

### Requirement: Receive-side jitter buffering

The desktop SHALL buffer decoded audio in a bounded jitter buffer, emitting silence on underrun and dropping the oldest audio on overrun, so the virtual source never stalls and latency never grows without bound.

#### Scenario: Underrun produces silence

- **WHEN** the virtual source needs samples and the jitter buffer is empty
- **THEN** silence is emitted and playback resumes seamlessly when frames arrive again

#### Scenario: Overrun drops the oldest audio

- **WHEN** the jitter buffer reaches its maximum depth (6 frames)
- **THEN** the oldest frame is dropped so added latency stays bounded

### Requirement: Mute control

Both applications SHALL offer a microphone mute toggle. Mute SHALL stop frame transmission while keeping the stream started, so unmute resumes instantly.

#### Scenario: Mute stops transmission

- **WHEN** the user mutes the microphone from the phone
- **THEN** the phone stops sending audio frames without stopping the stream
- **AND** the desktop's virtual source emits silence

#### Scenario: Unmute resumes instantly

- **WHEN** the user unmutes
- **THEN** frame transmission resumes without renegotiation

#### Scenario: Desktop can mute remotely

- **WHEN** the user toggles the microphone off from the desktop UI
- **THEN** the phone stops sending audio frames
- **AND** both UIs reflect the muted state

### Requirement: Microphone gain

The phone SHALL apply a user-adjustable software gain between 0.5× and 4.0× (default 1.0×) to captured samples before encoding, with clipping prevented by saturation.

#### Scenario: Gain changes take effect immediately

- **WHEN** the user adjusts the gain slider while streaming
- **THEN** subsequent frames are scaled by the new gain without restarting the stream

#### Scenario: Gain saturates instead of wrapping

- **WHEN** a gain setting would push a sample beyond the 16-bit range
- **THEN** the sample is clamped to the maximum magnitude rather than overflowing

### Requirement: Noise suppression toggle

The phone SHALL offer a noise suppression toggle backed by the platform noise suppressor, shown only when the device supports it.

#### Scenario: Toggle applies the suppressor

- **WHEN** the user enables noise suppression on a supporting device
- **THEN** the platform noise suppressor is attached to the capture session

#### Scenario: Unsupported device hides the option

- **WHEN** the platform reports no noise suppressor is available
- **THEN** the toggle is not offered

### Requirement: Audio level indication

Both applications SHALL display a live input level while the microphone stream is active — computed from captured samples on the phone and from decoded samples on the desktop — updated at least 10 times per second and showing silence when muted.

#### Scenario: Levels move with speech

- **WHEN** the user speaks into the phone while streaming
- **THEN** both the phone and desktop level indicators rise and fall with the audio within one second of each other

#### Scenario: Muted level reads zero

- **WHEN** the microphone is muted
- **THEN** both level indicators show silence
