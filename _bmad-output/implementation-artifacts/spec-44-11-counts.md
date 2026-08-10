# Spec 44.11 — Counts

status: implemented
epic: 44 (The vocabulary is the space, and the note is a document)
binds: FR-166
resolves: DW-163 (`keeper.limit` is read, shipped and written back and has never been applied)
needs: 44.10 (the three lists are windowed)

## What was already there and dead

The epic's recurring lesson held for a third time, and twice over.

1. **`NoteListVm.total` was already correct and already invisible.** It has been
   the size of the whole filtered set since Story 37.2, computed in
   `project_list` before the offset and the page. Nothing showed it to anyone;
   its only consumer was `NoteList`'s `onGrow` predicate. The notes surface
   needed a reader, not a number.
2. **`FilesListingVm.truncated` was on the wire and nothing read it.**
   `keeper_sync::browse` stops at `LISTING_CAP` and reports that it did; Story
   43.8 shipped the boolean and the shell composed a sentence from it, but no
   TypeScript outside a test fixture has ever looked at the field. It is exactly
   the bit that tells an exact count from a floor — the one fact this story
   needed for the Files tree, already computed.
3. **`keeper.limit`** — DW-163's own finding, restated: parsed, clamped, put on
   `NoteSpaceVm`, round-tripped through the editor, written back on every save,
   never applied.

One thing was genuinely missing: the recordings archive has no count at all, and
`hits.length` is not one (below).

## The trap, and which number each surface shows

44.10 windowed all three lists. After it, "how many are rendered" and "how many
exist" differ by two orders of magnitude and render identically, so a surface
that prints the first while looking like the second is a wrong answer wearing
the shape of a right one — nothing distinguishes it, so nobody checks it.

**Every count in this app is a count of what EXISTS.** Not one of them is
derived from an array the component just rendered.

| Surface | Says | Read from | Exact? |
| --- | --- | --- | --- |
| Note list | how many notes the active lens SELECTS | `NoteListVm.total` / `NoteChangeBatch.total` | exact |
| Note list, under a cap | `20 of 347 notes` | `total` and `matched` together | exact, both |
| Recordings | how many sessions the filter matches in the archive | `RecordingSearchVm.total`, a `COUNT(*)` | exact |
| An open folder | how many entries the listing holds | `entries.length` | exact **up to `LISTING_CAP`** |
| An open folder at the cap | `1,000+ items` | `entries.length` + `truncated` | a declared floor |

Three of the four are exact and say so by saying a plain number. The fourth
cannot be, and says *that* in the number — `1,000+` — rather than in a sentence
underneath it that a reader may never reach.

### Why the recordings surface needed a backend change and the others did not

`recordings_fts::search_recordings` has always stopped at `DEFAULT_LIMIT = 200`.
So `hits.length` is 200 for an archive of exactly two hundred sessions and 200
for an archive of nine thousand. Before 44.10 that at least looked like a list
that ended; windowed, a list that stops at row 200 is indistinguishable on
screen from a complete one. `count_recordings` is a `COUNT(*)` built by the same
`Predicates` the search uses — extracted, not copied, because a count applying a
different `WHERE` from the list beneath it is wrong in exactly the way nobody
checks: both numbers look plausible and only one is being read.

### Why the folder count is a floor and not a total

Counting past `LISTING_CAP` means continuing to `stat` dirents so the same
exclusion rule applies to each — on the fifty-thousand-entry folder that
motivated the cap, fifty thousand syscalls to turn `1,000+` into `48,213`. That
is a worse answer to the same question: "more than a thousand, open it in
Finder" is already what the reader can act on, and it is what the cap notice
below the rows already says.

### Why a closed folder has no count

`listings` survives a collapse, so a closed folder *could* show the number it
had when it was last read — a count of rows nobody can see, taken at a moment
nobody can name. A count belongs beside the rows it counts. An unexpanded folder
has never been read at all, and reading every folder to number it is what lazy
expansion exists not to do.

