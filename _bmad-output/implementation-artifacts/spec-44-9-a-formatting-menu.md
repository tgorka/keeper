---
title: 'Story 44.9: A Formatting Menu'
type: 'feature'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: 'f782acc'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-44-the-vocabulary-is-the-space.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-1-tab-belongs-to-the-editor.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-7-the-attachment-panel.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-9-the-slash-menu-can-open.md'
---

<intent-contract>

## Intent

**Problem:** a note can only be typed. The owner knows markdown and says plainly they would rather
not have to write it — there is no bold, no heading, no list, no quote, no link, and no table
builder. Epic 43's `/` menu inserts one literal at the start of an empty line; it cannot act on a
selection and it has no idea what a selection already is.

The failure to avoid while fixing that is not "no toolbar". It is a toolbar that half works:
a button that adds a second `**` to text that is already bold, a button that steals the caret so the
next keystroke goes nowhere, and a table whose pipes do not line up in a vault that is also read in
Obsidian and in `git diff`.

**Approach:** every action is a CodeMirror `Command` over the current selection, and every one of
them toggles. Commands, not string surgery on `doc.toString()`, because a command runs inside a
transaction — which is what folds it into one undo step, keeps the caret, handles multiple selection
ranges through the same `changeByRange` the rest of the editor uses, and gets the edit reported to
Rust by the update listener exactly the way a typed character is. A buffer rewrite pushed through
`applyExternal` would be annotated `remote`: the note would change on screen and never reach the file.

Whether a mark is already there is decided by the markdown parser that is already in the editor's
extension list, not by looking at the characters either side of the selection. And there is one
table builder, called by both the toolbar and `/`.

## Boundaries & Constraints

**Always:**
- Every action toggles. Applying bold to bold text removes it, from a selection, from a selection
  that includes the delimiters, and from a bare caret inside the span.
- The toolbar does not move focus. Every control that is not a text field cancels its own
  `mousedown`, and each command hands focus back to the view.
- The table is aligned GFM: padded cells, a delimiter row wide enough to match.
- One table command. `/`'s Table row calls the same builder the toolbar's form does.
- No new dependencies. The two popovers are hand-rolled, for the reason in Design Notes.
- No `@codemirror/*` value may leave the note editor's boot closure. The toolbar is in the main
  bundle; it speaks `FormatAction`, which is plain data.

**Block If:**
- Nothing.

**Never:**
- No change to `recording-embed.ts`, `recording-transport.ts` or `live-preview.ts`.
- No keybindings. The epic asks for a menu; adding `Mod-b` is a second surface with its own
  conflicts against 43.1's keymap and the platform, and it is not what was asked for.

## I/O & Edge-Case Matrix

Every row is asserted by the document text produced from a named selection.

### Inline marks (bold `**`, italic `*`, strikethrough `~~`, code `` ` ``)

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Wrap | `a word here`, `word` selected, bold | `a **word** here`, `word` still selected | none |
| Toggle off | press bold again | `a word here` | none |
| Delimiters inside the selection | `**word**` selected, bold | `word`, selection `word` | none |
| Caret inside the span | caret in `**w│ord**`, bold | `word` | none |
| Nothing selected | caret after `a `, bold | `a ****`, caret between the pairs | none |
| Bold is not two italics | `**word**`, `word` selected, italic | `a ***word*** here` — never `*word*` | none |
| Inner pair | `***word***`, `word` selected, bold | `*word*` | none |
| Outer pair | `***word***`, `word` selected, italic | `**word**` | none |

### Link

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Wrap | `the docs` selected | `see [the docs](url) today`, `url` selected to type over | none |
| Unwrap | caret/selection inside `[the docs](https://x.test)` | `see the docs today`, `the docs` selected | none |
| Round trip | wrap then press again | the original sentence | none |

