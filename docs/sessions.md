# Sessions

keeper's board over LLM work sessions. One folder per session, inside a folder keeper
already synchronises — the `60-sessions/` zone layout the drives themselves define.

This is the operator document: what is on disk, what keeper reads and writes, what it will
never touch, and what to do when something is wrong. The design reasoning lives in
`_bmad-output/planning-artifacts/` (PRD phase 7, `ARCHITECTURE-SESSIONS-PHASE7.md` for the
first contract and `ARCHITECTURE-SESSIONS-FLAT.md` for the flat one, `EXPERIENCE-SESSIONS.md`);
the requirement numbers below point there.

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
      <name>/                          # a named template — one per way you work
    _spaces/                           # the zone's saved queries, one file each
    active/
      2026-08-10-keeper/               # one session: YYYY-MM-DD-<slug>
    archive/
      2025/2025-03-01-taxes/           # finished sessions, filed by close year
    .keeper/                           # keeper's own cache + journal + trash. NEVER synced.
```

### The two shapes

A session folder follows one of two contracts, and keeper reads both. Neither is
deprecated on a timetable: these are folders on your drives, and a session nobody
migrates has to keep working.

```text
2026-08-10-keeper/          # FLAT — what keeper's own template now writes
  AGENTS.md                 # how to read this folder. The navigation contract.
  about.md                  # the record: summary, decisions, promote table
  2026-08-10-0930-opened.md # a log — one sitting, tagged `log`
  ship-it.md                # a task — tagged `task`, carrying `status` and `order`
  house-style.md            # a prompt — tagged `prompt`
  workspace/                # scratch — unversioned, unsynced, READ-ONLY to keeper
  artifacts/                # promoted output — versioned and synced

2026-08-10-keeper/          # FOLDER — the original, still read exactly as before
  README.md                 # THE record: summary, decisions, log, promote table
  workspace/                #   ⋮
  artifacts/                #   ⋮
  refs/                     # inputs worth keeping
  prompts/                  # reusable prompts, numbered
