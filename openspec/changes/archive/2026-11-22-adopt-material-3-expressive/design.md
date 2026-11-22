## Context

The Android UI lives in `ui/RedesignedScreens.kt` (the reachable screens) and `ui/Screens.kt` (an older card set whose composables are mostly unreachable but whose state holders `CameraControls`, `MicControls`, and `SpeakerControls` are still used). Both use baseline Material 3 widgets. The ViewModel exposes stable flows and `MainActivity` already maps them into immutable state-plus-callback holders, which is the integration boundary this change preserves.

## Goals / Non-Goals

**Goals:**

- Make Material 3 Expressive the real foundation: `MaterialExpressiveTheme`, `MotionScheme.expressive()`, expressive shapes, emphasized typography, and Expressive components.
- Use shape, motion, and progress style to communicate live state, in addition to color and text.
- End with one Android presentation system and no unreachable legacy composables.
- Keep functional parity and the existing navigation model.

**Non-Goals:**

- Dynamic (wallpaper) color. The mint/teal palette is the product identity and stays fixed.
- A bottom navigation bar, navigation rail, or adaptive two-pane layout. Phone stack navigation stays; wide-screen adaptation is a separate change.
- Any ViewModel, protocol, media, or service change.
- Rebuilding the HTML presentation board.

## Decisions

### 1. Depend on stable `material3` 1.5.0 and opt in to the Expressive API

Expressive components still carry `@ExperimentalMaterial3ExpressiveApi`. They ship in the stable 1.5.0 artifact reached through Compose BOM 2026.11.01, so no alpha dependency is needed. The opt-in is declared once through a Kotlin compiler argument for the module rather than annotating each file, so a later graduation of the APIs needs no source churn. Alternative considered: pinning an alpha for newer components; rejected because the stable artifact already contains what this design uses.

### 2. Keep the brand palette, change the system around it

`MaterialExpressiveTheme` takes the existing light and dark color schemes. Expressive contributes motion, shape, and type. Tertiary roles are filled from the existing amber `Degraded` tone so warning state has a first-class container color instead of ad hoc tints.

### 3. Navigation stays a stack, the app bar does more work

`LargeFlexibleTopAppBar` with an exit-until-collapsed scroll behavior replaces the fixed bar. Because the large bar lays the title out in both its expanded and collapsed form, UI tests address the title with `onAllNodesWithText`. Its title is the destination name; its subtitle is the live connection summary, so connection state is visible on every screen without a bottom bar. Back and Settings remain icon actions with the same labels and 48 dp targets, so the `UnifiedTopBar` accessibility contract and `pushDestination`/`popDestination` are unchanged.

### 4. State is expressed by shape and motion, with a non-motion fallback

- Connection hero: a cookie-shaped badge when connected, a `LoadingIndicator` while connecting or reconnecting, and a distinct shape with an error icon when failed.
- Stream activation uses `ToggleButton`, which morphs its shape between checked and unchecked and carries a text label, so state is not color-only.
- Audio levels use `LinearWavyProgressIndicator` whose amplitude follows the level and is flat when muted or silent.
- Animations read the ambient motion scheme and the system animation scale; with animations disabled the same state is shown by static shapes, labels, and icons.

### 5. Shared component library, screens as composition

Reusable components live in `ui/components/` (shape badge, stream card, metric tile, quality pill, level meter, notice, connected choice group). Each destination is its own file in `ui/` (`HomeDestination`, `CameraDestination`, `DevicesDestination`, `SettingsDestination`) and composes them, so `MainActivity` and tests keep the same imports. `Screens.kt` and `RedesignedScreens.kt` are removed; the three state holders move to `ui/StreamControls.kt`.

### 6. Icons

`material-icons-core` lacks most glyphs this design needs (videocam, mic, volume, wifi, computer, flip camera). `material-icons-extended` is added. Release minification is currently disabled in this project, so the APK grows; enabling R8 is noted as follow-up rather than done here.

### 7. Design reference

`design/Android.DESIGN.md` is rewritten as the normative description. `design/Android.html` is left in place and called out as pre-Expressive history inside the Markdown, because regenerating the board is a documentation exercise that does not affect the shipped app.

## Risks / Trade-offs

- Expressive APIs are marked experimental and may change signature before graduation; mitigated by confining usage to `ui/components` and the theme.
- `material-icons-extended` enlarges the unminified APK; accepted for now.
- Wavy progress and shape morphing add per-frame work on low-end devices; mitigated by animating only while a stream is active and by respecting the system animation scale.
- Material 3 1.5.0 comes with Compose 1.13, which requires `compileSdk` 37.1. It is raised in this change while `targetSdk` stays at 36, so no runtime behavior is opted into. Any build environment must have, or be able to download, the Android 37.1 platform; the CI Android job verifies this.
