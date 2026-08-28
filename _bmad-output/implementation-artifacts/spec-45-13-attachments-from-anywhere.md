# Story 45.13 — Attachments From Anywhere

**Epic 45, wave 2.** Bindings: FR-188, FR-189, UX-DR76. Also touches FR-110 (the
attachment import path this story finally connects to a caller), FR-145 (no
absolute path in a synced artefact) and AD-65 (the webview never joins a root to
a subpath).

---

## What already existed

Seven times in this epic family the asked-for thing turned out to be present as
a value nobody applied, so the first work here was looking. What I found:

| Thing | Where | Verdict |
| --- | --- | --- |
| An attachment inserter | `AttachmentsPanel` (43.7) — `onInsert(`![[rel]]`)` → `NoteEditor.insertAtCursor` | Reused. This is the survivor. |
| **A second attachment inserter** | `keeper/src/notes_vault.rs::attachment_markdown` (epic 37, FR-110) — `![name](rel)` / `[name](rel)` | **Deleted.** See below. |
| An import-a-file-into-the-vault path | `notes_vault::import_attachment` → `attachments/` with `unique_name` | Reused, with one bug fixed. |
| A command that called it | `notes_attachment_drop(vaultId, noteId, paths)` | **Deleted** — superseded, and it had no caller. |
| A searchable note list | `notes_link_targets(vaultId, prefix)` + `fold` | Reused as the model for `notes_attach_targets`. |
| A duplicate-by-name predicate | `embeddedAttachmentNames` in `attachments-panel.tsx` (43.7) | Lifted into `@/lib/notes/attach` and widened. |
| A multiselection in the Files pane | `selection` / `deletable` (45.3) | Reused. **No second selection model was added.** |
| A file picker | `@tauri-apps/plugin-dialog`, already a dependency, already initialised in `lib.rs`, already used by five surfaces | Reused. No new dependency. |
| A gallery "inserter" | `withPin` in `gallery-block.ts` (44.15) | **Not** a third inserter and left alone: it writes `[[…]]`, a *link*, into a gallery block over a folder, and 44.15 documents at length why a link and not an embed. |
| A relative-path helper | `notes_vault::vault_relative` (private) | **Not** reused; see "Two things that look like duplication". |

Nothing else. In particular there was no note chooser, no way to attach from
outside a recording note, and no caller anywhere for FR-110's import path.

### The two inserters, and which spelling won

`AttachmentsPanel` wrote `![[recordings/…/screen.mov]]`. `attachment_markdown`
wrote `![screen.mov](attachments/screen.mov)`. One act, two spellings.

**`![[rel]]` won, and not because it was here first.** It is Obsidian's own
embed syntax, so the vault stays a folder Obsidian reads unchanged; it is the
only spelling `live-preview.ts` decorates into a player, an image or a table, so
the other one would have rendered as flat text inside keeper; and it is what a
person typing by hand would write, so an embed made by keeper is
indistinguishable from one made by hand — including to `recording-embed.ts`,
which is the point.

**Nothing on disk carries the losing spelling from keeper.** `attachment_markdown`
was reachable only through `notes_attachment_drop` and `notes_attachment_paste`,
and `git log -S` shows both commands and both TypeScript wrappers arriving in
one commit (`33535bc`, epic 37) and never being called from `src/` since. So no
note keeper wrote can contain it, and **there is no migration**. A note may
still contain `![alt](path)` because a human or Obsidian wrote it — that is
ordinary CommonMark, `links::extract` has always indexed it, and this story
additionally makes it *count* as holding the file (below). Recognised
everywhere, written nowhere.

`NoteAttachmentVm.markdown` went with it. The field had zero readers; a dead
field is an untested code path waiting for its first caller, which is how
`NoteCreateReq.dest` turned out to be an armed data-loss path the moment
something set it. The deletion is recorded in the VM's own doc comment so the
next reader finds the reason rather than a gap.

---

## The design

### One insertion path

`src/lib/notes/attach.ts` is the whole of the decision, and it is pure:

- `attachmentEmbed(rel)` — the one spelling. Nothing else in the app composes an
  attachment embed.
- `embeddedAttachmentNames(body)` — the one duplicate predicate.
- `planAttachments(body, relPaths)` → `{ text, inserted, alreadyThere, unnameable, refusal }`.
- `bodyWithAttachments(body, plan)` — the append form, for a note with no caret.

All three entry points call `planAttachments` and then differ only in delivery:
spliced at the caret of an open editor, or appended to a note read from disk.
That is what makes "the same result from anywhere" a property of the code rather
than a promise in a comment, and it is what the byte-for-byte test asserts.

### The three entry points

| Entry point | Surface | Where the file comes from | How the text lands |
| --- | --- | --- | --- |
| A folder on the drive | `AttachFileButton` in the editor header | `@tauri-apps/plugin-dialog` multi-file picker | `insertAtCursor` |
| A Files-pane multiselection | `AttachToNoteDialog`, opened from the Files header | 45.3's `selection`, filtered to files | `notesBodyWrite` on the chosen note |
| The attachment panel | `AttachmentsPanel` (43.7) | The note's own `files:` key | `insertAtCursor` |

**"Into a note or into a quick capture, with the same result" — what is actually
established.** The epic asks for this and it is an acceptance criterion, so it
is worth being exact rather than triumphant about it.

*Established here:* the picker and the panel both deliver through
`NoteEditor.insertAtCursor`, and neither they nor `@/lib/notes/attach` contain a
branch on which window they are in. There is no capture-aware code path to get
wrong, because there is no capture-aware code path.

*Depends on someone else:* that quick capture mounts the real `NoteEditor` at
all, which is 45.14's story and was in flight while this was written.

