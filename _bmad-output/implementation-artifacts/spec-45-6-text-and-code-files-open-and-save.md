# Spec 45.6 — Text and code files open and save

status: implemented
story: Epic 45, Wave 1, Story 45.6
binds: FR-179, UX-DR69, AD-88, AD-89, AD-65, AD-55, AD-56, DW-162, DW-171, DW-172

## The one sentence

A text-shaped file opens in the editor the note editor already configures, with the
syntax it deserves, and saves the exact bytes that were typed — or, if it is too
large to edit comfortably, opens read-only and says how big it is.

## What was found already present

Two things this story was about to build already existed and were already applied,
which is the seventh instance of the pattern in this epic family:

- **`EditorState.lineSeparator` was NOT present**, and its absence was a live
  data-loss-shaped bug in every naive `EditorView` — see "The CRLF trap" below.
  This is the inverse of the pattern and worth recording as such: the thing that
  looked like a detail was the defect.
- **43.1's `indentBindings`** is the whole Tab answer and needed no second version.
  This story imports it; it does not restate it. `text-viewer.test.tsx` presses Tab
  and Escape-then-Tab at the new host, so 43.1's fix and 43.1's escape hatch are now
  defended in two editors rather than one.
- **`keeper_core::size::format_file_size`** (45.5, landed during this wave) meant the
  read-only banner needed no byte formatter. Main's ruling — no TypeScript byte
  formatter, consume the Rust label — is why `TextFileVm` carries `sizeLabel` rather
  than the component computing one.

## Where the decisions live

| Decision | Home | Compiles on Linux |
| --- | --- | --- |
| Is this text, or bytes no editor should show? | `keeper_core::text_file::open_text_file` | yes |
| Is it too large to edit? | `keeper_core::text_file::TEXT_EDIT_MAX_BYTES` | yes |
| How big is it, in words? | `keeper_core::size::format_file_size` (45.5) | yes |
| Which grammar for which language id? | `src/components/viewers/text-editor-host.ts` | n/a |
| Which language id for which extension? | `src/lib/viewers` (45.2) — **not here** | n/a |
| Resolve a subpath to a real path | `keeper_sync::browse::resolve` — **not restated** | yes |
| Write the bytes back | 45.3's `sync_write_entry` — **not a second writer** | yes |

The Tauri shell (`keeper/src/sync_ipc.rs::sync_read_text`) is a call site: resolve,
`spawn_blocking`, hand the result over. It contains no threshold, no classification
and no sentence, because the shell crate does not build on a Linux developer machine
and a decision written there would be one nobody could exercise until macOS
(AD-55, AD-56).

## The size threshold, and why it is this number

`TEXT_EDIT_MAX_BYTES = 1_000_000` — one megabyte, **decimal**.

**Why decimal rather than `1 << 20`.** `format_file_size` is decimal (45.5, ruled
binding by Main: the number keeper shows must equal the number Finder shows). So a
decimal limit renders as `1.0 MB` and a file of 1 048 576 bytes also renders as
`1.0 MB` — if the limit were binary, a user would see "this file is 1.0 MB" beside
"keeper edits files up to 1.0 MB" and one of the two would be lying. A limit stated
in units the surface never uses is a limit users misread.

**Why one megabyte and not ten.** The cost is not CodeMirror's viewport rendering,
which is fine at any size; it is the layers keeper puts over it. A Lezer parse and —
for markdown — the decoration pass in `live-preview.ts` walk far more than the
visible lines. One megabyte is on the order of twenty thousand lines, which stays
responsive; past it the first keystroke after opening starts to be perceptible, and
an editor that stutters on every keypress is worse than one that says plainly it will
not edit this file.

**Why it is comfortably above real hand-edited files.** A large `Cargo.lock` is
~200 kB; this repository's largest source file is under 300 kB; a note is a few kB.
A file over a megabyte is nearly always machine-written, and "look, do not edit" is
the honest answer for one.

**Over the limit means truncated, and that is why read-only is not advisory.**
Nothing above the limit is read: `open_text_file` stats, then reads at most the cap
through `Read::take`. Sending a 2 GB log across IPC to then refuse to edit it would
freeze the pane exactly as thoroughly as editing it would — the failure the story
names. So `text` holds a prefix, `oversize` says so, and the read-only lock is not a
courtesy: **the buffer is not the file**, and saving it would delete everything past
the first megabyte. The lock is enforced in three independent places (Rust refuses,
the hook declines and logs, the component forces the editor read-only) because this
is the only path in the story that can destroy data.

