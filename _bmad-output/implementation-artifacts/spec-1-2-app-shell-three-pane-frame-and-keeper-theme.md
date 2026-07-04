---
title: 'Story 1.2 — App Shell: Three-Pane Frame and keeper Theme'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
baseline_revision: 'da8089c07dd0fcab8e63f87990898753a35b8fa5'
final_revision: '7264927732e2a77b0141f5ea596d0f905d72c721'
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper opens as an 800×600 starter window showing only a centered "keeper" heading, with neutral shadcn defaults and the Inter font. There is no application frame and no brand identity, so every later Epic-1 feature (login, room list, timeline, composer) would have to be built inside a placeholder UI and later re-parented onto the real shell.

**Approach:** Establish the final visual frame and brand layer up front: apply keeper's brand tokens (UX-DR1) to `src/index.css` on top of wholesale shadcn defaults for both light and dark themes, wire system-following theme selection, and build the three-pane app shell (UX-DR2) — `[sidebar 260px | chat list 320px | conversation ≥480px]` plus a toggleable 320px detail panel — with an overlay macOS titlebar, enforced minimum window, responsive collapse, ARIA landmarks, visible focus rings (NFR-14), and static data-free placeholders (UX-DR18) that later stories fill in place.

## Boundaries & Constraints

**Always:**
- Brand tokens are defined in `src/index.css` for **both** `:root` (light) and `.dark`; unlisted shadcn tokens are left untouched and inherit shadcn defaults (UX-DR1). Keeper green overrides `--primary`/`--primary-foreground`; all other brand colors (held amber, incognito violet, bridge trio, search highlight) are **additive** new tokens.
- Held amber is used **only** for written-not-sent states (drafts/queued/approval/undo) — so it must NOT reuse shadcn's `--accent`, which is the hover/selected token. Introduce a dedicated `--held` token instead.
- The radii scale resolves to exactly 5/7/10/14px (sm/md/lg/xl); the font family is the macOS system stack throughout (no Inter, no custom display font).
- Theme follows the OS by default via the existing `.dark` class strategy; dark values are hand-picked (this story wires system-following, not a manual toggle UI).
- Panes use semantic landmarks: sidebar = `<nav>`, chat list = `<ul>`, conversation = `<main>`, detail = `role="complementary"`. 1px `border` between panes, **no** inter-pane shadows. Every focusable control shows the shadcn `ring` focus-visible ring (NFR-14).
- Minimum window 940×600 enforced by the Tauri window config; sidebar auto-collapses to a 48px icon rail below 1080px; detail panel is a pinned 320px pane at ≥1280px and a shadcn `Sheet` below 1280px. The sidebar header reserves the 78×12px macOS traffic-light inset in every state.
- TS: no `any`, `import type` for type-only imports, `@/` path alias, 2-space/100-col/double-quote Biome formatting. Reuse `cn()` and installed shadcn primitives; do not hand-write files in `src/components/ui/`.

**Block If:**
- Achieving the overlay titlebar / traffic-light inset or the 940×600 minimum would require a Tauri capability or config option not available in the pinned Tauri 2.11.x (would signal a stack-anchor conflict).

**Never:**
- No Matrix data, IPC calls, zustand stores, or crypto/login/sync logic — panes render static placeholders only (those are stories 1.3+). No `matrix-js-sdk` or any Matrix JS lib.
- No manual light/dark toggle UI or Settings screen (later epic); only system-following wiring here.
- No new heavy layout dependency; build the frame from Tailwind + installed shadcn primitives.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Wide layout | window width ≥ 1280px | four panes; detail panel renders pinned (320px) when toggled open | n/a |
| Mid layout | 1080px ≤ width < 1280px | sidebar full (260px); toggling detail opens it as a `Sheet` over the conversation, not a pinned pane | n/a |
| Narrow layout | 940px ≤ width < 1080px | sidebar collapses to 48px icon rail (tooltips on icons); chat list + conversation remain | n/a |
| Below minimum | attempt resize < 940×600 | OS blocks the resize (Tauri `minWidth`/`minHeight`); layout never squishes below minimum | Enforced by window config, not JS |
| Detail toggle | user activates the detail toggle control | detail panel opens/closes; on close, focus returns to the toggle control | n/a |
| System dark mode | OS `prefers-color-scheme: dark` | `.dark` applied on `<html>`; dark brand token values active; contrast holds | n/a |

