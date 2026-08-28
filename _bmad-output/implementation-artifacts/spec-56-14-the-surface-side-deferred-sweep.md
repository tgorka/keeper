---
title: 'Story 56.14 — the surface-side deferred sweep'
type: 'chore'
created: '2026-08-28'
status: 'in-review'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** Epic 56 shipped nine stories plus four follow-ups and left every deferred-work
entry it recorded at `status: open`, so nobody can tell a deliberate trade-off from a defect
nobody looked at — and the browser harness stopped at the first-run at-rest-encryption card,
which meant the Files pane and the folder settings could not be seen at all without a Mac.

**Approach:** Teach `dev/mock-shell.ts` to answer the two commands that decide which screen
boots, then sweep every epic-56 ledger entry whose fix lives in `keeper-syncd`, `docs/`,
`src/` or `keeper-core` to one of three recorded outcomes — fixed with a named test, stale
with quoted proof, or kept with the trade-off and the blocker stated.

## Boundaries & Constraints

**Always:**
- `src-tauri/crates/keeper/**` is untouchable — the Tauri shell crate cannot be compiled on
  this host, so an entry whose fix reaches it stays open with the exact symbol named.
- `keeper-sync` belongs to the sibling agent `EngineSweep`; anything needed there is asked
  for over `hub` and never edited here.
- Rust's sentences are never paraphrased on a surface: the pane renders `detail` verbatim.
- Production behaviour never branches on a prose sentence. A gate reads a token (`hold`), not
  a sentence (`detail`).
- Every fix carries a test that fails without it. Every `stale` carries the code or test that
  disproves the claim.
- The deferred-work ledger is shared: append, or change the `status:` line of an entry this
  story resolved. Never reflow or reorder another entry.

**Block If:**
- A fix requires changing `keeper/src/sync_ipc.rs` or any other shell-crate file, and there is
  no equivalent repair outside it. (Record `keep` and continue — this is not a HALT.)

**Never:**
- No `cargo` or `bun` invocation while the sibling agent is working; the coordinator runs
  every gate once.
- No new IPC command, no new wire field, no `SyncProfileVm` / `SyncProfileReq` change — all
  three live in the shell crate.
- No silencing of a flake and no widening of a guard to make a suite pass.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Harness boot | `bun run dev`, no Tauri shell | `session_restore` answers one `AccountVm` and `encryption_posture` answers `false`, so `App` mounts `<AppShell />` | No error expected |
| Row description | Files/session row with any sync state | The mark's `id` is in the row's `aria-describedby`, so Rust's sentence is spoken | No error expected |
| Release, pinned row | `release.hold === "Pinned"` | Release is absent from the cluster AND the menu; Pin stays; the release cell still carries the word and the sentence | No error expected |
| Release, mode-gated row | `release.hold === "Kept"` (`ModeKeeps` / `LfsOff`) | Release absent; the cell's sentence is the explanation | No error expected |
| Release, TTL off | `release.hold === "Manual"` (`releaseTtlMs = 0`) | Release OFFERED — `release_resolved` has no TTL gate, so it succeeds on request | No error expected |
| Release, unproven | `release.hold === "Not sent"` (`Unconfirmed`) | Release OFFERED — no `synced_at_ms` gate either | Refusal shown verbatim if the remote proof fails |
| Two row verbs in one burst | Press Release on row A, then row B while A is in flight | B is queued, not sent; A's refusal survives B's success; two refusals accumulate as two sentences | Both sentences in the one `role="alert"` sink |
| CLI selector collision | `wanted` is folder X's id and folder Y's name | Resolves to X, exactly one folder | — |
| CLI selector, shared name | Two folders named `media`, neither with that id | Both match; a write verb exits `2` asking for an id | `SyncError::Config` → exit `2` |
| Size-floor box | Box holds `""`, `"0"` or `"1e"` | The no-floor note is on screen and the save sends `virtualOverBytes: 0` | No error expected |
| Verify counts | A HEALTHY folder card | `Check files` is present and reports `checked` / `virtualPaths` | Refusal in the card's error slot |

</intent-contract>

## Code Map

- `dev/mock-shell.ts` -- the harness; gained `session_restore` + `encryption_posture` (the boot
  gate) and a `hold: "Kept"` row so the withheld-Release state can be looked at.
- `src/App.tsx` -- read only. `renderContent`'s gates are what the harness fixture had to satisfy:
  `hydrated`, then `hasAccount`, then `postureChosen === null` → the encryption card.
- `src/components/layout/sync-status-mark.tsx` -- gained an optional `id`, so a row can name the
  mark's sentence in its own description.
