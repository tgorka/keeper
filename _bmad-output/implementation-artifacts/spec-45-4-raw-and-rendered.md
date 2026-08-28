# Spec 45.4 — Raw and Rendered

status: implemented
story: Epic 45, Story 45.4
bindings: FR-177, AD-88, UX-DR67
depends on: 45.2 (the viewer registry), 45.6 (the text editor and the loading hook), 44.16 (`keeper-core::notes::csv`), 37.6 (`live-preview.ts`)
author: W1RawRendered

## What shipped

One component, two views, a toggle that remembers per format. `raw` is 45.6's
editor over the file's real bytes and can always save. `rendered` is the
format's own view: 44.16's table for a CSV, the note editor's own preview for
markdown, a structure list for JSON and JSONL.

| file | what it is |
| --- | --- |
| `src/components/viewers/view-mode.ts` | the remembered view, per format, in a cookie. Pure. |
| `src/components/viewers/json-structure.ts` | JSON/JSONL read as a structure, with the line named when it is not one. Pure. |
| `src/components/viewers/markdown-preview.ts` | mounts `live-preview.ts` read-only over a file, guarded. |
| `src/components/viewers/raw-rendered-view.tsx` | the toggle, the refusal banner, the three rendered panes. Presentational and controlled. |
| `src/components/viewers/text-file-viewer.tsx` | the registry's `text` binding: loads, and maps `ViewerProps` onto the above. |
| `src/lib/viewers/components.tsx` | one line: `text: TextFileViewer`. |
| `src/test/layout.ts` | `withRangeRects`, so a real `EditorView` can be measured under jsdom. |

## The four decisions

### 1. `RawRenderedView` is presentational; `TextFileViewer` is the binding

AD-88 says raw and rendered are one component, and 45.2's registry keys one
component per viewer id. Both are satisfied by **one id and one binding**, with
the state in a thin adapter above a controlled body:

- `RawRenderedView` takes `content` in and hands the exact characters back. This
  is what makes AD-88's "there is one buffer" a thing a test can assert rather
  than a thing the code hopes: `raw-rendered-view.test.tsx` passes a real
  controlled `<textarea>` as the editor and checks the characters that come out.
- `TextFileViewer` calls 45.6's `useTextFile`, handles loading / unreadable /
  binary, and mounts `RawRenderedView` with 45.6's `TextEditorSurface` as its
  raw half.

Agreed with W1TextEditor over `hub` before either of us wrote a line, and
confirmed by W1Registry and Main: **there is no second component under `text`,
and no second CodeMirror configuration anywhere in this story.** The markdown
preview builds an `EditorView`, but it is `live-preview.ts`'s own extension set,
read-only, and it is the renderer 37.6 already declared to be the only one.

### 2. JSON parses in TypeScript, and here is what that cost

44.16 put CSV grammar in `keeper-core` and forbade a second opinion in the
webview for one reason: **the CSV view writes.** Two parsers over bytes one of
them will rewrite is how the table you looked at and the file you saved become
two different answers.

JSON and JSONL are **read-only in this story**. The bytes are already in the
webview because the raw editor is holding them. A round trip to Rust to be told
what the reader is already looking at buys no authority, costs a copy of the
file and a command in the `keeper` shell crate that cannot be compiled on this
machine. So the parse is in TypeScript.

What Rust would have bought, plainly: `serde_json` reports a line and a column
on its error for free, and it is one parser instead of two. Both are paid for
here instead — the line and column are computed, and
`json-structure.test.ts > parseJsonStructure agrees with JSON.parse about what is JSON`
runs a 36-document corpus through both and asserts the accept/reject verdicts
match. "A second parser" is a claim under test rather than a hope.

**If this view ever gains a write path, that argument collapses and the parse
moves to `keeper-core`.** Written in the module header as well as here.

Why not simply `JSON.parse`, which would be no second parser at all — three
things a structure view needs that a parsed JavaScript value cannot give back:

