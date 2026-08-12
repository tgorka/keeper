---
name: keeper
parent: EXPERIENCE.md
status: final
sources:
  - _bmad-output/planning-artifacts/product-inputs-sessions-2026-08-12.md
  - _bmad-output/planning-artifacts/prds/prd-keeper-2026-07-03/phase-7-sessions.md
  - _bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SESSIONS-PHASE7.md
  - _bmad-output/planning-artifacts/ux-designs/ux-keeper-2026-07-03/EXPERIENCE.md
  - _bmad-output/planning-artifacts/ux-designs/ux-keeper-2026-07-03/EXPERIENCE-NOTES.md
  - _bmad-output/planning-artifacts/ux-designs/ux-keeper-2026-07-03/DESIGN.md
created: 2026-08-12
updated: 2026-08-12
---

# keeper — Experience Spine, Sessions extension (Phase 7)

> Desktop only, gated behind the `sessions` capability flag (FR-223). Extends
> `EXPERIENCE.md` and leans on `EXPERIENCE-NOTES.md`; everything not restated here
> behaves exactly as those spines specify. Token references `{...}` resolve in
> `DESIGN.md` / `DESIGN-NOTES.md`. FR/NFR/AD/UX-DR numbers are allocated in
> `product-inputs-sessions-2026-08-12.md` and referenced, never restated. UX
> benchmarks: an airport departure board (status at a glance), a lab notebook (the
> record is written during the work), mission control (one console per stream), the
> zone's own README (the copy source of truth — the UI speaks in its words).

## Foundation

Sessions is a **top-level view inside the existing three-pane frame**, the same claim
notes made (UX-DR36): `[sidebar][session list][detail/editor]`. The unit in pane 2 is a
**session folder**, not a file; pane 3 hosts the session detail, and any file opened
from it is the notes editor or a registry viewer in the same pane (two-at-once splits
as in Files). A user who knows notes already knows every control here (UX-DR92): the
chip filter bar, tag chips, pins, unread dots, origin glyphs, the search field, the
capture chrome — identical components, identical behaviour, sessions-scoped data.

**Capability gate (FR-223).** `sessions` rides the FR-57 handshake. On: the SESSIONS
group in the sidebar, its `⌘digit` slot, the registry actions (palette, cheat sheet,
native menu), the tray items, the capture target. Off: all absent, never disabled. The
flag is not per-root: a desktop with zero flagged folders keeps the capability and
answers with the no-root states below.

**Files are the only truth** shapes every surface: nothing here shows a fact that
`ls`, `cat` and `git log` could not reconstruct (AD-110). There is no progress bar, no
status enum, no session "state" beyond location, frontmatter and mtimes.

## Surfaces

| Surface | Reached from | Purpose |
|---|---|---|
| Sessions board | Sidebar SESSIONS group · `⌘7` · `⌘K` | The status board: filters, search, session rows with two freshness signals (FR-228/229, UX-DR85) |
| Session detail | Row click · quick switcher | Header (title, lineage, tags, pin, freshness pair), tree, README open by default (FR-233) |
| Promote panel | Detail → Promote · palette | Two-column workspace→artifacts truth of the README table (FR-243/244, UX-DR90) |
| Archive checklist | Detail/row → Archive… · palette | The zone's closing checklist, visible and resumable (FR-245, UX-DR87) |
| New session | SESSIONS group + · palette · menu | Title in, folder out; template/pattern preview shows the real tree (FR-238/239, UX-DR88) |
| Log today | `⌘⌥L` · tray · palette · row | Today's dated entry in the target session's Log, caret placed (FR-240) |
| Capture (session target) | Global capture chord with target switched · tray | The existing capture window appending to the current session's log (FR-241) |
| Root settings | Settings → Sync → folder card | The sessions flag + subfolder; skeleton creation behind its own confirmation (FR-222) |

Modal discipline is unchanged: the archive checklist and the new-session sheet are
panels in pane 3, not dialogs; the only `AlertDialog` this phase may raise is session
delete (FR-246/247) — destruction earns a dialog, exactly like note delete.

### The board (FR-228, FR-229, UX-DR85, UX-DR86)

Rows sort pinned-first within status, then by record freshness. A row:

```
[pin] [status glyph] 2026-08-10-keeper            [origin] [unread]
      last log: "0.6.5 shipped; drafting release"  ws ◔2m · rec ●1h
```

- **Status glyph**: active = filled circle in `{color.success}` family when fresh,
  hollow when stale (`is:stale`); archived = the archive box glyph, muted.
- **Two freshness signals** (UX-DR86): `ws` (workspace — the agent is iterating) and
  `rec` (record — something was promoted/written). Two separate marks with relative
  times; never merged. A workspace hotter than five minutes pulses once on update —
  reduced-motion honours the existing rule.
- **Unread** dot and **origin** glyph are the notes row's own components.
- The chip filter bar is the notes bar with the sessions `is:` vocabulary; saved
  session spaces (Active, Archived, Stale, Agent-written) sit in the sidebar group
  exactly as note spaces sit in theirs (FR-231).
- Search parses with the notes grammar; a parse error matches nothing and shows the
  underlined token, verbatim behaviour (FR-229). The *include workspace* toggle
  appears only while a text search is active, off by default, resets with the search
  (FR-230).

### Session detail (FR-233–FR-237, UX-DR89)

Header: title (README H1), lineage breadcrumbs (`← continues · continued-by →` chips,
each navigating — dangling refs render inert, muted; UX-DR89), tag chips, pin toggle,
the freshness pair, and the overflow with Reveal, Copy Path, New like this, Archive…,
Delete….

