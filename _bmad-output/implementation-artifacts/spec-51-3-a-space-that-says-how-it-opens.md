# Spec 51.3 — A space that says how it opens and how much it shows

story: 51.3
status: in-progress
branch: `feat/51-3-a-space-that-says-how-it-opens` (on 51.2)
binds: FR-289, FR-290; Story 49.3 (the fold and its default), AD-121
sentinel: `MUT51-3`

<intent-contract>

**The ask.** *"gdy edytuje space nie mam chceckboca czy ba byc by default folded/unfolded i ile
elementow ma wyswietlac (powyzej ktorych bedzie scroll bar)"*

**Problem.** A `_spaces/*.md` carries six readable keys — `space`, `sort`, `icon`, `order`, `default`,
and the name — and neither a fold nor a row cap. Story 49.3 shipped the fold as ONE user-global
setting (`sessions.spaces_folded`) plus a per-space cookie; the owner wants it **per space, in the
space's own file**, which is also where it belongs: a space is a definition on disk, and how it opens
is part of the definition. And the row list is uncapped (`session-spaces.tsx:743`).

**THE TRAP, and it is the whole risk of this story.** `spaces::render_edit` replaces the entire
`keeper:` map on every save (`spaces.rs:600`, `Frontmatter::set_in(source, "keeper", Map(pairs))`).
Any key not threaded through `SpaceEdit` → `render_edit` → `render_new` → `read_one` →
`SessionSpaceVm` **is destroyed the first time somebody presses Save**. Both new keys go through all
five hops in one pass, and there is a test that saving an unrelated edit preserves them.

**Approach.** Two new keys on the space file, both optional, both surfaced in the editor, and a fold
layering that keeps the person's own answer on top.

**The layering, stated once.** For a given space:
1. what the person folded or unfolded **by hand** (the 49.3 cookie) wins;
2. else the space's own `keeper.folded`, if set;
3. else `sessions.spaces_folded`, the user-global setting;
4. else unfolded.
`sessions.spaces_folded` **stays** — it is now the fallback for spaces that say nothing, which is what
a user-global default is for.

**Always.**
- `keeper.rows` caps what the section RENDERS, not what the query SELECTS. Notes' `keeper.limit` caps
  the selection and sessions deliberately omitted it; these are different features and the docs must
  not blur them.
- Over the cap, the remainder folds behind a *Show N more* / *Show less* control — the sync folder
  card's idiom (`sync-pane.tsx:256-274`), not a scrollbar, because a nested scroll area inside a
  scrolling pane is the thing that surface already rejected.
- Both keys are optional; a space file that omits them behaves exactly as today.
- An unreadable value is a WARNING on the space, never an error: the space still works. That is the
  existing severity split (`vm.rs`'s `warnings` vs `error`).

**Block if.**
- `keeper.rows` is not a positive integer → warning, and the cap is ignored.
- `keeper.folded` is not a boolean → warning, and the setting decides.

**Never.**
- Never write a key the editor did not read (the trap above).
- Never let the cap hide the count: the header's number is the whole selection, so a capped section
  still says how much it is not showing.
- Never cap below one row.

**I/O and edge-case matrix.**

| # | input | expected |
|---|---|---|
| 1 | a space with `keeper.folded: true` | arrives folded even with the global setting off |
| 2 | the same, but the person unfolded it by hand | arrives unfolded — the cookie wins |
| 3 | a space with no `folded` key, setting ON | arrives folded |
| 4 | a space with `folded: false`, setting ON | arrives unfolded — the file beats the setting |
| 5 | `keeper.rows: 3` over a 10-file selection | 3 rows plus *Show 7 more*; the header still says 10 |
| 6 | pressing *Show 7 more* | all 10, and *Show less* |
| 7 | `keeper.rows: 0` / `-2` / `"many"` | a warning on the space; no cap applied |
| 8 | `keeper.folded: "yes"` | a warning; the setting decides |
| 9 | editing an unrelated field and saving | `folded` and `rows` survive — the trap test |
| 10 | the editor with a space that has neither key | the checkbox is unset and the cap field is empty; saving writes neither |
| 11 | the editor setting both | the file gains exactly those two keys, in the map's existing order |
| 12 | a restored default space | the seeded definition carries whatever the default says, and nothing else changes |

</intent-contract>

## Code Map

| file | change |
|---|---|
| `keeper-core/src/sessions/spaces.rs` | `SpaceDef`/`SpaceEdit` gain `folded: Option<bool>` and `rows: Option<u32>`; `read_one` parses them with the warning path; `render_edit` **and** `render_new` write them; the seeded defaults carry them where sensible |
| `keeper-core/src/sessions/vm.rs` | `SessionSpaceVm` gains both, so the surface never re-reads the DSL |
| `keeper/src/sessions_ipc.rs` | projection only |
| `src/lib/stores/session-spaces-fold.ts` | `isSpaceFolded` takes the space's own answer as the middle layer; the store's doc states the four-step layering |
| `src/components/sessions/session-space-editor.tsx` | a checkbox and a number field, with the "what this does" line each |
| `src/components/sessions/session-spaces.tsx` | the cap and the *Show N more* control, copying `useListFold`'s shape |
| `docs/sessions.md`, `docs/settings-keys.md` | the two keys, and that `sessions.spaces_folded` is now the fallback |

## Tasks & Acceptance

- [ ] both keys through all five hops, with the preservation test (row 9)
- [ ] the four-step fold layering, with rows 1–4
- [ ] the row cap and its *Show N more*, rows 5–7, header count unchanged
- [ ] editor controls, rows 10–11
- [ ] docs, including the selection-cap vs render-cap distinction

**Acceptance.** The owner can say, per space, that it arrives folded and shows five rows, and pressing
Save on an unrelated field does not lose either answer.

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
