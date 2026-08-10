# Epic 45 — Open it, change it, put it back

status: draft
created: 2026-08-10
altitude: epic
parent: Phase 5 (Notes), Epic 43 (a note can show you the file), Epic 44 (the vocabulary is the space)
source: a third field report from the owner after epics 43/44 were installed and used — thirty-nine items
binds: FR-173–FR-199, AD-87–AD-97, UX-DR65–UX-DR76

## Why this epic exists

Thirty-nine items, and they are one sentence with three corollaries.

**keeper shows you the name of a thing where it should show you the thing.**

The Files pane lists a PDF it will not open. A note embeds a CSV as a dead link. A table you typed
renders as the pipes you typed. Quick capture takes text and refuses a tag. Every one of these is
the same shape: the app already *found* the thing — it indexed it, it synced it, it knows its type
from `kind_for_file_name` — and then hands you a label.

Three corollaries, and each is a wave:

1. **A thing you can open, you can change.** Not a viewer and, later, an editor: the same surface,
   because a read-only pane that grows a write path six months later grows a *second* write path.
   The owner asked for this in eleven separate items — edit text files, edit a CSV table, edit an
   embedded JSON, delete a file, create a file, edit tags — and they are one decision made once.

2. **Two things at once, or it is not a workspace.** Single click replaces, double click opens
   beside. Every viewer item in the report is worth half as much without this: a PDF you can open
   only by losing the note you were reading it for is a PDF you will open in Preview instead.

3. **Quick capture is the note editor, or it is a text box that lies.** Markdown, tags, attachments,
   templates, several at once, any note. Six items that are one item: stop having two editors.

## Where we take a position

**One viewer registry, not a viewer per surface** (AD-87). A `.csv` opened from Files, embedded in a
note, and attached to a quick capture is the same renderer over the same bytes. The alternative
writes three CSV widgets that disagree about a ragged row, and 44.16 already put that decision in
`keeper-core::notes::csv` precisely so it could be answered once. Kind → viewer is a table; adding
a format is a row, not a surface.

**Raw and rendered are one component, and raw is always editable** (AD-88). The owner asked for the
toggle on md, csv, json and jsonl. Generalised: every text-shaped format gets it, `raw` is a text
editor over the real bytes, `rendered` is the format's own view and is editable only where the
format has a structure to edit (a CSV cell, a table row). Never a read path and a write path that
can disagree about what the file says.

**AD-75 is overturned, deliberately and by the owner** (AD-89). "The files surface never writes" was
a good rule when Files was a window onto the sync engine's world. The owner has now asked it to
delete, create, rename and edit. It writes — through `write_vault_file` + `mark_dirty`, the *same*
path notes and 44.16's CSV editor use, never a second writer and never a reach into the engine.
Every destructive act is confirmed, and the confirmation names the file. Writing this down as a
reversal rather than quietly relaxing it, because the next reader will find AD-75 and need to know
it was retired on purpose and what replaced it.

**A panel is a view of a target, and targets are addressable** (AD-90). `note:<id>`, `file:<vault>/<rel>`,
`recording:<session>`. Panels are a list; the active one is an index. Single click sets the active
panel's target, double click appends. Without a real model, "open beside" becomes a boolean on four
components and the fifth surface cannot join.

**Unknown is a first-class kind** (AD-91). A format with no viewer renders a named placeholder that
says the extension, the size, and offers Reveal and Open With — not an error, not a blank. The
report asked for this in one clause and it is the difference between a file browser and a demo.

**The emoji vocabulary is generated data, not code** (AD-92). The cheat-sheet mapping is a build-time
table checked into `src/lib/`, with the generator beside it. Nobody hand-maintains 1800 shortcodes,
and nobody ships a network call to render `:tada:`.

**Quick capture hosts the note editor** (AD-93). Not "quick capture gains markdown". It mounts the
same `NoteEditor` in a smaller window, so every mark, the `/` menu, the format toolbar, tags and
attachments arrive at once and stay in step forever. The window chrome is quick capture's own; the
document is not.

## What is NOT in this epic

- Collaborative editing, presence, or conflict UI beyond what sync already does.
- Writing to files outside a vault. A panel can *view* one; the write path is vault-scoped and stays
  that way (FR-145, AD-65).
