---
name: keeper
parent: DESIGN.md
description: Visual token delta for the Notes phase (Phase 5). Notes adds no second visual language — it adds four semantic colour roles, two type roles for the editor, the capture panel's spacing, and the notes tray glyph set including its Linux-visible variants. Everything not listed here inherits DESIGN.md, which inherits shadcn.
status: final
created: 2026-08-02
updated: 2026-08-02
colors:
  # Delta only. Every token in DESIGN.md still applies unchanged; nothing below
  # replaces an existing role, and no existing role gains a second meaning.
  # Contrast validated AA in both themes against the solid (non-vibrancy) surfaces.
  #
  # Co-author blue — "someone who is not you wrote here, and you have not read it".
  # The only semantic slot the palette had left: green is kept, amber is
  # written-not-sent, violet is incognito, the red family is disconnection and
  # live capture. Blue signals authorship-by-another and signals nothing else.
  agent: '#1D4ED8'
  agent-foreground: '#FFFFFF'
  agent-dark: '#7AA2F7'
  agent-foreground-dark: '#0A1733'
  # Diff pair. Backgrounds are line washes; foregrounds are the +/- gutter marks
  # and the word-level emphasis. Deliberately NOT bridge-healthy / destructive:
  # a diff is not health and a removed line is not a deletion you performed.
  diff-add: '#DAF3E4'
  diff-add-foreground: '#0B5132'
  diff-add-dark: '#123626'
  diff-add-foreground-dark: '#8FE3B4'
  diff-remove: '#FADCDC'
  diff-remove-foreground: '#7A1B1B'
  diff-remove-dark: '#3B1618'
  diff-remove-foreground-dark: '#F3A9A6'
  # Tag chip surfaces. Neutral by construction: a tag is a name, not a status,
  # and a hue here would collide with every brand meaning within one row.
  tag-chip: '#ECEFF1'
  tag-chip-foreground: '#3F4A52'
  tag-chip-dark: '#23282C'
  tag-chip-foreground-dark: '#B9C2C8'
  tag-chip-active: '#3F4A52'
  tag-chip-active-foreground: '#FFFFFF'
  tag-chip-active-dark: '#C9D2D8'
  tag-chip-active-foreground-dark: '#14181B'
  # Mermaid canvas. A diagram is a figure inside prose, so it needs its own
  # paper — one step off `card` in both themes — and its own ink. Mermaid's
  # theme variables are bound to exactly these three; no fourth hue enters.
  mermaid-canvas: '#F7F8F8'
  mermaid-node: '#FFFFFF'
  mermaid-ink: '#2A3138'
  mermaid-canvas-dark: '#191C1E'
  mermaid-node-dark: '#22272A'
  mermaid-ink-dark: '#CBD3D9'
typography:
  # Delta only. Same macOS system stack as DESIGN.md; the editor is the one place
  # in keeper that renders long-form prose, so it gets a reading size, and the one
  # place that renders code and YAML, so it gets a code size.
  prose:
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif'
    fontSize: 14px
    lineHeight: '1.65'
  code:
    fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace'
    fontSize: 12.5px
    lineHeight: '1.55'
    tabSize: '2'
spacing:
  # Quick-capture panel (FR-101, AD-60).
  capture-width: 620px
  capture-height-min: 216px
  capture-height-max: 420px
  capture-padding: 16px
  capture-footer-height: 28px
  capture-top-offset: '22% of the active display work-area height (macOS/X11); compositor-chosen on Wayland'
  # Editor.
  note-measure: 68ch          # prose column cap inside pane 3, centred when wider
  properties-row-height: 28px
  diff-bar-height: 34px
  # Lenses.
  table-row-height: 32px
  board-column-width: 260px
  # Sticky windows (FR-124).
  sticky-default: '320x380'
  sticky-min: '240x200'
  sticky-title-strip: 28px
