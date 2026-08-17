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
                                       # copies it and never edits it unasked — a
                                       # placeholder is filled in on the way INTO the new
                                       # session, never here.
      <name>/                          # a named template — one per way you work
      _spaces/                         # optional: spaces this template offers the zone
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
  README.md                 # the record: summary, decisions, promote table
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
`prompts/` and `logs/` do not exist; the lists you see are saved queries over
the tags. A file declaring none of them is **untagged**, which is not an error:
the last space, **Untagged**, is the saved query for exactly those files — every
kind negated — so it is listed, counted and foldable like anything else, and it
is absent from a session where every file has said what it is.

The known cost is real: a flat session opened in Finder is an undifferentiated pile
of markdown until something reads the tags. `AGENTS.md` is the mitigation and is
why it is the one file keeper always writes — it is the navigation contract, written
for whoever, or whatever, is handed the folder with no other context.

`artifacts/` and `workspace/` survive in both shapes because their difference is
about *versioning*, not about kind, and no tag can replace that.

**Markdown is found wherever it sits.** keeper reads a session's markdown in
subdirectories too, so a `spaces/`, a `log/` or any folder you make is a real home: the
file is in the pool, the space whose tag it carries lists it, and a file carrying no tag
is listed by **Untagged** rather than invisible. Two folders are never read, in both shapes —
`artifacts/`, which is output, and `workspace/`, which is scratch that dies with the
session — and neither is anything dotted, like `.git` or `.obsidian`. A folder-shaped
session's root is read the same way, beside its `README.md`: the record stays the record
and does not also appear as an ordinary file.

**Moving a file still never changes what it is.** The tag decides the kind, so a sitting
you drag into `log/` is still a log, and a file you drag into `spaces/` is whatever it
said it was before you moved it. keeper itself keeps writing at the session root — the
folders are yours to make and yours to file into. A sitting is dated by its **filename**,
which is where the clock is written, so the log you filed into `log/` sorts by its own date
and the board row and the session's Log agree about which sitting is the newest one.

**The scan is budgeted, because a session is not a vault index.** It stops after a fixed
number of entries and after ten megabytes of prose, and either stop is reported as
truncated rather than quietly cutting the list short — as is a folder keeper is not allowed
to read, because a card that leaves a board with nothing anywhere saying so is the worst
kind of failure there is.

