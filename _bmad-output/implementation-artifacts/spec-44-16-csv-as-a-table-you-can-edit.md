# Spec 44.16 — CSV as a Table You Can Edit

status: implemented
created: 2026-08-09
epic: 44 (the vocabulary is the space, and the note is a document)
binds: FR-172
depends on: 43.5 (attachment kinds), 43.7 (the attachments panel and the one embed syntax)
related: DW-165 (filed by this story — a mermaid fence crashes the editor)

## What this story settles

A `.csv` in the vault renders as a table inside the note and can be edited there.
The round trip is the story: a file the user has not touched comes back
**byte-identical**, and an edited cell changes that cell and nothing else.

## What was already there, and what was dead

The epic keeps finding that the thing a story is asked to add already exists as a
value nobody applied. Here it did not, and the search is recorded so nobody
repeats it:

* **No CSV code anywhere.** A case-insensitive search for `csv` over `src/`,
  `src-tauri/crates/`, `src-tauri/Cargo.toml` and `package.json` finds exactly two
  hits, both prose: `keeper-sync/src/profile.rs:637` and
  `keeper-sync/src/lfs/stage.rs:80`, each using "the 6 GB `.csv` export" as the
  example of a file you must not turn into a git blob. No parser, no widget, no
  dead field, no half-wired command.
* **No CSV crate, transitively or otherwise.** `grep -n csv src-tauri/Cargo.lock`
  returns nothing — no `csv`, no `csv-core`. So the "check before you ask"
  instruction resolves to: there is nothing to reuse, and the parser is written
  here without adding a dependency.
* **`.csv` already has a kind, and it is `File`.** 43.5's `kind_for_file_name`
  classifies it correctly as something keeper cannot render inline as media. This
  story does not touch that table and adds no second classifier: the one
  predicate it introduces, `isCsvTarget`, is about *this embed*, not about what
  the file IS.

What this story reuses rather than rebuilds: the `![[…]]` embed syntax (there is
one, the attachments panel writes it, Obsidian reads it), `notes_vault`'s atomic
write, `note_protocol::contained_read` for containment, `content_rev` for the
stale-file check, and `notes::bom_len` — which was private to `frontmatter.rs` and
is now shared from `notes/mod.rs`, because two span-recording scanners that
disagree by three bytes about where a file starts is one of them eating Excel's
marker on the first edit.

## The decisions this story was asked to make and justify

### The parser records byte spans; it never re-serialises the file

This is `Frontmatter`'s design applied to a second format for the same reason.
The alternative is what almost every CSV library does:

| Candidate | What it buys | What it costs |
| --- | --- | --- |
| Parse to `Vec<Vec<String>>`, edit, write the whole grid back | Trivial; the writer is ten lines | Every write normalises the file. `"a"` becomes `a` or `a` becomes `"a"`, CRLF becomes LF, a missing final newline grows one, the BOM Excel needs is dropped, a ragged row is padded to the header. All of it in a synced file, all of it in the diff, none of it asked for. |
| Record each field's byte range; a write is a splice over the original bytes | The promise is **structural**: everything outside one field is copied, not re-emitted, so terminators, the BOM and the trailing newline cannot change because nothing ever writes them | The parser is longer, and the odd shapes (junk after a closing quote, an unterminated quote) must be *recorded* rather than rejected |

**Chosen: spans.** `Frontmatter`'s module doc states the same rule as a byte-level
promise and refuses a YAML dependency to keep it. Holding the same line here is
not consistency for its own sake — it is the only way the AC's word
"byte-identically" can be true for inputs nobody thought of.

### An unchanged value is not an edit, and does not get written

`set_cell` compares the new value against the parsed field and returns the source
**unchanged** when they are equal, before encoding anything. This is what makes
byte-identity total rather than dependent on the encoder being perfect: any field
whose bytes the encoder could not reproduce exactly — `"a"x`, a value quoted for
no reason, a shape RFC 4180 does not have — still round-trips, because the write
never happens.

It is also what a person wants: entering a cell, looking at it and leaving writes
nothing, and a save-on-blur cannot reformat a file the user only read.

