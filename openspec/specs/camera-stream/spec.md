# Camera Stream Specification

## Purpose

The end-to-end virtual camera: phone-side camera capture, MJPEG encoding, and transmission on stream ID 1; desktop-side reassembly, decoding, and presentation as a virtual webcam ordinary applications can select; camera controls (toggle, facing, resolution) and stream-state indication in both UIs. How the camera is presented is a platform concern — a device the operating system provides, as on Linux, or a component each consuming application loads into its own process.

See `openspec/specs/protocol.md` §3.9 and §7 for the normative wire format these requirements are implemented against.

## Requirements

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

### Requirement: Phone-side camera capture

The Android application SHALL capture camera frames only while a camera stream is active, under the existing foreground service declaring camera foreground use, and SHALL release the camera immediately when the stream stops. Capture failure while the stream is active SHALL stop the stream and surface a camera error state.

#### Scenario: Capture runs only with an active stream

- **WHEN** the camera stream is accepted
- **THEN** camera capture starts, and the platform camera indicator is shown
- **AND** stopping the stream releases the camera immediately

#### Scenario: Capture failure is visible

- **WHEN** camera capture fails to start or aborts while the stream is active
- **THEN** the stream is stopped and both UIs show a camera error state rather than a frozen virtual webcam

### Requirement: Camera permission handling

The phone SHALL request the camera permission before its first capture, SHALL treat a desktop-initiated stream request as a request and not a command, and SHALL refuse the stream when permission is denied so the desktop can show why no video arrived.

#### Scenario: Permission granted on first use

- **WHEN** the user enables the camera for the first time and grants the camera permission
- **THEN** the stream start flow proceeds

#### Scenario: Permission denied refuses the stream

- **WHEN** the camera stream is initiated and the camera permission is denied
- **THEN** no stream start is sent (or the pending request is refused)
- **AND** the desktop UI shows that the camera is unavailable on the phone

### Requirement: Video format negotiation

The camera stream SHALL be started with explicit video parameters — codec, width, height, and maximum frame rate — where MJPEG is the mandatory baseline codec every peer supports. The default offering SHALL be 1280×720 at up to 30 fps. A sink receiving a codec it cannot decode SHALL refuse with reason `unsupported_codec`.

#### Scenario: Baseline format is accepted

- **WHEN** the phone starts the camera stream offering MJPEG 1280×720 at 30 fps
- **THEN** the desktop accepts the format and media may flow

#### Scenario: Unsupported codec is refused

- **WHEN** the desktop receives a camera stream start naming a codec it cannot decode
- **THEN** it refuses the stream with reason `unsupported_codec`
- **AND** no media flows for that stream

#### Scenario: Resolution change replaces the stream parameters

- **WHEN** the user selects a different resolution while the camera stream is active
- **THEN** the phone sends a new stream start with the new dimensions
- **AND** the desktop re-acks, resets receive state, and reopens the virtual camera device at the new format

### Requirement: Camera payload format

Camera video SHALL be carried on stream ID 1, one complete, independently decodable JPEG image per transport frame, with the media header timestamp set to the frame's capture time in microseconds since session start. Loss of any fragment SHALL cost exactly that one video frame.

#### Scenario: A video frame round-trips

- **WHEN** a JPEG frame larger than one datagram is sent over the transport
- **THEN** it is fragmented, reassembled, and delivered byte-identical with its capture timestamp

#### Scenario: Loss costs exactly one frame

- **WHEN** one fragment of a video frame is lost or arrives too late
- **THEN** only that frame is discarded and counted, and decoding continues with the next complete frame

#### Scenario: An undecodable frame does not kill the stream

- **WHEN** a delivered frame fails JPEG decoding on the desktop
- **THEN** that frame is dropped and counted
- **AND** the stream continues with subsequent frames

### Requirement: Frame pacing and gap tolerance

The phone SHALL drop frames rather than queue them when the send path cannot keep up, bounding added latency, and SHALL NOT exceed the negotiated maximum frame rate. The desktop SHALL tolerate an arbitrary gap in arriving frames — the virtual camera holds its last frame — and SHALL resume normally when frames reappear.

#### Scenario: A slow link drops frames instead of adding latency

- **WHEN** frames are produced faster than the send path can transmit them
- **THEN** excess frames are dropped at the source and the delivered video remains current rather than increasingly delayed

#### Scenario: A frame gap freezes rather than breaks the virtual camera

- **WHEN** no frames arrive for an arbitrary period while the stream is active
- **THEN** the virtual camera holds the last delivered frame and resumes when frames arrive again

### Requirement: Camera toggle from either device

Both applications SHALL offer a camera on/off toggle. The phone toggle SHALL start or stop the stream directly as its source; the desktop toggle SHALL drive the same outcome through a sink-initiated stream request, gated on the negotiated `cam` capability.

#### Scenario: Desktop toggle starts the stream

- **WHEN** the desktop user enables the camera toggle while a session is Connected and `cam` is negotiated
- **THEN** the desktop sends a stream request for the camera stream
- **AND** the phone initiates the stream start flow and media flows after acceptance

#### Scenario: Desktop toggle stops the stream

- **WHEN** the desktop user disables the camera toggle while the stream is active
- **THEN** the stream is stopped, the phone releases the camera, and the desktop releases the loopback device

#### Scenario: Phone toggle starts the stream

- **WHEN** the phone user enables the camera toggle while a session is Connected and `cam` is negotiated
- **THEN** the phone initiates the stream start flow directly

### Requirement: Camera facing selection

The phone SHALL offer front/back camera selection. Switching facing SHALL restart capture without stopping the stream or changing its negotiated parameters.

#### Scenario: Facing switch keeps the stream alive

- **WHEN** the user switches between front and back cameras while the stream is active
- **THEN** capture restarts on the selected camera and transmission continues on the same stream
- **AND** the desktop observes at most a brief frame gap

### Requirement: Phone-side preview

The phone SHALL display a local preview of the captured video while the camera stream is active, so the user can frame the shot without looking at the desktop.

#### Scenario: Preview reflects what is transmitted

- **WHEN** the camera stream is active
- **THEN** the phone shows a live preview from the same camera and facing being transmitted

### Requirement: Stream state indication

Both applications SHALL indicate the camera stream state. The desktop SHALL show the negotiated resolution and the delivered frame rate, so a stalled stream (0 fps) is distinguishable from a stopped one; the phone SHALL show capture state and any refusal reason.

#### Scenario: Desktop shows delivered frame rate

- **WHEN** the camera stream is active
- **THEN** the desktop UI shows the negotiated resolution and a delivered-fps figure that reflects arriving frames within one second

#### Scenario: Refusals are attributed

- **WHEN** a camera stream start is refused (permission denied, missing v4l2loopback, unsupported codec)
- **THEN** the refusing side's reason is shown on the peer that initiated, in user-readable form
