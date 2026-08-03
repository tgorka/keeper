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

## Spaces and the query language

A space is a saved query, stored as an ordinary note under `spaces/` — so it syncs, it has
history, and an agent can write one with a text editor.

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
wrong. Delete `<vault>/.keeper/` and restart, or use *Rebuild index*. A cold scan of 10 000
notes takes under five seconds; nothing is lost, because everything in the cache is derived
from the files.

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

Also deliberately out of scope this phase: vault encryption, a full-text search engine (the
bounded parallel scan is never stale and is fast enough well past ten thousand notes), a
plugin API, notes on the phone, and publishing a note into a Matrix room.
