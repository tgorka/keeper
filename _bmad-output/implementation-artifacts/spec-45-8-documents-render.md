# Story 45.8 — Documents Render

status: implemented
epic: 45 (Open it, change it, put it back), wave 2
binds: FR-181, FR-182, UX-DR71, AD-65, AD-87, AD-91
agent: W2Documents

---

## What shipped

**Four formats render. Three of them are fully verified on this machine; the
fourth's page images are drawn by a renderer that does not exist on Linux.**

| Format | What renders | Where the work happens | Verified here |
|---|---|---|---|
| PDF | The document itself, in the webview's own PDF renderer, plus page count, version and encryption from a Rust probe | `<embed>` over 45.7's `keeper-file://`; probe in `keeper_core::document` | Probe yes; **page images no** — see "what I could not verify" |
| XLSX | Sheet tabs, cell grid, per-sheet real row count | `keeper_core::document::read_xlsx` | Yes, end to end |
| DOCX | Paragraphs with outline level and bold/italic runs, in document order | `keeper_core::document::read_docx` | Yes, end to end |
| PPTX | A slide outline: slide count, per-slide title and text lines | `keeper_core::document::read_pptx` | Yes, end to end |

Nothing fell back to the unknown viewer as a shipped outcome. The brief allowed
"two good renderers and two honest placeholders"; what made four viable is that
DOCX, PPTX and XLSX are **one problem** — a ZIP of XML — so after the container
spine exists each format is a different XML shape rather than a different
project. PPTX is the weakest and the UI says so in a word: its body is labelled
**"Slide outline"**, not "Presentation", because keeper extracts text in reading
order and does not lay out DrawingML.

### Check-whether-it-already-exists

Asked before building, per the epic's standing instruction. **Most of the
registry work was already done and I wrote none of it:**

- `ViewerId` already had `document`. Not added.
- `ViewerFormat` already had `pdf`, `docx`, `pptx`, `xlsx`. Not added.
- `registry.ts` already had four `documentRow(...)` entries with
  `icon: "file-document"` and `writable: false`. Not added, not edited.
- The only registry change this story makes is **one line** in
  `VIEWER_COMPONENTS`: `document: DocumentViewer`. That is exactly the "adding a
  format is a row, not a surface" that AD-87 promised, arriving as a one-line
  diff.

Also searched and **found nothing**: no PDF, ZIP or OOXML library anywhere in
`bun.lock` (checked `pdfjs`, `pdf-lib`, `jszip`, `fflate`, `pako`, `xlsx`,
`mammoth`, `exceljs`, `adm-zip` — zero hits). `mermaid-widget.ts` lazy-imports
`mermaid`, whose graph is katex, d3, cytoscape, dompurify, marked and roughjs —
nothing document-shaped. There was no free ride in the bundle.

What I *did* find was on the Rust side: `zip` and `quick-xml` are already
compiled in every keeper build for unrelated reasons.

---

## Dependencies

Approved over `hub` before use, with costs named.

| Crate | Why already free | Feature choice |
|---|---|---|
| `zip` 4.6.1 | Already in `Cargo.lock` via `tauri-plugin-updater` | `default-features = false, features = ["deflate-flate2"]` |
| `quick-xml` 0.39 | Already in `Cargo.lock` via `plist` ← `tauri-utils` | defaults |
| `flate2` 1 | Already in keeper-core's own graph via `reqwest` | defaults |

**`deflate-flate2`, not `deflate`.** They are not synonyms: plain `deflate`
expands to `deflate-zopfli` + `deflate-flate2-zlib-rs`, which pulls the zopfli
**compressor** and a second zlib backend. My first resolution added `zopfli` as a
genuinely new lockfile package; narrowing the feature removed it. keeper only
reads a document, so paying for a compressor bought nothing.

**Final `Cargo.lock` delta: zero new packages.** Three added dependency edges on
`keeper-core`, and `flate2` added to `zip`'s own dependency line. I overstated
this to Main once ("zero lockfile entries") and corrected it unprompted; the
lockfile *does* change, just not by gaining a package.

