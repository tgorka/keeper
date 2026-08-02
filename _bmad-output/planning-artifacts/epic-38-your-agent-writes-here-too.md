# Epic 38 — Your agent writes here too

status: draft
created: 2026-08-02
altitude: epic
parent: Epic 35 (the vault is a folder you already sync), Epic 37 (a place to read and write)
source: `product-inputs-notes-2026-08-02.md` (the numbering spine), the divergent session in
`brainstorm-keeper-notes-2026-08-02/`, and a full read of `keeper-sync`'s `watch.rs`,
`stability.rs`, `provenance.rs` and the conflict-copy path in the git engine
binds: FR-112, FR-113, FR-114, FR-116, NFR-29, NFR-30, AD-63, UX-DR39

## Why this epic exists

The brainstorm called this the differentiating cluster, and then said something more useful
about it: **it is nearly free.** Unread agent marks, per-note history, "who changed this and
on which machine" — all of it is a projection of commit provenance the sync engine already
writes on every change. `provenance.rs` has been stamping `Keeper-Device`, `Keeper-Origin` and
`Keeper-Source` trailers on every engine-authored commit since story 28.1, and story 34.10
made `Keeper-Source` honest for manual syncs. Nobody has ever read them back for a user.

So AD-63 is not an architecture decision so much as an instruction not to build the obvious
thing: **keeper adds no parallel history store.** "Who changed this note" is a git question and
git already has the answer.

What is *not* free is the other half — making a live editor safe while another writer has the
same file open. That is the part with real engineering in it, and the phase's position on it is
categorical: **never lock a note; watch it.**

### The one push stream problem

There is exactly one push stream to the webview today: `SyncProgressVm` over
`tauri::ipc::Channel`. There is no per-file change event anywhere in the codebase — the sync UI
learns that something happened by re-reading a status snapshot. That is adequate for a progress
bar and hopeless for an editor: NFR-29 requires an external write reflected in the UI within
one second, and a poll that meets that would poll a 10 000-note vault every second, which
NFR-28 forbids in the same breath.

The detection side, though, already exists and is already fast. Story 34.9 wired
`watch::FolderWatcher` into the supervisor: a `notify` watcher per profile with a 500 ms
debounce and a 15-minute backstop rescan, feeding `note_close_write` in `stability.rs`. An
agent, another editor, or a `git checkout` writing inside a synced folder is *already*
observed within about half a second. Story 38.1 is the missing half — turning that observation
into a typed event the webview receives, and into a single-record `IndexDelta` rather than a
rescan.

### Where we take a position

