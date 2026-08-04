---
title: "PRD Phase 5: Notes"
status: final
created: 2026-08-02
binds: [FR-94..FR-124, NFR-27..NFR-30]
sources:
  - _bmad-output/planning-artifacts/product-inputs-notes-2026-08-02.md
  - _bmad-output/brainstorming/brainstorm-keeper-notes-2026-08-02/brainstorm-intent.md
  - _bmad-output/brainstorming/brainstorm-keeper-notes-2026-08-02/.memlog.md
---

# PRD Phase 5: Notes

## 0. Document purpose

This document is the Phase 5 increment of the keeper PRD. It specifies only what notes add,
continuing the global sequence that prior phases left at FR-93 and NFR-26 with **FR-94–FR-124
and NFR-27–NFR-30**. `prd.md` §1–§14 and the Phase 4 folder-sync requirements inventory remain
authoritative for everything else; nothing here relitigates them.

It deliberately does not restate four things that live elsewhere and are cross-referenced by
number only:

- **The numbering spine.** Every FR, NFR, AD, UX-DR and Epic number in this phase is allocated
  in `product-inputs-notes-2026-08-02.md` and nowhere else. This document expands those numbers;
  it never mints one.
- **AD-54–AD-63.** The architecture amendment owns crate placement, the index model, the IPC
  shape, the custom scheme, the second window, the tray construction order, the cadence hook and
  the provenance projection. This document states the *what*; the amendment states the *how*, and
  where the two appear to disagree the amendment is wrong until amended.
- **UX-DR35–UX-DR44.** The experience extension owns layout, interaction and copy.
- **Epics 35–39.** Sequencing, story breakdown and the dependency between the Linux tray
  remediation and the notes affordances that rely on it belong to the epic document.

### 0.1 Vocabulary delta

The notes surface introduces terms that collide with `prd.md` §3. The collisions are named here
so no story is written against the wrong one.

- **Vault** — a notes-flagged sync profile plus a named subfolder inside it (FR-94). Not a store,
  not a database, not a keeper-owned location: an ordinary directory the user already syncs.
- **Note** — one `.md` file inside a vault. The file is the record; everything else is a
  projection of it.
- **Space** *(notes)* — a saved query over a vault, stored as an ordinary note under `spaces/`
  (FR-105). **Distinct from the Matrix Space of `prd.md` §3** (FR-23). The two never appear on the
  same surface; notes copy says "space" unqualified, messaging copy keeps saying "Space".
- **Archived note** — a note excluded from the default lens by frontmatter, never moved (FR-119).
  **Distinct from the Archive view** (chats) and the **Local Archive** (events).
- **Lens** — a way of presenting the same note set: the filtered list (FR-103), the physical tree
  (FR-106), the table or board (FR-123). Changing lens is a filter, never a navigation.
- **Origin** *(notes)* — the provenance of a change, read from the sync engine's existing commit
  trailers (AD-63): device, origin host, source. "Another origin" means a change whose device
  identity is not this installation's, or whose source is `bot`.
- **Quick Capture** — the always-on-top hotkey panel (FR-101). The product's front door.

## 1. Why notes belong in keeper

keeper is not entering the note-taking market. Obsidian has won the file-based-notes argument and
the plugin ecosystem that goes with it, and keeper will not out-feature it. The case for this
phase is narrower and harder to copy: keeper already owns two assets that every note application
lacks, and the cost of turning them into a note system is mostly the cost of an editor.

**Asset one: notes beside a Matrix timeline.** keeper is the only note surface already looking at
the user's conversations. Every fact worth writing down arrives in a message first, and today the
path from "this matters" to "this is written down" crosses an application boundary — which is
exactly where it stops happening. The capture surface does not have to be good to beat that; it
has to be *present*. This phase does not yet ship capture from a message (§6), but it is the
reason the frontmatter contract reserves a provenance key for it now rather than migrating later.

**Asset two: notes on top of a sync engine that already stamps provenance.** Phase 4 shipped
git-protocol folder sync in which every engine-authored commit carries device, origin and source
trailers (AD-44). That is, precisely, per-file authorship history for anything inside a synced
folder — the thing Obsidian Sync does not expose and the Obsidian Git plugin exposes only as raw
git. So "who changed this note, on which machine, and was it the agent" (FR-114) and "the agent
touched notes you have not read" (FR-113, FR-102) are **projections of data already on disk**, not
new machinery. That is the whole differentiating cluster, and it is nearly free.

**The thesis those two assets make possible:** agents write markdown files today, into folders
nobody is watching. The user discovers what changed by reading a diff in a terminal, or not at
all. keeper's note surface is therefore first a *review* surface — unread marks, per-note history,
inline diffs, a dot on the tray glyph — and only second an editing surface. No shipping note
application treats the machine as a co-author with an audit trail. That is the headline, and §4
specifies it as a requirement rather than as polish.

**What the phase costs, honestly.** The vault-as-sync-element decision (FR-94, AD-54) deletes an
entire settings surface before it is built: no vault picker, no path validator, no import, no
migration, no second configuration store. Attachments reuse the existing custom URI scheme;
actions reuse the existing registry; conflicts reuse the existing surviving-copies model; the
cadence reuses the existing 1 Hz supervisor. What is genuinely new is a pure notes domain in
`keeper-core`, a vault IO and watching module in the shell, and a markdown editor in the webview.
The editor is the expensive part, and it is the part with the fewest keeper-specific decisions.

**What keeper is not claiming.** Not a plugin ecosystem, not a graph view, not a mobile note app,
not a better markdown editor than the one the user already has. If the user prefers Obsidian for
authoring, that is a supported configuration — the vault is the same folder, and §7 is a contract,
not a courtesy.

## 2. Target user and jobs to be done

The user is the same person `prd.md` §2 describes: the self-hosting power communicator who already
runs keeper for messaging and already syncs at least one folder. The eight jobs below come from the
divergent session; each is stated with the design consequence it forces, because a job that forces
nothing is decoration.

1. **Catch a thought in under two seconds, without leaving the context I am in.** Outranks every
   other job; if this fails the rest is unused. *Forces:* a global hotkey and a second always-on-top
   window that is already loaded and hidden (FR-101), a hard latency bar (NFR-27), and a
   prohibition on every dialog, prompt and save button in the entire feature (UX-DR35).
2. **Give me and my agent one shared brain we both write to, in files.** *Forces:* plain `.md` as
   the record of truth, not a database with a markdown export (FR-96 makes the index disposable);
   an external write is a first-class input path, not an edge case (FR-112).
3. **Keep what I write in files I own that are already synced.** *Forces:* the vault is a folder
   inside an existing sync profile (FR-94) — which in turn forbids a vault picker, an import
   wizard and any notion of "adding" a vault that is not "flagging a profile".
4. **Find the thing I wrote three months ago in five seconds, from a half-remembered fragment.**
   *Forces:* content scan over an index-free path so results are never stale (FR-118), and a filter
   model where narrowing is composable and instant (FR-103) rather than a search page you navigate
   to and back from (UX-DR41).
