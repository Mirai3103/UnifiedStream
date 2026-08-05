# Camera Stream Specification (Delta)

## MODIFIED Requirements

### Requirement: Desktop virtual camera device

The desktop SHALL present received camera video as a virtual webcam that ordinary applications can select as a camera, writing decoded frames to the platform's virtual camera device only while a camera stream is active. When no usable virtual camera device exists, the desktop SHALL refuse the stream and SHALL show the setup guidance supplied by the platform implementation, including the command that resolves the failure where the platform can name one. On Linux the device is a v4l2loopback device labeled "UnifiedStream Camera" and the supplied command is the `v4l2loopback` module load.

The device identifier shown in the user interface SHALL be a label supplied by the platform implementation rather than a path in a fixed platform-specific form.

#### Scenario: Applications see a normal webcam

- **WHEN** the camera stream is accepted by the desktop and frames are arriving
- **THEN** a virtual camera device labeled "UnifiedStream Camera" delivers the phone's video to applications that select it
- **AND** the desktop UI shows the platform-supplied identifier of the device being written to

#### Scenario: A missing virtual camera device is reported with guidance

- **WHEN** a camera stream start arrives and no virtual camera device is available
- **THEN** the desktop refuses the stream with reason `internal`
- **AND** the desktop UI shows the platform's message describing what is missing
- **AND** where the platform supplies a command that resolves it, the UI shows that command — on Linux, the exact `v4l2loopback` module-load command

#### Scenario: The device is released when the stream ends

- **WHEN** the camera stream is stopped or the session ends for any reason
- **THEN** the desktop stops writing and releases the virtual camera device
- **AND** no writer remains attached after the desktop app exits