- **A line.** `JSON.parse`'s message is engine-specific prose. The acceptance
  criterion is that the reader is pointed at a line.
- **The number that is in the file.** `JSON.parse` puts every number through a
  double: `{"id": 12345678901234567890}` comes back as `12345678901234568000`.
  A viewer whose entire purpose is to show you the file must not show you a
  number the file does not contain. Numbers are kept as their own characters.
- **Key order and repeats.** An object with a key twice is real; a JavaScript
  object silently keeps the last. Both rows are drawn and the later is marked.

### 3. A malformed file: JSON and JSONL differ, on purpose

- **JSON is one document.** If it did not parse there is no structure, and
  drawing the fragment that parsed before the failure would be a picture of a
  file that does not exist. Any error ⇒ the banner names the message, the line
  and the column, and the pane shows the source, which is editable.
- **JSONL is one document per line.** The lines that parsed are whole and true,
  so they render, and each bad line gets an error row at its own file line.
  Withholding 99,999 good records because line 4,000 was truncated throws away
  most of why the format exists. Only when nothing at all parsed does it fall
  back to source.

**A forced fallback never rewrites the reader's preference.** The cookie is
written only by a click on a tab. A broken file shows source with `Structure`
still selected, so the next good file of that format opens rendered. Pinned by
M15 and by two assertions on `cookie.read() === ""`.

**An empty file is a file, not a failure.** Whitespace-only input reports
`empty` rather than a parse error, and the pane says "this file is empty, so
there is nothing to show as a structure". `JSON.parse("")` throws; this is the
one deliberate divergence from the agreement test and it is asserted as such.

### 4. The mermaid guard, and DW-165