5. **Turn a message I just received into a note without leaving the messenger.** The capture
   surface for this is not in this phase (§6). *Forces, now:* the frontmatter namespace reserves a
   `source` key (§7) so the eventual feature is a new capture path over an unchanged file format,
   not a migration.
6. **Keep a daily journal going without ever deciding where today's entry lives.** *Forces:* a
   deterministic journal path with a per-vault template (FR-99, FR-120) and a Today action that is
   idempotent — the second invocation of the day must not duplicate a template header.
7. **See what changed while I was away — by whom, on which machine, and whether it was the agent.**
   *Forces:* unread-by-origin marks and a diff on open (FR-113), per-note history with provenance
   (FR-114), and a tray indicator that is visible without opening the window (FR-102).
8. **Park a half-finished thought in an inbox that is honest about being unfiled.** *Forces:* the
   deepest consequence in the phase — because nothing is filed at creation, the *virtual*
   organisation is the only organisation that exists. Tags (FR-104), spaces (FR-105) and the
   filtered list (FR-103) are therefore load-bearing, and the physical tree (FR-106) is the escape
   hatch rather than the model.

**Non-users this phase.** Anyone on a shell without folder sync: the iOS client renders no notes
surface at all rather than a dead one (FR-122). Anyone who wants a hosted, encrypted, multi-user
notes service — that is a different product and §6 says so.

## 3. Product principles for this phase

The five verdicts carried from the divergent session, restated as things the user can observe. Each
one forbids something; the prohibition is the useful half.

### 3.1 The window can be closed

Everything the notes feature knows is computed in Rust and handed to the webview as a view model.
The user can close the main window and keep working from the tray and the capture panel for a whole
day; a webview crash costs no text; the same note, open in two places, is the same note.

*Forbids:* note state that exists only in the browser, a list the frontend filters or sorts by
itself, a body that crosses IPC as a large JSON payload, and any parsing, linking or tag logic
written in TypeScript.

### 3.2 Your vault is a folder, and it stays yours

Obsidian opens the vault unchanged, before and after keeper touches it. keeper adds files where the
user asked for files; it never relocates one. Everything keeper knows that is not in a file is a
cache the user may delete with no consequence beyond a few seconds of rescan.

*Forbids:* moving a note as a side effect of any action (including archiving), reading or writing
`.obsidian/`, committing keeper's cache, storing a user-visible property anywhere but the note
itself, and any feature whose value disappears when the user opens the folder in another editor.

### 3.3 Rename anything; nothing breaks

A note has a durable identity independent of its path. Renaming a note — in keeper, in Obsidian, in
a file manager, on the other machine — preserves its links, its pin, its unread mark and its
history.

*Forbids:* path-keyed durable state for notes keeper authored, and a link format that resolves only
by filename.

### 3.4 Nothing is ever locked, and nothing is ever a modal

Two writers are the normal case, not the failure case. keeper watches instead of locking: a clean
buffer takes the external change live, a dirty buffer merges what does not overlap and shows an
inline bar for what does. Where the two genuinely disagree, the disagreement becomes a visible row
the user can resolve later, not a dialog blocking the sentence they were writing.

*Forbids:* file locks, advisory or otherwise; a modal anywhere in the feature; discarding the
user's unsaved text under any circumstance; silently choosing a winner.

### 3.5 The tray is the product on every OS

Capture is tray-first, so the tray must be equally good on Linux and macOS. No affordance the
feature depends on may be macOS-only, and the known Linux tray defects — template-only glyphs that
render black on a dark panel, a menu that cannot be replaced after it is first set, unsupported
click events — are fixed before the notes items rely on them.

*Forbids:* a notes affordance reachable only by tray left-click; a tray item added after tray
creation; a glyph shipped in template form only; and a hotkey that is claimed to be bound when the
compositor refused to bind it.

## 4. Functional requirements

*FR-94–FR-124 continue the global sequence. Each requirement states what must be true, what an
acceptance test can observe, and the failure mode the implementation must not have. Terms are used
per §0.1. "User" means the single desktop operator; "agent" means any non-interactive writer of
files in the vault, whether an LLM tool, a script or another machine.*

### 4.1 The vault (FR-94–FR-97, FR-121–FR-122)

#### FR-94: Notes flag on a sync profile

**Requirement:** A sync profile gains a `notes` option: an opt-in flag plus the vault subfolder path
relative to the profile root, defaulting to `notes/`. Flagging is the *only* configuration a vault
requires. The setting rides the profile's existing persisted representation and defaults to
not-a-vault, so profiles written by an earlier build load unchanged.

**Acceptance (observable):**
- Flagging an existing profile makes its vault appear on every notes surface without a restart, and
  scans only the named subfolder — not the whole profile.
- Unflagging removes the vault from every notes surface and modifies no file inside it.
- A profile that has never been flagged behaves in every respect exactly as it did in Phase 4.
- The subfolder is created on first write if absent; a subfolder that exists and contains a
  non-keeper Obsidian vault is adopted as-is, with no conversion step.

**Must not:** introduce a second configuration store, a vault registry, a path validator, an import
wizard or any migration. There is no user-facing concept of "adding a vault" separate from flagging
a profile.

#### FR-95: Multi-vault with switching as a filter

**Requirement:** Every notes-flagged profile is a vault. keeper lists all of them and switches the
active one, from a switcher holding the position and affordance of the account switcher (UX-DR36). A
switch re-scopes already-held state; it is not a teardown and reload.

**Acceptance (observable):**
- With two or more vaults, the switcher lists each with its display name (FR-120) and note count.
- Switching to an already-indexed vault paints within the NFR-28 list bar and performs no disk scan.
- Each vault remembers its own last selection and active filters; switching back restores them.
- The buffer of any note being edited is flushed before the switch completes.

**Must not:** block the UI on a rescan, discard unsaved text on switch, or require a restart to see a
newly flagged vault.

#### FR-96: Vault index as a disposable cache

**Requirement:** keeper scans the vault and builds an index of every note's path, title, identity,
frontmatter fields, tags, outbound links, backlinks and attachment references. The index is a cache:
deleting it and relaunching reproduces identical behaviour, differing only in cold-start time.

**Acceptance (observable):**
- Deleting keeper's cache directory and relaunching yields a list identical to the one before.
- A note created by an external process appears without any manual refresh (NFR-29).
- A note with malformed frontmatter is indexed with its raw body and a parse-warning field, and
  appears in the list — it is never dropped, and it never fails the scan.
- A file that is not a `.md` note is indexed as an attachment candidate or ignored, never as a note.

**Must not:** become a source of truth. No user-visible property may exist only in the index, and no
action may be unavailable because the index is stale — a stale index costs a stale row, never data.

#### FR-97: Note identity survives renames

**Requirement:** Every note keeper writes carries a ULID `id` in its frontmatter. Links, pins,
archive state, unread marks, history and the recent list resolve by `id` first and by path second, so
a rename performed anywhere breaks nothing. keeper stamps an `id` only on notes it writes; a note
authored elsewhere is tracked by path and gains an `id` the first time keeper writes to it.

