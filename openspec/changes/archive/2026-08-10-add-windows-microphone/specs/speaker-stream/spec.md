## ADDED Requirements

### Requirement: System audio capture refuses to capture the virtual microphone

Where a platform presents its virtual microphone as an endpoint that also appears in the user's list of output devices, the user MAY select that endpoint as their system output. If they do so while both media features are in use, the system audio capture would capture the audio the desktop is rendering for the microphone — the phone's own microphone input — and transmit it back to the phone.

The desktop SHALL detect that its system audio capture and its virtual microphone would resolve to the same device, and SHALL refuse the speaker stream rather than establish that path. The refusal SHALL name the conflict and what resolves it.

The refusal falls on the speaker stream rather than on the microphone stream. Audio the desktop is rendering for the microphone is not system audio under any reading, so the speaker is the stream whose source is wrong; refusing the microphone instead would leave a working speaker quietly transmitting the user's own echo, which is the harder fault to diagnose and the one that still sounds like a network problem.

This is not a defence against a misbehaving component. Every part of the configuration is supported and the user may have arrived at it deliberately, which is why it is detected rather than prevented, and reported rather than silently corrected.

#### Scenario: The system output is set to the virtual microphone's endpoint

- **WHEN** a speaker stream start arrives and the endpoint the desktop would capture is the same device the virtual microphone renders into
- **THEN** the desktop refuses the speaker stream
- **AND** the desktop UI names the conflicting device and states that the system output must be changed

#### Scenario: The conflict is created while the speaker is already streaming

- **WHEN** the user selects the virtual microphone's endpoint as their system output while a speaker stream is active
- **THEN** the desktop stops capturing that endpoint rather than transmitting what it finds there
- **AND** the user is shown the same conflict and its resolution

#### Scenario: The conflict is resolved

- **WHEN** the user selects any other device as their system output after being shown the conflict
- **THEN** the speaker stream can be started and operates normally

#### Scenario: A platform whose microphone endpoint is not an output device

- **WHEN** the running platform's virtual microphone does not appear in the user's output device list
- **THEN** no such conflict is possible and no check constrains which endpoint is captured
