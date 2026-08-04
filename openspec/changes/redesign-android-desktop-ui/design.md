## Context

UnifiedStream already has working React/Tauri and Jetpack Compose interfaces connected to stable backend contracts. The desktop UI is currently a single narrow page in `App.tsx`; the Android UI uses a small set of Compose screens and reusable stream cards. Both expose the MVP controls, but neither implements the complete visual language and information architecture documented by the paired Markdown and HTML references in `design/`.

The redesign crosses two UI stacks but must not disturb the Rust/Tauri command and event surface, Android `UnifiedStreamViewModel` state flows, stream lifecycles, permissions, foreground service ownership, or media pipeline. The HTML files are visual examples rather than runtime sources, while the Markdown files define the intended visual system and responsive principles.

## Goals / Non-Goals

**Goals:**

- Implement recognizable, platform-appropriate versions of the supplied Android and desktop designs.
- Preserve access to every existing action, state, diagnostic, and error path.
- Establish maintainable theme tokens and reusable components instead of duplicating presentation rules in monolithic screens.
- Make navigation and live-state ownership explicit so changing views never starts, stops, or resets a session or stream.
- Support dark and light presentation, responsive layout, keyboard/touch accessibility, and reduced-motion preferences.
- Add automated UI coverage for navigation, state rendering, and critical controls while retaining existing backend tests.

**Non-Goals:**

- Changing the wire protocol, discovery, pairing policy, media codecs, media quality, or reconnection behavior.
- Adding functionality that appears only as illustrative content in the static HTML examples but has no existing product contract or backend support.
- Replacing React/Tauri or Jetpack Compose, or sharing a lowest-common-denominator component library between platforms.
- Reworking camera encoding performance or the archived camera quality-autotune follow-ups.
- Reproducing presentation wrappers from the HTML examples inside the shipped applications, including simulated desktop title bars/window borders and Android phone frames or surrounding canvases.

## Decisions

### 1. Treat the design references as a hierarchy, not generated source

`Android.DESIGN.md` and `Desktop.DESIGN.md` define the normative colors, typography, component language, layout principles, and responsive intent. The corresponding HTML files define concrete composition and visual examples, but their presentation wrappers are not application content. On desktop, the outer 28-pixel canvas, fixed 1420-by-880 rounded frame, simulated title bar/traffic-light controls, surrounding border, and presentation shadow are mock-device documentation. On Android, the board heading, surrounding canvas, labels, and imported phone frames serve the same documentation-only role. Existing application behavior and OpenSpec capabilities remain authoritative when illustrative HTML content conflicts with real states or controls.

This avoids copying a static prototype that cannot represent live backend behavior. The alternative—embedding or mechanically translating the HTML—would bypass the existing React and Compose state architecture and create inaccessible, difficult-to-maintain markup.

### 2. Preserve existing state and command boundaries

The desktop root continues to subscribe once to Tauri events and invoke the existing commands. State is passed into presentational workspace, network, settings, telemetry, and media-control components. Android continues to collect `UnifiedStreamViewModel` flows at the application/navigation boundary and passes immutable state plus callbacks into screen components.

Navigation state is UI-local and independent from connection or stream state. Global pairing requests, connection failures, and other time-sensitive notices render above the active destination so a view change cannot hide or discard them.

Creating new backend commands or duplicating backend state in per-screen stores was rejected because it would expand scope and make functional parity harder to verify.

### 3. Build separate platform-native component systems from shared semantic roles

Desktop receives CSS custom properties for the documented dark/light roles, reusable glass panels, status lights, switches, telemetry values, audio meters, navigation rows, and responsive grids. Space Grotesk is the primary family and JetBrains Mono is used for technical values, with deterministic fallbacks when fonts cannot load.

Android receives Material 3 light/dark color schemes, typography roles, tonal containers, stream cards, metric readouts, status rows, switches, segmented choices, and a consistent title bar with Back and Settings actions. Roboto remains the platform primary family and JetBrains Mono is used selectively for measurements.

The platforms share status semantics—active/connected, secondary telemetry, warning, destructive/error—but do not share source components or force identical geometry. A cross-platform UI toolkit was rejected because it would add migration risk and weaken each reference design's platform character.

### 4. Use explicit information architecture

