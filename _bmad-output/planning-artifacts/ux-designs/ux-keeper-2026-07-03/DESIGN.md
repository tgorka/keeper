---
name: keeper
description: Open-source, client-only universal Matrix messenger for macOS and iPhone. shadcn/ui on Tauri 2 + React 19 + Tailwind v4; this DESIGN.md specifies the brand-layer delta and the platform envelopes only. The iOS phase adds layout tokens, not a second visual language.
status: final
created: 2026-07-03
updated: 2026-07-16
colors:
  # Brand overrides on top of shadcn defaults. All unlisted tokens inherit from
  # shadcn (background, foreground, muted, muted-foreground, popover, popover-foreground,
  # card, card-foreground, border, input, ring, destructive, secondary, sidebar-*).
  # [ASSUMPTION] Palette authored without brand input; hues need owner sign-off, roles are stable.
  primary: '#0F6E5C'                 # keeper green — trust, permanence, "kept"
  primary-foreground: '#FFFFFF'
  primary-dark: '#3ECFAE'
  primary-foreground-dark: '#06231C'
  accent: '#B45309'                  # held amber — "written but not sent"
  accent-foreground: '#FFFFFF'
  accent-dark: '#F5A623'
  accent-foreground-dark: '#231303'
  incognito: '#6D28D9'               # incognito violet — outbound signals suppressed
  incognito-foreground: '#FFFFFF'
  incognito-dark: '#A78BFA'
  incognito-foreground-dark: '#1E1038'
  bridge-healthy: '#16A34A'
  bridge-degraded: '#D97706'
  bridge-disconnected: '#DC2626'     # shares hue with shadcn destructive by design
  search-highlight: '#FDE68A'        # FTS match highlight (light)
  search-highlight-dark: '#78560A'
  # Screen Recording (Phase 3, PRD §14). Live-capture red — deliberately NOT the
  # destructive/bridge-disconnected red: warmer and brighter, and only ever worn by
  # the record dot and the active-recording pill/banner, never by a button or body text.
  recording: '#E5322D'               # active capture — "recording is live"
  recording-foreground: '#FFFFFF'
  recording-dark: '#FF6259'
  recording-foreground-dark: '#2A0605'
typography:
  # macOS system stack everywhere; the platform owns rendered metrics.
  body:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif'
    fontSize: 13px
    lineHeight: '1.45'
  title:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Display", sans-serif'
    fontSize: 15px
    fontWeight: '600'
  section-label:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif'
    fontSize: 11px
    fontWeight: '600'
    letterSpacing: 0.06em
  caption:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif'
    fontSize: 11px
    lineHeight: '1.35'
  mono:
    fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace'
    fontSize: 12px
rounded:
  sm: 5px       # inputs, badges, kbd chips
  md: 7px       # buttons, chat rows, cards
  lg: 10px      # dialogs, popovers, command palette
  xl: 14px      # message bubbles (outgoing/incoming)
  full: 9999px  # pins, avatars, status dots, countdown pill
spacing:
  # Tailwind v4 4px scale inherited; named layout tokens below.
  traffic-light-inset-x: 78px    # width reserved left of sidebar header for macOS window controls
  traffic-light-inset-y: 12px
  sidebar-width: 260px
  chat-list-width: 320px
  detail-panel-width: 320px
  conversation-min-width: 480px
  content-max-width: 720px       # timeline text measure inside the conversation pane
  phone-breakpoint: 768px        # below this width: single-pane stack tier (PRD FR-58)
  touch-target-min: 44px         # HIG minimum for every tappable on the phone tier (FR-60)
  safe-area: 'env(safe-area-inset-*) exposed as --safe-top / --safe-right / --safe-bottom / --safe-left (viewport-fit=cover)'
  kb-inset: '--kb-inset — visualViewport-driven on-screen-keyboard height; 0 when the keyboard is closed'