### Block actions (bullet, numbered, quote, heading 1–6)

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Multi-line bullet | three lines selected | `- ` on each; second press removes all three | none |
| Multi-line numbered | three lines selected | `1. `, `2. `, `3. ` in document order | none |
| Multi-line quote | three lines selected | `> ` on each; second press removes all three | none |
| Multi-line heading | three lines selected, H2 | `## ` on each; second press clears them | none |
| Level change | two `## ` lines, H4 | `#### ` on both — a change, not a clear | none |
| Mixed selection | `- alpha` + `beta`, bullet | both bulleted; never "remove the one bullet" | none |
| Marker swap | two bullets, numbered | `1. ` / `2. `, not `1. - ` | none |
| Indent survives | `  - alpha`, numbered | `  1. alpha` | none |
| Quote a list | `- alpha`, quote | `> - alpha` — never `> alpha` | none |
| List inside a quote | `> alpha`, bullet | `> - alpha` | none |
| Blank line between paragraphs | `alpha\n\ngamma` selected, quote | `> alpha\n\n> gamma` | none |
| Only blank lines | empty document, bullet | `- `, caret after the marker | none |
| Selection survives | two lines selected, quote | the selection still covers both, markers included | none |
| Partial selection survives | `beta` inside `alpha beta`, quote | `> alpha beta`, `beta` still selected | none |

### Table builder

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| 3×2 with a header | rows 3, columns 2, header on | header `Column 1`/`Column 2`, delimiter row, **2** body rows | none |
| 3×2 without one | rows 3, columns 2, header off | an **empty** header row, delimiter row, **3** body rows | none |
| Alignment | any shape | every row's pipes at identical character offsets | none |
| It is a table | 3×2 output | the editor's own GFM parser reports `Table`, `TableHeader`, two `TableRow`s | none |
| Caret | after insertion | in the first cell, so the first column can be named | none |
| Mid-sentence | caret after `notes: ` | a newline first — a table owns its lines | none |
| Degenerate input | rows 0, columns 0 | one column, one row; never an empty or malformed table | none |
| `/` agrees | `/tab`, accept | the same builder's output, two columns and one body row | none |

### The toolbar itself (asserted against the real mounted `NoteEditor`)

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Bold from the toolbar | `beta` selected, click Bold | store text `alpha\n**beta**\n` | none |
| Toggle off from the toolbar | click Bold again | store text back to `alpha\nbeta\n` | none |
| Caret theft | mousedown on any non-field control | cancelled (`defaultPrevented`) | none |
| Focus returns | focus moved off the editor, click Italic | the selection is still `beta` and the view has focus again | none |
| Heading panel | click Heading, then Heading 3 | `### alpha`, and the panel closes | none |
| Table form | rows 3, columns 2, Insert | the exact aligned table, appended on its own lines | none |
| Headerless table form | uncheck the header, Insert | the empty-header form of the same table | none |
| Undo | click Strikethrough, then `undo` | one step back to the original document | none |

</intent-contract>

## Code Map

- `src/components/notes/editor/format-commands.ts` — **new.** Every command, plus `gfmTable` and the
  `FormatAction` type. Lives in the editor's lazy chunk with the rest of the `@codemirror/*` code.
- `src/components/notes/format-toolbar.tsx` — **new.** Ten controls, a heading-level panel and a
  table form. Holds no editor: it emits `FormatAction`, and its one import from `format-commands` is
  `import type`, which is erased.
- `src/components/notes/note-editor.tsx` — `EditorRuntime` gains `runFormat`; the boot closure adds
  `./editor/format-commands` to its `Promise.all` and turns an action into a command there; the
  toolbar is seated above the editor host in edit mode only.
- `src/components/notes/editor/slash-menu.ts` — `TABLE_SKELETON` is now `gfmTable({ rows: 2,
  columns: 2, header: true })` instead of a hand-written string.
- `src/components/notes/editor/slash-menu.test.ts` — the table expectation updated to the aligned
  output, with the reason in the test. See "What changed in `/`", below.
- `src/components/notes/editor/format-commands.test.ts` — **new.** 41 tests over a real
  `EditorView` carrying the product's markdown extension.
- `src/components/notes/format-toolbar.test.tsx` — **new.** 10 tests that mount the real
  `NoteEditor` and click the real buttons.

## What changed in `/`, and why it is deliberate

43.9's spec pins the `/` menu's inserted text as byte-identical to what shipped. This story breaks
that pin for one row, on purpose, and this is the paragraph that says so out loud.

| | Inserted by `/tab` |
|---|---|
| Before (43.9) | `\| Column \| Column \|`<br>`\| --- \| --- \|`<br>`\|  \|  \|` |
| After (44.9) | `\| Column 1 \| Column 2 \|`<br>`\| -------- \| -------- \|`<br>`\|          \|          \|` |