*Never run:* **nothing in 45.13 has ever executed in the quick-capture
webview.** Every test here mounts the editor in the main window's realm. So
"the same result" is an argument from the absence of a branch, not a
measurement — the load-bearing half of the claim, and not the whole of it.
W3Capture established the fact that makes the argument work (the capture window
is a separate webview with its own JS realm, so a module singleton is per-realm
and constrains nothing here); I read that finding rather than reproducing it.

### A file outside the vault: **copy it in**

The three options were link it, refuse it, copy it.

**Linking is not available.** A link out of the vault is an absolute path, and
FR-145 forbids one in a synced artefact. The reason is mechanical rather than
stylistic: the vault syncs to other machines, where `/Users/alice/Desktop/photo.png`
names nothing — or, worse, names a different file. A note that shows a picture
on exactly one computer is not a note that has the picture.

**Refusing loses the story.** "Attachments from anywhere" that only accepts
files already in the vault is the thing that already worked.

**So keeper copies it into `attachments/` and the note names the copy**, which
is a file that travels with the note by construction. `import_attachment`
already did exactly this, with a collision-free name; it had simply never been
called. The surface says a copy was made afterwards rather than warning
beforehand: a person who picks a file off their Desktop has not asked to be
warned, they have asked for the file.

**A file already inside the vault is named where it lies, not copied.** The dead
`notes_attachment_drop` copied unconditionally, so attaching a file the vault
already held would have made a second copy in `attachments/` and pointed the
note at the duplicate. Nothing ever called it, so nobody found out. This is the
common case for the Files-pane entry point and it is now the first branch.

### The duplicate rule

By **file name**, folded to lower case, counting **both** embed spellings.

- *By name, not path*, because Story 40.4 renames a session folder after its note
  is written: `![[old/screen.mov]]` and `![[new/screen.mov]]` are one file shown
  twice, which is a duplicate by the only definition a reader can see.
  `recording-embed.ts` resolves by name too, so "already there" and "this is that
  file" cannot come apart.
- *Folded*, like `index::link_key` and for its reason: APFS is case-insensitive
  by default, so `Photo.PNG` and `photo.png` are one file on the machine that
  wrote the note. Under-reporting writes the picture in twice, which is the exact
  failure this story exists to refuse; over-reporting says "already there" about
  a file the person can see is already there.
- *Both spellings*, because a note that already shows the photograph shows it
  whichever spelling put it there. This widens 43.7's rule, which only counted
  `![[…]]`.
- *A link is not an embed.* `[[photo.png]]` mentions the file; `!` is the whole
  of the difference.

**A duplicate is refused with a sentence, never silently.**
`photo.png is already in this note, so keeper left it out.` A mixed selection
writes the rest and names the ones it did not.

### The mirror, and why there is one

`keeper_core::notes::attach::embedded_attachment_names` and its TypeScript twin
exist because the two callers live in different processes:

- the open editor's buffer exists **only** in the webview — Rust cannot read what
  has not been saved, and the panel must flip to "In the note" on the keystroke
  after the insert;
- the note chooser must answer the same question about notes that are **closed on
  disk**, which only Rust can read, and a list never ships bodies (AD-58).

They are pinned to each other by
`src-tauri/crates/keeper-core/src/notes/attach-vectors.json`, 23 vectors, loaded
by both test suites — the same mechanism `keeper_core::size` and
`src/lib/file-size.ts` already use, for the same reason. If they drift, the
chooser offers a note the panel then refuses to write into, which is the "two
answers to one question" the two inserters were. A mirror documented as a mirror
drifts within a month; a mirror pinned to a shared table fails on the commit that
breaks it.

The Rust side reuses `links::extract` rather than growing a private scanner:
that function is the one place this codebase knows what a link looks like, and it
already skips fenced and inline code, drops anchors, percent-decodes a markdown
destination and refuses an external URL. The TypeScript side mirrors it line for
line — including `code_spans` — which is why `attach.ts`'s scanner reads as
un-idiomatic TypeScript. That is deliberate: a reader checking one against the
other should be able to do it by eye.

### Two things that look like duplication and are not

**`notes_vault::vault_relative` already exists** (private, epic 37). I did not
reuse it and did not merge them. It maps every component through
`to_string_lossy`, including `..` and a bare `/`, which is safe for the walked
dirents it is given and is not safe for a path a person picked in a dialog;
and it lives in the shell, where a rule cannot be unit-tested without a vault.
`attach::vault_relative` refuses any non-`Normal` component outright — a
traversal that survived as text would end up inside `![[…]]` in a synced file.
Changing the watcher's helper to match is a behaviour change to indexing and is
not this story.

**`nameList` in `attach.ts` is not `countLabel`.** `countLabel` words how many of
a noun exist and deliberately cannot be handed a list; this names the files back
to the person in the order they picked them, which is what makes the refusal
answerable.

---

## I/O and edge-case matrix

### `planAttachments(body, relPaths)`

| Body | Paths offered | `text` | `alreadyThere` | `refusal` |
| --- | --- | --- | --- | --- |
| `intro\n` | `[b/second.png, a/first.png]` | `![[b/second.png]]\n![[a/first.png]]` | — | `null` |
| `![[attachments/photo.png]]\n` | `[attachments/photo.png]` | `""` | that path | `photo.png is already in this note, so keeper left it out.` |
| `![[old/screen.mov]]\n` | `[new/screen.mov, map.pdf, photo.png]` | the two new ones | `new/screen.mov` | names `screen.mov` |
| `![A photo](attachments/photo.png)\n` | `[attachments/photo.png]` | `""` | that path | already-there |
| `[[attachments/photo.png]]\n` | `[attachments/photo.png]` | the embed | — | `null` (a mention is not an embed) |
| ` ```…![[photo.png]]…``` ` | `[photo.png]` | the embed | — | `null` (code is not a use) |
| `![[FOTO.PNG]]` | `[foto.png]` | `""` | `foto.png` | already-there (folded) |
| `""` | `[photo.png, photo.png]` | one embed | second occurrence | already-there |
| any | `[attachments/why#not.png]` | `""` | — | names the characters, and says renaming is the fix |
| `![[a.png]]\n` | `[a.png, b\|c.png, d.png]` | `![[d.png]]` | `a.png` | both clauses |

