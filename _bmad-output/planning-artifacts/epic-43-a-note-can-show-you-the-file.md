# Epic 43 — A note can show you the file, and a tag can say no

status: draft
created: 2026-08-09
altitude: epic
parent: Phase 5 (Notes), Phase 6 (Recording × Sync), Epic 42 (the recordings archive)
source: a field report from the owner on their own machine, eight items, after epics 41 and 42
shipped and the first real recordings landed in a synced folder
binds: FR-146–FR-153, AD-73–AD-78, UX-DR53–UX-DR56

## Why this epic exists

Epic 42 ended with a note that knows where its files are: `session:`, `recording:` and `files:` in
the frontmatter, a dropdown that can reveal or copy each one, and — story 42.6 — a `keeper-recording://`
protocol that renders one `![[…mov]]` embed as a seekable video.

Then the owner used it, and every one of the eight things they asked for next is the same sentence
said from a different seat: **keeper knows where a file is and still will not show it to you.**

- A note lists four files and can play exactly one kind of them.
- A session has two videos of one moment and offers two unrelated players.
- The properties panel knows the attachments and the body cannot reach them.
- The synced folder holds everything and nothing in the app browses it.
- Getting a widget into a note means typing `![[` and a path by hand.

The two remaining items are not about files at all, and they are here because they are the same
size and arrived in the same breath: a tag you can *exclude* rather than only include, and spaces
you can edit. Plus one plain bug — Tab in the editor inserts whitespace nobody asked for.

## Where we take a position

**One attachment vocabulary, not three widgets.** The obvious reading of "add a photo widget and an
audio widget" is two more modules beside `recording-embed.ts`. That is the wrong shape: three
widgets means three parsers, three resolution paths, three degrade behaviours and three places to
fix the next bug. There is ONE question — *what is this file and how should it be shown* — and it is
already half-answered by `RecordingNoteTargetKind`. Widen the kind, keep one widget, branch at the
element (AD-73).

**The protocol stays contained; the kinds widen.** `keeper-recording://` is rooted at the effective
recordings destination and refuses anything that escapes it after canonicalisation. Serving images
and audio widens the *allow-list of extensions*, not the root. The files tab (43.8) reads synced
folders that are NOT the recordings root, and it must not reach for this protocol to do it — a
browser is a listing, and a listing is not a licence to serve bytes (AD-74).

**Two videos of one moment are one recording.** A screen track and a camera track from the same
session are not two files that happen to be adjacent; they are two views with one clock. One
transport, one scrub, one `±10s`. Volume and mute stay per track because that is a mixing decision,
not a time decision — the owner said so and they are right (UX-DR53).

**An exclusion is a first-class term, not a syntax.** `-tag` typed into a search box is a power
feature that nobody discovers. The tag already renders as a chip in the tree and in the filter bar;
a chip that can be include, exclude or off is one control the user can *see* has three states
(UX-DR54). The space DSL already has the grammar underneath — this gives it a face.

**The files tab is a reader, and it stays one.** No rename, no delete, no move. keeper's whole
promise about a synced folder is that it never moves a file you did not ask it to move, and a
browser with a delete key in it is the shortest path to breaking that promise by accident (AD-75).

## Stories

### Story 43.1: Tab Belongs to the Editor
**Frontend-only.** Bindings: no. Binds FR-146.

Pressing Tab while editing a note does not indent — it inserts whitespace and, in the owner's words,
"maybe two blank lines go in". CodeMirror's default `Tab` binding is deliberately *not* indentation
(it moves focus, for accessibility), so an editor that wants indentation must bind it explicitly and
must keep an escape hatch so the keyboard trap the default exists to prevent is not reintroduced.

Tab indents the line or the selection; Shift-Tab outdents; `Escape` then `Tab` still leaves the
editor, which is the accessibility contract. Inside a list, Tab nests the item rather than inserting
a literal tab. Nothing is inserted that the markdown does not mean.

**AC:** every one of Tab, Shift-Tab, Tab-with-a-selection, Tab-in-a-list and Escape-then-Tab does
exactly one named thing, asserted; and the reported symptom — stray blank lines — cannot recur,
asserted against the document text and not against a keymap table.

### Story 43.2: A Recording Note Says It Is One
**`keeper-core` only.** Bindings: no. Binds FR-147.

`compose` writes `session:`, and 42.6 made that the predicate for the Recordings lens. But a human
browsing the tag tree sees nothing: the note carries the session's own tags and no tag saying what
KIND of note it is. Add `recordings` to the stub's tags, resolved against the same vocabulary
story 42.5 established, so it lands in the tree beside every other tag rather than as a second
convention.

**AC:** a stub carries `recordings` in addition to the session's own tags, exactly once even when
the session was tagged `recordings` by hand; and the tag is the vocabulary's, not a literal — a
session tagged `Recordings` does not produce two.

### Story 43.3: Include, Exclude, Off
**Frontend + `keeper-core` predicate.** Bindings: maybe. Binds FR-148, UX-DR54.

A tag chip in the filter bar and in the tag tree is a three-state control: off, include (`+`),
exclude (`−`). Excluded tags narrow the list the way included ones widen it, and the two compose:
`client/acme` and not `draft`.

The predicate lives where 42.6 put the text matcher — `keeper-core`, one definition, testable on
Linux. The DSL that spaces already speak gains nothing new to parse; this is a face for grammar
that exists.

