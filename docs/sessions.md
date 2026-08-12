# Sessions

keeper's board over LLM work sessions. One folder per session, inside a folder keeper
already synchronises — the `60-sessions/` zone layout the drives themselves define.

This is the operator document: what is on disk, what keeper reads and writes, what it will
never touch, and what to do when something is wrong. The design reasoning lives in
`_bmad-output/planning-artifacts/` (PRD phase 7, `ARCHITECTURE-SESSIONS-PHASE7.md`,
`EXPERIENCE-SESSIONS.md`); the requirement numbers below point there.

## The one idea

**A sessions root is a folder you already sync, plus a flag.**

There is no picker, no import, no session store and no second place to configure anything.
Settings → Sync → a folder → *This folder has sessions*. keeper lists sessions from a
subfolder of that folder (`60-sessions` by default) and syncs them with everything else
there. keeper **adopts** the layout that is already on disk; it does not create one.

Everything else follows from that: sessions sync because the folder syncs, they have
history because the folder is a git repository, and every visible fact — status,
freshness, tags, lineage — is derivable from the files alone. Anything a Finder edit
could desync, keeper does not store.

## On disk

```text
<your synced folder>/
  60-sessions/                         # the zone — the subfolder you named
    README.md                          # the zone's own contract (the drives wrote it)
    AGENTS.md                          # agent rules for the zone
    _template/                         # the skeleton a new session copies. Yours; keeper
                                       # copies it and never edits it unasked.
    active/
      2026-08-10-keeper/               # one session: YYYY-MM-DD-<slug>
        README.md                      # THE record: summary, decisions, log, promote table
        workspace/                     # scratch — unversioned, unsynced, READ-ONLY to keeper
        artifacts/                     # promoted output — versioned and synced
        refs/                          # inputs worth keeping
        prompts/                       # reusable prompts, numbered
    archive/
      2025/2025-03-01-taxes/           # finished sessions, filed by close year
    .keeper/                           # keeper's own cache + journal + trash. NEVER synced.
```

### What keeper promises

- **`workspace/` is read-only.** keeper lists it for the freshness signal and never
  writes, commits, syncs or searches it. An edit aimed there is refused with the zone's
  own words and a pointer at promotion.
- **`_template/` and every `_*`/dotted name is never a session.** The scan skips them.
- **keeper never moves a session folder unasked.** Archive, delete and unarchive are
  explicit actions; each is a journaled plan whose irreversible step runs last, so a
  crash mid-verb resumes or rolls back and never leaves a half-moved folder.
- **A write that changes one frontmatter key leaves every other byte identical** — the
  notes writer's promise, kept here for pins, lineage and the promote table.

## The session README

The README's frontmatter is the session's identity, tags, properties and pins, under the
notes three-tier contract: keeper-owned (`id` — a ULID minted on first index; `pinned`;
the `keeper:` namespace), Obsidian-native (`tags`), yours (anything else). Lineage lives
one level under `keeper:` as flow lists:

```yaml
keeper:
  session-continues: [01J4…]
  session-continued-by: [01J6…]
```

Status is the folder's location — `active/` or `archive/<year>/` — never a stored flag.
The board's two freshness signals are derived mtimes: **workspace** (the agent is
iterating) and **record** (README/artifacts/refs/prompts changed). They are never merged;
the distinction is the review loop.

## The verbs

