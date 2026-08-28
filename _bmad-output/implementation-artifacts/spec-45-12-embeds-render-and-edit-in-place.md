# Spec 45.12 — Embeds Render and Edit in Place

status: implemented
story: Epic 45, Story 45.12
bindings: FR-186, FR-187, UX-DR75
depends on: 45.2 (the viewer registry), 45.4 (raw/rendered), 45.6 (the text editor and the loader), 44.16 (`keeper-core::notes::csv`), 42.4 (recording embeds), 37.6 (`live-preview.ts`)
author: W2Embeds

## What shipped

A `.csv`, `.json` or `.jsonl` embedded in a note renders through 45.2's registry
and 45.4's raw/rendered toggle, inside the note, and an edit in the raw view
writes back to the real file. An embed whose file has moved says so where the
embed is, naming every path keeper looked for.

| file | what it is |
| --- | --- |
| `src/components/notes/editor/file-embed.ts` | the ONE embed widget, the registry-driven target test, and the write bus. No React. |
| `src/components/notes/editor/file-embed-host.tsx` | the React panel: vault coordinates in, `TextFileFrame` out. Dynamically imported. |
| `src/components/viewers/text-file-frame.tsx` | lifted out of `TextFileViewer`: the four states above a loaded file, shared by both surfaces. |
| `src/components/viewers/use-text-file.ts` | lifted: `useTextBuffer(source)` is the loader; `useTextFile` is now the profile facade over it. |
| `src-tauri/crates/keeper-core/src/notes/embed.rs` | the candidate list, the missing-file sentence, the write refusal, `NoteEmbedVm`. Pure. |
| `src-tauri/crates/keeper/src/notes_ipc.rs` | `embed_path` (was `csv_path`, now shared), `notes_embed_read`, `notes_embed_write`. |
| `src/components/notes/editor/csv-table.ts` | **lost** `CsvTableWidget` and `isCsvTarget`. Keeps `renderCsvTableInto`. |
| `src/components/notes/editor/live-preview.ts` | one branch: the `isCsvTarget` test became `embedEntryFor(target) !== null`, and the note's session goes with it. |
| `src/components/notes/editor/recording-embed.ts` | `renderRecordingEmbedInto` now answers **whether it claimed the embed**. |

## What was lifted, and what stayed

The story's instruction was: if you find yourself copying `csv-table.ts`, stop
and lift the shared part. Four things were lifted; one stayed.

**Lifted — the widget.** `CsvTableWidget` was never about CSV. It was "replace an
embed with an asynchronously rendered block, degrade to the link, keep the
events". That is now `FileEmbedWidget`, and which targets it claims is
`embedEntryFor`, which asks 45.2's registry. Adding JSON was not a branch; it
was already a row. `CsvTableWidget` and `isCsvTarget` are **deleted**, not
deprecated — a second widget for one embed syntax is exactly the drift AD-87
exists to prevent, and a dead export is the thing this epic family keeps finding.

**Lifted — the panel.** 45.4's `RawRenderedView` is mounted as it stands. There
is no second toggle, no second parse banner, no second refusal wording.

**Lifted — the loader.** 45.6's `useTextFile` already said in its own header that
"Story 45.12's note embed will need to load the same way inside a note". The two
surfaces differ in exactly one thing — which commands address the file — so that
is now a `TextFileSource` the caller supplies, and `useTextBuffer` is everything
else: the generation counter that stops a slow read for the previous file
overwriting the current one's buffer, `persisted` being state rather than a ref,
the rule that a refused save leaves the buffer dirty and never rolls it back, and
the four reasons a save declines out loud. `useTextFile` is now a facade; its 19
tests are unedited and green, which is the proof the lift changed nothing.

**Lifted — the four states above a loaded file.** `TextFileViewer`'s body became
`TextFileFrame`: loading, no VM, not text, and the format's write refusal. Its 11
tests are unedited and green.

**Stayed — the CSV table.** `renderCsvTableInto` is untouched. The cell grammar,
the ragged-row rule, the revision check and the byte-identical splice remain in
`keeper-core::notes::csv`, which is still the only thing that spells a CSV.

