## Why

The current Android and desktop interfaces expose the MVP functionality but do not yet match the product-specific reference designs in `design/`. Rebuilding the presentation and navigation now will give both clients a coherent, platform-appropriate experience without changing the established connection, media, or telemetry behavior.

## What Changes

- Redesign the desktop client as a compact, responsive streaming-console workspace with sidebar navigation, telemetry summaries, and dedicated workspace, network, and settings views based on the in-app content shown in `design/Desktop.DESIGN.md` and `design/Desktop.html`.
- Treat the desktop example's outer canvas, simulated title bar and traffic-light controls, fixed rounded window frame, border, and presentation shadow as mock-device documentation that MUST NOT be reproduced inside the Tauri webview.
- Redesign the Android client as a Material 3 companion application with a stack-based Devices → Home → Camera flow and Settings reachable from every screen, based on `design/Android.DESIGN.md` and `design/Android.html`.
- Introduce the specified dark and light color roles, typography, spacing, surfaces, controls, status indicators, and restrained state-driven motion on each platform.
- Replace the placeholder/small launcher artwork with a cohesive UnifiedStream app icon across desktop bundles and Android adaptive/legacy launchers, using platform-safe optical sizing.
- Use maintained platform icon sets for interface actions—Lucide React on desktop and Material Icons on Android—while reserving custom artwork for the UnifiedStream launcher identity.
- Preserve every existing connection, pairing, discovery, camera, microphone, speaker, synthetic-test-stream, error, and telemetry action and state while relocating them into the new information architecture.
- Add responsive and accessible behavior so layouts remain usable at narrower desktop widths and supported Android screen sizes, with visible focus, semantic labels, adequate touch targets, and non-color state cues.
- Add UI-focused automated tests and update existing tests where necessary to protect functional parity through the redesign.

## Capabilities

### New Capabilities

- `application-interface`: Defines the cross-platform visual system, navigation, responsive behavior, accessibility, and preservation of existing controls and live state across the redesigned Android and desktop interfaces.

### Modified Capabilities

None. Existing media, discovery, session-control, and telemetry requirements remain behaviorally unchanged and are presented through the new interface.

## Impact

- Desktop frontend: `desktop/src/App.tsx`, `desktop/src/App.css`, supporting React components, assets, fonts, and frontend tests.
- Android UI: `android/app/src/main/java/com/laffy/unifiedstream/ui/`, Compose theme files, launcher resources, navigation structure, and Compose/UI tests.
- Desktop packaging: Tauri icon sources and generated platform bundle icons under `desktop/src-tauri/icons/`.
- Existing Tauri commands/events and Android ViewModel/state flows remain the integration contracts; backend Rust, transport, protocol, capture, and playback behavior are not intentionally changed.
- Frontend and Android dependency manifests may gain narrowly scoped UI, icon, font, navigation, or test dependencies when the platform stack does not already provide them.