**The webview deliberately does not copy this rule.** `csv-table.ts` sends the
value even when it looks unchanged, and Rust decides. A short-circuit in the
webview would be a second copy of the comparison, and the copy that never runs is
the copy that rots — which is the failure mode this epic has now found four
times.

### A cell's quoting is the file's convention, not something an edit votes on

A real edit is quoted when the field was already quoted, and also when the value
forces it (a comma, a quote or a line ending would otherwise end the field early
and shift every column after it). Editing `b` inside `"a","b","c"` yields
`"a","x","c"`, not `"a",x,"c"`. Minimal-quoting-always was rejected for the same
reason as the grid rewriter: it changes the file's style in a diff the user did
not ask for.

### A ragged row is shown, and keeper does not add a field to it

A record with more or fewer fields than the first is drawn with the fields it
has. A short row's missing columns are drawn as **absent** (hatched,
`aria-label="no field"`), not as empty editable cells, because an empty cell
invites an edit that would have to invent a delimiter — and `set_cell` refuses
that in a sentence naming the line: *"the row at line 2 has 2 field(s), so it has
no column 3; keeper shows a row like this as it is rather than adding fields to a
row it did not write."* A wide row keeps all its fields and renders wider than the
header, which is what the bytes say.

`width` is the **first** record's field count, not the widest. A file whose header
is the odd one has every other row reported ragged; that is the honest reading,
because keeper has no way to know which record is the mistake.

### A record ends at `\n` or `\r\n`, and a lone `\r` is field content

The same definition `notes::line_bounds` already uses for every other file in the
crate. Adopting old-Mac `\r` terminators would give this module a second opinion
about what a line is, and `line_bounds`'s own doc explains what a one-byte
disagreement costs. Stated, not discovered.

### The delimiter is a comma and is not sniffed

`;`, tab and `|` exist in real exports, and a sniffer that guesses wrong rewrites
the wrong bytes — the one failure this design cannot undo. A `.tsv` is a different
file with a different extension. Not configurable, and said in the module doc
rather than left as an omission.

### Where the write goes, and whether sync notices

`write_vault_file` + `mark_dirty(vault.id)`. No second write path was invented and
nothing reaches into the sync engine.

* `write_vault_file` is the same temp-and-rename `write_note` uses, under the
  `.keeper.<ulid>.tmp` name that is already a tier-0 sync exclusion — so a
  `kill -9` between write and rename leaves no torn CSV in the vault.
* `mark_dirty` is the announcement `import_attachment` already makes for a
  non-note file the user cares about: the commit cadence runs, and the change is
  committed and synced.
* `touch` is **deliberately absent**. It asks the reconciler to re-read a path,
  and the notes walk never collects a `.csv`, so it would be a request for an
  index entry that cannot exist. This is the same split `write_vault_file`'s own
  doc already draws for the default-space ledger.
* The engine's `EchoSuppressor` is engine-internal by design (see `write_note`'s
  doc). This write is *meant* to be seen by the watcher, because a file the user
  edited is a file that must be committed.

### A stale revision is refused, not merged

`notes_csv_set_cell` takes the `rev` the table was read at and compares it against
the file on disk. A file that moved underneath — a sync pull, the user's own
spreadsheet — is refused with a sentence and the table reloads. No conflict copy,
unlike `notes_save`: at that point there is nothing of the user's to lose, only a
stale table.

### What crosses IPC

Down: `(vaultId, target, rev, row, column, value)`. Up: decoded cells, their
coordinates, and Rust-composed sentences. The webview never holds the file's bytes
and cannot spell its quoting — which is precisely why it cannot reformat it.

`target` is passed **verbatim**; the vault root is never joined to a subpath in
TypeScript (AD-65), and Rust resolves exactly two candidates with no search: the
target as a vault-relative path, and — only when it names no directory — the same
name inside `attachments/`. A resolver that walked the vault would make which file
an embed opens depend on what else is in the vault, and an edit would then write
to whichever one it found today.

