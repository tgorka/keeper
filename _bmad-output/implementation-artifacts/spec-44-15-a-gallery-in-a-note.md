---
title: 'Story 44.15: A Gallery in a Note'
type: 'feature'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: ''
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-44-the-vocabulary-is-the-space.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-5-one-attachment-vocabulary.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-8-the-files-tab.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-44-10-lists-that-render-what-you-can-see.md'
---

<intent-contract>

## What was already there and dead

The epic asks each story to say what it found. Three things:

1. **`keeper_sync::browse` could already list any folder** — the lexical containment test, the
   canonicalizing one behind it, the noise filter, the cap and the stable order — and was reachable
   only through a `&SyncProfile`. A notes vault is not a profile, so the one directory reader in the
   repo was unreachable from the one surface that most needed it. This story did not write a reader;
   it took the profile out of the middle of the existing one.
2. **`kind_for_file_name` already answered "is this media"** for every file in the vault, and no
   notes surface had ever asked it. `notes_tree` reads the INDEX, which holds notes; a folder of
   four hundred photographs is invisible to every notes command that existed.
3. **`RecordingNoteTargetKind::Image | Audio` already existed** (43.5) and outside a recording note
   nothing produced them. The gallery is the second producer, and it needed no new vocabulary.

One thing was found broken rather than dead: **`MermaidWidget` asks for `block: true` from a
`ViewPlugin`, and CodeMirror throws for it.** Story 44.16 found the same wall from the other side.
The mermaid fence therefore does not render in the real editor today and no test covers it, because
`mermaid-widget.test.ts` drives the widget directly and `recording-embed.test.ts`'s editor tests
never build a fence. Not fixed here — see "Deliberately NOT done".

## Intent

**Problem:** a note can embed one file at a time. Standing over a folder — a shoot, a scan batch, a
trip — and showing it is not expressible, and a vault with four hundred photographs beside the note
about them is the ordinary case. Rendering all four hundred is the other failure (AD-84): the
machine with the most to show is the one that stops responding.

Two things had to be decided rather than built:

- **The syntax**, because Obsidian reads the same vault and will never render this widget. A block
  that degrades to a broken mess is not acceptable; one that degrades to a plain list of links is.
- **Where a pin lives**, because two notes over one folder pin different things. A pin written into
  the folder is one note editing another note's view of shared photographs, and it is a write into
  the user's vault that nobody asked for.

**Approach:** the block is Obsidian's own callout, `> [!gallery] <folder>`, holding the pins as
ordinary wikilinks. The listing is `keeper_sync::browse` with the profile taken out of the middle.
The window is 44.10's window, with its arithmetic split out of the React hook so a CodeMirror widget
can drive it without mounting a React root. A pin is a one-line splice into the note.

## Boundaries & Constraints

**Always:**
- The block is legible markdown in Obsidian: a blockquote, a callout marker Obsidian defines, and
  wikilinks. Nothing else.
- A pin is written into the note that holds the block, as the vault-relative path Rust produced
  (FR-145) — never absolute, and never composed in the webview (AD-65).
- A pin edit is a one-line splice. Every other byte of the block, including lines this module does
  not understand and the terminators it found, survives untouched.
- The kind comes from the one classifier (AD-73). The gallery decides which KINDS it renders; it
  never decides what a file IS.
- A folder that cannot be listed says so, in the sentence Rust composed about it.
- Rendering is bounded by the viewport, asserted by counting.

**Never:**
- No second directory reader, no second classifier, no second windowing implementation, no second
  embed syntax.
