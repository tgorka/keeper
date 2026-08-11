# keeper UI inventory — what the app actually looks like today

Audited 2026-08-11 against the worktree at `/home/dev/.paseo/worktrees/2va3pp5x/quick-donkey`.
Every claim below is grounded in a file:line that was read. Counts are over the **331
non-test source files** under `src/` (test files and `src/lib/ipc/gen/**` excluded).

---

## Verdict

**This is a token swap, not a campaign.** keeper's appearance is already almost entirely
centralised: `src/index.css` holds 55 colour custom properties (light + dark, at full
parity), and of **1287 colour-bearing utility occurrences across 138 files, exactly 19 are
raw colours — 1.5%**. There is one runtime colour literal in the whole app
(`read-receipts.tsx:74`), zero gradients, zero hex in TSX outside comments, and the two
CodeMirror themes are written entirely in `var(--token)`. The 19 offenders are individually
listed below and are a half-day of work.

What is *not* free is the other three axes. Typography is unowned: no self-hosted face, the
stack is `-apple-system` (`index.css:9`), `--font-heading` is aliased straight to
`--font-sans` (`index.css:8`) so the display slot exists but resolves to the body face, and
**11 distinct text sizes** are in use, two of them (`text-[11px]` ×39, `text-[10px]` ×6)
arbitrary. The brand green already exists — `--primary: #0f6e5c` (`index.css:73`), the same
green as the app icon — but it renders on only **11 surfaces** (`bg-primary` ×8,
`text-primary` ×3) because shadcn's `--accent` is a neutral grey, not the brand. And the
identity surfaces are literally blank: the splash is an empty `bg-background` div with an
`sr-only` label (`App.tsx:144-151`), `index.html:5` still ships `/vite.svg` as the favicon
(a file that does not exist in the repo) under the title `Tauri + React + Typescript`
(`index.html:15`).

So: retheming is cheap, *characterising* is the work, and the work concentrates in about
eight files plus one new typeface and one new mark.

---

## 1. Every colour the app can render

### 1.1 The theme block — `src/index.css`

`@theme inline` (`index.css:7-64`) maps every `--x` to a Tailwind `--color-x` utility, so
each token below is reachable as `bg-*` / `text-*` / `border-*`. Light values are in
`:root` (`index.css:66-144`), dark in `.dark` (`index.css:146-206`). **Every colour token
is defined in both blocks — dark parity is complete.**

**shadcn core (18)**

| token | light (`:root`) | dark (`.dark`) |
|---|---|---|
| `--background` | `oklch(1 0 0)` | `oklch(0.145 0 0)` |
| `--foreground` | `oklch(0.145 0 0)` | `oklch(0.985 0 0)` |
| `--card` | `oklch(1 0 0)` | `oklch(0.205 0 0)` |
| `--card-foreground` | `oklch(0.145 0 0)` | `oklch(0.985 0 0)` |
| `--popover` | `oklch(1 0 0)` | `oklch(0.205 0 0)` |
| `--popover-foreground` | `oklch(0.145 0 0)` | `oklch(0.985 0 0)` |
| `--primary` | `#0f6e5c` | `#3ecfae` |
| `--primary-foreground` | `#ffffff` | `#06231c` |
| `--secondary` | `oklch(0.97 0 0)` | `oklch(0.269 0 0)` |
| `--secondary-foreground` | `oklch(0.205 0 0)` | `oklch(0.985 0 0)` |
| `--muted` | `oklch(0.97 0 0)` | `oklch(0.269 0 0)` |
| `--muted-foreground` | `oklch(0.556 0 0)` | `oklch(0.708 0 0)` |
| `--accent` | `oklch(0.97 0 0)` | `oklch(0.269 0 0)` |
| `--accent-foreground` | `oklch(0.205 0 0)` | `oklch(0.985 0 0)` |
| `--destructive` | `oklch(0.577 0.245 27.325)` | `oklch(0.704 0.191 22.216)` |
| `--border` | `oklch(0.922 0 0)` | `oklch(1 0 0 / 10%)` |
| `--input` | `oklch(0.922 0 0)` | `oklch(1 0 0 / 15%)` |
| `--ring` | `oklch(0.708 0 0)` | `oklch(0.556 0 0)` |