## The CRLF trap, and the three lines that close it

`EditorState.create({ doc })` splits the document on `/\r\n?|\n/` and
`doc.toString()` rejoins with `state.lineBreak`, which defaults to `"\n"`. Verified
in this worktree: `EditorState.create({ doc: "a\r\nb\r\n" }).doc.toString() === "a\nb\n"`.
So a naive editor over a Windows file holds an LF buffer from the instant it is
constructed, and the first character the reader types produces an `onChange` with
every line ending rewritten — a whole-file diff carried straight into git by sync,
on a file the user thought they barely touched. That breaks `TextFileVm`'s own
promise, in its own words.

The fix is `EditorState.lineSeparator.of("\n")`: CodeMirror then splits on `"\n"`
only, each line keeps its trailing `"\r"` as an ordinary character, and the document
round-trips byte for byte.

**The alternative that was considered and rejected**: detect the file's dominant
terminator and set `lineSeparator` to it. That needs a guess, needs the boot effect
keyed on the guess, and mangles a mixed-ending file in both directions. The chosen
form needs neither and degrades correctly on mixed input.

**The cost, stated plainly**: a newline typed into a CRLF file is an LF, so an edited
Windows file gains mixed endings on the lines actually touched. That is a change
confined to the lines the user edited, which is the smaller wrong — and it is a
change confined to *edited* lines rather than *all* lines, which is the whole point.

A UTF-8 BOM is in the same family and is handled by the same discipline: Rust does
not strip it (`a_byte_order_mark_survives`) and the editor holds it as an ordinary
leading character, so it survives an open-and-save.

## The dependency

**`@codemirror/legacy-modes` 6.5.3**, approved by Main with the cost named.

- **Per-mode dynamic entry points.** Each grammar is its own `import()`
  (`@codemirror/legacy-modes/mode/toml`), so the bundler emits one chunk per mode
  and a user who never opens a `.toml` downloads zero bytes of TOML tokeniser. A
  user who does pays roughly 2–6 kB gzipped for that one mode.
- **No transitive weight** beyond `@codemirror/language`, already present.
- It rides the dynamic-import seam `note-editor.tsx` already established for NFR-27
  rather than opening a new one.
- `bun run check:licenses`: 669 packages scanned, 0 denied.

**Legacy modes are CodeMirror 5 stream tokenisers, not Lezer grammars.** They colour
text; they build no syntax tree. Nothing structural can ever be built on one — no
"select this TOML table", no format-on-save, no `syntaxTree()` query. A future story
that wants any of that for TOML, YAML, Rust or shell needs a real grammar for it
first. This is written down here so nobody in a year assumes a parse tree exists
because the file is coloured.

The four formats that DO have real Lezer grammars — markdown, JavaScript/TypeScript,
CSS, HTML — come from packages already in the tree (`lang-markdown` → `lang-html` →
`lang-javascript` + `lang-css`), cost nothing new, and are preferred wherever they
apply. JSON uses `legacy-modes/mode/javascript`'s `json` variant rather than adding
`@codemirror/lang-json`: one package, not two, for one format.

## Two tables that cannot contradict each other

`src/lib/viewers` (45.2) maps **extension → language id**, and its
`classifier-agreement.test.ts` proves it is the only table that does.
`text-editor-host.ts` maps **language id → grammar**. Neither mentions the other's
vocabulary, so they cannot disagree about what a `.rs` is.

`text-editor-host.test.ts > can load a grammar for every language id the registry
uses, except php` derives the ids in use from `FILE_FORMAT_ENTRIES` — their real
table, not a curated list, which would be a third place the vocabulary lives — and
asserts the shortfall is exactly `["php"]`. Adding a registry row with an unwired id
fails that test rather than shipping a file that opens monochrome.

`php` is the one deliberate hole: `@codemirror/legacy-modes` has no PHP tokeniser and
`@codemirror/lang-php` would be a second dependency for one row. It opens as plain,
editable text and logs the reason at `console.info`.

## I/O and edge-case matrix

### `keeper_core::text_file::open_text_file(path)`