components:
  vault-switcher:
    control: 'the account-switcher component verbatim (UX-DR36): mark + vault name + sync-state glyph + DropdownMenu'
    placement: 'first row of the sidebar NOTES group; collapses to the vault initial with a tooltip on the 48px rail'
    menu-tail: '"Vault settings…" then "Add a notes vault…" — always last, never gated by count'
  note-row:
    height: 64px
    radius: '{rounded.md}'
    active-background: 'shadcn sidebar-accent'
    unread-title-weight: '600'
    layout: 'leading unread-dot slot · line 1 title + pin/conflict glyph + right-aligned caption timestamp · line 2 caption excerpt (or the provenance line when unread) + up to 3 trailing tag-chips with a "+N" overflow'
    conflict-variant: '3px leading shadcn destructive edge; sorts above pinned rows'
  unread-dot:
    size: 7px
    fill: '{colors.agent}'
    radius: '{rounded.full}'
    motion: 'none — appears and disappears as a cut, never pulses (the deliberate contrast with bridge-health-dot)'
  tag-chip:
    background: '{colors.tag-chip}'
    foreground: '{colors.tag-chip-foreground}'
    active-background: '{colors.tag-chip-active}'
    active-foreground: '{colors.tag-chip-active-foreground}'
    radius: '{rounded.full}'
    typography: '{typography.caption}'
    height: 18px
    padding-x: 7px
    dismiss: 'trailing 10px glyph on filter-bar chips only; row chips are not dismissible'
  filter-chip-bar:
    placement: 'top of pane 2, above the search field'
    order: 'lens · scope · tag chips · origin · date range · pinned — fixed, so the bar has a learnable shape'
    overflow: 'wraps to at most 2 lines, then scrolls horizontally'
    save-as-space: 'ghost Button at the trailing end, visible whenever any chip beyond scope is active'
  note-properties-row:
    height: '{spacing.properties-row-height}'
    key: '{typography.section-label}'
    value: 'typed control per kind — Input / number Input / date Input / tag-chip list / Switch / read-only mono for the ULID id'
    unparsed: 'raw YAML in {components.code-block} + one caption line; body still editable'
  external-write-highlight:
    fill: '{colors.agent} at 12% alpha'
    timing: 'hold 400ms, fade 1600ms ease-out'
    reduced-motion: 'cut in, hold, cut out on the next keystroke or after 4s'
  diff-bar:
    height: '{spacing.diff-bar-height}'
    placement: 'pinned under the editor header; reserves its own height so text under the caret never shifts'
    edge: '3px leading {colors.agent}'
    content: 'summary line + Show changes / Accept / Resolve (Resolve only on overlapping hunks)'
    radius: '{rounded.md}'
  diff-hunk:
    added: '{colors.diff-add} line wash, "+" gutter in {colors.diff-add-foreground}'
    removed: '{colors.diff-remove} line wash, "−" gutter in {colors.diff-remove-foreground}'
    intraline: 'the same tint at 2x alpha behind the changed span'
    typography: '{typography.code}'
    rule: 'the gutter mark is rendered text in both themes — colour is never the only carrier'
  conflict-block:
    frame: '1px border, {rounded.md}, side label in {typography.section-label} ("This Mac" / "hesperia · 14:02")'
    actions: 'Keep per side, Keep both on the pair'
    progress: 'footer bar "2 of 5 resolved" + primary Finish, disabled until every block is resolved'
  backlinks-list:
    header: '{typography.section-label} "LINKED FROM (n)"'
    row: 'source title in body + referencing line in {typography.caption}'
    empty: 'the whole section is absent at zero'
  code-block:
    background: 'shadcn muted'
    typography: '{typography.code}'
    radius: '{rounded.md}'
    highlighting: 'none this phase — and no colour that implies any'
  mermaid-block:
    canvas: '{colors.mermaid-canvas}'
    node-fill: '{colors.mermaid-node}'
    ink: '{colors.mermaid-ink}'
    theme-binding: 'mermaid background = canvas, primaryColor = node-fill, primaryTextColor / lineColor = ink, primaryBorderColor = ink at 45%'
    radius: '{rounded.md}'
    error: 'the fence rendered as {components.code-block} with the parser message above it; the last good render is kept while typing (UX-DR44)'
  capture-panel:
    width: '{spacing.capture-width}'
    height: '{spacing.capture-height-min} growing to {spacing.capture-height-max}, then scrolls'
    padding: '{spacing.capture-padding}'
    radius: '{rounded.lg}'
    surface: 'shadcn popover + 1px border + one shadow (a transient layer)'
    body: '{typography.prose} textarea, no border of its own'
    footer: '{spacing.capture-footer-height} — destination chip (leading) · vault name in {typography.caption} (centre, only when >1 vault) · "Esc saves" in {typography.caption} (trailing)'
    error: 'one extra line, persistent, shadcn destructive — the panel does not hide while it shows'
  sticky-window:
    size: '{spacing.sticky-default}, min {spacing.sticky-min}'
    title-strip: '{spacing.sticky-title-strip} drag region — truncated title, always-on-top toggle, close'
    unread: 'unread-dot in the strip + 3px leading {colors.agent} edge on the strip'
    body: 'the live-preview editor only — no properties panel, no backlinks, no diff bar'
  table-lens:
    row-height: '{spacing.table-row-height}'
    first-column: 'Title, frozen, carrying the unread dot and conflict glyph'
    empty-cell: 'em dash, never blank'
    header: '{typography.section-label}; click sorts asc → desc → none; trailing "+" opens the column picker'
  board-lens:
    column-width: '{spacing.board-column-width}'
    card: 'note-row line 1 + up to 3 tag-chips, {rounded.md}, 1px border'
    no-value-column: 'always present, never collapsible, always last'
  note-save-state:
    typography: '{typography.caption}'
    placement: 'trailing corner of the editor header strip'
    states: '"Saving…" (only past 400ms) → "Saved · 12:04" → "Synced · 12:04" / "Pending push" / "Offline — will push when you are back"'
    motion: 'none'
  tray-notes:
    unread-mark: 'filled disc, centre (38.0, 38.0), radius 4.0 in the 44x44 grid; ≥3.2px clear of all other ink at 44px (≥1.6px at the 22px downscale), keylines included'
    composites-onto: 'idle + all six sync glyphs — never onto the recording or error glyph'
    precedence: 'recording > fault > sync activity > notes unread > idle'
    linux-variant: 'white ink + 1px dark keyline, same 44x44 RGBA8 geometry, selected by target'
    generator: 'scripts/gen-tray-notes-icons.ts, the gen-tray-sync-icons.ts pipeline extended — no glyph is hand-drawn'
