---
title: "PRD Phase 7: Sessions"
status: final
created: 2026-08-12
binds: [FR-222..FR-252, NFR-36..NFR-39]
sources:
  - _bmad-output/planning-artifacts/product-inputs-sessions-2026-08-12.md
  - _bmad-output/brainstorming/brainstorm-keeper-sessions-2026-08-12/brainstorm-intent.md
  - _bmad-output/brainstorming/brainstorm-keeper-sessions-2026-08-12/ground-truth.md
---

# PRD Phase 7: Sessions

## 0. Document purpose

This document is the Phase 7 increment of the keeper PRD. It specifies only what sessions
add, continuing the global sequence that prior phases left at FR-221 and NFR-35 with
**FR-222–FR-252 and NFR-36–NFR-39**. `prd.md`, the Phase 4 sync inventory, the Phase 5
notes PRD and the Phase 6 recording×sync PRD remain authoritative for everything else;
nothing here relitigates them.

Numbers are allocated in `product-inputs-sessions-2026-08-12.md` and nowhere else. The
architecture amendment owns AD-107–AD-115 (crate placement, index model, IPC shape, the
archive plan, watcher wiring); the experience extension owns UX-DR85–UX-DR92; Epics 47–49
own sequencing. Where a downstream document appears to disagree with this one, the
downstream document states the *how* and this one the *what*.

### 0.1 Vocabulary delta

- **Sessions root** — a sessions-flagged sync profile plus a named subfolder (default
  `60-sessions`) inside it (FR-222). The same construction as a notes *vault* (FR-94),
  over the same profiles; a folder may be both.
- **Session** — one directory `active/YYYY-MM-DD-<slug>/` or `archive/YYYY/…/` inside a
  sessions root, shaped by the zone's `_template/`: `README.md`, `workspace/`,
  `artifacts/`, `refs/`, `prompts/`. The folder is the record; keeper is a lens over it.
- **Session README** — the session's `README.md`: summary, key decisions, dated log, what
  worked, follow-ups, the Promote table. Its frontmatter is the session's identity, tags,
  properties and pins. It is an ordinary markdown file the notes editor opens.
- **Workspace** — the session's `workspace/` subtree. Gitignored at drive level, never
  versioned, never backed up, never synced. keeper lists it read-only and never writes,
  commits, or searches it by default.
- **Artifact** — a file under `artifacts/`: promoted output, versioned and synced.
- **Promotion** — an explicit copy of a `workspace/` file to a stable name under
  `artifacts/`, recorded as a row of the README's `## Promote` table. Re-promotion under
  the same name is the normal mid-session rhythm; git history keeps the versions.
- **Archive (verb)** — the zone's closing checklist: promote finals per the table, empty
  `workspace/` (leave `.gitkeep`), close the README, move the folder to
  `archive/<close-year>/`. **Distinct from** the notes `archived` flag (FR-119), the
  chats Archive view, and the Local Archive: a session's archived state is its location.
- **Continuation / lineage** — a new session created from a previous one, the two linked
  both ways by frontmatter id references (`continues` / `continued-by`).
- **Current session** — the sticky per-root choice of which active session receives
  captures and log entries by default (FR-242).

## 1. Why sessions belong in keeper

The owner already runs LLM work sessions as folders — the `60-sessions/` zone exists on
both drives, has a written contract (zone README + AGENTS.md), a template, and live
sessions. The zone works because it is plain folders and markdown: any agent with a text
editor is a first-class author, and keeper's sync engine already versions, stamps and
ships everything but the scratch space.

What is missing is the *human* side of the loop. The session record is supposed to be
written during the work, but writing it means alt-tabbing to an editor; reviewing what an
agent did means a terminal and `git log`; archiving means a careful sequence of manual
copies and moves that the zone's own README has to spell out as prose; and finding last
quarter's session means Finder. Every one of these is a surface keeper already built for
notes — the editor, capture, live change streaming, history/blame, search, pins — pointed
at files the sync engine already owns.

The phase is therefore mostly *reuse*: a folder-level domain (index, lifecycle, lineage,
the promote airlock) over the existing file-level machinery. What notes did for a `.md`
file, sessions do for a directory with a contract.