</intent-contract>

## Code Map

- `src/index.css` -- brand-token layer: override `--primary`/`--primary-foreground` (keeper green) in `:root` + `.dark`; add additive tokens `--held`/`--held-foreground`, `--incognito`/`--incognito-foreground`, `--bridge-healthy`/`--bridge-degraded`/`--bridge-disconnected`, `--search-highlight`/`--search-highlight-foreground`; register each additive token in `@theme inline` as `--color-*`; set `--radius-sm:5px`/`--radius-md:7px`/`--radius:10px` (lg=10, xl=14); swap `--font-sans` to the macOS system stack and remove the `@fontsource-variable/inter` import.
- `src-tauri/crates/keeper/tauri.conf.json` -- window: `width:1280`, `height:800`, `minWidth:940`, `minHeight:600`, `titleBarStyle:"Overlay"`, `hiddenTitle:true` (overlay macOS titlebar so traffic lights float over the sidebar header).
- `src/main.tsx` -- wrap `<App/>` in next-themes `ThemeProvider` (`attribute="class"`, `defaultTheme="system"`, `enableSystem`, `disableTransitionOnChange`).
- `src/App.tsx` -- render `<AppShell/>` instead of the placeholder heading.
- `src/components/layout/app-shell.tsx` -- NEW: top-level frame; composes the panes, owns the detail-panel open/close state and the responsive layout via `useShellLayout`; draggable overlay top strip (`data-tauri-drag-region`).
- `src/components/layout/sidebar-pane.tsx` -- NEW `<nav>` placeholder: header reserving the 78×12px traffic-light inset; a static view list (Chats / Bridges / Settings as focusable ghost `Button`s); collapses to a 48px icon rail (icons + tooltips) when `collapsed`.
- `src/components/layout/chat-list-pane.tsx` -- NEW `<ul>` placeholder: empty-state copy "Synced. No conversations yet." (no Matrix data).
- `src/components/layout/conversation-pane.tsx` -- NEW `<main>` placeholder: empty conversation copy; hosts the detail-panel toggle control (aria-label, focus ring).
- `src/components/layout/detail-panel.tsx` -- NEW: `role="complementary"` 320px panel content; rendered pinned or inside a `Sheet` by `AppShell` per width.
- `src/hooks/use-shell-layout.ts` -- NEW matchMedia hook returning `{ sidebarCollapsed: boolean /* <1080 */, detailFloating: boolean /* <1280 */ }`, mirroring the `use-mobile.ts` pattern.
- `src/test/setup.ts` -- add a `window.matchMedia` mock (jsdom omits it) so shell/hook tests don't throw.
- `src/App.test.tsx` -- update to assert shell landmarks/placeholders instead of the old heading.
- `src/components/layout/app-shell.test.tsx` -- NEW render test (landmarks, placeholder copy, detail toggle).
- `src/hooks/use-shell-layout.test.ts` -- NEW hook test over mocked matchMedia breakpoints.
- `package.json` -- remove the now-unused `@fontsource-variable/inter` dependency.

## Tasks & Acceptance