## DW-163 resolved: `keeper.limit` caps SELECTION

Both readings were live. The decision, and why:

- **A rendering cap is now incoherent.** 44.10 bounds rendering by the viewport
  for every list. A second render cap would bound only *what can be scrolled
  to*, which is a selection cap wearing a disguise. And the transport window
  already has an owner — `NoteQueryReq.limit`, the page the frontend grows as it
  scrolls. Two answers to one question is the second convention the house style
  forbids.
- **A selection cap is a thing a person can mean and cannot otherwise say:**
  *this space holds the twenty most recent*. It composes with 44.4's `sort` —
  the cap is applied AFTER the ordering, so the space keeps the twenty the sort
  put first rather than twenty arbitrary matches.

**The count therefore shows the capped number**, because that is the number of
notes a person can reach; a `347` on a space that will only ever hand back 20
names a set nobody can get to.

**And a cap that bites is never silent**, in either place it could be:

- On screen, `matched` travels beside `total` and the surface reads
  `20 of 347 notes`.
- In the log, `Selection::report` words the decline at `INFO`. Declining to
  select notes the query matched is this story's one way of doing nothing, and
  DW-162 is the record of what a `debug!` is worth on the owner's machine. The
  level is `keeper-core`'s and is asserted there against the literal
  `Level::INFO`, copying 44.3's pattern including the reason it works — a floor
  compared only against itself accommodates whatever level someone reaches for.

`report` returns `None` when the cap declined nothing, because `project_list`
runs on every list read and a log that repeats a non-event drowns the events.

### Two repairs that fall out of it

- **Unset stopped meaning 500.** The old reader mapped an absent, zero or
  negative `limit` onto the shell's `MAX_LIMIT`, so "sets no cap" and "caps at
  500" were the same value. The editor round-trips a `limit` it does not render
  and wrote it unconditionally, so **every space saved once grew a
  `limit: 500` it never had** — a cap the user did not set, in a file they can
  read, that nothing was applying. Now zero is no cap and no cap writes no key,
  the rule `icon` and `order` already followed.
- **The space's cap is no longer clamped to the page size.** `limit: 2000` now
  means 2 000. Clamping it to 500 would silently drop 1 500 notes, which is the
  defect this story exists to remove.

### The other count that was being derived rather than read

`notesListStore.applyBatch` carried `total` forward itself: `+1` per `upsert` of
an unseen id, `-1` per `remove`. That is right only while every change to the
matched set also changes the window. A note that starts matching the filter
three thousand rows below the page produces no op, so it moved no count — and
once 44.10 windowed the list there is no scroll that would correct it. Both
counts now ride on `NoteChangeBatch`, recomputed in Rust for every batch, and
`NoteListOp::Reset` no longer carries a `total` of its own (a second copy on the
op that only some batches contain is a second copy that goes stale between
them). `stream_changes` also sends when only the counts moved.

## Where the code is

