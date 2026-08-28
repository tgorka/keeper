# Spec 45.9 — A table you can edit

status: implemented
story: Epic 45, Wave 2, Story 45.9
binds: FR-183, UX-DR72, AD-88, DW-165, DW-172

## The one sentence

A GFM table in a note renders as a table like every other block, its columns and rows
are added and removed from controls on the block itself, and putting the caret in it
brings the pipes back and keeps them lined up on every keystroke.

## What was found already present

Grepped before building, per this epic family's recurring lesson. What was found:

- **`gfmTable` in `format-commands.ts` (44.9)** — the aligned builder, the `/` menu's
  table, and the padding rule. This story does **not** write a second aligner. The
  padding was lifted out of `gfmTable` into an exported `alignedTable`, and `gfmTable`
  is now one caller of it. 44.9's 41 tests pass unedited, which is the proof that the
  lift is byte-identical.
- **`galleryLayer` in `gallery-block.ts` (44.15)** — the `StateField` shape a
  multi-line block decoration needs, and the reason (DW-165). Copied in shape, not in
  code: same field, same doc-driven scan, same selection-driven reveal.
- **44.16's CSV embed widget** (`CsvTableWidget` as this story was written; Story 45.12
  is generalising it into one embed widget in the same wave) — the `ignoreEvent` trade a
  widget with controls has to make, and the reason a press on a control must not reach
  CodeMirror. Referenced by behaviour, not imported.
- **`spliceBetween` in `live-preview.ts` (37.6)** — the minimal replacement between two
  strings. Imported rather than reimplemented; see "Why the realign is line by line".
- **Nothing at all** for parsing a table out of the document, for splitting a row on
  unescaped pipes, for alignment markers, or for any row/column operation. There was no
  dead field to find here: 44.9 says in as many words that editing an existing table
  was out of its scope, and nothing since has added one.

## Where the decisions live

| Decision | Home | Note |
| --- | --- | --- |
| How wide is each column, and how is a row padded? | `format-commands.ts::alignedTable` | **the one aligner**, shared with `/` |
| What do the delimiter row's colons mean? | `format-commands.ts::delimiterCell` + `markdown-table.ts::tableAligns` | written and read beside each other |
| Where are the tables in this document? | `markdown-table.ts::tableHits` | line-driven, like `galleryHits` |
| Which characters are one cell? | `markdown-table.ts::splitTableRow` | an escape scanner, not `split("\|")` |
| Is this table rendered or shown as source? | `markdown-table.ts::tableDecorations` | the selection decides, as for every block |
| Is this structural edit allowed? | `markdown-table.ts::tableRefusal` | one place, read by the button and the command |
| When does the source get realigned? | `markdown-table.ts::realignTables` | a `transactionFilter`, `sequential: true` |

No Rust. Nothing in this story is a fact about the machine: a table is text in a note,
and the note's bytes already have one writer.

## The three jobs, and why they are separate

The epic's sentence has three clauses and they are three different mechanisms.

**Render.** A `StateField` replaces the block with a `<table>`. A field and not the
renderer's `ViewPlugin`, because a table is several lines replaced by one element and
CodeMirror refuses a block decoration from a plugin — that is DW-165, verbatim, and
`galleryLayer` is the in-repo precedent for the shape that works.

**Edit the structure.** Add and remove a column, add and remove a row, from four
controls on the rendered block. These are the operations that cannot be expressed by
typing: adding a column means editing the header, the delimiter row and every body row
in one step, and doing that by hand is exactly the chore that stops people using
tables. Every one of them rewrites the whole block through `alignedTable`, so none of
them can leave the delimiter row behind.

**Edit the text.** Put the caret in the table and the pipes come back — the same reveal
rule the gallery, the CSV embed, the image and the wikilink already follow — and you
type markdown, realigned on every keystroke.

### Why there is no cell editor floating over the rendered table

It is the obvious design and it is the wrong one here. A text input inside a widget has
to own a caret, and CodeMirror owns the caret in its own `contentEditable`: the moment
the widget writes through to the document, CodeMirror re-syncs the DOM selection to its
own — which is not in the input — and takes the focus back mid-word. Every way out of
that is a controlled-input reimplementation that has to survive a decoration rebuild per
keystroke.

The CSV embed widget gets away with a cell input because its commit goes to a **file**
over IPC and never dispatches a transaction. A markdown table is in the buffer, so it
cannot.