**Execution:**
- [x] `src/index.css` -- apply the brand-token layer per the Design Notes token table: override keeper green primary in `:root`+`.dark`, add the additive brand tokens (both themes) and register them in `@theme inline`, fix the radii to 5/7/10/14px, switch `--font-sans` to the macOS system stack, and remove the Inter `@import`.
- [x] `src-tauri/crates/keeper/tauri.conf.json` -- set the window default size (1280×800), enforce `minWidth:940`/`minHeight:600`, and enable the overlay titlebar (`titleBarStyle:"Overlay"`, `hiddenTitle:true`).
- [x] `src/hooks/use-shell-layout.ts` -- implement the matchMedia hook (`<1080` → `sidebarCollapsed`, `<1280` → `detailFloating`) with add/remove listener cleanup.
- [x] `src/components/layout/{sidebar-pane,chat-list-pane,conversation-pane,detail-panel}.tsx` -- implement the four static placeholder panes with the correct landmarks, brand styling, traffic-light inset, empty-state copy, and focus-visible rings; no Matrix data.
- [x] `src/components/layout/app-shell.tsx` -- compose the frame: fixed 260/320px + flexing ≥480px conversation, 1px inter-pane borders/no shadows, draggable overlay top strip, detail-panel open state with focus-return-on-close, and the pinned-vs-`Sheet` + icon-rail responsive behavior driven by `useShellLayout`.
- [x] `src/main.tsx` + `src/App.tsx` -- mount the next-themes provider (system default) and render `<AppShell/>`.
- [x] `src/test/setup.ts` -- add the `window.matchMedia` mock.
- [x] `src/components/layout/app-shell.test.tsx`, `src/hooks/use-shell-layout.test.ts`, `src/App.test.tsx` -- cover the I/O matrix edge cases (landmarks render, placeholders render without Matrix data, detail toggle opens/closes, hook returns correct booleans per mocked width).
- [x] `package.json` -- remove `@fontsource-variable/inter`.

**Acceptance Criteria:**
- Given DESIGN.md's brand-layer tokens, when the app renders in light and dark mode, then `src/index.css` defines keeper green primary, held amber, incognito violet, the bridge-health trio, and search-highlight tokens for both themes plus the macOS system font stack and the 5/7/10/14px radii, with all unlisted tokens inheriting shadcn defaults, and light/dark follow the system by default (UX-DR1).
- Given the window frame, when keeper opens, then the layout is `[sidebar 260px | chat list 320px | conversation ≥480px]` with a toggleable 320px detail-panel slot, an overlay titlebar whose sidebar header reserves the 78×12px traffic-light inset, and 1px borders between panes with no inter-pane shadows; the 940×600 minimum is enforced and the sidebar auto-collapses to a 48px icon rail below 1080px (UX-DR2).
- Given keyboard use, when focus moves through the shell, then every focusable control shows the visible focus ring and the pane placeholders (sidebar view list, empty chat list, empty conversation) render without any Matrix data (NFR-14, UX-DR18).
- Given the quality gates, when `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check` (from `src-tauri/`) run, then all pass.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 1, low 3)
- defer: 0
- reject: 4: (high 0, medium 0, low 4)
- addressed_findings:
  - `[medium]` `[patch]` Pinned detail panel was clipped off-screen in the 1280–1379px band — the conversation `<main>` carried a hard `min-w-[480px]` while the ancestor had `overflow-hidden`, so 260+320+480+320=1380px could not fit and the rightmost pane was cut off. Changed the conversation to `min-w-0` so it absorbs the shortfall and the detail pane stays fully visible.
  - `[low]` `[patch]` Overlay-titlebar traffic-light overlap: the `data-tauri-drag-region` strip was only `h-3` (12px), so the macOS traffic lights overlapped the top of the panes and (in the 48px collapsed rail) the first sidebar icon, and the draggable band was a thin sliver. Widened it to `h-7` (~28px, standard macOS titlebar height) so the lights float over the empty draggable band above all panes in every state — this resolves both the collapsed-rail overlap and the thin-drag-region findings.
  - `[low]` `[patch]` `useShellLayout` hardcoded its initial state to `{false,false}` and only computed real values in the post-mount effect, flashing the wide layout for one frame when the window opens narrow. Switched to a lazy `useState` initializer that reads `window.matchMedia` synchronously (guarded for its absence).
- rejected (noise, dropped): resize below 1280 with detail open auto-presenting the Sheet (this is the spec-defined pinned→Sheet behavior); manual `.focus()` vs Radix focus-restore race on Sheet close (benign — Radix restores to the toggle in the invoked case; NFR-14 met); `addEventListener('change')` on old WKWebView (matches the existing `use-mobile.ts` convention and the target macOS WebView supports it); `<ul>` empty-state announced as a one-item list (placeholder markup that story 1.4 replaces wholesale).

## Design Notes