Note the trap: **`--accent` is a hover-surface grey, not a brand accent.** `bg-accent` (22)
and `hover:bg-accent` (15+10) are keeper's row-hover, nothing more. The only brand-coloured
pixels in the app are `bg-primary` ×8, `text-primary` ×3, `text-primary-foreground` ×5, and
`.cm-lp-link` / `.cm-lp-wikilink` (`live-preview.ts:606`).

**Chart (5) — `--chart-1` … `--chart-5`** (`index.css:85-89`, `165-169`). Greyscale ramp
`oklch(0.87 → 0.269 0 0)`, identical light and dark. **Zero call sites.** Dead.

**keeper semantic (10)**

| token | light | dark | where |
|---|---|---|---|
| `--held` / `--held-foreground` | `#b45309` / `#ffffff` | `#f5a623` / `#231303` | 55 uses — the undo-send pill, held drafts |
| `--incognito` / `--incognito-foreground` | `#6d28d9` / `#ffffff` | `#a78bfa` / `#1e1038` | 5 uses (`composer.tsx:910`, `conversation-pane.tsx:366`) |
| `--recording-red` | `#dc2626` | `#ef4444` | 6 uses, `active-recording-banner.tsx` |
| `--bridge-healthy` | `#16a34a` | `#16a34a` | 22 uses across bridge/sync dots |
| `--bridge-degraded` | `#d97706` | `#d97706` | ” |
| `--bridge-disconnected` | `#dc2626` | `#dc2626` | ” |
| `--search-highlight` / `-foreground` | `#fde68a` / `#231303` | `#78560a` / `#fde68a` | 4 uses |

The three `bridge-*` tokens are **identical in light and dark** — they are the only colour
tokens with no dark treatment at all.

**Swipe aliases (6)** — `--swipe-archive|read|discard` + foregrounds
(`index.css:100-108`, `180-187`). Pure `var()` indirection onto `muted-foreground` /
`primary` / `destructive`; no new palette. Correct pattern, keep it.

**Sidebar (8)** — `--sidebar`, `-foreground`, `-primary`, `-primary-foreground`, `-accent`,
`-accent-foreground`, `-border`, `-ring` (`index.css:124-131`, `188-195`). Only `bg-sidebar`
(4 uses) and `border-border` are consumed; **`--sidebar-primary` has zero call sites** and
in dark is `oklch(0.488 0.243 264.376)` — a blue-violet, stock shadcn residue.

**Account hue wheel (8)** — `--account-hue-0` … `-7` (`index.css:136-143`, `198-205`),
evenly spaced oklch hues, brighter in dark. Consumed only via
`accountHueVar(room.hueIndex)` as a 3px row edge bar (`chat-row.tsx:271-273`).

**Non-colour tokens** — `--radius: 10px` (`index.css:109`) plus the `@theme` ramp
`--radius-sm: 5px`, `--radius-md: 7px`, and `lg/xl/2xl/3xl/4xl` derived by multiplier
(`index.css:57-63`); `--phone-header: 52px` (`:112`); `--safe-top|right|bottom|left` from
`env(safe-area-inset-*)` (`:116-119`); `--kb-inset: 0px` (`:123`). One `@utility`:
`touch-callout-none` (`:211-214`).

### 1.2 Raw colour outside `index.css` — **19 occurrences, 11 files**

Scanned every non-test `.ts`/`.tsx`/`.css` under `src/` for hex, `rgb(`, `hsl(`, `oklch(`,
and Tailwind named-colour utilities.

- **hex: 0.** (Five `#nnnnn` matches are all issue numbers in prose comments — e.g.
  `App.tsx:54` "tauri#14371".)
- **`rgb(` / `oklch(`: 0.**
- **`hsl(`: 1** — `read-receipts.tsx:74`,
  `style={{ backgroundColor: \`hsl(${hueOf(userId)} 55% 45%)\` }}`. **The single worst
  offender**: a second, undocumented per-user hue wheel that does not go through
  `--account-hue-*`, and it is the only runtime-computed colour in the app.
- **Named-colour utilities: 18**