And the reveal design is strictly better for what was actually asked. "The source stays
legible markdown at every keystroke" is a claim about the buffer, and the buffer is what
the user is typing into. Obsidian reads the same file; a half-written table is what sync
carries if the app closes mid-edit; and in this design there is no second model of the
table that could be mid-flight when that happens.

## The one aligner, and the escaped pipe

`alignedTable(rows, aligns)` is 44.9's padding, lifted. `gfmTable(shape)` now builds its
cells and calls it. Byte-identical for every shape 44.9 asserts, because the widths it
computed from the header row are the same widths computed over all rows when the body
rows are empty.

A second aligner would disagree with this one about exactly one thing, and it is the
thing this story keeps tripping over: **`a \| b` is one cell.** Split a row on every
pipe and it becomes two, the header and the delimiter row stop matching cell for cell,
GFM stops calling the block a table — and a realign written by the naive reader would
have rewritten the user's one cell into two on the way past. So `splitTableRow` is a
scanner that copies an escape through with its backslash intact, and the source keeps
every byte it was written with. Only the rendered table unescapes, in `tableCellText`,
for display.

That is also why the model never holds an unescaped cell: unescaping on the way in and
re-escaping on the way out is a round trip that has to guess which of the user's
backslashes were escapes.

## Why the realign is line by line

The realign appends its changes to the user's own transaction with `sequential: true`
rather than dispatching a second one. Three consequences, all of them the point:

- the document is never once observable in an unaligned state, which matters because
  sync carries whatever is in the buffer when the app dies;
- the keystroke and its padding undo as **one** step;
- the note is reported to Rust once per character instead of twice.

Within that, the changes are computed **per line** and minimally within each line, via
`spliceBetween`. A whole-block replacement would collapse the caret to the block's edge
on every keystroke, and so would a single minimal splice over the whole block: typing in
a cell widens that column in *every* row, so the first difference is in a row **above**
the one being typed in, and the splice that starts there swallows the caret. Per line,
the only change on the caret's own line is padding *after* the caret, which the caret
survives — which is what "keep the source aligned while you type" has to mean if it is
to be usable.

Two transactions are left alone. A **remote** change came from another editor that has
already aligned it its own way, and re-padding it here would send a change straight back
for that editor to re-pad in turn. **Undo and redo** are left alone for the plainer
reason: a realign appended to an undo would stop undo restoring what was there.

## I/O and edge-case matrix

### Reading the source (`splitTableRow`, `tableAligns`, `tableHits`)

| Scenario | Input | Expected | Error |
|---|---|---|---|
| Escaped pipe | `\| a \\\| b \| c \|` | two cells, `a \\\| b` and `c` | none |
| Empty cell | `\| a \|  \| c \|` | three cells, the middle one `""` | none |
| No fence pipes | `a \| b` | two cells | none |
| Alignment colons | `\|:--\|:-:\|--:\|---\|` | `left, center, right, none` | none |
| Prose with a pipe | `grep a \| wc -l` | no table — a leading pipe is required | none |
| Table inside a fence | ```` ```md ```` … | no table: the reader asked for the pipes | none |
| Header wider than the delimiter row | 3 header cells, 2 delimiter cells | **not a table** — GFM's own rule, and what makes a half-typed table safe | none |
| Header line with no row under it | `\| a \| b \|` alone | no table | none |
| Indented table | `  \| a \| b \|` … | a table; the header line's indent is used for every rewritten line | none |

### The aligner (`alignedTable`)

| Scenario | Input | Expected | Error |
|---|---|---|---|
| Agreement with `/` | `gfmTable({rows:2,columns:2,header:true})` | identical to `alignedTable([["Column 1","Column 2"],["",""]])` | none |
| Alignment preserved | cells + `["left","center","right"]` | `\| :-- \| :-: \| --: \|` | none |
| Short body row | header 2, row 1 | the row filled out to 2 cells | none |
| Long body row | header 2, row 3 | **all three kept**; header and delimiter stay at 2 | none |
| Escaped pipe | aligned table containing `a \\\| b` | realign is the identity, byte for byte | none |
| Narrowest column | any empty column | three dashes — GFM needs one, three is what every tool writes | none |

### The rendered block

| Scenario | State | Expected | Error |
|---|---|---|---|
| A table renders | caret outside | `<table>` with `<th>`/`<td>`; the pipes are not in the DOM text | none |
| Escaped pipe on screen | source `a \\\| b` | the cell reads `a \| b`; the file still holds the backslash | none |
| Caret inside | selection touches the block | the source comes back, the widget is gone | none |
| Caret at offset 0 | a note that opens with a table | source — offset 0 *is* inside the first cell | none |
| Ragged body row | 3 cells against a 2-cell header | row marked, two cells drawn (GFM draws two), the third still in the file | none |
| Fenced table | inside ```` ``` ```` | source, untouched | none |
| Alignment on screen | `:-:` | the column's cells are centred | none |

