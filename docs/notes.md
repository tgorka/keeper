# Notes

keeper's note taker. Markdown files in an Obsidian-shaped folder, inside a folder keeper
already synchronises.

This is the operator document: what is on disk, what keeper reads and writes, what it will
never touch, and what to do when something is wrong. The design reasoning lives in
`_bmad-output/planning-artifacts/` (PRD phase 5, `ARCHITECTURE-NOTES-PHASE5.md`,
`EXPERIENCE-NOTES.md`); the requirement numbers below point there.

## The one idea

**A vault is a folder you already sync, plus a flag.**

There is no vault picker, no import, no separate notes store and no second place to configure
anything. Settings → Sync → a folder → *This folder is a notes vault*. keeper keeps notes in a
subfolder of that folder (`notes/` by default) and syncs them with everything else there.

Everything else in the feature follows from that: notes sync because the folder syncs, notes
have history because the folder is a git repository, and keeper knows who changed a note
because the sync engine already stamps every commit with its origin.

## On disk

```text
<your synced folder>/
  notes/                               # the vault root — the subfolder you named
    *.md                               # notes, flat at the root by default
    journal/2026/2026-08-02.md         # the journal, path configurable per vault
    templates/*.md                     # templates, applied at creation
    spaces/*.md                        # saved queries — ordinary notes, see below
    attachments/                       # pasted and dropped files
    .keeper/                           # keeper's own cache. NEVER synced.
      index.json                       # the index cache — safe to delete
      search.db                        # the search index (Epic 76) — derived, safe to delete
      trash/<id>/<original path>       # deleted notes, recoverable
    .obsidian/                         # yours. keeper never reads or writes it.
```

`journal/`, `templates/`, `spaces/` and `attachments/` are created on first use, never at flag
time — an empty scaffold in an existing vault is exactly the "keeper reorganised my files"
surprise the feature is built to avoid.

### What keeper promises

- **`.obsidian/` is never read and never written.** Not "not modified" — not opened. The scan
  skips the directory by name before descending into it.
- **keeper never moves a file you did not ask it to move.** Retitling a note does not rename
  its file; there is an explicit *Rename file to match title* action for that.
- **`.keeper/` never syncs.** It is a tier-0 exclusion in the engine, so it cannot reach a
  commit, the pending list or the activity feed.
- **A write that changes one frontmatter key leaves every other byte identical** — comments,
  key order, quoting style and all. Obsidian reads the file afterwards exactly as before.

## Frontmatter

Three tiers, and the tier says who may write a key.

| tier | keys | notes |
| --- | --- | --- |
| keeper-owned | `id` | A 26-character ULID, written once. Links, pins, unread marks and history follow it through a rename. A note that already has a *non*-ULID `id` keeps it; keeper indexes that note by path instead and says so. |
| | `created`, `updated` | ISO-8601. Obsidian renders them as dates. |
| | `pinned`, `archived` | Booleans. Absent means false. |
| | `keeper` | keeper's reserved namespace, one level deep: `keeper.space`, `keeper.template`, `keeper.capture`. |
| Obsidian-native | `tags` | Read, and merged with inline `#a/b` tags from the body. keeper appends; it never reorders or reformats what is there. |
| | `aliases` | Read for link resolution. Never written. |
| | `cssclasses` | Read only so it is preserved. Never interpreted. |
| yours | anything else | Parsed, indexed, queryable through `field:`, editable in the properties panel, and preserved byte-for-byte by any write that does not target it. |

**The block is never in the editor.** The body is what you type in; the frontmatter renders above it
as the properties panel, with a control per key. So there is no `---` in the buffer to type in front
of, and the caret at the top of a note is the top of its *text*. keeper re-joins the two on every
write, which is why editing a property and editing a paragraph are one save and cannot overwrite each
other.

## Spaces and the query language

A space is a saved query, stored as an ordinary note under `spaces/` — so it syncs, it has
history, and an agent can write one with a text editor. Being a definition rather than a
note you write, it is a service file of the note list: hidden while the eye toggle is on,
listed in the rail.