| site | utility | verdict |
|---|---|---|
| `ui/dialog.tsx:33`, `ui/alert-dialog.tsx:28`, `ui/sheet.tsx:31`, `layout/phone-search-surface.tsx:185` | `bg-black/10` | stock shadcn scrim — 4 sites, one token fixes all |
| `chat/media-attachment.tsx:212`, `:267` | `bg-black/50` | media overlay scrim |
| `chat/media-attachment.tsx:212`, `:268` | `text-white` | on the scrim above |
| `chat/read-receipts.tsx:73` | `text-white` | on the `hsl()` chip |
| `layout/phone-inbox-header.tsx:74`, `layout/account-footer.tsx:131`, `layout/conversation-pane.tsx:232` | `text-white` | avatar initials over `--account-hue-*` |
| `bridges/bridge-card.tsx:61` | `text-white` | on `bg-bridge-disconnected` |
| `settings/key-backup-dialog.tsx:257`, `settings/device-verification-dialog.tsx:115` | `text-emerald-600 dark:text-emerald-400` | **the only chromatic offenders** — an ad-hoc success green that is *not* the brand green |
| `settings/device-verification-dialog.tsx:147`, `bridges/bridge-login-sheet.tsx:253` | `bg-white` | QR-code plate; legitimately must be white |

**Migration size: 19 edits, of which 2 are legitimate (QR plates) and 4 collapse into one
scrim token. Call it 13 real changes.**

### 1.3 Guardrail baseline (against the tgsite DESIGN.md rules)

| rule | current state |
|---|---|
| no gradients | **clean** — 0 matches for `bg-gradient`/`bg-linear`/`*-gradient` |
| no glass / backdrop-blur | **4 violations**: `App.tsx:199` (`backdrop-blur-sm` behind the add-account overlay); `ui/dialog.tsx:33`, `ui/alert-dialog.tsx:28`, `ui/sheet.tsx:31` (`supports-backdrop-filter:backdrop-blur-xs`) |
| no purple/violet | **2 live violations**: `--incognito` `#6d28d9` light / `#a78bfa` dark, on 5 surfaces. Plus dead `--sidebar-primary` violet in dark |
| shadow discipline | near-clean: `shadow-xs` ×8, `shadow-none` ×5, `shadow-md` ×2, `shadow-lg` ×3 (context-menu, dropdown-menu, sheet) |
| emoji as UI | clean — the 1520 emoji live in the reaction picker and emoji-autocomplete data, which is content, not chrome |

---

## 2. Typography

**No font is self-hosted.** Zero `@fontsource` imports, zero `.woff`/`.woff2` files anywhere
in the repo, zero `@font-face`.

- **Body/UI:** `--font-sans: -apple-system, BlinkMacSystemFont, "SF Pro Text",
  "Helvetica Neue", sans-serif` (`index.css:9`), applied to `html` via
  `@apply font-sans` (`index.css:223-225`). macOS system face.
- **Display:** `--font-heading: var(--font-sans)` (`index.css:8`) — **an alias, not a
  face.** 21 `font-heading` call sites already exist (every pane `<h1>`: `approval-pane.tsx:239`,
  `bridges-pane.tsx:37`, `files-pane.tsx:1807`, `recording-pane.tsx:202`,
  `settings-pane.tsx:32`, `sync-pane.tsx:733`, `recordings-pane.tsx:259`; the wizard's three
  `text-2xl` headings `first-run-wizard.tsx:152/248/351`; `ui/card.tsx:41`,
  `ui/dialog.tsx:108`, `ui/sheet.tsx:99`, `ui/alert-dialog.tsx:106`; the document viewer's
  four block styles `document-viewer.tsx:113-116`). **Changing one line lights up 21
  surfaces.** This is the single highest-leverage typographic move available.
- **Mono:** `--font-mono` is **never defined** in `index.css`; `font-mono` (62 uses) and
  `var(--font-mono, ui-monospace, monospace)` (5 uses, `live-preview.ts:596/618/663/740/829`)
  both fall through to Tailwind v4's default `ui-monospace, SFMono-Regular, Menlo, …`.
- **Weights in use:** `font-medium` ×115, `font-semibold` ×4, `font-normal` ×4. Effectively
  a two-weight system (400/500).
- **Tracking/leading:** `leading-none` ×13, `tracking-wide` ×12, `tracking-widest` ×3,
  `leading-normal` ×1, `leading-4` ×1. Essentially unmanaged.

**Sizes in use — 11 distinct steps:**

| utility | uses | note |
|---|---|---|
| `text-xs` (12px) | 372 | **the app's real body size** |
| `text-sm` (14px) | 236 | secondary body / pane subtitles |
| `text-[11px]` | 39 | arbitrary — note-editor status lines (`note-editor.tsx:142/753/859/868/882/889`), path captions |
| `text-lg` (18px) | 12 | every pane `<h1>` |
| `text-base` (16px) | 6 | |
| `text-[10px]` | 6 | arbitrary |
| `text-2xl` (24px) | 4 | wizard headings only |
| `text-[7px]` | 2 | read-receipt chip initials (`read-receipts.tsx:73`, `:82`) |
| `text-[9px]` | 2 | arbitrary |
| `text-xl` (20px) | 1 | `document-viewer.tsx:113` |
| `text-[0.7rem]` | 1 | arbitrary |

