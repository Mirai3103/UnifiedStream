## MODIFIED Requirements

### Requirement: Desktop virtual camera device

The desktop SHALL present received camera video as a virtual webcam that ordinary applications can select as a camera, writing decoded frames to the platform's virtual camera only while a camera stream is active. How that camera is presented is a platform concern: a platform MAY write frames to a virtual camera device the operating system provides, or MAY deliver them to a component that each consuming application loads into its own process. When no usable virtual camera exists, the desktop SHALL refuse the stream and SHALL show the setup guidance supplied by the platform implementation, including the command that resolves the failure where the platform can name one.

Where the platform's virtual camera depends on a component that must be installed before it exists, its absence SHALL be reported as setup guidance in the same way as any other missing prerequisite, and SHALL NOT be reported as the platform having no implementation at all.

The device identifier shown in the user interface SHALL be a label supplied by the platform implementation rather than a path in a fixed platform-specific form.

#### Scenario: Applications see a normal webcam

- **WHEN** the camera stream is accepted by the desktop and frames are arriving
- **THEN** a virtual camera named "UnifiedStream Camera" delivers the phone's video to applications that select it
- **AND** the desktop UI shows the platform-supplied identifier of the device being written to

#### Scenario: A missing virtual camera is reported with guidance

- **WHEN** a camera stream start arrives and the platform has no usable virtual camera
- **THEN** the desktop refuses the stream with reason `internal`
- **AND** the desktop UI shows the platform's message describing what is missing
- **AND** where the platform supplies a command that resolves it, the UI shows that command — on Linux, the exact `v4l2loopback` module-load command

#### Scenario: A platform whose virtual camera component is not installed

- **WHEN** a camera stream start arrives on a platform that presents its virtual camera through a loadable component, and that component is not installed
- **THEN** the desktop refuses the stream with reason `internal`
- **AND** the desktop UI shows that the component is missing and the command that installs it
- **AND** the same platform's other media features are unaffected

#### Scenario: The device is released when the stream ends

- **WHEN** the camera stream is stopped or the session ends for any reason
- **THEN** the desktop stops writing and releases the platform's virtual camera
- **AND** no writer remains attached after the desktop app exits

## ADDED Requirements

### Requirement: Virtual camera delivery tolerates its consumers

Where the platform delivers frames to consuming applications across a process boundary, the desktop SHALL remain the single producer and SHALL NOT depend on any consumer being present, responsive, or alive. Applications MAY select and release the virtual camera, and MAY terminate without releasing it, at any point during a stream.

The desktop MUST NOT block the camera receive path waiting for a consumer to read, and MUST NOT stop or degrade the stream because no application is consuming it. Where a frame cannot be delivered in time, it SHALL be dropped rather than queued, bounding added latency exactly as the phone-side send path already does.

#### Scenario: No application is consuming the camera

- **WHEN** the camera stream is active and no application has selected the virtual camera
- **THEN** the desktop continues to accept and process arriving frames
- **AND** the stream is reported as active with its delivered frame rate

#### Scenario: An application selects the camera mid-stream

- **WHEN** an application selects the virtual camera while the camera stream is already active
- **THEN** it begins receiving frames without the stream being restarted
- **AND** applications already consuming the camera are undisturbed

#### Scenario: A consuming application terminates without releasing the camera

- **WHEN** an application consuming the virtual camera exits abruptly
- **THEN** the camera stream continues and the desktop keeps producing frames
- **AND** remaining consumers are undisturbed
- **AND** nothing is left behind that prevents an application from selecting the camera afterwards

### Requirement: Virtual camera component compatibility

Where the platform's virtual camera is presented by a separately installed component, the frame format between the desktop and that component SHALL carry an explicit version. A component that does not understand the desktop's version SHALL decline to deliver frames rather than interpret a format it does not recognise, and the desktop SHALL report the mismatch as setup guidance naming the resolution.

A version disagreement SHALL be treated as the component being unusable, in the same way an absent component is, and never as a stream that merely produces incorrect video.

#### Scenario: An outdated component is installed

- **WHEN** the installed virtual camera component and the desktop disagree on the frame format version
- **THEN** the desktop refuses the camera stream
- **AND** the desktop UI shows that the installed component is incompatible and what resolves it
- **AND** no frames are presented to applications that selected the camera

#### Scenario: Versions match

- **WHEN** the installed component understands the desktop's frame format version
- **THEN** the camera stream proceeds and frames are delivered normally