**Acceptance (observable):**
- Renaming a keeper-authored note outside keeper preserves its pin, its unread mark and every
  inbound wikilink.
- A note with no `id` is fully usable — listed, filtered, searched, opened, edited — and is not
  rewritten merely by being read or displayed.
- Two notes carrying the same `id` (a copy-paste duplicate) are both listed, the older one is treated
  as canonical for link resolution, and the duplicate carries a visible marker.

**Must not:** rewrite a file solely to add an identity; treat a missing `id` as an error state; lose
durable state because the user renamed a file.

#### FR-121: Obsidian coexistence

**Requirement:** `.obsidian/` is never read and never written. keeper never moves a file the user did
not explicitly ask it to move. keeper's cache directory is excluded from sync through the *profile's*
ignore rules — keeper writes nothing into the vault to achieve the exclusion.

**Acceptance (observable):**
- After a full session exercising every notes feature — create, capture, journal, template, tag, pin,
  archive, attach, link, resolve a conflict — every path under `.obsidian/` is byte-identical and its
  modification times are untouched.
- The cache directory never appears in a commit, at any point, in any profile.
- The vault opens in Obsidian after that session with no repair prompt, no missing file and no file
  in a place the user did not put it.

**Must not:** read Obsidian's configuration to "helpfully" import anything; write into `.obsidian/`;
commit the cache; relocate a note as a side effect of any keeper action, including archiving.

#### FR-122: Notes capability gate

**Requirement:** A `notes` capability flag rides the existing IPC capability handshake, present only
on shells that have folder sync. Where it is absent the notes surface does not exist: no navigation
entry, no palette action, no tray item, no settings section, no reachable command.

**Acceptance (observable):**
- With the flag off, no notes affordance renders anywhere and every notes IPC command is unreachable
  rather than erroring.
- With the flag on and no vault flagged, every surface renders its empty state and the tray items are
  present-but-disabled with an explanatory label (FR-102) — the capability is a platform property,
  not a function of whether a vault exists.
- The flag is data-driven per platform, so a future shell that gains folder sync gains notes with no
  UI rework.

**Must not:** render a disabled notes view on a shell that should not have one; return an error from a
command that should not be reachable; couple the flag to vault presence.

### 4.2 Capture, journal and the tray (FR-98–FR-102, FR-117, FR-120)

#### FR-98: Create a note without a dialog

**Requirement:** The first line of the body is the title. The filename is `YYYY-MM-DD-<slug>.md`,
where the slug is derived from the first line — lowercased, ASCII-folded, non-alphanumerics collapsed
to a single `-`, truncated at a bounded length — and a collision appends `-2`, `-3` and so on. An
empty first line at write time yields `untitled`. The filename is derived once, at creation:
subsequent title edits do not rename the file, and a "Rename file to match title" action exists for
when the user wants it.

**Acceptance (observable):**
- Creating a note, typing and dismissing produces the expected file on disk within the FR-115
  debounce, with no prompt of any kind having appeared.
- Two notes created the same day with the same first line produce two files, both intact.
- Editing the first line later changes the displayed title and leaves the path alone; invoking the
  rename action moves the file and preserves links (FR-97).

**Must not:** prompt for title, folder or tags; block the first keystroke on anything; overwrite an
existing file; rename a file the user did not ask to rename.

#### FR-99: Journal

**Requirement:** The journal lives at `journal/<YYYY>/<YYYY-MM-DD>.md` by default, with the path
template configurable per vault (FR-120). The entry is created on demand from the journal template
(FR-100). A "Today's Journal" action opens today's entry, creating it if absent and placing the caret
at the end if present.

**Acceptance (observable):**
- Invoking Today twice in one day opens the same file the second time, with no duplicated template
  content and the caret at the end.
- Invoking it after midnight creates the next day's entry under the correct year folder.
- The action is reachable from the tray, the palette and the native menu with one declaration
  (FR-117).
- Changing the path template affects future entries only (FR-120).

**Must not:** create an empty journal file as a side effect of rendering any date-oriented UI; append
the template header to an existing entry; hardcode the path such that a flat `journal/` layout is
unsupported.

#### FR-100: Templates

**Requirement:** `templates/*.md` inside the vault are ordinary notes applied at creation.
Placeholders expand for date, time, title and caret position, with a documented format argument for
the date and time forms. Expansion is a pure function in the notes domain.

**Acceptance (observable):**
- Creating from a template places the caret at the caret placeholder, or at the end of the document
  when none is present.
- An unrecognised placeholder is left verbatim in the output rather than erased.
- A template folder authored by Obsidian works unchanged; templates appear in the note list like any
  other note.
- A malformed template still creates the note, with the template's raw text as the body.

**Must not:** execute anything. A template is text substitution — no scripts, no shell, no embedded
expressions, no filesystem access. Template application must never be able to fail note creation.

#### FR-101: Quick capture

**Requirement:** A third global hotkey — joining the two that already exist — raises an always-on-top,
undecorated, skip-taskbar capture panel with focus already inside its text area. Escape saves and
hides. The buffer and the caret position survive dismissal and process restart. The capture
destination is a per-vault setting (FR-120), defaulting to the dated inbox note (FR-98).

**Acceptance (observable):**
- The NFR-27 bar holds, and a character typed immediately after the hotkey lands in the buffer.
- Killing the process mid-capture and relaunching restores the buffer text and caret position.
- Escape on an empty buffer writes no file.
- The panel raises over a full-screen application and works with the main window never having been
  opened in that session.
- Where the compositor refuses to register the hotkey (§8), keeper shows a persistent, named state
  giving the actual reason and pointing at the tray item and the palette action — the panel remains
  reachable by both.

**Must not:** show a save button, a title field, a folder picker or a tag picker; require the main
window; lose the buffer on an accidental dismissal; claim a binding it does not hold.

#### FR-102: Tray

**Requirement:** The existing menu-bar icon gains New Note, Today's Journal and the last five touched
notes, and carries an indicator when notes changed by another origin are unread (FR-113). Every item
is present in the menu built at tray creation.

**Acceptance (observable):**
- With a vault flagged, all items are present and enabled; the recent entries relabel in place as
  notes are touched.
- With no vault flagged, the same items are present and disabled with an explanatory label — never
  absent, because the menu cannot be rebuilt on Linux.
- The indicator appears within the NFR-29 window of an other-origin write landing, and clears when
  the last unread note is accepted.
- On Linux the glyph — plain and indicated — is legible against both a light and a dark panel
  (UX-DR43), verified visually as a release gate.
- Every tray-reachable notes action is also reachable from the palette and the native menu (FR-117).

**Must not:** depend on tray left-click or click events that Linux does not deliver; attempt to
replace the tray menu after it is set; ship the indicator glyph in a form that renders invisibly on a
dark panel.

#### FR-117: Notes actions in the action registry

**Requirement:** New Note, Quick Capture, Today's Journal, Open Note…, Search Notes and Switch Vault
are declared once in the existing action registry. The palette, the cheat sheet and the native menu
are generated from that declaration (UX-DR42).