### The structural edits

| Scenario | Press | Expected | Error |
|---|---|---|---|
| Add column | Add column | every row gains a cell **including the delimiter row**; all pipes at identical offsets | none |
| Remove column | Remove column | the last column goes from every row | none |
| **Remove the last column** | Remove column at 1 column | **refused.** The control is disabled and its title says to select the table and delete it as text | refusal, not a silence |
| Add row | Add row | one empty body row at the header's width | none |
| Remove row | Remove row | the last body row goes | none |
| **Remove the header row** | Remove row with no body rows | **refused**, same shape | refusal |
| Caret theft | mousedown on any control | cancelled (`defaultPrevented`) | none |
| Stale widget | the note changed under the button | the block is re-found by its own text; if it is gone, nothing happens | returns `false` |

### Typing

| Scenario | Action | Expected | Error |
|---|---|---|---|
| Every intermediate state | nine characters into one cell | after **each** one: the GFM parser reports a table, `tableHits` finds exactly one, every row's pipes at identical offsets | none |
| Caret | type `x` after `c` | caret one character further on, still in `\| cx` | none |
| Column that grew | `charlequin` into a 3-wide column | that column widens; the others stay at the floor | none |
| Becoming a table | `\|a\|b\|` + `\|-\|-` then `\|` | untouched until the delimiter row matches, then aligned in the same keystroke | none |
| Undo | type, then undo | **one** step back to the pre-keystroke text | none |
| One transaction | type one character | the update carries exactly **one** transaction, and its document is already aligned | none |
| Remote change | an unaligned row annotated `remote` | left exactly as it arrived | none |
| A table nothing touched | edit a paragraph below | the table above is not reformatted | none |

## Decisions stated out loud

**Removing the last column is refused, not "removes the table".** The acceptance
criterion offered either. Deleting the block would be a destructive act that eats text
the user can see, triggered by a button labelled "Remove column", with nothing on screen
saying the whole table went — and this epic's rule (AD-89, and the epic preamble) is
that a destructive act is confirmed and names what it destroys. A table that is genuinely
unwanted is three lines of text: selecting them and pressing delete already works, is
visible, and undoes in one step. Removing the header row stops for the same reason, with
GFM's rule behind it as well: there is no table without a header row and a delimiter row.

**A leading pipe is required.** GFM allows `a | b` with no fence pipes. Accepting that
form would make every prose sentence containing ` | ` a table candidate, and it is not
what keeper, Obsidian, or 44.9's builder writes. A pipe-less table stays legible source.

**A long body row keeps its extra cells; a short one is filled out.** GFM ignores the
excess and inserts the missing, so both are semantically identical to what the user
wrote — but dropping a cell would delete text they can see in their own file, and
filling one in makes the source legible. Removing a column truncates every row to the
new width, including a ragged row's extras: the header defines the shape.

**The block's indent is the header line's.** A table whose lines start at different
columns has no aligned form, and picking the first line is the only choice that does not
depend on which line the user happened to edit last.

**No keybindings, no toolbar row, no `/` entry for the row and column operations.**
44.9's reason still holds — a keybinding is a second surface with its own conflicts
against 43.1's keymap — and the toolbar and `/` menu are Story 45.10's files this wave.
The operations are not dead: the four controls on the rendered block are their call
sites, and `applyTableOp` is already the shape a command would wrap.

## Tasks and acceptance

**Execution:**
- [x] `alignedTable` lifted out of `gfmTable`; one aligner, byte-identical output.
- [x] Alignment markers (`:---`, `:---:`, `---:`) read and carried through a realign.
- [x] `tableHits`: a line-driven scan that respects fences and GFM's header/delimiter rule.
- [x] `splitTableRow`: an escape scanner, so `a \| b` is one cell.
- [x] The rendered block, as a `StateField` block decoration with the selection reveal.
- [x] Four structural controls, with the two refusals stated on the disabled control.
- [x] Realign appended to the typist's own transaction, line by line.
- [x] 37 tests, every view test through a real `EditorView` with `withRangeRects`.