```markdown
---
keeper:
  space:
    query: 'tag:project/keeper -tag:archive (field:status=open | field:status=review) date:modified>=-14d'
    sort: modified desc
    lens: list
---

# Active keeper work

Everything still moving on the keeper project, touched in the last fortnight.
```

The grammar is the one you already know from Gmail and GitHub: terms side by side mean AND,
`|` means OR, `-` negates the term that follows it, parentheses regroup, and a bare word is a
full-text search.

| predicate | example | means |
| --- | --- | --- |
| `tag:` | `tag:project`, `tag:project/*` | Segment prefix. `tag:project` matches `project/keeper`; `project/*` matches strict descendants only. |
| `path:` | `path:journal/**` | Glob, vault-relative. |
| `field:` | `field:status`, `field:priority>=3` | Frontmatter. Bare = present and non-empty. `=` against a list means *contains*. Comparing incompatible types is false, never an error. |
| `date:` | `date:modified>=-14d`, `date:created<2026-01-01` | `created`, `modified` or `touched`, against `YYYY-MM-DD`, `today`, `yesterday` or a relative `-<n>[dwmy]`. |
| `origin:` | `origin:agent`, `origin:device:hesperia` | From the last commit that touched the note. |
| `is:` | `is:pinned`, `is:unread`, `is:orphan` | A closed set: `pinned archived unread conflict journal template space capture orphan untagged`. |
| `text:` | `text:"two words"` | Case- and diacritic-folded, over title and body. |
| `link:` / `backlink:` | `link:"Vault as a lens"` | This note links to the target / the target links to this note. |

`sort`, `lens` and `limit` are deliberately *not* part of the query. A boolean expression that
grows an `order by` grows a parser.

A query that does not parse **matches nothing** and says why, with the offending token
underlined. It never falls back to matching everything — a space is a surface people run bulk
actions from.

The chip editor above the results is the query, not a picture of it: every predicate above
round-trips through it, including `field:status=open`, which for a while parsed but could
only be edited as text. A chip that cannot represent a term truthfully — an ordered
comparison like `field:priority>=3` — says so and leaves you the text field, because a chip
that silently widened `>=` to `=` would be worse than no chip.

## Widgets in a note

Three blocks turn a note into a small application over its own vault:

```markdown
> [!board] tag:project/keeper
> [!log] tag:project/keeper
> [!refs]
```

`board` is four columns — in preparation, to do, done, deferred — over the notes the query
selects; dragging a card writes `status:` and a fractional `order:` into that one note and
touches nothing else. `log` folds the matching notes newest-first. `refs` lists what the
note points at, with the same six kinds and the same missing-first ordering the sessions
surface uses.

They are **callouts, not fenced blocks**. A note carrying one opens in Obsidian, on GitHub
or in `cat` as a labelled quote — degraded, but still readable prose. A fence would have
made it a wall of grey source everywhere but here, and a note that only makes sense inside
keeper is a note keeper has taken hostage. That rule is for blocks whose content is prose or
links; a block whose content is configuration is a fence — the media block below, and D-30
says why.

Sessions use all three, but nothing about them is session-specific: a board is a widget
that happens to be useful in a session, not a session feature that leaked.

*Insert widget* on the format toolbar, and the same entries in the `/` menu, put one in at
the caret: *Media player…* first, then *Gallery*. The board, log and refs callouts are typed
by hand.

## Media in a note

A recording, a transcript or any audio or video file plays inside a note as one block: the
transcript viewer itself, its player and under it the transcript's lines, following playback.

````markdown
```keeper-media
session = "01J8…-01J8…"          # exactly one of: session | transcript | [[part]] | src | record
title = "Pricing, with Kelly"    # optional
from = "00:12:00"                # optional window, [from, to), on the recording's own clock
to = "00:15:30"
picture = "both"                 # optional: screen | camera | both
sound = "both"                   # optional: system | microphone | both

[[marker]]
name = "The price we agreed"
at = "00:13:05"
```
````