- A plugin API for viewers. The registry is internal; a third-party viewer is a different epic.
- Editing a PDF, a DOCX, a PPTX or an XLSX. They render and they annotate (45.21); their bytes are
  read-only, because a lossy round trip through a document format is how people lose work.

---

## Wave 1 — Foundation

### Story 45.1: A Panel Shows a Target

**Frontend + `keeper-core`.** Bindings: FR-173, AD-90, UX-DR65.

A panel is a view of an addressable target. Single click on a row sets the active panel's target;
double click opens a new panel beside it. Closing the last panel is not possible; closing any other
moves focus to its neighbour.

The target vocabulary is `note`, `file` and `recording`, and it is one enum in `keeper-core` because
the command palette, the Files tree, the notes list and the recordings list all produce one and none
of them should invent its own shape.

Panels survive a restart. A target that no longer resolves — the note was deleted, the drive was
unplugged — renders the reason, not an empty frame, and keeps its place so the pane comes back when
the drive does.

### Story 45.2: One Viewer Registry

**Frontend + `keeper-core`.** Bindings: FR-174, AD-87, AD-91.

A table from kind to viewer, consulted by the Files pane, note embeds and quick capture alike. The
kind comes from 43.5's `kind_for_file_name`, widened where this epic needs it and nowhere else — a
second classifier is the defect this story exists to prevent.

Unknown is a kind, not a failure: its viewer names the extension, states the size, and offers Reveal
and Open With.

The registry is the only thing that knows which component renders what. A surface asks it; a surface
never switches on an extension.

### Story 45.3: The Files Surface Can Write

**`keeper-sync` + `keeper` + frontend.** Bindings: FR-175, FR-176, AD-89, UX-DR66.

Delete a file, with a confirmation that names it and says whether it syncs. Create a text file in a
folder. Multiselect, as the selection model the rest of the pane reads — delete acts on the
selection and the confirmation counts it.

Everything goes through `write_vault_file` + `mark_dirty`. Deleting uses the same removal path the
reconciler already understands, so the change is announced rather than discovered on the next scan.
A file outside a vault can be listed and viewed; it cannot be written, and the surface says why
rather than offering an action that will fail.

Read AD-75 and this story's reversal of it before you start.

### Story 45.4: Raw and Rendered

**Frontend + `keeper-core`.** Bindings: FR-177, AD-88, UX-DR67.

One component, two views, a toggle that remembers per format. `raw` is a text editor over the real
bytes and can always save. `rendered` is the format's own view.

The formats in scope are markdown, CSV, JSON and JSONL. CSV's rendered view is 44.16's table and is
editable; JSON and JSONL render as a structure and are read-only in this story; markdown renders as
the note editor's own preview.

A malformed file renders in `raw` with the parse error named and the line pointed at — never a blank
rendered pane, and never a silent fall back that makes the user think the file changed.

### Story 45.5: A File Says What It Is and How Big

**Frontend.** Bindings: FR-178, UX-DR68.

An icon per known type, driven by the registry from 45.2 rather than a second extension list. A size
in the units a person uses, computed once in Rust so every surface agrees on whether 1 kB is 1000 or
1024 bytes.

The notes folder and the recordings folder carry their own icons, because "which of these forty
folders is my vault" is a question the pane can answer and currently does not.

### Story 45.6: Text and Code Files Open and Save

**Frontend.** Bindings: FR-179, UX-DR69.

Text, markdown, JSON, CSV, config and source files open in an editor with the syntax the file
deserves, and save through 45.3's write path. Reuse the CodeMirror host the note editor already
configures; a second editor configuration is how two surfaces end up with different tab behaviour.

A file too large to edit comfortably opens read-only and says so with its size, rather than freezing
the pane. Pick the threshold, state it, and test at both sides of it.

---

## Wave 2 — Viewers and the editor's vocabulary

### Story 45.7: Media Opens

**Frontend.** Bindings: FR-180, UX-DR70.

Images, audio and video open in the pane, served over `keeper-recording://` with its range support
(43.5), using the transport 43.6/44.1 already built rather than a second player.