- **New session** — the title, and what to shape it from. The folder is named
  `YYYY-MM-DD-<slug>` with a collision counter, the id is minted, and the README is
  stamped from the pattern's own headings with today's date.

  The **pattern** is the second half of the question, and it is already answered: the
  zone's `_template/` is pre-chosen, and every session in the zone follows it in the
  list, newest change first — because the thing you want to start from is nearly always
  what you were last working in. Under the picker, the preview names every file that
  travels **and** every file that does not, each with its reason. That list is not a
  description of the copy; it is the copy, projected from the same rule the plan is
  compiled from, so what you are promised and what lands on disk cannot drift.

  Choosing the template copies it verbatim. Choosing a **session** is a continuation
  (the zone's preferred reopen): structure only — `prompts/` and `refs/` travel,
  artifacts and workspace contents never do, and the new README is the source's
  headings, empty. `continues`/`continued-by` are written into BOTH READMEs —
  including an archived source — so the lineage is visible to `cat`, Obsidian and the
  agent, not only to keeper.

  A zone with no `_template/` offers its sessions alone; a zone with neither offers no
  picker at all and creates the standard empty skeleton.
- **New like this** — the same door, opened with this row already chosen as the pattern.
  Not a second create verb: the title is still asked and the preview still shown.
- **Log today** — appends `### YYYY-MM-DD — ` under `## Log`, newest last (the zone's
  convention), once per day, and opens the README. `⌘⌥L`, the palette, or the row menu.
- **Pin** — `pinned: true` in frontmatter; pinned sessions sort first in their group.
- **Archive** — per the zone's checklist: workspace emptied (`.gitkeep` stays), folder
  filed under `archive/<current year>/`. Promote what matters first — workspace contents
  do not survive. The confirm dialog says exactly this.
- **Delete** — the folder moves into the zone's `.keeper/trash/<id>/`, workspace and all,
  recoverable. Never an unlink.
- **Unarchive** — one move back to `active/`. Lineage untouched. Prefer a continuation.

## The session's files

A session folder is a small workspace, so the detail browses it as one: a real tree under
**Files**, not a list per section. The zone's own sections come first in the zone's own
order — `artifacts/`, `refs/`, `prompts/`, `workspace/` — each followed by whatever is
inside it, and everything else in the session after them. The sections arrive open and
their subtrees closed, which shows the session's shape without unrolling a `node_modules`
the agent installed. Arrow keys walk it; `Enter` opens a file in the panel beside the
board or toggles a folder.

Each row carries the three facts the Files tab carries, and for the same reason:

- **Its sync mark**, from the same engine answer the Files tab reads. A session lives in
  a synced folder and therefore has a sync story; a row that hid it would let the two
  surfaces disagree about one file. An excluded row says so, in the engine's words.
- **Its size and age**, described rather than named — so a screen reader announces the
  file, then the numbers, rather than reading a name nobody typed.
- **A lock, where keeper will not write**, carrying the fence's own refusal sentence.
  Everything under `workspace/` is locked, including an empty `workspace/` itself.

The verbs on a row are open, open in the default app, and reveal — a review surface, so
there is no selection, no rename and no delete here. Deleting a session's file is a
question about the promote table, not a generic file delete, and creating one inside a
session is not offered at all yet: keeper's create path is the notes vault's, and a
`60-sessions/` zone is the vault's sibling.

Sessions are bounded by their own contract, so the whole tree is read in one pass rather
than one call per folder. A workspace somebody let a package manager into can still
outgrow that: the tree then stops and says so rather than showing a prefix of itself.

## What the session points at

The tree lists what a session *holds*. **References** lists what it *names* — a different
set on purpose, because the zone's own rule is that big files stay in their zone and a
session points at them by path. So the thing that goes wrong is the pointer, and this is
the only place that would ever say so.

Keeper reads the session's README and everything in `refs/` and `prompts/`, and reports
one row per distinct target — six kinds, each with a real test behind it:

- **note** — the target resolves in the vault index, the same resolution a wikilink in a
  note gets.
- **recording** — a note whose frontmatter carries a `session:` key. That is what makes a
  recording a recording; a loose `.m4a` sitting in the session is a *file*, and calling it
  a recording because of its extension would be a guess.
- **file** — a path that exists, looked for beside the session first and then from the
  drive root, so `artifacts/notes.md` and `40-media/clip.mov` both work as written.
- **session** — a path that lands inside another session in the same zone.
- **link** — an external URL, reported without being fetched. Keeper does not know whether
  a website is up, and a red row that only means "no internet" is worse than no row.
- **missing** — the path resolves to nothing.

**Missing sorts first and says what keeper looked for.** "Keeper could not find it" sends
somebody searching four hundred folders; naming both paths it tried usually shows the file
is one `mv` away. The heading states the count, so a session with nothing broken says so
in a sentence instead of making a person read thirty good rows to conclude it.

Pressing a row opens it where that thing belongs: a note or a file in the panel beside the
board, a link in the system browser. A missing row is not a button — there is nothing to
open, and the fix is an edit in the file named on the row.

Two directories are deliberately not scanned. `artifacts/` is a deliverable, so a
reference inside it is the artifact's business, not the session's. `workspace/` is scratch
that dies with the session, and a broken pointer in a file nobody keeps is not worth
reporting. The scan is bounded by total text rather than by file count — the cost here is
parsing markdown — and says so when it stops early.

## Editing

Opening a session opens its README in the same editor every other keeper surface uses —
markdown with live preview, the raw/rendered toggle, mermaid, tables, embeds. Any text
file in the session opens the same way; `workspace/` files open read-only. An agent
editing a file on disk shows up live: the row updates, and an open buffer takes clean
external writes silently or raises the diff bar when dirty — the notes pipeline,
unchanged.

## When something is wrong

**"No sessions folder yet."** No folder is flagged. Settings → Sync → your folder →
*This folder has sessions*. If the subfolder name differs from `60-sessions`, set it in
the same place.

**The board is missing a session that is on disk.** The scan is a cache and is allowed to
be wrong. Press *Rescan* — a full pass over the zone costs well under the 2 s budget.

**A lifecycle verb refused with "changed while this was being planned".** An agent wrote
the README between planning and writing. Nothing was written; run the verb again — the
refusal is the fence working, not a fault.

**A crash mid-archive.** Relaunch. The journal in the zone's `.keeper/` resumes the
remaining steps; every step is idempotent, and the folder move is always last.

**A deleted session is gone.** It is not: `<zone>/.keeper/trash/<session id>/`.

**A reference says missing and the file is right there.** Read the two paths on the row —
they are what keeper tried. A path written from the drive root (`40-media/clip.mov`)
resolves against the synced folder, not against your home directory, and a path with `..`
in it is refused rather than followed out of the folder.

## What is not here yet

The promote panel (per-row promote review with staleness badges), the full notes query
grammar over sessions (`is:`, `origin:`, `field:` — the board's free-text filter covers
title/path/tags/snippet/log today), unread marks and per-file history projections on
rows, capture-into-session-log, the sticky current session in the tray, wikilinking a
session from a note, and creating or deleting a file from inside the session tree. All
specified (FR-229/235/236/241/242/250) and scheduled; none implemented. Sessions on iOS are out of scope with the rest of the sync surface.