## The four decisions

### 1. The coordinate problem, confirmed by eye

The brief asked me to confirm rather than take on trust that a note can get the
CSV table where a Files panel cannot. It is true, and here is the chain I read:

- `notes_csv_read(vault_id, target)` calls `vault_of(&vault_id)` — a **notes
  vault** id (`notes_ipc.rs`).
- `TextFileViewer` passes `csv={CSV_NEEDS_A_VAULT}`, a `null` constant whose doc
  says a panel holds a **sync profile** id and that deriving one from the other
  in the webview is the path arithmetic AD-65 forbids; the resolution is 45.18's.
- `RawRenderedView` turns that `null` into "keeper can only show a CSV as a table
  inside a notes vault, so this file opens as its source".
- `note-editor.tsx` builds `livePreview({ vaultId })`, so inside a note the vault
  id is in hand.

So the embed passes `csv={{ vaultId, target }}` and gets the table.
`file-embed.test.tsx > a CSV embed > renders the table through 44.16, from the
vault coordinates a note has` asserts `notesCsvRead(VAULT, target)` and the whole
rendered grid, and the mutation that replaces those coordinates with `null` fails
it. **The same `.csv` is a table in a note and its source in a Files panel, on
purpose**, until 45.18 lands.

### 2. React inside a CodeMirror widget, and why the chunk stays clean

The panel is React and the widget is not. 44.16 declined to mount a React root in
a widget — to dodge a stated row cap, which was the wrong reason to pay for it.
Here the panel *is* the story: "renders through 45.4's raw/rendered toggle"
cannot be satisfied by a second DOM-only toggle without building the second
answer AD-88 exists to prevent.

So the boundary is drawn to cost nothing:

- `file-embed.ts` imports no React. `live-preview.ts`'s static graph is unchanged
  in that respect, and the "React-free lazy chunk" `gallery-block.ts` describes
  stays true.
- The panel arrives through `await import("./file-embed-host")` — the one place
  in this story where a static import genuinely cannot work, named at the call
  site as the rule requires. A static edge would drag `RawRenderedView`,
  `TextEditorSurface` and every CodeMirror language pack into the editor's chunk
  for the benefit of a note that may contain no embed (NFR-27).
- The registry comes from `@/lib/viewers/registry`, not the `@/lib/viewers`
  barrel, for the same reason: the barrel re-exports the component table, which
  reaches `TextFileViewer`. `registry.ts` is pure and imports nothing, which 45.2
  made true on purpose.
- `destroy()` unmounts in a `queueMicrotask`. CodeMirror tears widgets down while
  updating its DOM, and that can itself be inside a React commit when the note
  editor unmounts from an effect cleanup; React refuses a synchronous unmount
  then and leaves the tree attached.

The panel's box is a fixed 384 px, from one constant used by both the inline
style and `estimatedHeight`. Fitted-to-content was rejected twice over: the
reader toggles Source/Table *inside a note they are scrolled into*, and a box
that resizes on that click scrolls the note out from under them; and 44.10's
windowed list measures its viewport to decide how many rows to mount, so an
auto-height pane would report its own content's height and mount all of it.

### 3. A second coordinate problem, which I found rather than inherited

44.16 justified checking `.csv` **before** the recording branch like this: "a
session's own files are video, audio and images — never a spreadsheet". That
premise is load-bearing for an ordering decision and **it is false the moment the
embed set includes JSON**: `manifest.json` is a session file, and
`recording-embed.test.ts` has shipped a test for it since 42.4.

The real shape of it: a recording session and a notes vault are **different
address spaces**. In a recording note, `![[…]]` may name the session's own file
under the recordings destination, or a vault attachment beside the note — which
is what the attachments panel writes — and **no synchronous test on the bracket
text can tell them apart**. Ordering data-first breaks `manifest.json` (the panel
looks in the vault and correctly reports it missing). Ordering recording-first
breaks a `.csv` attached to a recording note (the index declines and the embed
degrades to a link), which is precisely what 44.16 shipped.