**AC:** the three states cycle and are visible without hovering; an exclusion removes notes an
inclusion would have shown; include-and-exclude of the same tag is impossible to express rather
than resolved by precedence; and the empty result says which term emptied it.

### Story 43.4: Spaces You Can Edit
**Frontend + the space store.** Bindings: maybe. Binds FR-149, UX-DR55.

A space is a saved filter. It can be created ("Save as space") and never edited: changing one means
deleting it and building it again from memory. Give it an edit surface — rename, change its icon,
and adjust its terms with the same three-state chips story 43.3 builds.

**AC:** editing a space changes what it selects; the icon is chosen from a fixed set and persists;
cancelling leaves the saved space byte-identical; and a space whose every term was removed refuses
to save rather than silently becoming "everything".

### Story 43.5: One Attachment Vocabulary
**`keeper-core` + `keeper` shell + frontend.** Bindings: YES. Binds FR-150, AD-73.

Widen `RecordingNoteTargetKind` from `folder | video | file` to name what a file IS —
`video | image | audio | file | folder` — and let the one embed widget branch on it: a `<video>`
for video (unchanged), an `<img>` for an image, an `<audio>` for audio, and a file chip carrying the
existing reveal / copy-path actions for everything else. `keeper-recording://`'s extension
allow-list widens to match; its root and its containment checks do not move.

`file` keeps its existing spelling deliberately. The enum answers one question — how should this be
shown — and its last non-folder variant covers a `.zip`, a `manifest.json`, a `.partial`
mid-rotation and an extensionless dotfile. An earlier draft of this epic called it `document`,
which is a misnomer for most of them and would have read as a bug to the next person.

The chip is the point of the story as much as the players are: an attachment keeper cannot render is
still an attachment, and rendering it as a dead link is what 42.6 already refuses to do.

**AC:** each kind renders its element and no other; an unknown extension is a chip, never a broken
player; the protocol still refuses a path that escapes the root after canonicalisation, asserted;
and the bindings regenerate with no drift.

### Story 43.6: Two Videos, One Transport
**Frontend-only.** Bindings: no. Binds FR-151, UX-DR53.

When a note embeds two videos from the same `session:`, they render under one transport: one scrub
bar, one play/pause, one `±10s`, and one current-time readout. Volume and mute stay per track.

The hard part is not the controls, it is the truth: two `<video>` elements drift, and one of them
will stall on a seek while the other does not. The transport must show the state of the pair rather
than the state of whichever element it asked last, and a seek that only half-lands must be visible
rather than silently desynchronised.

**AC:** play, pause, scrub and `±10s` move both; volume and mute move one; a stalled track shows the
pair as buffering rather than playing; and drift beyond a stated threshold is corrected toward the
scrub position, with the threshold named in the code and asserted.

### Story 43.7: The Attachment Panel
**Frontend + the existing `recording_note_targets` command.** Bindings: no. Binds FR-152, UX-DR56.

The properties panel knows a note's attachments (42.6) and the body can render them (43.5), and
there is no way to get from one to the other except typing `![[` and a path. Give the editor a panel
that lists the note's attachments and inserts the embed at the cursor.

It is an inserter, not a renderer: what it writes is the ordinary Obsidian embed, so a note written
through the panel is byte-identical to one typed by hand and stays legible in Obsidian.

**AC:** the panel lists exactly the note's own attachments; inserting writes the same text a user
would type; inserting twice does not duplicate; and a note with no `session:` gets a panel that says
so rather than an empty one.

### Story 43.8: The Files Tab
**Frontend + `keeper` shell.** Bindings: YES. Binds FR-153, AD-74, AD-75.

A tab that browses every synced folder as a tree: the recordings, the notes vault, and everything
else the folder holds. Read-only by construction (AD-75) — reveal in Finder, copy path, open with
the system default, and nothing that writes.

Listing is the shell's job, not the frontend's: one command returns a directory's entries with the
same `kind` vocabulary story 43.5 establishes, resolved against the profile's own root, so no
frontend surface ever joins a root and a subpath (AD-65 again). Tier-0 exclusions apply — a browser
that shows `.keeper/` and `node_modules/` is a browser nobody scrolls twice.

**AC:** every enabled profile appears; a folder lists its children on demand rather than eagerly
(these trees hold 100 000 files); an unplugged removable profile says the drive is out rather than
showing an empty folder; `..` cannot escape the profile root, asserted; and nothing in the surface
can write, delete or move.

## Sequencing

43.5 precedes 43.6 and 43.7 — a transport needs players and a panel inserts what the widget renders.
43.3 precedes 43.4 — a space editor reuses the three-state chip. Everything else is independent.

```
wave 1:  43.1   43.2   43.3   43.5   43.8
wave 2:         43.4 (needs 43.3)   43.6 (needs 43.5)   43.7 (needs 43.5)
```

## Out of scope

- Writing, renaming, moving or deleting from the files tab (AD-75).
- Serving bytes from a synced folder that is not a recordings root — the files tab lists and reveals;
  it does not stream (AD-74).
- Transcription, thumbnails, or any derived media. Every element here plays or shows a file that
  already exists.
- A second embed syntax. Obsidian's `![[…]]` is the one form, and a note keeper wrote must stay a
  note Obsidian renders.