components:
  chat-row:
    height: 64px
    radius: '{rounded.md}'
    active-background: 'shadcn sidebar-accent'
    unread-title-weight: '600'
  unread-badge:
    background: '{colors.primary}'
    foreground: '{colors.primary-foreground}'
    radius: '{rounded.full}'
    mention-variant: 'filled; non-mention variant is a neutral dot'
  network-badge:
    size: 16px
    placement: 'bottom-right overlay on chat Avatar, 2px background ring'
  account-marker:
    style: '3px left edge bar on chat row + account initial chip in chat header'
    colors: 'per-account hue assigned at add-time from an 8-hue wheel'
  message-bubble-outgoing:
    background: '{colors.primary}'
    foreground: '{colors.primary-foreground}'
    radius: '{rounded.xl}'
  message-bubble-incoming:
    background: 'shadcn muted'
    foreground: 'shadcn foreground'
    radius: '{rounded.xl}'
  undo-send-pill:
    background: '{colors.accent}'
    foreground: '{colors.accent-foreground}'
    radius: '{rounded.full}'
    content: 'radial countdown + "Sending in Ns — Undo"'
  draft-marker:
    color: '{colors.accent}'
    style: 'pencil glyph + "Draft" prefix in chat-row preview line'
  incognito-chip:
    background: 'transparent'
    foreground: '{colors.incognito}'
    border: '1px solid {colors.incognito}'
    radius: '{rounded.full}'
  bridge-health-dot:
    size: 8px
    healthy: '{colors.bridge-healthy}'
    degraded: '{colors.bridge-degraded}'
    disconnected: '{colors.bridge-disconnected}'
    disconnected-behavior: 'pulses twice on state change, then steady — persistent, never auto-clears'
  risk-tier-badge:
    low: 'shadcn secondary badge, label "Low risk"'
    maintenance: 'outline badge tinted {colors.bridge-degraded}, label "Maintenance-heavy"'
    volatile: 'filled badge {colors.bridge-disconnected}, label "Volatile — opt-in"'
    conditional: 'outline badge, label "Advanced"'
  command-palette:
    width: 640px
    radius: '{rounded.lg}'
    result-active-background: 'shadcn accent'
    kbd-chip: '{typography.mono} on shadcn muted, radius {rounded.sm}'
  swipe-action:
    # Phone tier only. Revealed behind chat rows / Approval rows; full row height,
    # glyph first, label appears past the half-swipe commit threshold.
    archive: '{colors.primary} background, {colors.primary-foreground} glyph'   # archived = kept
    read-toggle: 'shadcn secondary background, foreground glyph'
    mute: 'shadcn muted background, muted-foreground glyph'
    discard: 'shadcn destructive background and foreground'
  phone-header:
    height: 52px
    back-affordance: 'chevron + previous-level title, {spacing.touch-target-min} hit area'
    background: 'shadcn background, 1px bottom border — same flat-pane language as desktop'
  # Screen Recording (Phase 3, PRD §14) — desktop-only, renders only when the
  # `recording` capability flag is on (FR-66). Glyphs are part of the recording glyph set.
  recording-dot:
    size: 10px
    fill: '{colors.recording}'
    radius: '{rounded.full}'
    motion: 'steady when recording; reduced-motion never pulses. The single sanctioned use of recording-red.'
  recording-glyphs:
    set: 'record-dot, display (monitor), app-window, camera (webcam on/off), microphone (on/off/muted), system-audio (speaker waves), stop (filled square), reveal-in-finder'
    style: 'stroke glyphs matching the system SF Symbols weight; camera/mic carry a slashed "off" variant'
  tray-recording:
    idle: 'existing app-icon template (Story 10.3 unchanged)'
    recording: 'template icon with a {colors.recording} record-dot badge, bottom-right'
    warning-error: 'template icon with an amber (warning) / {colors.recording} (error) badge; error badge is filled, warning is outline'
    menu-elapsed-line: '{typography.mono} disabled menu item, ~1 Hz text update'
  source-picker-row:
    height: 44px
    layout: 'leading glyph (display/app-window) + name + optional app icon; single-select radio semantics'
    radius: '{rounded.md}'
    active-background: 'shadcn accent'
  device-picker:
    control: 'shadcn Select; leading device glyph (camera/microphone), "System default" always first for mic'
    disabled-state: 'greyed with helper caption when the source toggle is off'
  permission-row:
    layout: 'permission name + live status pill + right-aligned action (Request / Open System Settings)'
    granted: '{colors.bridge-healthy} check glyph + "Granted"'
    not-requested: 'shadcn muted dot + "Not requested yet"'
    denied: '{colors.bridge-disconnected} glyph + "Denied" + "Open System Settings" action'
    note-line: '{typography.caption} for macOS quirks (relaunch-after-grant, monthly re-confirm, ad-hoc-dev-build caveat)'
  active-recording-banner:
    placement: 'top of the Recording view; persistent while capture is live or faulted'
    recording: '{colors.recording} 3px left edge, {recording-dot} + "Recording", {typography.mono} elapsed · segment · size, Stop button (destructive-outline)'
    warning: 'amber left edge + persistent warning line (mic unplugged / low disk); never auto-clears'
    error: '{colors.recording} filled banner + reason + "Restart recording" — loud, not a toast (FR-75)'
    radius: '{rounded.md}'
  segment-meter:
    style: 'shadcn Progress bar toward the configured segment size; {typography.caption} "segment N · 412 / 500 MB"; resets each rotation'