**Brand token table (exact values from DESIGN.md — light / dark).** Register every additive token in `@theme inline` (e.g. `--color-held: var(--held);`) so Tailwind emits `bg-held`, `text-incognito`, etc.

| Token (CSS var) | Light | Dark | Notes |
|---|---|---|---|
| `--primary` | `#0F6E5C` | `#3ECFAE` | override shadcn primary = keeper green |
| `--primary-foreground` | `#FFFFFF` | `#06231C` | |
| `--held` | `#B45309` | `#F5A623` | additive; written-not-sent only |
| `--held-foreground` | `#FFFFFF` | `#231303` | |
| `--incognito` | `#6D28D9` | `#A78BFA` | additive; incognito only |
| `--incognito-foreground` | `#FFFFFF` | `#1E1038` | |
| `--bridge-healthy` | `#16A34A` | `#16A34A` | additive semantic dot |
| `--bridge-degraded` | `#D97706` | `#D97706` | additive |
| `--bridge-disconnected` | `#DC2626` | `#DC2626` | additive |
| `--search-highlight` | `#FDE68A` | `#78560A` | additive; match background tint |
| `--search-highlight-foreground` | `#231303` | `#FDE68A` | readable text on tint |

Radii: set `--radius: 10px` (=lg), `--radius-sm: 5px`, `--radius-md: 7px`; `--radius-xl` already resolves to 14px via the existing `* 1.4` calc (leave 2xl–4xl as-is). Font: `--font-sans: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;` and drop the `@import "@fontsource-variable/inter";` line.

**Why held amber is a new token, not `--accent`:** shadcn's `--accent`/`--accent-foreground` are the hover/selected background across Button/DropdownMenu/Command/etc. UX-DR1 restricts held amber to written-not-sent states and forbids it on hovers, so overriding `--accent` would wrongly repaint every hover amber. Keep `--accent` inherited; expose amber only as `--held`.

**Why a custom shell, not shadcn `Sidebar`:** the installed `sidebar.tsx` is built around a 768px mobile→Sheet model with its own provider/cookie state; keeper's breakpoints are 1080 (icon-rail) and 1280 (detail pinned↔Sheet). A small custom flex frame + `useShellLayout` is simpler and gives clean seams the later stories fill, while still reusing `Button`, `Tooltip`, `Sheet`, `Separator`, and `Skeleton` primitives.

**Overlay titlebar:** `titleBarStyle: "Overlay"` (macOS) makes the traffic lights float over content; the sidebar header must pad ≥78px left and ≥12px top so its first control clears them, and the top strip needs `data-tauri-drag-region` to stay window-draggable (interactive controls sit outside the drag region or opt out). On non-macOS the option degrades to the native chrome — acceptable (macOS-first).

**Theme wiring:** next-themes (`attribute="class"`, `defaultTheme="system"`, `enableSystem`) toggles `.dark` on `<html>`, which the existing `@custom-variant dark (&:is(.dark *))` already targets. No SSR flash guard needed (Vite client render).

**jsdom note:** jsdom has no `window.matchMedia`; both `useShellLayout` and (indirectly) `App` need the mock added to `src/test/setup.ts`, or the render tests throw.

## Verification

