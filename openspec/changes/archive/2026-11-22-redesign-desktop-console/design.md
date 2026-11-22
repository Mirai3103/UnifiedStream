## Context

The desktop client is a React 19 + Tauri 2 webview. All state arrives through one `get_status` snapshot plus ten Tauri events, and all actions are Tauri commands. That contract is correct and stays. The presentation layer is what changes: one `App.tsx` owns subscriptions, state, and every view's markup, styled by a single stylesheet of glass panels.

## Goals / Non-Goals

**Goals**
- A layout that answers "is my phone connected, and is it healthy?" before anything else, then gives each media channel equal, self-contained controls.
- Light and dark designed together from semantic tokens, with no raw colors in components.
- State owned by a stable shell so navigation never touches streams, with view code that can be read and tested on its own.
- Full feature parity, platform-neutral text, and the VB-CABLE attribution preserved.

**Non-Goals**
- No backend, protocol, or Android change. No new commands or events.
- No camera preview image: the backend does not deliver frames to the webview, and a fabricated preview is excluded by the parity requirement.
- No custom window chrome; the native title bar stays.
- No new UI framework or CSS toolchain.

## Decisions

**Signal path instead of a card grid.** The product is a chain (phone → network → PC devices). A link header followed by three channel strips mirrors that chain and gives hierarchy through position and size rather than through glow and elevation. Telemetry becomes one hairline-divided row of tabular figures in the header, not four decorative cards, because the numbers describe the link, not four separate things.

**Semantic tokens.** `tokens.css` defines semantic tokens (`--bg`, `--surface`, `--surface-raised`, `--line`, `--ink`, `--ink-muted`, `--accent`, `--live`, `--warn`, `--danger`, plus on-colors) once for light and once for dark, along with a spacing, radius, and type scale. Components reference only these. Dark uses lighter, desaturated tonal accents rather than the light values; text/background pairs are chosen to meet 4.5:1 and meaningful non-text pairs 3:1.

**Palette and type.** Warm neutral surfaces, one cobalt accent for interaction and focus, and green/amber/red reserved for state, each always paired with an icon or text. Geist Sans and Geist Mono are bundled with `@fontsource` (the app is offline); Mono with tabular figures is used for every changing number so the layout does not shift.

**Shell owns state; views are pure.** A `useBackend` hook holds the snapshot, the connection state, every stream state, and the event subscriptions, and returns state plus action callbacks. `App` composes the hook with the navigation and renders a view; views and components receive props and own no subscriptions. This keeps the existing guarantee that navigation neither invokes commands nor disturbs streams, and lets components be tested without mocking Tauri.

**Pending flags with cancellable timers.** The camera and microphone enable calls are acknowledged by a later status event, so a "starting" flag is set on click and cleared by that event or by a six-second fallback. The fallback timers live in refs that are cancelled on unmount and on the acknowledging event, instead of firing unconditionally.

**Appearance.** A `useAppearance` hook stores `system | light | dark` in `localStorage` inside try/catch, resolves `system` from `matchMedia('(prefers-color-scheme: dark)')`, and writes the resolved value to `document.documentElement.dataset.theme`. Failure to read or write storage degrades to System.

**Meters and switches.** Level meters are `role="meter"` with `aria-valuenow` and a text value, drawn as a bar with a peak marker. Enable controls are native checkboxes with `role="switch"`, which keeps keyboard and form semantics.

**Modal and destructive flows.** Pairing is an `aria-modal` dialog that moves focus to Allow on open, cycles Tab within itself, treats Escape as Deny, and restores focus on close. "Forget trusted devices" becomes an inline two-step control (Forget, then Confirm or Cancel) rather than a second modal, so it stays next to the action it guards.

**Responsive behavior.** At wide widths the sidebar is 232px with labels. Below 900px it collapses to a 64px icon rail with accessible names; below 640px it becomes a top tab bar. The channel grid is two columns (camera spanning two rows) when wide and one column otherwise. Viewport media queries are used rather than container queries, to stay within what the oldest supported webview engine handles.

**Motion.** The only animated elements are the link line (a travelling dash while connected) and meter fill transitions. Both are disabled under `prefers-reduced-motion`, where the same state remains readable from text and shape.

**Window.** Default 1080×720, minimum 480×560, so the wide layout is the first thing a user sees.

## Risks / Trade-offs

- **A full rewrite risks silently dropping a behavior.** Mitigated by rewriting the test suite against the same commands and events, preserving the intent of the existing tests (routing control omitted when `null`, setup-hint rendering, attribution on every platform, no platform names in fallback text) and adding tests for the new behavior.
- **Fonts add package weight.** Geist replaces two existing font packages, so the net is neutral; only the weights in use are imported.
- **`role="switch"` changes the accessible role tests and assistive technology see.** Intentional and more accurate; tests are updated.
- **No preview image makes the camera strip sparse.** It states the negotiated format, device, delivered rate, and counters honestly instead of imitating a preview.