## I/O matrix — the parser (`keeper_core::notes::csv`)

Every "unchanged" row is asserted as **byte equality over the whole file**, never
`contains`: a dropped BOM, LF for CRLF, an invented final newline and normalised
quoting all leave a file that still contains every cell.

| # | Input | Operation | Output |
| --- | --- | --- | --- |
| 1 | `""` | `parse` | 0 rows, `width` 0 |
| 2 | `""` | `set_cell(0,0,"x")` | `Err(NoSuchRow { row: 1, rows: 0 })` |
| 3 | `"\u{feff}"` (BOM only) | `parse` | 0 rows — the BOM belongs to no field |
| 4 | `"a,b\nc,d\n"` | `parse` | 2 rows; a trailing terminator opens no phantom record |
| 5 | `"a,b\nc,d"` (no final newline) | `set_cell(1,1,"Z")` | `"a,b\nc,Z"` — no newline grown |
| 6 | `"a,b\r\nc,d\r\n"` | `parse` | cells `b`, `d` — no stray `\r`; row 0 span `(0,3)` |
| 7 | `"\u{feff}a,b\n"` | `parse` | cell `a`; row 0 span `(3,6)` |
| 8 | `"\u{feff}a,b\n"` | `set_cell(0,0,"Z")` | `"\u{feff}Z,b\n"` — BOM undisturbed |
| 9 | `"a,\"one, two\",\"line\nbreak\",\"say \"\"hi\"\"\"\n"` | `parse` | `["a", "one, two", "line\nbreak", "say \"hi\""]`, one row |
| 10 | `"a\rb,c\n"` | `parse` | 1 row, cell `a\rb` — a lone `\r` is content |
| 11 | `"a,b,c\n1,2\n3,4,5,6\n7,8,9\n"` | `parse` | 4 rows of 3/2/4/3 fields; `width` 3; `ragged_rows` 2 |
| 12 | `"a,b\n\nc,d\n"` | `parse` | 3 rows; the blank line is a record of one empty field, and ragged |
| 13 | `"a,b\n\"never closes,c\n"` | `parse` | `unterminated_quote` = line 2; that record has **one** field holding the rest of the file |
| 14 | `"\"a\"x,b\n"` (junk after a closing quote) | `parse` | cell `ax`, field recorded quoted |
| 15 | `"\"a\"x,b\n"` | `set_cell(0,0,"ax")` | source **unchanged** — the value matched, so nothing is re-encoded |
| 16 | `"\"a\"x,b\n"` | `set_cell(0,0,"z")` | `"\"z\",b\n"` — a real edit normalises that one field only |
| 17 | every input above | `set_cell(r,c, cell(r,c))` for **every** cell | source unchanged, byte for byte |
| 18 | `"a,b\n"` | `set_cell(0,0,"z")` | `"z,b\n"` — bare stays bare |
| 19 | `"\"a\",b\n"` | `set_cell(0,0,"z")` | `"\"z\",b\n"` — quoted stays quoted |
| 20 | `"a,b\n"` | `set_cell(0,0,"x,y")` | `"\"x,y\",b\n"` — a delimiter forces quotes |
| 21 | `"a,b\n"` | `set_cell(0,0,"x\"y")` | `"\"x\"\"y\",b\n"` — a quote is doubled |
| 22 | `"a,b\n"` | `set_cell(0,0,"x\r\ny")` | `"\"x\r\ny\",b\n"` — a line ending forces quotes |
| 23 | `"a,b\n"` | `set_cell(0,0,"")` | `",b\n"` — emptying is an edit |
| 24 | `"a,b,c\n1,2\n"` | `set_cell(1,2,"x")` | `Err(NoSuchColumn { line: 2, column: 3, have: 2 })` |
| 25 | `"a,b,c\n1,2\n"` | `set_cell(1,1,"x")` | `"a,b,c\n1,x\n"` — the row's own columns still edit |
| 26 | `"a,b\n\"two\nlines\",z\nlast,row\n"` | `parse` | record lines 1, 2, 4 — newlines inside quotes counted |
| 27 | BOM + CRLF + a quoted field holding a comma and a CRLF + no final newline | `set_cell(1,1,"Roe, Richard")` | exactly that cell replaced; prefix and suffix byte-identical |