**Acceptance (observable):**
- Each of the six appears in all three surfaces with identical label and accelerator.
- A test asserts that the notes section of the registry and the set rendered by each surface are
  equal — a hand-added or a missing entry fails the build.
- All six disappear from all three surfaces when the capability is off (FR-122).
- No declared accelerator collides with an existing binding; the registry enforces uniqueness.

**Must not:** hand-register a notes entry in the native menu, the palette or the cheat sheet; allow
the three surfaces to be edited independently.

#### FR-120: Per-vault settings

**Requirement:** Journal path template, default note template, capture destination, sync cadence
(idle-commit debounce and push interval) and the vault's display name are per-vault settings, stored
with the profile.

**Acceptance (observable):**
- Every setting has a stated default that makes a freshly flagged vault fully usable with zero
  configuration.
- Changing the journal template affects future entries only; no existing file is renamed or moved.
- The settings surface is reachable from the vault switcher and from the profile's existing sync
  settings, and both routes edit the same values.
- Settings written by a newer build load in an older one without corrupting the profile.

**Must not:** create a second settings store; require any setting to be filled in before the vault
works; apply a template or path change retroactively by touching existing files.

### 4.3 Organisation and retrieval (FR-103–FR-106, FR-118–FR-119)

#### FR-103: Filtered list

**Requirement:** The note list is the phase's primary surface and the editor is secondary (UX-DR37).
It filters by free-text query over title and frontmatter, tag chips (intersecting), space, date
range, origin ("changed by agent") and pinned. Filters compose into one value evaluated in Rust and
delivered as one view model.

**Acceptance (observable):**
- Two tag chips show only notes carrying both; removing one widens the result immediately.
- The composed filter is one keystroke from becoming a space (FR-105).
- Applying a filter does not clear or change the editor: a note that is open stays open even when the
  filter excludes it from the list (UX-DR41).
- Filtering performs no disk scan and holds the NFR-28 paint bar at 10 000 notes.

**Must not:** treat a filter change as a navigation; require a modal filter builder; drop the open
note; sort or filter in the frontend.

#### FR-104: Tags

**Requirement:** Tags come from the frontmatter `tags` field and from inline hierarchical `#a/b` tags
in the body. A tag tree with per-node counts drives filtering, and selecting a parent matches its
descendants.

**Acceptance (observable):**
- A note carrying `#project/keeper` inline is matched by both `project` and `project/keeper`; a parent
  node's count is the deduplicated union of its subtree, not the sum.
- Typing `#` in the editor autocompletes from the vault's existing tag set — adding a tag costs no
  dialog and no declaration step.
- A frontmatter tag stays in frontmatter and an inline tag stays inline: keeper does not migrate one
  form to the other.
- A `#` that begins a markdown heading, sits inside a code fence, or forms a URL fragment is not a tag.

**Must not:** rewrite the user's tag syntax; require a tag to be registered before use; produce a tag
tree that disagrees with what a plain-text search for `#tag` would find.

#### FR-105: Spaces

**Requirement:** A space is a saved query stored as an ordinary note under `spaces/`, with the query
expressed as declarative frontmatter — so it syncs, diffs, and is readable and writable by an agent
with a text editor. The query language is a bounded declarative set (tag include and exclude, path
prefix, frontmatter field comparison, date range, origin, text match) evaluated in the pure notes
domain.

**Acceptance (observable):**
- Saving the current filter as a space writes a note a human can read and an agent can edit; the exact
  key shape is the contract in §7.
- Editing that note in Obsidian and saving changes the space's membership within the NFR-29 window.
- An unrecognised or malformed clause renders an inline "this part of the query was not understood"
  row on the space and still evaluates the clauses that parsed.
- A space appears as a filter in the list (FR-103), never as a folder on disk.

**Must not:** be executable in any sense — no embedded expressions, no note-supplied regular
expressions, no unbounded matching; be stored in the cache or in a database; silently discard a clause
it cannot parse.

#### FR-106: Physical tree lens

**Requirement:** Virtual organisation is the default lens and the real folder structure is always one
click away (UX-DR38): the physical tree is available as an alternative lens, and any row in any lens
can reveal its real path — copy path, reveal in the file manager, open with the OS handler.

**Acceptance (observable):**
- The physical lens shows exactly what is on disk, including files keeper does not manage, which
  appear as leaves rather than being hidden.
- Conflict copies (FR-116) are visible here even while they are also presented as conflict rows.
- Switching to and from this lens leaves the open note open (UX-DR41).
- Reveal opens the platform file manager positioned at the file.

**Must not:** present a virtual row whose real path cannot be obtained; hide files it does not
understand; treat a lens switch as a navigation.

#### FR-118: Search

**Requirement:** Search is a bounded parallel content scan over the active vault, streaming results
as they are found, each carrying the matching line with the match highlighted. There is no separate
search index, so a result is never stale.

**Acceptance (observable):**
- A note written one second earlier is found by the next query.
- On a 10 000-note vault the first results paint before the scan completes, and the full set arrives
  within the NFR-28 envelope.
- Changing the query cancels the in-flight scan; no work is completed for a query the user has
  abandoned.
- Binary files and files above a size threshold are skipped, and the skip is not silent at the
  result-count level.

**Must not:** block the UI thread or the sync supervisor; spawn unbounded concurrency; serve results
from a cached index.

#### FR-119: Pin and archive

**Requirement:** `pinned` in frontmatter floats a note to the top of every list. Archiving sets
frontmatter that removes the note from the default lens without deleting it and **without moving
it**. Both are frontmatter, so both sync, diff and are agent-writable.

**Acceptance (observable):**
- Setting the pin by hand in Obsidian is reflected in keeper within the NFR-29 window, and vice versa.
- An archived note is absent from the default lens, present in the physical lens, and present in
  search when the user asks for archived results.
- Pinning and archiving change exactly one frontmatter key and nothing else in the file.

**Must not:** move a file in order to archive it; store pin or archive state in the cache; make an
archived note unfindable.

### 4.4 Authoring (FR-107–FR-111)

#### FR-107: Live-preview editor with typed properties

**Requirement:** The editor renders markdown live; the source of a construct is revealed only on the
line under the caret. Frontmatter is not shown as raw YAML in the body — it renders as a typed
properties panel (text, list, date, boolean, number inferred from the value) which writes back valid
YAML preserving key order and every key keeper does not own.

**Acceptance (observable):**
- Moving the caret onto an emphasised line reveals its markers on that line only; moving away
  re-renders it.
- A note round-trips through the properties panel byte-identically for every key the user did not
  edit, including key order, comments and quoting style.
- There is no save button anywhere; the buffer commits on idle per FR-115.
- Live preview is the primary mode; a source view exists but is not the primary affordance (UX-DR40).

**Must not:** reformat the document. No prettifying of markdown keeper did not author, no rewriting of
list markers, quote style or line endings, no reordering or reflowing of YAML.

#### FR-108: Wikilinks and backlinks

**Requirement:** `[[note]]` links autocomplete over the index by title, alias and path. Enter on a
target that does not exist creates the note using FR-98's naming rules, inserts the link and leaves
the caret where it was. A backlinks list sits at the foot of the editor.

