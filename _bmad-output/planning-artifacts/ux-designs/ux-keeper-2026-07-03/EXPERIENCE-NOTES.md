---
name: keeper
parent: EXPERIENCE.md
status: final
sources:
  - _bmad-output/planning-artifacts/product-inputs-notes-2026-08-02.md
  - _bmad-output/brainstorming/brainstorm-keeper-notes-2026-08-02/brainstorm-intent.md
  - _bmad-output/brainstorming/brainstorm-keeper-notes-2026-08-02/.memlog.md
  - _bmad-output/planning-artifacts/ux-designs/ux-keeper-2026-07-03/EXPERIENCE.md
  - _bmad-output/planning-artifacts/ux-designs/ux-keeper-2026-07-03/DESIGN.md
  - docs/project-context.md
created: 2026-08-02
updated: 2026-08-02
---

# keeper — Experience Spine, Notes extension (Phase 5)

> Desktop only (macOS + Linux), gated behind the `notes` capability flag (FR-122). Extends `EXPERIENCE.md`; everything not restated here behaves exactly as that spine specifies. Paired with `DESIGN-NOTES.md` — token references `{...}` resolve in `DESIGN.md` first and in `DESIGN-NOTES.md` for tokens this phase introduces. Spines win on conflict with any mock. FR/NFR/AD/UX-DR numbers are allocated in `product-inputs-notes-2026-08-02.md` and are referenced, never restated. UX benchmarks: Obsidian (files are the product), make.md (the lens is never a move), Apple Notes (tag chips that filter by intersection), Raycast Notes (a panel that never asks where to save).

## Foundation

Notes is a **top-level view inside the existing three-pane frame** (UX-DR36), not a fourth product. It reuses the frame verbatim — `[sidebar {spacing.sidebar-width}][note list {spacing.chat-list-width}][editor ≥ {spacing.conversation-min-width}]` — and deliberately **does not** claim the toggleable `{spacing.detail-panel-width}` slot: frontmatter properties and backlinks are things you edit *while* editing the body, and putting them in a fourth pane would split one act of writing across two panes and one `⌘I` press. They live inside pane 3.

The architecture contract from `EXPERIENCE.md.Foundation` holds without amendment and is what makes the phase cheap: the UI is a **pure renderer of Rust-owned view models**. The note list, the tag tree, the link graph, the provenance line on a row, the "Saved · 12:04" state word and the tray's five recent-note labels are all one Rust-composed model (AD-55/AD-58), so the tray, the palette and the window can never word the same fact differently. Bodies stream per open note; assets arrive over `keeper-note://` (AD-59); nothing large crosses IPC as JSON.

**Capability gate (FR-122).** `notes` rides the FR-57 handshake, exactly as `recording` does. When it is on, six things appear: the NOTES group in the sidebar, `⌘6`, the notes actions in the registry (and therefore in the palette, the `⌘?` cheat sheet and the native menu — UX-DR42), the tray section, the quick-capture window, and the `keeper-note://` scheme. When it is off — the iOS shell, or a desktop build with no folder sync — all six are **absent, not disabled**: no dead rows, no greyed menu items, no palette entries that answer "unsupported on this platform". The flag is *not* per-vault: a desktop with zero notes-flagged folders still has the capability, and answers with the no-vault states in `Failure and edge states`.

Vaults are flagged sync profiles (FR-94, AD-54). There is no vault picker, no path field, no "import vault" flow, and no migration surface anywhere in this document — the absence is the design, and reintroducing any of them is a regression.

## Surfaces

| Surface | Reached from | Purpose |
|---|---|---|
| Quick-capture panel | Global hotkey (default `⌃⌥N`) · tray → Quick Capture · `⌘⌥K` in-app | Catch a thought without leaving the app you are in; the only surface that works with no vault configured (FR-101) |
| Tray notes section | Menu bar / system tray | New Note, Today's Journal, five recent notes, the unread indicator — the app window is optional for a whole day (FR-102) |
| Notes view | Sidebar NOTES group · `⌘6` · `⌘K` | The three-pane home of notes: filters, list, editor (FR-103–111) |
| Table lens · Board lens | Filter chip bar · `2` / `3` on the focused list · `⌘K` | The same filtered note set as a frontmatter table or a grouped board (FR-123) |
| Sticky note window | `⌘⇧T` · editor tear-off glyph · `⌘K` | A torn-off always-on-top note; several at once (FR-124) |
| Note history / diff | `⌘⇧D` · row context menu · the diff bar's Show changes | Per-revision history with provenance and a diff (FR-113/FR-114) |
| Conflict resolution | A conflict row in the note list | Two surviving bodies reconciled inside the editor (FR-116) |
| Vault settings | Settings → Sync → folder card · vault switcher → Vault settings… | The notes flag and the four per-vault knobs (FR-94/FR-120) |

Modal discipline is unchanged and tightened by UX-DR35: **nothing in this phase is a dialog.** Not capture, not filing, not conflict resolution, not restore-a-version. The only `AlertDialog` the phase may raise is the existing FR-121-adjacent one already owned by Settings → Sync (removing a folder).

### Quick-capture panel (FR-101, AD-60, NFR-27)

**The window.** A statically declared second window (`quick-capture`), created hidden at process start with its textarea already focused, so `show()` never has to focus anything and the first keystroke is never dropped (NFR-27). Undecorated, always-on-top, skip-taskbar, `{rounded.lg}`, 1 px `border`, one shadow (it is a transient layer, per `DESIGN.md.Elevation & Depth`). `{spacing.capture-width}` wide; `{spacing.capture-height-min}` tall, growing with the text to `{spacing.capture-height-max}` and then scrolling. Horizontally centred on the display holding the pointer, top edge at `{spacing.capture-top-offset}` of that display's work area — above centre, so the panel lands where the eye already is and does not cover the middle of the thing being read.

**What is on it.** Two things.

1. The textarea. Plain, `{typography.prose}`, full-bleed inside `{spacing.capture-padding}`, no border of its own, caret at the restored offset.
2. A single `{spacing.capture-footer-height}` footer line: the **destination chip** at the leading edge ("Today's journal" / "Inbox — 2026-08-02-dropped-a-thought.md"), the **vault name** centred in `caption` when more than one vault exists (absent when there is exactly one — a label that is always the same value is noise), and `Esc saves` in `caption` at the trailing edge.

**What is deliberately not on it,** and must not be added: a title field, a folder picker, a tag picker control, a template picker, a save button, a formatting toolbar, a close button, window chrome, a preview toggle, a note list, a search field, a character count, a "cancel" of any kind. Tags and links are *typed* (`#`, `[[`, `/` all work here — see `Interaction grammar`); everything else is retro-filing, which happens after the words exist.

**The save-on-Escape contract.** `Esc` writes the buffer to the active vault and hides the panel — in that order, and the hide waits on the shell's write acknowledgement, never on optimism. Three branches, all specified:

- **Write acked** → the panel hides, the buffer is cleared *after* the ack, and the note exists on disk. No toast, no confirmation, no sound. (Confirmation is the file; NFR-30 is what makes that honest.)
- **Write failed** (vault folder gone, disk full, read-only volume) → the panel **does not hide**. It grows by one line and shows a persistent, non-dismissible error naming the reason and the path, with a single action: `Retry`. The text is untouched. This is the FR-28 persistence rule applied to capture: the one thing capture may never do is swallow words.
- **Buffer empty or whitespace-only** → the panel hides and writes nothing. keeper never creates an empty note.