**A dirty buffer is merged, not defended.** The tempting behaviours are both wrong: refusing
the external write (which loses the agent's work, or forces a conflict copy for a change that
did not conflict) and clobbering the buffer (which loses the user's). Non-overlapping hunks
merge silently with an inline diff bar saying what arrived; overlapping hunks fall through to
the conflict path in story 38.6, which preserves both. **Never a modal** — a modal at the
moment an agent writes is a modal that fires while the user is typing in another app.

**The dot is the headline, not the polish.** The brainstorm flagged the tray glyph gaining a
subtle dot when the agent has touched notes you have not read as the breakthrough wildcard, and
UX-DR39 makes silence a defect: a dot on the tray glyph, an unread mark on the row, a diff in
the editor. Three surfaces, one state. Story 38.3 owns all three, and it depends on story 36.1
having made the glyph visible on Linux first — a dot nobody can see is not an indicator.

**Nothing is deleted without a commit that still holds it.** NFR-30 is the phase's one
unacceptable failure, and this epic is where it is most at risk, because this is the epic that
overwrites note bodies with content that came from somewhere else. Every write path here — the
merge apply, the diff accept, the conflict resolve — either commits the prior content first or
leaves a conflict copy. Each story's acceptance says which.

## Stories

### Story 38.1: A Note Change Reaches the Webview
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 35.6, 37.2.

A second push stream: `Channel<NoteChangeVm>` in `keeper/src/notes_ipc.rs`, fed by
`notes_vault::registry`'s subscription to the existing per-profile `notify` stream. Each event
carries the note id, the relative path, the new mtime, a content hash and the change kind, and
is applied to the resident index as a single-record `IndexDelta` (story 35.4) — never a
rescan. Events coalesce: a burst inside the watcher's 500 ms debounce produces one event per
path, and a bulk change produces one batched message rather than one per file. The 15-minute
backstop rescan reconciles anything the watcher dropped, and an editor-atomic
rename-into-place is treated as a modification of the destination, which is the failure mode
the brainstorm named explicitly.
AC: `echo x >> note.md` from a shell is visible in the list within 1 s, measured on macOS and
Linux (NFR-29); a `git checkout` touching 500 notes produces one batched message and 500
single-record deltas, not 500 messages and not a rescan, asserted by a counter; killing the
watcher degrades to the backstop rescan loudly, never silently; `bun run bindings:check`
passes.

### Story 38.2: A Clean Buffer Applies, a Dirty Buffer Merges
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 37.6, 38.1.

`keeper-core/src/notes/merge.rs` — a pure three-way merge over the base revision the buffer
opened at (story 37.6 already carries it), the buffer, and the incoming content. A clean buffer
takes the external write live with a fading highlight over the changed lines. A dirty buffer
takes every non-overlapping hunk and raises an inline diff bar naming what arrived; overlapping
hunks are not merged and route to story 38.6. No modal anywhere in the path (UX-DR39).
AC: an agent appending a section while the user edits a different paragraph converges with no
prompt and no lost characters, asserted by a property test over randomised non-overlapping edit
pairs; an overlapping edit raises the bar, keeps the user's buffer intact, and loses nothing on
either side; the caret and selection survive an applied external write; no code path in the
merge writes to disk — the crate that owns it cannot (AD-55).

### Story 38.3: Unread, and the Dot on the Tray
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 36.7, 38.1.

Origin comes from the sync engine's existing commit trailers, parsed back through
`keeper-sync`'s `provenance.rs`: a change whose `Keeper-Device` is not this machine, or whose
`Keeper-Origin` says it came from an agent, is non-local. A per-note `last_read_ms` lives in
the `.keeper/` cache — advisory, like everything else there, so losing it marks things unread
rather than losing data. The row gains an unread mark, `decide_tray_state` gains a
notes-unread input, and the glyph set gains a dotted variant in both the macOS template and the
Linux-visible families from story 36.1. No parallel history store is introduced (AD-63).
AC: an agent commit pulled from the remote marks exactly its notes unread on this machine; the
tray dot appears within one supervisor tick of that pull and clears when the last unread note
is accepted; the dot is visible on an XFCE panel in both themes; deleting `.keeper/` marks
everything unread and loses no note content; a hand-made `git` commit with no trailers is
treated as unknown origin, not as local.

### Story 38.4: The Diff, and Accept
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 38.3.

Opening an unread note offers the diff of the non-local change that marked it — computed in
Rust from the two blobs the sync engine already holds, never by re-reading a "previous version"
keeper saved separately. Accept clears the mark and stamps `last_read_ms`; dismissing the diff
without accepting leaves both the mark and the dot.
AC: the diff shows exactly the hunks between the revision last read and the current one, across
a rename; Accept clears the row mark and, when it was the last one, the tray dot; closing the
note without accepting leaves both set after a restart; a note changed twice since last read
shows one cumulative diff, not two.

### Story 38.5: Note History, With Its Provenance
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 38.3.

Per-note commit history projected from `keeper-sync` — rename-following, so a note's history
survives the filename changes its ULID identity already survives — with each revision carrying
device, origin and source parsed from the trailers, and a diff per revision. Projection only:
no new table, no new file, nothing written (AD-63).
AC: a note renamed twice lists its full history including revisions from before both renames;
every engine-authored revision names the machine that wrote it and whether it was an agent, a
watch, a manual sync or a pull; a hand-made commit without trailers is listed as unknown
origin rather than dropped; opening history on a 400-revision note is bounded and paginated,
not a full revwalk into memory.

### Story 38.6: A Conflict Is a Row, Not Litter
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 38.2, 38.4.

Conflict copies inside a vault — the engine's existing
`<stem>.sync-conflict-<UTC>-<device>.<ext>` shape from story 24.7 — are recognised, folded into
their parent note as a conflict row in the list rather than surfacing as two unrelated notes,
and resolved inside the editor: take mine, take theirs, or edit the merged result. Resolution
writes the chosen body and removes the conflict copy in one commit, so the commit that deletes
the copy is the commit that still contains it.
AC: a two-sided edit produces one conflict row, not two note rows, and the row states both
devices and both times; resolving leaves no `.sync-conflict-` file in the vault and one commit
whose parent tree still holds both revisions (NFR-30); a conflict copy created by another tool
with the same naming shape is recognised identically; declining to resolve leaves both files
untouched and the row present after a restart.

## Out of scope

- A batched "what did the agent change everywhere" activity feed. The open question from the
  brainstorm is answered for this phase as per-note review; a cross-vault feed is a later
  question and is not stubbed here.
- Blame at line granularity. History is per revision.
- Any new provenance field. If a question cannot be answered from the trailers the engine
  already writes, it is not answered this phase.
- Merging overlapping hunks by heuristic. They go to the conflict path, deliberately.
- Publishing a note or its diff into a Matrix room — declared out of phase.