Two changes: the pipes now line up, and the two columns are told apart. The alternative was two table
commands in one editor with two different outputs, which is the kind of divergence nobody notices
until a diff is unreadable and half the vault's tables are the other shape. The trigger grammar,
the row's label, its detail (`two columns, one row` — still one body row) and its position in the
table are all untouched; the `slash-menu.test.ts` assertion was updated with the reason written
beside it rather than quietly relaxed.

## Tasks & Acceptance

**Execution:**
- [x] Inline marks as toggling commands, decided by the markdown parser.
- [x] Link, wrap and unwrap.
- [x] Bullet, numbered, quote and heading 1–6 over every line the selection touches.
- [x] An aligned GFM table builder, shared with `/`.
- [x] A toolbar that cannot take the caret.
- [x] Tests asserted by document text; every one proven by reverting.

**Acceptance Criteria:**
- Each action applied to a named selection produces the exact document text asserted — 41 command
  tests and 10 through the mounted editor.
- Each mark round-trips: applied twice, the document is the one it started as.
- The table output parses as a GFM table, asserted by the parser the editor itself runs.
- The toolbar does not steal the caret: every non-field control cancels its `mousedown`, and after a
  click with focus deliberately parked elsewhere the selection is unchanged and the view has focus.

**Revert proof.** Each mutation applied to the shipped source, the affected suites run, the source
restored. 51 tests pass unmutated; every mutation was caught.

| # | Mutation | Failed | Which tests caught it |
|---|---|---|---|
| M1 | Toolbar buttons stop cancelling `mousedown` | 1 | "does not let a button take focus off the text it is formatting" |
| M2 | Inline toggle stops calling `view.focus()` | 1 | "leaves the selection on the same words and the caret back in the note" |
| M3 | Mark detection reverts to looking at the neighbouring characters | 9 | all eight "delimiters inside the selection" / "caret sitting inside it" cases, **and** "does not mistake one half of a bold run for an italic delimiter" |
| M4 | Block action toggles off when *any* line has the marker | 1 | "only toggles off when every selected line already has the marker" |
| M5 | Block action rewrites the whole line instead of splicing the marker | 3 | "keeps a bullet's indent", "bullets inside a quote", "keeps a partial selection inside the line it started in" |
| M6 | The line prefix becomes one token instead of quote + marker | 2 | "quotes every selected line", "bullets inside a quote rather than replacing it" |
| M7 | Table columns stop being padded | 6 | five table tests **and** `slash-menu.test.ts`'s insertion test |
| M8 | Headerless table omits the header and delimiter rows | 4 | both headerless tests, the mid-sentence test, the degenerate-input test |
| M9 | The toolbar is never seated in `note-editor.tsx` | 10 | every test in `format-toolbar.test.tsx` |
| M10 | A table no longer starts its own line | 1 | "starts the table on its own line when the caret is mid-sentence" |
| M11 | Block selection mapping goes back to CodeMirror's inward bias | 2 | "acts on the empty line itself", "keeps the selection over the same lines" |
| M12 | Ordered list numbers every line `1.` | 3 | "numbers the selected lines from one", "swaps one list marker for the other", "keeps a bullet's indent" |

M3 is the row worth reading. The naive implementation — *is the character before me a `*`?* —
**passes the plain wrap-then-unwrap round trip for all four marks**, because after wrapping, the
delimiters really are immediately outside the selection. It only breaks when the user selects the
delimiters too, when they put a bare caret inside the span, and when they press italic on bold text
and get `*word*` instead of `***word***`. If this suite had asserted the toggle round trip and
stopped, the shipped toolbar would silently downgrade bold to italic and nothing would have said so.
Those nine cases exist for that.

M9 is the ledger's recurring lesson answered directly: 41 correct command tests say nothing about
whether any button is wired to any of them. `format-toolbar.test.tsx` mounts the real editor, finds
the buttons by the label a user reads, and reads the document back out of the notes store — the
buffer that would be written to disk.

## Design Notes

