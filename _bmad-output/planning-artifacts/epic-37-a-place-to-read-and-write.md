# Epic 37 — A place to read and write

status: draft
created: 2026-08-02
altitude: epic
parent: Epic 35 (the vault is a folder you already sync), Epic 36 (capture in two seconds)
source: `product-inputs-notes-2026-08-02.md` (the numbering spine), the divergent session in
`brainstorm-keeper-notes-2026-08-02/`, and a full read of the frontend shell
(`primary-view.ts`, `app-shell.tsx`, `sidebar-pane.tsx`), `media_protocol.rs` and the
`Channel<SyncProgressVm>` streaming precedent
binds: FR-103–FR-111, FR-118, FR-119, FR-123, AD-58, AD-59, UX-DR36, UX-DR37, UX-DR38,
UX-DR40, UX-DR41, UX-DR44

## Why this epic exists

Epic 36 makes notes arrive. This epic is where they are found again — the "find the thing I
wrote three months ago in five seconds, from a half-remembered fragment" job — and where they
are actually written at length rather than caught.

It is the largest epic in the phase because it is almost entirely greenfield on the frontend
side. The recon is unambiguous: the app today has **no markdown renderer, no syntax
highlighter, no mermaid, no editor component and no virtualised list**. `package.json`'s
dependency set is a shell toolkit — Radix, cmdk, zustand, tailwind — and nothing that reads or
renders a document. Every one of those is introduced here, and each one passes
`check:licenses` before it lands (CodeMirror 6 and mermaid are pre-cleared MIT in the phase
inputs; the virtualiser is the one remaining choice and is held to the same bar).

### The rule that shapes every story here

**Rust composes, the webview renders.** This is not a preference in this epic, it is a
capacity constraint. A 10 000-note vault cannot send its bodies over IPC, and it must not send
its rows as anything but view models. So:

- The list carries `NoteRowVm`s composed in Rust. Never a body, never a full frontmatter map.
- A body streams over a `Channel` when the note is opened. Today the *only* push stream to the
  webview is `SyncProgressVm` over `tauri::ipc::Channel`; the body channel is the second, and
  it is the precedent story 38.1 will follow for change events.
- An image is never base64 over IPC. It is a `keeper-note://` URL, and that scheme is a clone
  of `media_protocol.rs` — 512 lines that already solve async handling, range requests, MIME
  and the not-found path.

Both halves of AD-58 have a failure mode the acceptance criteria name explicitly, because both
are the kind of thing that works in a fixture and dies in a real vault.

### Where we take a position

**The list is the product; the editor is the guest.** UX-DR37 says the filtered list is the
primary surface. That is a claim about layout — the list occupies the middle pane where the
chat list lives, and the editor occupies the conversation pane — and a claim about behaviour: a
filter change is a *filter*, not a navigation (UX-DR41). Change a tag chip, switch to the
folder lens, switch the vault: the note under the cursor survives all three. Every story that
touches a filter carries that as an acceptance criterion, because it is the difference between
a tool you explore in and one you keep losing your place in.

**A space is a note, and its query is a grammar we own.** FR-105 requires a saved query that
syncs, diffs and is agent-editable — which rules out a serialised store and rules in a plain
note under `spaces/`. It also rules out anything resembling `eval`: the query is a small total
grammar over tag, path, field and date predicates, parsed in `keeper-core` and evaluated
against `NoteRecord`. A malformed query is a parse error rendered in the row, and it matches
*nothing*. It never matches everything — a query that silently widens is how a saved view
becomes a data-loss story when someone bulk-edits from it.

**Degrade to source, never to a box.** UX-DR44. A mermaid diagram that fails to parse renders
its error and its source; an image whose file is missing renders its alt text and its path. An
empty rectangle tells the user their note is broken when in fact it is intact on disk, and that
is the single most alarming lie a note app can tell.

## Stories

### Story 37.1: Notes Is a View, and the Vault Switcher Is Where the Account Switcher Is
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 35.6, 36.2.