- **What it names.** `session` is a recording's identity — the one its note's `session:`
  carries, which a retitle does not change. `transcript` is a transcript file; `[[part]]`
  tables list media with no transcript, in play order (`file`, and optionally `camera`,
  `offset`, and `system`/`microphone` track numbers counted from 1); `src` is a `.toml` file in
  the drive holding the same grammar. Paths are relative to the drive that holds the note.
- **Only keeper's Rust reads it.** The editor hands the body over as written and draws what
  comes back (`media_block_resolve`); a key keeper does not know, two sources, a bad time or a
  path outside the drive shows the block's own text with a sentence naming what is wrong.
  `version = 2` says the block was written by a newer keeper.
- **The transcript viewer, in the note.** The block is the viewer's own component, not a
  read-only copy of it: each line's ⋯ (*Edit text*, *Split…*, *Add a line after*, *Change
  speaker*), the speakers' chips (*This is…*, *Rename label…*, *Merge into…*, *Add speaker*,
  *Go to their nearest line*, *Go to their next line*), search, *Transcribe again…* and *Copy
  clip*. It differs only in showing the block's window and its markers, and in having no
  dialog chrome (`docs/transcription.md` § *A transcript in a note*). Every correction, here
  or in the viewer, and every transcript a job writes, reaches every open block through
  `keeper://transcript-written`, with no rewrite of the note. A block before its transcript
  plays the media under "Not transcribed yet.", with *Transcribe* where this Mac can.
- **One scrollbar.** The block has no height of its own and no scroll box: it grows with its
  lines and the note's editor is the only thing that scrolls. Its lines are windowed against
  the editor's scroller, so a 300-line meeting does not mount 300 rows, and the pinned player
  sticks to the top of the note's scroll area. It has no card behind it — the note's
  background shows through, and only its controls keep their own surfaces.
- **Editing the block's text.** A click on the block does not open its source. Its ⋯ has
  *Edit block source*, which shows the fence's text with the caret inside it, and *Remove
  widget*, which deletes the fence and a `[!transcript]` callout attached to it; ⌘Z brings
  both back.
- **Hints while you write it.** With the source open, completion offers the keys that may
  stand on the caret's line — the root's, or a `[[part]]`'s or a `[[marker]]`'s under that
  header — each with a line saying what it does, never one already written in that table and
  never a second source. After `=` it offers the key's values: `picture`/`sound`/`record`'s
  words, the player's time when the block was playing before its source was opened and
  `"00:00:00"`, recent recordings as *title · date* writing the identity, and the note's
  drive folder by folder, only the files the key takes (`transcript`, audio or video for
  `file`, video for `camera`, `.toml` for `src`). A body that does not read is underlined on
  the key or line Rust refused, the refusal's sentence on hover. The catalogue is Rust's
  (`media_block_schema`) and so is the check (`media_block_check`).
- **Markers.** *Mark this moment* and *Mark a window…* add a `[[marker]]` table; rename and
  remove change only that table and keep every other byte, comments included
  (`media_block_edit`). `[[Note#The price we agreed]]`, or `[[#…]]` in the same note, opens the
  note, brings the block into view and moves its player there, paused.
- **Clips.** *Copy clip…* puts a new block for the same recording on the clipboard, with the
  window you chose and the markers inside it — and, unless you untick it, the window's lines
  as a folded `[!transcript]` callout after the fence, for readers without keeper. keeper
  hides that callout inside the block and never refreshes it.
- **One player at a time.** Media mounts only near the screen, a block that scrolls away
  gives its media back, and starting one block pauses the others in the pane.
- **In the notes list.** A row's preview and a search result read a block as one line —
  "▶ Media · Kelly sync · 45:57" — never as its source. The title is the block's `title`,
  else, in a recording note, its recording's title when the block names that recording; the
  length is the block's window, else that recording's `duration:`. What the list cannot say
  without opening a file is left out, and a block that does not read is "▶ Media".