| File | What changed |
| --- | --- |
| `keeper-core/src/notes/counts.rs` | **New.** `Selection`, `select`, `page`, `read_limit`, `REPORT_FLOOR`, `Selection::report`. |
| `keeper-core/src/notes/mod.rs` | Registers it. |
| `keeper-core/src/notes/vm.rs` | `NoteListVm.matched`; counts moved onto `NoteChangeBatch`; `NoteListOp::Reset` loses `total`; `NoteSpaceVm.limit`/`NoteSpaceReq.limit` re-documented (0 = no cap). |
| `keeper-core/src/archive/recordings_fts.rs` | `Predicates` extracted; `count_recordings` added; `search_recording_vms` returns `RecordingSearchVm`. |
| `keeper-core/src/vm.rs` | **New** `RecordingSearchVm { rows, total }`. |
| `keeper/src/notes_ipc.rs` | `SpaceDef.limit: Option<u32>` via `counts::read_limit`; `clamp_limit` deleted; `space_lens` → `SpaceLens` carrying the cap and the name; `project_list` applies the cap after the sort, reports a decline, pages through `counts::page`; `notes_space_save` writes `limit` only when there is one; `stream_changes` sends counts per batch. |
| `keeper/src/ipc.rs` | `search_recordings` returns `RecordingSearchVm`. |
| `src/lib/count-label.ts` | **New.** The one place a count becomes words. |
| `src/lib/ipc/client.ts` | `searchRecordings` returns `RecordingSearchVm`. |
| `src/lib/stores/notes-list.ts` | `matched`; counts taken off the batch; the ±1 arithmetic deleted. |
| `src/hooks/use-notes-changes.ts` | The folder lens supplies `matched`. |
| `src/components/notes/notes-pane.tsx` | The count line, sibling of the list AND of the empty state. |
| `src/components/recordings/recordings-pane.tsx` | `total` state; the count in the header. |
| `src/components/layout/files-pane.tsx` | `entryCount`; `TreeNodeRow.count`; the count via `aria-describedby`. |
| `src/lib/ipc/gen/*` | Regenerated. |

### Why `countLabel` takes a number and a noun and nothing else

It cannot be handed a DOM node, an array of rendered rows or a windowing hook,
so every caller has to reach past whatever it just rendered to fetch the count
from the backend value that knows the whole set. The wrong number is not
available at the call site. That is the enforcement; the pluralisation is
incidental.

### Why the folder count is a description and not part of the row's name

`aria-label` on a `treeitem` replaces its subtree's contribution to the
accessible name, so a count rendered only as a child would be visible and
unspeakable. Folding it into the name instead would stop "Vault" being the row
called Vault — which is what a person navigating by first letter is matching
against, and what twenty existing tests query by. `aria-describedby` is where
supplementary facts belong, and it cost zero changes to those tests.

## I/O matrix

### `counts::select(matched, limit)`

| Input | Output |
| --- | --- |
| `(347, None)` | `total 347`, `matched 347`, not capped, no report |
| `(12, Some(500))` | `total 12`, not capped, **no report** — a cap nobody reached is not an event |
| `(20, Some(20))` | `total 20`, not capped, no report |
| `(347, Some(20))` | `total 20`, `matched 347`, capped, reports at `INFO` naming 347, 20 and 327 |
| `(usize::MAX, None)` | saturates to `u32::MAX` — a preposterous number beats a small wrong one |

### `counts::read_limit(raw)`

| Input | Output |
| --- | --- |
| absent key | `None` (no arm matches; `SpaceDef::limit` starts `None`) |
| `0`, negative, `0.5` | `None` — not one whole note is not a cap |
| NaN, ±∞ | `None` |
| `20` | `Some(20)` |
| `42.9` | `Some(42)` — truncates; rounding up admits a note the file did not ask for |
| `2000` | `Some(2000)` — **not** clamped to the page size |
| `1e30` | `Some(u32::MAX)` |
| a non-numeric value (`limit: soon`) | `None`; the space is uncapped, never capped at something invented |

### `counts::page(selection, offset, size)`

| Input | Output |
| --- | --- |
| uncapped 347, `(0, 200)` | `0..200` |
| uncapped 347, `(200, 200)` | `200..347` |
| capped to 20 of 347, `(0, 60)` | `0..20` |
| capped to 20 of 347, `(20, 60)` | `20..20` — **no second page into the declined notes** |
| capped to 20, `(300, 60)` | `20..20`; the echoed offset clamps to 20 |
| `(3, u32::MAX)` over 5 | `3..5` — the add saturates |

### `count_recordings` / `RecordingSearchVm`

| Input | Output |
| --- | --- |
| 250 sessions, no filter | `rows.len() == 200` (the page), `total == 250` |
| any `filter.limit` | `total` unchanged — the page size is not the archive's size |
| every predicate (text, tag, participant, date, durability, profile) | count and page narrow together; under the cap they are equal by assertion |
| no match | `total == 0`, not an error |
| pre-42.1 `archive.db`, or no `archive.db` | `rows: []`, `total: 0` |