## I/O matrix — the projection (`csv::project`)

| # | Input | Output |
| --- | --- | --- |
| 28 | `"a,b\n1,2\n"` | `notices` **empty** — a clean file says nothing, so the reader does not learn to ignore notices |
| 29 | `"a,b,c\n1,2\n3,4,5,6\n"` | one notice, "2 of 3 rows do not have 3 fields…"; cells 2 and 4 wide, unpadded |
| 30 | 521 records | `totalRows` 521, `rows` 500, notice "showing the first 500 of 521 rows…"; row indices stay the file's, so `set_cell(510, …)` still edits record 510 |
| 31 | `"a,b\nc,\"open\n"` | a notice naming line 2 |
| 32 | a 9 MB file | `too_large_notice` naming both 9 MB and the 4 MB ceiling |

## I/O matrix — the widget (`csv-table.ts`)

| # | Situation | Behaviour |
| --- | --- | --- |
| 33 | `attachments/people.csv`, `EXPORT.CSV` | recognised; `clip.mov`, `notes/csv`, `csv.md` are not |
| 34 | a clean table | every row drawn; the first record is `<th>` and still editable |
| 35 | a ragged short row | marked, two real cells, one drawn absent — never three blanks |
| 36 | a ragged wide row | keeps its fourth field |
| 37 | read rejects (too large, not UTF-8, missing) | Rust's sentence in `role="alert"` **and** the ordinary wikilink; never an empty box (UX-DR44) |
| 38 | an empty file | "…has no rows", which is an answer rather than a blank |
| 39 | a cell edited and blurred | `setCell(vault, target, rev, row, column, value)`; the table repaints from the answer |
| 40 | a cell entered and left untouched | still sent — whether to write is Rust's decision, in one place |
| 41 | Escape during an edit | the cell reverts; no IPC at all |
| 42 | the write is refused | the reason in `role="alert"`, and the cell shows what is on disk, not the edit that did not land |
| 43 | the widget is destroyed mid-fetch | the resolved table is dropped; nothing is written into DOM CodeMirror has thrown away |
| 44 | the caret moves over the embed | `eq` on (vault, target) lets CodeMirror reuse the DOM, so a half-typed cell survives |
| 45 | a click on a cell | claimed from CodeMirror — letting it through would reveal the line, drop the decorations and destroy the table instead of editing it |
| 46 | a click on the degraded link | given up, so it behaves like the wikilink it is |
| 47 | `![[x.csv]]` in a note | becomes a table; an ordinary `[[x.md]]` on another line stays a link |

## Where a decline is announced

DW-162's rule: a path that can decline to act must say so at INFO or above, and
`tracing::debug!` cannot reach the log at all.

| Decline | User-visible | Log |
| --- | --- | --- |
| File larger than 4 MiB | error, Rust's sentence with both sizes | `warn!` |
| Not UTF-8 | error, naming why keeper will not guess | `warn!` |
| `rev` moved on disk | error, and the table reloads | `warn!` |
| Column a ragged row does not have | error, naming the line | `info!` |
| Value already equal — nothing written | nothing, correctly: nothing changed | `info!` "csv cell unchanged, nothing written" |
| A write that did happen | the repainted table | `info!` "csv cell written" |

`warn!` rather than `info!` for the first three is not decoration:
`debug_log::GatedMakeWriter` writes `INFO` to the file only when debug mode is on,
and lets `WARN` and above through always. A refusal is the thing the user is
asking about later, so it has to already be on disk.

## Tests, and the mutations that prove they bite

