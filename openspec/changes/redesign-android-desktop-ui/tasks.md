## 1. Baseline and Functional Inventory

- [x] 1.1 Confirm implementation remains on a non-`main` feature branch and record the current desktop commands/events, Android ViewModel callbacks/flows, permission states, connection states, stream states, errors, and diagnostics that the redesigned UI must preserve.
- [x] 1.2 Map every inventory item to its new desktop view and Android destination, resolving any illustrative control in the HTML references that has no existing backend contract without inventing new behavior.
- [x] 1.3 Add or update focused regression tests around the existing desktop event/command bindings and Android UI state/callback models before replacing their presentation.
- [x] 1.4 Decide and document the offline-safe, license-compatible source and fallback strategy for Space Grotesk and JetBrains Mono, adding only the required font/assets or dependencies.

## 2. Desktop Design Foundation

- [x] 2.1 Refactor the desktop frontend into a stable application shell plus reusable navigation, status, telemetry, media-control, form, dialog/banner, and audio-meter components while keeping Tauri subscriptions and command invocation centralized.
- [x] 2.2 Implement semantic desktop light/dark tokens, Space Grotesk and JetBrains Mono typography roles, glass surfaces, borders, status colors, control states, spacing, focus styles, and reduced-motion behavior from `design/Desktop.DESIGN.md`.
- [x] 2.3 Make desktop web content fill the Tauri webview without simulated title-bar or outer-window mock decoration, and build sidebar navigation with explicit selected state, keyboard operation, device/backend context, theme control, and compact/drawer behavior at narrow widths.
- [x] 2.4 Implement responsive desktop layout primitives for the wide four-column telemetry strip, primary media workspace, two-column secondary views, stacked narrow layouts, and camera-preview reflow without reducing required control sizes.

## 3. Desktop Views and Behavior Parity

- [x] 3.1 Build the primary workspace with connection context, live latency/throughput/loss/jitter/quality telemetry, camera status and delivered FPS, microphone controls/level, and speaker controls/level using live existing state.
- [x] 3.2 Build the network view with advertising control, device identity, control/media ports, connection details, disconnect/reconnect-related state, and all existing synthetic test-stream configuration, actions, and reports.
- [x] 3.3 Build the settings view for theme and appropriate existing settings-level controls, keeping unsupported prototype-only items absent or clearly non-interactive.
- [x] 3.4 Make pairing requests, global failures, unavailable backend guidance, pending/refused states, and recovery actions visible and actionable from every desktop destination.
- [x] 3.5 Verify desktop navigation and theme changes do not recreate subscriptions, clear live data, disconnect a session, or alter active camera, microphone, or speaker streams.

## 4. Android Design Foundation

- [x] 4.1 Implement the complete Material 3 light/dark color schemes, Roboto and JetBrains Mono typography roles, tonal elevation, shapes, spacing, system-bar treatment, status colors, and state-driven motion from `design/Android.DESIGN.md`.
- [x] 4.2 Extract reusable Compose components for app bars, device/session summaries, stream cards, metric values, level meters, switches, segmented choices, notices, search/manual-entry fields, and loading/error/permission states with accessibility semantics.
- [x] 4.3 Add a stable Home, Camera, Devices, and Settings navigation shell with bottom navigation, correct selected state, system inset handling, preserved scroll/destination state, and top-level collection of existing ViewModel flows.
- [x] 4.4 Ensure Android screens scroll and reflow for supported small screens, orientation/inset changes, and enlarged text without clipped actions, obscured final content, distorted preview, or undersized touch targets.

## 5. Android Destinations and Behavior Parity

- [x] 5.1 Build Home with the connected-host summary, camera/microphone/speaker status cards, live compact metrics, primary connection/stream action, and clear pending, inactive, refused, permission-required, and failed states.
- [x] 5.2 Build Camera with the live PreviewView, recording/active status, start/stop action, facing switch, resolution selection, permission handling, and stream failure/refusal feedback while preserving the existing capture callbacks.
- [x] 5.3 Build Devices with Wi-Fi/no-network/discovery states, searchable discovered-device results, connection initiation, timeout handling, manual IP/port entry and validation, and connection progress/failure feedback.
- [x] 5.4 Build Settings with theme and appropriate existing preferences/session/device information, omitting or clearly disabling illustrative settings that lack a real product contract.
- [x] 5.5 Verify destination and theme changes do not restart discovery unnecessarily, detach an active session service, reset ViewModel state, or start/stop/renegotiate active media streams.

## 6. Automated UI Verification

- [x] 6.1 Add desktop tests for navigation, theme switching, keyboard focus, pairing availability, all existing command actions, event-driven stream/error rendering, live telemetry updates, and metric clearing on disconnect.
- [x] 6.2 Add Android Compose/UI tests for all four destinations, bottom-navigation semantics, discovery/manual-entry states, camera controls, microphone and speaker controls, permission/refusal/error states, and connection-state continuity across navigation.
- [x] 6.3 Add focused responsive checks for wide and narrow desktop layouts and Android checks with enlarged text, ensuring all controls remain reachable and content is not hidden.
- [x] 6.4 Verify accessible names, roles, selected/checked/enabled states, visible desktop focus, non-color status cues, minimum targets, and reduced-motion behavior on both clients.

## 7. Visual and End-to-End Verification

- [ ] 7.1 Compare desktop dark and light modes at representative wide and narrow sizes against only the in-app content of `design/Desktop.DESIGN.md` and `design/Desktop.html`, confirming the mock canvas, simulated title bar, traffic-light controls, and outer window frame are absent and recording intentional deviations required for real behavior or accessibility.
- [ ] 7.2 Compare Android Home, Camera, Devices, and Settings in dark and light modes on a physical or representative emulated device against `design/Android.DESIGN.md` and `design/Android.html`.
- [ ] 7.3 Manually exercise discovery, manual connection, pairing accept/reject/timeout, reconnect, disconnect, camera preview/control, microphone controls, speaker controls, telemetry, test stream, permission denial, and backend-unavailable guidance through the redesigned interfaces.
- [ ] 7.4 Confirm active streams continue unchanged while navigating and switching theme, global prompts remain actionable, and stale session/stream metrics clear when their owner stops.

## 8. Validation and OpenSpec Lifecycle

- [x] 8.1 Run `openspec validate --all`, Rust formatting/Clippy/workspace tests, Android debug unit tests and APK assembly, and the frozen-lockfile frontend production build required by `CONTRIBUTING.md`.
- [ ] 8.2 Reconcile the implementation and verification results with the `application-interface` delta spec, then sync the delta to main specs and archive this completed change on the same feature branch.
- [x] 8.3 Push the feature branch and open or update its pull request targeting `main`, documenting visual comparisons, manual verification, dependencies, intentional reference deviations, and every local check run.
- [ ] 8.4 Confirm the required `openspec`, `rust`, `android`, and `frontend` pull-request checks pass before merge; after merge, monitor the `main` workflow and confirm the same required jobs pass.