So: **5 Tailwind steps carry 630 of 681 uses (92.5%)**, and 5 arbitrary steps carry 50.
A 5-step scale is not a demolition — `xs / sm / base / lg / 2xl` is already the de-facto
system. The job is to name those five, kill the five arbitraries (`[11px]` is the only one
with real weight, at 39 sites), and put a real face behind `--font-heading`.

---

## 3. The shadcn surface — `src/components/ui/`

32 non-test files. The registry is **`radix-vega`**, not stock shadcn
(`components.json:3`), `baseColor: neutral`, `cssVariables: true`, `iconLibrary: lucide`,
pulled through `@import "shadcn/tailwind.css"` (`index.css:3`). Primitives import from
`radix-ui` (the unified package), e.g. `select.tsx:2`.

**Stock registry primitives — 27 files, untouched.** No doc comment, no story/AD/FR
reference, no keeper imports beyond `@/lib/utils` and sibling primitives. Each is
retheme-by-token; the new design should not need to open any of them except to remove the
`bg-black/10` scrim and the `backdrop-blur`:

`alert-dialog` (179 ln), `alert` (73), `avatar` (97), `badge` (45), `button` (67),
`card` (88), `checkbox` (28), `command` (179), `context-menu` (244), `dialog` (141),
`dropdown-menu` (249), `input-group` (143), `input` (19), `kbd` (26), `label` (19),
`popover` (72), `progress` (29), `radio-group` (44), `scroll-area` (53), `separator` (28),
`sheet` (127), `skeleton` (13), `sonner` (45), `switch` (31), `tabs` (80), `textarea` (18),
`tooltip` (51).

**Customised — 1 file.**
- `select.tsx` (221 ln) — a documented behavioural patch (`select.tsx:6-25`): Radix Select
  hard-codes `disableOutsidePointerEvents`, which blinds Tauri's drag-region shim, so
  `SelectContent` is hardened to never let the body lock outlast the open state. Cosmetically
  stock; do not rewrite it, only retheme.

**keeper-authored, living in `ui/` — 4 files.** These are the ones a redesign must actually
read:
- `window-list.tsx` (416 ln, 5 story refs) — the shared virtualiser. Owns
  `DEFAULT_OVERSCAN = 6` (`:65`) and `ASSUMED_VIEWPORT_HEIGHT = 640` (`:71`). Consumed by
  `note-list.tsx`, `files-pane.tsx`, `recordings-pane.tsx`, `document-viewer.tsx`,
  `raw-rendered-view.tsx`, and `editor/gallery-block.ts`.
- `resizable-columns.tsx` (249 ln, 8 story refs) — the draggable column boundary. Writes
  `--keeper-columns` / `--keeper-column-rows` (`:92`, `:95`) onto a grid container
  (`:98-99`). Imports `@/lib/column-widths`.
- `overflow-value.tsx` (220 ln, 3 story refs) — truncation-with-popover.
- `cookie-writer.ts` (34 ln) — persistence helper, no visual surface.

---

## 4. Density and rhythm

### Dominant spacing

Grid is 4px (Tailwind default), and keeper lives at the 4–12px end of it.

- **`gap-*`:** `gap-2` (8px) ×241 dominates, then `gap-1` ×118, `gap-1.5` ×51, `gap-3` ×47,
  `gap-4` ×43, `gap-0.5` ×37, `gap-6` ×10. Half-steps (`0.5`, `1.5`) account for 88 uses —
  keeper genuinely runs on a 2px sub-grid in places.
- **Padding:** `px-2` ×68, `px-3` ×66, `py-1` ×38, `py-1.5` ×34, `py-2` ×32, `px-1` ×23,
  `py-0.5` ×18. The `px-6 py-4` pane header (`approval-pane.tsx:238`, `bridges-pane.tsx:36`)
  is the outlier — Settings already disagrees at `px-4 py-3` (`settings-pane.tsx:31`) and
  the note editor at `px-3 py-1.5` (`note-editor.tsx:744`). **Pane-header padding is the one
  genuinely inconsistent rhythm in the shell.**