Every claim below comes from a **private** harness under `~/.w3csv/`. An earlier
sweep of mine ran from `/tmp/mutate.py`, `/tmp/mutate2.py` and `/tmp/mutate_ts.py`,
which several agents were overwriting concurrently; **those numbers were
discarded and every mutation was re-run**. The harness runs the unmutated suite
as a baseline **before and after** the sweep, byte-compares the restored file,
aborts outright if the opening baseline is not clean, and writes a marker naming
the currently-applied mutant so a killed run can be repaired rather than guessed
at. It reports `DID-NOT-COMPILE` (not evidence either way) and `DID-NOT-FINISH`
(a timeout, which is **not** a pass) as verdicts distinct from `SURVIVED`.

**That machinery earned its keep immediately, and the finding is worth carrying
forward.** The opening baseline of the re-run came back *red* — three failures
with no mutation applied — and the first mutant reported `PATTERN-NOT-FOUND`.
Both symptoms had one cause: a mutant from the clobbered `/tmp` era was still
sitting in the working tree. Somebody's background job had executed my
`/tmp/mutate.py` and been killed while its first mutation was applied, leaving
`let mut at = bom_len(source)` replaced by `let mut at = 0` in `csv.rs` — a real
behaviour change, the byte-order mark becoming part of the first cell's text.
Without the opening baseline the harness would have snapshotted the *mutated*
file as pristine and measured all sixteen verdicts against broken code; the
contaminated run had already begun doing exactly that, reporting M2 as caught by
five tests where the truth is two. The file was repaired, verified at 21/21, and
only then re-snapshotted.

Three rules came out of it, and they generalise past this story: run the
unmutated baseline at **both** ends; treat a missing anchor as an **alarm**
rather than a skip, because it usually means the mutation is already in the file;
and remember that a cancelled or timed-out sweep **leaves its mutant applied**.

### The widget

`bun run vitest run src/components/notes/editor/csv-table.test.ts` — **14 passed**,
baseline verified at both ends, restore verified. **15 mutations, 15 caught.**

| Mutation | Caught by |
| --- | --- |
| W1 pad a ragged row with empty editable cells | "shows a ragged row with the fields it has, marked, and never padded" |
| W2 swallow a read failure and render nothing | "keeps the link and shows the reason when the file cannot be read" |
| W3 short-circuit an unchanged cell in the webview | "sends an unchanged cell too, because whether to write is Rust's decision" |
| W4 Escape commits instead of abandoning | "abandons an edit on Escape without asking Rust to write anything" |
| W5 a refused write says nothing and leaves the failed edit on screen | "shows a refused write and puts the cell back to what is on disk" |
| W6 send a stale revision | "sends an edited cell with the revision it read…" and "sends an unchanged cell too…" |
| W7 keep rendering into destroyed DOM | "drops a table that resolved after the widget was destroyed" |
| W8 give a cell's events back to CodeMirror | "claims a cell's events from CodeMirror and gives up the link's" |
| W9 the ragged mark never reaches the row | "shows a ragged row with the fields it has, marked, and never padded" |
| W10 an empty file renders nothing | "says an empty file has no rows rather than rendering nothing" |
| W11 the embed is not recognised and stays a plain wikilink | "turns a csv embed into a table and leaves an ordinary wikilink alone" |
| W12 `isCsvTarget` becomes case-sensitive | "claims a csv embed whatever case the export used, and nothing else" |
| W13 the header row loses its `<th>` | "draws every row and every cell the backend described" |
| W14 Rust's notices are never rendered | "shows a ragged row with the fields it has, marked, and never padded" |
| W15 `eq` ignores the target, so two embeds share one table | "reuses the DOM for the same embed, so the caret moving cannot lose a cell" |

### The parser

`cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::csv`
— **21 passed**. Opening baseline 21 passed, closing baseline 21 passed, restore
byte-verified. **16 mutations, 16 caught, zero survivors.**