Resolved by asking both, in the order that is right in a recording note:

- `renderRecordingEmbedInto` now returns `Promise<boolean>` — whether it claimed
  the embed. Additive; every existing call site ignores it. `false` is not
  "missing", it is "not one of this session's files", which is the licence to
  look in the vault. The two failure paths also answer `false`, deliberately: the
  index not answering is not evidence about the vault either.
- `FileEmbedWidget` owns the whole data-target case, session or not. It hands the
  recording renderer its own host carrying `cm-lp-recording` — so a claimed embed
  has the DOM every other recording embed has — and mounts the vault panel only
  when the answer is `false`.
- Nothing is released on destroy for the claimed case, and that is a proof rather
  than an omission: a target only reaches this widget when the registry called it
  a text-shaped data format, and `classifier-agreement.test.ts` pins that no such
  extension is video, image or audio in Rust. The session's answer for one is
  always a chip, which holds nothing to release.

### 4. Two embeds of one file, keyed on the resolved path

Two embeds of one file in one note are two React roots with two buffers and no
common ancestor; the note's document did not change, so no CodeMirror update
fires either. A module-level bus in `file-embed.ts` closes that gap.

The key is the **`relPath` Rust answered with**, not the text between the
brackets. `![[data.csv]]` and `![[attachments/data.csv]]` are the same file —
Rust resolves a bare name into the attachments folder — and that is exactly the
pair a person creates by hand. The announcer never hears itself: the panel that
wrote already knows, and a reload it did not ask for would throw away the buffer
it just persisted.

Both write paths announce: the raw save, and 44.16's cell edit (which never
touches the raw editor and therefore needs its own wiring). Both are tested, and
each has its own mutation.

## The Rust half

`keeper-core::notes::embed` holds the rules; the shell holds the effect.

- `candidates(target, attachments_dir)` — the paths keeper tries, in order. The
  attachments folder is a **parameter**, so the shell's `ATTACHMENTS_DIR` stays
  the single spelling of that directory rather than gaining a copy in another
  crate.
- `not_found_notice(target, candidates)` — the sentence, built from the same list
  the loop walked, so the words cannot describe a search the code did not run.
  44.16's message named neither path it tried; this one names both, which is the
  acceptance criterion.
- `write_refusal(rel, extension)` — a note is never written through an embed. It
  is here rather than at the call site because it is the one rule in this story
  that can lose work: a `.md` written this way bypasses `notes_save`'s
  `base_rev`, its conflict copy and its reindex, so a stale buffer in one
  machine's embed would silently overwrite a note edited on another with nothing
  left to recover. The frontend does not route markdown here — and that is
  exactly why the guard exists, because a rule enforced only by the caller that
  happens to exist today is enforced by nothing. `extension` is the caller's
  already-lowercased value, so this crate grows no second answer to "what is this
  file's extension" that could disagree with the vault walk's.
- `NoteEmbedVm { rel_path, name, kind, file }` — `file` is 45.6's `TextFileVm`,
  unchanged, so a file too large to edit in a panel is too large to edit in a
  note and one constant is the only way those can agree. `kind` is Rust's
  `kind_for_file_name`, because 45.2's registry refuses to answer without one.

`notes_csv_read` and `notes_csv_set_cell` now resolve through the same
`embed_path`, so the CSV table and the raw editor cannot disagree about which
file an embed means.

## I/O and edge-case matrix

### `embedEntryFor` (which embeds get a panel)

| target | result | why |
| --- | --- | --- |
| `attachments/people.csv`, `EXPORT.CSV` | the `csv` row | the registry lowercases |
| `attachments/config.json` | the `json` row | |
| `rows.jsonl`, `rows.ndjson` | the `jsonl` row | one format, two spellings — the registry's fact, not this module's |
| `Weekly review.md` | `null` | transclusion is a different feature; a raw editor over a note would be a second way to write one |
| `notes/readme.txt`, `src/main.rs` | `null` | no rendered half — a toggle showing the same bytes twice |
| `attachments/clip.mov`, `report.pdf` | `null` | not `viewer: "text"` |
| `notes/csv`, `csv.md` | `null` | no matching extension |