**Rejected: the JavaScript route.** ~900 kB of SheetJS plus ~250 kB of mammoth
plus a PPTX library, shipped to every user including those who never open a
spreadsheet. Parsing in Rust costs **zero bundle bytes** because what crosses IPC
is a bounded view model, never a document.

**Failure mode when it cannot load:** there is no load — both crates are compiled
in. A file that is not a valid ZIP, or is a valid ZIP missing its main part,
produces a named refusal (`"keeper could not read this word document:
word/document.xml is missing"`) rendered on 45.2's unknown viewer with the file's
name, size and Open With.

---

## Architecture

### The bytes never cross IPC

`sync_read_document(id, subpath) -> DocumentVm`. Rust opens the file, parses, and
returns structure. **A 400-page PDF and a 50 000-row spreadsheet both return a
few kilobytes.**

I originally proposed and had approved a `sync_read_bytes` command with a size
cap. I then retired it before writing it, because W2Media's `keeper-file://`
scheme is Range-served straight into the webview — so a 200 MB PDF costs nothing
to open instead of being refused at a cap. Deleting the expensive path beat
capping it. Main's condition 4 ("a cap AND a stated behaviour above it") moved
from the marshalling to the **parse**: a 2 GB file must not be inflated to answer
"how many sheets".

### The format comes from the CONTENT

`sniff()` reads magic bytes and, for a ZIP, which main part the container holds.
A `.xlsx` holding a Word document is reported as a Word document and rendered as
one, and the header says so. This is deliberately **not** a second copy of 43.5's
`kind_for_file_name`: that answers *which viewer should keeper mount*, from a
name, before anything is read; this answers *what did keeper actually find*,
after opening. The first is routing and must be cheap; the second is a fact.

### One entry, two surfaces

`DocumentViewer` (loads) is split from `DocumentView` (renders a `DocumentVm`),
the same seam 45.4/45.6 established with `useTextFile` + `RawRenderedView`. A
note embed holding vault coordinates mounts `DocumentView` directly rather than
forking the rendering. Coordinated with W2Embeds over `hub`.

### `useWindowedRows` — used, and where it does not fit

44.10's window is used for Word blocks, slides and sheet rows: a document body is
exactly what it is for, a long flat keyed list, and it needed no modification.

**It does not fit the PDF, and that is not an omission.** There is no list of
pages to window, because the pages live inside one `<embed>` that the platform
pages itself. The bound is structural — one element for 400 pages — which is
strictly better than a cap that has to be chosen and tuned.

Bounded twice, and both are needed: Rust caps what is **sent**, the window caps
what is **mounted**. 3 000 paragraphs is a small message and a large DOM.

---

## I/O and edge-case matrix

### `keeper_core::document::open_document`