**Acceptance (observable):**
- Autocomplete opens within one keystroke of `[[` and ranks by recency, then by match quality.
- Create-on-Enter produces the file immediately — an agent or the sync engine sees it at once — without
  navigating away from the note being written.
- Links continue to resolve after the target is renamed (FR-97), and the backlinks list of a note
  updates within the NFR-29 window when another note links to it.
- The written link format is one Obsidian resolves unchanged.

**Must not:** block typing while the autocomplete query runs; write a link form Obsidian cannot follow;
rewrite links in other notes as a side effect of anything other than an explicit rename action.

#### FR-109: Links to files in the same synced folder

**Requirement:** A note can link to a file elsewhere inside the same sync profile — not merely inside
the vault subfolder — using a relative path. keeper opens it with the OS handler or reveals it in the
file manager.

**Acceptance (observable):**
- A link such as `../invoices/2026-07.pdf` inside the same profile resolves and opens.
- A link resolving outside the profile root renders as plain text with an inline note that it lies
  outside the synced folder, and is not opened.
- A link to a missing file renders as broken with the path visible, not as a silent no-op.

**Must not:** open a path outside the profile root; follow a symlink out of the profile root; copy the
target into the vault.

#### FR-110: Attachments

**Requirement:** Pasting or dropping an image writes it into `attachments/` with a collision-safe name
and embeds a standard Obsidian-resolvable reference. Assets render by way of the `keeper-note://`
scheme; bytes never cross IPC.

**Acceptance (observable):**
- Pasting from the clipboard produces a file on disk and a rendered image, and the note opens
  identically in Obsidian.
- A large image renders without any part of it appearing in an IPC JSON payload.
- A request for a path that resolves outside the vault root is refused, including via `..` segments
  and symlinks.
- An attachment whose file has been deleted renders as a broken embed showing the path.

**Must not:** base64 anything over IPC; overwrite an existing attachment; serve a byte from outside the
vault root.

#### FR-111: Mermaid

**Requirement:** Fenced `mermaid` blocks render inline. A diagram that fails to parse renders its error
alongside its source text.

**Acceptance (observable):**
- A valid diagram renders inline in the editor and in any read view.
- A syntax error shows the parser message and the original fence content — the note's other content is
  unaffected (UX-DR44).
- The mermaid bundle loads only when a note containing a mermaid fence is opened; the capture panel
  never loads it.
- Rendering does not block editing: the surrounding text stays interactive while a diagram renders.

**Must not:** blank the note or any part of it on a render failure; load mermaid in the capture window;
render on the critical typing path.

### 4.5 Cohabitation with agents and other machines (FR-112–FR-114, FR-116)

#### FR-112: External writes applied live

**Requirement:** A write by an agent, another editor or another machine is detected and applied. With a
clean buffer the change is applied live with a fading highlight. With a dirty buffer, hunks that do not
overlap the user's edits merge and an inline diff bar appears; hunks that overlap fall through to the
conflict path (FR-116). Never a lock, never a modal.

**Acceptance (observable):**
- The NFR-29 latency bar holds for create, modify, delete and rename.
- A write performed as write-temp-then-rename — the ordinary behaviour of most editors and many agents
  — is treated as a modification of the destination, not as a delete followed by a create.
- Any event the watcher misses is reconciled on window focus and by the periodic backstop rescan.
- The caret position and the selection survive an applied external write.

**Must not:** lock the file; show a modal; discard unsaved text; merge overlapping hunks by guessing;
treat rename-into-place as a deletion.

#### FR-113: Agent-change review

**Requirement:** An agent change is never silent (UX-DR39). Notes changed by another origin since the
user last read them are marked unread; the tray glyph carries an indicator while any exist (FR-102);
opening such a note offers the diff of that change, and Accept clears the mark. When many notes change
at once, a batched "changed while you were away" list groups them, with Accept-all.

**Acceptance (observable):**
- An agent writing five notes overnight produces one tray indicator, five unread rows and one batched
  list; accepting one clears one, accepting all clears the indicator.
- The user's own local writes are never marked unread.
- Read state is per-device and is stored in keeper's cache, not in the vault. Losing the cache treats
  the current state as read rather than marking the entire vault unread.
- Origin is derived from the sync engine's commit trailers: a change is another origin's when its
  device identity differs from this installation's or its source is `bot`.

**Must not:** apply another origin's change with no trace anywhere; store read state in the vault where
it would sync and conflict; require the main window to be open for the indicator to appear.

#### FR-114: Per-note history with provenance

**Requirement:** A note's history is projected from the sync engine's commit log for that path: time,
device label, origin host, source (watch, manual, cli, bot) and a diff against the preceding revision.

**Acceptance (observable):**
- The revision list matches the repository's history for that path, following renames.
- A note that has not yet been committed shows an honest not-yet-committed state, not an empty list.
- Opening history on a note with thousands of revisions pages rather than loading all of them, and does
  not block the UI.
- Rendering a revision reads history only — the working tree is untouched, and nothing is checked out.

**Must not:** create a parallel history store; mutate the working tree to display a revision; present a
provenance field the commit trailers do not actually contain.

#### FR-116: Conflicts as first-class rows

**Requirement:** A conflict — a surviving copy produced by the sync engine's existing model — is a
first-class row in the note list with a conflict badge, resolved inside the editor: the two versions
side by side, keep-mine, keep-theirs or keep-both-hunks, after which the losing copy is removed.

**Acceptance (observable):**
- An induced conflict produces exactly one conflict row, not two ordinary rows plus an unexplained file.
- Resolving writes one file, removes the conflict copy, and leaves the pre-resolution content
  retrievable from history (NFR-30).
- The conflict copy remains visible in the physical lens (FR-106) for as long as it exists on disk —
  keeper does not hide what is there.
- A conflict on a note that is currently open surfaces in that editor rather than only in the list.

**Must not:** auto-resolve by picking a side; delete a conflict copy without a recoverable copy existing
first; leave a resolved conflict's file on disk; invent a note-specific merge algorithm alongside the
engine's existing model.

### 4.6 Cadence (FR-115)

#### FR-115: Auto-sync cadence for notes vaults

**Requirement:** Notes vaults sync themselves. A local commit fires after a short idle period,
coalescing a burst of edits into one commit; a push fires on an interval or on window blur; both are
force-flushed when the window hides and when the application quits. On by default for notes profiles,
with the values per-vault (FR-120), driven by the existing supervisor tick.

**Acceptance (observable):**
- Two minutes of continuous typing produces a small bounded number of commits — not one per keystroke,
  not one per tick.
- Hiding the window flushes the pending commit within one tick; quitting flushes it and attempts the
  push, bounded by a timeout so exit is never blocked indefinitely.
- A file still being written is not committed mid-write: the engine's existing quiescence and stability
  rules apply unchanged.
- Turning the cadence off leaves the profile behaving exactly as a non-notes profile does.

**Must not:** introduce a second scheduler or a second watcher; commit a partially written file; push on
every keystroke; leave the user's last sentence uncommitted at quit when a commit was possible.

### 4.7 Should tier (FR-123–FR-124)