**Every block keeper writes says what else it may say.** The stub's block, *Insert widget*,
*Play in a player* and a copied clip carry the keys they set, then each optional key they do
not set, commented out, then a marker's shape and the list of sources:

````markdown
```keeper-media
session = "01J8…-01J8…"
# title = ""
# from = "00:00:00"
# to = ""
# picture = "both"      # screen | camera | both
# sound = "both"        # system | microphone | both
# [[marker]]
# name = ""
# at = "00:00:00"
# sources: session | transcript | [[part]] file/camera/offset/system/microphone | src
```
````

Uncommenting a line, and filling in what it leaves empty, gives a key the block reads; the
parser skips comments. A one-part block lists the optional keys above its `[[part]]`, because
a key below it belongs to the part.

**Embedded audio and video no longer play by themselves.** `![[clip.mov]]` is a chip — the
file's name and kind, *Reveal*, *Copy path* and *Play in a player*. That last one replaces the
embed with a block: in a recording note every embed of the recording's media becomes one
`session` block, and any other file a one-part block naming it. Images still draw inline.

The note keeper writes when a recording ends carries a `session` block, below its heading.
Older notes carry one embed per video, or a three-line block with no comments;
*Use the media player in recording notes…* in the notes options menu rewrites both — a stub
whose embeds are still exactly what keeper wrote, or whose block is still exactly those three
lines naming its own recording, and nothing else in it — and names the ones left alone because
somebody edited their embeds.

**Recording from a note.** *Insert widget → Media player… → New recording* (on a Mac that
records) inserts a block that has not recorded yet — with the commented optional keys every
block keeper writes carries:

````markdown
```keeper-media
record = "new"
```
````