`live-preview.ts:221` supplies `Decoration.replace({ …, block: true })` from a
`ViewPlugin`. CodeMirror refuses block decorations from a plugin, so the
`EditorView` **throws on construction** for any document containing a
```mermaid fence. Shipped since story 37.8. DW-165.

Main ruled: **degrade, pin the test, hand the fix to 45.10.** `live-preview.ts`
is the note editor's core, no other wave-1 agent is in it, and changing it
mid-wave is something fourteen agents build against.

So `mountMarkdownPreview` checks the document for a mermaid fence **using the
same markdown grammar the renderer uses**, over the `EditorState` before any
view exists — not a regex, which would be a second opinion about what a fence
is and would disagree about a tilde fence or an indented one. When it finds one
it declines by name and line, logs at `console.info` (DW-162: `console.debug`
never reaches the packaged app's log), and hands over the editable source.

The `try`/`catch` around the construction stays regardless, and is load-bearing
on its own merits: this story's acceptance criterion forbids a blank rendered
pane for **any** reason, and that has to hold for the throw nobody has found
yet. M12 removes the guard; M11 removes the line; both are caught.

**`markdown-preview.test.ts > DW-165 tripwire` performs the assembly nothing in
this repository has ever performed**: a real `EditorView` carrying BOTH
`@codemirror/lang-markdown` AND `livePreview`. It asserts the throw, as a
passing test, so the wave's gate stays green while the defect stays pinned.
**When that test fails, DW-165 has been fixed: invert it and delete the guard.**
45.10's `StateField` lift is then verified by a test whose author had no reason
to shape it around the fix.

The second test in that block is the transferable half: the same document
without the markdown language does **not** throw. That is exactly why the
existing suite is green — `recording-embed.test.ts` builds a real view around
`livePreview` but loads no grammar, so `syntaxTree` yields no `FencedCode` and
the mermaid branch is never entered.

## I/O and edge-case matrix

### `view-mode.ts`

| in | out | why |
| --- | --- | --- |
| jar with `json:raw\|csv:rendered` | both formats read back | one cookie, not one per format |
| format never stored | `rendered` | the rendered view is the thing; raw is asked for deliberately |
| `json:sideways`, `:raw`, `nocolon`, `9bad:raw` | dropped, no throw | the jar is shared with every other cookie on the origin and with older builds |
| `not_keeper_viewer_modes=json%3Araw` | ignored | a suffix match is not a name match |
| write `json:raw` into a jar holding `csv:rendered` | both survive | a cookie write replaces one name's value wholesale |
| write `null` | the format is forgotten | a reset leaves nothing to re-adopt |

### `json-structure.ts`

| in | out |
| --- | --- |
| `{"a":1}` | one object row (1 property), one number row `1` |
| `{"id": 12345678901234567890}` | `text` is the 20 digits, verbatim |
| `{"a":1,"a":2}` | two rows, second marked `duplicate` |
| `{"who":"caf\u00e9\n"}` | decoded to `café\n` — the character, not its encoding |
| `{\n "a":1,\n "b": oops\n}` | error at line 3, column 8, "a value was expected here" |
| `{"a" 1}` | "a colon was expected after the property name" |
| `{a:1}` | "a property name in double quotes was expected here" |
| `{"a": "unclosed\n}` | "this text is never closed with a quote", at the **opening** quote's line |
| `0123` | "a number may not have a leading zero" — named at the number, not at the digit |
| `{"a":1}\n{"b":2}` | "there is more text after the value the file ends with", line 2 |
| 133 nested `[` | "this file nests more than 128 levels deep, which keeper will not draw" — a named refusal, not a stack overflow |
| `""`, `"   "`, `"\n\n"`, `"\uFEFF"` | `empty: true`, no error |
| `\uFEFF{"a":1}` | parsed; the mark is skipped and the column on line 1 stays honest |
| JSONL `{"a":1}\n{"a":\n{"a":3}` | rows for 1 and 3, one error naming line 2 |
| JSONL `\n{"a":1}\n\n\n` | one record, reported at its **file** line (2), blanks skipped |
| JSONL CRLF | records parse; the `\r` belongs to the terminator |
| 5,150-element array | 5,000 rows, `totalRows` 5,151 |

### `RawRenderedView`

| situation | what the reader gets |
| --- | --- |
| `rendered: null` (a `.txt`) | no tablist at all, source only |
| toggle to source on `a.json`, then open `b.json` | source, without asking again |
| toggle to source on JSON, then open a CSV | the CSV table — the choice is per format |
| the same mount changes file to another format | that format's remembered view, adopted during render, so no frame of the wrong view |
| no `cookie` prop | reads and writes `document.cookie` |
| malformed JSON | `role="alert"` with line and column; source, editable; `Structure` still the selected tab; the jar untouched |
| the source is then fixed | the alert clears and the structure returns, with no toggle |
| markdown with a mermaid fence | `role="alert"` naming DW-165 and line 3; source, editable; `Preview` still selected |
| the fence is then deleted | the preview mounts |
| CSV with no vault coordinates | `role="alert"` "…inside a notes vault…"; source, editable |
| CSV cell edited | `notes_csv_set_cell(vault, target, rev, row, column, value)` — coordinates and the revision, never a re-serialised file — then `onExternalWrite()` |
| CSV cell **refused** by Rust | 44.16's own degrade repaints and names the refusal; `onExternalWrite` is **not** called, so the host does not discard the reader's buffer for a write that did not land |
| an empty CSV | 44.16's "…has no rows" |
| an empty markdown file | the preview mounts, no alert; source is empty |
| `readOnly` with a reason | `role="status"` with the reason; the editor is read-only |
| 5,000-row cap hit | `role="status"` "showing the first 5000 of 5101 values" |

### `TextFileViewer`

| situation | what the reader gets |
| --- | --- |
| a `.json` in a profile | structure, and `syncReadText("profile-1", "inbox/config.json")` — the listing's path, nothing joined |
| still reading | `role="status"` "opening config.json", not an empty editor |
| `vm.binary` | Rust's sentence, **no tablist and no editor** — `text ?? ""` would put an editable pane over a `.png` and offer to save it |
| read rejected | Rust's sentence |
| `profileId === null` | the hook's sentence; **no command is called at all**, because reading through `absolutePath` would go around `browse.rs`'s containment |
| edited and saved | `syncWriteEntry` with the exact buffer, tabs and trailing newline included |
| a CRLF file, one word edited | CRLF survives end to end (see below) |
| nothing changed, `Mod-s` | no write, and `console.info` says why (DW-162) |
| the save is refused | Rust's sentence in a banner; the buffer is **not** rolled back |
| `entry.writable === false` | the format refusal, by name |

## Findings

**`NoteVaultVm` already carries `profileId` and `subfolder`.** So "which notes
vault is this profile-relative path in" is already answerable in Rust — a
lookup, not new machinery. I did not use it: doing so in the webview means the
frontend stripping a vault subfolder off a profile path, which is the same
family of path arithmetic AD-65 forbids, and Main assigned the resolution to
45.18. Reported to W1Registry and Main. It is the seventh time this epic family
has found the asked-for value already present and unapplied.

**DW-165 is live and is a crash, not a quiet no-op.** Reported to Main with the
mechanism and the fix; ruling recorded above.

**A false alarm I raised and retracted, recorded because the mechanism is real.**
I reported to W1TextEditor that CodeMirror normalises CRLF to LF, which it does:
`EditorState.create({ doc: "a\r\nb\r\n" }).doc.toString() === "a\nb\n"`, verified
in this worktree. I was wrong that their editor had the bug —
`text-editor-host.ts:401` already sets `EditorState.lineSeparator.of("\n")`,
which makes CodeMirror split on `"\n"` only so each line keeps its trailing
`"\r"` as an ordinary character. I read the mount effect and not the extension
list. The mechanism still matters for **the next person who builds an
`EditorView` in this repo without that facet** — I nearly was, in
`markdown-preview.ts`; mine is read-only, so it cannot write a normalised buffer
back, and that is now written in the module. I have pinned their property from
my side anyway: `text-file-viewer.test.tsx > keeps a Windows file's line endings
when one word is edited`, through the real hook and the real editor.

**A test that passed for the wrong reason, caught by re-reading it.** That CRLF
test originally replaced the whole document with a CRLF string, which
re-introduces the terminators as ordinary characters and hides the thing being
asserted — it passed with or without the facet. It now edits `beta` → `BETA`
**by position**, leaving the text the editor was constructed with, which is
where a normalising buffer does its damage.

**`fireEvent.keyDown(el, { key: "s", metaKey: true })` matches nothing under
jsdom.** CodeMirror's `Mod` is `Ctrl` on a non-Mac platform and jsdom reports
one, so a `Mod-s` assertion written with `metaKey` passes because nothing
happened. The tests pick the modifier with the same predicate CodeMirror uses.

**jsdom has no `Range.getClientRects`, and CodeMirror's measure pass calls it**
on any animation frame that elapses during a test. The throw lands outside every
`try` a test can write, is reported as an unhandled error, and takes the run's
exit code with it whether or not an assertion failed — and whether it happens at
all depends on how many milliseconds the test spent, so a suite is green until it
is slow. `withRangeRects` in `src/test/layout.ts` models it, in the file whose
whole purpose is modelling the browser jsdom is not. It is installed per test
file and removed, never in global setup, so no other suite's behaviour changes.

## Deliberately NOT done

- **Markdown's rendered view is read-only.** Editing markdown *is* the note
  editor, which has a save path, a conflict story and a properties panel of its
  own; wiring an arbitrary file into it would be a second write path, which
  AD-88 exists to prevent. Markdown is still editable — through `raw`, which is
  always editable. Pinned by M13.
- **JSON and JSONL rendered views are read-only**, as the story says. The
  structure has no editable cell, and adding one would need the span-splicing
  writer `notes::csv` has and this does not.
- **DW-165 is not fixed here.** Guarded, named, logged and pinned; 45.10's.
- **No profile→notes-vault resolution.** 45.18's, by Main's ruling. The
  consequence is honest and pinned: a CSV opened from a panel shows its source
  with a sentence saying why, rather than an empty table. When 45.18 lands, the
  assertion in `text-file-viewer.test.tsx` named for it is the one that changes.
- **No second byte formatter.** `sizeLabel` is passed through from Rust.
- **No new dependencies.** Everything used is already in `package.json`.
- **No `keeper-core` change and no new IPC command.** The story's Rust half is
  44.16's `notes::csv`, consumed unmodified; 45.6 added `sync_read_text` and we
  agreed over `hub` that one reader is enough.
- **A Save button.** The raw editor's `Mod-s` and the host's dirty state are
  45.6's; a second control here would be a second answer to "is this saved".

## What I could not verify here, and why

- **The `keeper` shell crate was not compiled.** It does not build on this box
  (glib-sys, no pkg-config), per AD-55/AD-56. I added no Rust, so nothing of
  mine needs it — but `notes_csv_read` / `notes_csv_set_cell`, which the CSV
  rendered view calls, live there and have never been through a compiler on this
  machine (DW-171 says the same of 44.16's own wiring). What my tests prove is
  the *shape* of the call: profile-scoped arguments, the revision, the
  coordinates, and no re-serialised file.
- **Nothing here ran in WebKit.** Every rendered pane was exercised in jsdom
  with a real `EditorView`. What jsdom cannot answer: whether the toggle's
  `aria-selected:bg-muted` actually paints, whether the structure list scrolls
  at 5,000 rows in a real compositor, and whether the CSV table's column widths
  survive a narrow pane. Those are browser facts.
- **The windowed structure list's geometry is jsdom's.** `useWindowedRows` falls
  back to a 640 px assumed viewport when nothing reports a height, so the tests
  see roughly thirty rows mounted. That the window *slides* correctly is 44.10's
  own suite's claim, not re-asserted here.
- **`TextEditorSurface`'s internals are 45.6's.** My suites mount the real
  component, so they would notice it failing to render or failing to save — but
  the claims "the buffer reaches `onChange` byte for byte through a real
  `EditorView`" and "a `content` prop change reconciles without a remount" are
  asserted in `text-viewer.test.tsx` and cited rather than duplicated.
- **A real notes vault.** `notes_csv_read` is exercised through injected doubles
  with 44.16's own `NoteCsvVm` shape. The byte-identical round trip itself is
  `keeper-core`'s claim and is covered by its 21 `notes::csv` tests, which this
  story left untouched and which still pass.
- **The `text` binding's behaviour inside a note embed or quick capture.** Those
  hosts are 45.12 and 45.14. What is proven is that two different hosts asking
  the registry for one file mount the same component and render the same markup
  (`src/lib/viewers/components.test.tsx`).
- **Whether `entry.writable === false` can reach this viewer.** It cannot today
  — no `viewer: "text"` row is non-writable — so that guard is tested by
  building the row by hand. A guard that only runs on inputs the current table
  cannot produce is precisely the guard that rots unnoticed, so it is tested
  directly and the comment says why.

## Verification

`bun run vitest run src/components/viewers/ src/lib/viewers/` — **12 files, 233
tests, green.**

`cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::csv`
— **21 passed, 0 failed.** Unchanged: this story adds no Rust and edits none.

`bunx tsc --noEmit`, filtered to this story's paths — clean.

### Mutation sweep

Harness `~/.W1RawRendered/mutate.py` (never `/tmp`). Baseline green at exactly
the verdict's scope — the five test files below — **before and after** the
sweep. Every mutation applied to a pristine copy, one at a time, and restored.

Scope: `view-mode.test.ts`, `json-structure.test.ts`, `markdown-preview.test.ts`,
`raw-rendered-view.test.tsx`, `text-file-viewer.test.tsx`.

**29 mutations, 29 caught, 0 survived, 0 unproved.**

| # | mutation | caught by |
| --- | --- | --- |
| M1 | default view becomes `raw` | `viewModeFor` default; the tab selected on first open |
| M2 | writing one format's choice forgets the others | `viewModeCookie keeps every other format's choice` |
| M3 | a jar entry that is not one of the two views is adopted | `readViewModes drops what it cannot read` |
| M4 | a number is shown as a double | `keeps a number's own characters`; the structure pane's `12345678901234567890` |
| M5 | a repeated key is drawn but not marked | `draws both halves of a repeated key`; `marks a repeated key` |
| M6 | the reported column is 0-based | the three line/column assertions |
| M7 | two concatenated documents accepted as one | `names text after the value` |
| M8 | one bad JSONL line withholds the rest | `keeps the good lines when one line is truncated` |
| M9 | an empty file reported as a parse error | the four emptiness cases; the component's "this file is empty" |
| M10 | a capped document reports the drawn count as the total | `caps the rows and still reports the true total` |
| M11 | the mermaid guard removed | `declines a mermaid document by name and line` (throws) |
| M12 | the guard always names line 1 | `mermaidFenceLine finds the fence`; `failureLine` is 3 |
| M13 | the markdown preview becomes editable | `is read-only, because editing markdown is the note editor` |
| M14 | the toggle does not write the cookie | `keeps a format's chosen view across files`; the real-cookie test |
| M15 | a refusal rewrites the remembered preference | `cookie.read()` is `""` after a malformed JSON and after a mermaid file |
| M16 | a refusal is shown but the failed view stays | the source editor is present alongside every alert |
| M17 | one bad JSONL line falls back to source | `keeps a JSONL file's good records` |
| M18 | the host re-reads even when Rust refused | `does not tell the host to re-read when Rust refused` |
| M19 | a format change keeps the previous view | `adopts the new format's remembered view` |
| M20 | a CSV with no coordinates renders nothing | `says a CSV outside a notes vault opens as source` |
| M21 | `sizeLabel` never reaches the editor | `passes the registry's language and Rust's size label down untouched` |
| M22 | the truncation notice removed | `says how much of a very large document it is not drawing` |
| M23 | the read-only reason removed | `says why writing is refused` |
| M24 | a refusal outlives the bytes it was about | `previews again the moment the source is edited` |
| M25 | a binary file gets an editable pane | `refuses bytes that are not text` |
| M26 | a non-writable format offered as editable | `refuses a format keeper must not rewrite, by name` |
| M27 | an editor drawn over an unread file | `says it is opening rather than flashing an empty editor` |
| M28 | Rust's save refusal swallowed | `puts Rust's refusal of a save where the reader is looking` |
| M29 | the bare name read instead of the listing's path | `resolves a .json file`'s `syncReadText` argument assertion |

M24, M26 and M28 **survived the first sweep** and are reported as found rather
than rounded up. Each was a real gap:

- M24 — nothing edited a *markdown* file out of its refused state, only a JSON
  one. Added `previews again the moment the source is edited into something it
  can draw`.
- M26 — the guard's input cannot be produced by the current registry table. The
  branch also carried a second, genuinely unreachable case (`profileId === null`,
  which the loading hook short-circuits first); that dead branch was **deleted**
  rather than tested, and the reachable-by-design half is now tested with a
  hand-built row.
- M28 — no test made a save fail. Added `puts Rust's refusal of a save where the
  reader is looking`, which also pins that the buffer is not rolled back.

## Files I touched that I do not own

- `src/lib/viewers/components.tsx` — one binding line. Cleared with W1Registry.
- `src/lib/viewers/components.test.tsx` — the host-parity test asserted on
  `UNKNOWN_VIEWER_TESTID`, which only held while `text` was unbound. Rewritten to
  compare the two hosts' markup, which is the claim the describe block makes and
  which keeps meaning something as wave 2 binds the remaining ids. Cleared with
  W1Registry, who re-ran their own 13-mutation sweep against it: all still caught.
- `src/test/layout.ts` — added `withRangeRects` and `TEST_LINE_PX`. Additive.
