# Epic 44 — The vocabulary is the space, and the note is a document

status: draft
created: 2026-08-09
altitude: epic
parent: Phase 5 (Notes), Epic 42 (the recordings archive), Epic 43 (a note can show you the file)
source: a second field report from the owner after epic 43 was installed and used — twenty-two items
binds: FR-154–FR-172, AD-79–AD-86, UX-DR57–UX-DR64

## Why this epic exists

Epic 43 made a note able to *show* a file. Using it produced twenty-two items, and they are not
twenty-two features. They are four sentences:

1. **The rail lies about its own vocabulary.** Today, Inbox, Journal, Pinned and Recordings are
   hard-coded rows; Spaces are a second, weaker thing underneath them. The owner asked for the fixed
   rows to *become* spaces — which is the correct read, because they already are saved filters and
   the only difference is that four of them are unteachable. One of them, Today, does nothing at all
   and cannot even be clicked.

2. **A note cannot be authored, only typed.** No New Note. No template. No formatting menu — the
   owner knows markdown and says plainly they would rather not have to. No table builder. No way to
   put a gallery, a CSV, or the recording's own videos into the body without hand-writing the
   syntax. Epic 43 added an attachment panel and a `/` menu; both insert *one* thing.

3. **Every list is a fixed-width guess.** Columns cannot be resized, content is cut rather than
   fitted, a property that does not fit is truncated with nowhere to read the rest, there are no
   counts, and every list renders every row.

4. **What epic 43 shipped for two videos is not finished.** With one video it is right. With two,
   the transport wraps into "Back 10 / Pla y / Forward 10", the mute sliders float away from their
   tracks, and — the part that is not cosmetic — both videos render as empty grey boxes.

## Where we take a position

**Spaces are the only vocabulary; the fixed rows become instances of it** (AD-79). Not "spaces plus
four special cases": the four are converted into default spaces, seeded on first run, editable like
any other. This is the change that makes every other space feature — icon, order, sort, default
template — apply to Inbox and Recordings for free, instead of being a feature only power users see.
A default space is deleteable, and restoring the set is one action; keeper does not own rows in
someone's own vault forever.

**Today is deleted, not fixed** (AD-80). It renders, it is not clickable, and there is no query it
could run that `date:created>=today` does not already express as an ordinary space. A row that has
never worked is not a feature with a bug; keeping it because it is on screen is how surfaces rot.

**Order is a property of the note, and the sort is a property of the space** (AD-81). Two different
facts and they must not be one setting. A note's `order` travels with it — it is frontmatter, it
survives a clone, and Obsidian shows it — while "sort this space by order, or by name, or by
recorded date" is a lens the viewer chose. Every note gets a default so a list is never half-ordered,
and the list shows each note's order beside it, because an ordering the reader cannot account for
reads as randomness.

**A template is a note, and its tag is the marker that leaves** (AD-82). Not a new file type, not a
directory keeper owns: a note tagged `template`, which is why it is searchable, syncable and
editable with the tools that already exist. Making a note *from* it copies the body and drops that
one tag — the copy is not a template, and leaving the tag on would make every note a template of
itself. Propagating a template edit to notes already made from it is offered, never automatic:
overwriting what someone wrote is not a feature (UX-DR59).

**Fit, then truncate, then offer the rest** (AD-83). In that order. Columns size to their content
first, the user may resize, and only what still does not fit truncates — with the whole value one
click away. A cut value with nowhere to read the rest is the failure this replaces.

**Render what is on screen** (AD-84). Every list and every gallery. A vault of ten thousand notes
and a folder of a thousand photos are both ordinary here, and a surface that renders all of them is
one that stops responding on the machine that has the most to show.

## Stories

### Story 44.1: Two Videos, Properly
**Frontend-only.** Bindings: no. Binds FR-154, UX-DR57.

The follow-through on 43.6. With two tracks embedded the videos render as empty grey boxes, the
transport wraps its labels mid-word, and each track's mute slider sits away from the track it
governs. Establish first WHY the elements are blank — that is a functional defect and it may not be
CSS at all — and report it before styling anything.

**Measured during implementation, and it changed this story.** `preload="metadata"` stops at
`HAVE_METADATA`, and the spec says a video element in that state with no video data obtained
represents *transparent black*; WebKit obeys, and the pane's own `--muted` shows through. The
finding is that the SINGLE video is equally frameless — 42.6 shipped a player that has never painted
a frame, and it read as working only because WebKit draws its native controls chrome over the
emptiness. The transport strips those controls at two tracks and the emptiness becomes visible.

So this is not a two-video regression. It is a one-video defect that two videos exposed, and the
fix applies to both — the original AC below said the single case must stay unchanged, which was
written to prevent a regression and would instead have preserved a defect in the common path. The
seek that produces a frame costs a range request against real bytes on removable media, and that
price is named in the story's spec rather than left to be discovered.

Then: the pair reads as one player. A row of two tracks side by side, one transport beneath, each
track carrying its own compact mute/volume. Labels are glyphs with accessible names, not wrapped
sentences.

