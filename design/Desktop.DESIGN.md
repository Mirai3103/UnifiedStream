# Design System: Unified Stream Desktop
**Project ID:** 0e3606fe-cab6-4bf2-baf8-c7b53fc38fee

## 1. Visual Theme & Atmosphere

Unified Stream Desktop is a compact, instrument-like control surface for a low-latency media bridge. Its mood is technical, calm, precise, and quietly futuristic. The interface should feel closer to a professional streaming console than a consumer dashboard: information-rich without becoming noisy, with live values, restrained motion, and clear stream states always visible.

Dark mode is the primary expression. Near-black foundations, translucent glass panels, hairline borders, and cool teal light create depth without heavy ornament. A faint violet ambient glow balances the teal and prevents the workspace from feeling monochrome. Light mode preserves the same hierarchy with cool gray surroundings and milky white surfaces.

Use motion only to communicate system life: audio bars breathe, the connected dot pulses, network bars change gently, and the ambient background drifts almost imperceptibly. Avoid decorative illustration, large gradients inside controls, or playful animation. The product should convey trustworthy performance and sub-50-millisecond responsiveness.

## 2. Color Palette & Roles

### Dark mode

- **Obsidian Workspace (#08090B):** Outer application background; establishes the deepest visual layer.
- **Ink-Black Window (#0E1116):** Main application shell and solid control backdrop.
- **Frosted White (#FFFFFF at 4.5% opacity):** Default glass card surface; allows ambient color to remain subtly visible.
- **Raised Frosted White (#FFFFFF at 7.5% opacity):** Selected rows, inner control wells, and elevated secondary surfaces.
- **Whisper Border (#FFFFFF at 10% opacity):** Default hairline separation between panels.
- **Focused Border (#FFFFFF at 18% opacity):** Stronger outlines for interactive and selected elements.
- **Ice-White Text (#E8ECF2):** Primary labels, page titles, and high-priority values.
- **Cool Silver Text (#B6BDC9):** Supporting copy and secondary control labels.
- **Steel Muted Text (#7C8492):** Metadata, units, inactive states, and technical annotations.
- **Signal Teal (#00C4C4):** Primary actions, connected states, active streams, range thumbs, and live charts.
- **Telemetry Violet (#AD9BF6):** Secondary data series, especially bandwidth and speaker output.
- **Diagnostic Amber (#E4A339):** Warnings and conditions requiring attention without indicating failure.
- **Recording Coral (#FF645F):** Recording or destructive stop states.
- **Deep Action Ink (#06131A):** Text placed on bright teal actions to maintain crisp contrast.

### Light mode

- **Cool Mist Workspace (#EEF0F4):** Outer page background.
- **Clean White Window (#FFFFFF):** Main application shell.
- **Milky Glass (#FFFFFF at 62% opacity):** Default translucent panels.
- **Raised Milky Glass (#FFFFFF at 85% opacity):** Selected rows and nested surfaces.
- **Graphite Text (#12161C):** Primary text and headings.
- **Slate Text (#3C444F):** Secondary information.
- **Neutral Muted Text (#6B7280):** Metadata and inactive states.
- **Deep Teal Signal (#008889):** Primary actions and active states with sufficient light-mode contrast.
- **Muted Violet Telemetry (#7461B6):** Secondary charts and speaker data.
- **Burnished Amber (#B67700):** Warning and diagnostic accents.

Color communicates state consistently: teal means active, connected, or healthy; violet distinguishes secondary media telemetry; amber means caution; coral means recording or stop. Never rely on color alone—pair it with a label such as “live,” “off,” “Stable,” or “REC.”

## 3. Typography Rules

Use **Space Grotesk** as the primary interface family. Its geometric but approachable forms give the product a modern engineering character. Use **JetBrains Mono** for IP addresses, versions, codecs, latency, bandwidth, percentages, timestamps, driver paths, and other machine-readable values.

- Page titles use Space Grotesk at approximately 24 pixels, semibold weight, with slightly tightened letter spacing.
- Card titles and important device names use 13–16 pixels at medium or semibold weight.
- Standard interface copy uses 12–13 pixels at regular or medium weight.
- Technical values use JetBrains Mono at 10–13 pixels; headline telemetry may grow to 26 pixels.
- Section eyebrows use 10–10.5 pixels, uppercase, with generous tracking around 0.10–0.14 em.
- Supporting descriptions use the muted palette and comfortable line height rather than reduced opacity alone.

Keep labels concise and sentence case. Reserve uppercase for small structural labels such as “WORKSPACE,” “SOURCES,” and measurement headings. Numeric values and their units should sit on the same baseline, with units visibly quieter.

## 4. Component Stylings

* **Buttons:** Primary actions use a solid Signal Teal fill, Deep Action Ink text, medium weight, and compact corners around 10 pixels. Secondary actions use a translucent raised surface with a focused hairline border. Small option chips use gently rounded 8–9 pixel corners; status and identity controls may be fully pill-shaped. Hover states should strengthen the border or slightly reduce fill opacity without moving the control.
* **Cards/Containers:** The outer desktop window has confidently rounded 18-pixel corners and a deep, diffused shadow of approximately 24 by 60 pixels at 55% black. Main content cards use 14–16 pixel rounding, translucent surfaces, one-pixel whisper borders, and 14–16 pixel backdrop blur. Nested rows use 9–12 pixel corners. Depth comes from transparency, borders, and blur rather than stacking multiple shadows.
* **Inputs/Forms:** Range tracks are thin four-pixel pills with a 14-pixel teal thumb, a dark inner border, and a subtle outline. Toggle tracks are compact 46 by 26 pixel pills; the knob slides between edges and changes the track from neutral glass to teal. Segmented choices use small outlined chips and express selection through teal border, softly tinted fill, and brighter text. Keep all controls visibly operable in both themes.
* **Status Indicators:** Use small circular lights between 6 and 8 pixels beside explicit state text. Connected indicators pulse slowly; inactive indicators use muted steel. Recording badges sit on a dark translucent pill over the camera preview and pair coral with a “REC” label.
* **Telemetry:** Prominent network values use large monospaced numerals, compact units, and minimal chart marks. Audio visualizers use narrow teal or violet bars inside a subtly raised well. Motion should be irregular enough to feel live but smooth enough to avoid visual stress.
* **Navigation:** The sidebar uses compact rounded rows and a quiet dot icon. Selected navigation should receive a raised translucent background, focused border, and brighter text. Keyboard hints remain monospaced and muted. The top chrome uses three small traffic-light circles, a version label, host status, and a pill-shaped theme control.

## 5. Layout Principles

Design for a desktop canvas up to 1420 by 880 pixels, centered within 28 pixels of outer breathing room. The application shell is divided into a fixed 46-pixel window chrome and a two-column workspace: a 252-pixel navigation sidebar plus a flexible main area.

The main content uses 22–30 pixels of inset spacing and a disciplined 12–18 pixel gap rhythm. Begin with a title/action row, follow with a four-column telemetry strip, then place media controls in larger modular cards. Camera receives more horizontal area because it includes a preview; microphone and speaker may share equal columns below. Network and Settings views reuse the same card language and align content in balanced one- or two-column grids.

Maintain dense but readable grouping:

- Keep related labels and values 2–8 pixels apart.
- Separate control clusters by 12–16 pixels.
- Use 18–26 pixels between major sections.
- Align numeric telemetry vertically and preserve consistent unit placement.
- Keep the device identity, source states, and operating-system backend visible in the sidebar.

The interface is desktop-first, but narrower layouts should collapse the four telemetry cards to two columns, stack the camera preview above its controls, and eventually place the sidebar behind a navigation drawer. Do not simply scale down text; preserve minimum control sizes and semantic grouping.