**Which one a folder is** is decided by presence, not absence: a folder holding
`AGENTS.md` is flat, and that is the whole test. A flat session's record is
`README.md`, so the record's name cannot be the signal — adding it to the test
would flip every folder-shaped session on your drives to flat on one rescan.
A folder holding neither is folder-shaped, including a hand-built one that has an
`about.md` and no `AGENTS.md`: that session is *migrated* rather than
special-cased (see **Migrating the record's name** below). Nothing is stored to
say which shape a session is; the files are the truth.

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

`README.md`, in both shapes. Same file, same name, same job.

It was `about.md` in a flat session until story 52.1 moved it, because `AGENTS.md`
was meant to be the only navigation file and a README beside it looked like a
second answer to one question. It is not: `AGENTS.md` says *how to read this
folder* and `README.md` says *what this session is*, and only the second is the
file every other tool, host and human already opens by name.

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
instead — which is why a rename has to rewrite the links that named it rather than
relying on an id to carry them, and keeper does exactly that (see *The session's files*)
rather than minting one. Stamping at index time would mean *opening a session mutates
every file in it*: the scan would dirty a real git tree and sync would commit changes
nobody made.

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

  Choosing a template copies it, minus its record and plus two directories:
  `README.md` is restamped with *your* title and today's date, since copying the
  template's would name every new session after the template. An unmigrated
  template that still keeps its record in `about.md` is read for its headings all
  the same, so nothing you already have stops inheriting. `AGENTS.md`
  travels untouched — a zone that edited its own navigation contract meant it. A markdown
  file that carries a `{{placeholder}}` arrives with it filled in; everything else is
  copied byte for byte. See *Templates* below for the list, and for the `_spaces/` a
  template may offer the zone.

  **Every new session has `artifacts/` and `workspace/`** (FR-288), whether or not the
  thing it was made from did — a folder-shaped pattern gets that shape's four. So a
  template create is *nearly* verbatim rather than exactly so, and the preview says which
  directories it is adding. The alternative was worse: a hand-made template without them
  produced a session whose own `AGENTS.md` describes two directories it does not have, and
  whose first promoted artifact had nowhere to go. A create from a **session** always
  worked this way; this is that rule extended to templates rather than a new one.

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
  `README.md`, one seed log and one seed prompt. The seeds exist so the log and prompt
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
  `_template/` of its own. What lands is a **skeleton**: `AGENTS.md`, a `README.md` titled
  `<session title>`, and `artifacts/` and `workspace/` — and deliberately not the seeds. A
  template has no title and no minute — freezing either would name every session after the
  moment you pressed the button — so the seeds stay something keeper writes per create,
  with that session's own title. Add your own afterwards if you want different ones; a
  create will prefer yours. Pressing it a second time is safe for the directories in a way
  that matters: the two files are moved into `.keeper/trash/` and rewritten, so an edited
  `AGENTS.md` is recoverable, while a directory is only ever *made* — an `artifacts/` with
  your output in it is never trashed for the crime of already being there.
- **Templates** — a mode on the board, sitting with the status chips rather than as a
  fourth button in the pane header. The chip is a toggle: press it again to come back to the
  session rows, or press a status chip, whichever you reach for. It lists every template the
  zone has — its own `_template/` and each named one — and under each, every file inside it,
  newest change first. That listing is the same walk the picker previews a create from, so
  the room and a new session cannot disagree about what a template holds: a file in a
  subdirectory is a row too, grouped under the folder it sits in and labelled with its own
  name — the answer carries each file's path within the template, and the tree is read back
  out of that rather than out of a second walk. Arrow keys walk it and `Enter` opens a file
  or folds a folder, as in a session's own tree. The one difference is the create's, not the
  room's — a new session gets a
  *stamped* record rather than a copied one, so `README.md` is a row here and never under
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
  `README.md` into `.keeper/trash/` and write keeper's in their place — recoverable, and
  still not what "one beside it" means. The refusal is on the name as typed, so a name
  that only *folds* onto an existing one (`Interview Kit` where `_template/interview-kit`
  is) does reach the command, and does trash-then-write those two files. The search box and
  the Pinned/Unread filters are hidden in this mode: they filter session rows, and a
  template has no status.

  **A template's own files and folders can be changed from here** (FR-284), which is why the
  list is a **tree** rather than a column of paths: a template's shape is its folders, and a
  create copies that shape whole. The tree is grouped out of the same single read — a file's
  row carries its path inside the template, so the folders are already in the answer and
  nothing walks the directory twice. **A folder is a row the moment it exists**, empty or
  not, because a folder you cannot see is one you cannot rename or delete — the row is
  keeper's answer about the directory rather than something inferred from the files inside
  it. The one thing a directory's row does not show is an age: a folder's timestamp moves
  whenever anything in it does.

  Four verbs, each a journaled plan like every other zone write, so the drive gets one commit
  with keeper's provenance on it rather than an edit keeper's watcher reads as somebody
  else's:

  - **New file** and **New folder**, on the template's own heading and always visible — the
    field takes the path *inside* the template, so `notes.md` lands at its root and
    `refs/inputs.md` lands in a `refs/` **that is already there** — a folder in the path
    that is not on the drive is refused, naming it, and *New folder* is what makes one.
    That is one rule rather than a restriction: only the last thing you type is folded to a
    name, so a folder created around a filename would have been spelled exactly as typed
    (`Interview Kit/`) while the same words typed into *New folder* fold to
    `interview-kit` — one room with two spellings of one directory. A new file arrives
    **empty**: keeper stamps no `id` into a template, because a create copies the file and
    every session made from it would inherit that one identity. New folder is idempotent —
    asking for `artifacts/` in a template that has it is not an error — and a template's
    `workspace/` may be created and deleted like any other folder, unlike a live session's,
    where the write fence exists to keep scratch scratch.
  - **Rename**, on a row, for a file or a folder. The name you type is folded exactly as
    *New template* folds one, except that **a file keeps its extension**: `Kick Off.md`
    becomes `kick-off.md` and never `kick-off-md`, and a name typed without an extension keeps
    the one the file has. The entry stays in its folder — this renames, it does not move
    between folders. What a rename may **not** do is carry a file out of the set keeper
    writes: `README.md` cannot become `README.sh`, because that would author through a rename
    exactly what *New file* refuses to author. Keeping an extension the file already has is
    always free, whatever it is — `logo.png` renamed to `Logo Mark` stays a `.png`, since
    those bytes were already in the template. A name already taken is refused, naming it; a
    name that only differs in case from the one on disk is allowed, because on macOS that
    destination "exists" only because it *is* the file being renamed.
  - **Delete**, on a row, for a file or a folder, behind a confirmation that says where it
    goes: the zone's `.keeper/trash/<id>/`, never an unlink and never a recursive erase. A
    folder goes whole and comes back whole. Sessions already made from the template keep their
    own copies. The template *itself* is not a row here, so this verb cannot be aimed at it —
    a whole template is made with *New template* and renamed with *Rename template*.

  **This is where the no-rename rule stops applying, and that is a judgement rather than an
  oversight.** *The session's files* below refuses renaming for a reason that is about link
  identity: a hand-written file has no `id`, so its path *is* its identity and a rename breaks
  the pins aimed at it. A template has no such graph — nothing pins a template's files, and a
  create *copies* them rather than referencing them, so the only reader of the name is a copy
  that reads the directory fresh. The room already renames a whole template directory, which
  moves every file in it at once; renaming one of them is strictly less than the verb it
  already had.

  One thing this room still will not show you: a stray `.DS_Store`, or any other dotfile
  except `.gitkeep`. The walk that lists a template is the walk a create copies from, so
  giving the room eyes for those would also hand them to every new session. No verb here can
  name one; Finder is where that gets removed.

  **Placeholders a template's markdown may carry.** A create copies your template — and
  where a markdown file holds one of these, it arrives in the new session with the token
  filled in. The room states the list under its heading, because that is where you stand
  when you open a template file to edit it; here it is in full:

  | you write | you get |
  | --- | --- |
  | `{{title}}` | the new session's title, as you typed it |
  | `{{id}}` | the new session's id — the one in its record's frontmatter |
  | `{{date}}` | the day it was created, as `2026-08-17` |
  | `{{date:YYYY-MM}}` | that day in a format you choose (`YYYY` `YY` `MM` `DD`) |
  | `{{time}}` | the minute it was created, as `14:35` |
  | `{{time:HHmm}}` | that minute in a format you choose (`HH` `mm` `ss`) |

  **Anything else is left exactly as you typed it.** `{{TODO}}` stays `{{TODO}}` and the
  `{n}` in your maths stays `{n}` — the set is closed, and an unknown token is never
  guessed at. That rule is what makes it safe to expand a document full of braces at all.
  This is the same vocabulary a note template speaks, deliberately: a template written in
  Obsidian keeps working, and keeper does not have two grammars for one idea.

  Three limits worth knowing. Only `.md` files are expanded — a `.png` whose bytes happen
  to contain `{{title}}` is a PNG and is copied untouched. The record keeper stamps for you
  (`README.md`) is composed from your template's headings and is never
  expanded, because it was never copied. And **your template is not edited**: the expansion
  happens on the way into the new session, so the file in `_template/` still says
  `{{title}}` afterwards. `{{date}}` and `{{time}}` are one reading of the clock, the same
  one the session's folder name and record carry, and the resolved text is written into the
  create's journal entry — so a create that is interrupted and resumed finishes with the
  same bytes rather than a second, later timestamp.

  **A template can also carry the spaces a zone should have.** Put `_spaces/*.md` inside a
  template — the same one-file-per-query shape the zone's own `_spaces/` uses — and a create
  from that template gives the **zone** any of them it does not already have. Not the
  session: a per-session copy of a saved query is the thing this shape exists to avoid, so
  these files land beside `_template/` and never inside `active/`. Seeding only ever fills a
  hole. A space your zone already has — by file name, by title, or as one of keeper's own
  defaults — is left exactly as you tuned it, because a verb you press several times a week
  must not be able to rewrite a query you wrote once. An entry keeper cannot run — one with
  no query in it, or a query that does not parse, or something that is not a `.md` sitting
  directly inside `_spaces/` — is skipped and named in the log, and the session is still
  created: a typo in a template's space file is not a reason to refuse you a session. The
  one case keeper declines outright is a zone with no `_spaces/` directory at all, since an
  absent one is how keeper knows to write you the defaults; open the sessions board
  once and the zone has them, and a template can fill in from there.
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

**New folder** sits beside them (FR-287), always offered, under either shape. Its field
takes the **path inside the session** — `log` at its root, `log/2026` inside a
folder that is already there — because only the last thing you type is folded to a name
and the parts in front of it address folders that exist. `Interview Kit` becomes
`interview-kit`, the way a template's folder name does; unlike a template's, a session
folder folds the whole segment, dots included, since there is no filename here to keep an
extension for and a directory that reads as a filename is a trap in a pool that walks
subdirectories. Making a folder that is already there writes nothing and is not an error,
and `a/b/c` arrives as one journaled plan. Refused, each for its own reason: `workspace/`
or anything inside it **however it is capitalised** (scratch is fenced — keeper never
writes there, so a folder there would be a place for writes keeper goes on refusing, and
`Workspace/notes` is that same directory on the drive keeper ships on), a path that leaves
the session, a dotted name (the tree does not list one, so it would be a folder you could
not undo), and a name with nothing in it — `###` is not a folder name and keeper will not
invent one.

**What a folder in a session is for, and what it is not.** A kind is still a tag and never
a directory: markdown in a folder you make is read exactly as markdown in the session root
is, so a `spaces/` or a `log/` is a real home — and the file's tag is what decides which
space lists it, which means a `log/` full of files carrying no `log` tag is a directory
nothing lists. `artifacts/` and `workspace/` are the two exceptions to the reading: one
holds output, the other is scratch, and neither is scanned. This is why the `AGENTS.md`
keeper writes into a flat session no longer says *"do not create other directories"* — a
contract that forbids what the app's own button does is one an agent reads as noise. It
says instead that a directory is a **container**, for what is not markdown or for thirty
of something, and that a new *kind* of thing is still a new tag.

A delete moves the file into the zone's `.keeper/trash/`, never an unlink, and
`workspace/` refuses every write with the fence's own sentence.

**A file's name follows its title, and this is where the no-rename rule stops applying.**
It stopped applying because half of the reason it was written down turned out to be
defending something that was never built. The rule said *"renaming a file whose id is its
path silently breaks the pins pointing at it"* and elsewhere that *"a rename is a
link-rewriting problem, not a file-system one, and half of it would be worse than none"*.
The second sentence is true and the first is not: a session pool entry carries no pin —
`is:pinned` in a session space is *false* rather than wrong — and a session's `pinned`,
`unread` and `head_rev` live in its record's own frontmatter, per session, where renaming a
file inside it cannot reach them. So the pins clause was protecting a store that does not
exist, and what was left was the link-rewriting problem. keeper now does that half rather
than skipping it, which is the only version of this verb that is not the "half" the
refusal warned about.

Change a file's `title` — in the properties panel, or through **Rename** on the row's own
menu, which is the same verb — and the file is renamed to match **and** the pointers that
named it are rewritten, in one journaled plan. Either all of it landed or none of it did.
A stamped name keeps its stamp: `2026-08-16-1812-untitled.md` retitled to *Kick Off*
becomes `2026-08-16-1812-kick-off.md`, because the stamp is what makes the pool sort itself
in Finder and in `ls`, and a rename that re-stamped with today's clock would file
yesterday's entry as today's work. The folder and the extension are untouched — neither is
anything the title says.

**What is rewritten, and what deliberately is not.** Every class is decided; none is left
to chance.

| what points at the file | rewritten? | why |
|---|---|---|
| a markdown link's destination in the session's own markdown | **yes** | this is the link-rewriting half, and it is the half with teeth |
| a `[[wikilink]]` naming the file's stem | **yes** | the same pointer, in the spelling a vault writes it in |
| the record's `## Promote` row, where it names the file | **yes** | that table is a list of paths, and a row naming a file that moved has stopped being the contract it claims to be |
| the record's own `title` | n/a | that is the edit that started this |
| a path in **`workspace/`** | no | scratch is fenced, and keeper never writes there |
| a path in **`artifacts/`** | no | a deliverable's name IS the promotion contract, and a reference inside an artifact is a reference *from* the artifact |
| a backticked path in prose | no | a link is an author saying "this is a thing"; a backticked path is an author typing, and rewriting one would edit the `cat …` in a pasted transcript into a command that was never run |
| a link inside a fenced block | no | a wikilink in a code fence is documentation *about* wikilinks |
| session pins, unread marks, `head_rev` | no — unaffected | they are facts about the session, not about a file in it |
| `keeper.session.continues` lineage | no — unaffected | it names sessions by id and names no file |
| the recordings lens | no — unaffected | it keys on a `session:` frontmatter value |
| the `.keeper/` cache | no — unaffected | it holds trash, keyed at delete time, and rebuilds from disk |

One case is left exactly as the author wrote it and said so rather than guessed at: a
destination whose text keeper cannot spell the old name back out of — a percent-encoded
one, say. It is not rewritten, and **References** then reports it as missing, which is the
honest answer. Guessing an encoding to write back would be keeper editing somebody's link
on a hunch.

**And four things refuse.** A title with no letters or digits in it is refused *and the
title is not written either*, because a file renamed halfway is exactly what the old rule
was afraid of. A name already taken in the folder is refused, naming the file it would
have overwritten — a *create* counts up to `-2`, because a create has no expectation about
its name, but somebody who typed a title expects the file to be called after it. The
session's record and its contract file — `README.md`, `AGENTS.md`, and an unmigrated
`about.md` — change their title and keep their filename: `AGENTS.md` is the name keeper
reads to decide how to read the folder at all, `README.md` is the record, and an
`about.md` still holding a record would be renamed halfway, which is worse than refused.
And `workspace/` refuses, with the fence's own
sentence.

What a rename still cannot fix, and does not pretend to: a reference written in *another*
session, or in a vault note, pointing into this one. Those are outside the tree this verb
reads, and a rewrite that ranged over the whole drive to catch them would be a different
and much louder act.

The Templates room got here first, and for a smaller reason: nothing points at a
template's files, and a create copies them rather than referencing them, so there was never
a link graph for a rename to break there.

Sessions are bounded by their own contract, so the whole tree is read in one pass rather
than one call per folder. A workspace somebody let a package manager into can still
outgrow that: the tree then stops and says so rather than showing a prefix of itself.

## What the session points at

The tree lists what a session *holds*. **References** lists what it *names* — a different
set on purpose, because the zone's own rule is that big files stay in their zone and a
session points at them by path. So the thing that goes wrong is the pointer, and this is
the only place that would ever say so.

Keeper reads the session's record and its pointer files — a folder-shaped session's root
markdown plus everything in `refs/` and `prompts/`, and a flat session's whole markdown
tree, the folders you made included — and reports one row per distinct target, six kinds,
each with a real test behind it:

- **note** — the target resolves in the vault index, the same resolution a wikilink in a
  note gets.
- **recording** — a note whose frontmatter carries a `session:` key. That is what makes a
  recording a recording; a loose `.m4a` sitting in the session is a *file*, and calling it
  a recording because of its extension would be a guess.
- **file** — a path that exists, looked for beside the file the pointer was written in
  first, then beside the session, then from the drive root — so `notes.md` inside
  `spaces/plan.md` means `spaces/notes.md`, the way it does in every other markdown
  reader, while `artifacts/notes.md` and `40-media/clip.mov` still work as written.
- **session** — a path that lands inside another session in the same zone.
- **link** — an external URL, reported without being fetched. Keeper does not know whether
  a website is up, and a red row that only means "no internet" is worse than no row.
- **missing** — the path resolves to nothing.

**Missing sorts first and says what keeper looked for.** "Keeper could not find it" sends
somebody searching four hundred folders; naming every path it tried usually shows the file
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

Two directories are deliberately not scanned, in either shape. `artifacts/` is a
deliverable, so a reference inside it is the artifact's business, not the session's.
`workspace/` is scratch that dies with the session, and a broken pointer in a file nobody
keeps is not worth reporting. Dotted folders are furniture and are not read either. The
scan is bounded by total text rather than by file count — the cost here is parsing
markdown — and says so when it stops early.

## Editing

A session's text files open in keeper's own editor, in the panel beside the board. A
markdown file gets three tabs, and they are three views of one buffer rather than three
copies of the file:

- **Preview** — the rendered document, mermaid included, and read-only: the characters in
  the file and the picture of them can never disagree.
- **Source** — the characters, with the code editor's line numbers and grammar.
- **Note** — the same live preview as the first tab, editable, and where a markdown file
  keeper can write OPENS. It renders as you type, the way writing in Notes does, and it is
  offered only for a markdown file keeper can actually write: not in `workspace/`, not past
  the size limit, not a format keeper refuses. Where it cannot be offered the tab is absent
  rather than present and refusing, and the file opens in Preview instead. A view you
  picked yourself is remembered per format and still wins over that default.

Switching tabs never loses an unsaved edit and never resets the caret — there is one
buffer under all three, and one Save. Every other text format gets the code editor: line
numbers, a grammar, byte-for-byte line endings. `workspace/` files open read-only, because
that folder refuses every write.

When a file's own properties are on screen as a form above the views, the Preview and Note
tabs stop drawing that same `---` block as document text. It is hidden from the view, never
from the file: the Source tab shows every byte and a save writes them all.

**A session log writes like a note.** On both the Source and Note tabs a markdown file has
the format toolbar, the `/` command menu and `:shortcode:` emoji completion. Not copies of the
ones in Notes — *the same ones*: they live in one module both editors import, so a table
inserted from `/` in a session log and one inserted in a note are the same bytes, and a
change to either is a change to both. There is no autosave for a file, on purpose — in
Note mode either: `⌘S` or the Save button is the write, and the header says whether the
buffer differs from the disk.

**What still needs a vault, and why it always will.** Wikilink completion, tag
completion, `![[…]]` embeds and the CSV table are addressed by a notes vault plus a
vault-relative path. A sessions zone can never be inside a notes vault — keeper refuses
that layout in either direction, because the notes indexer and the sessions indexer would
each claim the same markdown — so a session file has no vault coordinates to offer them.
They are therefore absent rather than present and failing. Attachments, backlinks, note
history and the conflict resolver are note surfaces for a related reason: each is
addressed by a note id, which a file does not have.

**The properties panel is not one of them.** Frontmatter is addressed by
the file itself — the sync profile and the path inside it — so it works over a session
file, and it is the same panel Notes uses rather than a second one wearing its clothes.
See **How a file gets filed** below.

An agent editing a session file on disk shows up in the tree, and in the log the agent
appends to. An open file buffer is not a subscription, though: a file you are looking at
is re-read when you open it again, not while you watch it.

## Spaces, tasks and the log

**Spaces come first, then Files.** A session's own sections read: what it is (the record's
header), then the spaces, then the file tree, then what it points at, then the log. The
spaces sat *after* the tree until they were moved: the tree is what the session holds and a
space is a reading of it, so the contents came first. The order asks a different question —
which of the two you read more often — and About, Tasks and Log are what a session gets
opened for. The tree is where you go when a space has not surfaced something, which is the
second question. It is not a setting.

All of these surfaces read the same pool, and none of them stores anything except the fold
you leave a space in.

**Spaces** are the zone's saved queries, one markdown file each under
`60-sessions/_spaces/`. Six ship by default — About, Tasks, Log, References, Prompts, and
**Untagged** last — and they are ordinary files: rename them, reorder them, delete one,
write your own. A default you delete stays deleted; *Restore default spaces* is how you ask
for it back. The query language is the notes vault's, the same grammar and the same chip
editor, because a `tag:` that meant one thing in notes and another in sessions would be a
trap. A broken query selects nothing and says so; an unreadable sort still runs the query.

**Untagged** is that grammar read the other way round: `-tag:about -tag:log -tag:prompt
-tag:ref -tag:task`, which is every kind negated, so it holds exactly the files that have
not said what they are. It sorts last and it renders **only when it has something** — on a
session where every file declares a kind there is no such section, because a permanent
empty row reporting the absence of a problem is noise. Its rows fold, count and carry the
same menu as any other space's.

**A space you can write into.** A space whose query names exactly one kind carries its
own **New note** button — Tasks, Log, References and Prompts do, and so does any space you
write that asks for one of those tags. The button is always there, not revealed by
hovering: it is the one thing a space exists to let you do. Edit and delete stay on
hover, because they are maintenance. The file is tagged as it is created, so it lands in
the space you made it from rather than in Untagged, and it opens immediately.

**Where the new file goes is your session's shape.** A flat session keeps everything in
one pool, so the file is written at the session root. A folder-shaped session keeps
references in `refs/` and prompts in `prompts/`, so that is where they are written —
exactly the directories that shape reads its pool from, which is what makes the new file
appear in the space you pressed. The directory is *where keeper puts it*; the tag is what
makes it a reference. A file dropped into `refs/` by hand with no `tags: [ref]` still
declares no kind, in either shape — it is listed by **Untagged** — and keeper will say so
rather than guessing from the folder.

**How a file gets filed.** A file that already exists — one you wrote in your editor, one
an agent dropped in, the `README.md` a session has had since the day it was made — gets
its tag from the **Properties** panel, above the editor when you open it. Add
`tags: [ref]` and the file is in References on the next read, which happens by itself.
There is no other step: the tag is the filing, and the folder never was.

The panel is the one from Notes, over the file rather than over a note. It writes the
frontmatter block and nothing else — every byte of the body is left exactly as it was,
line endings included — and it stamps nothing of its own: no `id`, no `updated`, no kind
guessed from where the file sits. A file keeper did not author stays a file keeper did
not author. If somebody else changed that file's properties while you had them open, the
write refuses and offers to re-read rather than dropping their edit; a change to the
*body* underneath you is neither refused nor lost, because only the block is rewritten.

It is offered for a markdown file keeper can save, and only there: not for a `.csv`
(which has no frontmatter), not for a format keeper will not rewrite, and not for
anything under `workspace/` — that folder refuses every write, so there is no panel
rather than a panel that would refuse.

**One boundary that used to exist and does not any more.** A folder-shaped session's
pool was once its `README.md` plus `refs/` and `prompts/`, so markdown sitting loose at
the root of one was in no pool at all — tagging it filed it nowhere, and it did not even
show up as untagged. It is read now, in both shapes: tag a root file and the space whose
tag it carries lists it, leave it untagged and **Untagged** lists it. The record itself is
still the record and never doubles as an ordinary file.

**A space that cannot be written into still has the button, and the button says why.**
It is there, greyed out, and the reason is what it describes itself with — so the answer
to "why has this space no *New note* when every other one does" is on the control itself
rather than left for you to work out. Three spaces are in that state:

- **Tasks**, on a folder-shaped session. That contract has no tasks file — keeper will
  not write one where the shape keeps none — so there is nowhere to put one. *Convert to
  flat* is how such a session gets a place for keeper to write tasks into. (The board
  itself is not flat-only; see **Tasks** below.)
- **Log**, on a folder-shaped session. That shape's log is a `### ` entry under `## Log`
  inside `README.md`, not a file at all, and **New log** on the Files header already
  writes one there.
- **Untagged**, on any session. Its query is every kind negated, so it names no kind at
  all and there is nothing a file made there could be. Make the file from **Files** and it
  appears in Untagged until you give it a kind tag — which is the whole of what the space
  is for.

**About says why too**, and offers the verb that does apply. A session has one about
record — `README.md`, under both contracts — and keeper edits it rather than making a
second, so its create is greyed out saying exactly that, and **Open README** sits beside
it. If the space's query asks for more than one thing — the About space on the live drives
asks `tag:about tag:recordings` — that is what it says instead, because a create would
have to pick one kind and the query names two. A space asking for two ordinary tags gets
the same line.

The button is **absent** in only one case: where keeper has neither a kind nor a reason to
give you. A space asking for an ordinary tag that is not a kind — `tag:project/alpha` —
never offered a create to miss, and a space whose query keeper cannot read carries its own
fault line rather than a second sentence about creating.

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
off unless you turn it on. It is the **fallback**: it decides the spaces you have never
folded or unfolded yourself *and* whose own file says nothing about it. The ones you have
touched keep your answer and the ones that carry `keeper.folded` keep theirs, so turning it
on does not shut the three you just opened. The key is `sessions.spaces_folded`, user-global
like most of them, so a
`[settings]` table in `~/.keeper/keeper.toml` — or in the main sync folder's — can set it
like any other user-global key, and a file that sets it keeps winning: the switch shows as
file-controlled instead of quietly losing to the next toggle. The section is not there
until some folder is flagged as a sessions zone, because before that there are no spaces
to arrange.

**A space that says how it opens.** Two optional keys in a space's own file settle it,
and they win over the setting because a space is a definition on disk and how it opens is
part of the definition:

```yaml
keeper:
  space: tag:log
  sort: modified desc
  folded: true    # arrives shut, whatever the setting says
  rows: 5         # draws five rows; the rest is one press away
```

`folded` takes `true` or `false` and nothing else — not `yes`, which YAML readers disagree
about. A space that omits it follows the setting, which is exactly what a user-global
default is for, and a space you fold or unfold **by hand** keeps your answer over both.
Four steps, most specific first: your own hand, then the file, then the setting, then open.

`rows` caps **what the section shows, not what the query finds** — and that distinction is
the whole of it. The query still selects every matching file, the count beside the space's
name is still the whole list, and the remainder folds behind *Show 7 more* / *Show less*
rather than behind a scrollbar (a scroll area inside a scrolling pane is two scrollbars a
few pixels apart). A notes space's `keeper.limit` is the other thing — it narrows the
selection itself — and sessions deliberately have no such key: a session holds tens of
files, so there is no read to save, and a section that had *found* three of twelve could
not honestly tell you it was hiding nine.

Both are optional and the editor writes neither unless you set one, so a space that says
nothing behaves exactly as it always has. A value keeper cannot use — `rows: 0`, `rows:
many`, `folded: yes` — is a **warning** on the space and never an error: the line is
ignored, the space still lists its files, and the pencil shows you the sentence. The five
seeded defaults set neither key: they are the reading order of a session and are meant to
be read on arrival, and *Start spaces folded* is where "shut them all" belongs.

Both survive editing anything else. Rename a space, retag it, change its sort — the two
keys come back out of the file unchanged.

**A space that says where its new files go.** One more optional key, and it is the only
one about *writing* rather than about what the space shows:

```yaml
keeper:
  space: tag:log
  create_dir: logs    # New note here writes logs/2026-08-17-0914-untitled.md
```

The editor calls it **New files go in**, and an empty box — which is every space until you
type something — means what it has always meant: new files land at the session's own root.
Set it and keeper makes the folder if it is not there, in the same write that creates the
file. `logs` and `logs/` are the same request, and `notes/2026` is allowed.

**It does not change what a file *is*.** The new file still carries the kind tag in its
frontmatter, and that tag is the only reason any space lists it: a file in `logs/` tagged
`ref` is a **reference**, and the References space is where it appears. keeper never reads a
kind out of a folder. Nothing already in the session moves either — the key governs creates
and nothing else — and the **New log** and **New prompt** buttons on the Files header belong
to no space, so they keep writing where the session's own contract keeps that kind.

Three destinations are refused when you save the space, each saying which rule it broke:
one that leaves the session, `workspace/` — scratch that is not synced and dies with the
session — and any folder starting with a dot, because keeper's markdown scan never enters
one, so a file filed there would be in no space, on no board and not even Untagged. On a
session still in the **folder** shape the contract's own `refs/` and `prompts/` keep
winning, because that shape's pool reads exactly those two folders and the root: convert
the session to flat and the key takes effect.

**Where your own fold is kept.** Per space, in a cookie in keeper's own webview — not in the
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

**The board follows the pool, not the shape.** It used to be drawn for flat sessions
only, and the reason was true at the time: a folder-shaped session had no pool to tag, so
its board would have been four empty columns saying nothing. That shape's markdown is in
the pool now, so a `task`-tagged file in one is a card you can drag like any other. A
session with nothing tagged says what a task is instead of drawing columns over nothing.
Every card also carries a move menu, so a column change is one keystroke away without a
pointer; keeper does not ship a verb you can only reach by dragging.

**The log** is last, because it is the thing you scroll to rather than the thing you act
on. Newest first, folded from the files tagged `log`.

All three are also **markdown widgets** — `> [!board]`, `> [!log]` and `> [!refs]` in any
note, not only in a session. Callout syntax rather than a fence, so a note carrying one
degrades to a labelled quote block in Obsidian or on GitHub instead of a wall of grey
source.

## Migrating a folder-shaped session

**Sessions → the row menu → Convert to flat.** Never automatic, and never on a timetable:
this rewrites files on your drive, so it is a verb you press.

The preview shows every write before any of them happens — the record rewritten in
`README.md` from its own prose minus its `## Log`, one `YYYY-MM-DD-HHMM-<slug>.md` per
`### ` log entry, and every file hoisted out of `refs/` and `prompts/` with its kind tag
added. The record write is guarded on the bytes it was planned against, so an agent
editing your README while you migrate refuses the run rather than losing the edit. There
is no signpost to leave any more: the record IS the README, so every link and agent
instruction that ever pointed at a session's README still resolves to the record it always
named.

It is a journaled plan like every other lifecycle verb: `AGENTS.md` is written only after
everything the flat shape needs exists, so there is no moment where the session reads as
flat but its logs are missing, and the two directory removals run last. Run it twice and
the second run does nothing — the answer for an already-flat session is "no plan", which
is idempotence stated in the type rather than promised in prose. A session whose record is
still an `about.md` is declined here and belongs to the verb below.

## Migrating the record's name

**Sessions → open the session → *Move records to README.md*.** Every flat session written
before story 52.1 keeps its record in `about.md`, and `AGENTS.md` alone decides the shape
now — so such a session reads with no record at all until its record is moved: no title, no
pins, no lineage, and a board row that has lost its history. The session's own page says so
and offers the verb, because that is the page whose emptiness is the symptom. Like every
other verb that writes to your drive, it is a button you press.

**One press moves the whole zone**, not just the session you are looking at. The record's
name is a zone-wide contract and the pointer pass is zone-wide anyway, so fixing one row at
a time would be forty presses for a break you did not make. It is a journaled plan per
session and a session already at `README.md` compiles no plan at all, so the rest of the
zone costs nothing. When it finishes it tells you how many records moved and names any
session it had to skip.

What it does, per session, in this order:

- **Writes `AGENTS.md` if the session has none.** A hand-built session with an `about.md`
  and no `AGENTS.md` reads as folder-shaped, and moving its record without writing the
  contract file would leave it folder-shaped with a flat pool — every log invisible behind
  a `## Log` heading that is not there.
- **Trashes whatever is standing on `README.md`**, into `.keeper/trash/`, never an unlink.
  Two things can be: an older migration's signpost, recognised by being tagged `ref` *and*
  pointing at `about.md`; or, in a session with **no `AGENTS.md`**, a `README.md` you put
  there yourself. The second one is trashed too, and that is a decision worth stating: such
  a session was *flat* before story 52.1, so `about.md` was its record and the `README.md`
  was an ordinary file no reader ever took for one. It goes to the trash under its own key
  so you can put it back under another name — and without that, the session had no way
  forward at all, since keeper will not delete or rename a `README.md`.
- **Refuses when the session HAS its `AGENTS.md` and a `README.md` you wrote.** There
  keeper is already reading that `README.md` as the record while your `id`, pins and
  lineage sit in `about.md`: two files hold record content and keeper chooses neither. The
  refusal names both, and asks you to merge them by hand — the sweep goes on to the next
  session and reports this one as skipped.
- **Rewrites every prose pointer at the record, across the zone.** Two spellings: `about.md`
  or `[[about]]` beside the file that says it, and the record's full path from the drive
  root (`60-sessions/active/2026-08-10-keeper/about.md`), which is the only way a file in
  *another* session can name this record and have keeper resolve it. A session still
  holding its own `about.md` keeps its bare `[[about]]` — that resolves to *that* session's
  record, and its own run will move it — but a full path naming this session is followed
  wherever it was written.
- **Moves the file, last.** One rename, never a copy-then-delete, so the bytes travel
  verbatim: every frontmatter key, the `id`, the `pinned` flag and the `keeper:` lineage
  map arrive untouched. The one thing left stale on purpose is a link the record holds *at
  itself*, because a byte-for-byte move cannot also edit what it moves.

**`about.md` is still not deletable, and will not be until this has run everywhere.**
`README.md`, `AGENTS.md` and `about.md` are the three a delete refuses: moving the record's
name moved nobody's files, so until a session's record has actually been moved, its
`about.md` is the only place its identity, pins and lineage exist — and it renders in the
About space as an ordinary row. That row's Delete is refused for the same reason
`README.md`'s is.

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
files inside one. And **files can be created, renamed and deleted from inside the session
tree**; a rename rewrites the pointers that named the file, in the same journaled plan,
which is what made it shippable — *"half of it would be worse than none"* was the old
refusal, and doing the link-rewriting half is the answer to it rather than a way round it.
**Moving a file between folders** still cannot be done here: that is a different verb with
a different question behind it — which folder, and why — and the tree offers no answer to
it yet. See *The session's files* for what a rename rewrites and what it deliberately
leaves alone.