---

## Brand & Style

Notes adds **no fifth brand hue and no second visual language.** Archival calm holds: the note list is the chat list's density, the editor is the timeline's measure discipline, the capture panel is a popover with one shadow. Every treatment below is either an inherited shadcn component, an existing keeper token, or one of the four semantic roles this phase genuinely could not express with what already existed.

The three brand ideas extend cleanly. **Kept** is what a vault *is* — plain files in a folder you already sync — so keeper green needs no new job. **Held** stays exactly one thing (written, not sent) and therefore never touches a note: an unsaved note does not exist in this product. **Honest** does all the new work: the unread dot, the diff, the provenance line and the conflict row are the phase's whole differentiator, and each one is a state made visible rather than a decoration.

## Colors

Four new roles, and the argument for each is that the existing palette could not carry it without giving an existing hue a second meaning.

- **Co-author blue (`{colors.agent}` light / `{colors.agent-dark}` dark)** — "someone who is not you wrote here, and you have not read it." It is worn by exactly four things: the `{components.unread-dot}` on a note row, the diff bar's leading edge, the sticky's unread edge, and the `{components.external-write-highlight}` wash at 12 %. Nothing else, ever. It is blue because blue is the only slot the palette had left — green is kept, amber is written-not-sent, violet is incognito, and the red family is already split between disconnection and live capture — and because authorship-by-another is genuinely a *fifth* semantic, not a shade of any of them. It never appears on chrome, on a button, on text, or on the app icon; `DESIGN.md`'s "network identity lives in badges, not wallpaper" rule applies to agents too.
- **Diff pair (`{colors.diff-add}` / `{colors.diff-remove}` and their foregrounds, both themes)** — line washes with the `+` / `−` gutter marks as the foreground. Deliberately not `bridge-healthy` and `destructive`: a diff is not a health reading, and a removed line is not a destructive action the user is about to take. They are pale enough to read `{typography.code}` on in light mode and dark enough not to glow in dark mode, and the gutter mark is rendered text so the diff survives both colour-blindness and a screen reader.
- **Tag chip surfaces (`{colors.tag-chip}` / `{colors.tag-chip-active}`, both themes)** — neutral by construction. A tag is a name, not a status, and a hue on a chip would collide with three brand meanings inside a single 64 px row. The active (filtering) chip inverts to the resting chip's foreground rather than taking a colour, so "this filter is on" reads as contrast, which is the one signal that works in every theme and at every chip density.
- **Mermaid canvas (`{colors.mermaid-canvas}` / `{colors.mermaid-node}` / `{colors.mermaid-ink}`, both themes)** — a diagram is a figure inside prose and needs its own paper, one step off `card` in each theme. Mermaid's theme variables bind to exactly these three plus alpha derivations of the ink; no diagram may introduce a fourth hue, and mermaid's own default palette is never used. A parse error uses `destructive` for the message line only — the fence itself falls back to `{components.code-block}`.

