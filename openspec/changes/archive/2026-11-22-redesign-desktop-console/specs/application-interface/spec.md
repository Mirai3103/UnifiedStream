## MODIFIED Requirements

### Requirement: Reference-driven platform presentation
The Android client SHALL implement the visual roles, typography hierarchy, component language, and layout character defined by its Markdown and HTML references in `design/`, using native Compose components rather than shipping the presentation-board examples. The desktop client SHALL implement the signal-path console presentation defined by this specification, using native React/Tauri components.

#### Scenario: Desktop renders the streaming-console design
- **WHEN** the desktop client opens at a supported wide window size
- **THEN** its web content fills the Tauri webview and presents a navigation sidebar, a link header, and camera, microphone, and speaker channel strips using one shared set of semantic color, type, and spacing tokens

#### Scenario: Android renders the Material companion design
- **WHEN** the Android client opens on a supported phone
- **THEN** it presents the documented Material 3 tonal surfaces, rounded control groups, title navigation, stream cards, and mobile typography roles without a bottom navigation bar

#### Scenario: Presentation-board decoration is excluded
- **WHEN** either shipped client renders its interface
- **THEN** documentation-only device frames, board labels, surrounding prototype canvas, simulated desktop title bar/traffic-light controls, rounded outer window border, and presentation shadow are not rendered as application content

## ADDED Requirements

### Requirement: Desktop signal-path workspace
The desktop workspace SHALL present the connection and the three media channels as one signal path: a link header stating this device, the peer, the connection state, and link quality together with latency, upload bandwidth, packet loss, and jitter; followed by one strip per channel (camera, microphone, speaker). Each strip SHALL show its state in text, its enable control, its live level or delivered rate while active, its negotiated parameters, and its failure text where present.

#### Scenario: Connected workspace
- **WHEN** a phone is connected and telemetry is arriving
- **THEN** the link header shows the peer's name, the connection state, the link quality as text, and all four telemetry figures, and each channel strip shows its own state and enable control

#### Scenario: Workspace before a phone connects
- **WHEN** the connection state is idle, discovering, or failed
- **THEN** the workspace states that no phone is connected, tells the user how to connect to this device by name, and offers to start advertising when the device is not discoverable
- **AND** channel enable controls are unavailable rather than hidden

#### Scenario: Level meters expose their value
- **WHEN** a microphone or speaker stream is active
- **THEN** its level is exposed to assistive technology as a meter with a name and a numeric value, and an inactive stream exposes a zero value

#### Scenario: Link quality is not color-only
- **WHEN** the link header displays good, degraded, poor, or unknown quality
- **THEN** the quality is stated as text and accompanied by an icon or shape distinct from the other qualities

### Requirement: Appearance preference
The desktop client SHALL offer System, Light, and Dark appearance, defaulting to System, and SHALL remember the choice locally without affecting any connection or stream.

#### Scenario: Default follows the system
- **WHEN** the desktop client opens with no stored preference
- **THEN** it renders the light or dark presentation matching the operating system setting

#### Scenario: Explicit choice is remembered
- **WHEN** the user selects Light or Dark and later reopens the client
- **THEN** the same appearance is applied

#### Scenario: Preference storage is unavailable
- **WHEN** local storage cannot be read or written
- **THEN** the client falls back to System appearance and remains fully operable

### Requirement: Destructive actions require confirmation
The desktop client SHALL require an explicit second step before forgetting trusted devices, and the step SHALL be cancellable.

#### Scenario: Forgetting trusted devices
- **WHEN** the user chooses to forget trusted devices
- **THEN** the interface asks for confirmation and invokes the underlying command only after the user confirms
- **AND** cancelling leaves the trusted devices unchanged and invokes no command
