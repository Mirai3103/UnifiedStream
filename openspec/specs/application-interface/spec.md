# Application interface

## Purpose

Define the shared presentation, navigation, accessibility, and live-state requirements for the UnifiedStream Android and desktop clients.

## Requirements

### Requirement: Reference-driven platform presentation
The Android and desktop clients SHALL implement the visual roles, typography hierarchy, component language, and layout character defined by their respective Markdown and HTML references in `design/`, while using native Compose and React/Tauri components rather than shipping the presentation-board examples.

#### Scenario: Desktop renders the streaming-console design
- **WHEN** the desktop client opens at a supported wide window size
- **THEN** its web content fills the Tauri webview and presents the documented navigation sidebar, telemetry hierarchy, glass-panel workspace, and media controls using the desktop color and typography roles

#### Scenario: Android renders the Material companion design
- **WHEN** the Android client opens on a supported phone
- **THEN** it presents the documented Material 3 tonal surfaces, rounded control groups, title navigation, stream cards, and mobile typography roles without a bottom navigation bar

#### Scenario: Presentation-board decoration is excluded
- **WHEN** either shipped client renders its interface
- **THEN** documentation-only device frames, board labels, surrounding prototype canvas, simulated desktop title bar/traffic-light controls, rounded outer window border, and presentation shadow are not rendered as application content

### Requirement: Light and dark themes
Both clients SHALL provide complete light and dark presentations using the semantic roles in their platform reference, and all text, controls, status indicators, previews, and telemetry values MUST remain legible and operable in both themes.

#### Scenario: Theme changes without losing state
- **WHEN** the user switches between light and dark themes during an active connection
- **THEN** the interface updates its visual tokens without disconnecting, stopping streams, resetting navigation, or clearing live state

#### Scenario: Status is not color-only
- **WHEN** a connected, active, warning, recording, or failed state is displayed in either theme
- **THEN** color is paired with text, iconography, shape, or another non-color cue identifying the state

### Requirement: Stable platform navigation
The redesign SHALL organize content into explicit platform-appropriate destinations while retaining connection and stream state independently from navigation.

#### Scenario: Android launch and connection flow
- **WHEN** the Android client launches and the user starts connecting to a device
- **THEN** the client initially shows Devices and then advances to Home without displaying a bottom navigation bar

#### Scenario: Android camera flow
- **WHEN** the user selects “Open camera controls” from Home
- **THEN** the client pushes Camera and Back returns to Home without changing the camera stream

#### Scenario: Android title navigation
- **WHEN** any Android screen is displayed
- **THEN** its title bar exposes icon-based Back and Settings actions with meaningful accessible labels and minimum 48-dp targets, Settings opens above the current screen, and Back restores the previous screen or exits normally from the root Devices screen

#### Scenario: Android title bar avoids system controls
- **WHEN** edge-to-edge content renders on a device with a status-bar inset
- **THEN** the title-bar controls are laid out below that inset and remain fully visible and operable

#### Scenario: Desktop workspace navigation
- **WHEN** the user navigates among the desktop workspace, network, and settings views
- **THEN** the main content changes while Tauri event subscriptions and current backend state remain owned by the stable application shell

#### Scenario: Time-sensitive global prompt
- **WHEN** a pairing request or connection-level failure arrives on a non-default destination
- **THEN** the interface exposes an actionable prompt or notice without requiring the user to discover a different destination first

### Requirement: Existing feature parity
The redesigned interfaces MUST retain access to every existing user action, live value, lifecycle state, permission state, failure reason, and diagnostic currently exposed for discovery, pairing, connection control, camera, microphone, speaker, telemetry, and the synthetic test stream.

#### Scenario: Existing command remains reachable
- **WHEN** an action was available before the redesign and its existing preconditions are satisfied
- **THEN** the user can invoke the same underlying command or ViewModel callback from an appropriate destination in the redesigned interface

#### Scenario: Existing state remains visible
- **WHEN** the backend or ViewModel reports an active, pending, refused, unavailable, permission-required, reconnecting, disconnected, or failed state
- **THEN** the redesigned interface displays that state and its available recovery action without replacing it with fabricated prototype data