| Mutation | Caught by |
| --- | --- |
| M1 parse from offset 0, so the BOM becomes cell text | `the_byte_order_mark_belongs_to_no_field`, `an_edit_at_either_end_leaves_the_files_edges_alone`, `an_empty_file_parses_to_no_rows_and_refuses_an_edit_by_name` |
| M2 re-encode every write instead of keeping an unchanged field's bytes | `writing_a_cell_its_own_value_back_reproduces_the_file_byte_for_byte`, `junk_after_a_closing_quote_is_kept_rather_than_dropped` |
| M3 always write minimal quoting, forgetting the field's own | `an_edit_keeps_the_fields_quoting_and_adds_quotes_only_when_it_must`, `junk_after_a_closing_quote_is_kept_rather_than_dropped` |
| M4 stop treating CRLF's `\r` as a terminator | `a_crlf_record_ends_before_the_carriage_return` |
| M5 include the terminator in the record's span | `a_crlf_record_ends_before_the_carriage_return`, `the_byte_order_mark_belongs_to_no_field` |
| M6 an out-of-range column silently edits the last field instead | `editing_a_column_a_ragged_row_does_not_have_is_refused_with_a_reason` |
| M7 collapse `""` to nothing instead of one quote | `a_quoted_field_holds_its_commas_newlines_and_doubled_quotes` |
| M8 drop the bytes after a closing quote | `junk_after_a_closing_quote_is_kept_rather_than_dropped` |
| M9 a trailing terminator opens a phantom empty record (the naive-split bug) | 9 tests, led by `a_final_newline_adds_no_row_but_a_blank_line_between_rows_is_one` |
| M10 measure raggedness against the widest row, not the header | `a_ragged_row_is_kept_with_the_field_count_it_actually_has`, `a_ragged_file_says_how_many_rows_are_odd_and_does_not_pad_them` |
| M11 never quote a new value, whatever it contains | `an_edit_keeps_the_fields_quoting_and_adds_quotes_only_when_it_must` |
| M12 cap the table silently, with no notice | `a_long_file_is_capped_out_loud_and_keeps_the_files_row_numbers` |
| M13 the refusal stops naming the line ("the csv write failed") | `editing_a_column_a_ragged_row_does_not_have_is_refused_with_a_reason` |
| M14 the ragged notice stops saying how many rows are odd | `a_ragged_file_says_how_many_rows_are_odd_and_does_not_pad_them` |
| M15 a clean file grows a notice anyway | `a_clean_table_has_nothing_to_say`, `a_ragged_file_says_how_many_rows_are_odd_and_does_not_pad_them` |
| M16 the unterminated quote is closed at the field's end instead of reported | `an_unterminated_quote_is_reported_rather_than_closed`, `an_unterminated_quote_reaches_the_reader_as_a_sentence_naming_the_line` |

M15 came back `DID-NOT-COMPILE` inside the sweep — a peer was mutating a
different `keeper-core` module at that moment, so the crate did not build and the
verdict was about their tree rather than my test. It was re-run on its own
afterwards and is the `CAUGHT` above. That is the case the `DID-NOT-COMPILE`
verdict exists to keep out of the table: on a shared box a failed build is not
evidence either way, and folding it into `SURVIVED` would have invented a hole.

### Regression

`notes::frontmatter` (edited — `bom_len` moved out to `notes/mod.rs`) — **24
passed**. `csv-table.test.ts`, `recording-embed.test.ts`, `live-preview.test.ts`,
`mermaid-widget.test.ts`, `tab-wiring.test.tsx`, `client.test.ts` — **98 passed**.

### The defect neither system could see

A `TS2352` in `csv-table.test.ts` — `new MouseEvent("click") as Event & { target:
Element }`, a cast TypeScript is right to reject because `MouseEvent.target` is
`EventTarget | null` — sat in this story's own test file while the suite was
14/14 green and the mutation sweep was 15/15. Both of the verification systems
this story leaned on are blind to it, for different reasons: **vitest transpiles
without typechecking**, and **mutation testing only probes behaviour the tests
already assert**, so a mutant cannot reach a type that never survives to runtime.
A green suite plus a green mutation sweep is not a green build.

It was caught by a sibling running `bunx tsc --noEmit -p tsconfig.json`, and the
cast turned out to be pointless as well as wrong: an untargeted `MouseEvent`
already has `target === null`, which *is* the "not a cell" case the assertion is
about, so the fix was to delete it. `tsc` is now clean for every file this story
touched. Recorded because it generalises: `tsc` and the macOS Rust gate are the
two checks nothing else in this loop substitutes for.