- No thumbnails and no derived media (43.5's rule). A video tile is a `<video preload="metadata">`,
  not a generated poster.
- No write into the gallery's folder, ever. The files surface never writes (AD-75) and neither does
  this one.
- No hand-written `src/lib/ipc/gen/*`.

## I/O & Edge-Case Matrix

### The block's syntax

| Scenario | Input | Expected | Error |
|---|---|---|---|
| A gallery | `> [!gallery] Photos/Trip` | `{ folder: "Photos/Trip", pins: [] }` | none |
| With pins | head + `> [[Photos/Trip/a.jpg]]` | pins in the note's own order | none |
| Case | `[!Gallery]`, `[!GALLERY]` | a gallery — Obsidian's matching is case-insensitive | none |
| Another callout | `> [!note] x` | not a gallery; an ordinary quote | none |
| Plain quote | `> a thing somebody said` | not a gallery | none |
| Unquoted | `[!gallery] x` | not a gallery — a callout is a blockquote | none |
| Prose inside | head + `> the good ones:` + a pin | a gallery; the prose line is ignored and preserved | none |
| No title | `> [!gallery]` | `folder: ""` — never "list the vault root" | none |
| Trailing slash | `> [!gallery] Photos/Trip/` | `folder: "Photos/Trip"` | none |
| Head mid-quote | a quote whose SECOND line is `> [!gallery]` | not a gallery — it is part of that quotation | none |

### Pinning

| Scenario | Input | Expected | Error |
|---|---|---|---|
| First pin | a block with prose, no pins | the line goes directly under the head | none |
| Second pin | a block with one pin | the line goes after the last pin — pin order is pin-time order | none |
| Already pinned | the same path twice | unchanged text | none |
| Unpin | a pinned path | that one line removed, every other byte identical | none |
| Unpin a stranger | a path not pinned | unchanged text | none |
| Round trip | pin then unpin | byte-identical to the original | none |
| CRLF vault | a block with `\r\n` | the spliced line carries `\r` too | none |
| `>` with no space | `>[!gallery] x` | the new line copies `>` and not `> ` | none |
| Stale position | a toggle after the block moved | the range is re-read from the doc; `null` splices nothing | none |
| Two notes, one folder | pin in note A | A's file changes; B's file does not, and B's tiles stay unpinned | none |

### What a gallery shows

| Scenario | State | Expected | Error |
|---|---|---|---|
| Mixed folder | video, image, audio, `manifest.json`, `board.sketchpad` | three tiles; two counted as "not media and not shown" | none |
| A non-media file | `manifest.json` | skipped — no tile, and nothing requests its bytes | none |
| Pinned | two pins present in the folder | those two first, in the note's order, outlined | none |
| Pin the folder lost | `gone.png` | counted as "1 pinned item is not in this folder"; nothing invented | none |
| Pin naming a non-media file | `manifest.json` pinned | counted as missing — a pin cannot promote a file to a tile | none |
| 400 photographs | a 400-item folder | a bounded window of tiles, canvas as tall as the folder | none |
| Scrolled | scroll to 6 000 px | the first row is gone, a later row is mounted, still bounded | none |
| Empty folder | zero entries | "0 items", no grid | none |
| Truncated | `truncated: true` | "this folder holds more than the listing shows" | none |
| Caret in the block | selection touches any of its lines | the source comes back, editable | none |

### Listing and the degrade paths

| Scenario | State | Expected | Error |
|---|---|---|---|
| Folder listed | a vault folder | every entry with its kind; a URL only for the servable kinds | none |
| Unreadable folder | `chmod 000` | `BrowseRefusal::Unreadable`; its sentence shown verbatim, INFO logged | none |
| Missing folder | a folder not in the vault | `Missing`; the "not in the vault" sentence, INFO logged | none |
| Escaping folder | `../../.ssh`, `/etc`, `.`, `a//b` | `BrowseRefusal::Escapes`; its sentence, INFO logged | none |
| Symlink out of the vault | a folder symlinked outside | `EscapesAfterResolution`, refused after canonicalisation | none |
| No such vault | a bad id | the call rejects; the widget says it could not list just now | rejects |
| Host gone / IPC throws | loader rejects | "this folder could not be listed just now."; pins still linked | none |
| No loader at all | the renderer with no `listFolder` | "keeper is not listing this folder here." — never a silent blank | none |
| Sync marks | any entry | `EntrySyncStatus::Unknown` — a gallery asks the engine nothing | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/browse.rs` — **the profile taken out of the middle.** `browse`
  keeps its signature and its check order; the directory read moves into `list_resolved(root, …)`
  and a new `pub fn browse_root(root, …)` is the entry point for a caller that has a root and no
  profile. Neither entry point resolves twice. Four new tests, all on any machine.
- `src-tauri/crates/keeper-core/src/notes/vm.rs` — `NoteGalleryVm` and `NoteGalleryItemVm`. Every
  entry crosses with its kind, including the ones a gallery will not show.
- `src-tauri/crates/keeper/src/notes_ipc.rs` — `notes_gallery`, ~50 lines of wiring: resolve the
  vault, call `browse_root` on the blocking pool, classify each dirent with `kind_for_file_name`,
  compose each URL with `notes_vault::asset_url`. Three INFO log lines for the three ways it can
  decline. **Written, never executed on Linux.**
- `src-tauri/crates/keeper/src/lib.rs` — one line in the handler list.
- `src/lib/ipc/gen/NoteGallery*.ts` — **generated by ts-rs**, which runs on Linux under
  `cargo test -p keeper-core`.
- `src/components/ui/window-list.tsx` — `rowOffsets` and `windowSlice` split out of the hook, which
  now calls them. No behaviour change; the eight existing tests are the proof.
- `src/components/notes/editor/gallery-block.ts` — new. The syntax, the pin splices, the ordering,
  the tile, the windowed grid, the widget and the `StateField` that hosts it.
- `src/components/notes/editor/live-preview.ts` — one option, the theme, and `galleryLayer` in the
  extension array.
- `src/components/notes/editor/slash-menu.ts` — one row, so the block is reachable without knowing
  its syntax.
- `src/components/notes/note-editor.tsx` — the loader.

## Tasks & Acceptance

**Execution:**
- [x] `browse_root`, with the containment rule proved on a plain root.
- [x] The VMs, the command, the registration, the bindings, the client wrapper.
- [x] The callout syntax, justified against a fence, with the degrade asserted.
- [x] Pins in the note, spliced one line at a time, CRLF-safe and round-tripping.
- [x] The windowed grid over 44.10's arithmetic, bounded and asserted by counting.
- [x] Media-only tiles; non-media skipped and counted; an unreadable folder's sentence shown.
- [x] A slash-menu row, and a test that the menu and the parser agree.
- [x] Reverts proved: seven mutations, each run and each watched to fail.

**Acceptance Criteria:**
- `bun run test src/components/notes/editor/gallery-block.test.ts` — 32 passing.
- `bun run test src/components/notes/editor/recording-embed.test.ts` — 46 passing, unchanged.
- `cargo test -p keeper-sync --lib browse` — 27 passing (23 before).
- `cargo test -p keeper-core --lib notes::vm` — 42 passing, bindings regenerated with no drift.

## Design Notes

**Why a callout and not a fence.** The alternative was ` ```keeper-gallery ` with `folder:` and
`pin:` keys, which is what `MermaidWidget`'s neighbour suggests and what every Obsidian plugin in
this shape does. In Obsidian that renders as a grey monospace box containing keeper's configuration
language: legible, and strictly worse than the callout, which renders as a titled box holding
working links to the pinned photographs. The reader gets their own material instead of keeper's
syntax. It also costs no new grammar — a blockquote is a blockquote, `[!type]` is Obsidian's own
callout marker, and an unknown callout type is specified to fall back to the default style rather
than to an error. This is 43.5's `![[…]]` argument applied to a block.

**Why a pin is `[[…]]` and not `![[…]]`.** An embed would make Obsidian show the pinned images
inline, which reads better for three pins and lies: the gallery is over a folder of hundreds and the
note holds none of them. A link says the true thing — *these are the ones this note singles out* —
and costs Obsidian no decode of somebody's raw camera files.

**Why the pin is a splice and not a serialisation.** `withPin` inserts one line and `withoutPin`
removes one line; everything else in the block, including a prose line the parser ignores and a
`\r` it found, is carried through byte for byte. This is `Frontmatter`'s rule, and the reason is the
same: a note is a file in somebody's git history, and a renderer that reformats a block it merely
touched turns a one-line change into a whole-block diff. The round-trip test is the assertion.

**The gallery's rule is the gallery's, and the classifier is still singular.** Rust returns every
entry with its kind and filters nothing. Which kinds get a tile is a decision about a surface, and
putting it in the Tauri shell would have made "a non-media file is skipped" a claim that no test on
this machine could reach — the shell crate does not build here. The surface tests it, and it tests
it by asking whether Rust composed a URL, which is exactly the set `keeper-note://` will serve. A
tile for a file the protocol would 404 is the dead player 42.6 refuses, and it is now impossible to
construct: no URL, no element.

**One window, two bindings.** `useWindowedRows` is a React hook and the editor's lazy chunk is
React-free by design — `recording-embed.ts` documents that even a shared string is not worth pulling
React in for, and a React root inside a CodeMirror widget is a great deal more than a string. The
two honest options were a second windowing implementation or a shared core, and 44.10's whole point
is that "which indices are on screen" is answered once. So `rowOffsets` and `windowSlice` came out
of the hook, which now calls them; the gallery calls them too. The hook keeps the half the gallery
does not need — per-row measurement, the ResizeObserver, the roving tab stop — because a grid of
uniform tiles has nothing to measure. `useWindowedRows` fits a grid fine, incidentally: a grid row
IS a row, and the column count is arithmetic on the viewport width.

**The `StateField`, and the wall two stories hit in one wave.** A gallery replaces several lines
with one element, and CodeMirror refuses both halves of that from a `ViewPlugin`: `block: true` is
rejected outright, and an inline replace spanning a line break is rejected the moment a block has
its first pin. Story 44.16 met the same wall with a single-line embed and could take the inline
form; this one could not. So `galleryLayer` is a `StateField` composed into `livePreview`'s
extension array — still one renderer, still one reveal rule. Its scan is by line rather than through
the parse tree, because a field has no view to ask for visible ranges and a regex per line is far
cheaper than walking the whole tree on every keystroke; the scan runs only on a document change, and
a caret move rebuilds the decoration set from the blocks already found.

**`updateDOM`, because a pin must not cost a listing.** Pinning changes the block's source, so
`eq` is false and CodeMirror would tear the widget down, re-list a folder of four hundred over IPC
and drop the reader back to the top of the grid — to move one tile. The mounted listing therefore
lives in a `WeakMap` keyed by the widget's DOM, and `updateDOM` adopts it when the new widget names
the same folder. `list` is asserted to have been called exactly once across a pin.

**The three ways this command declines, all at INFO.** DW-162 is the standing lesson: `RUST_LOG` is
unset on the owner's machine, so `tracing::debug!` reaches no log and a story shipped twice while
doing nothing. `notes_gallery` can decline for a missing folder, an unreadable one and an escaping
path, and each is `tracing::info!` with the vault, the folder and the reason. Each also returns a
`problem` sentence rather than rejecting, because a block on screen has to say something and a
rejected promise gives a widget nothing to say.

**`PendingView::Unavailable`, deliberately.** `browse_root` still takes the pending view, and the
gallery passes the variant that claims nothing. An empty `PendingView::Known` would have been the
convenient default and it marks every entry `Synced` — telling somebody their photographs are safe
on a remote that has never heard of them. The type would not have stopped it; the argument makes it
a choice somebody had to type.

**What the reverts proved.** Seven mutations, each applied, run and watched to fail, then reverted:

| Mutation | Caught by |
|---|---|
| `galleryOrder` renders every kind (drop the `url !== null` filter) | 4: `skips a file nothing renders…`, `floats the pinned items…`, `counts a pin the folder no longer holds…`, `turns the block into a gallery of the folder's media` |
| `paint` mounts every row instead of `windowSlice`'s | 2: `mounts a bounded number of tiles over a folder of hundreds`, `mounts the tiles the scroll position reaches…` |
| `withPin` re-serialises the block instead of splicing one line | 4: `adds the new pin after the last one…`, `adds the first pin directly under the head`, `round-trips…byte for byte`, `keeps CRLF terminators…` |
| The pin toggle repaints instead of dispatching into the note | 3: `writes a pin into the note that holds the block`, `takes a pin back out of the note`, `keeps one note's pins out of another note over the same folder` |
| The `listing.problem` branch is skipped | 1: `says what an unreadable folder said, in Rust's own words` |
| A pin is written as `![[…]]` rather than `[[…]]` | 8, including `stays a callout of plain wikilinks, which is all Obsidian needs to render it` |
| `browse_root` swallows the containment refusal (`unwrap_or(None)`) | 1: `a_plain_root_refuses_every_escape_a_profile_refuses` |

**What could NOT be verified here, stated plainly.** The `keeper` shell crate does not build on
Linux, so `notes_gallery` is **written and never compiled**. A type error in it is possible and it
must be built on the macOS host. What that costs is bounded on purpose: the command holds no
decision — the listing, the containment rule and the classifier are all called, not reimplemented —
so the untested surface is a resolve, a `spawn_blocking`, a map and three log lines.

Nothing here was verified in a real webview either. jsdom decodes no image, plays no audio and lays
nothing out, so what the frontend suite proves is which elements are constructed with which
attributes and which rows are mounted against a modelled scroll position — not that four hundred
thumbnails scroll smoothly at 60 Hz on a network share. `withListGeometry` is what makes the
bounded-tiles assertion an assertion about keeper rather than about jsdom; without it every list in
this repo renders its first window forever and "bounded" passes for the wrong reason.

## Deliberately NOT done

- **No thumbnails, no posters, no derived media.** 43.5's rule. A video tile is a
  `<video preload="metadata">` primed to its first frame by the function 44.1 already wrote, and the
  window is what bounds how many of those exist at once. Generating and caching posters is a real
  feature with a cache-invalidation problem and a disk budget, and it is not this story.
- **No recursion.** A gallery lists one directory. Subfolders come back in the listing and get no
  tile, because a folder has no element (43.5's `Folder` row) and a gallery that silently descended
  would list a vault.
- **No selection, no lightbox, no reordering beyond pinning.** Clicking a tile does nothing but
  operate its own media element. A full-screen viewer is a surface, not a widget.
- **No `keeper.gallery` frontmatter and no per-folder configuration.** Everything the block needs is
  in the block, which is what makes two notes over one folder independent.
- **The mermaid fence's `block: true` bug is not fixed.** `MermaidWidget` asks a `ViewPlugin` for a
  block decoration and CodeMirror throws; the fix is moving the renderer's whole decoration set into
  a `StateField`, which would change every existing decoration's lifecycle and belongs to 37.8 or to
  a story of its own. `galleryLayer` shows the shape that fix would take.
- **No sort.** The gallery shows the listing's order — folders' natural, case-insensitive name order
  — with pins floated to the top. A space's `sort` (44.4) is a space's, and inventing a second sort
  vocabulary for a block is exactly what this epic's "Out of scope" forbids.
- **The pinned links are not deduplicated against a pin written twice by hand.** A note that lists
  the same path twice counts the second as a missing pin rather than rewriting the note to remove
  it. Silently editing somebody's note to agree with a listing taken one second ago is the one
  unrecoverable thing this surface could do.
