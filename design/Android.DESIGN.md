# Design System: Unified Stream Android
**Project ID:** 0e3606fe-cab6-4bf2-baf8-c7b53fc38fee

## 1. Visual Theme & Atmosphere

Unified Stream Android is an expressive Material 3 companion experience for controlling camera, microphone, speaker, and PC connections. It feels calm, tactile, and mobile-native. Large tonal surfaces and generous rounding make technical streaming controls approachable, while live metrics and concise labels preserve the product’s professional character.

The atmosphere is softer and more spacious than the desktop client. Dark mode uses green-black surfaces with luminous mint accents; light mode uses clean, slightly green-tinted neutrals. Components should feel carved from neighboring tonal layers rather than floating as glass. Strong color is reserved for connection, activation, recording, and the principal action.

Motion communicates live state rather than decoration. Audio bars breathe, discovery and recording indicators pulse, and switches change shape and position with a quick Material-style transition. The camera viewfinder is the only intentionally immersive area; it uses a near-black field with restrained overlays so the video feed remains dominant.

## 2. Color Palette & Roles

### Dark mode

- **Deep Forest Surface (#0E1413):** Primary phone canvas and navigation foundation.
- **Tonal Forest Container (#171D1C):** Standard cards, bottom navigation, and grouped settings.
- **Raised Forest Container (#212827):** Audio wells, icon backgrounds, and nested control surfaces.
- **Highest Forest Container (#2B3231):** Inactive tracks and maximum tonal elevation.
- **Soft Porcelain Text (#DEE4E2):** Primary headings and body content.
- **Sage-Gray Text (#9AA5A2):** Supporting descriptions, labels, and inactive navigation.
- **Mineral Outline (#56605E):** Strong control outlines and switch borders.
- **Quiet Outline (#3B4342):** Low-emphasis separators and container edges.
- **Luminous Mint (#7AD7C6):** Primary actions, live indicators, active tracks, and progress.
- **Deep Mint Ink (#00382F):** Text and icons placed on the luminous primary color.
- **Teal Tonal Container (#005046):** Connected device hero cards and active icon containers.
- **Pale Mint Content (#9DF1E0):** Text and symbols placed on deep teal containers.
- **Muted Sage Container (#2C4B46):** Selected navigation and lower-priority highlighted information.
- **Pale Sage Content (#CDE8E1):** Content placed on muted sage containers.
- **Soft Error Coral (#FFB4AB):** Stop-camera and destructive streaming states.
- **Canvas Night (#0A0D0D):** Presentation-board background outside the phone frames.
- **Recording Coral (#FF5A4D):** Blinking live-recording dot in the viewfinder.

### Light mode

- **Minted White Surface (#F5FAF8):** Primary phone background.
- **Soft Sage Container (#E9EFED):** Standard grouped cards and navigation.
- **Raised Sage Container (#E3EAE8):** Nested controls and icon wells.
- **Highest Sage Container (#DDE4E2):** Inactive tracks and elevated neutral areas.
- **Forest Ink (#171D1B):** Primary text.
- **Slate Sage Text (#3F4947):** Secondary labels and descriptions.
- **Mineral Gray Outline (#6F7977):** Strong outlines.
- **Pale Outline (#BFC9C6):** Quiet borders and separators.
- **Grounded Teal (#00695F):** Primary actions and active states.
- **Pure White Content (#FFFFFF):** Content placed on grounded teal.
- **Pale Mint Container (#9DF1E0):** Selected and active tonal surfaces.
- **Deep Teal Content (#00201C):** Text placed on pale mint.
- **Pale Sage Selection (#CDE8E1):** Selected navigation and secondary highlights.
- **Deep Forest Selection Text (#062019):** Content on pale sage selection surfaces.
- **Material Error Red (#BA1A1A):** Destructive and stop states.
- **Cool Page Mist (#EEF2F0):** Presentation-board background around phone frames.

Use mint and teal to mean active, connected, discoverable, or progressing. Error coral or red is reserved for stopping an active capture or communicating a destructive action. Tonal elevation should do most of the grouping work; avoid unnecessary outlines around every card.

## 3. Typography Rules

Use **Roboto** throughout the Android experience to preserve native Material character. Use **JetBrains Mono** selectively for latency, bandwidth, packet loss, frame rate, battery usage, and other compact technical measurements.

- Screen titles use Roboto at 22–30 pixels, regular to medium weight, with subtly tightened letter spacing.
- Card and device titles use 15–16 pixels at medium weight.
- Standard body copy and control labels use 12.5–15 pixels.
- Supporting metadata uses 11–12.5 pixels and the sage-gray text color.
- Live measurements use JetBrains Mono at 19–26 pixels for prominent values and 11–12 pixels for compact readouts.
- Section labels use 12 pixels, uppercase, with approximately 0.08 em tracking.
- Presentation-board labels use 12 pixels, uppercase, with approximately 0.10–0.16 em tracking.

Use sentence case for actions and navigation. Keep technical descriptions short and scannable, such as “Opus 48 kHz” or “Wi-Fi 6 · 192.168.1.10.” Pair every metric with a plain-language label so the interface remains understandable outside engineering contexts.

## 4. Component Stylings

* **Buttons:** The primary extended action is 56 pixels high with comfortably rounded 18-pixel corners, a mint or grounded-teal fill, contrasting text, and a soft 6 by 18 pixel shadow at 22% black. Compact actions use 9–12 pixel corners; lightweight actions and filter chips may be pill-shaped. Destructive camera controls switch to the error color and can tighten their inner stop symbol to a squared shape.
* **Cards/Containers:** Major streaming cards use expressive 26-pixel corners and tonal fills without outlines. Device rows use 22-pixel corners, settings groups use 24-pixel corners, and bottom sheets use 28-pixel top corners. Nested wells and status rows use 14–16 pixel corners. Elevation is primarily tonal and flat; reserve shadows for the floating primary action and phone frame.
* **Inputs/Forms:** Material switches measure approximately 52 by 32 pixels and are fully pill-shaped. Active switches use the primary color with a 24-pixel contrasting thumb; inactive switches use the highest neutral container, a mineral border, and a smaller 16-pixel thumb. Segmented resolution controls share one pill-shaped outline, while frame-rate choices use separate 12-pixel rounded segments. Search fields are 52 pixels high and fully pill-shaped.
* **Chips:** Selection chips use 8-pixel corners in dense cards and pill shapes over the camera viewfinder. Active choices receive a tonal mint or sage fill; inactive choices are transparent or outlined. Keep chip labels around 12.5 pixels and preserve comfortable horizontal padding.
* **Navigation:** Bottom navigation is 78 pixels high. Each destination uses a 62 by 32 pixel pill indicator above a 12-pixel medium-weight label. The active destination receives a muted sage container and stronger foreground color; inactive destinations remain transparent and quiet.
* **Camera Controls:** The viewfinder uses a dark neutral field with white overlays. Status and setting overlays sit on translucent black pills. The lower control sheet uses the normal surface palette, a small centered drag handle, large 56-pixel control targets, and grouped resolution and frame-rate choices.
* **Telemetry and Progress:** Use monospaced values, mint progress tracks, and gently animated narrow audio bars. Battery, volume, and live-network values should favor simple bars and numbers over decorative charts.

## 5. Layout Principles

Design each Android screen for a 428 by 908 pixel phone frame. Use 16 pixels of horizontal inset for primary content and a vertical rhythm centered on 12–16 pixel gaps. Touch targets should be at least 40 pixels, preferably 48–56 pixels for primary controls.

The Home screen follows a clear vertical sequence: compact app bar, large screen title, connected-host summary, camera card, microphone card, speaker card, extended primary action, then bottom navigation. The Camera screen dedicates its upper region to the viewfinder and anchors controls in a rounded bottom sheet. Devices uses a search field followed by network results, pairing options, and a contextual notice. Settings groups related switches inside large tonal containers, then places session battery information in a separate card.

Maintain the following spatial behavior:

- Use 12 pixels between related cards and 14–18 pixels inside containers.
- Use larger 22–28 pixel gaps only when separating screen regions or presentation frames.
- Align switch controls to the trailing edge and let label/description stacks occupy the flexible width.
- Keep the primary streaming action near the lower-right area but above bottom navigation.
- Keep bottom navigation stable across Home, Devices, and Settings.
- Allow content to scroll vertically without shrinking type or touch targets.

When presenting multiple screens on a design canvas, place the four phone frames—Home, Camera, Devices, and Settings—in a wrapping row with approximately 46 pixels between them. This presentation spacing is documentation-only; it must not be reproduced inside the mobile application itself.