The alternative — a session tracker with its own store — is rejected for the same reason
it was rejected for notes: the zone predates the feature, agents write to it directly,
and a second source of truth would desync the first time anyone touched the files in
Finder. **Files are the only truth** is a hard rule of this phase: every visible fact
must be derivable from files plus git, and any state a Finder edit could desync is a
design defect.

## 2. Target user and jobs to be done

Same owner-operator as prior phases, plus the second neuradrive operator, plus the
*agent* as a co-author (it writes most of the words; the human reviews).

1. **See what is being worked on** — all active sessions across both drives with honest
   freshness: what moved, in which subtree, by whom, how long ago. Without a terminal.
2. **Review what happened while I was away** — unread sessions, per-file history,
   "changes since I last looked". The session surface is a review surface first.
3. **Write the record where the work is** — edit README/artifacts/prompts with the full
   notes editor; capture a log line in two seconds without leaving the current context.
4. **Start a session in zero decisions** — template pre-staged, date and slug generated;
   "new like this one" from any previous session, with lineage recorded both ways.
5. **Close a session honestly** — the archive checklist runs the zone's own rules,
   promotes what the table names, and offers deletion when nothing is worth keeping.
6. **Find it a year later** — search a half-remembered fragment across README, artifacts
   and prompts; browse by tags, properties, pins; follow lineage chains.

## 3. Product principles for this phase

1. **A sessions root is a folder you already sync, plus a flag.** No picker, no import,
   no second store, no scaffolding surprise (adopt-only; skeleton creation is explicit).
2. **Reuse the notes machinery wholesale.** One editor, one capture system, one change
   pipeline, one frontmatter writer, one query grammar, one trash. A sessions-specific
   fork of any of these is a defect.
3. **Files are the only truth.** Status = location; freshness = derived; lineage, tags,
   pins = frontmatter; promotion = the README table. The index is a disposable cache.
4. **keeper never runs the agent.** Sessions are observed and edited, not executed.
   "Manual or hybrid" means human-in-keeper and agent-on-disk see each other live.
5. **The zone's rules are the feature's rules.** Promote-then-archive, delete-if-empty,
   continuation-over-growth, workspace-is-scratch — keeper enforces nothing the zone
   README does not already say, and surfaces everything it does say.
6. **A user who knows notes already knows sessions.** Search chips, tags, pins, unread,
   origin glyphs, capture — identical look and behaviour (UX-DR92).

## 4. Functional requirements

### 4.1 Roots & capability — FR-222…FR-224

**FR-222 — the flag.** Settings → Sync → a folder → *This folder has sessions*, with a
subfolder name defaulting to `60-sessions`. Flagging requires the subfolder to exist and
contain at least one of `active/`, `archive/`, `_template/`; otherwise keeper offers the
explicit *Create the sessions skeleton here* action (creates `active/`, `archive/`,
`_template/` with the canonical template README, zone README, and the workspace ignore
rule if absent) behind its own confirmation naming exactly what will be written. Never at
flag time silently.

**FR-223 — capability.** `CapabilitiesVm.sessions` is true only where sync is available;
every sessions surface (view, menu items, palette verbs, tray items) is absent rather
than disabled when false. Flagged-but-missing-git degrades exactly as sync does.

**FR-224 — multi-root.** All flagged roots list together, root named per row group or
switcher (mirroring the vault switcher); every operation is `(rootId, sessionId)` scoped.

### 4.2 Index & model — FR-225…FR-228

**FR-225 — what is a session.** Direct children of `active/` and of `archive/YYYY/` are
sessions. `_template/`, any `_*` or dotted entry, and loose files are not. The index
(title, frontmatter, freshness, log dates, promote table, lineage) caches under the
zone's `.keeper/` (tier-0 excluded from sync) and rebuilds from disk; *Rebuild index* is
available as in notes.

**FR-226 — identity.** A session's `id` is a ULID in its README frontmatter, minted on
first index if absent, written once, byte-preserving. Pins, unread marks, lineage and
links follow the id through the archive move and renames. A non-ULID pre-existing `id`
is preserved and the session indexed by path with the *unstable identity* caveat, as in
notes.