### `bodyWithAttachments(body, plan)`

| Body | Result |
| --- | --- |
| `intro` | `intro\n![[…]]` — a separator is added |
| `intro\n` | `intro\n![[…]]` — none is added twice |
| `""` | `![[…]]` |
| anything, empty plan | the body, byte-identical (no save, no sync, no commit for a refused gesture) |

A terminator is never appended. That is what keeps the three entry points
byte-identical: a caret at the end of a body ending in a newline produces exactly
this, and one extra byte here would make the closed-note path write something the
open-note path does not.

### `notes_attach_sources(vaultId, sources)`

| Source | `relPath` | `copied` | `refusal` |
| --- | --- | --- | --- |
| A file inside the vault | its vault-relative path | `false` | `null` |
| A file outside the vault | `attachments/<collision-free name>` | `true` | `null` |
| A symlink inside the vault pointing outside | `attachments/…` | `true` | `null` — judged on where it really points |
| A directory | `null` | `false` | "…is a folder. A note can embed a file, but there is nothing to show for a directory." |
| A device, a pipe | `null` | `false` | "…is not a regular file…" |
| Unreadable, or a broken symlink | `null` | `false` | "keeper could not read…" |
| Inside `.keeper/`, `.obsidian/` or `.git/` | `null` | `false` | "…is inside a folder keeper, git or Obsidian owns…" |
| A copy that fails | `null` | `false` | names the OS error |

One entry per source, in the order given, **including the refused ones**: a
person who selected six files and got four needs to know which two and why, and
a shorter array cannot say.

### `notes_attach_targets(vaultId, query, names)`

| Case | Answer |
| --- | --- |
| Empty query | every note, capped at `MAX_LINK_TARGETS` (30) |
| Query matches title or path, folded | those notes, sorted by title |
| A candidate whose body embeds one of `names` | offered with `holds` naming it; the surface then shows no button |
| A candidate that cannot be read | offered with `holds: []` — a transient read error must not hide a note |

The cap is load-bearing here in a way it is not for the wikilink completion:
each candidate costs a file read. Thirty small markdown files per query, and no
more, whatever the vault holds.

### `notes_body_read` / `notes_body_write`

The read-modify-write for a note nobody has open. `notes_body_write` makes the
same promises `notes_save` makes the editor and through the same three functions:
the frontmatter block on disk survives byte for byte except for `updated`
(`save_document`, FR-121), and a `base_rev` older than disk means the disk bytes
are written aside as an AD-43 conflict copy **before** this write lands.

If the note happens to be open in the editor, this is an external write like any
other — the body watcher sees it and the editor adopts it, or raises its diff bar
over unsaved edits. Not special-cased: a headless write that announced itself
would be a second protocol for one event.

### The chooser's ordering, and the one place it is advisory

Search → choose → **then** resolve. Resolution is not free: a file outside the
vault is copied in, which is a change to the user's disk, and doing it before a
note is chosen would leave copies behind every time somebody opened the dialog
and pressed Escape.

So the list's `holds` filter is advisory and the plan built against the chosen
note's actual body is authoritative. They can disagree in exactly one direction:
a file copied in under a collision-free name (`photo-2.png`) is genuinely new
although an existing `photo.png` hid the note from the list. Conservative about
offering, exact about writing.

---

## Deliberately NOT done

- **Drag-and-drop onto the editor.** `notes_attachment_drop` took paths from
  Tauri's window drag-drop event and nothing ever wired one up. Adding it now
  would be a fourth entry point before the first three shared a path, which is
  how the two inserters happened. The picker needs no window-level plumbing and
  works from the keyboard.
- **Clipboard paste.** `notes_attachment_paste` still refuses with `Unsupported`
  and names the alternative. It is a documented refusal for a capability this
  build does not link, not an inserter, so it is out of "one insertion path".
- **Attaching a whole folder's contents.** A folder is refused with a sentence.
  Standing over a folder of media is 44.15's gallery and it already exists.
- **Merging `notes_vault::vault_relative` with `attach::vault_relative`.** See
  above — it is a behaviour change to indexing.
- **Migrating notes that contain `![alt](path)`.** There is nothing to migrate:
  keeper never wrote it, it is valid CommonMark, and this story makes it count
  as holding the file everywhere the duplicate rule is asked.
- **Debouncing the chooser's search.** Each keystroke is one bounded IPC call
  over an in-memory index plus at most thirty small reads. Adding a debounce
  before measuring a problem would be a knob nobody can justify the value of.
- **A second selection model in the Files pane.** The control hangs off 45.3's
  `selection`; the only thing added is a `useMemo` that drops folders.
- **Gating the control on `write.writable`.** That flag answers "may keeper
  change this file"; attaching changes the note. A read-only PDF on a paused
  drive is a perfectly good thing to put in a note, and the test asserts it.

---

## What I could not verify here, and why

- **The `keeper` shell crate does not build on Linux**, so
  `notes_attach_sources`, `notes_attach_targets`, `notes_body_read` and
  `notes_body_write` are **not compiled**. Everything in them that is a decision
  rather than an effect lives in `keeper_core::notes::attach` and is unit-tested
  (6 tests, `EXIT=0`); what is unverified is the shell wiring itself: the
  `#[tauri::command]` signatures, the `invoke_handler` registration in `lib.rs`,
  the `spawn_blocking` boundaries, `vault.root.canonicalize()` on a real APFS
  volume, and `notes_vault::is_internal`'s new `pub(crate)` visibility. These
  need `cargo check -p keeper --target …-apple-darwin` on the macOS host.