- **Radius:** `rounded-full` ×67, `rounded-md` ×59 (7px), `rounded-sm` ×17 (5px),
  `rounded-lg` ×12 (10px), plus three arbitrary `rounded-[14px]` and two `rounded-[2px]`.

### Load-bearing dimensions (a virtualiser or a measured row depends on these)

| value | where | why it is load-bearing |
|---|---|---|
| `NOTE_ROW_HEIGHT = 64` | `notes/note-list.tsx:48` | the virtualiser's row pitch; the row's own `h-16` must equal it — the comment says so |
| `OVERSCAN = 8` | `notes/note-list.tsx:51` | |
| chat row `h-16` (64px) | `chat/chat-row.tsx:262` | fixed-height inbox rows |
| account hue bar `w-[3px]` | `chat/chat-row.tsx:272` | 3px edge bar, `inset-y-0` |
| files tree row ≈32px | `layout/files-pane.tsx:165` doc + `files-pane.test.tsx:907` (`ROW_PX = 32`) | *measured on first mount*, not fixed — prose rows wrap |
| recordings row 68px | `recordings-pane.test.tsx:283` | |
| `SHEET_ROW_HEIGHT = 30` | `viewers/document-viewer.tsx:107` | CSV/sheet virtualiser |
| `SLIDE_ESTIMATE = 96` | `viewers/document-viewer.tsx:106` | |
| `STRUCTURE_ROW_HEIGHT = 22` | `viewers/raw-rendered-view.tsx:129` | |
| `GALLERY_TILE_HEIGHT = 168`, `GALLERY_TILE_WIDTH = 160`, gap 8 | `notes/editor/gallery-block.ts:120/125/128` | windowed gallery grid inside CodeMirror |
| `EMBED_HEIGHT_PX = 384` | `notes/editor/file-embed.ts:81` | |
| `ASSUMED_VIEWPORT_HEIGHT = 640` | `ui/window-list.tsx:71` | jsdom/pre-layout fallback |

### Shell geometry

| dimension | value | where |
|---|---|---|
| sidebar collapsed | `w-12` (48px) | `layout/sidebar-pane.tsx:115` |
| sidebar expanded | `w-[260px]` | `layout/sidebar-pane.tsx:115` |
| folded surface column | `SURFACE_COLUMN_FOLDED_WIDTH = 48` | `layout/surface-column.tsx:63` — deliberately equal to the sidebar rail |
| notes rail | default 240, min 180 | `lib/column-widths.ts:98` |
| note list | default 320, min 240 | `lib/column-widths.ts:103` |
| files tree | default 360, min 220 | `lib/column-widths.ts:111` |
| chat list | default 320, min 240 | `lib/column-widths.ts:115` |
| any column clamp | `MIN_COLUMN_WIDTH = 72`, `MAX_COLUMN_WIDTH = 640` | `lib/column-widths.ts:44/47` |
| resize key step | 8 / 32 px | `lib/column-widths.ts:50/53` |
| detail sheet | `w-[320px] sm:max-w-[320px]` | `layout/app-shell.tsx:342` |
| macOS overlay title band | `h-7` (28px) | `layout/app-shell.tsx:248` — split per column so the sidebar half is `bg-sidebar` (`:255`) |
| capture window chrome | `h-8` (32px) | `capture/capture-window.tsx:187` |
| phone header | `--phone-header: 52px` | `index.css:112` |
| sidebar-drawer breakpoint | 1080px | `layout/app-shell.tsx:150` |
| phone tier | 768px | `layout/app-shell.tsx:272` |
| touch targets | `size-11` ×6, `min-h-11` (44px) | `chat/chat-row.tsx:251` |

Control heights: `h-6` ×24, `h-7` ×12, `h-8` ×10, `h-9` ×5 (the shadcn button default,
`ui/button.tsx:25`), `h-11` ×5. Four button sizes exist in the registry; keeper mostly uses
`xs` (`h-6`) and `sm`.

Only **two breakpoint prefixes** appear in the whole app: `sm:` and `md:`. This is a desktop
app with a phone tier bolted on, not a responsive site.

---

## 5. Where identity can live, and where it costs money

### Free — express here

1. **The splash.** `App.tsx:144-151` and again `:170-177`: a full-screen
   `bg-background text-foreground` div containing *only* `<span className="sr-only">Loading
   keeper</span>`. There is no mark, no wordmark, no colour. Two boot paths, both blank.
   This is the single largest unclaimed surface in the app.