#### Scenario: Navigation during streaming
- **WHEN** the user changes destination while one or more media streams are active
- **THEN** navigation does not stop, restart, renegotiate, mute, or otherwise modify those streams

### Requirement: Live information remains coherent
The application shell SHALL render connection state, telemetry, stream levels, delivered camera rate, and alerts from the existing live data sources, and MUST prevent stale values from appearing current after their owning session or stream ends.

#### Scenario: Live telemetry update
- **WHEN** a connected client receives a telemetry update
- **THEN** every visible representation of the affected latency, throughput, loss, jitter, or quality value updates from that same live snapshot

#### Scenario: Disconnection clears session metrics
- **WHEN** the active session disconnects
- **THEN** live session metrics and stream-only measurements are cleared or explicitly marked unavailable while historical-looking values are not presented as current

#### Scenario: Stream level becomes inactive
- **WHEN** an audio stream stops or becomes muted according to its existing semantics
- **THEN** its live meter and accompanying label visibly represent the inactive or zero-level state

### Requirement: Responsive and adaptive layout
The desktop client SHALL reflow from the documented wide workspace to narrower layouts, and the Android client SHALL support its supported screen sizes and content scaling without shrinking essential controls or clipping required content.

#### Scenario: Narrow desktop window
- **WHEN** the desktop window becomes too narrow for the fixed sidebar and multi-column workspace
- **THEN** navigation becomes compact or drawer-based, telemetry uses fewer columns, media content stacks, and every existing control remains reachable

#### Scenario: Scrollable Android content
- **WHEN** an Android destination does not fit vertically because of screen size, system insets, or font scaling
- **THEN** content scrolls below the title bar while primary controls remain usable and final content is not obscured

#### Scenario: Camera content adapts
- **WHEN** camera controls render at a constrained width
- **THEN** preview, capture status, facing, resolution, and stream actions reflow without distorting the preview or reducing required touch targets

### Requirement: Accessible interaction
All redesigned interactive elements SHALL expose an accessible name and state, support the input methods expected on their platform, and preserve visible focus and minimum operable target sizes.

#### Scenario: Desktop keyboard operation
- **WHEN** a keyboard user traverses desktop navigation, buttons, switches, dialogs, and form controls
- **THEN** focus order follows the visual hierarchy, focus remains visible, and each action can be completed without pointer input

#### Scenario: Android semantic controls
- **WHEN** Android accessibility services inspect navigation items, stream toggles, selectors, meters, and connection actions
- **THEN** each actionable control exposes a meaningful label, role, enabled state, and selected or checked state where applicable

#### Scenario: Enlarged text
- **WHEN** the user enables supported enlarged text or display scaling
- **THEN** labels and values reflow without hiding required actions or communicating state through truncated text alone

### Requirement: State-driven restrained motion
Motion SHALL communicate live or transitional state, SHALL NOT be required to understand or operate the interface, and SHALL respect platform reduced-motion or disabled-animation preferences.

#### Scenario: Live state animation
- **WHEN** connection, discovery, recording, switch, or audio-level state changes and animation is enabled
- **THEN** the corresponding restrained indicator may animate without shifting surrounding controls or blocking input

#### Scenario: Reduced motion
- **WHEN** the operating system or browser requests reduced motion or disables animations
- **THEN** nonessential movement stops and the same state remains understandable through static visual and textual cues

### Requirement: Platform launcher identity
The desktop and Android packages SHALL use a cohesive UnifiedStream launcher mark that remains recognizable at required platform sizes and uses the available safe area without excessive empty padding.

#### Scenario: Desktop bundle icon
- **WHEN** a supported desktop bundle or window icon is rendered
- **THEN** it uses the UnifiedStream mark at an optically legible scale rather than a placeholder or undersized symbol

#### Scenario: Android launcher icon
- **WHEN** Android renders the adaptive, round, monochrome, or legacy launcher icon
- **THEN** it uses the UnifiedStream mark within platform mask and safe-zone constraints rather than the Android project-template icon