- **The copy itself is untested end to end.** `import_attachment` is shell code;
  that a file outside the vault really lands in `attachments/` under a
  collision-free name, and that `mark_dirty` gets it committed and synced, is
  asserted only through the mocked IPC boundary here. The branch the frontend
  takes on `copied: true` **is** asserted.
- **The OS file picker is mocked.** `@tauri-apps/plugin-dialog`'s `open` is
  stubbed, as it is in the six other suites that use it. That a real multi-select
  picker returns an array of absolute POSIX paths is taken from those surfaces'
  existing behaviour, not measured here.
- **APFS case-insensitivity is asserted as a rule, not observed.** The fold is
  tested against vectors; that `Photo.PNG` and `photo.png` are the same file on
  the owner's disk is the premise, and it matches `index::link_key`'s.
- **No screenshot, no real window.** jsdom measures zero; the chooser's layout,
  the header's crowding with four controls, and whether the outcome banner reads
  well at width are not verified. The accessible names and the sentences are.
- **Rust/TypeScript parity is pinned, not proven identical.** 23 shared vectors
  cover both embed spellings, code fences, inline code, escapes, anchors,
  aliases, percent-encoding, angle-bracket destinations, quoted titles, external
  URLs, folding and sorting. Inputs outside those vectors could still diverge —
  most plausibly deep in `closing_paren`'s balanced-parenthesis handling or in
  Unicode lowercasing, where `str::to_lowercase` and
  `String.prototype.toLowerCase` are not byte-identical for every code point.

---

## Tests, and the mutation table

**The command the acceptance names, first, because that is what a reviewer
checks:** `bun run test src/components/notes/ src/components/layout/files-pane.test.tsx`.

Best observed: **EXIT=0, 761/761, zero unhandled errors.** But three consecutive
repeats gave red / green / red, so it is **not reliably green while the wave is
live**, and the honest form of that is to name the reds rather than quote the
one green:

| Red | Owner | Nature |
| --- | --- | --- |
| `editor/file-embed.test.tsx` | W2Embeds | known load-dependent `waitFor`, ~1 in 3 |
| `notes/note-file-links.test.tsx` | W3NoteFile | mid-mutation-sweep on that exact file |

**Zero of this story's symbols appear in any failure across all three runs.**
Neither red is 45.13's, and neither is a defect — one is load, one is a sibling's
sweep window. A reviewer running this after the wave settles should see 761/761;
a reviewer running it during a sweep should not conclude anything from a red
without grepping for the failing file's owner first.

**What is claimed, at a scope that survived three consecutive repeats and that
nobody else is editing:**

- `bun run test src/lib/notes/ src/components/notes/attach-entry-points.test.tsx src/components/notes/attachments-panel.test.tsx src/components/layout/files-pane.test.tsx` → **EXIT=0, 124/124**, zero unhandled, zero `export is defined`.
- `cargo test -p keeper-core --lib notes::attach` → **EXIT=0, 6/6**.
- `cargo test -p keeper-core --lib` (the WHOLE crate, because this story deletes
  a field from a shared VM and "my six pass" says nothing about the 1680 that
  also compile against it) → **EXIT=0, 1686/1686**.
- Direct consumers — every suite that mounts the real `NoteEditor` whose header
  gained a button, plus `panel-strip` and `properties-panel` → **EXIT=0,
  174/174**, three repeats, zero unhandled. Re-confirmed after Main's
  `withRangeRects` change, which `attachments-panel.test.tsx` consumes.

**And what is deliberately NOT claimed.** A wide run over `src/components/notes/`
+ `src/components/layout/` + `src/lib/` is not a measurement while the wave is
live: three repeats returned 1806, 1806 and 1815 tests, because siblings landed
test files between them. *Four runs over three different trees is four
measurements of nothing* (W2Media). Repeats do not rescue a moving target.

The central test has no expected literal in it: it runs all three entry points
and compares their results against **each other**, byte for byte. A change to the
embed spelling can only pass by changing all three at once, which is what having
one spelling means.