| Input | `text` | `sizeBytes` | `oversize` | `binary` | `detail` |
| --- | --- | --- | --- | --- | --- |
| ordinary UTF-8 file | exact bytes | real | false | false | `None` |
| `"one\ntwo\n"` | `"one\ntwo\n"` | 8 | false | false | `None` |
| `"one\ntwo"` (no final newline) | `"one\ntwo"` | 7 | false | false | `None` |
| CRLF + hard tab | preserved verbatim | real | false | false | `None` |
| UTF-8 BOM | BOM kept as `\u{feff}` | real | false | false | `None` |
| empty file | `Some("")` | 0 (`"0 bytes"`) | false | false | `None` |
| exactly 1 000 000 bytes | whole file | 1 000 000 | **false** | false | `None` |
| 1 000 001 bytes | first 1 000 000 | 1 000 001 (`"1.0 MB"`) | **true** | false | names size + "read-only" |
| 4 000 000 bytes | first 1 000 000 | 4 000 000 (`"4.0 MB"`) | true | false | names **4.0 MB**, not 1.0 |
| bytes containing NUL, valid UTF-8 | `None` | real | (as sized) | **true** | "not text … no editor can show" |
| executable header (`MZ\x90\0…`) | `None` | 8 | false | true | same |
| Latin-1 (`caf\xe9`), no NUL | `None` | real | false | true | "not valid UTF-8" |
| oversize, cap splits a `☃` in half | valid head only | real | true | **false** | oversize sentence |
| complete file ending mid-character | `None` | real | false | **true** | "not valid UTF-8" |
| path does not exist | — | — | — | — | `Err(NotFound)`, never "binary" |

The last two rows are the pair that matters: a half character is an artefact of *the
cap* when the file was truncated and a property of *the file* when it was not, and
the recovery must not extend to the second case.

### `TextEditorSurface`

| Input | Result |
| --- | --- |
| `content`, any bytes | document is exactly those bytes, CRLF and tabs intact |
| a keystroke | `onChange(exact buffer)`, every time |
| `content` prop changes | live document adopts it, **same view object** — caret, selection and undo stack survive |
| `content` prop comes back unchanged (the controlled loop) | no dispatch; caret stays where it is, including mid-document |
| `Mod-s` | `onSave(exact current text)` |
| `Tab` | claimed; indents two spaces; never a literal `\t` |
| `Escape` then `Tab` | **not** claimed — 43.1's accessibility escape hatch survives |
| `readOnly` | typing, paste, Backspace and Enter all refused; no banner (this file is not too big, it is simply not writable here) |
| buffer > 1 000 000 UTF-8 bytes | read-only regardless of `readOnly`, banner naming `sizeLabel` |
| buffer > limit, no `sizeLabel` | banner says "too large to edit" and names **no** size — never an invented one |
| 400 000 × `☃` (1.2 MB, 400 000 UTF-16 units) | oversize; measured in bytes, so Rust and the browser agree |
| a programmatic `view.dispatch` while read-only | document changes (dispatch is allowed to); `onChange` does **not** fire |
| `language: "plain"` / `"csv"` / `null` | plain text, editable, **nothing logged** |
| `language: "php"` (unwired) | plain text, editable, `console.info` names the id |
| grammar chunk rejects | plain text, editable, saveable; `console.info` names the id and the error |

### `useTextFile({ profileId, subpath })`

| Situation | `content` | `dirty` | `error` | writes? | logs? |
| --- | --- | --- | --- | --- | --- |
| ordinary open | exact bytes | false | null | — | — |
| `profileId === null` | `""` | false | "not inside a synced folder…" | no | on save attempt |
| read rejected | `""` | false | Rust's sentence verbatim | no | — |
| oversize file | prefix | false | **null** (it opened fine) | — | — |
| binary file | `""` | false | Rust's sentence | no | on save attempt |
| edit then save | typed text | false after | null | `syncWriteEntry(id, subpath, exact text)` | — |
| trailing newline added / removed | as typed | — | — | exact bytes, no normalisation | — |
| edit typed and typed back | original | **false** | — | — | — |
| save with nothing changed | unchanged | false | — | **no** | "nothing changed" |
| save an oversize file | prefix | true | — | **no** | names size + "truncate" |
| save a binary file | `""` | — | — | **no** | "not text" |
| write rejected | **buffer kept** | **true** | Rust's sentence | attempted | — |
| `reload()` after an outside write | disk's new text | false | — | — | — |
| slow read for a previous file lands late | current file's text | — | — | — | — |

Every decline is `console.info`, not `console.debug` — DW-162 applied to the browser
console. A save that silently does nothing is the failure the epic names by name, and
mutation M20 (downgrading the level) is caught.

## Mutation table

Baseline green **before and after** each sweep, at exactly the verdict's scope:
TypeScript 44 tests over the three suites named below; Rust 14 over
`cargo test -p keeper-core text_file`. Harness in `~/.W1TextEditor/mutate.py`
(never `/tmp`).