Desktop web content fills the complete Tauri webview and uses a sidebar with switchable main views. Native operating-system/Tauri window decoration remains outside the web UI; the app does not draw a second title bar, rounded outer window frame, or presentation canvas. The primary workspace contains connection context, a four-metric telemetry strip, and camera/microphone/speaker controls. Network holds advertising, device identity, ports, link details, test-stream diagnostics, and related actions. Settings holds presentation preferences and other existing settings-level controls. At narrow widths, the sidebar collapses behind a navigation control and content grids reduce columns without shrinking text below usable sizes.

Android launches into Devices, which owns discovery, manual address entry, and connection initiation. Starting a connection advances to Home, which summarizes the connected host and the three streams. “Open camera controls” pushes Camera, giving preview and capture settings suitable priority. Settings is pushed from the title action on every screen. There is no bottom navigation. Every screen uses a title bar with Back and Settings; Back pops the previous screen, while Back from the root Devices screen exits through the normal Android activity behavior. Navigation history and active stream/session state remain independent, and returning from Settings restores the preceding screen.

### 5. Preserve functional parity through a control inventory

Before replacing each current screen, implementation will enumerate all existing commands, callbacks, state variants, permission states, error messages, and telemetry fields. Every inventory item must map to a destination in the new structure. Controls shown in the static designs without backend support remain non-interactive explanatory presentation or are omitted; existing controls absent from the examples remain available in the most appropriate new view.

This inventory is also the basis for tests. It is preferred over visual-only acceptance because a redesign can look correct while silently dropping rejection, timeout, reconnect, manual-entry, or diagnostic paths.

### 6. Make motion progressive and state-driven

Animation is limited to state communication: connection pulse, discovery/recording indication, switch transitions, live audio meters, and subtle ambient changes. CSS `prefers-reduced-motion` disables nonessential desktop animation. Android respects system animator settings and avoids animation as the only carrier of state.

### 7. Test at component, integration, and build levels

Desktop tests cover destination changes, command invocation, event-driven state rendering, pairing availability, disconnected metric clearing, and responsive navigation behavior where practical. Android UI tests cover title-bar Back/Settings actions, the Devices → Home → Camera navigation flow, Settings return behavior, stream actions, discovery/manual-entry states, permission and error states, and semantic labels. Theme/token tests or focused snapshots may protect stable visual roles without relying on brittle full-page pixel snapshots.

Final verification runs all commands required by `CONTRIBUTING.md` and performs manual comparison against both light and dark reference examples at representative desktop and Android sizes.

## Risks / Trade-offs

- **[Static examples contain controls or values that are not backed by the product]** → Keep existing runtime contracts authoritative, document the control inventory, and do not fabricate backend behavior.
- **[Splitting monolithic UIs can accidentally reset subscriptions or state]** → Keep subscriptions and ViewModel collection at stable top-level owners; make destination components presentational.
- **[Visual fidelity conflicts with accessibility or small screens]** → Preserve semantic grouping, minimum targets, contrast, readable type, and responsive reflow even when exact prototype geometry must change.
- **[Remote font loading makes builds or offline use unreliable]** → Prefer bundled/package-provided font assets with explicit fallbacks; keep the application usable if the preferred family is unavailable.
- **[Animation causes distraction or excess rendering work]** → Animate only state indicators, throttle live meters as today, and honor reduced-motion/system animation settings.
- **[Large UI replacements are difficult to review]** → Implement shared foundations first, then migrate one destination/control group at a time with parity tests before deleting old presentation code.

## Migration Plan

1. Record the desktop and Android functional control/state inventory and add regression tests around the current backend integration boundaries.
2. Add platform theme tokens, typography, icons/assets, and reusable primitives without changing current behavior.
3. Introduce the new navigation shells and migrate live status/pairing handling to stable top-level owners.
4. Migrate desktop views and Android destinations incrementally, verifying each mapped action and state before removing superseded UI code.
5. Run automated builds/tests and manual dark/light, responsive, keyboard, screen-reader/semantic, permission, disconnect, and active-stream checks.
6. Sync the new capability to main specs and archive this change in the same implementation pull request after all required checks pass.

Rollback is a normal Git revert of the redesign commits on the feature branch or pull request. No data, protocol, or backend migration is involved.

## Open Questions

- Whether Space Grotesk and JetBrains Mono should be bundled as repository assets or sourced through existing platform/font packages will be resolved during dependency review; offline availability and license compatibility are required either way.
- Exact desktop breakpoint values may be adjusted during implementation to match the real Tauri minimum window size, while preserving the specified wide, two-column, stacked, and navigation-drawer behaviors.