**Why the syntax tree decides whether a mark is present.** The obvious implementation inspects the
characters either side of the selection, and it is wrong in the case people hit first. With the caret
inside `**bold**`, *"is the character before me a `*`?"* is true for italic as well, so italic eats
one star from each side and quietly turns bold into italic. The markdown parser is already in the
editor's extension list and has already decided which run of stars is `Emphasis` and which is
`StrongEmphasis`, so this module asks it. That is also what makes nesting work for free: inside
`***both***` the parser reports an `Emphasis` wrapping a `StrongEmphasis`, so bold removes the inner
pair and italic the outer one, and neither needs to know the other exists.

**Why the block actions splice the marker and never rewrite the line.** Replacing a whole line
collapses any position inside it to the line's edge, so the user's selection would vanish on every
block action, and an existing indent or an enclosing quote marker would be re-derived (badly) on
every press. Splicing only the marker region also gives the two-group prefix its point: `> - a` is
both a quote and a bullet, and a toolbar that treated the prefix as one token would answer "quote
this" by deleting the list.

**Why block selections are mapped outward.** CodeMirror's `SelectionRange.map` biases inward, which
for a marker written at the line's start means the selection slides past the two characters that were
just added — so the block the user selected is no longer the block their selection covers, and the
second press acts on something else. Ranges are therefore mapped with the outward bias, except empty
ones: a caret must stay a caret and land *after* the marker, because the next thing that user does is
type the item.

**Why "no header" still writes a header row.** GFM has no table without a delimiter row, and the row
above the delimiter *is* the header whether or not anyone asked for one. So the checkbox cannot mean
"omit the header row"; it means "leave it empty". `rows` then counts what the user counts — with a
header, the header is the first of them; without, all of them are rows to type in. Both readings are
asserted.

**Why the table is padded.** This vault is read in Obsidian and in `git diff`. A table whose pipes do
not line up in the source is a table nobody edits by hand afterwards — they retype it, or they leave
it wrong. Padding costs a few spaces per row and every renderer ignores them.

**Why the two popovers are hand-rolled.** They need `mousedown` cancelled on exactly the controls
that must not take focus and honoured on the two number fields that must, which is the one thing a
menu primitive does not let you say — and the repo takes no new dependency for this.

**Why the toolbar holds no editor.** `note-editor.tsx` keeps every `@codemirror/*` value inside its
boot closure so the main bundle never pulls the editor chunk in to paint a pane. The toolbar is in
that main bundle, so the two sides meet over `FormatAction` — plain data — and the closure that owns
the view is the only place that turns an action into a command.

## What I could not prove here

**That a real browser's focus does not move on mousedown.** jsdom does not implement focus-on-
mousedown at all, so no test in this repo can observe the theft the `preventDefault` prevents. What
is asserted instead is the mechanism and its consequence: the mousedown *is* cancelled
(`fireEvent` reports `defaultPrevented`), and with focus deliberately parked on another element
before the click, the command still applies to the editor's selection and hands focus back. The
remaining gap — that a real WebKit build would also have moved focus without the cancel — is
argued, not measured.

**That the parser is fully parsed for a very large note.** `syntaxTree` returns the tree parsed so
far. For every document in these tests it is complete, and in the product the selection is by
definition in the viewport, which the language extension parses first. A pathological case where the
selection sits in an unparsed region would fall back to *wrap* rather than toggle off — a wrong
toggle, never a corrupted document. Not reproduced.

## Deliberately not done

- **Keybindings.** The epic asks for a menu. `Mod-b` and friends are a second surface with their own
  conflicts against 43.1's keymap and the platform's own bindings, and every command here is already
  shaped to be bound later without being rewritten.
- **Active-state highlighting on the buttons.** Showing "the selection is bold" means recomputing
  from the syntax tree on every selection change and pushing it into React, which is a per-keystroke
  cost for a hint the toggle already gives on press. It is a real improvement and it is a separate
  decision.
- **Column alignment markers (`:---`, `:---:`).** The builder writes `---`. Per-column alignment is a
  fourth question on the form for something the user can type in three characters.
- **Editing an existing table.** Adding a row or a column to a table already in the note is a
  different feature — it needs the table under the caret, not the selection — and the epic asks for
  a builder.
- **A separate "paragraph" / "clear formatting" action.** Heading toggles to plain, the list buttons
  swap and clear, and quote unquotes; a clear-everything button would need a definition of
  "everything" that markdown does not supply.
- **Task list, code fence and mermaid buttons.** `/` already inserts all three and does it well. A
  second door to the same literal is clutter, not a feature.
