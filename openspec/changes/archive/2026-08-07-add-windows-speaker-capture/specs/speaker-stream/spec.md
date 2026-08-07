## ADDED Requirements

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

## MODIFIED Requirements

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

## REMOVED Requirements

### Requirement: Desktop virtual audio sink

**Reason**: The requirement specified one platform's mechanism — a PipeWire virtual sink — as the contract, which no longer holds now that a second platform captures the existing output device directly and creates no sink at all. Its behavior is preserved, and generalized, by the new "Desktop system audio source" requirement.

**Migration**: None for users. The Linux behavior is unchanged and is covered by the first scenario of "Desktop system audio source", including the "UnifiedStream Speaker" device name, the device's lifetime, and the reporting of an unavailable audio system.