*Specified now, scheduled last. The phase may ship without them, but only by the explicit decision
§9 requires — not by silent omission.*

#### FR-123: Table and board lens *(Should)*

**Requirement:** The same note set rendered as a table whose columns are frontmatter fields, or grouped
into a board by the values of one field. Editing a cell or dragging a card writes that frontmatter key.
The column set and grouping are saveable as a space (FR-105).

**Acceptance (observable):**
- Adding a column for a field some notes lack shows empty cells for those notes; it never filters them
  out.
- Dragging a card between columns changes exactly one frontmatter key in one file.
- The table is virtualised and holds the NFR-28 paint bar at 10 000 rows.
- Switching to this lens keeps the open note open (UX-DR41).

**Must not:** become the default lens; require a declared schema — fields are discovered from the notes,
never registered; write a field into a note merely because a column displays it.

#### FR-124: Sticky torn-off note windows *(Should)*

**Requirement:** A note can be torn off into a small always-on-top window; several may exist at once.
Each is an ordinary editor over the same file and participates in FR-112 like any other buffer. Position,
size and the set of open stickies survive a restart.

**Acceptance (observable):**
- Two stickies on the same note, and the main editor, stay consistent with one another.
- Closing the main window leaves the stickies working.
- Each sticky window's IPC surface is scoped by its own least-privilege capability declaration; without
  one it can invoke nothing, and that is the tested default.
- A sticky whose note is deleted externally shows the deletion rather than holding a phantom buffer.

**Must not:** hold note state the main process does not have; require the main window to exist; be
created by a code path that can produce an uncapabilitied window.

## 5. Non-functional requirements

*NFR-27–NFR-30 continue §7's numbering. Measured on the reference hardware of the existing bars, release
build, with a generated fixture vault where a size is named.*

- **NFR-27 Capture latency.** Quick capture is visible and focused within **300 ms** of the hotkey on a
  warm process, and the first keystroke is never dropped.
  *Measured:* a `tracing` span from entry of the global-hotkey handler to the capture webview reporting
  focus, p95 over 100 invocations on the reference machine; plus a keystroke-integrity test that injects
  a character within 10 ms of the hotkey and asserts it appears in the persisted buffer. The panel is a
  statically declared, already-loaded hidden window with a plain text area, so no bundle load sits on the
  measured path — a regression that puts one there fails this bar by construction.

- **NFR-28 Scale.** A **10 000-note** vault indexes cold in under **5 s**, the list paints in under
  **100 ms**, and steady-state watch cost is one `lstat` per changed path — not per note.
  *Measured:* a generated 10 000-note fixture (realistic frontmatter, tags and links) in a repeatable
  benchmark; cold index timed with the cache removed; list paint timed from IPC response to rendered
  frame; watch cost asserted by an instrumented stat counter over a ten-minute idle window during which
  one file is touched — the counter must be O(changed paths), and a count proportional to vault size
  fails the bar regardless of wall-clock time. The measured retrieval time at this size is recorded at
  release and is the trigger condition for revisiting the search decision (§6).

- **NFR-29 External-write latency.** An external write to a note is reflected in the UI within **1 s**.
  *Measured:* an integration test in which a separate process performs each of create, modify,
  rename-into-place, rename and delete, asserting the corresponding view-model change arrives over the
  channel within 1 s, p95 over 50 runs per operation. Rename-into-place is measured separately because it
  is the operation most likely to regress into a delete-then-create.

- **NFR-30 No unrecoverable loss.** No keeper code path deletes or overwrites a note body without leaving
  a recoverable copy — a commit or a conflict copy. Data loss is the one unacceptable failure.
  *Measured:* an induced-failure matrix in which each of — process kill mid-write, external write against
  a dirty buffer, overlapping-hunk fallthrough, conflict resolution, template application, archive,
  rename, sticky-window close during an edit — is asserted to leave the pre-change content retrievable
  from history or from a surviving copy. Structurally enforced as well: every write to a vault path goes
  through one guarded writer, and a test asserts no other module writes to a vault path. This bar extends
  the existing no-silent-loss rule to notes and is release-gating rather than advisory.

## 6. Scope boundaries

### 6.1 Must (this phase)

FR-94–FR-122 and NFR-27–NFR-30 in full. In product terms: the notes flag and the vault model; the vault
writer with journal and dated-inbox naming; the global-hotkey capture panel; the tray items; the filtered
list with tag and space filtering; the live-preview editor with typed properties; external-write live
refresh; the auto-sync cadence; the multi-vault switcher; wikilink and tag autocomplete; templates and
journal; mermaid; attachment paste and local-file links; and agent-change marks with per-note history and
diffs.

### 6.2 Should (specified, scheduled last)

FR-123 (table and board lens) and FR-124 (sticky windows). Both are fully specified in §4.7 so they can be
built without a further planning pass, and both are deliberately last because neither is required for any
of the eight jobs in §2 to be satisfied.

### 6.3 Should (from the divergent session, deliberately unnumbered this phase)

- **Capture from a chat message.** The strongest strategic feature in the phase and still not scheduled,
  because it needs a message-source link format that outlives it, and because the capture surface it
  depends on (FR-101) must exist and be trusted first. Forward compatibility is bought now, cheaply: the
  `source` frontmatter key is reserved in §7 and keeper reads it from day one, so the eventual feature is
  a new writer over an unchanged format.
- **A dedicated attachments browser.** The physical lens (FR-106) already shows `attachments/` and the
  OS file manager already browses it. A second browser earns its place only on evidence.
- **A backlinks *pane* separate from FR-108's foot-of-editor list.** The list answers the question; the
  pane is layout, and belongs to the experience document if it is ever wanted.

### 6.4 Could

Calendar lens; transclusion; graph view; publishing a note into a Matrix room; restoring a revision as a
one-click action (FR-114 ships read and copy; restoring is an ordinary edit the user makes with the
copied text).

### 6.5 Won't (this phase)

Vault encryption; a real full-text engine; a plugin API; a notes surface on the phone shell.

### 6.6 Out of scope, with the reasoning

Each exclusion below is a decision, not an omission.

- **Vault encryption.** The vault is an ordinary folder on a disk the user already trusts, and at-rest
  encryption is a property of the volume, not of a note application. Encrypting it would break §7's entire
  contract — Obsidian could not read it, an agent could not write it — and the phase's central claim is
  that the files are the product. Revisit only if a genuinely untrusted remote or shared vault becomes a
  use case, at which point it is a sync-engine decision, not a notes decision.
- **A real full-text engine.** FR-118's bounded scan answers a personal vault in tens of milliseconds and
  cannot be stale. An index adds an invalidation bug class — the exact class this phase spends FR-96
  avoiding — for no measured gain at the sizes in evidence. The revisit trigger is concrete: the NFR-28
  measurement exceeding the five-second retrieval job at the release-recorded vault size.
- **A plugin API.** A plugin surface is a permanent API commitment and a new security boundary, and
  nothing in the eight jobs needs one. Because the vault is plain files, an "extension" here is a script
  the user runs against the folder — which already works and which keeper watches (FR-112).
