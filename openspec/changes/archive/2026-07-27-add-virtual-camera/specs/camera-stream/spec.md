# Camera Stream Specification (Delta)

## ADDED Requirements

### Requirement: Desktop virtual camera device

The desktop SHALL present received camera video as a v4l2loopback virtual webcam that ordinary applications can select as a camera, writing decoded frames to the device only while a camera stream is active. When no suitable v4l2loopback device exists, the desktop SHALL refuse the stream and SHALL show actionable setup guidance including the module-load command.

#### Scenario: Applications see a normal webcam

- **WHEN** the camera stream is accepted by the desktop and frames are arriving
- **THEN** a v4l2 device labeled "UnifiedStream Camera" delivers the phone's video to applications that select it

#### Scenario: Missing v4l2loopback is reported with guidance

- **WHEN** a camera stream start arrives and no v4l2loopback device is available
- **THEN** the desktop refuses the stream with reason `internal`
- **AND** the desktop UI shows that v4l2loopback is missing, with the exact command to load it

#### Scenario: The device is released when the stream ends

- **WHEN** the camera stream is stopped or the session ends for any reason
- **THEN** the desktop stops writing and releases the loopback device
- **AND** no writer remains attached after the desktop app exits

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
