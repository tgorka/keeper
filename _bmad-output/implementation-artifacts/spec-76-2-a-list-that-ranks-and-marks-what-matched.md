---
status: review
baseline_revision: a925c4b
final_revision: ''
---

# Story 76.2 — A list that ranks and marks what matched

<intent-contract>

## Problem
Rows cannot show why a full-body search matched.

## Approach
Render Rust's UTF-16 excerpt ranges through a pure sorted, merged, clipped run splitter. Use the shipped search color pair; meaning-only results have a literal label and no invented marks.

## Always
Preserve ordinary rows when hit is absent. Search evidence takes precedence over unread origin per UX-DR95; origin remains accessible.

## Block If
Ranges are stale: Rust owns the excerpt and ranges together.

## Never
Never reinterpret UTF-16 offsets as bytes, inject HTML, or rematch query words in JavaScript.

## I/O and edge-case matrix
| Input | Output |
| --- | --- |
| Marks spanning ł and emoji | Exact UTF-16 slices |
| Unsorted overlapping marks | Sorted merged runs |
| Out-of-range marks | Clipped to text; empty ranges dropped |
| Empty marks | Plain text |
| Words/both hit | Real mark elements |
| Meaning hit | matched by meaning, no marks |
| No hit | Existing snippet/origin branch |

</intent-contract>

## Code Map
- `src/components/notes/note-row.tsx:316-318` — excerpt slot; `:390` hover detail.
- `src/lib/mark-runs.ts:1` — new pure run splitter.
- `src/index.css:179-180` — search theme pair.

## Tasks & Acceptance
Acceptance, verbatim from the epic: with the query `taxes` a note whose body says it and whose title does not is in the list, above a pinned note that does not say it, and its row shows an excerpt with exactly the runs `search::find` returns for that excerpt, marked (a component test drives `NoteRow` with a `hit` whose marks straddle a `ł` and asserts the `<mark>` text equals the sliced snippet — mutation-proved by converting the offsets as bytes instead of UTF-16); clearing the query restores pinned → updated → path with no `<mark>` in the DOM; a row admitted by a tag in the head chunk shows its ordinary snippet and no marks; NFR-64's p95 measured on hesperia against the owner's vault and written in the spec; the `notes_list` branch itself is **by inspection, awaiting CI macOS**.

## Design Notes
AD-266 and UX-DR95 govern coordinates and evidence; research §8 informs marked excerpts. No ranking logic lives in React.

- Review amendment: rejected search reads clear stale rows, retain `loaded`, and place their error sentence ahead of progress in the bar; no zero-result/count claim is shown until recovery.
- Lexical ranking/count is capped at 1,000 **notes**, selecting each note's best chunk in SQL; DW-267 owns any future full-vault count.
- Shell wiring (by inspection): absent, corrupt, or first-fill indexing search databases use legacy `matches_text`; only the plain lens applies chips, tier order is preserved, empty marks use prose snippets, and streams reproject on phase/vault transitions rather than count updates.

## Verification
Coordinator owns gates. UTF-16 regression must reject a byte-slicing mutation (TextEncoder byte slicing differs after ł/emoji). NFR-64 and visual measurement remain coordinator gates.

### UI proof (2026-09-19)
With Main's scoped-test permission, `node node_modules/vitest/vitest.mjs run src/lib/mark-runs.test.ts src/components/notes/note-row.test.tsx src/components/notes/editor/search-marks.test.ts src/components/notes/search-settings.test.tsx src/components/notes/note-editor.test.tsx` passed: 5 files, 61 tests. Mutation: replacing `text.slice(from, to)` in the marked run with UTF-8 byte slicing failed both the helper and actual row tests: expected `ł😀tax`, received `ł😀`. Restored source and all five files passed again. This proves rendered runs, not backend ranking or NFR latency. Browser measurement is coordinated with Bar.

### Shell wiring (by inspection)

By inspection, awaits CI macOS; none of this shell was compiled here. Gate: **Rust (fmt, clippy, test)**, `.github/workflows/ci.yml:28-52`, `macos-latest`.

`src-tauri/crates/keeper/src/notes_ipc.rs:334` (`last_queries`), `:365` (`row_of`), `:432` (`matches_filter`), `:490` (`project_list`), `:662` (`search_unavailable`), `:1249` (`notes_list`), `:2459` (`unwritten_row`), `:5087` (`stream_changes`), `:5154` (`current_window`), `:5169` (`default_query`) form the shell path. Ordinary rows and the colocated row fixture set `hit: None`. The obsolete test asserting snippet-only text filtering was removed rather than re-pinning the discarded implementation.

Nonblank text opens a fresh read-only search connection after any provider await, queries the bounded lexical pool and optional vector pool, intersects the current scope and non-text chips, sorts by score then updated time/path, and pages via `counts`. Head-chunk hits retain the ordinary snippet with no fabricated body marks; body hits use core byte marks/excerpt/UTF-16 conversion; meaning-only hits use prose and zero marks. Clearing text restores the existing browse/space ordering.

The subscription also wakes when search catches up, including when its initial database read failed. Requests are remembered before provider awaits; `Arc` request identity prevents an old asynchronous window from replacing the newer subscription query.

Matrix awaiting macOS execution: body-only hit → ranked row; tag-only head hit → plain ordinary snippet; tied scores → updated/path tie-break; empty text → existing pinned/space order; initial unavailable index → subscription remains alive; out-of-order provider reply → current request wins. Core mark/FTS tests and frontend render mutations do not prove this command. NFR-64 timing on hesperia remains owed.