---

## Brand & Style

keeper is the messenger that keeps your messages: an open-source, client-only Matrix client that makes a user-owned homeserver + bridges stack feel like a polished product. The brand posture is **archival calm** — the visual language of a well-made native macOS utility, not a consumer social app. Nothing bounces, nothing gamifies, nothing pleads. The interface recedes so conversations and the user's own judgment stay in front.

Three brand ideas carry the whole visual layer:

1. **Kept** — keeper green (`{colors.primary}`) marks what the product promises: delivery confirmed, archive intact, bridge healthy-adjacent actions. It is the color of "this is safe now."
2. **Held** — held amber (`{colors.accent}`) marks the airlock between writing and sending: drafts, the Approval Pane, the Undo-Send countdown. Amber never decorates; when the user sees amber, something they wrote has *not* gone out yet.
3. **Honest** — state is never hidden or softened. Bridge health, risk tiers, unofficial-API labels, and "best effort" caveats are first-class visual citizens with their own tokens, rendered in plain badges and persistent indicators rather than buried toasts.

keeper inherits shadcn/ui defaults wholesale (Tailwind v4 CSS-variable theming in `src/index.css`). This DESIGN.md specifies only the brand-layer delta: the palette above, the macOS system type stack, the three-pane layout tokens, and keeper-specific component treatments. Components that ship from shadcn unmodified are the contract — restyling them is against brand discipline. The app must feel at home next to Mail and Finder: system font, native window controls, restrained chrome, real dark mode.

## Colors

- **keeper green (`{colors.primary}` light / `{colors.primary-dark}` dark)** — the brand color and shadcn `primary` override. Used on: primary buttons, outgoing message bubbles, unread mention badges, sent/delivered confirmation ticks, active "connected" states in bridge flows, the app icon field. It reads as trust and permanence, and deliberately avoids Beeper's blue/purple family.
- **Held amber (`{colors.accent}` light / `{colors.accent-dark}` dark)** — the airlock color, overriding shadcn `accent` only where "pending human intent" is meant. Used on: the Undo-Send countdown pill, Draft markers in chat rows, the Approval Pane badge count, queued-offline message state. Never used for hover states, chrome, or emphasis — shadcn's neutral `accent` handles list hover/selection. Amber means exactly one thing: *written, not sent*.
- **Incognito violet (`{colors.incognito}` / `{colors.incognito-dark}`)** — outbound-signal suppression. Used on the incognito chip in chat headers, the composer's incognito border tint, and the scope indicator in settings. Only violet may signal incognito, and violet signals nothing else.
- **Bridge health trio (`{colors.bridge-healthy}` / `{colors.bridge-degraded}` / `{colors.bridge-disconnected}`)** — semantic status only: bridge session dots, network row states, health banners. Disconnected shares the red family with shadcn `destructive` on purpose — a dead bridge is data loss in progress.
- **Search highlight (`{colors.search-highlight}` / dark variant)** — FTS match emphasis in search results and in-timeline jump targets. Background tint behind matched terms; never borders, never text color.
- **Recording red (`{colors.recording}` / `{colors.recording-dark}`, Phase 3)** — the live-capture color, and *not* a fifth brand hue on the chrome: it appears only as the `{components.recording-dot}` and on the `{components.active-recording-banner}` / tray record badge while capture is running. It is deliberately warmer and brighter than `destructive`/`bridge-disconnected` so a live indicator never reads as a delete button, and the two never share a surface — recording red says "on," destructive red says "gone." When capture is faulted, the same red carries the loud error banner (FR-75). Never on buttons, text, hovers, or decoration; if it isn't the record state, it isn't recording red.
- **Everything else inherits shadcn** — `background`, `foreground`, `muted`, `border`, `ring`, `card`, `popover`, `destructive`, and the `sidebar-*` family stay stock. If a color can't be justified by one of the three brand ideas, it isn't overridden.