| Input | `format` | Body | `detail` | Test |
|---|---|---|---|---|
| Valid PDF, classic page tree | `pdf` | probe | none | `a_pdf_reports_its_version_and_page_count` |
| 400-page PDF | `pdf` | `pageCount: 400` | none | `a_four_hundred_page_pdf_reports_four_hundred` |
| PDF 1.5+, page tree in `/ObjStm` | `pdf` | counted after inflation | none | `a_page_tree_inside_an_object_stream_is_counted` |
| PDF with a 99-entry outline | `pdf` | `pageCount: 3` | none | `an_outline_count_is_not_mistaken_for_a_page_count` |
| PDF whose page tree the probe cannot read | `pdf` | `pageCount: null` | none | `an_unreadable_page_tree_reports_no_count_rather_than_guessing` |
| PDF > 25 MB | `pdf` | `pageCount: null`, version kept | "…the pages themselves still render" | `a_pdf_over_the_probe_cap_keeps_rendering_and_drops_only_the_count` |
| PDF at exactly 25 MB | `pdf` | counted | none | `a_pdf_at_the_probe_cap_is_still_counted` |
| Encrypted PDF | `pdf` | probe, `encrypted: true` | "…may not render without its password" | `an_encrypted_pdf_says_so` |
| Truncated PDF | `pdf` | `pageCount: null` | none | `a_corrupt_pdf_degrades_rather_than_throwing` |
| DOCX with styles | `docx` | blocks + styles | none | `a_word_document_renders_its_paragraphs_and_their_styles` |
| DOCX with `w:val="0"` on bold | `docx` | run NOT bold | none | `run_emphasis_is_read_including_its_off_switch` |
| DOCX with a table | `docx` | cell text as paragraphs, in order | none | `text_inside_a_table_is_not_dropped` |
| DOCX with field codes / tracked deletions | `docx` | neither shown | none | `field_codes_and_deleted_text_are_not_shown` |
| DOCX, > 3 000 paragraphs | `docx` | 3 000 blocks, real `blockCount` | "…showing the first 3000" | `a_long_word_document_is_bounded_and_says_so` |
| DOCX ending mid-paragraph | `docx` | **none** | "…the file is truncated" | `a_corrupt_word_document_degrades_with_a_named_reason` |
| PPTX | `pptx` | slides, titles, lines | none | `a_presentation_renders_its_slides` |
| PPTX with 12 slides | `pptx` | slide 2 before slide 10 | none | `slides_past_nine_stay_in_order` |
| PPTX with no slides | `pptx` | none | "it contains no slides" | `a_presentation_with_no_slides_is_a_named_refusal` |
| XLSX | `xlsx` | sheet names + cells | none | `a_workbook_renders_its_sheet_names_and_cells` |
| XLSX with a sparse row | `xlsx` | `D1` in the 4th column | none | `a_sparse_row_keeps_its_columns` |
| XLSX, `rId` → `sheet7.xml` | `xlsx` | right data under right tab | none | `a_sheet_is_found_through_its_relationship_not_its_position` |
| XLSX, out-of-range shared string | `xlsx` | empty cell, no panic | none | `an_out_of_range_shared_string_does_not_panic` |
| XLSX, 2 500 rows | `xlsx` | 500 rows, `rowCount: 2500` | truncation note | `a_huge_sheet_is_bounded_and_reports_its_real_height` |
| XLSX, 20 sheets | `xlsx` | 16 sheets, `sheetCount: 20` | "…showing the first 16" | `a_workbook_with_too_many_sheets_is_bounded_and_says_so` |
| ZIP bomb, honest header | `docx` | none | "…declares 16.0 MB…" | `a_decompression_bomb_is_refused_before_it_is_inflated` |
| ZIP bomb, **forged** header | `docx` | none | "…inflates past…" | `a_bomb_that_lies_about_its_size_is_refused_while_inflating` |
| Many legal parts, > 48 MB total | `pptx` | none | "…expands to more than…" | `parts_that_are_individually_legal_still_exhaust_the_document_budget` |
| Billion-laughs DTD | `docx` | bounded | — | `entity_expansion_is_refused` |
| Container > 50 MB | `docx` | none | "…use Open With" | `a_container_over_the_cap_is_refused_with_its_size` |
| Container at exactly 50 MB | `docx` | parsed | none | `a_container_at_the_cap_is_read` |
| `.xlsx` holding a DOCX | `docx` | Word body | — | `format_comes_from_the_bytes_not_the_extension` |
| Not a document | `null` | none | "could not recognise…" | `a_file_that_is_not_a_document_is_named_rather_than_thrown` |
| A plain ZIP | `null` | none | same | `a_plain_zip_is_not_a_document` |

### The viewer

| Situation | Renders |
|---|---|
| `.odt` (not implemented) | Unknown viewer, never reaches the loader |
| No `profileId` | Unknown viewer, "not inside a synced folder" |
| `DocumentView` mounted with `pdfSrc: null` | Unknown viewer, "cannot reach this PDF's bytes" |
| Command rejected | Unknown viewer with Rust's own sentence |
| Any refusal | Unknown viewer with Rust's own sentence — **never a blank pane** |
| Name/content mismatch | Renders what it IS, header says both |

### Main's four conditions

1. **Decompressed size bounded, not compressed.** Three guards: declared size
   (cheap, distrusted), `Read::take` at 16 MB per part counting *produced* bytes,
   and a 48 MB document-wide `Budget`. All three have a fixture; the forged-header
   one exists because the first guard believes the container.