### The panel

| situation | what the reader gets |
| --- | --- |
| a `.csv` in a note | 44.16's table, from `notesCsvRead(vaultId, target)` |
| a cell edited | `notesCsvSetCell(vault, target, rev, row, column, value)` — coordinates and the revision, never a document — then a re-read, so the Source pane is not stale |
| a `.json` | the structure, and a `Structure` / `Source` tablist; no CSV command is called |
| malformed JSON | `role="alert"` naming the line; the source, editable |
| an edit in Source, `Mod-s` | `notesEmbedWrite` with the exact buffer — CRLF survives end to end |
| a bare `![[people.csv]]` | the target goes to Rust verbatim; Rust answers with `attachments/people.csv` |
| the file has moved | Rust's sentence, naming every path it looked for; no editor and no table |
| two embeds, one file, different spellings | the other re-reads; the writer does not |
| two embeds, different files | neither disturbs the other |
| Rust's `kind` disagrees with the syntactic guess | Rust wins: no tablist, and the format's write refusal |
| `manifest.json` in a recording note | 42.4's chip — the session is asked first |
| `attachments/people.csv` in a recording note | the session declines, the vault panel mounts |
| an ordinary `[[link]]`, or `![[note.md]]` | untouched; no read is issued |

### The widget

| situation | behaviour |
| --- | --- |
| `toDOM` | the wikilink immediately; the panel replaces it when the import and the read land |
| destroyed before the import resolves | nothing is mounted; the link stands |
| same vault, target and session | `eq` is true, so CodeMirror reuses the DOM and a half-typed cell survives the caret moving |
| an event inside the panel or on a chip action | claimed from CodeMirror |
| an event on the degraded link | given up |

## Tests, and the mutation table

`bun run test src/components/notes/editor/ src/components/viewers/ src/lib/viewers/`
— **exit=0, 31 files, 649 tests, three consecutive runs**. `cargo test -p
keeper-core notes::embed` — **exit=0**, 10 tests. Judged by exit code, not the
summary line.

22 of those are `file-embed.test.tsx`; `csv-table.test.ts` (8) and
`recording-embed.test.ts` (46) are green.

**One test of mine was green until the box was busy, and it is fixed rather than
re-run.** `the renderer > turns a data embed into a panel …` asserted on
`.cm-embed-block` after a fixed number of microtasks. It passed alone three
times and failed once inside the full run: CodeMirror recomputes its viewport on
a measure pass that runs in an animation frame, so whether the widget's host is
in `contentDOM` at a given microtask depends on whether a frame elapsed, which
depends on load. Exactly the shape of the `getClientRects` fault this wave
diagnosed. It is a `waitFor` now, with the reason written in the test.

22 mutations, all caught. Harness in `~/.W2Embeds/`, targeted string replace and
inverse replace, never a whole-file restore.