**AC:** the blank-video cause is named and fixed for one track and for two; the transport does not
wrap at the pane's minimum width, asserted; each volume control is inside its own track's box; and
the seek's cost on removable media is stated in the spec.

### Story 44.2: The Stub Embeds Its Own Videos
**`keeper-core` only.** Bindings: no. Binds FR-155.

The note written when a recording stops lists its files in frontmatter and shows a blank body. Put
the embeds in the body, so the note opens as the recording.

Insertion must go BELOW the heading. Story 43.7's panel inserts at the caret, and a caret at
position zero put the embed above the `# Title` — which made the embed the note's first line and
therefore its displayed title. Do not reproduce that here, and say in the spec where the body's
first insertion point is and why.

**AC:** a two-track session's stub carries both embeds under the heading, in ledger order; the
heading remains the note's title; a session with no video embeds nothing; and the body stays valid
Obsidian markdown.

### Story 44.3: Default Spaces Replace the Fixed Rail
**Frontend + `keeper` shell.** Bindings: maybe. Binds FR-156, AD-79, AD-80.

Inbox, Journal, Pinned and Recordings become seeded default spaces. Today is deleted. The rail
renders spaces and nothing else.

Seeding happens once per vault and is recorded, so a deleted default stays deleted. "Restore default
spaces" re-creates the missing ones without touching the ones that are there.

**AC:** a fresh vault shows the four; deleting one and reopening does not resurrect it; restore
re-creates only what is missing; an existing vault migrates without losing a user's own spaces; and
`Today` is gone from the rail, the store and the tests.

### Story 44.4: A Space Has an Icon, an Order and a Sort
**`keeper-core` + shell + frontend.** Bindings: YES. Binds FR-157, FR-158, AD-81.

Widen 43.4's editor: the icon set grows to cover the defaults and more; a space carries an `order`
that positions it in the rail; and a space carries a sort — `order`, `name`, `created`, `modified`,
or `recorded` — applied to the notes it lists.

`recorded` sorts by the session's own timestamp and is meaningful only where a note has one; say
what it does for a note that does not, rather than sorting it to an arbitrary end.

**AC:** each sort orders the list and is asserted against a fixture whose natural order differs from
every other sort; the rail honours space order; the icon set covers every default; and an unknown
sort in frontmatter falls back visibly rather than silently.

### Story 44.5: A Note Has an Order You Can See
**`keeper-core` + frontend.** Bindings: maybe. Binds FR-159, AD-81.

Notes carry an `order` in frontmatter, defaulted so no list is half-ordered, and the list shows it
beside the note — an ordering the reader cannot account for reads as randomness.

Decide and justify the default: a constant makes every note tie, a timestamp makes order a second
copy of a date. Say which you chose and what a tie does.

**AC:** the default exists for a note that has never had one; the list renders it; reordering
persists to frontmatter; and a tie resolves by a stated rule rather than by map iteration.

### Story 44.6: New Note
**Frontend + shell.** Bindings: maybe. Binds FR-160, UX-DR58.

There is no way to create a note. Add it: from the rail, from a space (inheriting that space's tags
and its default template), and from the command palette.

**AC:** a note created inside a space is selected by that space when it appears; the space's default
template is applied; the caret lands in the body; and creating from a space whose template was
deleted still creates a note and says the template is missing.

### Story 44.7: Templates
**`keeper-core` + shell + frontend.** Bindings: YES. Binds FR-161, FR-162, AD-82.

A template is a note tagged `template`. Creating from one copies the body, resolves its
placeholders, and drops that one tag. A space names a default template. Ship templates for journal,
recordings and inbox.

Placeholders are the existing `{yyyy}`/`{mm}`/`{dd}` vocabulary the recording path template already
uses (Story 21.x) — one substitution grammar in this app, not two.

**AC:** the copy carries every tag but `template`; placeholders resolve; a template that is itself
edited is not retroactively applied (44.8 owns that); and a note created from a missing template is
created anyway.

### Story 44.8: Update Notes From Their Template
**Shell + frontend.** Bindings: maybe. Binds FR-163, UX-DR59.

Editing a template offers to update notes made from it. Offered, never automatic, and never a
silent overwrite: the note records which template made it, the update shows what would change, and
the user chooses.

The hard part is not the diff, it is what "update" means for a note somebody has written in since.
Decide, justify, and make the destructive reading impossible to trigger by accident.

**AC:** notes made from the template are found; the preview shows per-note changes; declining
changes nothing; accepting is undoable through the existing note history; and a note edited since
creation is treated by the stated rule, asserted.

### Story 44.9: A Formatting Menu
**Frontend-only.** Bindings: no. Binds FR-164, UX-DR60.

A toolbar over the editor: bold, italic, strikethrough, code, heading levels, bullet and numbered
lists, quote, link, and a table builder that asks for rows, columns and whether the first row is a
header.

Every action is a CodeMirror command over the selection and round-trips: applying bold to bold text
removes it. The table builder writes an aligned GFM table, because a table nobody can read in the
source is a table nobody will edit by hand afterwards.

**AC:** each action applied to a selection produces the exact document text asserted; each is
idempotent-by-toggle where markdown allows; the table builder's output parses as a GFM table; and
the toolbar does not steal the caret.