2. **Entity expansion bounded — verified, not assumed.** `quick-xml` does not
   expand unrecognised entities; it errors, and `text_of` then keeps the raw
   characters. `entity_expansion_is_refused` asserts the **library's** behaviour
   so that a future `quick-xml` that started expanding would fail the build
   rather than silently making this module unsafe.
3. **Projection bounded.** `MAX_SHEETS` 16, `MAX_ROWS_PER_SHEET` 500,
   `MAX_COLUMNS` 40, `MAX_CELLS` 20 000 workbook-wide, `MAX_BLOCKS` 3 000,
   `MAX_SLIDES` 150. Every truncatable collection carries **both** a real count
   and a `truncated` flag — 44.11's rule, so a 500-row window over a 50 000-row
   sheet can never read as the whole sheet.
4. **A cap and a stated behaviour above it**, moved to the parse: 50 MB container
   (both sides tested), 25 MB PDF probe (both sides tested). Above the container
   cap: name, size, Open With. Above the probe cap: **the pages still render**,
   only the count is dropped.

---

## Mutation proof

22 mutations. **All 22 caught** after three initial survivors were fixed — and
all three survivors were real weaknesses, not equivalent mutants.

Baseline green before and after, at exactly the verdict's scope, judged by
**exit code**: `cargo test -p keeper-core --lib document::` (46) and
`bun run test src/lib/viewers src/components/viewers/document-viewer.test.tsx
src/components/layout/panel-strip.test.tsx` (296). Three consecutive repeats
each, EXIT=0.

| # | Mutation | Caught by |
|---|---|---|
| M1 | `index - 1` → `index` in `column_index` | 4 tests incl. `column_references_are_parsed_as_bijective_base_26` |
| M2 | `preceded_by_type` always true | `an_outline_count_is_not_mistaken_for_a_page_count` *(after fix)* |
| M3 | Remove the produced-bytes cap | `a_bomb_that_lies_about_its_size_is_refused_while_inflating` |
| M4 | `toggle_on` always true | `run_emphasis_is_read_including_its_off_switch` |
| M5 | Sort slides lexicographically | `slides_past_nine_stay_in_order` |
| M6 | `row_count` = projection length | `a_huge_sheet_is_bounded_and_reports_its_real_height` |
| M7 | Collect text anywhere in `w:p` | `field_codes_and_deleted_text_are_not_shown` + 1 |
| M8 | Ignore the sheet relationship | `a_sheet_is_found_through_its_relationship_not_its_position` |
| M9 | `Budget::charge` never decrements | `parts_that_are_individually_legal_still_exhaust_the_document_budget` |
| M10 | Disable ObjStm inflation | `a_page_tree_inside_an_object_stream_is_counted` *(after fix)* |
| M11 | Shared string returned as its index | 2 tests |
| M12 | Remove the container size gate | `a_container_over_the_cap_is_refused_with_its_size` |
| M13 | Remove the truncated-part refusal | `a_corrupt_word_document_degrades_with_a_named_reason` |
| M14 | Ignore the `%PDF-` magic | 9 tests |
| M15 | Unbind `document:` from `VIEWER_COMPONENTS` | 20 tests (DW-172) |
| M16 | Remove the `pdfSrc === null` guard | `refuses to draw an embed with no URL…` *(after fix)* |
| M17 | Remove the `body === null` degradation | 2 tests |
| M18 | Never report a format mismatch | `says so when the bytes are not the format the name promised` |
| M19 | `data-row-count` = window length | `mounts a window over a 50 000-row sheet and still reports 50 000` |
| M20 | Remove the no-profile short circuit | `shows the placeholder instead of an embed…` |
| M21 | Invert the page-count condition | 3 tests |

### The three survivors, and what each exposed

**M10 exposed a test that was green for the wrong reason — the most valuable
finding of this story.** `a_page_tree_inside_an_object_stream_is_counted`
deflated a 37-byte dictionary, and flate2 emits a **STORED block** for input that
short because compression does not pay. So `/Pages` stayed legible as plain ASCII
inside the supposedly compressed stream, the raw scan found it, and object-stream
inflation was **never exercised at all**. The fixture now pads the payload with
redundant filler and uses `Compression::best()`, and two assertions run *before*
the act: `find(&pdf, b"/Pages").is_none()` and `page_count_in(&pdf).is_none()`.
Without a mutation sweep this test would have shipped proving nothing.