`⌘W` is `Esc`. There is no keystroke, click, or window-manager action anywhere on this panel that discards text.

**The persisted buffer.** Every keystroke is debounced 250 ms into durable registry storage — not into the vault, and not into the index (the index is a disposable cache, AD-57; a capture buffer is not disposable). The buffer therefore survives dismissal, a crash, a force-quit and a reboot, and restores with its exact caret offset. It is cleared by exactly one event: a write acknowledgement.

**Second invocation while text is unsaved.** There is only one buffer and only one panel, so:

- Panel **visible and focused**, hotkey pressed again → the panel **hides and saves**. The summoning key is the dismissing key (the Raycast contract); it is never a second panel and never a no-op.
- Panel **hidden** with a non-empty buffer (the previous write failed, or the process died before `Esc`) → the panel reopens showing that exact text with the caret where it was, and, if the last attempt failed, the persistent error line is still there with its `Retry`. It never appends a separator, never starts a fresh note under the old one, and never asks "restore your draft?".
- Losing focus does **not** hide the panel. You often summon it to copy something out of the window behind it. It hides on `Esc`, on the hotkey, on `⌘W`, or on `⌘Enter` (which hides it *and* opens the written note in the main window).

**When no vault exists yet.** The panel opens, focuses and accepts text exactly as always — blocking the first keystroke on configuration is precisely the failure UX-DR35 exists to forbid. The destination chip reads **"No vault yet — this text is kept here"**, and the footer's trailing slot swaps `Esc saves` for the single action **"Choose a folder to keep notes in"**, which opens the main window at Settings → Sync with the notes flag pre-armed on the add-folder form. `Esc` in this state hides the panel and keeps the buffer durable (it is the only case in the product where `Esc` does not produce a file, and the chip says so before you type a word). The moment a vault is flagged, the next `Esc` writes the accumulated buffer into it as an ordinary note.

### Tray notes section (FR-102, AD-61, UX-DR43)

The tray is the primary surface for this phase, not a convenience mirror. Every item below exists in the **menu built at tray creation** and never in a replacement menu — a Linux tray menu cannot be swapped after it is set, so labels mutate on retained item handles and slot count is fixed forever (AD-61). Item order, top to bottom, inserted **above** the existing sync/recording status block and Show keeper / Quit:

1. **New Note** — creates and opens an empty note in the main window, raising it.
2. **Quick Capture** — shows the panel. Its accelerator renders beside it when the global hotkey is registered; when registration failed it renders the label alone plus the `caption`-styled suffix "— hotkey unavailable" (see `Responsive and platform`).
3. **Today's Journal** — opens or creates today's entry (FR-99).
4. *separator*
5. **Recent** — five fixed slots, newest first, each labelled with the note title truncated to 40 characters. A slot with no note is disabled and labelled `—`. Five slots exist from the first build whether or not five notes do.
6. *separator*
7. **The unread indicator item** — enabled and labelled "3 notes changed by your agent" when the count is non-zero (singular "1 note changed by your agent"), disabled and labelled "No unread note changes" when it is zero. Selecting it opens the Notes view with the origin chip active. This item is the *text* half of the indicator; the glyph dot is the other half, and neither is sufficient alone on Linux (UX-DR43).
8. *separator*

The glyph itself gains the unread dot per `{components.tray-notes}`. Precedence when several states compete: **recording > fault > sync activity > notes unread > idle**, and the notes dot composites onto the idle and sync glyphs but never onto the recording or error glyph — a live capture or a fault outranks "there is something to read".

The whole section is absent when the `notes` capability is off, and — because the menu is built once — absent for the process lifetime. Toggling a vault's notes flag at runtime updates labels and enablement, never structure.

### Notes view — pane 1 (sidebar)

A **NOTES** group beneath the existing primary views. Its rows are filters, never navigations (UX-DR41): selecting one changes what pane 2 lists and leaves the note in pane 3 open.