Body: the session tree — README pinned first and open by default in the editor;
`artifacts/`, `refs/`, `prompts/` as sections; `workspace/` last, visually muted, its
section header carrying the zone's own words: *"scratch — unversioned, dies with the
session"* (FR-237). Files open in the notes editor / registry viewers; `workspace/`
files open read-only with the same sentence as the refusal reason.

*Changes since you last looked* (FR-235) is a row under the header when unread: it
names the files changed since the acknowledged revision with per-file diff links —
the notes history/diff surfaces, session-scoped.

### Promote panel (FR-243/244, UX-DR90)

Two columns, workspace left, artifacts right, one row per Promote-table entry plus a
trailing group of unlisted workspace files. Badges: **stale** (source newer than
target — `{color.warning}`), **missing target** (a promise not kept — loud),
**source gone** (quiet, muted). Row action: Promote / Re-promote; unlisted rows:
Promote… (asks only the target name, prefilled with the source name). Every action
writes the table row in the same act — the panel renders the README's table, it never
owns a second list. A file the stability gate parks shows *"still changing"* with the
gate's own timing, never a spinner.

### Archive checklist (FR-245/246, UX-DR87)

A panel that walks the zone's own steps, in its words, each row check-marked as it
completes:

1. **Promote finals** — the outstanding-promote rows inline (skip = explicit per row);
2. **Warnings** — hot workspace (writes < 10 min), stale promotes, open follow-ups;
   each acknowledgeable inline;
3. **Empty the workspace** — states the count and size it will remove and that
   `.gitkeep` stays; skippable with a warning;
4. **File under `archive/2026/`** — the one unskippable step, last.

A session with an empty table and empty `artifacts/` opens the checklist with a
leading **Delete instead** offer quoting the zone rule (FR-246). A crash mid-run
reopens the checklist at the completed prefix (NFR-38) — the journal is the UI's
state. Sync consequences report through the existing pending/parked affordances on
the row (FR-249); the checklist itself never mentions git.

### New session (FR-238/239, UX-DR88)

One sheet: title field, root switcher (preselected), pattern picker — `_template/`
first, then recent sessions ("New like this" preselects its source). The preview *is*
the actual tree and README skeleton that will be created, rendered from the real
files (UX-DR45's move). Pattern-from-session states plainly what is copied (headings,
prompts, ref pointers) and what is not (summary, log, decisions). Create opens the
README with the caret in the Goal line. Lineage chips appear immediately on both
sessions.

### Capture and log-today (FR-240–FR-242)

The capture window gains a target line in its footer: `Note draft ⇄ Session log`
(persisted choice; `⌘.` toggles). In session mode the destination chip names the
current session and today's date; Escape appends under today's log heading and hides
— the identical save-on-Escape contract, including the failure branch. The tray shows
the current session with its freshness and *Log Today* beneath the notes items; when
the current session is a fallback guess (FR-242) the tray says *"(latest)"* after the
name rather than pretending a choice was made.

## Interaction grammar

Everything from the notes grammar applies in the editor. Board-specific: `j/k` move,
`p` pin, `e` archive… (opens the checklist), `u` mark read, `↵` open detail, `⌘↵`
open README directly. `⌘7` view, `⌘⌥L` log today. All verbs registered once in the
action registry (UX-DR42's rule) so palette, menu, cheat sheet and keys agree.

## Failure and edge states

- **No root flagged** — the board shows the one-sentence explainer and a button to
  Settings → Sync; the tray and palette omit session verbs (capability on, surface
  honest).
- **Zone missing on disk** (volume unplugged) — rows persist from cache, marked
  offline exactly as the sync card does (AD-48's absence-is-not-deletion posture).
- **Unparseable Promote table** — the panel shows readable rows plus the raw
  unreadable lines verbatim with a located note; nothing is rewritten (PRD §8).
- **Foreign `id` in a README** — indexed by path, *unstable identity* caveat chip, as
  notes.
- **Conflict copies** inside a session — conflict rows in the tree and the row badge,
  resolved through the existing conflict surface.
- **Archive target year-folder missing** — created as part of the move step, silently;
  its absence is not a user problem.

## Experience decisions UX-DR85 … UX-DR92

Allocated in the product inputs; bodies here.

### UX-DR85 — The board is a status surface first, a list second
Freshness-sorted, status glyphs, last log line as subtitle: "what is being worked on"
answers itself without a click. Search and chips sit above the fold, as the
recordings browser established (UX-DR50).

### UX-DR86 — Two freshness signals, never one
Workspace activity means *iterating*; record activity means *written/promoted*. One
merged dot would hide exactly the distinction the review loop needs.

### UX-DR87 — The archive flow is the zone's checklist made visible
Its steps, order, and words come from the zone README; keeper adds visibility and
resume, not policy. Warnings inline and acknowledgeable; one unskippable step; never
a modal wall.

### UX-DR88 — The preview is the documentation
New-session and new-like-this render the actual tree and skeleton they will create
from the real template files; no schematic mock. (UX-DR45's rule generalized.)

### UX-DR89 — Lineage is a breadcrumb chain, navigable both ways
`continues` / `continued-by` chips on the header; dangling refs inert, never errors.

### UX-DR90 — The promote panel renders the README table; it does not own a list
Every panel action is a table write; hand edits to the table are equally valid and
appear on next index. Two columns, staleness badges, loud missing-target.

### UX-DR91 — One editor everywhere, capture included
Any text file in a session opens the notes editor; the capture window hosts session
logging through the same machinery. A sessions-specific editor is a regression.

### UX-DR92 — A user who knows notes already knows sessions
Chips, tags, pins, unread, origin, search grammar, capture chrome: identical
components with sessions data. Divergence in look or behaviour is a defect, not a
choice.