Everything else inherits. `search-highlight` tints note-list excerpt matches exactly as it tints message matches. `destructive` carries the conflict row's edge, because a conflict is the loss-risk state of this phase. The bridge-health trio carries vault reachability in the switcher, unchanged, because "can keeper reach this thing" is the same question it already answers for bridges and folders.

Avoid: co-author blue on anything that is not an unread agent change; a coloured tag chip; a diff rendered by colour alone; mermaid's stock theme; a per-vault accent hue (vault identity is the switcher's name, not a colour — the "no per-network theming" rule, restated for vaults).

## Typography

Two new roles; the macOS system stack is unchanged and no webfont enters.

- **`prose` (14px / 1.65)** — the editor body, the sticky body, and the quick-capture textarea. One step up from `body` (13px) with a markedly looser line height, because this is the only surface in keeper where the user reads and writes *paragraphs* rather than scans rows and messages. The prose column is capped at `{spacing.note-measure}` (68ch) and centred when pane 3 is wider — the same measure discipline `{spacing.content-max-width}` already applies to the timeline, expressed in characters because a note's line length is a reading constraint, not a layout constant.
- **`code` (12.5px / 1.55, tab-size 2)** — code fences, the raw-frontmatter fallback, diff hunk bodies, and the note-history commit subjects. A hair larger than `mono` (12px) because these are multi-line blocks to be read, not single identifiers to be recognised. No syntax highlighting ships this phase, and no colour is used inside a code block that would imply there is any.

Unchanged and load-bearing: `title` for the editor header and dialog-shaped surfaces, `section-label` for the sidebar's TAGS / FILES / SPACES headers and the properties panel's keys and the backlinks header, `caption` for excerpts, timestamps, provenance lines, the save-state word and every honesty note, `mono` for the real path and the ULID `id`. The rule that **bold in a list means unread and nothing else** carries verbatim into the note list.

## Layout & Spacing

The frame is unchanged: notes reuses `[sidebar {spacing.sidebar-width}][note list {spacing.chat-list-width}][editor ≥ {spacing.conversation-min-width}]` and does not claim the `{spacing.detail-panel-width}` slot. Density stays macOS-utility: 64 px rows, 8 px vertical rhythm inside a row, 12 px pane gutters.

**Quick-capture panel.** `{spacing.capture-width}` × `{spacing.capture-height-min}`, growing with the text to `{spacing.capture-height-max}` and then scrolling; `{spacing.capture-padding}` on all sides of the textarea; a `{spacing.capture-footer-height}` footer line below it. 620 px is roughly 70 characters at `prose`, so the panel's natural line length matches the editor's measure and a capture does not re-wrap when it becomes a note. The panel is horizontally centred on the display holding the pointer with its top edge at `{spacing.capture-top-offset}` — above centre, because that is where the eye already is and it leaves the middle of the screen (the thing being read) uncovered. `{rounded.lg}`, 1 px border, one shadow: it is a transient layer, and `DESIGN.md`'s elevation rule already says transient layers are the only shadowed things in keeper.