2. **`index.html`.** Title is `Tauri + React + Typescript` (`index.html:15`); favicon is
   `/vite.svg` (`index.html:5`) — **and that file does not exist in the repo** (no `public/`
   directory, no `vite.svg` anywhere). The dev-server tab has a broken icon and a template
   title.
3. **The first-run wizard.** `wizard/first-run-wizard.tsx:151-155`, `:247-251`, `:350-354` —
   three centred `text-2xl font-heading` headings with prose. Full-window, low density,
   nothing virtualised.
4. **Pane headers.** Seven `<h1 className="font-heading font-medium text-lg">` plus a
   `text-sm text-muted-foreground` subtitle, one per surface
   (`approval-pane.tsx:239`, `bridges-pane.tsx:37`, `files-pane.tsx:1807`,
   `recording-pane.tsx:202`, `settings-pane.tsx:32`, `sync-pane.tsx:733`,
   `recordings-pane.tsx:259`). Uniform grammar, one shared type slot.
5. **Empty states.** All prose, all `text-muted-foreground text-sm`, no illustration
   anywhere: `conversation-pane.tsx:1533` "Select a conversation to start reading.",
   `note-editor.tsx:721` "Pick a note, or write a new one.",
   `files-pane.tsx:197` "This folder is empty.", `chat-list-pane.tsx:718-719` (the filter
   empty state with an inline Clear button), plus the Bridges/Archive states. Cheap to give
   character to; nothing measures them.
6. **The sidebar.** `sidebar-pane.tsx:168` — `bg-sidebar` with a right border, and its own
   token family already carved out (8 tokens, only 2 consumed). The one surface with a
   dedicated palette and no design on it.
7. **The capture window chrome.** `capture-window.tsx:187` (`h-8` strip, drag region) and
   `:313` (`h-screen bg-background`) — an undecorated always-on-top window keeper draws
   entirely itself. See the defect in §5.2 below.
8. **The macOS title band.** `app-shell.tsx:247-259` — a 28px `data-tauri-drag-region` strip
   keeper owns because `titleBarStyle`/`hiddenTitle` hide the OS chrome.
9. **The tray glyph set.** 10 monochrome template icons (see §6).

### Expensive — do not put character here

1. **Virtualised rows.** Six surfaces run on `ui/window-list.tsx`. Note rows are pinned at
   exactly 64px by `NOTE_ROW_HEIGHT` (`note-list.tsx:48`) with the row's `h-16` asserted to
   match; chat rows are `h-16`; the gallery grid inside CodeMirror is windowed on a
   168×160 tile. **Any change to row height, row padding, or line-height in these lists is a
   virtualiser change, not a CSS change.** The files tree is the exception — it *measures*
   its rows on first mount (`files-pane.tsx:165-169`) because prose rows wrap.
2. **The CodeMirror editor.** See §5.1 — most of the notes surface is not React.
3. **Dense viewers.** `viewers/document-viewer.tsx` (30px sheet rows, 96px slide estimate,
   four `font-heading` block styles at `:113-116`), `viewers/raw-rendered-view.tsx` (22px
   structure rows). Type changes here move virtualiser geometry.
4. **The chat timeline.** `chat/message-bubble.tsx`, `chat/chat-row.tsx` — fixed 64px rows,
   a 3px hue bar, 3.5px read-receipt chips carrying `text-[7px]` initials. There is no room.
5. **Media/QR plates.** `bridge-login-sheet.tsx:253`, `device-verification-dialog.tsx:147` —
   `bg-white` is a scanning requirement, not a style choice.

### 5.1 How CodeMirror is themed today, and how much of Notes lives inside it

**Two `EditorView.baseTheme` blocks, and nothing else:**

- `notes/editor/live-preview.ts:585-869` — ~285 lines of CSS-in-JS covering every
  live-preview decoration: `.cm-lp-strong/em/strike/underline/sub/sup/code/link/wikilink/
  h1..h6/quote/fence/fence-info/task/image`, the recording embed player, chip, stage, tracks
  and transport. **Every colour is a token**: `var(--muted)` (`:597`, `:619`, `:646`, `:659`),
  `var(--primary)` (`:606`, `:666`), `var(--border)` (`:613`, `:658`),
  `var(--muted-foreground)` (`:615`, `:624`), `var(--font-mono, …)` (`:596`, `:618`, `:663`).
  Zero raw colour.