`PrimaryView` in `src/lib/stores/primary-view.ts` gains `"notes"`; the ternary chain in
`src/components/layout/app-shell.tsx` and `BASE_VIEWS` in
`src/components/layout/sidebar-pane.tsx` gain the row, gated on the notes capability so it is
absent — not disabled — where the capability is off. A vault switcher occupies the position and
affordance of the account switcher (UX-DR36), backed by `notes_vault_list` and
`notes_vault_set_active` in a new `keeper/src/notes_ipc.rs`. Switching sets the active vault in
the Rust registry (story 35.6) and re-projects; the view does not remount.
AC: with no notes-flagged profile the Notes row is absent from the sidebar; switching vault
keeps the Notes view mounted, keeps the list's scroll position, and performs no filesystem
read; the switcher reads visually as the account switcher's sibling, not as a dropdown bolted
into the header; `bun run bindings:check` passes.

### Story 37.2: The List Projects, Virtualises, Pins and Archives
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 37.1.

`notes_ipc.rs` gains `notes_list(query: NoteQuery) -> NoteListVm` composing `NoteRowVm`s in
Rust — id, title, excerpt, mtime, tag set, pinned, archived, origin, conflict marker — and
nothing that resembles a body (AD-58). `NoteQuery` carries every FR-103 axis (text, tags,
space, date range, origin, pinned) from the outset even though the chips that drive them land
in 37.3–37.5, because widening a query type later is a bindings churn for no gain. `pinned` in
frontmatter floats a note to the top; `archived` removes it from the default lens without
deleting anything (FR-119). The frontend renders the app's first virtualised list.
AC: a 10 000-note vault paints its first screen in under 100 ms, measured and recorded; the
serialised payload for one query is bounded independent of vault size, asserted by a size
assertion at two vault sizes; toggling pin on a note rewrites only that note's frontmatter and
moves only that row; an archived note is absent from the default lens and present in the
archive lens with its file untouched on disk.

### Story 37.3: Tags Are a Tree, Chips Are the Filter
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 37.2.

`keeper-core/src/notes/tags.rs` extends story 35.4's tree: frontmatter `tags` and inline
hierarchical `#a/b` tags in the body merge into one set per note, and the tree carries counts
at every level. The frontend renders the tree in the leading pane and the active tags as chips
over the list; multiple chips **intersect**. Adding or removing a chip is a filter, so the
selected note survives it (UX-DR41).
AC: a note tagged `#a/b` counts once under `a` and once under `a/b`, and removing it decrements
both; a `#tag` inside a fenced code block or a URL fragment is not a tag, asserted by a table
of adversarial bodies; two chips return the intersection and never the union; adding a chip
that excludes the selected note leaves the editor on that note with the row visibly filtered
out, rather than jumping to another note.

### Story 37.4: Spaces Are Notes
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 37.3.

`keeper-core/src/notes/query.rs`: a small total grammar over tag, path, frontmatter-field and
date predicates with `and`/`or`/`not`, parsed to an AST and evaluated against `NoteRecord`. No
evaluation of user code, no regex denial-of-service surface, bounded by construction. A space
is a note under `spaces/` whose frontmatter carries the query string, so it syncs, diffs and an
agent can write one. "Save this filter as a space" is one keystroke from the chip row
(UX-DR37) and writes such a note through story 36.5's writer.
AC: a space note hand-authored in Obsidian evaluates identically to one keeper wrote; a
malformed query renders its parse error on the space row and matches zero notes — a property
test asserts no input makes an invalid query match anything; a space whose query names a
frontmatter field no note has returns empty rather than erroring; saving the current chip set
as a space produces a note whose query, re-parsed, reproduces the same result set.

### Story 37.5: Search Is a Scan, Not an Engine
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 37.2.

A bounded parallel content scan over the active vault in `notes_vault`, returning match
positions per note so the list can highlight them, cancellable when the query changes, and
bounded in both concurrency and total bytes read per query. Never stale, because there is no
index to invalidate — which is the whole argument for not shipping tantivy at this size.
AC: a full-text query over a 10 000-note vault returns first results in under 300 ms and
completes within a stated bound, recorded; a file written one millisecond before the query is
matched by it; typing a six-character query issues at most one in-flight scan at a time and the
superseded scans are cancelled, asserted by a scan counter; the scan never reads a file
excluded by the profile's `ExcludeSet`.