**Acceptance criteria:**
- A table renders as a table — asserted through the layer directly **and** through the
  product's own `livePreview`, which is the only assertion that says the layer is
  mounted at all (DW-172).
- Adding a column widens every row including the delimiter row, with every row's pipes
  at identical character offsets.
- Removing the last column is refused, and the control says so.
- A cell containing an escaped pipe survives a realign byte-identically.
- Typing in a cell keeps the source a parseable GFM table after **each** keystroke,
  asserted per character and not only at the end.

## Revert proof

17 mutations, each applied to the shipped source, this story's four suites run, the
source restored by inverse string replacement — never a checksum restore, because a
sibling was editing two of these three files at the same time — and every anchor
re-grepped by name afterwards. Baseline green before and after at the verdict's exact
scope: `markdown-table.test.ts` (37), `format-commands.test.ts`, `live-preview.test.ts`
and `slash-menu.test.ts`. **Every mutation was caught.**

| # | Mutation | Failed | Which tests caught it |
|---|---|---|---|
| M1 | `splitTableRow` splits on every pipe, escape or not | 3 | "keeps an escaped pipe inside the cell it belongs to", "leaves an escaped pipe byte-identical through a realign", "shows a pipe in the cell whose source holds an escaped one" |
| M2 | `alignedTable` stops padding cells | 17 | four of 44.9's builder tests, `slash-menu`'s "inserts a multi-line skeleton whole", and twelve of this story's — the aligner is load-bearing for the render, the structure edits and the typing alike |
| M3 | `alignedTable` ignores `aligns` and always writes dashes | 1 | "carries the alignment colons through a realign" |
| M4 | `tableHits` drops the header/delimiter cell-count rule | 1 | "does not call a half-typed table a table" |
| M5 | `tableHits` stops tracking fences | 2 | "leaves a table inside a fenced block alone", "leaves a table inside a fenced block as source" |
| M6 | The reveal rule is dropped: a table is always rendered | 1 | "puts the pipes back when the caret is in the table" |
| M7 | `realignTables` never appends changes | 2 | "keeps the source a parseable table after every single keystroke", "aligns a table the moment the typed delimiter row makes it one" |
| M8 | `sequential: true` becomes `false` | 1 | "keeps the source a parseable table after every single keystroke" — the padding is then placed against the pre-keystroke document and the table is corrupted, which the per-character parse assertion sees on the first character |
| M9 | `realignChanges` replaces the whole line instead of splicing within it | 2 | "leaves the caret in the cell it was typed into", "keeps the source a parseable table after every single keystroke" |
| M10 | `tableRefusal` always returns null | 3 | "refuses to remove the last column, naming what to do instead", "refuses to remove the header row", "refuses to remove the last column and says why on the control" |
| M11 | The controls stop cancelling their `mousedown` | 1 | "does not let a control take the caret out of the note it is editing" |
| M12 | The delimiter row is emitted at the widest row's width | 1 | "never drops a cell a long body row has" |
| M13 | `realignTables` stops skipping remote transactions | 1 | "does not realign a change that arrived from another editor" |
| M14 | The realign is not scoped to the tables the change reached | 1 | "leaves a table nothing touched alone" |
| M15 | `tableLayer()` is not composed into `livePreview` | 1 | "is mounted by the note editor's own renderer, not only by this test" |
| M16 | The realign becomes a second transaction from an `updateListener` instead of an appended spec | 1 | "makes one transaction of the keystroke and its realign" |
| M17 | The appended spec carries `addToHistory: false` — the plausible "do not pollute history with padding" mistake | 1 | "keeps the keystroke and its realign in one undo step" |

**M1 is the row worth reading.** The naive splitter passes the plain render test, both
column tests, both row tests and every typing test, because none of those cells contains
a pipe. It only breaks on the one cell that does — and it breaks it by *rewriting the
user's file*: one cell becomes two, the header no longer matches the delimiter row, and
the whole block drops out of the renderer on the way past. That is why the escaped pipe
is asserted three ways — in the split, in the source after a realign, and in what the
reader sees on screen.

