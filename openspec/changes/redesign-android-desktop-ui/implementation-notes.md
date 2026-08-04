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

### Prototype-only content policy

Static example values, unavailable battery data, backend selection, unsupported frame-rate controls, notifications preferences, and other controls without an existing runtime contract are not fabricated. They are omitted or shown only as clearly unavailable explanatory copy. Existing diagnostics absent from the prototypes remain available in Network (desktop) or Settings (Android).

## Font and asset decision

Desktop bundles `@fontsource/space-grotesk` and `@fontsource/jetbrains-mono` from the lockfile. Both packages carry the SIL Open Font License and make production/offline rendering independent of network access; CSS retains system sans-serif and monospace fallbacks.

Android uses the platform-bundled Roboto family and platform monospace through Compose, avoiding a downloadable-font runtime dependency. The monospace role is isolated so a bundled JetBrains Mono resource can replace it later without changing component APIs. No remote font request is made at runtime.

## Intentional reference adaptations

- HTML presentation boards and fake device frames are documentation only.
- Real backend states, errors, ports, capabilities, and controls replace illustrative values.
- Layout geometry reflows when needed for keyboard access, font scaling, system insets, and minimum targets.

## Automated verification

- `openspec validate --all`: 10 items passed.
- Desktop frozen install, five Vitest integration tests, TypeScript compilation, and Vite production build passed.
- Android JVM unit tests, debug APK assembly, and Compose instrumentation-test APK compilation passed.
- Rust formatting, warning-denied Clippy, and all workspace tests passed (263 tests across unit and integration targets).
- Desktop responsive rules cover 1420px-wide, compact-navigation, two-column, stacked, and single-column breakpoints; controls use visible `:focus-visible` styling and reduced-motion CSS.
- Android destinations use scrolling content, scaffold insets, Material minimum targets, explicit navigation/control semantics, non-color state text, and system-aware Compose animation.

## Manual verification still required

The available Android device `3B65BT02H6G00000` is reported as `offline` by ADB. Consequently, the Compose instrumentation suite, physical-device light/dark comparison, camera/permission/media end-to-end paths, and cross-device active-stream continuity remain pending. Desktop runtime startup was confirmed through `tauri dev`, but representative visual screenshots and the full paired-device scenario remain pending before archive and merge.