| # | mutation | caught by |
| --- | --- | --- |
| M1 | `embedEntryFor` returns every row | `which embeds get a panel > leaves alone everything a panel would be the wrong answer for` |
| M2 | tables only; JSON loses its panel | `… > is the registry's answer and not a list in this module` |
| M3 | the writer hears its own announcement | `a CSV embed > re-reads the file after a cell lands` |
| M4 | the bus keys on the bracket text | `two embeds of one file > … even spelled differently` |
| M5 | `eq` ignores the session | `the widget > reuses the DOM for the same embed` |
| M6 | `ignoreEvent` gives the panel's events up | `the widget > claims the panel's events from CodeMirror` |
| M7 | the session is never asked first | `a data embed in a recording note > lets the session have its own manifest` |
| M8 | the session always claims | `… > falls through to the vault for an attachment the session does not own` |
| M9 | a panel mounts into DOM CodeMirror threw away | `the widget > drops a panel whose import resolved after the widget was destroyed` |
| M10 | a raw save is never announced | `two embeds of one file > … even spelled differently` |
| M11 | a landed CSV cell is never announced | `two embeds of one file > … after a CELL edit in one` |
| M12 | the row comes from the spelling, not Rust's kind | `Rust's answer about the file, not the spelling > …` |
| M13 | `live-preview` routes every embed to a panel | `the renderer > leaves a markdown embed to the wikilink it has always been` |
| M14 | the note's vault coordinates never reach the table | `a CSV embed > renders the table … from the vault coordinates a note has` |
| M15 | `useTextBuffer` calls a command it has no coordinates for | `useTextFile, opening > says a file outside every profile cannot be opened here` |
| M16 | `TextFileFrame` draws an editor over bytes that are not text | `text-file-viewer > refuses bytes that are not text, in Rust's own words` |
| M17 | the attachments folder is prefixed onto a literal path | `notes::embed::a_target_with_a_slash_is_taken_literally` |
| M18 | the missing-file notice names no path | `notes::embed::the_missing_file_notice_names_every_path_that_was_tried` |
| M19 | a note may be written through an embed | `notes::embed::a_note_is_never_written_through_an_embed` |
| M20 | the kind is decided from the whole path | `notes::embed::the_name_and_the_kind_come_from_the_resolved_path` |

**One survivor, fixed rather than excused.** M15 initially survived. The reason
was mine: the profile facade's unreachable branch supplies a *rejecting stub*
rather than a call with an empty profile id, and I had given that stub a message
containing "not inside a synced folder" — the exact phrase the test asserted with
`toContain`. So removing the short-circuit changed the sentence the reader sees
(the half that says "Use Open With to read it" is lost) while the assertion
still passed. The assertion now pins the **whole** sentence, and M15 is caught.
The stub is still a rejection rather than a call with `""`, because that is what
makes a future regression a failed read instead of a command quietly aimed at a
profile that does not exist.

**I hit the deletion trap the brief warns about, exactly as described.** M13's
forward mutation turned a unique line into a second copy of
`if (match[0].startsWith("!")) {`; the inverse replace found two occurrences,
refused loudly, and left the mutant live in `live-preview.ts` — a shared file
mid-wave. Caught by the guard refusing rather than silently picking one, restored
by hand, and every anchor re-grepped **by name** afterwards. The re-run used a
unique sentinel (`/* W2EMBEDS-MUTANT */`) so the inverse was as targeted as the
forward. That same by-name grep found a **live mutant belonging to another
agent** (`attach.rs:88`, `link.embed || true) // MUTR2`), reported to W2Attach.

Baseline green before and after the sweep, at exactly the verdict's scope.

## Findings

**44.16's ordering justification was false.** See decision 3. It is the second
time in this epic family that a stated premise was load-bearing and untested; the
first was 45.4's discovery that `NoteVaultVm` already carries `profileId`.

**Nothing I was asked to build already existed, and here is the search.**
`grep -rni embed` over `keeper-core/src/notes` finds `links.rs`'s `embed` flag
(a boolean on an indexed link, for the index — no resolution, nothing to reuse)
and prose. No `NoteEmbedVm`, no vault-scoped text read or write, no general
attachment resolver: `csv_path` was the only one and it was private to the CSV
commands, which is why this story lifted it rather than adding a second. On the
frontend, `renderCsvTableInto`, `isCsvTarget` and `CsvTableWidget` were the whole
of the embed machinery. **`useTextFile`'s header already named this story** as
its second caller, and `text-viewer.tsx`'s says "Story 45.12 will mount it inside
a note" — so the seams were anticipated and I used them rather than adding new
ones. Nothing dead was found and nothing dead was left: `CsvTableWidget` and
`isCsvTarget` are deleted, not kept as aliases.

**`livePreviewTheme` is `EditorView.baseTheme`, so it applies only inside a
CodeMirror.** The `.cm-csv-*` rules 44.16 added live there, which means the CSV
table 45.4 mounts in a **Files panel** has no borders, no ragged-row colour and
no hatched missing cells. My embed is inside the editor, so it gets them.
Pre-existing, cosmetic, not mine to fix mid-wave; reported here so it is not
rediscovered as a regression of this story.