```

**Flat is one markdown pool.** Every file declares its own kind as a tag —
`about`, `task`, `log`, `prompt`, `ref` — so moving a file never changes what it
is, and a new kind of thing is a new tag rather than a new directory. `refs/`,
`prompts/` and `logs/` do not exist; the five lists you see are saved queries over
the tags. A file declaring none of them is **unfiled**, which is not an error but
does mean nothing surfaces it.

The known cost is real: a flat session opened in Finder is an undifferentiated pile
of markdown until something reads the tags. `AGENTS.md` is the mitigation and is
why it is the one file keeper always writes — it is the navigation contract, written
for whoever, or whatever, is handed the folder with no other context.

`artifacts/` and `workspace/` survive in both shapes because their difference is
about *versioning*, not about kind, and no tag can replace that.

**Which one a folder is** is decided by presence, not absence: a folder holding
`AGENTS.md` or `about.md` is flat. A folder holding both `README.md` and `AGENTS.md`
reads as flat — the safe direction, because the residual README then shows up as an
unfiled file and a half-finished migration is visible rather than merely survivable.
Nothing is stored to say which shape a session is; the files are the truth.

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

## The session's record

`about.md` in a flat session, `README.md` in a folder-shaped one. Same file, same job,
two names — everything below is true of whichever one your session has.

The record's frontmatter is the session's identity, tags, properties and pins, under the
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
iterating) and **record** (the markdown and `artifacts/` changed). They are never merged;
the distinction is the review loop.

**keeper never stamps.** A file it did not author keeps its bytes exactly, so a
hand-written or agent-written markdown file has no `id` and is identified by its path
instead — which means pins on it will not survive a rename, and keeper says so rather
than minting one. Stamping at index time would mean *opening a session mutates every
file in it*: the scan would dirty a real git tree and sync would commit changes nobody
made.

## The verbs

- **New session** — the title, and what to shape it from. The folder is named
  `YYYY-MM-DD-<slug>` with a collision counter, the id is minted, and the record is
  stamped from the pattern's own headings with today's date.

  The **pattern** is the second half of the question, and it is already answered: the
  zone's `_template/` is pre-chosen, its named templates follow, and then every session
  in the zone, newest change first — because the thing you want to start from is nearly
  always what you were last working in. Under the picker, the preview names every file
  that travels **and** every file that does not, each with its reason. That list is not
  a description of the copy; it is the copy, projected from the same rule the plan is
  compiled from, so what you are promised and what lands on disk cannot drift.

  **The new session inherits the pattern's shape.** A flat template begets a flat
  session and a folder-shaped one begets a folder — never a preference asked of you,
  because a session whose files say one thing and whose shape says another is readable
  by neither reader.

  Choosing a template copies it verbatim, minus its record: `about.md` and `README.md`
  are restamped with *your* title and today's date, since copying the template's would
  name every new session after the template. `AGENTS.md` travels untouched — a zone that
  edited its own navigation contract meant it.

  Choosing a **session** is a continuation (the zone's preferred reopen): structure
  only. In a folder-shaped source `prompts/` and `refs/` travel; in a flat one the files
  *tagged* `prompt` and `ref` do, because there is no directory to ask. Logs and tasks
  never travel — they are the previous session's record of its own sittings and its own
  state, and carrying either forward would make the new board a lie the moment it
  opened. Artifacts and workspace contents never travel either.
  `continues`/`continued-by` are written into BOTH records — including an archived
  source — so the lineage is visible to `cat`, Obsidian and the agent, not only to
  keeper.

  A zone with no `_template/` offers its sessions alone; a zone with neither offers no
  picker at all and creates a flat session from keeper's own default: `AGENTS.md`,
  `about.md`, one seed log and one seed prompt. The seeds exist so the log and prompt
  lists are not empty on day one and so the filename convention is visible as an example
  rather than only as a rule — delete them freely, nothing depends on them. A
  continuation gets the contract but not the seeds: it was made from a session that has
  real ones.

  **Keeper composes the seeds; it never copies them.** What a create adds beside the
  pattern is decided by *kind*, not by filename — a seed is named
  `YYYY-MM-DD-HHMM-opened.md`, so two of them written a minute apart share nothing but
  their meaning. If the pattern already supplies a file tagged `log`, that file travels
  and keeper composes none; supply a prompt too and it composes neither. This is why
  putting your own seed log in `_template/` works exactly as you would expect, and why
  keeper's own does not end up beside it.
- **Write keeper's template into this zone** — offered under the picker to a zone with no
  `_template/` of its own. What lands is a **skeleton**: `AGENTS.md` and an `about.md`
  titled `<session title>`, and deliberately not the seeds. A template has no title and
  no minute — freezing either would name every session after the moment you pressed the
  button — so the seeds stay something keeper writes per create, with that session's own
  title. Add your own afterwards if you want different ones; a create will prefer yours.
- **Templates** — a mode on the board, sitting with the status chips rather than as a
  fourth button in the pane header. The chip is a toggle: press it again to come back to the
  session rows, or press a status chip, whichever you reach for. It lists every template the
  zone has — its own `_template/` and each named one — and under each, every file inside it,
  newest change first. That listing is the same walk the picker previews a create from, so
  the room and a new session cannot disagree about what a template holds: a file in a
  subdirectory is a row too, carrying its path within the template. When such a row is too
  narrow for the whole path it gives way on the directory and keeps the filename —
  `prompts/hand-off.md` never shortens to `prompts/pro…`, because the filename is what you
  came to press. The one difference is the create's, not the room's — a new session gets a
  *stamped* record rather than a copied one, so `about.md` is a row here and never under
  *Copies*. Press a file to open it in the panel beside the board: the same editor a
  session's record opens in, because editing a template is editing a file and nothing more.
  A template whose directory has since been removed in Finder lists as empty rather than as
  an error; the list re-reads after every write, and your own edit is not a fault. If the
  read of the zone's templates is refused outright, the room is not drawn at all and
  keeper's own sentence sits in the pane's alert region instead of a spinner that never
  resolves; leaving the room and coming back re-runs the read, and that is the retry.
  The list's own header carries **New template**, which is *Write keeper's template into
  this zone* with a name — the way to gain keeper's skeleton as a named template beside
  the one you already have. Typing the name a named template already has is **refused in
  the list**, without asking Rust: case-insensitively, because the drives this syncs to
  are, and because the command underneath would move that template's own `AGENTS.md` and
  `about.md` into `.keeper/trash/` and write keeper's in their place — recoverable, and
  still not what "one beside it" means. The refusal is on the name as typed, so a name
  that only *folds* onto an existing one (`Interview Kit` where `_template/interview-kit`
  is) does reach the command, and does trash-then-write those two files. The search box and
  the Pinned/Unread filters are hidden in this mode: they filter session rows, and a
  template has no status.
- **Rename a template** — offered on a **named** template's row, and only there. The name
  you type is folded to a directory name exactly as *New template* folds it, the move
  runs as one journaled plan — so the drive gets a single commit with keeper's provenance
  on it, rather than a rename keeper's watcher reads as somebody else's write — and the
  row comes back under its new name after the rescan. A blank name is refused, and so is a
  name with no letters or digits in it: `###` is not a folder name, and keeper will not
  invent one for you. A name that is already taken by **another** template is
  **refused**, not merged and not trashed: *New template* may write over what it finds,
  because keeper's skeleton is what you asked for and the displaced bytes go to
  `.keeper/trash/`, but a rename has no such mandate and burying one template under
  another is not something a keystroke should be able to do. Retyping a name that already
  folds to the directory's own — `Interview` over a directory called `interview` — writes
  nothing and is not an error. The mirror case is a real move: a hand-made
  `_template/Interview/` renamed to `interview` normalises the directory itself, and
  keeper allows it on macOS too, where the destination "already exists" only because it
  *is* the source. A hand-made `Interview Kit` retyped verbatim likewise moves — to
  `interview-kit` — and pressing rename twice after it worked is refused, because the
  template it named has moved. Read the list again rather than trying again.

  **The zone's own `_template/` cannot be renamed**, and that is the point of it. Its
  name *is* the contract: `_template/` is what a create copies from and what the scan
  skips (see *What keeper promises*), and a zone has exactly one of it. Renaming it
  would not give you a differently-named zone template — it would give you a zone with
  none. Name templates *inside* it instead, one per way you work.