**Editor internals.** `{spacing.properties-row-height}` per frontmatter row, `{spacing.diff-bar-height}` for the diff bar — reserved rather than animated in, so text under the caret never shifts.

**Lenses.** `{spacing.table-row-height}` rows (tighter than a note row: a table is scanned in a column, not read as cards), `{spacing.board-column-width}` columns. Both lenses occupy the pane-2 + pane-3 area as one surface and keep the filter chip bar and search field at the top.

**Sticky windows.** `{spacing.sticky-default}`, minimum `{spacing.sticky-min}`, with a `{spacing.sticky-title-strip}` drag strip. The minimum is set by the prose measure degrading gracefully, not by the chrome.

## Elevation & Depth

Unchanged. The Notes view is flat panes with 1 px borders and no inter-pane shadows. The quick-capture panel, the sticky windows, the tag/wikilink/slash autocomplete popups and the vault switcher's menu are the only shadowed surfaces the phase adds, and all four are transient layers under the existing rule. Mermaid canvases and code blocks are **inset by contrast, never by shadow** — a shadowed figure inside prose reads as a card, and a note is not a dashboard.

## Shapes

Unchanged: `{rounded.md}` for note rows, diff bars, mermaid canvases, code blocks and board cards; `{rounded.lg}` for the capture panel; `{rounded.full}` for the unread dot and every tag chip. Tag chips are full-round because they are the same shape-family as pins, avatars and the incognito chip — things that name an identity — while board cards take `{rounded.md}` because they are rows in disguise.

## Components

Every treatment below has its behaviour in `EXPERIENCE-NOTES.md`; only visuals live here. Additions to install via the shadcn CLI when needed: `table`, `resizable`, `toggle-group`, `hover-card`. CodeMirror 6 (MIT) and mermaid (MIT) are the two non-shadcn UI dependencies and are licence-cleared; both are lazily loaded so only the main window and the stickies pay for them — the capture panel is a plain textarea and loads neither.