**FR-227 — session frontmatter.** The README's frontmatter follows the notes three-tier
contract verbatim: keeper-owned (`id`, `created`, `updated`, `pinned`, `keeper.*` —
adding `keeper.session` one level deep: `keeper.session.continues`,
`keeper.session.continued-by`, list-valued), Obsidian-native (`tags`, `aliases`), yours
(anything else — indexed, queryable via `field:`, editable in the properties panel).
Tool/model/goal from the template's bullets may be lifted to frontmatter by the user;
keeper reads either, prescribes neither.

**FR-228 — the row.** A session row derives: title (README H1, falling back to folder
name), status (active/archived, from location), **two freshness signals** — workspace
(newest mtime under `workspace/`) and record (newest of README/artifacts/refs/prompts
change) — last log entry date and first line, snippet (Summary), tags, pinned, unread,
origin glyph of the last versioned change, conflict presence, and lineage presence.

### 4.3 List, search, spaces, pins — FR-229…FR-232

**FR-229 — query grammar.** The sessions list filters with the notes grammar: `tag:`,
`field:`, `date:` (created/modified/touched), `origin:`, `text:`, `is:` over the closed
set `active archived pinned unread conflict stale lineage`. `is:stale` means active with
no change in either signal for 14 days (constant, not configurable this phase). A parse
error matches nothing and says why, as in notes.

**FR-230 — text sweep.** `text:` and the bare-word search sweep README, `artifacts/`,
`prompts/`, `refs/` (text files only, bounded parallel scan, no persistent FTS).
`workspace/` is excluded by default; an explicit *include workspace* toggle widens one
search, never sticks.

**FR-231 — default spaces.** Sessions ship default saved queries — Active, Archived,
Stale, Agent-written (`origin:agent`) — stored as ordinary notes under the zone's
`spaces/` (same mechanism, `keeper.space.query` with a `sessions:` scope marker),
editable, deletable, restorable via the existing restore-defaults verb.

**FR-232 — pins.** `pinned: true` in the README frontmatter; toggled from row, detail
and palette; pinned sessions sort first within their status group. Same writer, same
byte-preservation promise as notes.

### 4.4 Detail & editing — FR-233…FR-237

**FR-233 — the detail.** A session opens as a panel target showing its tree — README
first, then `artifacts/`, `refs/`, `prompts/`, then `workspace/` marked read-only —
plus the header: title, status, lineage breadcrumbs, freshness pair, tags, pin. Any
text file opens in the full notes editor (properties panel on the README, format
toolbar, slash menu, mermaid, tables, embeds); any other file goes through the existing
viewer registry. Two-at-once (open beside) behaves as in Files/notes.

**FR-234 — live changes.** The notes change pipeline applies unchanged: list rows
stream (`SessionChangeBatch` coalesced), an open clean buffer takes external writes
live with the fading highlight, a dirty buffer merges non-overlapping hunks and raises
the inline diff bar; never a modal, never a lost buffer. Workspace-only changes update
the freshness signal without touching any buffer.

**FR-235 — unread & since-last-looked.** A session accrues an unread mark when its
versioned content changes under another origin, cleared against the exact revision seen
(the notes `headRev` contract). The detail offers *Changes since you last looked*: the
list of files changed since the acknowledged revision, each opening its diff.

**FR-236 — history & blame.** Per-file history (device, origin, time, diff, restore)
for versioned session files, projected from commit trailers exactly as notes history.

**FR-237 — workspace projection.** `workspace/` lists name, size, mtime, read-only;
files open through read-only viewers (explicitly not editable in keeper); keeper never
writes there. The listing states the zone's own words: scratch, unversioned, dies with
the session.

### 4.5 Lifecycle — FR-238…FR-249

**FR-238 — new from template.** *New session* asks one thing (the title; root
preselected by context), then copies `_template/` verbatim to
`active/YYYY-MM-DD-<slug>/` (slug from title, collision counter), stamps the README
date line, mints the id, opens the README with the caret in the Goal line. The preview
in the dialog is the actual tree and README skeleton that will be created (UX-DR88).