### Story 37.6: The Editor — Live Preview, Typed Properties, Streamed Body
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 37.2.

CodeMirror 6 (MIT) in `src/components/notes/note-editor.tsx`: live preview is the only editing
mode, with source revealed on the line under the cursor and no preview toggle as the primary
affordance (UX-DR40). Frontmatter renders above the body as a typed properties panel — text,
list, date, boolean — driven by `keeper_core::notes::frontmatter`, and editing a property
rewrites only that key, preserving the user's unknown keys and their order. The body arrives
over a new `Channel<NoteBodyChunk>` opened by `notes_open`, and writes go back through
`notes_write` carrying the base revision the buffer opened at — the input story 38.2's merge
needs.
AC: opening a 2 MB note is editable before the final chunk arrives, and the caret does not jump
when it lands; the active line shows `**bold**` as source while the line above renders bold; no
command return value in `notes_ipc.rs` contains a note body, asserted by a convention test over
the returned types; editing one frontmatter property leaves every other key byte-identical.

### Story 37.7: Wikilinks, Backlinks and File Links
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 37.6.

`[[` opens autocomplete served from the core link graph; Enter on a name that does not exist
creates the note through story 36.5's writer and links it. A backlinks list sits at the foot of
the editor, projected from the same graph. A note may also link a file elsewhere in the same
synced folder — inside the profile root, not merely inside the vault — and keeper opens it with
the OS handler or reveals it via the existing `reveal_in_file_manager` capability.
AC: renaming a note keeps every backlink resolving, because links resolve through the ULID
(FR-97) and not the filename; a link to a path outside the profile root is refused with a
legible reason in the editor, never a silent no-op and never an open; the backlinks list
updates within one change event of a new inbound link being written by another process.

### Story 37.8: Attachments over `keeper-note://`, and Mermaid That Degrades
**Crosses the IPC boundary.** Bindings: no (new URI scheme, no new ts-rs type). Depends on 37.6.

`keeper/src/note_protocol.rs` clones `media_protocol.rs`'s async custom-URI-scheme recipe —
including range handling and the not-found path — and adds the AD-59 requirement it does not
need: a mandatory canonicalise-and-contain check of the resolved path against the vault root
before any read. Pasting or dropping an image writes it into `attachments/` and embeds a
relative markdown link that Obsidian reads unchanged. Mermaid (MIT) renders ` ```mermaid `
blocks inline; a parse failure renders the error and the source, and a missing image renders
its alt text and its path (UX-DR44).
AC: `keeper-note://<vault>/../../etc/passwd` and a symlink inside the vault pointing outside it
both return 404 and log a warning; a 12 MB pasted PNG produces an `attachments/` file and a
link, and no IPC message larger than a kilobyte; a syntactically broken mermaid block renders
red error text plus its own source and the rest of the note still renders; an image whose file
was deleted renders alt text, never an empty box.

### Story 37.9: The Other Lenses — Physical Tree, Table, Board
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 37.2, 37.3.

Virtual organisation stays the default lens; the physical folder tree is one click away and any
row reveals its real path (UX-DR38, FR-106). The same note set also renders as a table whose
columns are frontmatter fields, or grouped into a board by one field (FR-123 — the phase's
Should tier, which is why it is scheduled last in the epic). Every lens is a projection of the
same `NoteQuery` result, so switching one is a filter and the selected note survives it
(UX-DR41).
AC: switching between all four lenses keeps the note under the cursor selected and its editor
buffer intact, including when it is dirty; reveal opens the containing folder on both macOS and
Linux; a table column bound to a frontmatter field absent from a note renders empty rather than
omitting the row; a board grouped by a field with 40 distinct values does not render 40 columns
off-screen — it bounds and states the overflow.

## Out of scope

- External-write handling of any kind. A note changed under the editor is Epic 38; here the
  editor owns its buffer and the list refreshes on its own re-projection.
- Conflict rows. Story 38.6.
- A graph view, transclusion, a calendar lens — all declared out of phase.
- A real full-text engine. Story 37.5's scan is the answer up to the measured ceiling, and
  `docs/notes.md` (story 39.4) records where that ceiling is.
- Note history and provenance. Story 38.5.