### `countLabel`

| Input | Output |
| --- | --- |
| `(0, NOTES)` | `0 notes` — zero is a number, never a silence |
| `(1, NOTES)` | `1 note` |
| `(12_345, NOTES)` | `12,345 notes` (grouped) |
| `(20, NOTES, {of: 347})` | `20 of 347 notes` |
| `(1, NOTES, {of: 4})` | `1 of 4 notes` — the noun agrees with the number it follows |
| `(12, NOTES, {of: 12})` / `{of: 4}` | `12 notes` — a cap that did not bite gets one number |
| `(1000, ITEMS, {atLeast: true})` | `1,000+ items` |
| `(1, ITEMS, {atLeast: true})` | `1+ items` — plural, because "at least one" is not "one" |

## Edge cases

| Case | Behaviour |
| --- | --- |
| Notes: nothing matches | `NotesEmptyState` replaces the LIST; the count is its sibling and says `0 notes` |
| Notes: no vault at all | No count. There is no lens to count, and `0` would be a claim about a vault that does not exist |
| Notes: before the first read (`loaded === false`) | No count |
| Notes: physical-folder lens | `total == matched == notes.length`; one folder level IS the whole set |
| Notes: plain lens (no space) | No cap exists and none can — `keeper.limit` is a property of a space note |
| Recordings: before the first answer | No count. `0 sessions` before a query has run is a claim nobody has checked |
| Recordings: query rejected | The alert; the count holds its last answer rather than flashing a zero the archive never said |
| Recordings: filter changes | Count follows, debounced with the query, through the same stale-response guard |
| Files: folder unreadable (absent drive, foreign volume, moved) | No count. `entries === null`, and `0 items` would be a claim about a folder nobody opened |
| Files: read in flight | No count |
| Files: empty folder | `0 items`, beside the empty-folder sentence |
| Files: folder collapsed again | The count goes with it |
| A space with `sort` keeper cannot read AND a cap | Both still apply: the fallback ordering runs, then the cap. A cap is not conditional on a readable sort |

## Tests, and the mutation each one caught

Reverted, watched fail, restored. Nine mutations.

| # | Mutation | Tests that failed |
| --- | --- | --- |
| 1 | **Recordings count from the page** — `setTotal(result.rows.length)` | 2: `says the archive's count even when the page it was sent is smaller`, `groups a five-digit archive rather than setting it solid` |
| 2 | **Ignore `truncated`** — `atLeast: false` | 1: `marks a capped listing as a floor instead of passing it off as a total` |
| 3 | **Hide a zero folder count** — treat an empty listing as no listing | 1: `says zero for an empty folder rather than dropping the count` |
| 4 | **Store derives the count from its rows** — `total: rows.length` | 3: `takes the count off the batch rather than counting the ops`, `moves the count for a batch that moves no row at all`, `carries the matched count beside the selected one when a space caps` |
| 5 | **A cap truncates the count silently** — `capped = false` in `countLabel` | 2: `names both numbers when a cap declined some of them`, and the pane's `says both numbers when a space's keeper.limit declined some of them` |
| 6 | **`keeper.limit` never applied** — `total = matched` | 3: `a_cap_that_bites_keeps_both_numbers_so_nothing_is_silent`, `a_decline_is_reported_where_the_app_can_actually_print_it`, `a_page_is_carved_out_of_what_the_space_selected_and_never_past_it` |
| 7 | **Unset limit means the page size again** (the pre-44.11 conflation) | 1: `an_unset_limit_is_no_cap_rather_than_a_cap_of_zero` |
| 8 | **Lower `REPORT_FLOOR` to `DEBUG`** | 1: `a_decline_is_reported_where_the_app_can_actually_print_it` — the assertion pins the literal `Level::INFO`, so lowering the constant does not satisfy it |
| 9 | **`search_recording_vms` totals its own page** | **0 on the first run — a real gap.** Both count tests called `count_recordings` directly, and every existing `search_recording_vms` fixture is under the 200-row page, so a `total` taken from the vector was right in all of them. Closed by extending `the_count_is_the_whole_archive_and_not_the_page_the_search_returned` to assert the VM at 250 sessions, including `assert_ne!(total, rows.len())` so the fixture can never shrink under the cap and quietly re-open the hole. The mutation now fails it. |

Commands run:

```
bun run test src/lib/count-label.test.ts src/lib/stores/notes-list.test.ts \
             src/components/notes/notes-pane.test.tsx \
             src/components/notes/note-list.test.tsx \
             src/components/recordings/ \
             src/components/layout/files-pane.test.tsx