- **Vault switcher** — `{components.vault-switcher}`: the account switcher's component, unmodified. Vault mark, name, sync-state glyph from the existing bridge-health trio, `DropdownMenu` per vault, with `Vault settings…` and then `Add a notes vault…` always last. On the 48 px rail it becomes the vault's initial with the name in a tooltip.
- **Note row** — `{components.note-row}`: 64 px, matching the chat row so the two lists have one density. Unread sets the title to weight 600 and nothing else in the list is ever bold. The conflict variant takes a 3 px leading `destructive` edge, the same treatment an unhealthy bridge card gets, for the same reason.
- **Unread dot** — `{components.unread-dot}`: 7 px, `{colors.agent}`, full-round, **motionless**. The bridge-health dot pulses twice on change; this one never does. That difference is the design: a dead bridge is loss in progress, an agent write is information.
- **Tag chip** — `{components.tag-chip}`: 18 px tall, `caption`, full-round, neutral resting surface, inverted when active. Filter-bar chips carry a 10 px trailing dismiss glyph; chips inside a note row do not (they filter on click; they do not delete on click).
- **Filter chip bar** — `{components.filter-chip-bar}`: fixed chip order, wraps to two lines then scrolls, with the ghost `Save as space` button at the trailing end whenever the filter is worth keeping.
- **Properties panel** — `{components.note-properties-row}`: a two-column grid of `section-label` keys and typed controls, collapsed by default. Unparsable frontmatter renders raw in `{components.code-block}` with one `caption` line; the body still edits.
- **External-write highlight** — `{components.external-write-highlight}`: `{colors.agent}` at 12 %, hold 400 ms, fade 1.6 s. The only new animation in the phase, and it never scrolls the view.
- **Diff bar** — `{components.diff-bar}`: `{spacing.diff-bar-height}`, 3 px leading `{colors.agent}` edge, summary line plus actions. Persistent, non-modal, never focus-stealing; it reserves its height rather than sliding in.
- **Diff hunk** — `{components.diff-hunk}`: `{colors.diff-add}` / `{colors.diff-remove}` washes with rendered `+` / `−` gutter marks in the paired foregrounds, intraline emphasis at double alpha, body in `{typography.code}`. Unified only — side-by-side is not built this phase because pane 3 cannot hold two 68-character columns honestly.
- **Conflict block** — `{components.conflict-block}`: stacked pair of bordered frames with `section-label` side labels, `Keep` per side and `Keep both` on the pair, and a footer progress bar with a `Finish` that stays disabled until every block is resolved.
- **Backlinks list** — `{components.backlinks-list}`: a `section-label` header with the count, rows of source title plus the referencing line in `caption`, and **no empty state** — at zero the section is absent.
- **Code block** — `{components.code-block}`: shadcn `muted` surface, `{typography.code}`, `{rounded.md}`, no highlighting.
- **Mermaid block** — `{components.mermaid-block}`: `{colors.mermaid-canvas}` paper, `{colors.mermaid-node}` node fills, `{colors.mermaid-ink}` strokes and labels, `{rounded.md}`, no shadow. Mermaid's theme variables are bound to exactly these; its stock palette is never used. On a parse error the block degrades to `{components.code-block}` with the parser's own message above it, and the last good render is kept on screen while the source is mid-edit (UX-DR44).
- **Capture panel** — `{components.capture-panel}`: popover surface, `{rounded.lg}`, one shadow, `prose` textarea, one footer line. It appears at full opacity with no entrance animation. Nothing else is on it, and `DESIGN.md`'s "restyling stock components is against brand discipline" rule has a sibling here: **adding a control to this panel is against brand discipline.**
- **Sticky window** — `{components.sticky-window}`: `{spacing.sticky-default}`, `{spacing.sticky-title-strip}` drag strip, the live-preview editor and nothing else. Unread state rides the strip.
- **Table lens / Board lens** — `{components.table-lens}` / `{components.board-lens}`: frozen Title column, `{spacing.table-row-height}` rows, em dash for an absent value; `{spacing.board-column-width}` columns with a permanent trailing **No value** column and `{rounded.md}` cards.
- **Save-state word** — `{components.note-save-state}`: one `caption` string in the editor header's trailing corner. There is no save button in this design system, and this word is its replacement.

### Tray glyph set for notes (UX-DR43, AD-61, FR-102)

The tray already ships ten glyphs — idle, recording, error, and the seven-strong sync family — all **44×44 RGBA8, monochrome black + alpha, macOS template images**, with a test asserting identical dimensions across every state. Three constraints bind every glyph this phase adds, and they are not negotiable:

1. **44×44 RGBA8, always.** The dimension assertion is a shipped test; a glyph of any other size is a build failure, not a visual choice.
2. **Monochrome + alpha; state reads from SHAPE, never colour.** `icon_as_template(true)` hands recolouring to the system, so colour carries zero information on macOS. A state distinguishable only by hue is a state that does not exist.
3. **No stroke under ~1.5 px at the ~22 px retina downscale** — i.e. no feature thinner than ~3 px in the 44 px grid, and no gap narrower than that between two features. Below that, marks merge into a blob and stop carrying information.

**The notes mark** is an unread **dot**, not a new base glyph: `{components.tray-notes}` places a filled disc at centre **(38.0, 38.0)** with radius **4.0** in the 44 × 44 grid — the bottom-trailing corner, which the shipped bubble mark leaves entirely empty (its ink ends at x 38 / y 31, and its tail runs to the bottom-*leading* corner). Measured against the nearest existing ink at (32, 31), the clearance is 9.22 − 4.0 = 5.22 px, and 3.22 px after both keylines — **1.61 px at the 22 px downscale**, above the floor with margin. Because the dot is an overlay in unused space rather than an interior mark, it composites onto the idle glyph and all six sync glyphs without touching the marks already inside the bubble. It does **not** composite onto the recording or error glyph: a live capture or a fault outranks "there is something to read", and stacking two badges on one 22 px icon is how both become illegible.