Avoid: gradients, per-network theming of the app chrome (network identity lives in badges, not wallpaper), red for anything but destruction/disconnection or live capture, more than these four brand hues plus the semantic status set (bridge trio, recording red).

## Typography

The macOS system stack is the only UI family — `{typography.body.fontFamily}` — so keeper renders with SF Pro on macOS and falls back cleanly in dev. The platform owns the rendered result; sizes are authored for macOS desktop density:

- **`body` (13px)** — timeline text, chat previews, settings copy. The default voice.
- **`title` (15px/600)** — chat header names, dialog titles, wizard step headings.
- **`section-label` (11px/600, +0.06em, uppercase)** — sidebar group labels (SPACES, FAVORITES), settings section headers, Approval Pane group headers.
- **`caption` (11px)** — timestamps, per-message state text ("Edited", "Sending…", "Failed — Retry"), risk-tier fine print, keyboard hint text.
- **`mono` (SF Mono 12px)** — Matrix IDs, homeserver addresses, export paths, verification codes, kbd chips in the palette and cheat sheet.

Rules: no custom display font — keeper has no "hero" typography moment; the wizard headline is just `title` scaled by Tailwind utilities. Timestamps and IDs are always `mono` or `caption`, never body-weight. Bold in the chat list means exactly one thing: unread.

## Layout & Spacing

Tailwind v4's 4px spacing scale is inherited as-is. The app frame is a fixed three-pane + optional fourth:

```
[sidebar 260px][chat list 320px][conversation ≥480px][detail 320px, toggleable]
```

- **`traffic-light-inset-x/y`** — the window uses a transparent/overlay titlebar; the sidebar header reserves `{spacing.traffic-light-inset-x}` × `{spacing.traffic-light-inset-y}` so macOS traffic lights never overlap content, in both expanded and collapsed sidebar states.
- **Sidebar (`{spacing.sidebar-width}`)** — shadcn `Sidebar`, collapsible to a 48px icon rail; the chat list is *not* part of the sidebar and never collapses away.
- **Chat list (`{spacing.chat-list-width}`)** — fixed width, user-resizable ±25% with persistence.
- **Conversation pane** — flexes; timeline text column capped at `{spacing.content-max-width}` and centered when the pane is wider.
- **Detail panel (`{spacing.detail-panel-width}`)** — a fixed right pane when window width ≥ 1280px; below that it presents as a shadcn `Sheet` over the conversation.
- Minimum window: 940 × 600 (sidebar auto-collapses to rail below 1080px width).

Density is macOS-utility, not web-comfortable: 64px chat rows, 8px vertical rhythm inside rows, 12px pane gutters.

**Phone tier (< `{spacing.phone-breakpoint}`, iOS phase):** the same panes render one at a time as full-screen stack levels — Inbox → Room → Detail — under a `{components.phone-header}` bar. Nothing is restyled: same tokens, same components, same density, plus three phone-only constraints — every tappable ≥ `{spacing.touch-target-min}`, edge-to-edge rendering padded by `{spacing.safe-area}`, and the composer bottom-anchored above `{spacing.kb-inset}`. Row swipes reveal `{components.swipe-action}` surfaces. Behavior lives in `EXPERIENCE.md.Responsive & Platform`.

## Elevation & Depth

Flat panes separated by 1px `border` lines — the three-pane frame has **no** shadows between panes. shadcn's default shadow language applies only to transient layers: popovers, dropdowns, dialogs, the command palette, and the undo-send pill (which floats above the composer). 

Vibrancy: the sidebar *may* use macOS behind-window vibrancy (Tauri window effects) with `sidebar` tokens becoming translucent equivalents; this is optional polish with a mandatory graceful fallback to the solid `sidebar` token — all contrast rules below are validated against the solid fallback, and vibrancy must never reduce text contrast below AA. [ASSUMPTION] Vibrancy ships only if the Tauri effect is stable on target macOS versions; it is a nice-to-have, not identity.