- `src/components/layout/files-pane.tsx` -- `FILES_RELEASE_REFUSED_HOLDS`, the Release gate, the
  `syncId` in `describedBy`, and the serialized `runRowVerb`.
- `src/components/sessions/session-tree.tsx` -- the identical `aria-describedby` omission, fixed
  the same way.
- `src/components/settings/sync-section.tsx` -- `Check files` moved from inside the
  needs-attention alert onto every folder card's action row.
- `src/components/sync/add-folder-form.tsx` -- `SYNC_VIRTUAL_OVER_NONE_NOTE` and its render.
- `src-tauri/crates/keeper-syncd/src/commands.rs` -- `select` gained exact-id precedence.
- `docs/sync.md` -- §13's `ls-files` row/JSON split, the `sizeBytes` vs `size` asymmetry, and
  the selector's exit `2`.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- NOT edited here. `ReleaseSchedule::hold` gained
  `Indefinite => "Manual"` on `EngineSweep`'s side, by agreement over `hub`.

## Tasks & Acceptance

**Execution:**
- [x] `dev/mock-shell.ts` -- answer `session_restore` (one `AccountVm`, `satisfies`-annotated)
  and `encryption_posture` (`false`) -- the harness stopped at the first-run card, so no pane
  below it could be seen without a Mac.
- [x] `src/components/layout/sync-status-mark.tsx` -- optional `id` prop -- the mark's sentence
  was reachable only by a reader already inside the row's subtree.
- [x] `src/components/layout/files-pane.tsx` + `src/components/sessions/session-tree.tsx` --
  name the mark's id in each row's `aria-describedby` -- story 56.7's three virtual states were
  drawn and never announced.
- [x] `src/components/layout/files-pane.tsx` -- withhold Release when `hold` is `Pinned` or
  `Kept` -- the press was a guaranteed red alert from a control holding Rust's word for why.
- [x] `src/components/layout/files-pane.tsx` -- serialize `runRowVerb` and clear the sink per
  burst -- a second press erased the first's refusal and then reported `Busy`.
- [x] `src-tauri/crates/keeper-syncd/src/commands.rs` -- exact-id precedence in `select` --
  a selector that is one folder's id and another's name made both permanently unreleasable.
- [x] `src/components/settings/sync-section.tsx` -- move `Check files` onto the card --
  the counts were unreachable on the healthy folder they were added for.
- [x] `src/components/sync/add-folder-form.tsx` -- `SYNC_VIRTUAL_OVER_NONE_NOTE` -- the box
  silently meant "no floor", while the box beside it explained both of its coercions.
- [x] `docs/sync.md` -- §13: split the printed row from the `--json` document, name the
  `sizeBytes`/`size` asymmetry across BOTH producers, document the selector's exit `2`.
- [x] `_bmad-output/implementation-artifacts/deferred-work.md` -- record `fixed` / `stale` /
  `keep` on every entry in this half.