→ 7 files, 121 tests, all passing

cargo test -p keeper-core --lib notes::counts           → 11 passed
cargo test -p keeper-core --lib archive::recordings_fts → 38 passed
bunx tsc --noEmit  (findings filtered to this story's files — clean)
```

The AC's virtualisation clause is asserted on all three surfaces with the window
ON and a fixture far larger than one: 4 000 notes, 2 000 sessions and 3 000 files
in jsdom, each asserting a bounded mounted-row count in the same test as the
count itself, so neither assertion can pass by the other failing to windowing.
`withListGeometry` is load-bearing in all three — without it jsdom's zero
geometry makes "only a window mounted" true of a list that mounted everything.

## What could not be proved here

- **The `keeper` shell crate does not build on Linux** (AD-55/AD-56; its GTK
  deps fail before compilation). `project_list`, `space_lens`, `space_def`,
  `notes_space_save`, `stream_changes` and `search_recordings` are unverified by
  any compiler on this host. That is why the cap, the page arithmetic, the
  limit reader and the report level all live in `keeper-core` — the shell holds
  wiring and one `match` over a level. The updated shell tests
  (`a_space_definition_reads_the_one_level_form_and_the_nested_one`,
  `a_frontmatter_limit_reaches_the_one_reader_of_it`, and the two recordings IPC
  tests) run only on the macOS gate.
- **No browser.** Nothing here is verified against a real engine, including
  whether the notes count line's `role="status"` announces at a useful moment
  rather than on every keystroke of the search field.
- **The `COUNT(*)` cost on a real archive.** It is one extra statement per
  query over `recordings`, unindexed for most predicate combinations. On the
  fixtures here it is free; on a nine-thousand-session archive it has not been
  measured against NFR-33.
- **Whether `1,000+` reads as intended** to someone who has not read this spec.

## Deliberately NOT done

- **No count on a space's rail row.** "A space says how many notes it selects"
  is answered by the list header for the space you are looking at. Numbering
  every rail row means evaluating every space's query — including `text:` terms
  that read note bodies — on every index change, which is N full scans of the
  vault for a decoration. NFR-28 says no.
- **No count on an unexpanded folder, and none on a file.** Both would need a
  read that lazy expansion exists to avoid.
- **No count of the tags tree, the physical tree, the attachment panel or the
  search results.** The epic named three lists; giving a count to a surface
  whose set nobody asked about is scope, and each would need its own answer to
  the exact-or-floor question.
- **No `limit` control in the space editor.** `keeper.limit` now does something,
  and a form control for it is a UX decision (what does zero look like? what
  does the count say while you drag it?) that belongs to whoever wants the
  feature. The value round-trips untouched, and a person can write it in
  frontmatter today — which is how every space feature in this epic started.
- **No counting past `LISTING_CAP`.** See above.
- **No `matched` on the recordings or files surfaces.** Neither has a selection
  cap; `matched` exists solely because `keeper.limit` does.
- **DW-164 not touched.** `recorded` still reads a user-editable stamp. It is a
  schema decision beside 44.2, as its entry says.