44.1 is required reading: `preload="metadata"` settles a video at `HAVE_METADATA`, which paints
transparent black. Whatever this story mounts, it mounts having read why the last two-video story
was a one-video defect.

### Story 45.8: Documents Render

**Frontend.** Bindings: FR-181, FR-182, UX-DR71.

PDF, DOCX, PPTX and XLSX render — in the Files pane and embedded in a note, through the same
registry entry, because "it opens in Files but not in a note" is the bug this epic is about.

Their bytes are read-only (see "What is NOT in this epic").

Check `Cargo.lock` and `package.json` before adding anything. If a renderer needs a dependency, name
it, say what it costs in bundle size, and say what the fallback is when it fails — do not add it
silently. A format we cannot render honestly falls back to 45.2's unknown viewer, which is a fine
outcome and a much better one than a broken canvas.

### Story 45.9: A Table You Can Edit

**Frontend.** Bindings: FR-183, UX-DR72.

A GFM table in a note renders as a table — like every other block, not as the pipes you typed — and
can be edited as one: add and remove a column, add and remove a row, and keep the source aligned
while you type.

44.9 shipped the aligned-table builder and the `/` menu inserts one; this story makes the thing it
inserted alive. Reuse that builder for the realignment; a second aligner will disagree with the
first about a cell containing a pipe.

The source stays legible markdown at every keystroke, because Obsidian reads the same file and a
half-written table is what sync will carry if the app closes mid-edit.

### Story 45.10: The Editor's Missing Marks

**Frontend.** Bindings: FR-184, UX-DR73.

Subscript, superscript, underline, a task list (`[ ]` and `[x]`, clickable), and a fenced code block
— all in the format toolbar and the `/` menu that 44.9 and 43.9 built, using their command shape.

And the one that is a bug rather than a gap: **links do not render properly**. Diagnose before
building. Reproduce it in a real `EditorView` with the markdown language loaded — read DW-171 first,
because the last person to assume live-preview worked found it throws on a `mermaid` fence and
nothing in the suite could see it.

Pick the syntax for sub/superscript and underline knowing Obsidian reads the same file: state what
each renders as there, and reject any spelling that makes the note worse to read outside keeper.

### Story 45.11: Emoji

**Frontend + a generator.** Bindings: FR-185, AD-92, UX-DR74.

`:shortcode:` completion in the editor from the GitHub cheat-sheet vocabulary, with a chooser that
uses 44.13's completion machinery rather than a third completion popup.

The table is generated and checked in, with the generator beside it and a test that the table parses
and is non-trivial. No network call at runtime, ever.

### Story 45.12: Embeds Render and Edit in Place

**Frontend.** Bindings: FR-186, FR-187, UX-DR75.

A CSV, JSON or JSONL embedded in a note renders through 45.2's registry and 45.4's raw/rendered
toggle, inside the note, and an edit in the raw view writes back to the real file.

This is 44.16's CSV widget generalised, and it must stay one widget: if you find yourself copying
`csv-table.ts`, stop and lift the shared part instead.

An embed whose file has moved says so where the embed is, naming the path it looked for.

### Story 45.13: Attachments From Anywhere

**Frontend + `keeper`.** Bindings: FR-188, FR-189, UX-DR76.

One insertion path, three entry points: a folder on the drive, a multiselection in the Files pane,
and the attachment panel 43.7 built. Into a note or into a quick capture, with the same result.

Selecting from Files offers a note to attach to — searchable — and refuses to write the same
attachment twice into one note, silently doing nothing being the wrong answer: say it is already
there.

---

## Wave 3 — The surfaces

### Story 45.14: Quick Capture Is the Note Editor

**Frontend.** Bindings: FR-190, AD-93.

Quick capture mounts `NoteEditor`. Markdown, the format toolbar, the `/` menu, tags and attachments
arrive together because they are the same component, not because this story reimplements five
things.

What stays quick capture's own is the window: its size, its position, its dismissal. Read
`use-notes-shortcut.ts` and the capture path in `notes_ipc.rs` before deciding where the seam is.

### Story 45.15: Quick Capture Is a Window You Own