**29 mutations, 29 caught, 0 survived, 0 unproved.**

### TypeScript — scope: `text-viewer.test.tsx`, `text-editor-host.test.ts`, `use-text-file.test.tsx`

| # | Mutation | Verdict | Caught by |
| --- | --- | --- | --- |
| M1 | measure UTF-16 units instead of UTF-8 bytes | caught | `counts UTF-8 bytes, not UTF-16 units` |
| M2 | `>` → `>=` at the limit | caught | `is false at the limit and true one byte past it` |
| M3 | drift the mirrored limit away from Rust's | caught | `is the same number Rust decided` |
| M4 | read-only stops typing but not paste/Backspace | caught | `a file over the limit … refuses input` |
| M5 | drop `lineSeparator`: CRLF normalised on open | caught | `opens with the file's content byte for byte` |
| M6 | re-dispatch an unchanged `content` prop | caught | `does not re-dispatch when the prop comes back unchanged` |
| M7 | drop 43.1's Tab binding from this host | caught | `claims Tab, so 43.1's fix is not undone` |
| M8 | log "no grammar wired" for `plain`/`csv` | caught | `says nothing for the ids that are text with no syntax` |
| M9 | oversize no longer forces read-only | caught | `a file over the limit opens read-only…` |
| M10 | report edits even when read-only | caught | `ignores a programmatic change while read-only` |
| M11 | banner stops naming the size | caught | `a file over the limit … names its size` |
| M12 | save something other than the current buffer | caught | `saves exactly what was typed, on Mod-s` |
| M13 | `persisted` back to a non-reactive holder | caught | `is clean again after a save…` + 4 decline tests |
| M14 | save an oversize prefix over the whole file | caught | `declines, out loud, for an oversize file` |
| M15 | save text over a binary file | caught | `declines, out loud, for a binary file` |
| M16 | never mark clean after a successful write | caught | `is clean again after a save…` |
| M17 | mark clean even though the write was refused | caught | `keeps the buffer and stays dirty when the write is refused` |
| M18 | report an oversize file as an error | caught | `treats an oversize file as opened, not as an error` |
| M19 | let a stale read overwrite the current buffer | caught | `lets a late read for the previous file lose` |
| M20 | decline at `console.debug` instead of `info` | caught | all five decline tests |

Three of these — M6, M8, M10 — **survived the first pass** and were closed by
strengthening a test, not by rounding up:

- **M6** survived because the test put the caret at the end of the document, where a
  whole-document replacement maps the caret to the end either way. Rewritten to type
  in the middle of `abcdef`, which is where the failure is visible: without the guard
  the caret teleports to the bottom of the file after every keystroke.
- **M8** survived because nothing asserted the *absence* of a log line. Now covered.
- **M10** survived because `EditorState.readOnly` blocks paste before the guard is
  reached, so no user path exercised it. Now driven by a programmatic
  `view.dispatch`, which is a real path: 45.4's rendered CSV view holds this very
  buffer and dispatches into it.

**M13/M16 found a real defect.** `dirty` was derived from a `useRef`, so advancing it
after a successful save produced no re-render — the only state update a successful
save makes is `setError(null)`, which React bails out of when the error was already
null. The surface would have kept showing unsaved-changes chrome over a file that was
on disk. Fixed by making `persisted` a `useState`, with the reason in the comment.

### Rust — scope: `cargo test -p keeper-core text_file`

| # | Mutation | Verdict | Caught by |
| --- | --- | --- | --- |
| R1 | shrink the limit to 999 999 | caught | `a_file_exactly_at_the_limit_is_editable` |
| R2 | `>` → `>=` at the limit | caught | same |
| R3 | stop refusing NUL bytes | caught | `valid_utf8_containing_a_nul_is_still_refused` |
| R4 | recover a half character in a complete file | caught | `a_truncated_character_in_a_complete_file_is_still_binary` |
| R5 | call a cap-split character a binary file | caught | `a_multibyte_character_split_by_the_cap_is_not_a_binary_file` |
| R6 | reword the UTF-8 refusal | caught | `invalid_utf8_without_a_nul_is_refused_rather_than_mangled` |
| R7 | read the whole file instead of a bounded prefix | caught | `a_much_larger_file_states_its_real_size` |
| R8 | report the prefix's length as the file's size | caught | `one_byte_over_the_limit … names_the_files_own_size` |
| R9 | stop explaining an oversize file | caught | same |

**R3 survived the first pass**, and the reason is the interesting one: the existing
fixture was an executable header (`MZ\x90\x00…`), whose `\x90` is a stray
continuation byte — so the UTF-8 arm caught it and the NUL branch was never reached
by any test. A NUL byte **is** valid UTF-8. Closed with
`valid_utf8_containing_a_nul_is_still_refused`, whose fixture (`name\0\0value\0\0`)
asserts its own decodability first so it cannot silently drift back into the UTF-8
arm. That is the "fixture that cannot reach the boundary the code branches on" shape
the brief warns about, and it was a real gap rather than an equivalent mutant.

## Deliberately NOT done

- **No second write command.** Saving goes through 45.3's `sync_write_entry` /
  `syncWriteEntry`. This story adds only the *read* half, because reading and writing
  are different capabilities: a file outside a vault can be viewed but not written,
  and one command would have to refuse half of itself.
- **No registry row, and no component bound to `text`.** AD-88 says raw and rendered
  are one component; W1Registry ruled that 45.4 binds `text` and this editor is its
  raw half. `TextViewer` was designed, then deliberately deleted rather than
  exported, so there is nothing here that *could* be bound twice.
- **No extension table.** The grammar comes from `entry.language`. `fileName` is
  display and accessible naming only, and is never parsed.
- **No TypeScript byte formatter.** `sizeLabel` is Rust's. A caller with no label
  gets a banner that names no size rather than an invented one.
- **No PHP grammar.** Named, not silently absent — see "Two tables" above.
- **No structural editing of any legacy-mode format.** There is no syntax tree.
- **No merge on `reload()`.** It replaces the buffer. A three-way merge here would be
  a second, silent conflict resolver beside the one the notes path already has.
- **No line-ending normalisation, ever**, including on a mixed file. See the CRLF
  section for what a mixed file does get.
- **No autosave.** `Mod-s` and whatever the host wires; the note editor's
  save-on-blur is a notes-store behaviour and is not lifted here.
- **No second threshold for "too large to view".** Truncation to the edit cap makes
  one number do both jobs; a `TEXT_VIEW_MAX_BYTES` would be a second constant that
  could drift from the first.

## What I could not verify here, and why

- **The Tauri shell does not compile on this machine.** `keeper/src/sync_ipc.rs`
  (`sync_read_text`) and its registration in `keeper/src/lib.rs` are unbuilt and
  unrun: the crate needs glib-sys and there is no pkg-config on the Linux dev host.
  Everything that command *decides* is in `keeper-core` and is fully tested here; what
  is unverified is exactly the four lines of glue — `engine_of`, `find_profile`,
  `browse::resolve`, `spawn_blocking` — each copied in shape from `sync_open_entry`
  directly above it. **This needs the macOS gate before it can be called proven.**
- **`syncWriteEntry` was not in the worktree when this was written.** `use-text-file.ts`
  imports it with 45.3's committed signature `(profileId, subpath, content)`. My suite
  mocks `@/lib/ipc/client`, so it is green either way; if 45.3's command lands with a
  different name or arity, this is a one-line change and `tsc --noEmit` will say so.
  Flagged to W1FilesWrite over `hub`.
- **No end-to-end run in the packaged app.** Nothing here has been exercised against
  a real synced folder on a real disk: the read path is proved over temp directories
  in Rust and over mocked IPC in the browser suite. The specific things a real run
  would add are (a) that `browse::resolve` accepts the subpath shapes `sync_browse`
  actually produces, and (b) that a 4 MB file genuinely does not stall the pane —
  the threshold's *magnitude* is reasoned from what CodeMirror does, not measured on
  the target hardware.
- **The bundle-size claim is reasoned, not measured.** "Zero bytes for a user who
  never opens a `.toml`" follows from each mode being a separate ESM entry point
  behind its own `import()`, which is how Vite emits chunks — but no build output was
  inspected, because `vite build` is part of the gate Main runs at the end.
- **jsdom is not a browser.** The suite drives real `EditorView`s, but text arrives
  by paste rather than by `beforeinput`, because jsdom does not implement
  `contenteditable` and CodeMirror's DOM-mutation observer therefore never fires.
  Paste is a genuine user path and is gated on `EditorState.readOnly`, so the
  read-only assertions are real; what is *not* covered is composition input (IME) and
  the DOM-observation path a real WebKit would take.
- **The `withRangeRects` metrics are fake.** Every geometry-dependent behaviour —
  wrapping, the gutter's width, scroll position — is unasserted here and unassertable
  in jsdom.