**A wrong WHY in a converted comment.** When I moved `csv-table.test.ts` and
`recording-embed.test.ts` onto `withRangeRects` I copied a mechanism that
W2Table, W2Marks and W2Emoji then disproved by measurement — the shim was never
order-dependent, because vitest isolates per file. Both comments now state the
measured thing.

## Deliberately NOT done

- **No markdown embed.** `![[note.md]]` stays a wikilink. Transclusion is a
  different feature with a different meaning, and `notes_embed_write` refuses a
  `.md` in Rust so the two halves agree rather than merely coinciding.
- **No revision on `notes_embed_write`.** `notes_csv_set_cell` carries one
  because it splices into bytes it did not read again. This is a whole-file save
  of a buffer the reader is looking at — the same contract `sync_write_entry`
  gives the Files surface for the same file. Two answers to "may I save this",
  differing by which surface you opened it from, would be worse than either.
- **No `touch` after an embed write.** The notes walk collects `.md` and nothing
  else, so asking the reconciler to re-read a `.csv` would be asking for an index
  entry that cannot exist. 44.16's reasoning, unchanged.
- **No profile→notes-vault resolution.** 45.18's, by Main's standing ruling. The
  consequence is stated in `text-file-frame.tsx` where both callers can read it.
- **No second byte formatter, no second parser, no second classifier.** Sizes are
  `TextFileVm.sizeLabel`; CSV grammar is `keeper-core::notes::csv`; JSON is 45.4's
  `json-structure.ts`; what a file IS is `kind_for_file_name`; what it renders as
  is 45.2's table.
- **No new dependency.** `react-dom/client` is already in `package.json` and
  already loaded by `main.tsx`.
- **No virtualisation of the CSV table.** Still 44.16's 500-row cap, still
  stated. The panel is React now, so 44.10's hook is technically reachable — but
  raising a cap the story did not ask about is scope this epic can spend better.
- **No `RawRenderedView` changes.** W1RawRendered's component is consumed
  unmodified; `csv`, `csvOptions`, `preview` and `onExternalWrite` were already
  the props this story needed.

## What I could not verify here, and why

- **The `keeper` shell crate has never been compiled.** `cargo check -p keeper`
  fails in `gio-sys`'s build script before any type checking happens ("The
  pkg-config command could not be found"), per AD-55/AD-56 and DW-171. So
  `embed_path`, `notes_embed_read`, `notes_embed_write` and the two lines in
  `lib.rs` are **unchecked by a compiler on this machine** — every decision they
  carry is in `keeper-core::notes::embed`, which does compile and has 10 passing
  tests, but the wiring around it does not. What is at risk is spelling: an
  argument order, a `&Path` coercion, a missing import. The macOS gate is where
  that is found.
- **`keeper-core` compiled and its bindings regenerated.** `NoteEmbedVm.ts` was
  written by ts-rs, not by hand, and `git status src/lib/ipc/gen` shows it as the
  only file of mine there.
- **Nothing ran in WebKit.** Everything was exercised in jsdom with a real
  `EditorView`. What jsdom cannot answer, and what the macOS run should look at:
  whether a nested CodeMirror inside a widget of another CodeMirror behaves —
  the widget host is `contenteditable="false"` and the inner `.cm-content` is its
  own editing host, which is well-defined but untested in a real compositor;
  whether focus and selection cross that boundary sanely; whether the 384 px box
  reads well in a narrow note; and whether the `.cm-csv-*` rules paint inside the
  panel as they do for 44.16's table today.
- **The `keeper-note://` side is untouched**, so nothing here changes what the
  asset protocol serves.
- **No real vault was written.** Every write assertion is about the command's
  arguments. That the untouched rows of a CSV come back byte-identical is
  `keeper-core::notes::csv`'s promise and is proved by that module's
  `an_edited_cell_moves_its_own_bytes_and_no_others` and
  `writing_a_cell_its_own_value_back_reproduces_the_file_byte_for_byte`, not by
  anything in this story.