- **Vault switcher** — the group's first row, rendered with the account switcher's component and affordance verbatim (UX-DR36): a mark, the vault name, a sync-state glyph, and a `DropdownMenu` carrying every vault, `Vault settings…`, and — always last, never gated by count — `Add a notes vault…` (which opens Settings → Sync's add-folder form with the flag pre-armed). Switching is a filter change (FR-95): pane 2 re-lists, pane 3 keeps whatever it had if that note belongs to the new vault and otherwise shows the vault's empty state. Never a reload, never a spinner over the frame. The switcher renders whenever the capability is on and at least one vault exists — including outside the Notes view, because the tray, the palette and capture are all vault-scoped and the active vault must be legible and changeable without entering the view.
- **Inbox** — notes with no tags, no space membership and no journal date: the honest home of the unfiled.
- **Today** — today's journal entry; opens or creates it (FR-99).
- **Journal** — the journal tree by year then month, counts per node.
- **Pinned** — `pinned: true` in frontmatter (FR-119).
- **SPACES** (`section-label`) — one row per note under `spaces/` (FR-105), each a saved query. A space whose query fails to parse renders with a `bridge-degraded`-tinted dot and the row subtitle "This space's query can't be read" — it is an agent-writable plain note, so a broken one is expected and must not break the sidebar.
- **TAGS** (`section-label`) — the hierarchical tag tree with per-node counts (FR-104), collapsible, its own scroll container, `aria-expanded` per node. Selecting a node adds its tag as a chip; ⇧-selecting adds it to the intersection. Counts are of the *unfiltered* vault, so they never appear to shrink as you filter — a count that changes meaning mid-interaction is a lie.
- **FILES** (`section-label`) — the physical folder tree (FR-106, UX-DR38), collapsed by default, its own scroll container. Selecting a folder sets a folder scope chip. This is the alternative lens, and it is always exactly one click away.

Both trees are unbounded, so each pairs `min-h-0 flex-1` with a scroll container (the AD-34-4 rule), and the sidebar footer's account switcher stays reachable at every tree size.

### Notes view — pane 2 (the note list)

The primary surface (UX-DR37). Width `{spacing.chat-list-width}`, resizable ±25 % with persistence, exactly like the chat list.

**Filter chip bar** (`{components.filter-chip-bar}`), pinned at the top, one line that wraps to at most two and then scrolls horizontally. Chips in fixed order so the bar's shape is learnable: **lens** (List / Table / Board) · **scope** (Inbox / Today / Journal / Pinned / space name / folder path) · **tag chips**, one per tag, intersecting (FR-103/104) · **origin** ("Changed by agent") · **date range** · **pinned only**. Every chip except the lens chip is dismissible; `Esc` from the list clears the last-added chip before moving focus, walking the bar down one chip per press. The moment any chip beyond scope is active, a ghost **Save as space** button appears at the trailing end of the bar — `⌘⇧S` — which writes the current filter as a note under `spaces/`, prompting for nothing but the name inline in the row it just created (UX-DR37).

**Search field** directly beneath the bar: an `Input` with a leading magnifier, placeholder "Search this vault". `⌘F` focuses it. Typing runs the bounded parallel scan (FR-118) and tints matches with `{colors.search-highlight}` in the row excerpts. `Esc` in the field clears the query first, then moves focus to the list. A header `caption` states "Searching the files, not an index" — the same posture as the archive search's offline note, and true in the same way.

**Rows** (`{components.note-row}`, 64 px, matching chat-row density). What a row shows, in order:

- A leading `{components.unread-dot}` when the note has been changed by a non-local origin since it was last read (FR-113).
- **Line 1**: the title (weight 600 **only** when unread — the `DESIGN.md` rule that bold means unread holds here unchanged); a pin glyph when pinned; a conflict glyph when conflicted; a right-aligned relative timestamp in `caption`.
- **Line 2**: normally a one-line body excerpt in `caption` with search matches tinted; **when the row is unread it is replaced by the provenance line** — "changed by agent · hesperia · 2 h ago" — because for an unread row the interesting fact is who touched it, not what it says (FR-114/AD-63).
- **Line 2, trailing**: up to three tag chips (`{components.tag-chip}`), overflow rendered as `+2`. Clicking a chip adds it to the filter; it does not open the note.

**Ordering**: conflicted rows first, then pinned (FR-119), then the active sort (default: last modified). Conflicts outrank pins because a conflict is loss in progress and a pin is a preference.

**Empty states**, all of which carry an action so the surface never dead-ends:

| Situation | Copy | Action |
|---|---|---|
| No vault flagged | "No notes vault yet. Flag a folder you already sync and it becomes one." | Open Settings → Sync |
| Vault flagged, no notes | "This vault is empty. Write the first note." | New Note (`⌘⌥N`) |
| Filter matches nothing | "No notes match these filters." | Clear filters (and each chip stays visible for one-tap removal) |
| Search matches nothing | "No matches in this vault." | Clear search; the active chips stay listed |
| Only archived notes match | "Everything here is archived." | Include archived |

**Keyboard**: `↑`/`↓` or `j`/`k` move; `Enter` opens in pane 3; `⌥⌘↓`/`⌥⌘↑` walk **unread** notes (the exact chord and the exact semantic the chat list already uses for unread chats); `e` archive/unarchive; `u` toggle the unread mark; `p` pin/unpin; `1`/`2`/`3` select the List / Table / Board lens; `⌘⇧R` reveals the real path (UX-DR38); `⌘⇧S` saves the filter as a space.

### Notes view — pane 3 (the editor)

Flexes; the prose column is capped at `{spacing.note-measure}` and centred when the pane is wider, the same discipline the timeline already applies with `{spacing.content-max-width}`.

**Header strip**: the title, derived from the first line and never an editable field (FR-98); the real path in `mono` `caption`, shown on hover, on focus, and permanently after `⌘⇧R`; trailing icon actions — history (`⌘⇧D`), tear off (`⌘⇧T`), reveal (`⌘⇧R`), and an overflow `DropdownMenu` (Pin, Archive, Copy wikilink, Copy path, Open in default editor, Delete…).

**Properties panel** (`{components.note-properties-row}`), directly beneath the header, collapsed by default with its state persisted per vault. Frontmatter as typed controls (FR-107): text, number, date, list-of-tags as `{components.tag-chip}` chips with inline add, boolean as a `Switch`, and the ULID `id` as read-only `mono` with a copy affordance (FR-97). Keys keeper does not know render as text and **round-trip byte-identical** — the panel is a lens over YAML, not an owner of it (FR-121). A trailing `+` row adds a property, with completion over keys already used in this vault. A malformed frontmatter block does not render controls: it renders the raw block in `{typography.code}` with a single `caption` line "This note's properties can't be parsed — the text below is untouched", and the body still edits normally.

**Body**: live preview (UX-DR40, FR-107). Every line renders; the line holding the caret reveals its markdown source. Selecting across lines reveals source for the whole selection. Code fences render as `{components.code-block}` — monospace on a muted surface, **no syntax highlighting this phase**, and no colour that implies there is any. ` ```mermaid ` fences render the diagram (`{components.mermaid-block}`, FR-111); putting the caret inside one reveals its source and leaves the last good render above it. Images and embeds resolve over `keeper-note://` (FR-110, AD-59). Wikilinks render as links, resolve on click and on `Enter` when focused, and offer create-on-Enter for a missing target (FR-108); links to files elsewhere in the same synced folder open or reveal them (FR-109).

**Backlinks**, at the foot: a `section-label` "LINKED FROM (n)" list of rows — source note title plus the referencing line in `caption` (FR-108). Derived, never stored. **Hidden entirely when the count is zero** — an empty section that never fills is furniture.

**Diff bar** (`{components.diff-bar}`), pinned directly under the header whenever an external write arrived that the buffer could not silently absorb (FR-112/113). One line: "Changed by agent · 3 additions, 1 removal" plus `Show changes`, `Accept`, and — only when hunks overlapped — `Resolve`. It is persistent, non-modal, and **never steals focus**: your caret does not move, your selection survives, and the bar appears without shifting the text under the caret (it reserves its own height at the top of the pane). `Accept` clears the unread mark (FR-113) and dismisses the bar. Dismissing it is not an option; it is cleared by accepting or by the note becoming clean again.

### Table lens and Board lens (FR-123)

Both are **full-width lenses**: they take the pane-2 + pane-3 area as one surface. The filter chip bar and the search field ride along at the top, unchanged, because a lens is a rendering of the same filtered set, never a different query. Selection is preserved across every lens switch, and `Enter` (or clicking the title cell / card title) returns to the List lens with that note open in pane 3 — that is how UX-DR41's "the note under the cursor survives" is honoured when the editor is not on screen.

**Table** (`{components.table-lens}`). Columns are frontmatter fields. The first column is always **Title** and is frozen. A `+` in the header row opens a column picker listing every key present in the filtered set with its occurrence count, so you add a column by recognising it, not by typing it. Rows are `{spacing.table-row-height}`. Cells are inline-editable for scalar kinds and write frontmatter on `Enter` or blur; list and object kinds render read-only with a `caption` "list value — edit in the note". A key whose values are of mixed kinds across the set renders read-only and says "mixed types". Header click sorts, tri-state asc → desc → none. An absent value renders as an em dash, never as an empty cell — a blank cell reads as a broken table. The unread dot and the conflict glyph ride in the Title cell.

**Board** (`{components.board-lens}`). A group-by picker in the chip bar accepts any single-valued frontmatter field; a multi-valued field (like `tags`) is refused inline with "tags can hold several values — a board needs one per note". Columns are the field's distinct values in first-seen order, `{spacing.board-column-width}` wide, each header carrying its count, plus a trailing **No value** column that is never hidden and never collapsible — the notes that have not been triaged are the point. Cards show list line 1 plus up to three tag chips. Dragging a card between columns writes the field into that note's frontmatter; `⌘⌥←`/`⌘⌥→` does the same for the focused card, so the drag is never the only path. Columns are derived: there is no "add column" that persists, and typing a new value into the group-by picker creates a column that exists only once a card lands in it.

### Sticky window (FR-124)

Torn off with `⌘⇧T`, the editor's tear-off glyph, or the registry action. Default `{spacing.sticky-default}`, minimum `{spacing.sticky-min}`, always-on-top, undecorated, with a `{spacing.sticky-title-strip}` drag strip carrying the truncated title, an always-on-top toggle, and a close button. Several may live at once, one window per note; tearing off a note that already has a sticky focuses the existing one rather than opening a second.

Content is the same live-preview editor and nothing else — **no properties panel, no backlinks, no diff bar, no history**. A sticky is where you left something, not where you review. External-write state still reaches it: the title strip gains the unread dot and a `{colors.agent}` 3 px leading edge on the strip, and clicking either focuses the main window at that note's diff. Closing a sticky saves, because there is no save button anywhere in the product.

Open stickies persist across restart — note id and geometry — and are restored on launch. A sticky whose note has vanished restores into the missing-note state (see `Failure and edge states`) and stays open; it is never silently closed, because a silently closed sticky is indistinguishable from a note that was never important.

### Note history and diff (FR-114, AD-63)

Opened with `⌘⇧D`, the row context menu, or the diff bar's `Show changes`. It renders **in pane 3, in place of the editor** — not a dialog: you read history while scanning the list, and a dialog would cover the list and force a decision. A leading `Back to editor` returns, and so does `Esc`.

Layout: a revision rail on the leading side (newest first) and a diff on the trailing side. Each revision row carries the relative time, the device and origin projected from the sync engine's commit trailers, an agent marker when the origin is non-local, and the commit subject's first line. The diff is unified, per hunk, with `+`/`−` gutter marks and `{colors.diff-add}` / `{colors.diff-remove}` line washes; changed spans inside a line take the same tint at higher alpha. **Side-by-side is not built this phase** and its absence is deliberate: pane 3 is not wide enough for two 68-character columns, and a cramped side-by-side is worse than a good unified diff.

Per-revision actions: `Copy this version` and `Restore this version`. Restore writes a **new** revision and never rewrites history; its inline confirm says so — "Restoring writes a new version. Nothing is lost." (NFR-30).

Empty: a note whose first commit has not happened yet shows "No versions yet — the first one is written when this vault next commits", followed by the vault's cadence in words ("about 2 seconds after you stop typing").

### Conflict row and its resolution (FR-116, NFR-30)

**The row.** A conflict is a first-class row in the note list, sorted above pins, with a conflict glyph, a `destructive` 3 px leading edge, and a second line that names the two sides: "Two versions — from hesperia and this Mac". It is never litter to find on disk, and the Syncthing-shaped conflict copy is never the user's problem.

**The resolution**, in the editor, never in a modal. Opening a conflict row puts pane 3 in **conflict mode**: the body is read-only and renders as a sequence of blocks. Blocks the two sides agree on render once, plain. Blocks that differ render as a stacked pair — "This Mac" above, "hesperia · 14:02" below, each in its `{components.conflict-block}` frame with a `Keep` action, and the pair carrying `Keep both`. A footer bar reads "2 of 5 resolved" and holds the primary `Finish`, disabled until every block is resolved.

Keyboard: `⌥⌘↓`/`⌥⌘↑` walk the unresolved blocks; `⌘Enter` keeps the focused side; `⌘⌫` keeps the other; `b` keeps both. That is the Approval Pane's grammar (`⌘Enter` commits, `⌘⌫` discards) applied to the other place in the product where two candidate texts wait on a human.

`Finish` writes the resolved body as one new revision and **only then** deletes the conflict copy. `Esc` abandons: nothing is written, the conflict copy stays, the row stays. There is no deadline, no auto-resolution, and no path by which either side is dropped without the user choosing it.

### Vault settings (FR-120, FR-94)

Vault settings live **inside the folder's existing card in Settings → Sync** — not in a new settings section, not in a Notes-specific preferences pane. The card gains a `Notes vault` `Switch`; that switch *is* FR-94, and flagging is the only configuration a vault requires (AD-54). Switching it on reveals, in the same card:

- **Vault subfolder** — text, default `notes/`, with a live `caption` showing the resolved absolute path.
- **Journal path template** — text, with a live example beneath it ("today: journal/2026/2026-08-02.md").
- **Default template** — a `Select` over `templates/*.md` in this vault, plus "None"; empty vault → the control is present with only "None" and a `caption` "Put a `.md` file in templates/ and it appears here".
- **Capture destination** — a `RadioGroup`: Today's journal / Dated inbox note.
- **Sync cadence** — idle-commit seconds and push interval, with the on-by-default statement in words (FR-115, AD-62).

The card also carries a rendered `caption` list — three lines, not a docs link, in the egress-list posture: "`.obsidian/` is never read or written" · "`.keeper/` holds the index cache and is added to this folder's ignore rules, so it never syncs" · "keeper never moves a file you did not ask it to move" (FR-121).

The Notes view reaches the same place through the vault switcher's `Vault settings…`, which opens Settings → Sync scrolled to that card with the notes group expanded. **One editor, two doors.** A second copy of these fields anywhere in the Notes view would be a drift surface, which is the same reason `EXPERIENCE.md` mirrors rather than duplicates Settings → Recording.

## Interaction grammar

**Collision analysis.** The existing set is: the global `⌃⌥Space` summon and an unset global recording hotkey; `⌘K`, `⌘?`; `⌘1`–`⌘4` for Inbox / Archive / Approval / Bridges and `⌘5` for the capability-gated Recording view (`⌘6`–`⌘9` are unassigned, and the Sync view carries no numeric accelerator); `⌘⇧F`, `⌘F`, `⌘,`, `⌘I`, `⌘N`, `⌘W`, `⌘Q`; `⌃Tab`/`⌃⇧Tab`, `⌥⌘↓`/`⌥⌘↑`; `↑`/`↓`/`j`/`k`/`Enter`/`Esc`; the chat-list single keys `e u p f m`; the composer/timeline keys `Enter`, `⇧Enter`, `⌘⇧Z`, `↑`, `r`, `e`, `⌫`, `⌘⇧I`; the Approval Pane's `j k Enter ⌘Enter ⌘⌫`. Three rules resolve everything this phase needs:

1. **Numeric accelerators bind to registry action ids, not to sidebar ordinal position** — `⌘5` already survives Recording being absent. So Notes takes **`⌘6`**, the first free number, and keeps it whether or not Recording and Sync render.
2. **`⌘N` is not touched.** New Chat keeps it. Every notes verb lives in a new **`⌘⌥` cluster** — a modifier pair the app has never used — so nothing existing is re-bound and the cluster is learnable as one thing.
3. **Single-key verbs and `⌥⌘↓`/`⌥⌘↑` are list-scoped and reused deliberately.** `e`, `u`, `p` and the unread walk mean the same *thing* in the note list that they mean in the chat list (archive, toggle unread, pin, walk what needs attention) over a disjoint focus scope. That is reuse, not collision, and it is the reason a chat-list user needs no new muscle memory. `f` (favorite) and `m` (mute) have no notes meaning and stay **unbound** in the note list — binding a familiar key to an unfamiliar verb is worse than leaving it silent.

**Global (system-wide)**
- **Quick Capture** — a third configurable global hotkey, default `⌃⌥N`, alongside the existing summon and recording hotkeys, conflict-checked at assignment exactly as they are (FR-101). Pressing it while the panel is visible hides and saves.

**Navigation and verbs (app-wide, capability-gated)**
- `⌘6` Notes view · `⌘⌥N` New Note · `⌘⌥K` Quick Capture (the in-app twin of the global hotkey — always available even when the global registration failed) · `⌘⌥J` Today's Journal · `⌘⌥V` Switch Vault (opens the switcher menu) · `⌘O` Open Note… (the palette scoped to notes)

**Note list (focused)**
- `↑`/`↓`/`j`/`k` move · `Enter` open · `⌥⌘↓`/`⌥⌘↑` next / previous **unread** note
- `e` archive/unarchive · `u` toggle unread · `p` pin/unpin
- `1`/`2`/`3` List / Table / Board lens · `⌘⇧S` save filter as a space · `⌘⇧R` reveal real path
- `⌘F` focus the search field · `Esc` walks up: search query → last chip → … → first chip → list focus

**Editor**
- `⌘S` **force-flush** — commit and push now. There is no save button and no save semantic to attach it to, but people press `⌘S`, and a bound key that does something honest beats a no-op that teaches distrust. It sets the state word to "Saved" and never opens anything.
- `⌘⇧E` show source for the whole note (an escape hatch, registry- and menu-only — never a toolbar toggle, UX-DR40) · `⌘⇧D` history and diff · `⌘⇧T` tear off a sticky · `⌘⇧B` focus backlinks · `⌘⇧P` focus the properties panel · `⌘⇧R` reveal real path
- `⌥⌘↓`/`⌥⌘↑` next / previous **change** while a diff bar or conflict is present · `⌘Enter` accept / keep the focused side · `⌘⌫` reject / keep the other side · `b` keep both (conflict mode only)
- `⌘⇧V` paste as plain text · `Esc` walks up: autocomplete popup → source reveal → editor → note list

**Quick-capture panel**
- `Esc` save and hide · `⌘W` identical · the global hotkey toggles · `⌘Enter` save, hide, and open the note in the main window · `Tab` move to the destination chip (retro-file without leaving the panel) · `⌘⌥J` retarget this capture to today's journal · `⇧Tab` back to the text

**Board lens**
- `←`/`→`/`↑`/`↓` move the focus between cards · `⌘⌥←`/`⌘⌥→` move the focused **card** between columns · `Enter` open in List lens

**Inline triggers** (editor, capture panel, and sticky alike — one grammar everywhere text is typed):

| Trigger | Behaviour |
|---|---|
| `#` | Opens the tag tree inline, filtered as you type, hierarchical (`#work/clients/…` completes segment by segment, FR-104). `Enter` inserts; `Esc` closes the popup and leaves the literal `#` text. A `#` at the start of a line followed by a space is a markdown heading and does **not** trigger — the ambiguity is resolved by position, not by a setting. |
| `[[` | Opens note completion over the active vault, ranked by recency then title (FR-108). `Enter` inserts a wikilink; `Enter` on a query with no match creates the note and links it (create-on-Enter), leaving the caret where it was. `Esc` leaves the literal `[[`. |
| `/` | At the start of an empty line only, opens the command menu: templates (FR-100), today's date, current time, a table skeleton, a code fence, a mermaid fence, a task line. Anywhere else `/` is a slash. |

**Paste**

- An image on the clipboard → written into `attachments/` with a date-slug name and embedded at the caret; no dialog, no picker (FR-110). A name collision appends a counter, the same rule notes use.
- A file path pointing inside the same synced folder → a **relative link**, not a copy (FR-109).
- A URL with a non-empty selection → the selection is wrapped as a markdown link; with no selection, the bare URL.
- HTML → the clipboard's plain-text alternative, verbatim. keeper ships no HTML-to-markdown converter this phase and will not pretend to: a lossy silent conversion into a file the user owns is the wrong kind of clever.
- `⌘⇧V` always pastes plain text and never runs any of the above.

**Drag sources and targets.** Drag is never the only path to anything (the `EXPERIENCE.md` ban stands); every row below names its keyboard twin.

| Drag | Result | Keyboard twin |
|---|---|---|
| Note row → tag tree node | Adds that tag to the note's frontmatter | Type `#tag` in the note, or the row's context menu → Tags ▸ |
| Note row → FILES tree folder | **Moves** the file. The only move keeper ever performs, and only because it was asked (FR-121); the row shows the new path for 2 s afterwards | Context menu → Move to folder… |
| Note row → editor | Inserts a wikilink to that note at the caret | `[[` |
| Note row → SPACES row | Refused with an inline reason when the space's query is not a simple tag or field equality; applied as frontmatter when it is | Context menu → Add to space ▸ |
| Board card → column | Writes the group-by field | `⌘⌥←` / `⌘⌥→` |
| Image file → editor | Written into `attachments/` and embedded (FR-110) | Paste |
| File from the same synced folder → editor | Relative link, no copy (FR-109) | Paste the path |
| FILES tree row → editor | Relative link | Context menu → Copy link, then paste |
| Anything → tag tree structure | **Not a drag target.** Tag hierarchy comes from the text; dragging tags would imply a store that does not exist | — |
| Note row → chat composer | **Not wired this phase.** Notes are local files and a note row carries a local path; dropping one into a message would either leak a path or silently upload a file. Capture-from-chat is the Should-tier inverse and is out of this phase | — |

## Motion and feedback

**The applied external write** (FR-112, NFR-29). When a clean buffer absorbs an external change, the changed lines take a `{components.external-write-highlight}` wash — `{colors.agent}` at 12 % — which holds for 400 ms and fades out over 1.6 s ease-out. Total ≤ 2 s, and it is the only new animation in the phase. Rules that make it safe: it **never scrolls the view** (if the change is off-screen the diff bar's counter increments and offers `Jump to change` instead — a note that scrolls itself under your caret is worse than one that changes quietly); it never moves the caret; and it never blocks typing. Reduced motion: the wash appears as a cut, holds, and is removed by the next keystroke or after 4 s — a cut in, a cut out, no fade.

**The unread dot** (`{components.unread-dot}`, UX-DR39). It appears at full opacity and **never animates** — no fade-in, no pulse. This is a deliberate contrast with the bridge-health dot, which pulses twice on change: a dead bridge is loss in progress and has earned the right to move; an agent writing in your notes is information, and information that twitches trains people to ignore it. The dot appears in three places at once (tray glyph, note row, sidebar count) and clears in one act (`Accept`).

**Save feedback.** There is no save button (UX-DR35), so what the user sees instead is a single `caption` state word (`{components.note-save-state}`), composed in Rust, in the trailing corner of the editor's header strip. It is never a toast and never absent:

- Typing → nothing changes. The word does not flicker on every keystroke.
- A flush that completes inside 400 ms → the word goes straight to **"Saved · 12:04"**. "Saving…" is shown only when a flush actually exceeds 400 ms, so the common case has no intermediate state to read.
- Then it follows sync: **"Synced · 12:04"** / **"Pending push"** / **"Offline — will push when you're back"**.
- A failed write is not this word's job: it is the persistent error state in `Failure and edge states`.
- `⌘S` forces the flush and sets the word immediately.

**What must never animate**, and where each ban comes from:

- The **tray glyph** — no blink, no spinner, no animated dot. The existing sync tray renders motionless states and a moving menu-bar item is a distraction the user cannot dismiss.
- The **note list re-sorting** after a filter or lens change. UX-DR41 says the note under the cursor survives a filter change; an animated re-sort makes the eye lose it even when the selection does not. Filter changes are cuts.
- **Tag chips** appearing and disappearing in the bar; **properties panel** expand/collapse; the **diff bar** appearing (it reserves its height and appears — it does not slide, because sliding would move the text under the caret).
- **Mermaid diagrams** — rendered once, no entrance animation, no re-render animation on edit.
- The **quick-capture panel**. It appears at full opacity with no fade and no scale. A 150 ms entrance is 150 ms of the NFR-27 budget spent on nothing, and the panel's entire promise is that it is already there.
- Anything **celebratory**. Archival calm holds: no confetti on inbox zero, no checkmark animation on Accept, no streak for journaling every day.

## Accessibility

Extends `EXPERIENCE.md.Accessibility Floor`; everything there applies unchanged.

**Quick-capture focus order.** The panel is its own window, so focus is contained by construction. Order: **textarea** (focused before the window is ever shown, which is also how NFR-27 is met) → **destination chip** (a real button; `Enter` cycles Today's journal / Dated inbox) → the footer's trailing action when one exists (`Choose a folder to keep notes in`, or `Retry` in the error state). `Tab` from the last stop wraps to the textarea. The `Esc saves` hint is `aria-describedby` on the textarea, not a focus stop. The panel announces on show: "Quick capture, {destination}, edit text" — one announcement, not one per element. No element in this panel is reachable by pointer only, and none is reachable by keyboard only.

**Screen-reader labelling.**

- **Tag chips** — `role="button"`, `aria-pressed` reflecting whether the tag is in the active intersection, accessible name "Tag work/clients/acme, 12 notes, filter". A chip in a note row (not the filter bar) names itself "Tag work/clients/acme, on this note".
- **Tag tree and Files tree** — `role="tree"` / `treeitem` with `aria-level`, `aria-expanded`, `aria-setsize`, `aria-posinset`. Counts are in the accessible name, not a visually adjacent orphan.
- **Note rows** — "Note, Q3 pricing, unread, changed by agent on hesperia, 2 hours ago, 3 tags" — state before content only where state changes the meaning of the row, which for an unread agent-touched note it does.
- **Diff** — `role="list"` of hunks; each line's accessible name begins with the word **"Added"** or **"Removed"** and the line number. Colour is never the only carrier: the `+` / `−` gutter marks are rendered text, present in both themes and in the accessible name. A diff that reads as "line fourteen, line fifteen" with only a colour wash to distinguish them is a diff no screen-reader user can review, and reviewing is the point of this phase.
- **Conflict blocks** — each side is a labelled group: "Version from this Mac, 3 lines, Keep, button" / "Version from hesperia, 14:02, 3 lines, Keep, button". The footer announces progress on change: "2 of 5 resolved".
- **Properties panel** — every control has a `Label` bound to its frontmatter key; the read-only `id` announces "Note id, read only".
- **The save state word** — an `aria-live="polite"` region that announces transitions, not ticks.

**Live regions.** An applied external write announces politely ("This note changed on disk — 3 lines updated"). A **conflict announces assertively** — it is the loss-risk case for this phase, exactly as bridge health is for the messenger. The unread count on the sidebar's Notes row announces politely when it changes; the tray does not announce at all.

**No pointer-only path.** The phase adds five things that could easily have become pointer-only, and each has a real keyboard or registry route: lens switching (`1`/`2`/`3` + registry), retro-filing in capture (`Tab` to the destination chip), board card moves (`⌘⌥←`/`⌘⌥→`), tearing off a sticky (`⌘⇧T`), and conflict resolution (`⌥⌘↓`, `⌘Enter`, `⌘⌫`, `b`). Both drags in the phase — board cards and note-row-onto-tree — duplicate as context-menu and registry actions. Every notes action is in the registry, so every one of them is in the palette, the `⌘?` cheat sheet and the native menu bar (UX-DR42), which is also what gives macOS full-keyboard-access and VoiceOver users standard discovery.

**Focus management.** Opening a note moves focus to the editor body, not to the properties panel. `Esc` from the editor returns focus to the note row it came from, selection intact. Opening the history surface moves focus to the newest revision row; `Esc` returns to the editor at the caret it left. Tearing off a sticky moves focus into the sticky; closing it returns focus to the main window's editor.

**Reduced motion.** The external-write highlight becomes a cut in and a cut out. Nothing else in this phase moves, so nothing else changes.

## Responsive and platform

**Desktop only this phase.** macOS and Linux, both first-class. There is no phone tier for notes: the `notes` capability is off on iOS (FR-122), so the whole surface is absent rather than projected — and `EXPERIENCE.md`'s phone-tier rules therefore have nothing to say about it. Settings → About's "On this iPhone" list gains one line, in the established posture: "No notes — notes live in a folder your Mac syncs."

**Width tiers.** Unchanged from `EXPERIENCE.md`. The note list holds `{spacing.chat-list-width}` at every tier. Below 1080 px the sidebar collapses to the icon rail and the vault switcher becomes the vault's initial with the full name in its tooltip; the tag and files trees are then reachable from the rail's Notes icon, which opens the group as a `Popover`. The editor's properties panel and backlinks live inside pane 3 and are unaffected by the 1280 px detail-panel rule. Table and Board lenses require the pane-2 + pane-3 area, which the 940 px minimum window always provides.

**macOS.** Unchanged integration: overlay titlebar and traffic-light insets apply to the Notes view like every other; the native menu bar gains a **Notes** menu built from the same registry entries as the palette (UX-DR42); the quick-capture panel is a non-activating always-on-top panel that can appear over a full-screen app; template tray glyphs render per menu-bar appearance.

**Linux deltas (UX-DR43),** each concrete and each an acceptance criterion, not a caveat:

1. **The first tray menu is the only tray menu.** A Linux tray menu cannot be replaced after it is set (AD-61), so all five recent-note slots and the unread indicator item exist from tray creation, disabled and labelled `—` / "No unread note changes" until they have content. Labels mutate on retained handles. No code path calls `set_menu` after creation, and a notes affordance that only exists in a rebuilt menu is a defect, not a platform limitation.
2. **The glyph must be legible on a dark panel.** `icon_as_template` is a macOS-only no-op, so a pure-black-plus-alpha glyph is black-on-black on XFCE or an ayatana panel. Linux therefore ships the `-linux` glyph variants specified in `DESIGN-NOTES.md` — same 44×44 RGBA8 geometry, white ink with a 1 px dark keyline so the mark reads on light and dark panels alike — selected by target, not by runtime theme detection.
3. **Nothing may depend on a tray click.** `TrayIconEvent` clicks and `show_menu_on_left_click` are Linux-unsupported, so the tray *menu* is the only tray interaction. New Note, Quick Capture, Today's Journal, the recent notes and the unread indicator are all menu **items**. There is no click-to-open-capture gesture anywhere, on any platform, because a macOS-only shortcut into the phase's primary surface is exactly what UX-DR43 forbids.
4. **The unread indicator is shape and text, never colour.** The glyph dot carries it visually; the labelled menu item carries it in words. On a panel where the glyph renders badly, the words still work.
5. **Sticky always-on-top is a request, not a guarantee.** Most window managers honour it; some do not. If the WM refuses, the sticky keeps working as an ordinary window and its title strip shows a one-line `caption` — "Always on top isn't available on this desktop" — once per session, in the same honesty posture as the iOS lifecycle copy. keeper does not fight the window manager.

**The Wayland hotkey caveat and its user-visible fallback.** On Wayland there is no X11-style global grab: a global shortcut is compositor-mediated through the XDG GlobalShortcuts portal, which some desktops do not implement and some implement without delivering. This is not hidden and it is not a toast:

- Settings → Shortcuts renders the Quick Capture row with a **live registration state**, re-detected on focus: `Registered`, or the persistent state **"Not available on this desktop — this compositor doesn't hand out global shortcuts."**
- The same row names the two paths that always work: **the tray's Quick Capture item**, and **`⌘⌥K` while keeper is focused**. The in-app twin exists precisely so the global hotkey is never a single point of failure for the phase's headline feature.
- The tray item's label carries the "— hotkey unavailable" suffix in the same state, so the truth is visible where the user is, not only where they configured it.
- The panel's **position** degrades too: an undecorated always-on-top window cannot place itself on Wayland. keeper asks and accepts the compositor's answer rather than fighting it, so on Wayland the panel appears wherever the compositor puts it (typically centred) instead of at `{spacing.capture-top-offset}`. Nothing else about the panel changes, and no copy promises a position it cannot deliver.

## Experience decisions UX-DR35 … UX-DR44

### UX-DR35 — Capture never blocks the first keystroke

No title prompt, no folder prompt, no save button, anywhere in the feature.

The capture job outranks every other job in this phase: a note system whose capture costs two seconds is used, and one that costs eight is not — and every prompt is a decision the user does not yet have the information to make, because they have not written the words. So the panel is created hidden at startup with its textarea already focused; `show()` focuses nothing (NFR-27); the first line becomes the title (FR-98); the destination is a rule, not a question (FR-99/FR-120); and the only exit is one that saves.

What this forbids, permanently: a "new note" dialog; a folder picker; a template chooser before the text exists; a tag field; a save button in the panel, the editor, the sticky, or the table lens; a "discard" affordance; a confirm-on-close. It also forbids the subtler version — a required frontmatter field, or a vault that must be "initialised" before it accepts a note. The one honest exception is the no-vault case, and it is honest precisely because it still takes the keystrokes first and says where they are being kept.

The corollary is `⌘S`. Binding it to a force-flush rather than leaving it dead is not a contradiction of "no save button": the button is absent because saving is not a decision, and the key is bound because reaching for it is a reflex that deserves a truthful answer.

### UX-DR36 — Notes is a top-level view in the existing frame; the vault switcher takes the account switcher's position and affordance

Notes is not a second app inside keeper and must not grow a frame of its own. It gets `⌘6` and the same `[sidebar][list][detail]` shape as the inbox, so the pane the user's eye already uses for "the list of things" holds notes, and the pane it uses for "the thing" holds the editor. That reuse is why the phase adds no layout code and no second navigation model.

The vault switcher is the same decision at the scope level. keeper already has an identity selector with an established affordance — a row with a mark, a name, a state glyph, a per-item `DropdownMenu`, and an add-action that is always last and never gated by count. A vault is the notes scope in exactly the way an Account is the message scope, so it takes that component verbatim rather than inventing a picker. It sits at the head of the NOTES group rather than replacing the account switcher in the footer, because the account switcher is app-global and swapping it per view would make a global control's identity depend on which view is open — the drift UX-DR42 exists to prevent, in spatial form.

### UX-DR37 — The filtered list is primary, the editor secondary; filters are chips; any filter is one keystroke from a space

If the agent writes most of the words, the user's dominant act is finding and reviewing, not typing. The list is therefore the surface that gets the width guarantee, the search field, the chip bar and the single-key verbs; the editor is what happens after the list did its job.

Chips beat a filter panel because they are simultaneously the control and the state: what is filtering you is what is on screen, dismissible in place, in a fixed order so the bar's shape is learnable. And because a chip set *is* a query, promoting one to a saved space costs a name and nothing else — `⌘⇧S`, written as a plain note under `spaces/` so the organisation syncs, diffs and is agent-editable like everything else (FR-105). A filter you can build but not keep trains people not to build filters.

### UX-DR38 — Virtual organisation is the default lens; the physical tree is one click away and any row reveals its real path

Nothing is filed at creation (UX-DR35), so the virtual lens is not a nicety — it is the only organisation that exists. Tags, spaces and frontmatter are therefore the default way to see the vault.

The failure mode of every virtual-folder system is that users stop believing they know where their files are, and then stop trusting the tool with their files. The two antidotes are cheap and both are mandatory: the FILES tree is a permanent sidebar group, one click, no setting; and every row, in every lens, reveals its real path (`⌘⇧R`, context menu, and the editor header on hover/focus). A virtual row that cannot tell you its path is a row the user cannot verify, and this whole product is built on the claim that the files are yours.

### UX-DR39 — An agent change is never silent: a dot on the tray glyph, an unread mark on the row, a diff in the editor

This is the phase's headline, and it is three surfaces on purpose, at three distances. The tray dot is visible when keeper's window is buried — the only place the state can reach you while you work elsewhere. The row mark is visible when you are scanning. The diff is visible when you are deciding. Each answers a different question ("is there anything?", "which ones?", "what exactly?"), so none is redundant with another.

`Accept` is what clears the mark (FR-113) — not opening the note, not scrolling past it. Read-on-open would make the mark clear itself the first time you glance at the wrong note, which is how unread state becomes noise. And the mark persists across restart, because it is derived from commit provenance (AD-63), not from a session flag.

The ban this decision carries: no silent agent write anywhere, no "changes applied" toast as the only carrier, and no bulk "mark all read" in the tray. Accepting is per-note because reviewing is per-note.

### UX-DR40 — Live preview is the only editing mode; source appears on the active line; there is no preview toggle as the primary affordance

A preview toggle asks the user to maintain two mental models of one document and to keep pressing a button to move between them. Live preview collapses them: the document you read is the document you edit, and the source is revealed exactly where you are working — which is also the only place you need it.

This is what lets the editor be honest about markdown without teaching markdown: a heading is a heading until your caret enters it, and then it is `## a heading`. It is also what makes mermaid feel like a paragraph you type (FR-111): the diagram is the normal rendering, the fence appears when you are inside it.

The escape hatch exists but is deliberately not a button: `⌘⇧E` shows source for the whole note, from the palette, the cheat sheet and the menu (UX-DR42). Making it a toolbar toggle would recreate the two-mode model the decision rejects, and toolbars are the mechanism by which "one clean mode" becomes "seven".

### UX-DR41 — A lens or filter change is a filter, never a navigation: the note under the cursor survives it

Every make.md-shaped system that treats a view switch as a navigation loses the user's place, and losing your place is the cost that stops people from exploring their own organisation. So switching lens, adding a tag chip, switching vault, or picking a space changes only what is *listed*: selection is preserved, the open note stays open, scroll position in the list is preserved per scope, and no lens change ever triggers a loading state over the frame.

The concrete consequences are specified rather than left to interpretation: a lens switch preserves selection even when the editor is not on screen (Table and Board are full-width, and `Enter` returns to List with that note open); a vault switch keeps the open note if it belongs to the new vault and shows the vault's empty state if it does not; and the list re-sorts by cut, never by animation, because an animated re-sort loses the eye even when the code preserved the selection.

### UX-DR42 — Notes actions are declared once in the action registry

The UX-DR15 rule, applied to notes. New Note, Quick Capture, Today's Journal, Open Note…, Search Notes, Switch Vault and every verb in `Interaction grammar` are registry entries (FR-117), so the palette, the `⌘?` cheat sheet and the native menu are projections of one list and cannot drift.

The reason this matters more here than elsewhere: notes is the first feature whose *primary* surface is the tray, and the tray's labels come from the same composed model. Three surfaces wording the same action three ways would be three different products. The capability flag composes with it for free — a registry entry gated on `notes` disappears from the palette, the cheat sheet and the menu in one act, exactly as the recording verbs already do.

### UX-DR43 — Linux parity is a first-class acceptance criterion

The tray is the primary surface for this phase, and keeper's tray was built macOS-first: template glyphs that are invisible on a dark Linux panel, a replace-the-whole-menu strategy that Linux forbids, and click events Linux does not deliver. Building notes on top of that would make the phase's headline feature a macOS feature with a Linux stub — for a user who works across a Linux box and a Mac, which is the stated case.

So parity is spelled out as testable criteria rather than intent: every tray-reachable notes affordance exists in the **first** menu; every glyph has a Linux-visible variant with the geometry `DESIGN-NOTES.md` fixes; no affordance depends on a tray click; the unread indicator is carried by shape *and* text; and the Wayland hotkey failure has a visible state and two working alternatives. Each is a thing a reviewer can check on a machine, which is what "first-class" has to mean to survive a schedule.

### UX-DR44 — Mermaid diagrams and images degrade to their source text, never to an empty box

An empty box is the worst possible failure for a file-backed product: it tells the user their content is gone when the file is intact. Both renderers therefore fail *downward into the source*, never into nothing.

A mermaid diagram that will not parse renders its fence as a `{typography.code}` block with the parser's own message on one line above it and the caret-position hint the parser gives, so the diagram is editable in place and the error is actionable — the same "render the underlying tool's error verbatim" posture the bridge login stepper already takes. A previously good render is kept on screen while you type a broken intermediate state, so a diagram does not flash away between keystrokes.

A missing image renders its markdown source plus the path it looked for, with a `Locate…` action — the file may simply not have synced yet, and "not here yet" is a different fact from "broken", stated as such. The rule generalises: any renderer keeper adds later inherits it.

## Failure and edge states

Additive to `EXPERIENCE.md.State Patterns`. Everything not listed behaves as specified there. Every state below is persistent while it is true — none is a toast, and none blocks the vault's other notes from being read.

| State | Surface | Treatment |
|---|---|---|
| No vault configured | Notes view, capture panel, tray | Notes view: "No notes vault yet. Flag a folder you already sync and it becomes one." + Open Settings → Sync. Capture: accepts text, chip reads "No vault yet — this text is kept here", buffer stays durable (FR-101). Tray: New Note and Today's Journal are present and, when chosen, open Settings → Sync at the same place — never disabled, because the action is achievable |
| Vault folder missing (profile points at a path that is gone) | Vault switcher, note list, capture | Switcher row takes a `bridge-disconnected` dot; list replaces itself with a persistent card: "This vault's folder isn't there any more — {path}. keeper hasn't deleted anything." + `Locate folder…` / `Stop syncing this folder` (the existing Settings → Sync confirm). Capture writes fail loudly per the panel's error branch, text intact. Nothing in the index is discarded — a missing folder is not evidence the notes are gone |
| Vault on a detached volume | Vault switcher, note list, save state | Distinguished from "missing" because it comes back: switcher shows a paused glyph, the list renders **from the index cache, read-only**, with a persistent banner "This vault is on a volume that isn't mounted — showing the last index. Editing is off until it's back." Cadence backs off instead of retrying per tick; the state clears on remount with a rescan, no user action. Capture still accepts text and holds it in the durable buffer |
| Unreadable note (permissions, IO error) | Note row, editor | The row stays in the list with a warning glyph and the excerpt replaced by "Can't read this file". The editor shows the OS error verbatim, the path in `mono`, and `Retry` / `Reveal`. Never removed from the list — a note that vanishes from the list because it could not be read is indistinguishable from a deleted note |
| Enormous note (> 2 MB) | Editor | Opens read-only in `{typography.code}`, streamed and virtualised, with a persistent line: "This note is 6.2 MB — it's open for reading. Editing very large notes is off so keeper can't lose part of one." Search still matches it; history still works. The threshold is stated in the copy, never a mystery |
| Binary file with a `.md` extension | Note list, editor | Excluded from the index as a note and listed only in the FILES tree, where it opens externally. If it is opened through a stale row: "This file isn't text — keeper won't render it as a note." + `Open in default app` / `Reveal`. keeper never writes to it and never re-encodes it |
| Mermaid parse error | Editor, sticky | The fence renders as `{components.code-block}` with the parser's message above it and the last good render retained while you type (UX-DR44). Never an empty box, never a collapsed block, never a toast |
| Image missing | Editor, sticky | The embed renders its markdown source plus the path it looked for, in `caption`, with `Locate…`. Copy distinguishes not-yet-synced from broken: "Not here yet — this vault is still catching up" while a sync is in flight, "Not found" once it has settled (UX-DR44) |
| Conflict | Note list, editor, sidebar | The conflict row (above pins, destructive edge) and conflict mode in the editor. Announced assertively. Persists until the user resolves it; there is no auto-resolution, no deadline, and the conflict copy is deleted only after the resolved write is acked (NFR-30) |
| Sync offline | Save state word, vault switcher, note list | The state word reads "Offline — will push when you're back"; the switcher takes the offline glyph; the list and editor are fully usable, because notes are local files and offline is the normal case, not an error. No toast, no spam on flapping. Notes written offline commit locally and push on reconnect |
| Index rebuilding (cache absent or corrupt) | Note list | The list paints from disk as the scan progresses with a thin indeterminate bar under the chip bar and the count updating; a corrupt cache is a rescan, never an error message (FR-96, AD-57). Interactive throughout — you can open the first note before the last one is found |
| Vault larger than the scan budget | Search field | Search returns its bounded result set with a persistent `caption`: "Showing the first 200 matches in 10,412 notes — narrow the filters to see more." Never a truncated list that pretends to be complete (NFR-28) |
| Global hotkey unregistered | Settings → Shortcuts, tray | The live state row and the tray label suffix described in `Responsive and platform`. Both alternate paths named in place; never a silent failure |
| Two vaults, one note id | Note list | Ids are per-note and vault-scoped; a duplicate `id` (a copied file) is surfaced on the second row as "Shares an id with {title}" with `Give this note a new id`. keeper does not rewrite frontmatter unasked (FR-121) |
