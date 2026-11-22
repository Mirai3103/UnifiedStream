## Why

The Android client is described as "expressive Material 3" but is built on the baseline Material 3 component set: switches, outlined buttons, and flat tonal cards with fixed corner radii. Material 3 Expressive (stable in `androidx.compose.material3` 1.5.0) provides the spring-based motion scheme, shape morphing, emphasized typography, wavy progress, loading indicators, connected button groups, toggle buttons, and flexible app bars that suit a live-streaming control surface. Adopting it as a real platform foundation, rather than imitating it with hand-tuned radii, lets the client feel native to current Android and lets state changes (connecting, live, muted, failed) read through shape and motion as well as color.

## What Changes

- Move the Android build to the Compose BOM that ships `material3` 1.5.0 and wrap the app in `MaterialExpressiveTheme` with the expressive motion scheme, expressive shape scale, and emphasized type roles, keeping the existing UnifiedStream mint/teal palette and the JetBrains Mono role for measurements.
- Rebuild the Android presentation layer on Expressive components: a collapsing large flexible top app bar that carries Back, Settings, and a live connection subtitle; a shape-morphing connection hero; toggle buttons for stream activation; connected button groups for resolution and mute; wavy level meters and loading indicators for live and pending state; segmented list items for devices and settings; pill search and manual-entry fields.
- Replace the legacy `Screens.kt` card set, whose composables are no longer reachable, with the new shared component library so only one Android presentation system remains.
- Update `design/Android.DESIGN.md` to describe the Expressive system. The existing `design/Android.html` presentation board predates this change and is kept only as a historical reference.
- Keep every existing action, state, permission, failure, and diagnostic, and keep the stack navigation (Devices → Home → Camera, Settings from every screen) without a bottom navigation bar.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-interface`: The Android presentation requirement now names Material 3 Expressive instead of baseline Material 3 tonal surfaces, and the motion requirement permits spring-based shape and motion changes tied to live state while still honoring reduced-motion settings.

## Impact

- Android build: `android/gradle/libs.versions.toml` (Compose BOM, icon library) and `android/app/build.gradle.kts` (Expressive opt-in, and `compileSdk` if the new libraries require it).
- Android UI: `android/app/src/main/java/com/laffy/unifiedstream/ui/**`, `MainActivity.kt`, theme files, and Compose tests under `android/app/src/androidTest/`.
- Docs: `design/Android.DESIGN.md`.
- No change to the protocol, discovery, session control, media pipeline, ViewModel flows, permissions, or the foreground service. No desktop change.
