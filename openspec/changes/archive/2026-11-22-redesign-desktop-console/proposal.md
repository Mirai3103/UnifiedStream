## Why

The desktop client's current interface is a dark glass-panel dashboard: a grid of equally weighted cards, neon teal and violet accents, and a single 400-line component holding every view. It does not reflect what the product is — a live signal path from a phone into a PC's camera, microphone, and speaker — and it buries the one thing a user wants to know first, whether the phone is connected and healthy. This change rebuilds the desktop interface from the ground up around that signal path, without touching the backend contract.

## What Changes

- Replace the glass-panel workspace with a **signal-path console**: a link header that shows this PC, the phone, the connection state and link quality in one band, with the four telemetry figures inline, above three channel strips (camera, microphone, speaker) that each carry their own state, toggle, live meter and failure text.
- Add a purposeful **not-connected state** to the workspace that tells the user how to connect and offers to start advertising, instead of showing a row of dashes.
- Replace the neon teal/violet palette and Space Grotesk/JetBrains Mono with a warm-neutral palette, one cobalt accent, semantic live/warning/danger colors, and Geist Sans/Geist Mono. Light and dark are designed together.
- Add a **System** appearance option (the default) beside Light and Dark, remembered locally.
- Present audio levels as accessible `meter` elements with a peak marker, and switches as `switch` controls.
- Require confirmation before "Forget trusted devices", which is destructive and was previously one click.
- Make pairing a focus-managed modal dialog that can be dismissed with Escape (deny).
- Split the single `App.tsx` into a stable shell that owns backend state (a `useBackend` hook) and separate view and component modules.
- Resize the default window to fit the new layout. No backend command, event, or payload changes.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-interface`: the desktop presentation requirement is replaced by the signal-path console, and requirements are added for the desktop workspace structure, the appearance preference, and confirmation of destructive actions.

## Impact

- `desktop/src/**`: rewritten (`App.tsx`, new `state/`, `components/`, `views/`, `styles/`; `App.test.tsx` rewritten). `types.ts` is unchanged.
- `desktop/package.json`, `desktop/bun.lock`: Geist font packages replace Space Grotesk and JetBrains Mono.
- `desktop/src-tauri/tauri.conf.json`: default and minimum window size only.
- No change to Rust crates, the Tauri command/event surface, the protocol, or Android.
- Existing requirements for feature parity, platform-neutral text, the VB-CABLE attribution, and reduced motion continue to apply and are covered by the rewritten tests.
