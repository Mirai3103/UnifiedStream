## MODIFIED Requirements

### Requirement: Reference-driven platform presentation
The Android and desktop clients SHALL implement the visual roles, typography hierarchy, component language, and layout character defined by their respective Markdown references in `design/`, while using native Compose and React/Tauri components rather than shipping the presentation-board examples. The Android client SHALL be built on Material 3 Expressive.

#### Scenario: Desktop renders the streaming-console design
- **WHEN** the desktop client opens at a supported wide window size
- **THEN** its web content fills the Tauri webview and presents the documented navigation sidebar, telemetry hierarchy, glass-panel workspace, and media controls using the desktop color and typography roles

#### Scenario: Android renders the Material companion design
- **WHEN** the Android client opens on a supported phone
- **THEN** it presents Material 3 Expressive components, including the expressive motion scheme, expressive shapes, emphasized type roles, a collapsing flexible title bar, toggle buttons for streams, and connected button groups for choices, using the UnifiedStream color palette and without a bottom navigation bar

#### Scenario: Presentation-board decoration is excluded
- **WHEN** either shipped client renders its interface
- **THEN** documentation-only device frames, board labels, surrounding prototype canvas, simulated desktop title bar/traffic-light controls, rounded outer window border, and presentation shadow are not rendered as application content

### Requirement: State-driven restrained motion
Motion SHALL communicate live or transitional state, SHALL NOT be required to understand or operate the interface, and SHALL respect platform reduced-motion or disabled-animation preferences. Android MAY use spring-based shape morphing and expressive progress indicators for this purpose.

#### Scenario: Live state animation
- **WHEN** connection, discovery, recording, stream activation, or audio-level state changes and animation is enabled
- **THEN** the corresponding indicator may animate its shape, position, or amplitude without shifting surrounding controls or blocking input

#### Scenario: Reduced motion
- **WHEN** the operating system or browser requests reduced motion or disables animations
- **THEN** nonessential movement stops and the same state remains understandable through static shape, iconography, and textual cues