## What this story found and did not fix

**DW-165 — a ```mermaid fence crashes the editor.** Wiring the table into
`live-preview`'s decoration set surfaced it: those decorations come from a
`ViewPlugin`, CodeMirror refuses a block decoration from a plugin, and the mermaid
branch asks for exactly one. Building a real `EditorView` with
`@codemirror/lang-markdown` **and** `livePreview` over a document containing a
fence throws `RangeError: Block decorations may not be specified via plugins`. The
suite is green because `mermaid-widget.test.ts` drives the widget directly, and
the one integration test that loads `livePreview` does it without the markdown
language, so the `FencedCode` node never exists. Filed in full, with the
reproduction and the `ViewPlugin` → `StateField` fix shape, rather than fixed
mid-wave in a file three agents were editing.

The consequence for this story: `CsvTableWidget` is an **inline** replace whose
host is styled `display: block`, matching `RecordingEmbedWidget`. An embed is one
line, so the inline form costs nothing; a fence is several, which is why that
widget needs the block form and cannot have it today.

## Deliberately NOT done

* **No CSV crate, and no other new dependency.** `Cargo.lock` has none, and one
  was not added.
* **No delimiter sniffing.** Comma only. A wrong guess writes the wrong bytes,
  which is the one failure a span-preserving design cannot undo.
* **No encoding detection.** A non-UTF-8 file is declined with a sentence rather
  than guessed at. Guessing Latin-1 and writing back would corrupt the file it
  claimed to open.
* **No adding, deleting or reordering rows and columns.** `set_cell` edits an
  existing field. Inserting one means writing a delimiter into a row keeper did
  not write, and the whole design is about not doing that; a row insert wants its
  own story and its own justification for touching terminators.
* **No sorting, filtering or formulas.** This is a file rendered as a table, not a
  spreadsheet. Sorting would either reorder the file (a write nobody asked for) or
  show an order the file does not have.
* **No repair of a ragged row or an unterminated quote.** Both are reported. A
  repair is keeper deciding where somebody's record ends.
* **No virtualisation.** The table caps at 500 rows and says so. 44.10's
  `useWindowedRows` is a React hook and this is an imperative CodeMirror widget;
  mounting a React root inside a widget to avoid a stated cap is the worse trade.
  Worth revisiting if a real vault produces a CSV where the cap bites.
* **No column resizing.** 44.12's `resizable-columns.tsx` is a React component for
  the list surfaces; the same hook/widget mismatch applies, and cells ellipsis at
  `max-width` rather than being cut with nowhere to read the rest.
* **No second embed syntax, and no new file classifier.** `![[…]]` is the one
  embed; 43.5's `kind_for_file_name` remains the one answer to what a file is.
* **No conflict copy on a stale write.** `notes_save` writes one because there is
  a user's buffer to preserve. Here there is a stale table and a file on disk, so
  the refusal reloads instead.
* **No `touch` of the reconciler after a CSV write.** The notes walk never collects
  a `.csv`; asking it to re-read one would be asking for an index entry that
  cannot exist.
* **The `keeper` shell crate was not compiled.** Not a caveat — a section. See
  "The part of this story that has never been compiled" below, and DW-171.

## The part of this story that has never been compiled

`cargo check -p keeper` dies on this host inside `glib-sys`'s build script for
want of `pkg-config`, before reaching a line of keeper's own source. Measured,
not assumed. So `csv_path`, `read_csv`, `notes_csv_read` and `notes_csv_set_cell`
have never been type-checked anywhere.

This is AD-55/AD-56's standing condition and every wave-3 spec says it. It gets
its own section here for one reason: **44.16's uncompiled code overwrites a file
the user already has.** 44.6's uncompiled shell code creates a note, so a mistake
there is a create that fails and nothing is lost. A mistake here rewrites
somebody's export inside a synced vault, and the commit travels to every machine.
Nothing was observed wrong; nothing *could* have been, which is the point.

The questions a compiler would settle, none of which a test in this repo reaches:

| Symbol | The type question |
| --- | --- |
| `csv_path` | `contained_read` returns `Option<PathBuf>`; the `.filter()` closure takes `&PathBuf`, and `for rel in candidates` moves each `String` before `Ok((rel, path))` returns it |
| `read_csv` | `&PathBuf` coercing to the `&Path` parameter; `metadata(path)?.len()` as `u64` against `MAX_CSV_BYTES` |
| `notes_csv_set_cell` | `set_cell` takes `usize` and the command receives `u32`; `rel` is **borrowed** by `write_vault_file` and then **moved** into `csv::project` — an ordering a compiler checks and a reader can talk themselves into |
| both commands | `CsvError` is mapped to `IpcError` by hand rather than through `notes_error`, so nothing enforces the mapping stays exhaustive if `CsvError` gains a variant |
| the tracing calls | `info!(%rel, row, column, "…: {error}")` relies on implicit captured identifiers, with `rel` a `String` behind `%` |

Mitigated as far as AD-55 allows: every decision, the whole parser and every
sentence the user reads are in `keeper-core` and proved there. What is left is one
read, one compare, one write. **But small is not compiled.**

### The hand-audit, and what it found

A compiler is unavailable; reading each symbol against its actual definition is
not. Prompted by W3NewNote finding a real compile error this way in 44.6's shell
code — an `IpcErrorCode::InvalidInput` that does not exist — every external
symbol above was checked against its declaration rather than against memory:
`IpcError`'s four fields, `IpcErrorCode::{Unsupported, NotesInvalid}`,
`contained_read`'s `Option<PathBuf>`, `content_rev`, `write_vault_file`,
`mark_dirty`, `Vault::id`, `ATTACHMENTS_DIR`'s new `pub(crate)`, and every
`csv::*` signature. **All correct.** That is not proof it compiles — a borrow or
a coercion can still be wrong — but it removes the class of error that bit 44.6.

The audit did find something the code was quiet about, and it is a data question
rather than a type one. `contained_read` stats with **`symlink_metadata`**, so it
refuses a symlink outright rather than following it. That is load-bearing here in
a way it is not for a read-only asset: `atomic_write` finishes with a `rename`,
and a `rename` onto a symlink **replaces the link with a regular file** instead
of writing through it — so an editable table over a symlinked CSV would silently
destroy the link on the first edit. It never gets that far, but the reason was
living in another module. `csv_path`'s comment now states it, and the redundant
`is_file()` filter is deliberately kept so this function's precondition holds on
its own terms rather than by depending on which `stat` a neighbour chose.

The read-then-write window was checked in the same pass and is sound by
construction: `notes_csv_set_cell` resolves, reads and writes **one** `rel`
within a single call, and the `rev` comparison is against the bytes that call
read. If the embed's resolution changed between the read command and the write
command — the bare-name candidate now finding a different file — the write
command's own read produces a revision the client's `rev` does not match, and it
refuses. There is no path where it writes over a file it did not just read.

### What the macOS gate owes this story

`cargo check --manifest-path src-tauri/Cargo.toml -p keeper`, and then:

1. `![[data.csv]]` for a file dropped into `attachments/` — the bare-name
   candidate is a branch a vault-relative path never exercises.
2. Edit one cell, then **compare the file on disk byte for byte against a copy
   taken before the edit.** Only that cell's bytes may differ. A visual check of
   the table cannot see a rewritten line ending or a dropped BOM, and that is the
   entire promise of this story.
3. Enter a cell, change nothing, blur. The mtime must not move **and** Console.app
   must carry `INFO` "csv cell unchanged, nothing written". Both halves: this is a
   path whose whole observable behaviour is that nothing happens, and DW-162 is
   the story of a decision that existed only in a log the packaged app could not
   print.
4. Edit a cell in a BOM + CRLF file and confirm sync commits it. `mark_dirty`
   reaching the commit cadence is the one claim here that no test on any platform
   covers.
5. A file over 4 MiB refuses with the sentence naming both sizes, rather than
   hanging.