**Commands:**
- `bun run check` -- expected: biome lint + tsc strict + vitest (incl. new shell/hook tests and updated `App.test.tsx`) all pass.
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` clean (no Rust changed; must stay green).
- `bun run test:rust` -- expected: nextest green; bindings unchanged.
- `cd src-tauri && cargo deny check` -- expected: license firewall passes (no new crates).

**Manual checks:**
- `bun run tauri dev` -- window opens ~1280×800 with the overlay titlebar; traffic lights clear the sidebar header; three panes visible with 1px borders and no inter-pane shadows; toggling the detail control opens the 320px panel (pinned wide / `Sheet` mid-width); dragging the window below 940×600 is blocked; switching macOS appearance flips brand light↔dark; Tab shows a visible focus ring on every control.

## Auto Run Result

Status: **done**

### Summary
Built the keeper app shell and brand layer. `src/index.css` now applies the UX-DR1 brand tokens on top of wholesale shadcn defaults for both light and dark themes — keeper green as `--primary`, plus additive `--held` (amber), `--incognito`, the bridge-health trio, and search-highlight tokens (each registered in `@theme inline`) — with the 5/7/10/14px radii and the macOS system font stack (Inter removed). next-themes drives system-following light/dark via the existing `.dark` class. The Tauri window gained a 1280×800 default, an enforced 940×600 minimum, and an overlay titlebar. A custom `AppShell` renders the `[sidebar 260 | chat list 320 | conversation ≥480]` + toggleable 320px detail frame with 1px inter-pane borders and no shadows, ARIA landmarks (`nav`/`ul`/`main`/`complementary`), visible focus rings, a `useShellLayout` matchMedia hook (sidebar→48px icon rail <1080px; detail→`Sheet` <1280px), and static data-free placeholders. Held amber is deliberately a new token (not shadcn `--accent`) so hovers stay neutral per UX-DR1.

### Files changed
- `src/index.css` — brand tokens (green primary override + additive held/incognito/bridge/search tokens for both themes, registered in `@theme inline`); radii → 5/7/10/14px; `--font-sans` → macOS system stack; removed the Inter `@import`.
- `src-tauri/crates/keeper/tauri.conf.json` — window `1280×800`, `minWidth:940`/`minHeight:600`, `titleBarStyle:"Overlay"`, `hiddenTitle:true`.
- `src/main.tsx` — wrapped `<App/>` in next-themes `ThemeProvider` (system default).
- `src/App.tsx` — renders `<AppShell/>`.
- `src/components/layout/{app-shell,sidebar-pane,chat-list-pane,conversation-pane,detail-panel}.tsx` — NEW three/four-pane frame + placeholder panes.
- `src/hooks/use-shell-layout.ts` — NEW responsive matchMedia hook (1080/1280 breakpoints).
- `src/test/setup.ts` — added a `window.matchMedia` mock (jsdom omits it).
- `src/App.test.tsx`, `src/components/layout/app-shell.test.tsx`, `src/hooks/use-shell-layout.test.ts` — landmark/placeholder/detail-toggle and per-width hook tests.
- `package.json` / `bun.lock` — removed the unused `@fontsource-variable/inter` dependency.

### Review findings
- Two reviewers (adversarial-general Blind Hunter + edge-case-hunter). Triage: 0 intent_gap, 0 bad_spec, 4 patch (1 medium, 3 low), 0 defer, 4 reject. See Review Triage Log.
- Patches: conversation `min-w-0` (fixes pinned detail clipping in the 1280–1379px band); drag region `h-3`→`h-7` (traffic lights float over the titlebar band, not pane content, in every state — fixes collapsed-rail overlap + thin drag band); lazy `useShellLayout` init (removes first-paint layout flash on narrow launch).

### Verification
- `bun run check` (biome 57 files + tsc strict + vitest 11 + core-tauri-free) ✅ — re-run green after patches.
- `bun run check:rust` (rustfmt + clippy `-D warnings`) ✅ · `bun run test:rust` (17 tests) ✅ — no Rust changed.
- `cd src-tauri && cargo deny check licenses bans sources` ✅ (exit 0). The full `cargo deny check` `advisories` subsection remains red on pre-existing unmaintained transitive Tauri deps (present on baseline `da8089c`, no new crates added) — same residual noted in story 1.1; the license firewall passes.
- Not run: `bun run tauri dev` (blocking GUI). The overlay-titlebar visuals, drag feel, and the pinned↔Sheet transition at real widths are only verifiable there — see the manual checks above.

### Residual risks
- Overlay-titlebar chrome (traffic-light clearance, drag-band feel) and the responsive pinned↔Sheet detail transition are macOS-runtime-visual and were reasoned about, not visually verified (no `tauri dev` in this run).
- Placeholder panes (chat list, conversation, detail, sidebar view list) are static and will be replaced by stories 1.3–1.5; the `useShellLayout` breakpoints and brand tokens are the durable seams.
- Pre-existing `cargo deny` advisories (unmaintained gtk/unic transitive deps via Tauri) remain out of scope; consider a `deny.toml [advisories] ignore` housekeeping task if a fully-green `cargo deny check` becomes a release gate.
