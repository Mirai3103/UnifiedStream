## 1. Foundation

- [x] 1.1 Confirm work is on a non-`main` branch and record the baseline `testDebugUnitTest assembleDebug` result (passed before any change).
- [x] 1.2 Move the Compose BOM to the release that ships `material3` 1.5.0 (2026.11.01), add `material-icons-extended`, declare the Expressive opt-in for the module, and raise `compileSdk` to 37.1, which Compose 1.13 requires (`targetSdk` unchanged).
- [x] 1.3 Replace `UnifiedStreamTheme` internals with `MaterialExpressiveTheme`, `MotionScheme.expressive()`, the expressive shape scale, emphasized typography, and a tertiary warning role, keeping the existing light and dark palettes.

## 2. Component library

- [x] 2.1 Move `CameraControls`, `MicControls`, and `SpeakerControls` to `ui/StreamControls.kt` and delete the unreachable legacy composables in `Screens.kt`.
- [x] 2.2 Build shared components: connection hero, stream card with toggle button, metric tile, wavy level meter, notice, section header, and connected button-group helpers, each with semantic labels, roles, and state descriptions.

## 3. Screens

- [x] 3.1 Replace the fixed top bar with `LargeFlexibleTopAppBar` (Back, Settings, live connection subtitle, collapse on scroll) while keeping the `UnifiedTopBar` accessibility contract.
- [x] 3.2 Rebuild Home: connection hero, network metrics, and camera, microphone, and speaker cards with all existing states, permission, refusal, and error messages.
- [x] 3.3 Rebuild Camera: expressive preview surface, recording and resolution overlays, resolution button group, facing control, and start/stop action.
- [x] 3.4 Rebuild Devices: pill search, discovery state with loading indicator, segmented device list including the incompatible-protocol state, and the connect-by-address form with validation errors.
- [x] 3.5 Rebuild Settings: theme switch, phone identity, session actions, and transport diagnostics with report tiles.

## 4. Documentation, tests, and delivery

- [x] 4.1 Rewrite `design/Android.DESIGN.md` for the Expressive system and mark `design/Android.html` as historical.
- [x] 4.2 Update `RedesignedScreensTest` and unit tests for changed labels, keeping coverage of navigation, stream toggles, and error states.
- [x] 4.3 Verify reduced-motion behavior, light and dark themes, and enlarged text on an emulator (API 37, font scale 2.0, animations off).
- [x] 4.4 Run `openspec validate --all` and `(cd android && ./gradlew testDebugUnitTest assembleDebug)`.
- [x] 4.5 Sync the delta spec, archive the change in this pull request, and open the pull request against `main`.