**Frontend + `keeper`.** Bindings: FR-191, FR-192, UX-DR77.

A close button, not only Escape. A lock icon that makes the window movable and remembers where it
was put. Several capture windows at once, each holding its own note. And any note openable as one,
so the small window is a way of *looking* at a note rather than a special kind of note.

Several windows at once is the part that will break assumptions: find every place that assumes one
capture window exists and say what you found.

### Story 45.16: Quick Capture Knows Its Template and Its Tag

**Frontend + `keeper-core`.** Bindings: FR-193.

A quick capture applies a template — 44.7's, through 44.7's `expand`, not a second expander. Notes
it creates carry a tag, and the spaces that show captures select on that tag rather than on a path.

44.7's shipped templates deliberately add no tags of their own; read why before choosing this one.

### Story 45.17: Tags You Can Edit, Spaces and Notes You Can Delete

**Frontend + `keeper`.** Bindings: FR-194, FR-195, UX-DR78.

Edit a note's tags in the note — 44.14 made recording notes' tags editable through the properties
panel and 44.13 built the chooser; this generalises it and adds nothing new to the vocabulary.

Delete a space and delete a note, each confirmed, each naming what goes. A deleted default space is
44.3's business: it must not silently come back on the next seed, and the ledger already has the
concept — use it rather than inventing a tombstone.

### Story 45.18: A Note Knows Its File, a File Knows Its Note

**Frontend.** Bindings: FR-196, UX-DR79.

From a note, open its file in the Files pane. From a markdown file inside the notes vault, switch to
the Notes tab with that note open. Both are 45.1 targets, so this story is a resolution rule and two
actions, not a navigation system.

A markdown file *outside* the vault has no note to switch to, and the action is absent rather than
present-and-failing.

### Story 45.19: Recording Remembers the Last Session

**Frontend + `keeper`.** Bindings: FR-197, UX-DR80.

Every field of the "Next session" form is editable on the *last* recording, writing to its manifest
where 41.x put it.

A button from the Recording pane to the recordings space in Notes, when the two are linked. And from
a recording's note, "record another like this": it opens the Recording pane with the form filled from
that session's properties and stops — the owner presses Start. Never auto-start; a recorder that
begins without a deliberate press is a recorder people stop trusting.

### Story 45.20: Chrome That Makes Room

**Frontend.** Bindings: FR-198, UX-DR81, UX-DR82.

The menu and its submenus fold, with a folded rendering that is still navigable — icons that keep
their accessible names, not a strip of unlabelled glyphs.

Three smaller items that belong to the same surface: a **Recordings** entry in the menu bar that
opens the Recordings page; **Today's Journal** applying the journal template, which it does not
today (44.7 shipped `journal-entry.md` and this path never asked for it); and the space icon set
made much larger with a browsable chooser, including an icon that means *template* and a templates
space to go with it.

### Story 45.21: Export, and Comments on a PDF

**Frontend.** Bindings: FR-199, UX-DR83.

Export a note or a file to a location the user picks.

Then the speculative half, and treat it as 44.8 was treated: **decide what is honestly buildable
before building it.** Comments anchored to parts of a PDF, with a show/hide toggle, need an anchor
that survives the document being replaced — and if the honest answer is "annotations live beside the
file and anchor to a page and a rectangle", say so and build that well. A half-built annotation layer
that loses comments is worse than an export button and a written-down reason.

---

## What this epic must not repeat

Four defects shipped green in epic 44 and were found by hand:

- a `mermaid` fence crashes live-preview, because no test builds an `EditorView` with both the
  markdown language and the plugin (DW-171);
- three tray listeners were declared and never mounted, because `renderHook` mounts the hook itself
  and can never see that `App` does not (DW-172);
- `notes_create` could overwrite a note through an unreadable directory, because `dest` was a field
  every caller passed `None`;
- seeding blocked the app's startup thread on a removable volume, so a whole subsystem came up
  silent and could not recover.

The lesson they share: **a green suite and a green mutation sweep both only probe what the tests
already assemble.** Every story here that renders something must be exercised against a real
component in a real host — a real `EditorView`, a real panel, a real listing — and every story that
can decline to act must say so at INFO or above (DW-162).