- **New like this** — the same door, opened with this row already chosen as the pattern.
  Not a second create verb: the title is still asked and the preview still shown.
- **Log today** — in a folder-shaped session, appends `### YYYY-MM-DD — ` under `## Log`,
  newest last (the zone's convention), once per day, and opens the README. In a flat one
  it writes a new `YYYY-MM-DD-HHMM-<slug>.md` tagged `log` — a sitting is a file there,
  not a heading, so a second log on the same day is a second file rather than a refusal.
  `⌘⌥L`, the palette, or the row menu.
- **Pin** — `pinned: true` in frontmatter; pinned sessions sort first in their group.
- **Archive** — per the zone's checklist: workspace emptied (`.gitkeep` stays), folder
  filed under `archive/<current year>/`. Promote what matters first — workspace contents
  do not survive. The confirm dialog says exactly this.
- **Delete** — the folder moves into the zone's `.keeper/trash/<id>/`, workspace and all,
  recoverable. Never an unlink.
- **Unarchive** — one move back to `active/`. Lineage untouched. Prefer a continuation.
- **Convert to flat** — a folder-shaped session becomes a flat one. Shown only where it
  applies, previewed before anything is written, and described in full under *Migrating a
  folder-shaped session* below.

## The session's files

A session folder is a small workspace, so the detail browses it as one: a real tree under
**Files**, not a list per section — and it comes **first**, above the spaces, the board
and the log, because it is the part you reach for most. The zone's own sections come
first inside it in the zone's own order — `artifacts/`, `refs/`, `prompts/`,
`workspace/`, whichever of them this session has — each followed by whatever is inside
it, and everything else in the session after them. The whole tree arrives expanded: a
session is bounded by its own contract, so preloading its structure costs one read and
saves every click. (The Files *pane* is lazy per expand for the opposite reason — one of
those folders may be a pendrive with a hundred thousand files on it. The asymmetry is
deliberate.) Arrow keys walk it; `Enter` opens a file in the panel beside the board or
toggles a folder.

Each row carries the three facts the Files tab carries, and for the same reason:

- **Its sync mark**, from the same engine answer the Files tab reads. A session lives in
  a synced folder and therefore has a sync story; a row that hid it would let the two
  surfaces disagree about one file. An excluded row says so, in the engine's words.
- **Its size and age**, described rather than named — so a screen reader announces the
  file, then the numbers, rather than reading a name nobody typed.
- **A lock, where keeper will not write**, carrying the fence's own refusal sentence.
  Everything under `workspace/` is locked, including an empty `workspace/` itself.

The verbs on a row are open, open in the default app, reveal, and delete. **New file**
sits on the tree's header and makes a `.md`, `.csv` or `.json` in the folder you have
selected, with **New log** and **New prompt** beside it for the two you make constantly —
those write a correctly named, correctly tagged file rather than an empty one, because a
log you have to name and tag by hand is a log you write later or not at all. Each lands
where your session's shape keeps that kind: a prompt at the root of a flat session, in
`prompts/` in a folder-shaped one; **New log** in a flat session writes a file, and in a
folder-shaped one appends a heading to `README.md`, which is where that shape's log is.

A delete moves the file into the zone's `.keeper/trash/`, never an unlink, and
`workspace/` refuses every write with the fence's own sentence. There is still no rename
and no move **here**: renaming a file whose id is its path silently breaks the pins pointing
at it, and that is a conversation this surface has not had yet. The exception is the
Templates room, and it is an exception for a reason rather than an inconsistency — nothing
points at a template's files, and a create copies them rather than referencing them, so
there is no link graph for a rename to break there.

Sessions are bounded by their own contract, so the whole tree is read in one pass rather
than one call per folder. A workspace somebody let a package manager into can still
outgrow that: the tree then stops and says so rather than showing a prefix of itself.

## What the session points at

The tree lists what a session *holds*. **References** lists what it *names* — a different
set on purpose, because the zone's own rule is that big files stay in their zone and a
session points at them by path. So the thing that goes wrong is the pointer, and this is
the only place that would ever say so.

Keeper reads the session's record and its pointer files — everything in `refs/` and
`prompts/` in a folder-shaped session, the whole root markdown pool in a flat one — and
reports one row per distinct target, six kinds, each with a real test behind it:

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

**Adding one** is a picker rather than a text field, because a reference typed by hand is
a path that is right today. It searches three sources at once — files on the synced
drive, notes, and recordings — by name and by tag, and it writes the pointer in the
session's own convention with the kind already classified, so a reference lands as the
row it will always be rather than as a string keeper has to guess about later.

You choose the file it is written into, and keeper offers one: `references.md` at the
root of a flat session, `refs/references.md` in a folder-shaped one — the same rule the
spaces' create follows, so the reference you just added is in the References space rather
than in a file nothing reads. The file is created, tagged `ref` and seeded with
frontmatter if it is not there yet.

One case gets a question instead of a write: a target inside `workspace/`. That folder is
scratch and dies with the session, so a pointer into it is a dangling link with a delay
on it. Keeper offers to copy the file into `artifacts/` and reference that instead — an
offer, not a rule, since a deliberately temporary pointer is a thing a person is allowed
to want.

Two directories are deliberately not scanned. `artifacts/` is a deliverable, so a
reference inside it is the artifact's business, not the session's. `workspace/` is scratch
that dies with the session, and a broken pointer in a file nobody keeps is not worth
reporting. The scan is bounded by total text rather than by file count — the cost here is
parsing markdown — and says so when it stops early.

## Editing

Opening a session opens its record in the same editor every other keeper surface uses —
markdown with live preview, the raw/rendered toggle, mermaid, tables, embeds. Any text
file in the session opens the same way; `workspace/` files open read-only. An agent
editing a file on disk shows up live: the row updates, and an open buffer takes clean
external writes silently or raises the diff bar when dirty — the notes pipeline,
unchanged.

## Spaces, tasks and the log

Below Files, a flat session shows three more surfaces. All three read the same pool, and
none of them stores anything except the fold you leave a space in.

**Spaces** are the zone's saved queries, one markdown file each under
`60-sessions/_spaces/`. Five ship by default — About, Tasks, Log, References, Prompts —
and they are ordinary files: rename them, reorder them, delete one, write your own. The
query language is the notes vault's, the same grammar and the same chip editor, because a
`tag:` that meant one thing in notes and another in sessions would be a trap. A broken
query selects nothing and says so; an unreadable sort still runs the query.

**A space you can write into.** A space whose query names exactly one kind carries its
own **New note** button — Tasks, Log, References and Prompts do, and so does any space you
write that asks for one of those tags. The button is always there, not revealed by
hovering: it is the one thing a space exists to let you do. Edit and delete stay on
hover, because they are maintenance. The file is tagged as it is created, so it lands in
the space you made it from rather than in Unfiled, and it opens immediately.

**Where the new file goes is your session's shape.** A flat session keeps everything in
one pool, so the file is written at the session root. A folder-shaped session keeps
references in `refs/` and prompts in `prompts/`, so that is where they are written —
exactly the directories that shape reads its pool from, which is what makes the new file
appear in the space you pressed. The directory is *where keeper puts it*; the tag is what
makes it a reference. A file dropped into `refs/` by hand with no `tags: [ref]` is still
unfiled, in either shape, and keeper will say so rather than guessing from the folder.

Two spaces are told plainly that they have no button, in one line where the button
would have been — both of them a folder-shaped session's:

- **Tasks.** That contract has no tasks file — the board is the flat one's — so there is
  nowhere to put one. *Convert to flat* is how such a session gets tasks.
- **Log.** That shape's log is a `### ` entry under `## Log` inside `README.md`, not a
  file at all, and **New log** on the Files header already writes one there.

Three others simply have no button, with nothing said, because the reason is their own
query rather than your session: **About** in either shape (a session has one record, and
a second `about.md` would leave keeper with two answers about what the session is), a
space that asks for two things at once (`tag:log date:today` has no one file that still
satisfies it tomorrow), and one whose query keeper cannot read — that one already carries
its own fault line. Absent rather than present and refusing, in all three.

**What a row opens as.** A row is a single click and behaves like one everywhere: it
replaces what the panel you pressed in was showing rather than adding a panel beside it.
It opens **the file**, in keeper's file viewer, which is the same target the tree and the
Files pane use.

**A session file is not a vault note, and cannot be made into one.** keeper refuses to
let one folder be both a notes vault and a sessions zone — in either direction, whichever
contains which — because two indexers claiming one tree is a configuration nobody can
reason about afterwards. Try it and the profile is rejected with those words: *"one folder
cannot be both a vault and a sessions zone"*. So there is no arrangement in which a space
row is a note, and nothing to configure your way into. Notes and sessions meet through
**references** instead: add a reference to a note from a session, and the wikilink is the
link between them.

**A space you can shut.** A space folds and unfolds from its own title — the title is the
control, not a chevron beside it: the header of a ~208px card already carries the space's
glyph, its count and three buttons, and a fourth would spend the pixels the name is
already truncating out of. Folding hides the rows, not the section. The count stays, the
fault sentence stays, and so do **New note**, edit and delete, so a space you have shut is
still a space you can write into, and a count over no rows is how a folded space reads as
folded rather than empty.

**What an untouched space does is a setting.** Settings → Sessions → *Start spaces folded*,
off unless you turn it on. It decides only the spaces you have never folded or unfolded
yourself; the ones you have keep your answer, so turning it on does not shut the three you
just opened. The key is `sessions.spaces_folded`, user-global like most of them, so a
`[settings]` table in `~/.keeper/keeper.toml` — or in the main sync folder's — can set it
like any other user-global key, and a file that sets it keeps winning: the switch shows as
file-controlled instead of quietly losing to the next toggle. The section is not there
until some folder is flagged as a sessions zone, because before that there are no spaces
to arrange.

**Where the fold is kept.** Per space, in a cookie in keeper's own webview — not in the
zone, not in `keeper.toml`, not in any file on your drives. It survives a restart and it
does not sync: folding is a lens you chose rather than a fact about the session, so
arranging spaces on the laptop leaves the desktop exactly as it was, and arranging them
twice is the price of wanting both. keeper remembers the thirty-two you most recently
folded or unfolded and forgets the oldest beyond that, and the same space in two synced
folders folds apart. Lose the lot — a cleared cookie jar, a new machine — and you have
lost the arrangement and nothing else; every space is one press from open again.

**Tasks** is a board of four columns — in preparation, to do, done, deferred — over the
files tagged `task`. A card's column is its `status:` and its position is its `order:`, a
fractional number, so dragging one card rewrites one file rather than renumbering
everything below it. A `status:` keeper cannot read is shown as unreadable rather than
quietly filed under "to do".

**The log** is last, because it is the thing you scroll to rather than the thing you act
on. Newest first, folded from the files tagged `log`.

All three are also **markdown widgets** — `> [!board]`, `> [!log]` and `> [!refs]` in any
note, not only in a session. Callout syntax rather than a fence, so a note carrying one
degrades to a labelled quote block in Obsidian or on GitHub instead of a wall of grey
source.

## Migrating a folder-shaped session

**Sessions → the row menu → Convert to flat.** Never automatic, and never on a timetable:
this rewrites files on your drive, so it is a verb you press.

The preview shows every write before any of them happens — `about.md` from the README's
own prose, one `YYYY-MM-DD-HHMM-<slug>.md` per `### ` log entry, and every file hoisted
out of `refs/` and `prompts/` with its kind tag added. The README is not deleted; it is
replaced by a three-line signpost, because every link and agent instruction that ever
pointed at a session pointed at its README.

It is a journaled plan like every other lifecycle verb: `AGENTS.md` is written only after
everything the flat shape needs exists, so there is no moment where the session reads as
flat but its logs are missing, and the two directory removals run last. Run it twice and
the second run does nothing — the answer for an already-flat session is "no plan", which
is idempotence stated in the type rather than promised in prose.

## Finding things

`⌘F` searches inside the document you have open — the editor's own find, so it reaches
text below the fold that the rendered view has not drawn yet.

`⌘⇧F` searches everything: messages, notes, and session files. It opens on whatever you
were already looking at, and each hit opens the file at the same path a file row would —
one search, one open path, no second answer to "where is that file".

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

The promote panel (per-row promote review with staleness badges), unread marks and
per-file history projections on rows, capture-into-session-log, the sticky current
session in the tray, and wikilinking a session from a note. All specified
(FR-229/235/236/241) and scheduled; none implemented. Sessions on iOS are out of scope
with the rest of the sync surface.

Two things that were on this list are now shipped, and are listed here only because the
shape of what they do is worth knowing: **the notes query grammar reaches sessions**
through spaces — a space runs the same evaluator over the session pool that a note space
runs over the vault, so `tag:`, `is:`, `origin:` and `field:` all mean there what they
mean in notes — while **the board's search box remains free text** over
title/path/tags/snippet/log, because the board searches sessions and a space searches
files inside one. And **files can be created and deleted from inside the session tree**;
renaming and moving still cannot, which is deliberate — a rename is a link-rewriting
problem, not a file-system one, and half of it would be worse than none.