**M2 exposed an untested guard.** `preceded_by_type` stops `/Pages` appearing as
an ordinary name from being read as a page-tree node — but my outline fixture
contained no such name, so removing the anchor changed nothing. The outline dict
now also carries `/Next /Pages`; an unanchored probe reports 99 pages for a
three-page document.

**M16 exposed an unreachable-by-the-registry branch.** The `pdfSrc === null`
guard lives in `DocumentView`, which `DocumentViewer` never reaches with a null
URL (a file with no profile fails in the loader first). It is not dead code — it
is the guard for the *second host*, the note embed with vault coordinates — so
the fix was a test that mounts `DocumentView` directly. It is the one test in the
file that does not go through the registry, and it says why.

### I left a live mutant in a shared crate for ~40 minutes

Reported to the wave at the time. My sweep loop was `apply → run → revert` and a
cell timeout killed it **during M9's test run**, so
`Budget::charge`'s decrement sat mutated to `saturating_sub(0 * used)` — the
document-wide bomb budget silently disabled — in a worktree five other agents
were building.

**My verification grep said CLEAN and was lying, for a new reason.** I checked
with `grep -c -F '0 \* used'`. `-F` means fixed-string, so the backslash I had
escaped out of habit became a literal character to search for and the query could
never match the real text. This is W2Table's finding with the sign flipped, and
the two bracket the failure mode:

- unescaped metacharacter under a **regex** matcher → false clean;
- escaped backslash under a **fixed-string** matcher → false clean.

**Escaping is only correct relative to the matcher, and both ways of getting it
wrong fail silently toward "clean".**

What actually caught it was the **presence** direction: asserting every original
anchor is present exactly once, in Python with plain `str.count()` and no matcher
semantics anywhere in the path. A missing anchor cannot hide behind a bad pattern
the way a live mutant can. The harness now does both directions, uses
`finally: revert()` so an interrupt cannot strand a mutant, and never spells a
deletion as a replacement with `""` (whose inverse matches at every position —
the trap that bit W2Marks, W2Embeds and W2Media). I also ran W2Attach's
whole-diff read over every tracked file I touched: every changed line is mine and
intended, or a sibling's and correctly theirs.

---

## Deliberately NOT done

- **Any write path.** No save, no dirty tracking, no refusal wording for a save
  that cannot happen. `entry.writable` is `false` for all four rows. A lossy
  round trip through a document container is how people lose work; a viewer that
  can only destroy information should not offer a save button.
- **DOCX layout fidelity.** Columns, floats, page geometry, images, footnotes and
  table *structure* are not rendered. Reproducing Word's layout engine is a
  product, not a story, and a half-built one renders documents subtly wrong in a
  way a reader cannot detect. What **is** guaranteed: no text is silently
  dropped — paragraphs inside a table still appear, in document order.
- **PPTX visual fidelity.** A slide is positioned shapes with themes, masters and
  inherited layout; drawing one faithfully means implementing DrawingML. keeper
  extracts text in reading order and labels the result "Slide outline".
- **XLSX number formats.** Cells render their stored value. Excel's number-format
  language is not implemented, so `0.1+0.2` shows as the file stores it.
- **XLSX formulas, charts, merged cells, styling.**
- **ODT / ODS / ODP / `.doc` / `.pages` / `.key`.** A different container with a
  different XML vocabulary. They resolve to the unknown viewer, which names the
  extension and offers Open With — asserted by
  `leaves a document format keeper does not implement on the unknown viewer`.
- **A real PDF parser.** The probe resolves no cross-reference table and follows
  no indirect object. It is a probe and is documented as one; when it cannot
  determine a page count it returns `null` and the UI **omits** the number rather
  than guessing. A header saying "12 pages" for a 400-page document is worse than
  a header saying nothing.
- **45.21's annotations.** Explicitly a later story.