- `notes/editor/markdown-table.ts:577-618` — the rendered table. Uses `currentColor` +
  `color-mix(in srgb, currentColor 25%, transparent)` for borders (`:586-587`, `:606-607`),
  so it inherits automatically.

**Two real defects the redesign must fix, both grounded:**

1. **The editor's own body type is unthemed.** The CodeMirror host is
   `<div ref={hostRef} className="min-h-0 flex-1 overflow-auto" />` (`note-editor.tsx:939`) —
   no font, no size, no measure, no padding. Nothing in `src/` styles `.cm-scroller`,
   `.cm-content`, `.cm-line`, `.cm-cursor`, `.cm-selectionBackground` or `.cm-activeLine`
   (verified: the only references to those class names in non-test source are one in
   `viewers/text-viewer.tsx` and one in a comment in `editor/recording-transport.ts`). So
   the notes editor renders in CodeMirror's own baseTheme default —
   `.cm-scroller { font-family: monospace; line-height: 1.4 }`
   (`node_modules/@codemirror/view/dist/index.js`). **keeper's markdown editor is monospace
   at the browser default size, by omission.**
2. **The editor has no dark mode.** keeper never installs a theme with `dark: true` and
   never sets `EditorView.darkTheme` (zero matches for `darkTheme` / `dark: true` /
   `EditorView.theme(` in non-test source). CodeMirror resolves `&light` vs `&dark` from
   that facet (`themeClasses` → `baseLightID` when the facet is false), so the `&light`
   rules are permanently active: caret `borderLeft: 1.2px solid black`, selection
   `#d9d9d9` (unfocused) / `#d7d4f0` (focused), active line `#cceeff44`. **In dark mode the
   note editor draws a black caret and a pale-lilac selection over an `oklch(0.145)`
   background.** The pale lilac is also, incidentally, a violet.

**Notes surface split:** `src/components/notes/**` (non-test) is **9624 lines of React**
outside `editor/` and **6541 lines inside `editor/`** — 40% of the Notes code is
CodeMirror extensions. But by *pixels* the editor is the whole document body: the React
part is the rail, the list, the pane header, the panel strip, the attachments/history/
conflict panels and the notices, all of which are chrome around a full-height CM host.
Anything about how a note *reads* — heading scale, measure, code blocks, quotes, tables,
links, embeds — is a change to `live-preview.ts`, not to a stylesheet.

### 5.2 The capture window is light-only

`src/main.tsx:22` wraps the app in `<ThemeProvider attribute="class" defaultTheme="system"
enableSystem disableTransitionOnChange>` from `next-themes` — that is what puts `.dark` on
`<html>`. `src/capture-main.tsx` imports `./index.css` (`:51`) and mounts
`<CapturePanel />` (`:72`) with **no ThemeProvider**, and there is no Rust-side
initialization script setting the class (zero matches for `"dark"` / `classList` /
`init_script` under `src-tauri/crates/keeper/src`). So the quick-capture window renders the
`:root` light palette permanently, regardless of macOS appearance. The redesign should not
paper over this; it is a one-line fix in `capture-main.tsx` and it is the difference between
the new identity appearing on that window or not.

There is also no theme toggle anywhere in the UI — the only `useTheme` consumer is
`ui/sonner.tsx:14`. Appearance follows the OS, full stop.

---

## 6. The current icon

**What the mark is:** a solid rounded-square field in **`#0f6e5c`** — byte-verified from
`icon.png` pixels, and the exact value of `--primary` in `index.css:73` — knocked out with a
**white speech bubble with a tail at the lower-left**. Transparent outside the rounded
square (desktop `icon.png` is RGBA with a transparent corner; the iOS set is deliberately
opaque). The generator says so in its own words: *"a simple, legible centered white
'keep'/messenger mark (a rounded speech bubble with a small tail)"* —
`scripts/gen-ios-icons.swift:5-6`.

### App icons — `src-tauri/crates/keeper/icons/` (24 files)

| file | pixels | referenced by |
|---|---|---|
| `icon.png` | 512×512 | source of truth for the desktop set |
| `icon.icns` | 105 727 B (macOS multi-size) | `tauri.conf.json:71` |
| `icon.ico` | 6 entries: 16, 24, 32, 48, 64, 256 | `tauri.conf.json:72` |
| `32x32.png` | 32×32 | `tauri.conf.json:68` |
| `128x128.png` | 128×128 | `tauri.conf.json:69` |
| `128x128@2x.png` | 256×256 | `tauri.conf.json:70` |
| `Square30x30Logo.png` … `Square310x310Logo.png` (9 files) | 30, 44, 71, 89, 107, 142, 150, 284, 310 | Windows/MSIX; unreferenced by `tauri.conf.json` |
| `StoreLogo.png` | 50×50 | Windows store |

