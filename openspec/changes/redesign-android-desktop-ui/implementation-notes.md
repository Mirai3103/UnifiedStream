## Functional inventory and destination map

Implementation branch: `feat/redesign-ui` (confirmed non-`main`).

### Desktop integration contract

The stable shell owns `get_status`; advertising start/stop; disconnect; trusted-device reset; synthetic stream start/stop and configuration; microphone, camera, speaker, speaker mute, and speaker routing commands; pairing accept/reject; and clipboard copy for the v4l2loopback hint. It subscribes once to connection, pairing, telemetry, test-stream, microphone status/level, speaker status/level, and camera status/stat events.

| Existing surface | Redesigned destination |
| --- | --- |
| Connection status, telemetry, camera, microphone, speaker | Workspace |
| Advertising, identity, ports, capabilities, disconnect, trusted-device reset | Network |
| Synthetic transport diagnostics and counters | Network |
| Theme and presentation behavior | Settings |
| Pairing requests and command/global errors | Global shell above every destination |

Connection states retained: idle, discovering, connecting, connected, reconnecting with attempt count, and failed with every existing reason. Media states retain inactive, pending/starting, active parameters, errors, device/backend hints, routing, mute, levels, and camera delivered/decode counters. Metrics clear on disconnect exactly as before.

### Android integration contract

The stable activity collects identity, discovery devices/state/manual fallback/error, connection/dashboard/test-stream, all microphone state/control flows, all camera state/control flows, and all speaker state/control flows. It retains discovery start/stop, device/manual connect, disconnect, reconnect cancel, test-stream control, media enable/disable, permission launch/denial, camera facing/resolution/preview, microphone mute/gain/noise suppression, and speaker mute/volume callbacks. Foreground-service ownership remains tied to connection/camera state rather than destination.

| Existing surface | Redesigned destination |
| --- | --- |
| Connected host, compact telemetry, all three stream summaries/controls | Home |
| Camera preview, permission, facing, resolution, start/stop, refusal/error | Camera |
| Discovery, no-network, results, manual address, connection initiation | Devices |
| Identity, session details, theme, transport diagnostics, disconnect/cancel | Settings |

Connection and media state are not destination-local. Discovery runs while Devices is selected. Navigating or changing theme does not invoke media/session callbacks.

Android navigation is stack-based with no bottom bar. The app launches at Devices; starting a connection pushes Home; “Open camera controls” pushes Camera; Settings is available from every title bar; Back restores the previous screen and exits normally from the root Devices screen.

### Prototype-only content policy

Static example values, unavailable battery data, backend selection, unsupported frame-rate controls, notifications preferences, and other controls without an existing runtime contract are not fabricated. They are omitted or shown only as clearly unavailable explanatory copy. Existing diagnostics absent from the prototypes remain available in Network (desktop) or Settings (Android).

## Font and asset decision

Desktop bundles `@fontsource/space-grotesk` and `@fontsource/jetbrains-mono` from the lockfile. Both packages carry the SIL Open Font License and make production/offline rendering independent of network access; CSS retains system sans-serif and monospace fallbacks.

Android uses the platform-bundled Roboto family and platform monospace through Compose, avoiding a downloadable-font runtime dependency. The monospace role is isolated so a bundled JetBrains Mono resource can replace it later without changing component APIs. No remote font request is made at runtime.

Desktop action/navigation glyphs come from the pinned `lucide-react` package; Android title actions use Compose Material Icons. The UnifiedStream launcher identity remains custom rather than borrowing a generic library glyph: `desktop/src-tauri/icons/icon.svg` is the retained U-plus-Play vector master, with Tauri bundle exports plus matching Android adaptive, monochrome, round, and legacy resources. The larger optical footprint replaces the Android template artwork and the previous undersized desktop mark.

## Intentional reference adaptations

- HTML presentation boards and fake device frames are documentation only.
- Following design clarification, the desktop runtime fills the Tauri webview and explicitly excludes the example's simulated title bar, traffic-light controls, fixed rounded outer frame, presentation border/shadow, and surrounding canvas.
- Real backend states, errors, ports, capabilities, and controls replace illustrative values.
- Layout geometry reflows when needed for keyboard access, font scaling, system insets, and minimum targets.
- Android title actions use icon buttons with 48-dp minimum targets, and Material `TopAppBar` consumes the status-bar inset while edge-to-edge drawing remains enabled.

## Automated verification

- `openspec validate --all`: 10 items passed.
- Desktop frozen install, six Vitest integration tests, TypeScript compilation, and Vite production build passed.
- Android JVM unit tests, debug APK assembly, and Compose instrumentation-test APK compilation passed.
- Rust formatting, warning-denied Clippy, and all workspace tests passed (263 tests across unit and integration targets).
- Desktop responsive rules cover full-webview wide, compact-navigation, two-column, stacked, and single-column breakpoints; controls use visible `:focus-visible` styling and reduced-motion CSS.
- Android screens use scrolling content, scaffold/title-bar insets, Material minimum targets, explicit Back/Settings icon semantics, non-color state text, and system-aware Compose animation. The title-action test also enforces 48-dp minimum dimensions.
- Desktop 32/128/256/512 PNG, Windows ICO, macOS ICNS, Windows Store sizes, and Android mdpi through xxxhdpi legacy/round resources were regenerated and dimension-checked; adaptive and monochrome Android vectors are packaged by the debug builds.

## Manual verification still required

The available Android device `3B65BT02H6G00000` is reported as `offline` by ADB. Consequently, the Compose instrumentation suite, physical-device light/dark comparison, camera/permission/media end-to-end paths, and cross-device active-stream continuity remain pending. Desktop runtime startup was confirmed through `tauri dev`, but representative visual screenshots and the full paired-device scenario remain pending before archive and merge.