**24 sweep mutations, all 24 caught**, plus eleven later probes from auditing
for shapes peers hit after their own sweeps were clean (three against the
class-level test, two against the vector table's opacity, one against the
silent-drop fix, two against the picker's named cases, three against the
`flatMap` refactor, three against the picker's payload, four against the
`sources` and `body` props, three against what the picker SAYS, and two against
the picker's named answers). **44 in total; ten survived their first probe,
every one at a seam no sweep line was pointed at.**

The last three probes matter more than the sweep's, and W2Media said why: **a
mutation list is a list of lines you already thought about.** It cannot reach a
branch you never wrote because a ternary wrote it for you. All three of those
defects were found by auditing for a shape, not by mutating a line.

The sweep count was reported as 25 in three earlier messages to the wave and is
corrected here: the anchor file carries 25 lines because mutation B1's
replacement spanned two anchor strings I listed separately. Anchors are not
mutations, and a count taken from the wrong list is still a wrong count.

| # | Mutation | Caught by |
| --- | --- | --- |
| A1 | `attachmentEmbed` writes the deleted CommonMark spelling | 18 tests, incl. the three-entry-point parity assertion |
| A2 | duplicate check disabled (`if (held.has(key))` → `if (false)`) | 9 |
| A3 | refusal sentence suppressed (`return null`) | 8 |
| A4 | embed separator becomes a space | 4 |
| A5 | body separator dropped on append | `bodyWithAttachments` adds a separator to a body that has none |
| A6 | case fold dropped from the duplicate key | the `FOTO.PNG` vector |
| A7 | a mention counts as an embed | 3, incl. "does not count a mention" |
| A8 | code spans ignored | the fenced-block vector |
| A9 | markdown-image embeds stop counting | 2, incl. "counts the CommonMark embed spelling" |
| A10 | unnameable guard removed | 2, incl. "refuses a name no wikilink can spell" |
| D1 | chooser offers a note that already holds the file | "does not offer a note that already holds the attachment" |
| D2 | chooser writes even when the plan is empty | 3, incl. both duplicate cases |
| D3 | chooser writes the source name instead of the resolved path | 6 |
| D4 | chooser drops the search query | "asks Rust for the typed query" and "finds a note by title" |
| D5 | the copied-in notice is never said | "attaches the copy keeper made, and says that it made one" |
| B1 | the picker inserts nothing | the parity assertion |
| P1 | the attachments panel spells its own embed again | 6, incl. parity |
| F1 | the Files pane offers folders too | "offers nothing for a selection of folders" |
| F2 | the Files pane offers the control with no vault open | "offers nothing when no vault is open" |
| R1 | Rust drops the fold | the shared vector table |
| R2 | Rust counts a mention as an embed | the shared vector table |
| R3 | Rust's key becomes the whole path | 3, incl. the vectors and `attachment_name` |
| R4 | a traversal is flattened instead of refused | "a traversal in the remainder is refused" |
| R5 | `already_attached` loses its dedup | "keeps the caller's order and drops repeats" |

### Are the empty-expectation vectors green for the right reason?

W2Documents found the sharpest hazard of the wave: a PDF fixture that was
supposed to be compressed was below flate2's threshold, emitted a STORED block,
and stayed legible — so the inflation path under test was never executed and the
mutation disabling it passed. Generalised: **if a fixture is meant to be opaque,
assert that it is opaque before asserting what reading it produces.** Encoding,
compression and encryption all have thresholds below which they quietly do
nothing.

Nine of this story's 23 shared vectors expect `[]`, which is the same risk in a
different dress: `[]` is also what a broken fixture produces. Checked rather
than assumed, by disabling each mechanism and confirming the expected value
moves:

| Vector | Mechanism | Disabled → |
| --- | --- | --- |
| ` ```\n![[photo.png]]\n``` ` , `` `![[photo.png]]` `` | fenced / inline code skip | `["photo.png"]` — mutation A8 |
| `[[photo.png]]`, `[alt](…)` | the embed filter | `["photo.png"]` — mutation A7 |
| `\![[photo.png]]` | `isEscaped` | `["photo.png"]` — probed, caught |
| `![alt](https://example.com/photo.png)` | `isExternal` | `["photo.png"]` — probed, caught |
| `![[]]`, `![[#heading]]`, `""`, plain prose | none — degenerate input | unchanged, and correctly so |

So every vector with a mechanism behind it is exercised by that mechanism; the
four with none are asserting a negative about degenerate input and have nothing
to be green for the wrong reason about. `isEscaped` and `isExternal` had no
mutation in the original sweep and now do.

### A silent drop the VM's own contract made possible

`NoteAttachSourceVm` states in prose that exactly one of `relPath` and
`refusal` is set. Both surfaces originally derived their two lists by filtering
the two fields **independently** — paths from `relPath !== null`, sentences from
`refusal !== null`. A source that came back with *neither* would therefore land
in no list at all and vanish without a word.

That is this story's original bug — silently doing nothing — reachable not
through the duplicate rule it was written to guard, but through a view-model
shape enforced only by a doc comment. Found by taking W2Media's
same-sentence-wrong-meaning hazard seriously enough to audit for it rather than
acknowledge it.

Both surfaces now partition on **the field that decides what happens** —
`relPath === null` — and fall back to a composed sentence naming the file when
Rust supplied none. Keeper wording a worse sentence than Rust's is a far better
answer than silence, and the shape makes the drop impossible rather than
improbable.

Proved by reverting: restoring the independent-filter form fails
`says something about a source that came back with neither a path nor a reason`
and **only** that test. Unlike the class-level assertion above, this one is
load-bearing today.

### A branch nothing wrote, and a reason that was not true

W2Media found the same class as the silent drop one level up: `MediaViewer`
chose its element with a ternary whose *remainder* was `audio`, so a fourth
viewer id bound to it would draw a permanently empty control and say nothing.
Their observation is the sharper half: **a mutation sweep only covers lines you
already thought about, so it cannot reach a branch you never wrote because a
ternary wrote it for you.**

Audited this story for the shape. One hit, in the picker:

```ts
const paths = picked === null ? [] : Array.isArray(picked) ? picked : [picked];
```

With `multiple: true`, `@tauri-apps/plugin-dialog` declares
`OpenDialogReturn` as `string[] | null`, so the last arm is **unreachable** —
and the comment above it justified the arm as tolerance for "a platform that
ignores `multiple`", a shape the declaration excludes. Dead code with an
invented reason, which is the wrong-WHY problem again, in my own file, written
the same day.

Every case is named now, with no remainder: `null` is a cancelled dialog and
gets no sentence because a cancel is not an outcome; an array is the selection;
anything else is the plugin breaking its own contract at runtime and **gets a
sentence**, because handing a bare string to `notesAttachSources` would send
Rust a shape it cannot read and the person would watch the picker close and
nothing happen.

Both branches now have a test and both are load-bearing: restoring the
remainder ternary fails `says so when the picker breaks its own contract` and
only that; making a cancel report an outcome fails `says nothing at all when
the dialog is cancelled` and only that.

### A fallback for a case that cannot happen — and the hole behind it

W2Media's inverse of the unreachable arm: **a guard the compiler believes is
redundant is a guard the next reader deletes**, and a runtime guard no test can
distinguish often wants a *type* that makes it load-bearing, not a better test.

Audited for it. One hit, and it was the bad kind because it fabricated a value:

```ts
sources={attachable.map((node) => node.entry?.absolutePath ?? "")}
```

`Array.prototype.filter` does not narrow its element type without a type
predicate, so a list already filtered on `entry !== null` still reads as
`FilesEntryVm | null` downstream and the compiler *demands* the `?.` and the
`??`. The fallback cannot be reached, so no test can cover it — and if it ever
were reached it would hand Rust an **empty path to attach** rather than nothing.

Replaced by a `flatMap` that narrows inside the ternary, so the impossible case
has no value to fabricate: there is no `??` left to get wrong.

**The refactor then exposed a hole the whole sweep had missed.** Re-probing the
new shape, `absolutePath` → `relativePath` **survived**: the chooser's search is
keyed on file NAMES, and `a.md` is the basename of both spellings, so every
existing assertion passed. The consequence would not have been subtle —
`notes_attach_sources` calls `std::fs::metadata` on what it is handed, so a
profile-relative path resolves against the process working directory and *every*
attach from the Files pane refuses with "keeper could not read…". A total
feature break, invisible until the click reaches the command that consumes the
value.

The seam between the pane and Rust now has its own test —
`hands Rust the absolute path, not the one the tree renders` — which presses
Attach rather than stopping at the offer, and the mutation is caught by it and
only it. AD-65 in the direction that matters: the webview hands over the path
the shell gave it and composes nothing.

Worth stating plainly: this was the **first mutation of 44 to survive its first
probe**, and it was found not by the sweep but by re-probing a line a
refactor had touched. A sweep certifies the lines it was pointed at, on the day
it was run.

### A seam the mocks hid, and the wider rule behind it

W2Media's finding, applied here: their media tests all built a `ViewerFile` by
hand and asked the registry, so nothing exercised the function that turns a
listing *into* one — and putting `entry.name` where `relativePath` belonged
survived their whole sweep.

The same shape lives in a mock's indifference to its arguments.
`notesAttachSources.mockResolvedValue(…)` returns the same value whatever it is
called with, so every test above asserts the **result** and none asserts the
**call**. Three mutations proved it, all survivors on the first probe:

| Mutation | What would ship |
| --- | --- |
| `notesAttachSources(vaultId, [])` | the picker resolves nothing and inserts nothing — **silently**, at the entry point in this story's own title |
| `notesAttachSources("other-vault", paths)` | the file is copied into a different vault; the note embeds a path it cannot resolve, so it renders "not found" on a file keeper just accepted |
| `notesAttachSources(vaultId, paths.slice(0, 1))` | a five-file selection attaches one, and says it attached one, so nothing reads as wrong |

Closed by `sends the vault and every picked path, not a subset`, which asserts
the call and then the resulting note text. Two paths in the fixture rather than
one, deliberately: a mutation keeping only the first would pass on a
single-file selection.

**The generalisation, and it is the sharpest of the day:** a mock that ignores
its arguments converts every assertion downstream of it into an assertion about
the mock. The tests are not wrong, they are testing a different thing than they
appear to — and the appearance is the problem, because a green suite that
exercises the payload's *shape* while never checking the payload's *value* reads
exactly like one that does both.

**W2Media then widened it past mocks, and the wider form is the one that
matters:** *assert what you handed on, not only what came back.* "Handed on"
includes a **prop**, a constructor argument, and any struct you build for
someone else. A mock is the most common instance, not the shape.

Applied to the props this story introduces, four more survivors:

| Mutation | What would ship |
| --- | --- |
| `attachablePaths.slice(0, 1)` in the pane | select five, attach one, report one — nothing reads as wrong |
| `[...sources].slice(0, 1)` in the chooser | the same, one level down |
| `body=""` handed to the picker | **the duplicate check is blinded**, so the picker writes a second embed of a file the note already shows — the thing this story's title says it refuses, at one of its three entry points |
| `body={base}` handed to the picker | `base` is what Rust last acknowledged, so a file attached and attached again before the autosave fires slips through — precisely what that prop's doc comment claims to prevent |

Every duplicate test in this story drove the **chooser**, which reads the body
from disk. The picker reads the live buffer through a prop, and nothing checked
that the prop carried it. Closed by
`refuses a file the note already holds, and says so`, which presses the picker
twice with no save in between; both `body` mutations fail it and only it. The
two `slice` mutations are closed by multi-item fixtures — see below.

**Main's companion rule, applied to every collection fixture here:** *put at
least two items in any collection fixture, because a mutation that keeps only
the first passes every single-item test.* Audited all of them. The shared vector
table has 23 entries and asserts its own minimum length. The picker's payload
test picks two paths. The Files-pane test now selects **two files and a folder**,
so one fixture pins the multiselection promise and the folder rule together. The
chooser's multiselection tests now assert the call as well as the result.

A fixture that cannot distinguish the right answer from the mutant is a
decoration, not a test.

### Counting tests per entry point, which is where the last three came from

The shape that tells you *where* to look rather than what to look for: **when
one behaviour has many tests and they all enter through the same door, the other
doors are untested no matter how many tests there are.**

This story has three doors and it was written as though that were obvious. The
count says otherwise. Tabulated after everything above was already fixed:

| Behaviour | Panel | Picker | Chooser |
| --- | --- | --- | --- |
| the one embed spelling | ✓ | ✓ | ✓ (the parity test drives all three) |
| duplicate refused, with a sentence | ✓ (43.7) | **0 → ✓** | ✓ ×3 |
| multiselection, in order | n/a | ✓ | ✓ ×2 |
| a copy was made, and said so | n/a | **0 → ✓** | ✓ |
| Rust's refusal reaches the person | n/a | **0 → ✓** | ✓ |
| neither path nor reason | n/a | **0 → ✓** | ✓ |
| no absolute path in the note | n/a | ✓ | ✓ |

Four zeros, all in the picker, all found by counting rather than by mutating.
Each was then confirmed by a probe that survived, and the blast radius of all
four is the same sentence: **the picker says nothing.** A file copied onto the
user's disk with no receipt; a folder refused in silence; a source dropped
without a word; a duplicate written twice. At the entry point named in this
story's title.

The picker composes its own clauses from its own code — it does not share the
chooser's — and every test of an outcome sentence had gone through the chooser.
The suite looked thorough for those behaviours *because it was, once*.

### Every claim this story makes about another module, checked

W3Chrome's shape, and it is the cheapest audit of the lot: **a doc comment that
names another module's behaviour is an assertion nobody runs.** Theirs —
`column-widths.ts` saying it followed the cookie `SidebarProvider` writes — had
been false since the day it was written, because `SidebarProvider` has never
been rendered.

This story makes four such claims, three of them load-bearing for the design
rather than decorative. Checked rather than reasoned about:

| Claim | Where | Verdict |
| --- | --- | --- |
| "`recording-embed.ts` resolves by name too, so 'already there' and 'this is that file' cannot come apart" | `attach.rs` module doc | **True** — `recording-embed.ts:158` is `fileName(target.relativePath) === name`. Inherited from 43.7's comment and never previously checked by me; it is the whole justification for the duplicate key being a name rather than a path, so a false one would have invalidated the design and not just the sentence. |
| "Folded, like `index::link_key` and for its reason" | `attach.rs` | **True** — `link_key` ends `key.trim().to_lowercase()`. |
| "the same mechanism `keeper_core::size` and `src/lib/file-size.ts` already use" | `attach.ts` | **True** — both load a checked-in vector table; the Rust side by `include_str!`, the TypeScript side by `readFileSync` from the Rust tree. |
| "`plugin-dialog` … already used by five other surfaces" | `attach-file-button.tsx` | **True, and exactly** — six non-test files import it, one of which is this one. |

`notes_body_write`'s claim to make "the same promises `notes_save` makes, through
the same three functions" is not in this table because it is not a claim about
someone else's code: it calls `save_document`, `write_conflict_copy` and
`write_note` directly, which is checkable by reading the function itself.

### One claim of my own that did not survive being tested

W3Chrome's shape applied to a comment I wrote, with the outcome that is easy to
skip: not a defect found, a **claim corrected**.

`attaching`'s comment said it is a boolean rather than a captured selection so
that "a row that disappears on a refresh while the dialog is open cannot be
attached from a snapshot". True as far as it went, and it implied something
false: that the selection can change under the open chooser at all.

Tried to test it by selecting a different row with the dialog open. The test
cannot be written: the chooser is a Radix modal, everything outside it is
`aria-hidden`, and Testing Library's role query returns **no roles whatsoever**
while it is mounted. The tree is inert; the click path is structurally
impossible, not merely untested.

So the live derivation matters for exactly one case — a **background listing
refresh**, which drops a vanished row out of `selection` (it filters on
`entry !== null`) and therefore out of `sources`, where a snapshot would still
be offering it. That case is real and is not drivable with the dialog mounted
either.

The undrivable test was deleted and the comment now says which case it covers,
that it was verified by reading rather than by a test, and why no test is
available. **A comment that overclaims is worse than a missing test**, because
the next reader budgets for a guarantee that is narrower than advertised — and
the write-time plan is authoritative regardless, so the worst this can cost is
an offer Rust then refuses with a sentence.

### A restore failure worth recording

Mutation **F1** replaced `node.entry !== null && node.entry.kind !== "folder"`
with `node.entry !== null`. **That mutant is a substring of the anchor, and the
same substring already existed elsewhere in the file** — in 45.3's `selection`,
thirty lines above. The inverse replace matched the *first* occurrence, rewrote
45.3's line into a shape it never had, and left mine mutated.

Both standard post-checks passed: the anchor was present (in the wrong place),
and the mutant string was "absent" only because the check looked for the wrong
thing. **So "grep your anchor by name afterwards" did not catch this.** What
caught it was `files-pane.test.tsx` going red and *staying* red across three
runs — a deterministic red, not a flake — and then reading the diff.

The generalisation, which is narrower and sharper than the guidance we started
with: *a mutant that is a substring of its own anchor makes the inverse ambiguous
in a way no anchor grep can see.* The forward edit must carry a unique sentinel
every single time so the inverse is exactly as targeted as the forward. The Rust
half of this sweep did (`// MUTR<n>`) and held for all five; two ad-hoc
TypeScript re-runs did not, and one corrupted a neighbour's line.

### One test kept although it is measurably redundant

Three stories in this wave hit one hole from three directions — W2Media's
distinct-strings count, W2Embeds' `toContain`, and this story's A3 — and the
shape is worth naming: **a mutation that changes what the user READS survives
any assertion that only checks the SHAPE of what they read.** A3 (suppressing
the refusal sentence) failed 8 tests only because one of them pins the whole
sentence byte for byte; `toContain("already")` would have let the duplicate
refusal ship mute, which is the exact failure this story exists to end.

W2Media's remedy is to pin the *negative property the class shares* rather than
N exact sentences. Applied here as
`never names a file it wrote in the sentence about what it did not write`, plus
its converse, over four body/selection shapes.

**Measured, it adds no coverage today.** Two cross-bucket mutations were run at
it — a duplicate written as well as refused, and the refusal built from every
offered path — and both were caught by it *and* by five to seven of the instance
tests, which pin `inserted`, `alreadyThere` and `unnameable` directly. It is
never the only failure.

It is kept for one reason, stated in its own doc comment so nobody has to guess:
a future refusal bucket inherits the invariant for free, without anyone
remembering to write its instance test.

**The discriminator, which is the transferable part** (W2Media's, after
comparing the two opposite verdicts): write the class assertion when the
instance tests only check *shape*; when they already pin *content*, it buys
nothing today and only the future bucket. W2Media's four instances shared one
loop asserting distinctness, so the negative property was the only thing
holding the sentence honest and it was load-bearing immediately. This story's
instances pin the three buckets directly, so the same assertion arrives to find
the work already done. Same technique, opposite verdict, and the difference is
entirely in what the instance tests were already asserting.

Two corrections this produced, both mine:

- The test's first doc comment claimed the exact-sentence tests *would* pass
  under a bucket swap. Plausible, unmeasured, and disproved within a minute of
  probing it. A wrong WHY in the source is worse than no why, because it is what
  the next reader trusts.
- The first probe reported "0 failing, not caught" for a mutation that had
  failed seven tests: my regex over vitest's summary did not match, while
  `returncode` was 1 throughout. **A parsed summary lies in the reassuring
  direction too** — judging by exit code is not only about jsdom throws.

### Three ways to assert an absence, all of which failed silently toward "clean"

This wave found three, and mine is the third:

- **W2Table:** an absence asserted with an *unescaped-metacharacter regex* is
  not an absence — `(` reads as a capture group, so the query can never match
  text containing a literal `(`.
- **W2Documents:** an absence asserted with an *escaped string under a
  fixed-string matcher* is not an absence either — `-F` makes the backslash
  data, so the query can never match the real text.
- **Mine:** an absence asserted over *the anchors you remember mutating* is not
  an absence whatever the matcher. F1's corruption landed on 45.3's `selection`,
  which was never an anchor because I never mutated it. Every query I ran was
  correct and every one came back clean.

Absence here is finally asserted three literal ways over both trees —
`grep -rF "MUTR"`, `grep -rF "MUTANT"`, `grep -rEn "MUT(ANT|R[0-9]|[0-9][0-9])"`
— all empty, with no backslash anywhere near the `-F` queries.

**W2Documents' presence-direction check is the better instrument** and was run
here over all 25 anchors (24 mutations; B1 spans two): each must be back exactly
once, using plain Python
`str.count()` so no regex engine and no fixed-string flag is in the path at all.
A missing anchor cannot hide behind a bad pattern the way a present mutant can.

**But that would also have missed F1**, for the same reason the anchor grep did:
the corrupted line was not an anchor. The check whose coverage does not depend
on my own memory of what I touched is reading the diff —
`git diff -U0 | grep '^[-+][^-+]'` over every mutated file, confirming every
changed line is one I meant. It earned its keep immediately by catching a
dangling `{@link attachedAlready}` in `attachments-panel.tsx`: a symbol that does
not exist, left from renaming the function while writing the doc comment. Not a
mutation, just a wrong reference the next reader would have chased.

**The new files have no diff to read**, so their coverage is the anchor-presence
check plus a full read — weaker, and worth stating rather than implying.

Two of my Rust mutants were also seen live in the tree by siblings (W2Embeds and
Main). Both were mid-sweep and both restored correctly. The lesson is about the
announcement rather than the discipline: each `cargo test -p keeper-core` takes
four to five minutes under the shared build lock, so a Rust mutant sits in a
shared worktree for that long rather than for seconds. **Size the announced
window by the slowest gate times the mutation count**, not by the fast one.

### Also done, per the wave's shim cleanup

`attachments-panel.test.tsx` was the last hand-rolled `Range.prototype.getClientRects`
shim in `src/`. It is now `withRangeRects()` in `beforeAll` / `afterAll`, test
bodies untouched, and its comment carries the **measured** mechanism: the old
shim installed an *empty* `DOMRectList`, so a measure pass that did run read
`rects[0]` as undefined and threw anyway — a permanent latent fault that only
ever *showed* as an occasional red, because whether a frame elapses at all
depends on how busy the machine is. It was never an ordering problem: vitest
isolates per test file, measured this wave by two independent probes (W2Marks',
and W2Emoji's, which was additionally inverted to confirm it was not vacuously
green), with `isolate` and `pool` unset in `vitest.config.ts`.

---

## Files

**New**

- `src/lib/notes/attach.ts` — the one insertion path and the TypeScript mirror.
- `src/lib/notes/attach.test.ts`
- `src/components/notes/attach-to-note-dialog.tsx` — the Files-pane chooser.
- `src/components/notes/attach-file-button.tsx` — the picker entry point.
- `src/components/notes/attach-entry-points.test.tsx` — the parity test.
- `src-tauri/crates/keeper-core/src/notes/attach.rs`
- `src-tauri/crates/keeper-core/src/notes/attach-vectors.json`
- `src/lib/ipc/gen/NoteAttachSourceVm.ts`, `NoteAttachTargetVm.ts`, `NoteBodyVm.ts` (generated)

**Changed**

- `src/components/notes/attachments-panel.tsx` — stops spelling its own embed and
  its own duplicate rule; imports both. Behaviour identical, its 24 tests
  unedited except for the shim conversion.
- `src/components/notes/note-editor.tsx` — the picker control and its receipt banner.
- `src/components/layout/files-pane.tsx` — one header control and one `useMemo`
  on 45.3's existing selection.
- `src/components/layout/files-pane.test.tsx` — six tests, four mocked commands.
- `src/lib/ipc/client.ts` — four wrappers in, two out.
- `src-tauri/crates/keeper-core/src/notes/vm.rs` — three VMs in, one dead field out.
- `src-tauri/crates/keeper-core/src/notes/mod.rs` — one `mod` line.
- `src-tauri/crates/keeper/src/notes_ipc.rs` — four commands in, one out; `fold`'s
  caller note updated.
- `src-tauri/crates/keeper/src/notes_vault.rs` — `attachment_markdown` and
  `is_image` deleted; `is_internal` made `pub(crate)`.
- `src-tauri/crates/keeper/src/lib.rs` — registration.