### iOS icons — `src-tauri/crates/keeper/gen/apple/Assets.xcassets/AppIcon.appiconset/`

18 PNGs (`AppIcon-20x20@1x` → `AppIcon-512@2x`, i.e. 20…1024px) plus `Contents.json`.
**Generated**, opaque, verified for exact size and absence of alpha by the script itself
(`gen-ios-icons.swift:16-19`).

### Tray icons — 10 files, all 44×44 RGBA, macOS template images

`tray-idle-template.png`, `tray-error-template.png`, `tray-recording-template.png`, and
seven sync states (`tray-sync-template`, `-up`, `-down`, `-updown`, `-paused`, `-refresh`,
`-warning`). Monochrome + alpha, because `icon_as_template(true)` lets macOS recolour them —
**state must be legible from shape alone** (`scripts/gen-tray-sync-icons.ts:14-18`).

### What is generated vs hand-made — this is the crux

| set | generator | status |
|---|---|---|
| iOS AppIcon (18 files) | `scripts/gen-ios-icons.swift` — dependency-free CoreGraphics, draws the vector mark fresh at every size, `swift scripts/gen-ios-icons.swift` | **fully regenerable**; one new `drawMark()` reproduces all 18 |
| tray sync set (7 files) | `scripts/gen-tray-sync-icons.ts` — `bun run scripts/gen-tray-sync-icons.ts` | **regenerable, but parasitic**: it *reads* `tray-idle-template.png` and "keeps its speech-bubble outline pixel-for-pixel", compositing state marks into the empty interior (`:8-10`, `:38-46`, constants `CENTER_X 21.5 / CENTER_Y 18.5 / RING_RADIUS 6.2 / STROKE 1.6`). Those constants are *measured from the bubble*. A robot-head silhouette has a different interior, so **the geometry constants must be re-measured, not just the source glyph replaced** |
| `tray-idle`, `tray-error`, `tray-recording` | **none** | hand-made binaries. The sync generator's own doc comment says exactly this: *"committed as opaque binaries with no generator, so nobody could produce a matching variant without redrawing the brand mark by hand"* (`gen-tray-sync-icons.ts:5-7`) |
| desktop `icon.png` / `.icns` / `.ico` / `Square*` (15 files) | **none found** | hand-made / one-off `tauri icon` output, not reproducible from anything in the repo |
| favicon | **missing** | `index.html:5` points at `/vite.svg`, which does not exist |

**Regeneration cost for a new mark:** 18 iOS files free (edit one Swift draw function);
7 tray sync files free *after* re-measuring 4 geometry constants; 3 tray glyphs hand-drawn;
15 desktop/Windows rasters must be produced from a new 1024px master (no script exists —
one should be written, mirroring `gen-ios-icons.swift`); 1 favicon to create from nothing.
**Total: 44 raster files, of which 25 are already scripted.**

The brand green is not at risk: `#0f6e5c` is already the icon field, `--primary`, and
`gen-ios-icons.swift:29-31` mirrors it with a comment saying to update both together. A
greenish redesign inherits the mark's colour; only its silhouette changes.

---

## Appendix — the migration, sized

| axis | scattered sites | verdict |
|---|---|---|
| colour | **19** raw utilities + 1 `hsl()`, in 11 files | token swap |
| colour tokens | 55, light+dark parity, 3 dead (`chart-*` ×5, `sidebar-primary`) | edit one file |
| typography | 11 sizes → 5 real ones; 0 self-hosted faces; `--font-heading` is an alias with 21 waiting call sites | one `@font-face` + one token + ~50 size edits |
| shadcn | 27 stock + 1 patched + 4 keeper-authored | retheme centrally; open 4 |
| CodeMirror | 2 baseTheme blocks, both fully tokenised; **but** the editor body font and the dark-mode caret/selection are unthemed | 2 real bugs, ~30 lines |
| geometry | 13 load-bearing constants, 8 shell widths | respect, do not restyle |
| icon | 44 rasters, 25 scripted, 19 hand-made, 1 missing favicon | one new mark → one new draw function + one new desktop generator |