Dark mode is a first-class theme, not an inversion: dark tokens are hand-picked (see `colors`), surfaces layer by lightness (background → card → popover), and the bridge-health trio keeps AA contrast on both themes.

## Shapes

Slightly softer than stock shadcn to sit next to macOS Sonoma-era chrome: `{rounded.sm}` 5px inputs and badges, `{rounded.md}` 7px buttons/rows/cards, `{rounded.lg}` 10px dialogs and the palette, `{rounded.xl}` 14px message bubbles. Full-round (`{rounded.full}`) is reserved for avatars, pins (circular, Beeper-style), status dots, the unread badge, the incognito chip, and the undo-send pill. No sharp-cornered surfaces anywhere; no asymmetric radii except message bubbles' tail-side corner (4px on the sender side).

## Components

Used as-is from shadcn, unchanged: `Button`, `Dialog`, `Sheet`, `Tabs`, `DropdownMenu`, `ContextMenu`, `Popover`, `Tooltip`, `ScrollArea`, `Separator`, `Skeleton`, `Switch`, `Input`, `InputGroup`, `Textarea`, `Label`, `Card`, `Avatar`, `Badge`, `Command`, `Sidebar`, `sonner` toasts. Additions to install via shadcn CLI when needed: `alert`, `alert-dialog`, `progress`, `checkbox`, `select`, `radio-group`, `kbd` (or a 6-line local kbd chip on `{typography.mono}`).

keeper-specific treatments (behavior in EXPERIENCE.md; visuals here):

- **Chat row** — 64px: Avatar with `network-badge` overlay (bottom-right, 16px, 2px background ring), name + timestamp line, preview line, right-aligned `unread-badge` / `draft-marker` / muted-bell glyph. Account attribution: a 3px left-edge bar in the account's hue (`components.account-marker`). Unread rows set the name in weight 600; nothing else in the list is ever bold.
- **Pins row** — circular 44px avatars in a horizontal strip at the top of the chat list, network badge overlaid, no labels; removed from the chronological flow below.
- **Favorites section** — labeled group (`section-label`: FAVORITES) of compact 48px rows pinned between the Pins strip and the inbox scroll.
- **Message bubbles** — outgoing `{components.message-bubble-outgoing}`, incoming `{components.message-bubble-incoming}`; consecutive same-sender messages group with 2px gaps and single avatar. Per-message state renders in `caption` under the last bubble of a group: Sending… / Sent / Failed — Retry (destructive) / Queued (amber). Edited marker: "Edited" caption, click reveals history per archive rules. Reactions: pill row under the bubble, `muted` background, count in `caption`.
- **Undo-send pill** — floating pill above the composer, `{components.undo-send-pill}`: radial countdown ring, remaining seconds, "Undo" action. The only animated element in the send path.
- **Incognito chip** — `{components.incognito-chip}` in the chat header showing effective scope ("Incognito — this chat" / "— account" / "— global"); the composer's focus ring tints violet while incognito applies.
- **Bridge card** — shadcn `Card` per network: network glyph, name, `risk-tier-badge`, `bridge-health-dot` + state word, primary action (Connect / Re-link / Manage). Unhealthy cards get a left `{colors.bridge-disconnected}` 3px edge.
- **Risk-tier acknowledgment** — volatile networks use `AlertDialog` with the tier badge, plain-language ToS/ban copy from the tier table, and an explicit "I understand the risk — connect" confirm.
- **Command palette** — shadcn `Command` in a 640px `{rounded.lg}` panel; results show type glyph (chat/contact/action), network badge for chats, account hue dot, and a right-aligned kbd chip for actions with shortcuts. Active row uses stock shadcn accent.
- **QR login panel** — white card (`{rounded.lg}`) containing the QR at ≥ 240px with quiet zone, network glyph centered, `caption` instruction line, live state word below (Waiting for scan… / Linked ✓ in `{colors.bridge-healthy}`). QR renders identically in dark mode (white card is mandatory for scannability).

Screen Recording (Phase 3, desktop-only — every treatment below renders only when the `recording` capability flag is on; behavior in `EXPERIENCE.md.Screen Recording`):