---

## What I could not verify here, and why

**1. The PDF pages have never been drawn.** This is the big one. The `<embed
type="application/pdf">` is routed to the platform's PDF renderer (PDFKit under
WKWebView on macOS). The `keeper` shell crate does not build on Linux and jsdom
renders nothing, so what is proved here is that the element is mounted exactly
once with the right `type` and the right `keeper-file://` URL — **not that a
single pixel of a page appears.**

*The by-hand check the macOS gate owes this story:* open a PDF from the Files
pane and confirm pages are visible and scrollable; confirm the header's page
count matches what the renderer shows; confirm a 400-page PDF scrolls without the
pane stalling. If the embed does not render, the honest fallback is already
built — it degrades to probe facts plus Open With — but **it would degrade
silently to an empty element rather than to the placeholder**, because a failed
plugin render is not an error this code can observe. That is the one place where
the "empty box lie" could still occur, and it is stated here rather than
discovered later.

**2. `sync_read_document` has never been called over IPC.** The command is a thin
call site in `keeper/src/sync_ipc.rs`, which does not compile on Linux. Its body
is four lines, all of them copied in shape from `sync_read_text` directly above
it, and every decision it delegates to is tested in `keeper-core`. But the
registration, the argument names and the serialisation round trip are unproved
until macOS.

**3. `keeper-file://` serving a PDF is W2Media's code and their gate.** I
depend on `file_protocol::is_servable` accepting `pdf` (they landed it, with
`serves_a_pdf_and_no_other_document`) and on the handler resolving through
`browse::resolve`. Their Range and containment tests for the handler itself run
on the macOS gate for the same reason mine do not run here.

**4. No real-world document has been opened.** Every fixture is synthetic and
built in the test — deliberately, because a committed `.docx` is a blob nobody
can read in a diff and nobody can vary, and it is what made the bomb and
billion-laughs fixtures cheap enough to actually write. But synthetic OOXML is
*tidy* OOXML. Real files from Word, Pages, LibreOffice and Google Docs export
carry namespace prefixes, `mc:AlternateContent` fallbacks, and structures I have
not seen. **The most likely place for a first bug report is a real `.docx` whose
paragraphs render but whose lists or headings do not.**

**5. The 44.10 window's real heights are unmeasured.** jsdom measures zero, so
`withListGeometry` models the browser. Whether `WORD_BLOCK_ESTIMATE` (28 px) is
close to a real heading's height at a real font is not proved; a bad estimate
degrades scroll smoothness, not correctness.

**6. Not run, per instruction:** `cargo clippy`, `cargo fmt`, `bun run lint`, and
the full TS suite. `npx tsc --noEmit` **was** run and is clean over every file
this story touches. The full `keeper-core` lib suite is EXIT=0 (1620 tests).

---

## Files

**New:** `src-tauri/crates/keeper-core/src/document.rs`,
`src-tauri/crates/keeper-core/src/document/tests.rs`,
`src/components/viewers/document-viewer.tsx`,
`src/components/viewers/document-viewer.test.tsx`,
`src/components/viewers/use-document-file.ts`, and nine generated
`src/lib/ipc/gen/*.ts`.

**Edited:** `src-tauri/Cargo.toml` + `keeper-core/Cargo.toml` + `Cargo.lock`
(dependencies), `keeper-core/src/lib.rs` (one `pub mod`), `keeper/src/lib.rs`
(one command registration), `keeper/src/sync_ipc.rs` (`sync_read_document`),
`src/lib/ipc/client.ts` (`syncReadDocument` + type exports),
`src/lib/viewers/components.tsx` (**one binding line**).

**Other people's tests I edited, both a direct consequence of binding
`document`:** `src/lib/viewers/components.test.tsx` (added the pending
`syncReadDocument` mock) and `src/components/layout/panel-strip.test.tsx` (same
mock, plus the "draws a resolved file through the registry" case, whose comment
literally read *"the `document` viewer, which wave 2 has not bound"* — it was
asserting the placeholder as a stand-in for the real thing, and now asserts the
real thing in the same frame with the same aria-label). Both announced to the
wave.