**Acceptance Criteria:**
- Given `bun run dev` with no Tauri shell, when the app loads, then it mounts the shell with an
  account rather than the at-rest-encryption card, and `dev/` type-checks (it is inside
  `tsconfig.json`'s `include`).
- Given a row whose `hold` is `Manual` or `Not sent`, when the cluster and the menu are read,
  then Release is present in both — the gate keys on WHICH word, not on having one.
- Given two epic-56 entries with the same source spec, when the ledger is read, then each
  carries `fixed` with a named test, `stale` with quoted proof, or `open` with a stated
  trade-off and, where applicable, the shell-crate symbol that blocks it.
- Given `keeper-syncd dehydrate <name>` where two folders share that name, when it runs, then it
  exits `2` and `docs/sync.md` §13 says so.

## Design Notes

**The Release gate reads a token, not a sentence.** `FilesReleaseVm` carries
`releasesAfterMs`, `hold` and `detail` and nothing else. A `releasable` discriminant is the
structurally right fix and is impossible here: the struct literal is composed in
`keeper/src/sync_ipc.rs`, which does not build on this host. `detail` would have worked and
was rejected — branching production behaviour on prose means a rewording in Rust silently
re-enables a button that can only fail, with nothing going red. `lfsMode` off `sync_profiles`
does not close it either: `disabled` and `pointerOnly` are decidable, but `materialize` +
`"Kept"` is ambiguous between `ModeKeeps` and `Indefinite` since story 56.10, and
`materialize` is the default. So `EngineSweep` split `Indefinite` out of the shared word:

```rust
Self::Indefinite => Err("Manual"),
Self::ModeKeeps | Self::LfsOff => Err("Kept"),
```

Six characters, so it still fits the `w-16` cell beside `Pinned` and `23 hr`. The pane then
withholds on `["Pinned", "Kept"]` and nothing else.

**The row verbs are serialized rather than guarded.** Dropping a second press silently is the
anti-pattern the pane's own comments warn about, and disabling the controls needs a `disabled`
field on `FilesRowAction` that both the cluster and the menu would have to honour. Chaining is
smaller and strictly better: both verbs run, in press order, each against a folder the
previous one has finished with — which is what makes the spurious `SyncError::Busy`
unreachable rather than merely unlikely. The sink then clears once per burst and accumulates,
the shape `requestDelete`'s multi-path receipt already uses.

**Withheld, not disabled.** The pane's standing convention (`reveal`, the create control, the
delete confirmation's own button) and the right one here: the release cell on the same row
draws the word and speaks Rust's sentence, which explains more than a disabled button's
tooltip.

## Verification

**Commands** (run once by the coordinator, after both agents land — this session ran none, by
instruction, because `cargo` and `bun` share one target dir and one `node_modules` with the
sibling agent):
- `bun run check` -- expected: biome + tsc + vitest clean. Baseline 297 files / 4925 tests,
  typecheck clean, lint 4 warnings + 1 info. `dev/mock-shell.ts` is inside `tsconfig.json`'s
  `include`, so the `satisfies AccountVm[]` fixture is checked here.
- `bun run test:rust` -- expected: the two new `keeper-syncd` selector tests pass; baseline
  3553 passed / 0 failed.
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `-D warnings` clean across
  the three buildable crates.

**Named tests, one per fix:**
- `files-pane.test.tsx` > `names each row's sync mark in the row's own description` (3620)
- `session-tree.test.tsx` > `carries the Files tab's own sync mark and sentence` (3620, second
  caller)
- `files-pane.test.tsx` > `withholds Release exactly where Rust's word says the request cannot
  succeed` (3635)
- `files-pane.test.tsx` > `draws a word, no digit and no timer for a row on no clock` — now
  over all five words, including the `Manual`/`Kept` split (3640)
- `files-pane.test.tsx` > `keeps the first verb's refusal and does not run the second beside
  it` and > `shows both sentences when two verbs in one burst are refused` (3655)
- `commands.rs` > `an_id_that_is_also_another_folders_name_resolves_to_the_id` and >
  `two_folders_sharing_a_name_still_both_match_that_name` (3550)
- `sync-section.test.tsx` > `offers the check on a healthy folder and reports its counts there`
  and > `keeps exactly one check control on a flagged folder` (3731)
- `add-folder-form.test.tsx` > `says when the size floor is off, for every input that means
  off` (3735)

**Manual checks:**
- `bun run dev`, then look: the app lands in the shell with an account. The Files pane's
  `raw-2026-08.tiff` row reads `Kept` and offers Pin but not Release; `master-2026-08.wav`
  reads `Pinned` and does the same; `master-2026-07.wav` counts down and offers both.
- The harness boot was verified by READING `src/App.tsx`'s `renderContent` gates rather than by
  running vite, per the assignment: `!hydrated` → splash; `wizardActive` → wizard (not reached,
  because the boot effect starts the wizard only `if (!hasAccount)`); `!hasAccount` →
  `postureChosen === null` → `<AtRestEncryptionChoice>`. One restored account makes `hasAccount`
  true, so the whole `!hasAccount` block is skipped and `<AppShell />` mounts. `encryption_posture: false`
  is still load-bearing: the posture is read unconditionally on mount, and a hand-driven
  sign-out under the harness must land on the login screen rather than back on the card.

## Auto Run Result

Status: in-review

Fixed 8 entries, marked 3 stale with quoted proof, kept 6 with stated trade-offs (5 of them
blocked on `keeper/src/sync_ipc.rs`, which cannot be compiled on this host). One engine-side
word change (`ReleaseSchedule::Indefinite => "Manual"`) was agreed with `EngineSweep` over
`hub` and made in `keeper-sync` by that agent; every consumer of the word is updated here.

Left for the shell crate, by symbol:
- `sync_browse` (`keeper/src/sync_ipc.rs`) — two unfiltered `materialized` scans per listing.
- `sync_release_entry`'s doc comment (`keeper/src/sync_ipc.rs:2518-2519`) — still says no
  platform can answer the open-file question, false on Linux since story 56.11.
- `SyncProfileVm` / `SyncProfileReq` (both defined in `keeper/src/sync_ipc.rs`) — the ADD-path
  `folderOwned` gap, the three bare request slots, and the double folder-overlay derivation.
- `Engine::dehydrate_entry` / `sync_release_entry` — the whole-file hash inside an `async fn`.