**M15 is the ledger's recurring lesson answered directly** (DW-172): 36 correct tests
over a layer nobody composed would all have been green.

**M16 and M17 exist because the first sweep found two claims with no defender.** "One
undo step" and "reported to Rust once per character" were asserted in this document and
by nothing in the suite: the undo test survived M7, M8 and M14 untouched, because
CodeMirror's history merges adjacent input transactions within its group delay, so a
realign dispatched a microsecond later still undoes together in a test. M16 rewrites the
filter as an `updateListener` that dispatches separately — the shape a future refactor
would most plausibly reach for — and the transaction-count assertion is the only thing
that sees it. M17 covers the other half: a realign excluded from history undoes to the
wrong text, and that is what the undo test is for.

**One incident, recorded because it is the trap the harness exists to avoid.** M7
replaced a unique line with text that already occurred three times elsewhere in the file
(`return transaction;`), so the inverse replacement refused to run and left the mutant in
the tree for one iteration. It was found by the harness's own uniqueness assertion rather
than by a test, restored by a context-anchored replacement, and both directions of every
subsequent mutation were required to be unique before the sweep resumed. A checksum
restore would have hidden this by refusing outright — or, worse, by clobbering the
sibling edits that were landing in these files throughout.

## What I could not verify here, and why

**That a press on a control does not move the caret in a real browser.** `ignoreEvent`
returning true is what stops CodeMirror handling a press inside the widget — without it
the press would put the caret in the block, the block would reveal its source in the
same frame, and the button would be gone before its own handler ran. jsdom has no
layout, so CodeMirror's own `mousedown` path (`posAtCoords`) cannot be driven there
honestly; a test that dispatched one would be asserting jsdom's zero-height geometry, not
keeper. What **is** asserted is the mechanism and its consequence: the mousedown is
cancelled (`defaultPrevented`), and every control edits the document and hands focus back.
The remaining gap — that a real WebKit build would have moved the caret without
`ignoreEvent` — is argued from the CSV embed widget's identical trade, not measured. Same
class of gap as 44.9's focus-on-mousedown paragraph.

**That the rendered table looks right.** jsdom measures zero and `withRangeRects` is an
explicit monospace fiction, so nothing here may claim a pixel. The alignment assertions
are all on **character offsets in the source**, which is the thing Obsidian and `git
diff` see and the thing the story is actually about. The `text-align` written onto a cell
from a `:-:` column is asserted as an attribute, not as a rendering.

**That two editors open on one note converge.** The remote rule is asserted one way — a
transaction annotated `remote` is not realigned — which is the half this story controls.
Whether the *other* editor's realign and this one's settle to the same bytes is a claim
about two live sessions and there is no harness in this repo that runs two.

**That a very large note stays responsive.** `tableHits` scans the whole document on
every document change, which is `galleryHits`' cost and shape. For the notes this repo
tests with it is a regex per line and nothing measurable; a hundred-thousand-line note
was not tried. If it ever bites, the fix is the same one `galleryLayer` would need — map
the previous hits through the change set instead of rescanning — and it is a change to
one function.

**Anything Rust.** This story has no Rust and touches no IPC, so the shell crate's
inability to build on Linux never came up.

## Deliberately not done

- **A cell editor in the rendered table.** Argued above: it is a caret fight with
  CodeMirror, and the reveal gives the same capability with the buffer as the only model.
- **Per-column and per-row controls** (an `×` on each column header, an insert handle
  between rows). Four controls at the end is the literal read of the epic sentence, and
  a control per column is a hover affordance whose discoverability is a design question,
  not an implementation one.
- **Alignment controls.** The colons are read, carried and rendered. Setting them is a
  fifth control for something the user types in one character, and 44.9 already deferred
  the same decision on the builder side.
- **Column reordering, sorting, or a "format table" command.** None was asked for, and
  each is a separate model of what a table is for.
- **Auto-escaping a pipe the user types into a cell.** Obsidian does it; it is a real
  convenience and it is also a keystroke that silently edits itself, which deserves to be
  decided on purpose rather than smuggled in here. Today, typing a bare `|` splits the
  cell — the source stays exactly what was typed, the block stops being a table until it
  matches again, and nothing is lost.
- **Tables in the 45.4 markdown *preview*.** That surface renders through
  `markdown-preview.ts`, which is Story 45.10's file this wave; this story is the note
  editor's live view.