- **Notes on the phone shell.** iOS has no folder sync, so a notes surface there would be a UI over
  nothing. FR-122 exists precisely so the surface is absent rather than broken; when a mobile shell gains
  folder sync it gains notes by flipping a flag.
- **Publishing a note into a Matrix room.** It crosses the notes/messaging boundary and touches the send
  path, which is governed by the explicit-approval invariant (FR-41). That invariant is a PRD-level
  guardrail; a notes feature must not erode it as a side effect. It is a real future feature and deserves
  its own decision.
- **Transclusion.** Embedding one note inside another multiplies the rendering, watching, diff and
  conflict surface of every other requirement in §4. It is the most expensive Obsidian feature to match
  and none of the eight jobs needs it.
- **Graph view.** The question users actually ask is "what links here", and FR-108's backlinks answer it
  in the place where it is asked. A graph is a demo.
- **Calendar lens.** The journal (FR-99) already answers the daily-notes job. A calendar is a second lens
  over the same indexed data, cheap to add later, and load-bearing for nothing now.

## 7. Interoperability contract

This section is the contract a person using Obsidian — and an agent holding nothing but a text editor —
can rely on. It is testable (§9) and it is not allowed to drift.

### 7.1 On-disk layout

```
<sync profile root>/
├── notes/                      ← the vault (path configurable, FR-94)
│   ├── 2026-08-02-a-thought.md ← notes live flat at the vault root
│   ├── attachments/            ← images written by paste and drop (FR-110)
│   ├── journal/
│   │   └── 2026/
│   │       └── 2026-08-02.md   ← path template configurable (FR-99, FR-120)
│   ├── templates/              ← ordinary notes used as templates (FR-100)
│   ├── spaces/                 ← saved queries, one note each (FR-105)
│   ├── .obsidian/              ← Obsidian's. keeper never reads or writes it.
│   └── .keeper/                ← keeper's index cache. Never committed. Safe to delete.
└── … the rest of the synced folder, reachable by file link (FR-109)
```

Nothing in this tree is required to exist. A vault containing one flat `.md` file and nothing else is a
valid vault; the subfolders are created when a feature first needs one.

### 7.2 Frontmatter keys keeper writes

keeper claims **five** unprefixed keys on ordinary notes, and two more on space notes. Unprefixed, not
`keeper_`-prefixed, because Obsidian tolerates arbitrary keys and renders them in its own properties UI,
and because `tags` and `pinned` are already community conventions — a prefix would buy nothing and cost
readability.

| key | type | written when | meaning |
|---|---|---|---|
| `id` | ULID string | keeper first writes the note (FR-97) | durable identity; links, pins and marks resolve by it |
| `tags` | list of strings | the user adds a tag from the properties panel (FR-104) | frontmatter tags; inline `#tags` stay inline and are never migrated here |
| `pinned` | boolean | the user pins or unpins (FR-119) | floats the note to the top of every list |
| `archived` | boolean | the user archives or unarchives (FR-119) | excluded from the default lens; the file does not move |
| `created` | ISO 8601 date-time | at creation only | creation time; never rewritten afterwards |

On a space note (FR-105), two more:

| key | type | meaning |
|---|---|---|
| `space` | boolean | marks the note as a space definition |
| `query` | mapping | the declarative query: `tags`, `not_tags`, `path_prefix`, `field` comparisons, `after`, `before`, `origin`, `text` |

There is deliberately **no `updated` key**. Modification time and the commit history already answer it,
and maintaining one would rewrite the file on every save and put a diff in every commit.

**Reserved, read but not written this phase:** `source` — a structured reference to where a note came
from. keeper reads it and will show a provenance affordance for it; the capture-from-a-message feature
(§6.3) is its first writer. It is reserved now so that feature costs no format migration.

### 7.3 Keys keeper reads but never writes

`title` (when present it wins over the first line for display), `aliases` (fed into wikilink
autocomplete), and **every other key in the document**. Any key keeper does not own is preserved
byte-for-byte, in its original order, with its original quoting and any comments intact (FR-107). A note
that passes through keeper's properties panel differs only in the keys the user actually edited.

### 7.4 What keeper will never touch

- `.obsidian/` and everything beneath it — never read, never written, never inspected (FR-121).
- The location of any file. keeper creates files where the layout says; it moves a file only when the
  user invokes an action whose name says it moves a file.
- The user's markdown formatting: list markers, emphasis style, heading style, line endings, wrapping,
  trailing whitespace outside the region actually edited (FR-107).
- Anything outside the sync profile root, including through a link or a symlink (FR-109, FR-110).
- The user's tag syntax: a frontmatter tag stays in frontmatter, an inline tag stays inline (FR-104).

### 7.5 What an agent with only a text editor can rely on

- Create a `.md` file anywhere in the vault → it appears in keeper within 1 s while keeper is running
  (NFR-29), and on the next scan otherwise; it is committed within the vault's cadence (FR-115).
- Write frontmatter above the body → the keys appear in the properties panel, typed by value (FR-107).
- Write `#tag` or `#a/b` in the body → the tag tree and every tag filter update (FR-104).
- Write `[[Other note]]` → the link resolves and the target's backlinks list gains a row (FR-108).
- Write a note under `spaces/` with `space: true` and a `query` mapping → a new space appears (FR-105).
- Set `pinned: true` or `archived: true` → the note's placement changes (FR-119).
- Write with write-temp-then-rename → it is treated as a modification of the destination, which is the
  supported and preferred way to write a file that may be open in keeper (FR-112).
- Expect the user to *see* that you wrote: the note is marked unread, the tray shows an indicator, and
  the user is offered your diff (FR-113). There is no way to write into a keeper vault invisibly, and
  that is a feature.

The agent's obligations are three: do not write into `.keeper/`; do not change an existing `id`; do not
assume keeper is running.

### 7.6 If keeper is uninstalled

The vault is a folder of markdown files and a git repository, and that is the whole of it. Obsidian opens
it unchanged. There is nothing to export, because nothing was ever imported: notes, journal entries,
templates, tags, links and attachments are exactly the files they always were, and the history is
ordinary git history readable with ordinary git tools.

The only keeper artefact is `.keeper/`, which was never committed and can be deleted. The only keeper
*convention* that outlives it is the space note — and a space note degrades to a short, readable YAML
description of a filter, which is the worst outcome the format allows.

## 8. Risks and mitigations

- **Index/disk drift** — *medium likelihood, low impact by design.* The index can fall behind the
  filesystem through a missed watcher event, an editor-atomic rename, a network filesystem, or a change
  made while keeper was not running. *Mitigation:* the index is a cache and holds no truth (FR-96); the
  watcher is backed by a periodic rescan and by reconciliation on window focus (FR-112); the NFR-29 test
  covers create, modify, rename-into-place, rename and delete separately. Worst case is a stale row until
  the next reconcile — never a lost file, because no user-visible state lives only in the index.

