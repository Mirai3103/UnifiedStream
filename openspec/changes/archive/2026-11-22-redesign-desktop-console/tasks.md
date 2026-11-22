## 1. Foundations

- [x] 1.1 Replace the Space Grotesk and JetBrains Mono packages with Geist Sans and Geist Mono; update `bun.lock`
- [x] 1.2 Add `styles/tokens.css` (light and dark semantic tokens, spacing, radius, type scale) and `styles/base.css` (reset, focus ring, reduced motion)
- [x] 1.3 Add `state/useBackend.ts` owning the snapshot, all stream state, event subscriptions, and action callbacks, with cancellable pending timers
- [x] 1.4 Add `state/useAppearance.ts` with System/Light/Dark, guarded local storage, and system resolution

## 2. Components

- [x] 2.1 Build the sidebar / icon rail / tab bar navigation, the this-device card, the global notice, and the pairing dialog
- [x] 2.2 Build the link header (state, quality with icon, four telemetry figures) and the not-connected guidance
- [x] 2.3 Build the shared channel strip, level meter (`role="meter"`), and switch (`role="switch"`) components
- [x] 2.4 Build the camera, microphone, and speaker strips with errors, setup hint and copy action, and the routing control gated on the reported capability

## 3. Views

- [x] 3.1 Workspace view
- [x] 3.2 Network view: discovery/advertising, session details, disconnect, confirmed "forget trusted devices", synthetic transport test
- [x] 3.3 Settings view: appearance, about, and the third-party component attribution rendered on every platform
- [x] 3.4 Responsive layout: sidebar, icon rail, top tabs; one- and two-column channel grid

## 4. Verification

- [x] 4.1 Rewrite `App.test.tsx` covering navigation, theme and persistence, pairing, telemetry clearing, every media command, routing capability, setup hints, attribution, platform-neutral text, meters, and forget confirmation
- [x] 4.2 Update the default and minimum window size in `tauri.conf.json`
- [x] 4.3 Update documentation that names old UI labels
- [x] 4.4 Run `bun run build` and `bun run test`; inspect the running app in light and dark at wide, rail, and tab widths
- [x] 4.5 Run `openspec validate --all`; sync the delta spec and archive the change in this pull request