- **Recording view** — a single non-chat utility surface (no timeline, no composer) that flips between two states in place: *pre-record setup* (a stack of shadcn `Card` sections — Source, Audio, Webcam, Destination, Segmenting, and a collapsed Advanced for fps) and *active recording* (the `{components.active-recording-banner}` pinned to the top, the source summary, and the `{components.segment-meter}`). Uses `{spacing.content-max-width}` as a centered single column; it is not a pane frame.
- **Source picker** — a scrollable list of `{components.source-picker-row}` grouped "Displays" then "Applications" (`section-label` headers), each row a live entry with name and app icon; a subtle "refreshing…" affordance while the list re-enumerates. Full-screen displays show a monitor glyph; apps show their icon. Single-select.
- **Audio + Webcam controls** — a `Switch` for System audio (default on) and a `{components.device-picker}` for the microphone (default "System default input"); a `Switch` for Webcam (default off) revealing its camera `{components.device-picker}`. Each toggle-off greys its picker with a helper `caption`. Honest copy sits under the group: "Recorded locally. Nothing uploads." System audio and mic are labelled as *separate tracks*, not a mix.
- **Permission pre-flight** — a `{components.permission-row}` per required permission (Screen Recording always; Microphone / Camera only when those sources are enabled), each showing the live-detected state. The `note-line` carries macOS quirks plainly, including the subtle dev-facing "ad-hoc dev builds may be blocked on macOS 15+ — sign with an Apple Development certificate" caveat. Start is disabled with the blocking permission named until every required grant is green.
- **Active-recording banner** — `{components.active-recording-banner}`: recording-red edge, record dot, `mono` elapsed · segment · size line, Stop. It is the in-app twin of the tray, persistent and never a toast; its warning and error variants are the loud-failure surface (FR-75). Pause is not present this phase.
- **Tray recording states** — `{components.tray-recording}`: idle → recording (record-dot badge) → warning/error (badge). The menu gains a `mono` elapsed/segment line (disabled, ~1 Hz), Stop Recording, and Open Recordings Folder above the existing Show keeper / Quit. Recording forces the tray visible even when the opt-in tray toggle is off, and restores the prior state at stop. keeper never touches macOS's own purple screen-recording pill — the tray adds what the pill lacks (elapsed, segment, Stop, errors).
- **Completion card** — on Stop or recovery, a `Card` in the Recording view: "Saved N segments · 3.6 GB" with the session-folder path in `mono` and a primary **Reveal in Finder** action. No preview, no trim, no share — recording produces ordinary files and hands them off (FR-76). The parallel recovery notice ("A recording was interrupted; N segments were saved") uses the same card shape with a `bridge-degraded`-tinted edge.

## Do's and Don'ts

| Do | Don't |
|---|---|
| Inherit shadcn defaults for everything outside the brand layer | Restyle stock shadcn components "to feel more branded" |
| Amber only for written-not-sent (drafts, approval, undo-send, queued) | Amber for warnings, hovers, or decoration |
| Violet only for incognito state | Any second meaning for violet, ever |
| Persistent indicators for bridge health (dot + row + banner) | Toast-only error states that can be dismissed and lost |
| System font stack, `mono` for IDs/codes/timestamps | Custom display fonts or webfonts |
| 1px borders between panes; shadows only on transient layers | Shadowed/floating panes, glassmorphism beyond the optional sidebar vibrancy |
| White card behind every QR code in both themes | Theme-tinted or dark-background QR codes |
| Network identity as a 16px badge on the avatar | Per-network coloring of rows, panes, or bubbles |
| Bold in chat list = unread, nothing else | Bold for emphasis, favorites, or pinned state |
| Respect traffic-light insets in every sidebar state | Content or controls under the macOS window buttons |
| Phone tier rearranges the same components under the same tokens | A second "mobile" visual language, forked chat components, or platform-specific restyling |
| Respect safe-area insets on every phone surface, including sheets and overlays | Content under the notch or home indicator, unstyled bands at the screen edges |
| Recording red only for the live record dot / active pill / loud error banner | Recording red on buttons, text, hovers, or as a fifth brand hue on the chrome |
| Recording faults are persistent banners + tray error + notification | Toast-only recording errors, or a record indicator that isn't visible while capture runs |
| Recording UI ends at Reveal in Finder — ordinary local files | Any in-app trim/annotate/preview-scrub, upload, share link, or transcription affordance |
| Render every recording surface only behind the `recording` capability flag | Dead recording buttons on iOS or macOS < 13 |
