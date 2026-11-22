# Design System: Unified Stream Android

> **Status:** Material 3 Expressive. `design/Android.html` is the earlier baseline Material 3 presentation board. It is kept as history only and no longer describes the shipped app; this document is normative.

## 1. Visual Theme & Atmosphere

Unified Stream Android is a Material 3 Expressive companion for controlling camera, microphone, speaker, and PC connections. It is built on `androidx.compose.material3` 1.5.0 with `MaterialExpressiveTheme`. The product keeps its identity colors (mint and teal on green-black or minted white) and lets Expressive supply the rest: spring motion, shape, and emphasized type.

The point of Expressive here is to make live state legible at a glance. Shape, motion, and progress style say what is happening (connecting, live, muted, failed) alongside color and text, never instead of them. Large rounded surfaces and generous spacing keep technical controls approachable; monospaced measurements keep the product's professional character.

The camera viewfinder is the only intentionally immersive area. It uses a near-black field with restrained overlays so the video feed stays dominant.

## 2. Color Palette & Roles

The palette is fixed. Dynamic (wallpaper) color is intentionally not used, because mint must always mean connected or live.

### Dark mode

- **Deep Forest Surface (#0E1413):** Screen background and app bar.
- **Tonal Forest Container (#171D1C):** Stream cards, settings cards, scrolled app bar.
- **Raised Forest Container (#212827):** Metric tiles, notices, nested surfaces.
- **Highest Forest Container (#2B3231):** Inactive badges and tracks.
- **Soft Porcelain Text (#DEE4E2)** and **Sage-Gray Text (#9AA5A2):** Primary and supporting text.
- **Mineral Outline (#56605E)** and **Quiet Outline (#3B4342):** Strong and quiet outlines.
- **Luminous Mint (#7AD7C6)** on **Deep Mint Ink (#00382F):** Primary actions, live badges, progress.
- **Teal Tonal Container (#005046)** with **Pale Mint Content (#9DF1E0):** Connected hero and good-quality state.
- **Muted Sage Container (#2C4B46)** with **Pale Sage Content (#CDE8E1):** Connecting and secondary highlights.
- **Amber (#E4A339)** with container **#5B4300** and content **#FFDEA6:** Degraded link quality (tertiary role).
- **Soft Error Coral (#FFB4AB)** with container **#93000A** and content **#FFDAD6:** Failed connection, refused streams, stop actions.
- **Recording Coral (#FF5A4D):** The recording dot over the viewfinder.

### Light mode

- **Minted White Surface (#F5FAF8)**, **Soft Sage Container (#E9EFED)**, **Raised Sage (#E3EAE8)**, **Highest Sage (#DDE4E2)**.
- **Forest Ink (#171D1B)** and **Slate Sage Text (#3F4947)**.
- **Grounded Teal (#00695F)** on white for primary; **Pale Mint (#9DF1E0)** on **Deep Teal (#00201C)** for primary containers.
- **Pale Sage (#CDE8E1)** on **Deep Forest (#062019)** for secondary containers.
- **Amber (#7B5800)** with container **#FFDEA6** and content **#271900** for degraded quality.
- **Material Error Red (#BA1A1A)** with container **#FFDAD6** and content **#410002**.

Use mint and teal for active, connected, and progressing. Amber is for degraded. Error colors are reserved for failure and for stopping an active capture. Tonal elevation does the grouping; avoid outlines around cards.

## 3. Typography Rules

Roboto (the platform default) throughout, with **JetBrains Mono**, falling back to the system monospace, for latency, bandwidth, loss, jitter, resolution, ports, and identifiers.

- Use Expressive emphasized roles for hierarchy: `headlineMediumEmphasized` for the connection hero, `titleLargeEmphasized` for card titles and metric values, `titleMediumEmphasized` for section headers and list item titles.
- Body and supporting copy use `bodyMedium` and `bodySmall` in the on-surface-variant color.
- Use sentence case for actions, labels, and headings. There are no uppercase tracked labels.
- Every metric pairs with a plain-language label ("Latency", "Upload", "Loss", "Jitter").

## 4. Shape & Motion

- **Shapes:** Use the Expressive shape scale. Hero and viewfinder use `extraLargeIncreased`; stream, settings, and form cards use `extraLarge`; tiles, notices, and fields use `large`; chips, pills, and the search field are fully rounded.
- **Shape as state:** the connection badge is a cookie when connected, a soft burst with an error icon on failure, and a clover when offline. While connecting or reconnecting it is a `ContainedLoadingIndicator`. Stream badges are a cookie when live and a circle when off.
- **Motion scheme:** `MotionScheme.expressive()`. Color and level changes use the scheme's effects spec; toggle buttons, button groups, and list items use their built-in spring shape morphing.
- **Reduced motion:** when the system animation scale is zero the wavy meters render flat and no continuous motion runs. The same states remain readable from shape, icon, and text.

## 5. Component Stylings

- **Top app bar:** `LargeFlexibleTopAppBar`, collapsing on scroll. Title is the destination; the subtitle is the live connection summary, so state is visible on every screen. Back and Settings are icon buttons with 48 dp targets and labels "Back" and "Open settings".
- **Connection hero:** A single surface that changes container color with state (primary container connected, secondary container working, error container failed, neutral offline), holding the badge, a state word, the title, a detail line, and the "Find a computer" action when disconnected.
- **Stream card:** Badge, title, status line, and an on/off `ToggleButton` whose shape morphs between states and which always carries the words "On", "Off", or "Starting". Stream controls sit beneath: wavy level meter, mute toggle, slider, and optional switch.
- **Buttons:** Use the Expressive `shapes` variants so presses morph the shape. Start/Stop camera is a large button; Stop uses the error color pair. Secondary actions are tonal or outlined.
- **Choices:** Resolution is a connected button group of toggle buttons with leading, middle, and trailing shapes and the radio-button role.
- **Level meters:** `LinearWavyProgressIndicator`; amplitude is full while the stream is live and unmuted, flat otherwise.
- **Lists:** Devices and appearance settings use `SegmentedListItem` with `ListItemDefaults.segmentedShapes`, so a group reads as one connected surface with rounded outer corners.
- **Fields:** The search field is fully rounded with a leading search icon and a clear action; form fields use the `large` shape and inline error text.
- **Switches:** Settings switches show a check glyph in the thumb when on.
- **Quality pill:** Icon plus word (Good, Degraded, Poor, Unknown) on the matching container color.
- **Camera viewfinder:** Near-black field in an `extraLargeIncreased` surface, a translucent pill for REC and one for the resolution in monospace.

## 6. Layout Principles

Use 16 dp of horizontal inset, 12 dp between cards, 20 dp inside cards, and 24 dp inside the hero. Touch targets are at least 48 dp; the large primary action is at least the Expressive large button height. Content scrolls under the collapsing app bar and keeps a 32 dp bottom inset. Text scales with the system font size without hiding actions.

Navigation is a stack with no bottom bar: Devices is the root, a connection advances to Home, "Open camera controls" pushes Camera, and Settings opens above the current screen from the app bar on every screen. Back pops one screen, or exits from Devices. Stream and session state are independent of navigation.

- **Home:** connection hero, network section (quality pill and four metric tiles), then camera, microphone, and speaker cards.
- **Camera:** viewfinder, explanation notice, resolution group, facing toggle, start/stop action.
- **Devices:** search, discovery status with a loading indicator, segmented device list, and a connect-by-address form when discovery is empty, blocked, or failed.
- **Settings:** appearance, this phone (identity, session, disconnect or cancel reconnect), and transport diagnostics.