**FR-239 — new like this.** From any session (active or archived): copy structure only —
README headings and template-shaped scaffolding, the `prompts/` files, `refs/` entries
that are pointers (not copies) — never Summary/Log/Decisions content. Write
`keeper.session.continues: [<source-id>]` in the new README and append the new id to
`keeper.session.continued-by` in the source README (a real write, including into
`archive/` — files are truth). Offer *…using a different pattern* to pick another
session/template while keeping the lineage reference to the origin session.

**FR-240 — log today.** One verb appends `### YYYY-MM-DD — ` under `## Log` (newest
last, per the template's convention) of the target session — creating the section if
missing — opens the README, drops the caret at the entry. In menu, palette, tray.

**FR-241 — capture parity.** A session README opens as a capture window (same chrome,
lock, always-on-top). The quick-capture panel gains a target switch: *note draft*
(default, unchanged) or *current session log* — the latter appends to today's log entry
of the current session, buffer-safe per the capture contract. No new capture system;
the same prewarmed window, the same editor.

**FR-242 — current session.** Per root, a sticky explicit choice (set from row, detail,
tray); shown in the tray with freshness and *Log today* beside it. Unset, it resolves
to the most-recently-touched active session and the tray says it is a guess.

**FR-243 — promote.** From the workspace listing or the promote panel: pick a
`workspace/` file, name (or reuse) its `artifacts/` target, keeper copies under the
sync engine's stability gate and writes/updates the `## Promote` table row
(`| workspace/<src> | artifacts/<dst> | <note> |`) in the same action. Re-promotion
overwrites the target (git history keeps versions). Table edits by hand are equally
valid — the panel renders the table, it does not own it.

**FR-244 — promote panel truth.** The panel shows each table row with staleness (source
newer than target), missing-source (normal after cleanup — shown quietly), and
missing-target (a promise not yet kept — shown loudly). Plus unlisted workspace files,
one action away from promotion.

**FR-245 — archive.** The archive flow is a visible checklist in the zone's own words:
(1) outstanding promotes — run them or explicitly skip each; (2) warnings — hot
workspace (writes within the last 10 minutes), stale promotes, open follow-ups
(unchecked items under `## Follow-ups`) — each acknowledgeable; (3) empty `workspace/`
(leave `.gitkeep`) — skippable with warning; (4) move to `archive/<current-year>/` —
the one unskippable step. The flow is resumable and crash-safe (NFR-38); the result is
one visible operation in the session's history.

**FR-246 — delete instead.** At archive time, a session with an empty Promote table and
empty `artifacts/` is offered **Delete instead** first, quoting the zone rule. Deletion
is available on any session at any time via its own confirmed dialog.

**FR-247 — trash.** Session delete moves the folder to the zone's
`.keeper/trash/<id>/`, recoverable, never a hard unlink; workspace contents go with it
(the trash is their only afterlife).

**FR-248 — unarchive.** Moves the folder back to `active/`. The dialog offers
*Continue instead* (FR-239) first and states why continuation is preferred (the zone's
one-screen-README rule). Unarchive never rewrites lineage.

**FR-249 — sync honesty.** Create/promote/archive complete locally regardless of
network; sync state surfaces through the existing journal/pending surfaces; a session
row whose artifacts have parked uploads shows the existing parked affordance. Nothing
in the lifecycle blocks on push, and nothing claims "synced" that the journal does not.

### 4.6 Linking & cross-surface — FR-250…FR-252

**FR-250 — linking.** A note can wikilink a session (`[[session-title]]` resolving via
the sessions index; id-stable). A session README links notes and files by the notes
rules — session-relative paths within the session folder (they survive the archive move
untouched), repo-root-relative across zones (unaffected by the move). Backlinks resolve
across both indexes; the backlinks panel on a session README shows notes that point at
it.

**FR-251 — surfaces.** Sessions appear in: the app menu (Sessions view, New Session,
Log Today); the command palette (view, new, new-like, log-today, promote, archive, pin,
current-session); the quick switcher (sessions by title); the tray (current session +
freshness + Log Today). Keyboard: the view joins the existing ⌘-digit cycle; `⌘⌥L` is
log-today.

**FR-252 — reveal.** Row and detail expose Reveal in Finder and Copy Path for the
session folder and any file — the existing verbs, no bespoke tooling.

## 5. Non-functional requirements

**NFR-36 — index cost.** Cold scan of a 200-session zone < 2 s; the list paints from
the `.keeper` cache first and reconciles. (A zone has tens of sessions today; 200 is
the design ceiling.)

**NFR-37 — watch cost.** Workspace freshness comes from bounded scanning: watcher
events only for sessions currently visible in the list window; coarse on-demand stat
elsewhere. No measurable idle CPU attributable to sessions on a 400 GB drive.

**NFR-38 — archive safety.** The archive flow is a journaled plan: a crash mid-flow
leaves either a resumable checklist or a clean rollback, never a half-moved folder or a
workspace emptied without its promotes.

**NFR-39 — write discipline.** Every keeper write into a session (frontmatter key,
promote-table row, log-entry append, lineage append) is byte-preserving outside the
targeted span, per the notes writer contract; Obsidian and the agent read everything
else unchanged.

## 6. Scope boundaries

**In:** everything in §4.

**Out this phase, deliberately:**

- Running, launching, or supervising any agent from keeper; transcript ingestion or
  chat-log parsing (the README is the record, per the zone's own design).
- Board/table lenses over session properties (follows the notes FR-123/124 work).
- Cross-session property comparison tables.
- Automatic archiving, automatic promotion, or any unattended mutation.
- Editing `workspace/` files in keeper (read-only projection only).
- A persistent FTS index for sessions (the bounded scan is the phase posture, as it was
  for notes).
- Sessions on iOS/mobile; publishing a session to a Matrix room.
- Configurable staleness threshold, per-session cadence knobs.

## 7. Interoperability contract

- The zone stays fully Obsidian- and agent-compatible: plain folders, plain markdown,
  frontmatter only; keeper's cache confined to `.keeper/`; `_template/` is user data
  keeper copies but never edits unasked.
- Everything keeper writes, a human could have written by hand; everything a human
  writes by hand, keeper converges on at the next scan (index rebuild covers the rest).
- The drive's own ignore rules govern `workspace/`; keeper neither adds nor removes
  drive-level ignore rules at flag time (skeleton creation is the one exception, and it
  says so).
- Both existing zones (tgdrive, neuradrive) must index and operate without any on-disk
  migration; the neuradrive template variant ("who ran it") is ordinary content.

## 8. Risks and mitigations

- **Archive move races an agent mid-write.** Mitigation: hot-workspace warning
  (FR-245), stability gate on promote copies (FR-243), journaled plan (NFR-38).
- **Promote table drift** (hand edits, exotic formatting). Mitigation: the parser
  accepts the documented shape, surfaces unparseable rows verbatim as
  *unreadable-but-preserved* rather than failing the panel; never rewrites rows it did
  not touch.
- **Two identity systems** (path vs ULID) across archive moves. Mitigation: the notes
  precedent (index by id, path is presentation); the unstable-identity caveat for
  foreign ids.
- **Scope creep toward an agent console.** Mitigation: §6 exclusions; the review
  surface ships value without execution.
- **Watcher cost on huge workspaces.** Mitigation: NFR-37's visibility-bounded
  strategy; workspace freshness may lag when not on screen, and the UI never promises
  otherwise.

## 9. Phase acceptance

1. Flag an existing populated zone (tgdrive) → sessions list correct with zero on-disk
   writes beyond `.keeper/` and minted ids.
2. Agent edits a session README on disk → row updates and unread appears without
   focus-switching; open buffer shows the diff bar when dirty.
3. Create-from-template and new-like-this produce folders a hand copy would have,
   lineage readable in both READMEs with plain `cat`.
4. Promote → file at stable artifacts path, table row present, re-promote versioned in
   git history.
5. Archive a session with promotes outstanding → checklist runs them, workspace
   emptied to `.gitkeep`, folder in `archive/2026/`, id/pins/lineage intact; kill the
   app mid-archive → relaunch resumes or rolls back cleanly.
6. Search `text:` fragment known to be in an archived artifact → hit; `is:active
   origin:agent date:modified>=-2d` → the expected rows.
7. Quick capture targeted at the current session → line under today's log entry within
   the capture latency budget, buffer surviving dismissal.
8. Every session surface absent (not broken) when no root is flagged or git is
   missing.