### Story 44.10: Lists That Render What You Can See
**Frontend-only.** Bindings: no. Binds FR-165, AD-84.

Virtualise the note list, the recordings list and the files tree: render the viewport, not the
vault. Keep keyboard navigation, the roving tabindex 43.8 built, and the selection.

**AC:** a fixture of several thousand rows mounts a bounded number of nodes, asserted by counting
them; scrolling to the end reaches the last row; keyboard navigation still moves one row at a time
through rows that were never rendered; and the selected row survives scrolling out and back.

### Story 44.11: Counts
**Frontend + whatever already counts.** Bindings: maybe. Binds FR-166.

Every list says how many. A space says how many notes it selects, the recordings list how many
sessions, a folder how many entries.

Say whether the count is of what is loaded or of what exists — with 44.10 those differ, and a count
that means "loaded so far" while looking like a total is worse than none.

**AC:** the count is the total, not the rendered window, asserted with virtualisation on; a filtered
list counts the filtered set; and an empty set says zero rather than hiding the count.

### Story 44.12: Columns You Can Size, Content You Can Read
**Frontend-only.** Bindings: no. Binds FR-167, FR-168, AD-83.

Panes get resizable columns with persisted widths. Content fits before it truncates. A value that
still does not fit ends in `…` and opens its whole self on click — the Properties panel is where the
owner met this and it is not the only place.

**AC:** a drag resizes and persists across a reload; a narrow column truncates rather than clipping
mid-glyph; the overflow affordance shows the complete value including one long enough to need
scrolling; and a keyboard user can reach the full value.

### Story 44.13: Tag Entry That Completes
**Frontend-only.** Bindings: no. Binds FR-169, UX-DR61.

Choosing a tag is a dropdown. Make it a text field that completes as you type, with the list still
rendered for browsing — both, not one. Every place a tag is chosen uses it, including 43.3's chips
and 43.4's space editor.

**AC:** typing filters; the list stays browsable; a tag not in the vocabulary can still be created
where creating is allowed and cannot where it is not; and the control is reachable and operable by
keyboard alone.

### Story 44.14: Recording Notes Are Editable Notes
**Frontend-only.** Bindings: no. Binds FR-170.

The recording note's tags cannot be edited from the note. They are keeper's frontmatter and the
panel renders them inert. Let them be edited — they are the user's tags, in the user's note.

Keep `recordings` (43.2) and `session:` protected: one is keeper's classification and the other is
the identity everything resolves through.

**AC:** a tag can be added and removed and the file changes; `session:` cannot be edited from the
panel; removing `recordings` is refused with a reason; and the tag vocabulary is the same one 42.5
owns.

### Story 44.15: A Gallery in a Note
**Frontend + shell listing.** Bindings: maybe. Binds FR-171, AD-84.

A gallery block over a folder of media — images, video, audio — rendered in the note, virtualised,
with per-note pinning that floats chosen items to the top and is stored in that note rather than in
the folder.

Reuse 43.8's listing and 43.5's kinds; a second file classifier is not allowed.

**AC:** a folder of hundreds mounts a bounded number of tiles; a pin persists in the note and is
invisible to other notes; a non-media file is skipped rather than rendered as a broken tile; and an
unreadable folder says so.

### Story 44.16: CSV as a Table You Can Edit
**Frontend + `keeper-core` parser.** Bindings: maybe. Binds FR-172.

A CSV attachment renders as a table in the note and can be edited there, writing back to the file.

The parser goes in `keeper-core` — quoting, embedded newlines and separators are exactly the kind of
thing a hand-rolled TypeScript split gets wrong on someone's real export. Round-tripping must not
reformat rows the user did not touch, for the same reason `Frontmatter` refuses to.

**AC:** a file with quoted commas, embedded newlines and a BOM round-trips byte-identically when
untouched; an edited cell changes that cell and nothing else; a malformed row is shown rather than
dropped; and the file on disk is what the table showed.

### Story 44.17: Sync Status in Files
**Frontend + existing sync state.** Bindings: maybe. Binds FR-173.

The files tab shows nothing about whether a file is synced. Mark each entry: synced, waiting,
excluded, or not in a repository at all.

Read the state that exists — the engine already knows — and do not make the browser a second source
of sync truth (AD-74's spirit: a listing is not a licence).

**AC:** each state renders distinctly; an excluded file says excluded rather than waiting forever;
the mark updates when sync progresses; and browsing still perturbs nothing in the engine.

## Sequencing

```
wave 1:  44.1  44.2  44.3  44.9  44.12  44.13
wave 2:  44.4 (needs 44.3)   44.5   44.10   44.14   44.17
wave 3:  44.6 (needs 44.3/44.4)   44.7 (needs 44.6)   44.11 (needs 44.10)   44.15   44.16
         44.8 (needs 44.7)
```

## Out of scope

- Any second substitution grammar, file classifier, embed syntax or tag vocabulary. Each of those
  exists exactly once and this epic adds none of them.
- Writing to a synced folder from the files tab. AD-75 stands.
- Retroactive template application without consent (UX-DR59).