`record` takes only `"new"` and is a source of its own, so it cannot sit beside `session`,
`transcript`, `[[part]]` or `src` (nor in a `src` file). The block draws the Recording pane's
setup — what to capture, system audio, microphone and camera, where it saves — and *Start
recording*. Start records a session linked to this note (`linkedNote` in its `manifest.json`:
the vault and the note's path in it). The moment the start answers, the block that pressed it
names the session: its own body becomes `session = "<id>"` (Rust composes it — `record` turns
into `session` in place, every other byte of the block stays), as an ordinary edit, and the
note is written at once as ⌘S writes it. If the block moved or was removed within that round
trip it keeps `record = "new"` and says "Recording — this block could not take its name; stop
it in the Recording pane." While it records:

- the note's `tags` gain `recording` and `recording/<this Mac's host name, slugged>`;
- the block naming the live session shows the live banner with *Stop*; in Preview, in the
  Files preview or in another note, a block naming it says only "Recording…";
- every `record = "new"` block, in this note or another, shows "Recording in *note title*",
  a link to it, and no Start — keeper records one session at a time.

What a block is — a record block, the session it names, and whether that session is recording
into this note — is one question to Rust per block.

**keeper never edits the note's body for a recording** — only its tags, so typing in the note
while it records cannot race keeper. A tag change goes through the notes writer's block
amendment: with the note open in an editor, that editor's saves are held while keeper reads,
changes the frontmatter alone and writes; the editor adopts the new block and revision
(a `block` batch on its channel) and keeps every word typed, and a body save it had already sent
against the older revision is not a conflict — the disk differs only by keeper's own block
change. A properties edit composed before the new block arrived is refused rather than written
(it would undo the tags): the panel says so and the edit is made again. With no editor open it
is a plain re-read-and-write. An editor's saves go one at a time per note — blur, the idle
autosave and a block's own save wait for the one in flight and then write what is still
unsaved — so two of them never carry the same revision.

When it stops the tags go — `recording/<this Mac>`, and `recording` unless another Mac's
`recording/<x>` is still there. A session started in the same note while the last one's stop
is still being handled keeps its tags. The block becomes the player, transcription after
recording runs as it does for any recording, and no separate recording note is written while
the note still names the session — in a block on disk, or, while the note exists, in an open
editor's unsaved words; a failed session's too. A note deleted, or whose block was removed or
never got the name, gets the ordinary recording note instead. After a quit or a crash
mid-recording, the recovery pass at the next launch (or the next Start) does the same. Tags a
crash left behind are swept: whenever nothing records here — at launch once the vaults are
indexed, before each Start and after each stop — every note in an open vault tagged
`recording/<this Mac>` loses it (and `recording`, on the same rule), and another Mac's tag is
left for that Mac. In Preview a `record = "new"` block says only "Not recorded yet.".

In Obsidian, on GitHub or in `cat` the block is a code block: the title, the window and the
markers read plainly, and a clip's words read as a quote.

## The writing tools

Three affordances sit over the note body: the **format toolbar** above it, the `/`
**command menu** at the start of an empty line, and `:shortcode:` **emoji** — either
picked from the menu or typed straight through, where the closing colon turns `:tada:`
into 🎉.

Since Story 50.3 those three are not the note editor's private property. They live in one
module, and keeper's file editor imports the same one — so a markdown file opened from
Files or from a session has the same toolbar, the same `/` commands and the same emoji, to
the byte. A second copy would have been a second set of behaviours nobody noticed drifting
apart.

What stayed here is what needs a **vault**: wikilink and tag completion, `![[…]]` embeds
and the CSV table are all addressed by a vault plus a vault-relative path, and a file
outside a vault has neither.

**Live preview did not stay.** This paragraph used to say it had, because a note autosaves
against a subscription and a file is saved by hand — but that is a fact about *saving*,
not about *rendering*. Since Story 51.5 a markdown file has a third tab, **Note**, and it
is this same live-preview layer over the file's own buffer: rendered as you type, in Files
and in a session exactly as in a note. The distinction the old sentence was protecting is
intact, because it was never about the renderer — a note saves itself, and Note mode over
a file writes when you press `⌘S` or Save and at no other moment. One renderer, two save
contracts, and neither surface borrowed the other's.

**Preview, Note and Source over a note, too.** The note's header has the same three views a
markdown file has in Files, as one segmented control ("Show the note as"): *Preview* is the
Files markdown preview — rendered and read-only, its widgets live, no caret; *Note* is the
live-preview editor above, and the default; *Source* is the same editor with every widget and
decoration taken away, the raw markdown, still autosaving with its caret and undo. The choice
is one for every note, remembered in the viewers' cookie (`keeper_viewer_modes`, key
`keeper-note`) beside the Files viewers' own; it has no shortcut.

## Finding text

`⌘F` finds inside the note you have open — the editor's own find, so it reaches text below
the fold that the rendered view has not drawn yet, and `Enter`/`⇧Enter` walk the hits.

**The list's search field searches the bodies** (Epic 76). Type a word and the list is
re-ordered by relevance — FTS5 `bm25` over every note chunked heading by heading — with the
matching words marked in each row's excerpt, and marked again in the note when you open it
from that list. The marks go when the query goes, when you switch notes, or on `Esc` in the
editor. Folding is keeper's own, so `łódź` finds `Łódź` and `notatke` finds `notatkę`; the
last word you typed matches as a prefix, so the list narrows as you type.

**Meaning, if you have a model.** Settings → Notes search lets you pick an embedding model
from a provider you already configured (Ollama today; a model that advertises `embedding`).
With one chosen, every chunk is embedded through that provider's `/v1/embeddings` — paced in
the background, never blocking the list — and a query is answered by words and by meaning
together: a note that says *taxes* can answer `podatki`. Such a row says *matched by meaning*
and carries no marks, because there is no word to mark. With no model chosen search is words
only and the bar says so. keeper ships no model and downloads none (D-4, D-21).

**Service files.** The eye toggle in the bar hides `index.md`, `agents.md`, `claude.md` and
`log.md` from the list, and the space definitions in each drive's spaces folder
(`<spaces folder>/`, `spaces/` unless the drive says otherwise) — on by default, remembered
across launches; the count line says how many it hid, and turning the toggle off shows them
again. The names are a setting (`notes.service_file_names`); the spaces folder is the drive's.
Only the plain list hides space definitions: inside a space, its own query decides. A hidden
note still opens from a link, still matches `⌘⇧F`, and is still in the Files tree; a space
definition is still in the rail.

`⌘⇧F` searches everything at once: messages, notes and session files. It was the chat
search shortcut and is now the search shortcut; a person who wants to find a sentence
rarely remembers which surface they wrote it on. It reads the files, not the index.

## Capture

The point of the feature. A global hotkey raises a panel, you type, you press Escape, and the
note is on disk and on its way to your other machine. No title prompt, no folder prompt, no
save button anywhere in the product.

- Set the chord in Settings → it is unset until you do.
- Escape commits and hides. Pressing the chord again while the panel is up hides it *without*
  committing — the buffer is kept either way.
- The buffer survives a dismissal and a restart. Text you typed is never lost because the
  panel went away.
- The tray menu carries the same actions plus your five most recent notes, so a whole day of
  use never needs the main window.

## Your agent writes here too

An agent with nothing but a text editor is a first-class author. It edits the `.md` files; the
file is the API.

What you get for free:

- **The change appears live.** If you are not editing that note, the editor takes the change
  and marks the lines that moved. If you *are* editing it, non-overlapping edits merge and a
  bar appears in the editor showing what arrived — never a modal, never a lost buffer.
- **An unread mark**, on the row and on the tray glyph, until you have looked.
- **History and blame**, projected from the sync engine's commit trailers: which device, which
  origin, when.
- **A conflict is a row in the list**, not litter you have to find on disk.

## Cadence

Notes vaults sync themselves. Per vault:

| knob | default | meaning |
| --- | --- | --- |
| `commitIdleMs` | 2000 | Commit after this much quiet. Floor 500. |
| `pushIntervalMs` | 30000 | Push at most this often. Floor 5000. |
| `pushOnBlur` | true | Also push when the window loses focus. |

Hiding the window and quitting both force a flush. A commit needs no network; a push that
cannot complete becomes a journal row and is retried, so nothing here can block a close.

## When something is wrong

**"No notes vault yet."** No folder is flagged. Settings → Sync → your folder → *This folder is
a notes vault*.

**The list is missing a note that is on disk.** The index is a cache and is allowed to be
wrong. Delete `<vault>/.keeper/` and restart, or use *Rebuild index* — which also deletes
`search.db`, so search results and marks come back as the index is rebuilt. A cold scan of
10 000 notes takes under five seconds; nothing is lost, because everything in the cache is
derived from the files.

**A note cannot be opened by its id.** Its frontmatter `id` is not a ULID — written by another
tool, or hand-edited. keeper will not overwrite an id it did not write, so the note is indexed
by path and marked *unstable identity*: it works, but its pins and unread marks do not survive
a rename. Delete the `id` line and let keeper mint one.

**A deleted note is gone.** It is not: `<vault>/.keeper/trash/<id>/<original path>`. keeper
never unlinks a note.

**The tray icon is invisible / the tray menu never changes** on Linux — see the Linux section
of [constraints-and-limitations.md](constraints-and-limitations.md). Both are platform
behaviours keeper works around; the notes items are in the first menu built for exactly that
reason.

**The capture hotkey does nothing** under Wayland. Compositors without the global-shortcuts
portal refuse the registration; keeper logs `hotkey: OS refused to register global shortcut`
and carries on. The tray item and the command palette still work.

## What is not here yet

Table and board lenses over frontmatter fields, and torn-off sticky note windows. Both are
specified (FR-123, FR-124) and scheduled; neither is implemented.

Also deliberately out of scope this phase: vault encryption, a plugin API, notes on the phone,
and publishing a note into a Matrix room. Full-text search, once listed here as out of scope,
arrived with Epic 76 — see *Finding text* and D-21 in `docs/decisions.md` for what changed
and what did not (the in-memory index is still the model; `search.db` is derived).