**The Linux-visible variant** is required because `icon_as_template` is a macOS-only no-op: shipped as-is, keeper's pure-black glyphs are black-on-black on an XFCE or ayatana panel, and the phase's primary surface would be invisible on half its target platforms. Every glyph in the set therefore has a `-linux` sibling — **white ink (`#FFFFFF`) plus a 1 px dark (`#000000` at 70 %) keyline around the silhouette**, so the mark reads on a dark panel and its keyline keeps it readable on a light one. Still monochrome + alpha, still shape-carried state; the keyline is a legibility device, not information. Geometry is unchanged at 44 × 44 RGBA8, so the dimension test passes for both families. The shipped bubble stroke is 4–5 px in the grid (≈ 2–2.5 px at 22 px) and the keyline adds 1 px *outward* without eating it, landing at ≈ 3–3.5 px downscaled. Selection is by build target, never by runtime theme detection — a tray glyph that changes with the panel theme is a glyph that flickers when the theme does.

**The set, generated not drawn.** `scripts/gen-tray-notes-icons.ts` extends the `gen-tray-sync-icons.ts` pipeline: it reads the shipped glyphs, composites the dot into the corner (carving a transparent moat first, so the keyline pass can never bridge the gap), and emits both families. Nothing is hand-drawn, so brand consistency is mechanical.

| File | Base | Notes dot | Family |
|---|---|---|---|
| `tray-idle-notes-template.png` | idle | yes | macOS template |
| `tray-sync-notes-template.png` | sync armed | yes | macOS template |
| `tray-sync-up-notes-template.png` | sync up | yes | macOS template |
| `tray-sync-down-notes-template.png` | sync down | yes | macOS template |
| `tray-sync-updown-notes-template.png` | sync up+down | yes | macOS template |
| `tray-sync-refresh-notes-template.png` | sync refresh | yes | macOS template |
| `tray-sync-paused-notes-template.png` | sync paused | yes | macOS template |
| `tray-sync-warning-notes-template.png` | sync warning | yes | macOS template |
| `tray-{idle,recording,error,sync,sync-up,sync-down,sync-updown,sync-refresh,sync-paused,sync-warning}-linux.png` | the ten existing | no | Linux, white + keyline |
| `tray-{idle,sync,sync-up,sync-down,sync-updown,sync-refresh,sync-paused,sync-warning}-notes-linux.png` | the eight above | yes | Linux, white + keyline |

Thirty-six files, one generator, zero hand-drawn variants. The glyph is only half the indicator: the first tray menu also carries the unread state **in words** (`EXPERIENCE-NOTES.md`), because a 4 px dot on an unknown panel theme is not a contract you can accept for the phase's headline feature.

## Do's and Don'ts

| Do | Don't |
|---|---|
| Co-author blue only for an unread agent change — dot, diff-bar edge, sticky edge, applied-write wash | Co-author blue on chrome, buttons, links, text, or as a fifth brand hue |
| Diff by wash **and** a rendered `+`/`−` gutter mark | A diff distinguishable only by background colour |
| Neutral tag chips; active state by contrast inversion | A hue on a tag chip, or a per-tag colour palette |
| `prose` 14px at `{spacing.note-measure}` for note bodies | Full-pane-width prose, or a custom reading font |
| One capture panel with a textarea and one footer line | A toolbar, a title field, a picker, or a save button anywhere in the feature |
| Mermaid bound to the three mermaid tokens | Mermaid's stock theme, or a diagram that introduces a fourth hue |
| Degrade a diagram or an image to its source text | An empty box, a collapsed block, or a broken-image glyph |
| Motionless unread dot and motionless tray glyph | A pulsing dot, a blinking tray, or an animated list re-sort |
| 44×44 RGBA8 monochrome + alpha, state from shape, nothing under ~1.5px at 22px | A colour-coded tray state, a hand-drawn variant, or a glyph at any other size |
| Ship the `-linux` white+keyline family and select it by target | Rely on `icon_as_template` off macOS, or detect the panel theme at runtime |
| Render every notes surface only behind the `notes` capability flag | Dead notes rows on a shell without folder sync |