- **Commit storms** — *medium-high likelihood if unmanaged, medium impact.* A note editor writing on every
  keystroke against a git-backed folder produces an unusable history and constant sync churn.
  *Mitigation:* an idle debounce that coalesces a burst into one commit, push on an interval or on blur,
  force-flush on hide and on quit, all as per-vault knobs consuming the existing supervisor rather than a
  second scheduler (FR-115, FR-120). *Tested:* two minutes of continuous typing must produce a bounded
  commit count, asserted as a number.

- **Concurrent agent writes** — *high likelihood; it is the point of the feature.* The user and the agent
  edit the same note at the same moment. *Mitigation:* never lock, always watch (FR-112); non-overlapping
  hunks merge silently with an inline diff bar; **overlapping hunks do not merge** — they fall through to
  the conflict path (FR-116), where the user resolves them at their own pace with both versions intact.
  Every write passes through one guarded writer so NFR-30 is structural, not a promise.

- **Wayland global-hotkey unreliability** — *high likelihood on several compositors, medium impact.* Global
  shortcut registration is not reliably available under Wayland, and the failure is per-compositor and
  outside keeper's control. *Mitigation:* the hotkey is not the only path — the tray and the palette both
  raise capture (FR-101, FR-102, FR-117) and the tray is the declared primary surface (§3.5). Where
  registration fails, keeper states the actual reason in settings and does not pretend the binding is
  live. This is an honesty requirement, not a fix, and FR-101's acceptance includes it.

- **Linux tray limitations** — *certain; a constraint rather than a risk.* A Linux tray menu cannot be
  replaced after it is first set; tray click events and left-click menus are not delivered; the
  template-image flag is a macOS no-op, so keeper's pure-black glyph set renders black-on-black on a dark
  panel. *Mitigation:* every notes item exists in the menu built at tray creation, present-and-disabled
  when unavailable (FR-102); a Linux-visible glyph variant selected by target (AD-61); UX-DR43 makes
  legibility an acceptance criterion. *Scheduling consequence:* the remediation precedes the notes items
  that depend on it — the epic document owns that ordering.

- **Mermaid and CodeMirror bundle weight** — *medium likelihood, medium impact.* Two heavyweight libraries
  land in a frontend that currently has neither an editor nor a diagram renderer, against an existing
  cold-start bar. Licensing is not the risk — both are MIT and pass the dependency firewall. *Mitigation:*
  three separate payment points. The capture panel is a plain text area in its own window and loads
  neither (NFR-27 depends on this). The editor is a lazily loaded chunk only the main window ever
  requests. Mermaid is a second lazy chunk requested only when a note actually contains a mermaid fence.
  *Measured:* a size budget on the initial main-window chunk asserted in CI, and the existing cold-start
  bar re-measured with the notes surface present.

- **Vault size growth** — *medium likelihood over years, medium impact.* Attachments in a git repository
  grow it monotonically, and a note vault that accumulates images is the classic case. *Mitigation:* an
  attachment is an ordinary file in a synced folder, so the sync engine's existing large-file tier and
  path filters apply unchanged — this phase adds no new storage story and therefore no new growth story.
  NFR-28's 10 000-note bar is the stated ceiling; FR-118's scan is where exceeding it will hurt first, and
  the measured number recorded at release is the trigger for revisiting the search decision (§6.6).

- **Stamping an identity into a note the user authored elsewhere** — *low likelihood, sharp impact.*
  Writing an `id` into an existing file is a modification, which §3.2 forbids doing unasked. *Mitigation:*
  FR-97's rule — keeper stamps an `id` only on notes it writes, and tracks everything else by path.
  *Accepted consequence, stated rather than hidden:* pins, archive state and unread marks on notes keeper
  has never written are path-keyed and do not survive an external rename. The first keeper write fixes
  it permanently.

- **Space queries as an evaluation surface** — *low likelihood, medium impact.* A query stored in a file an
  agent can write is untrusted input. *Mitigation:* the query is declarative data with a fixed key set,
  evaluated by a bounded matcher in the pure domain — no expression evaluation, no note-supplied regular
  expressions, no unbounded backtracking, and an unparseable clause is reported rather than executed or
  dropped (FR-105).

- **Path escape through the asset scheme** — *low likelihood, high impact.* A note is untrusted content
  that names paths, and `keeper-note://` reads bytes off disk. *Mitigation:* canonicalise-and-contain
  against the vault root before any read (AD-59), with tests asserting that `..` traversal, absolute paths
  and symlinks pointing outside the vault are all refused (FR-110).

## 9. Phase acceptance

Phase 5 is done when every item below is true. Epics 35–39 carry the work; this list is what closes the
phase.

1. **Requirement coverage.** Every one of FR-94–FR-122 maps to a shipped story and is demonstrated on
   both macOS and Linux. The tray and hotkey requirements (FR-101, FR-102) are demonstrated separately on
   each, not once and assumed.
2. **Measured bars.** NFR-27, NFR-28, NFR-29 and NFR-30 are green on reference hardware with the numbers
   recorded in the release notes — including the measured retrieval time at the NFR-28 vault size, which
   is the trigger value for the §6.6 search revisit.
3. **The end-to-end gate.** Binary and demo-able, on both operating systems: press the hotkey over a
   full-screen window → type two sentences → press Escape → the file exists in the vault within the
   commit debounce → it is pushed within the cadence → the second machine has it → an agent edits it there
   → the tray shows the indicator → one click shows the diff → Accept clears it. This single sequence
   exercises FR-98, FR-101, FR-102, FR-112, FR-113, FR-114 and FR-115 and is the phase's headline.
4. **Interoperability assertion (§7).** After a session exercising every notes feature: `.obsidian/` is
   byte-identical with untouched modification times; the cache directory appears in no commit; every
   frontmatter key keeper does not own is preserved byte-for-byte; the vault opens in Obsidian with no
   repair, no missing file and no moved file.
5. **Uninstall test.** Remove keeper, delete the cache directory, open the vault in Obsidian: every note,
   link, tag, journal entry, template and attachment works, and every space note is readable as plain
   YAML by a human.
6. **Data-loss matrix.** NFR-30's induced-failure matrix passes in 100% of runs, and the test asserting
   that only the guarded writer writes to vault paths is in CI.
7. **Linux parity.** Every tray-reachable notes affordance is present in the first-built tray menu and
   legible against a dark panel; where the compositor refuses the global hotkey, the state is honest and
   capture remains reachable from the tray and the palette.
8. **Project gates green.** `check:core-tauri-free` and `check:core-sync-free` still pass with the notes
   domain in `keeper-core`; clippy at `-D warnings`; no `.unwrap()` on a production path; Biome clean with
   no `any`; generated bindings check clean; the dependency licence firewall clean with mermaid and
   CodeMirror 6 confirmed MIT; and the egress inventory diff for the phase is empty — notes add no network
   destination beyond the profile remotes already disclosed.
9. **Documentation.** The vault layout, the §7 frontmatter contract, an agent-authoring guide, the cadence
   settings and the Linux caveats are written and accurate — the §7 contract in particular is documentation
   an external tool author can implement against.
10. **The Should tier is decided, not forgotten.** FR-123 and FR-124 are either shipped and accepted, or
    deferred by a recorded decision naming the reason. The phase does not close with them silently missing.
