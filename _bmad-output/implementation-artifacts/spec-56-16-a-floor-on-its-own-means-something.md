---
title: 'A floor on its own means something'
type: 'bugfix'
created: '2026-08-29'
status: 'done'
review_loop_iteration: 0
baseline_revision: '84abf87'
final_revision: 'f519dbe'
followup_review_recommended: true
context: []
warnings: []
---

<intent-contract>

## Intent

**Problem:** The owner named a folder `tgdrive-light` and saved `virtualOverBytes: 1 MiB` with
`virtualPatterns: []`, plainly meaning "don't fetch the big files". `VirtualPolicy::resolve`
requires `self.patterns.matches(rela)` before it will answer `Virtual`, and with no permissive
line anywhere the compiled set is empty — so every one of his 16 GB downloads anyway. The form
accepted the setting and its note ("A matched file smaller than this is downloaded anyway")
described a match that never happens. A control that silently does nothing.

**Approach:** Make the floor a selector when nothing else selects: with the permissive set
empty and `virtual_over_bytes > 0`, every LFS path at or above the floor may stay away, under a
new `VirtualPolicyTier::SizeFloor` so the state is nameable rather than inferred. Protections
stay the union of every source and still win unconditionally. Then reword the form's two notes
so the pair is true in all three states, and correct `docs/sync.md` §9.

## Boundaries & Constraints

**Always:**
- The floor authorizes *hydration decisions only* (AD-123, FR-330). Nothing in this story
  deletes, prunes, dehydrates or truncates a byte, and `resolve` still performs no I/O (FR-328).
- A protection wins over a floor-only policy exactly as it wins over a pattern — from
  `.keepervirtual`, from `virtualPatterns`, from either folder TOML layer, unioned (story 56.14).
- Control files (`.keepervirtual`, `.gitattributes`, `.lfsconfig`, `.git/`, `.keeper/`, …) can
  never be virtual, floor or no floor.
- An **unset** `virtualPatterns` is still silence: an empty profile list still leaves the
  committed `.keepervirtual` deciding (the reading DW's "a host cannot decline the policy" entry
  says must not change). The floor selects only when the *effective* permissive set — after that
  precedence has run — is empty.
- `virtualOverBytes: 0` with no patterns stays silent: `tier == Unset`, nothing stays away.
- The form's note and the value it saves are decided by the same predicates, so the sentence and
  the wire cannot drift.

**Block If:** The semantics chosen would need a wire or `SyncProfile` field change. (They do
not: `virtual_over_bytes` already exists on both, so `EXPRESSED`/`PRESERVED` and the
`a_save_cannot_move_a_field_no_request_can_express` fixture are already correct.)

**Never:**
- No refusal path for "a floor with no patterns" — direction (b) is rejected, argued in Design
  Notes; the floor must not become un-saveable.
- No edit to `engine.rs`, `src/git/**` or `src/db.rs` (sibling agent `Story5615` owns them). The
  engine needs none: `authorizes_anything()` and `tier()` already gate it and both now answer
  correctly for a floor-only policy.
- No hand-edit of `src/lib/ipc/gen/**`.
- No change to `lfsMode`/`lfsThresholdBytes` routing. The floor sits above the LFS threshold and
  does not move it.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| The owner's folder | `virtualPatterns: []`, floor 1 MiB, no `.keepervirtual` | 4 MiB path → `Virtual`; 64 KiB path → `Materialize`; `tier() == SizeFloor`; `authorizes_anything()` | No error expected |
| Floor exactly at the boundary | floor 1 MiB, path of exactly 1 MiB | `Virtual` — the floor is inclusive | No error expected |
| Committed protection vs a floor-only policy | `.keepervirtual` = `!30-masters`, `virtualPatterns: []`, floor 1 MiB | `30-masters/a.mov` 4 MiB → `Materialize`; `40-media/a.mov` 4 MiB → `Virtual` | No error expected |
| Profile protection vs a floor-only policy | `virtualPatterns: ["!30-masters"]`, floor 1 MiB | Same two answers; `tier() == SizeFloor` | No error expected |
| Floor never widens an existing zone | `.keepervirtual` = `40-media/**`, floor 1 MiB | `50-iso/big.iso` 4 MiB → `Materialize`; `40-media/a.mov` 4 MiB → `Virtual`; `tier() == PatternFile` | No error expected |
| Zero floor, no patterns | `virtualPatterns: []`, floor `0`, no file | `tier() == Unset`; `!authorizes_anything()`; a 10 MB path → `Materialize` | No error expected |
| Control file under a floor-only policy | floor 1 MiB, `.gitattributes` of 4 MiB | `Materialize` | No error expected |
| Form, no patterns + no floor | box empty, floor box `""`/`0`/`1e` | `SYNC_VIRTUAL_OVER_NONE_NOTE`; save carries `virtualOverBytes: 0` | No error expected |
| Form, no patterns + a floor | patterns box empty, floor `1` | `SYNC_VIRTUAL_OVER_ALONE_NOTE` | No error expected |
| Form, patterns + a floor | patterns box `scans/**`, floor `1` | `SYNC_VIRTUAL_OVER_MATCHED_NOTE` | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/lfs/virtual_policy.rs` -- `VirtualPolicy::compile` gains the
  `floor_selects` decision and the `SizeFloor` tier; `resolve` and `authorizes_anything` consult
  it; `VirtualPolicyTier` gains the variant. All new tests live in the in-file `mod tests`.
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` -- `virtual_over_bytes`' doc comment, which
  today says "a matched path".
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- the same correction on `SyncProfileVm` and
  `SyncProfileReq`. No field added, no wire change; shell crate cannot be compiled here.
- `src/components/sync/add-folder-form.tsx` -- three mutually exclusive floor notes chosen by
  one pure helper; the patterns note gains the "and then the size decides" clause; the
  between-thresholds note.
- `src/components/sync/add-folder-form.test.tsx` -- the three-state test; the existing
  story-56.14 "no floor" test updated to the new sentence.
- `dev/mock-shell.ts` -- a floor-only fixture profile so the harness can show the state.
- `docs/sync.md` §9 -- the floor paragraph, the stale 56.14 override paragraph, and the
  `virtualOverBytes` row of the §3 table.

## Tasks & Acceptance

**Execution:**
- [x] `virtual_policy.rs` -- write the seven failing tests FIRST and record their pre-fix text.
- [x] `virtual_policy.rs` -- add `VirtualPolicyTier::SizeFloor`; compute
  `floor_selects = patterns.is_empty() && profile.virtual_over_bytes > 0` in `compile` and store
  it; make `resolve` answer `Virtual` on it after the protection and control-file gates; make
  `authorizes_anything` include it -- the floor is what says something, so it must participate in
  whether the tier says anything at all.
- [x] `virtual_policy.rs` -- mutate the new condition away and record each test's failure.
- [x] `profile/mod.rs` + `sync_ipc.rs` -- correct the three "a matched path" doc comments -- the
  field's contract changed and its own documentation was the first place the lie lived.
- [x] `add-folder-form.tsx` -- `syncVirtualOverNote(floor, patterns)` plus the three constants and
  the between-thresholds line -- the note said a match was required when no match ever happens.
- [x] `add-folder-form.test.tsx` -- assert each of the three sentences over the real component.
- [x] `dev/mock-shell.ts` -- add the floor-only profile.
- [x] `docs/sync.md` -- §9's floor paragraph and precedence; drop the pre-56.14 claim that a
  `!`-only list installs an empty positive list.

**Acceptance Criteria:**
- Given the owner's stored configuration verbatim and unchanged, when the policy compiles, then a
  4 MiB LFS path resolves `Virtual` and a 64 KiB one `Materialize`.
- Given any protection from any source, when a floor-only policy resolves the path it names, then
  the answer is `Materialize`.
- Given a floor of `0` and no pattern anywhere, when the policy compiles, then `tier()` is
  `Unset` and no path of any size resolves `Virtual`.
- Given the form in each of the FOUR states — no floor; a floor with nothing named; a floor with
  only `!` protections named; a floor with a positive pattern named — when Advanced is open, then
  exactly one floor sentence is on screen, it is the true one, and the other three are absent.
  (Amended from three states in review pass 1: a protections-only box authorizes nothing, so it
  is a fourth state and not an instance of the fourth.)
- Given a floor beside the LFS tracking size, when either sits above the other, then the line
  naming which of the two actually decides is on screen, and never both lines at once.
- Given a path with no named component — `""`, `"."` — when a floor-only policy resolves it, then
  the answer is `Materialize`.
- Given `cargo clippy -D warnings` over the three buildable crates, when it runs, then it is
  clean; Rust tests at or above 3579 passed / 0 failed; frontend green; typecheck clean; lint at
  baseline.

## Spec Change Log

### 2026-08-29 — review pass 1 amendments (sections outside `<intent-contract>` only)

**Triggering findings.** The two reviewers agreed on three defects whose root cause was this
spec's own prescription rather than the implementation's reading of it, and one whose root cause
was an under-stated consequence in the Design Notes:

1. This spec pinned the form's state predicate as `splitSyncList(virtualPatterns).length === 0`.
   That is the SAVE's predicate, not story 56.14's permissive-half predicate that Rust decides
   the same question on — so a patterns box holding only `!30-masters` rendered the *narrowest*
   sentence while the wire put the folder in the *widest* state. Verbatim the fixture of this
   story's own Rust test `a_profile_protection_still_wins_over_a_floor_that_selects_on_its_own`.
   Amended: the predicate counts POSITIVE entries, mirroring `Parsed::of` plus `anchor_line`, and
   a fourth state (protections only, floor selecting the rest) gets its own sentence.
2. This spec pinned the between-thresholds note's gate as `threshold < floor` — the ordering in
   which the ALONE sentence is already true — and left the inverse ordering, the one where it is
   false, unguarded. Amended: one helper returns which of two lines to show, so the two orderings
   can neither both render nor both vanish, and the ALONE/PROTECTED-ONLY sentences claim only
   about files "keeper tracks".
3. This spec's Design Notes enumerated the engine's policy consumers as `tier()` and
   `authorizes_anything()` and stopped there. It never reached `release_mode_gate`, which is the
   function `authorizes_anything()` actually gates, and which decides whether a whole
   `LfsMode::Materialize` folder is refused with `AlwaysMaterializes`. So the change also arms
   the release sweep for a floor-only folder. Amended: recorded below as intended behaviour with
   its argument, and stated in `authorizes_anything`'s own doc and in `docs/sync.md` §9.
4. Same shape for `tier()`: a floor-only folder moves `Unset` → `SizeFloor`, so `verify` stops
   calling its absent objects faults. Amended: stated in the code and the docs.

**Known-bad state avoided.** A form that tells a person their folder is in the narrowest
virtualization state while the engine puts it in the widest — the exact screen-versus-wire drift
the helper was introduced to make impossible — and a story that ships a release-sweep arming
while its own spec asserts nothing in it can lead to a deletion.

**Why patch and not a `bad_spec` loopback.** Findings 1 and 2 are spec-caused, and the workflow
prefers `bad_spec` when in doubt. Taken here they would revert ~900 lines of executed-green Rust
(45 passed / 0 failed in `lfs::virtual_policy`) whose correctness no reviewer disputed, to
re-derive it identically, because two TypeScript predicates were wrong by one clause each and
both have exactly one correct form with no design freedom left. The spec is amended so a
re-derivation would produce the corrected predicates, and the corrections are applied as patches.
That is recorded as a deliberate judgement, not an oversight.

**KEEP instructions — must survive any re-derivation.**
- `floor_selects` computed from the EFFECTIVE permissive set, after 56.14 precedence has run.
  Both reviewers independently confirmed this is what stops the floor widening a zone some source
  already named. Never compute it from `profile.virtual_patterns` directly.
- `VirtualPolicyTier::SizeFloor` as a distinct variant. A regression that stopped the floor
  selecting cannot pass an assertion that names it; reusing `Profile` would let it.
- The order of `resolve`'s gates: frame, control file, size, `never`, then the floor. Both
  reviewers checked it and it is what keeps AD-123 intact.
- `a_floor_of_zero_with_no_patterns_still_says_nothing` and
  `a_floor_never_widens_a_zone_a_pattern_file_already_named`. Neither can fail against the absent
  fix; each kills a specific plausible WRONG fix, and that is why they are worth their lines.

**Behaviour this story does change, stated because the first draft did not.** Making
`authorizes_anything()` true for a floor-only policy arms `engine.rs::release_mode_gate`, so such
a folder's local content becomes eligible for release past its `releaseTtlMs`. That is the point
rather than a side effect: the owner's request is that `tgdrive-light` stop holding 16 GB, and a
folder that may never let go can never become light. AD-123 is untouched because every individual
deletion still proves its case per object — the committed-pointer identity hash, `remote_serves`
at the moment of deletion, the pin read taken twice, and the fail-closed open-file probe. The
only folders whose behaviour changes are those carrying a positive floor and no permissive
pattern from any source: precisely the folders whose floor was a dead control, and `0` is the
default, so a positive floor is always something a person typed.

## Review Triage Log

### 2026-08-29 — Review pass 1
- intent_gap: 0
- bad_spec: 0 (2 spec-caused findings amended in the Spec Change Log and patched in place rather
  than looped back — the judgement is argued there)
- patch: 10: (high 3, medium 6, low 1)
- defer: 1: (medium 1)
- reject: 0
- addressed_findings:
  - `[high]` `[patch]` `syncVirtualOverNote` chose its sentence on list length, so a
    protections-only patterns box rendered the narrowest sentence over the widest state — now
    decided on the positive half via `syncVirtualPositivePatterns`, mirroring `Parsed::of` and
    `anchor_line`, with a fourth sentence for that state and a test quoting the Rust fixture.
  - `[high]` `[patch]` `SYNC_VIRTUAL_OVER_ALONE_NOTE` promised virtualization for files below
    `lfsThresholdBytes`, and the corrective band line was gated on the one ordering where the
    promise was already true — both sentences now claim only about files keeper tracks, and one
    helper returns which of two band lines to show, both fenced by tests.
  - `[high]` `[patch]` `authorizes_anything()` arms `release_mode_gate`, so a floor-only folder's
    content becomes release-eligible; the story asserted the opposite. Stated in the function's
    doc, in `docs/sync.md` §9 and in this spec, with the argument for why it is intended.
  - `[medium]` `[patch]` A floor-only folder moves `tier()` from `Unset` to `SizeFloor`, so
    `verify` stops reporting its absent objects as faults — stated at the `SizeFloor` variant and
    in `docs/sync.md`.
  - `[medium]` `[patch]` `is_inside_the_repository` admitted `""` and `"."`; `resolve`'s new
    `floor_selects ||` short-circuited ahead of `PatternSet::matches`'s empty-candidate guard, so
    the repository root itself resolved `Virtual` under a floor-only policy. Now requires a
    `Component::Normal`, with the test `a_path_with_no_components_is_not_selected_by_a_floor`.
  - `[medium]` `[patch]` `docs/sync.md`'s release-gate paragraph claimed only `pointerOnly` may
    let content go and `materialize` always refuses `AlwaysMaterializes` — false against
    `release_mode_gate` since 56.10, and load-bearing as of this story. Corrected.
  - `[medium]` `[patch]` `docs/sync.md` claimed `verify` is the policy's only consumer. Corrected.
  - `[medium]` `[patch]` The zero-floor claim was unscoped in §9 and in the §3 table ("nothing
    stays away", "the folder is not consulted at all") — false whenever a pattern is in force.
    Scoped to what a zero floor is actually silent about.
  - `[medium]` `[patch]` `SYNC_VIRTUAL_OVER_NONE_NOTE` said a file stays away "only if a pattern
    above names it", contradicting the patterns note one control higher in the default state of
    every seeded folder. Reworded source-agnostically.
  - `[low]` `[patch]` The note branched on the raw MB parse while the save sends
    `Math.round(mb * 1024 * 1024)`, so a floor under ~5e-7 MB rendered "this size decides on its
    own" while the wire carried `0`. The branch now reads the rounded byte count.
  - `[medium]` `[defer]` Gating the floor sentences on `lfsMode` beyond the tracking-threshold
    wording: under `pointerOnly` every tracked file stays away whatever its size, so the floor is
    not the operative control at all. Recorded in the ledger; the residual imprecision is now the
    same class as the pre-existing patterns-note wording rather than a stronger new claim.

## Design Notes

**Why (a) and not (b).** Direction (b) — refuse to save a floor with no patterns — is honest but
wrong here. The owner's instruction is already unambiguous: a size floor is a statement about
size, and the only reason it needed a pattern beside it was an implementation detail of
`resolve`. Refusing it would also make the *widest* useful configuration the one keeper cannot
express, and would break a stored row that already exists on his machine — a load-bearing
profile becoming unloadable is worse than the bug. (a) also keeps AD-123 intact untouched: the
floor still only authorizes a hydration decision and never a deletion, and per-object proof is
still the only thing that lets a byte go.

**The other direction cannot be reached silently.** The two silent readings are now both
impossible: a positive floor with no patterns *does* something (`tier() == SizeFloor`, and the
form says so), and a zero floor with no patterns does nothing *and says so* — the tier is
`Unset` and `SYNC_VIRTUAL_OVER_NONE_NOTE` is on screen. `SizeFloor` is a distinct variant rather
than reusing `Profile` for exactly this reason: `Profile` would claim `virtualPatterns` decided,
which is the empty list, and a regression that stopped the floor selecting could then pass a
tier assertion. It cannot pass one that names `SizeFloor`.

**The floor never widens an existing zone.** `floor_selects` is computed from the *effective*
permissive set, after precedence. So a folder that already names files keeps naming exactly
those, and the floor keeps its old job of holding the small ones back. Only a folder that named
nothing gains a selector — which is the folder that previously had a dead control.

**`lfsMode` and the two thresholds.** His profile is `materialize` with
`lfsThresholdBytes: 262144`, so keeper routes files ≥ 256 KiB through LFS, and the virtual floor
of 1 MiB sits above that. Three bands result:

```
< 256 KiB          not LFS at all — stored in git, always present locally
256 KiB … 1 MiB    LFS-tracked (uploaded to the server) AND kept on this computer
>= 1 MiB           LFS-tracked and a placeholder — fetched when he opens it
```

The middle band is the one he will ask about, and it is the intended shape: the floor is a
*local space* decision, the LFS threshold is a *transport* decision, and they are allowed to
disagree. The form says so in one line, rendered only when a positive floor sits above a
positive threshold under `lfsMode: materialize` — so it can never claim a band that mode does
not have. Gating the three main notes on `lfsMode: disabled` (where nothing is LFS-tracked and
so nothing stays away whatever either box says) is **out of scope**: that lie predates this
story, applies equally to the patterns note, and the mode select sits directly above both.

**Mutation proof — EXECUTED.** The first draft of this section reasoned over the seven tests
because a sibling agent held the shared `target/`. The lock came free after the review pass, so
both Rust mutations were run for real and this records what the runner actually printed. The
reasoning below it is kept because it is what the predictions were, and comparing the two is the
only way to know whether the reasoning was worth anything.

*Mutation 1 — the selector.* `let floor_selects = patterns.is_empty() &&
profile.virtual_over_bytes > 0 && false;`, which restores the pre-story `resolve`
(`self.patterns.matches(rela)` alone), the pre-story tier ladder (no `SizeFloor` arm reachable)
and the pre-story `authorizes_anything`. Result: `test result: FAILED. 41 passed; 5 failed`.

| Test that failed | Printed assertion | left / right |
|---|---|---|
| `the_owners_stored_configuration_keeps_his_large_files_away` | "a file above the floor is what the floor was set to keep away" | `Materialize` / `Virtual` |
| `a_committed_protection_still_wins_over_a_floor_that_selects_on_its_own` | "and it protects only what it names: the floor still selects the rest" | `Materialize` / `Virtual` |
| `a_profile_protection_still_wins_over_a_floor_that_selects_on_its_own` | "and the floor selects everything it did not name" | `Materialize` / `Virtual` |
| `a_control_file_is_never_virtual_under_a_floor_that_selects_on_its_own` | "the fixture really is a floor-only policy" | `Unset` / `SizeFloor` |
| `a_path_with_no_components_is_not_selected_by_a_floor` | "the fixture really is a floor-only policy, or the assertions below would pass with nothing selecting anything at all" | `Unset` / `SizeFloor` |

*Mutation 2 — the frame guard.* `is_inside_the_repository`'s `names_something` initialised to
`true` rather than `false`, i.e. the pre-review behaviour where `""` and `"."` were inside the
frame. Result: `test result: FAILED. 45 passed; 1 failed` —
`a_path_with_no_components_is_not_selected_by_a_floor`, on `"" names no file in the repository, so
there is nothing here for a floor to authorize`, left `Virtual` right `Materialize`. So the
review's finding was a real defect and its test is a real fence, not a restatement.

*Restore verified by reading, not by memory.* After each mutation the file was replaced from a
pre-mutation copy and `git diff --stat` plus `git status --porcelain` were both empty, then
`cargo test -p keeper-sync --lib lfs::virtual_policy` was re-run: `46 passed; 0 failed`.

**How the predictions held up.** The reasoned table said four of the original seven would fail.
The runner failed five, because the fifth is the test the review pass added and the table predates
it; of the original seven it failed exactly the four predicted, on exactly the predicted
assertions and values. The three the table said *cannot* fail did not fail. The reasoning was
accurate, and it was still worth executing: reasoning cannot distinguish "this assertion fails"
from "this assertion fails for the reason I think".

Four of the seven original tests fail without the fix. Three cannot, by construction, and each of
those exists to fail against a *wrong* fix rather than an absent one — recorded as such rather
than counted as coverage they do not provide:

| Test | Without the fix | First failing assertion and the value it sees |
|---|---|---|
| `the_owners_stored_configuration_keeps_his_large_files_away` | **FAILS** | `resolve("40-media/holiday.mov", 4 MiB)`: left `Materialize`, right `Virtual` — "a file above the floor is what the floor was set to keep away". `patterns` is empty (no `.keepervirtual`, `virtualPatterns: []`), so `patterns.matches` is `false` and the size gate above it has already been passed. The three later assertions are never reached; had they been, `tier()` would read `Unset` and `authorizes_anything()` `false`. This is the owner's 16 GB, exactly. |
| `a_committed_protection_still_wins_over_a_floor_that_selects_on_its_own` | **FAILS** | Second assertion, `resolve("40-media/a.mov", 4 MiB)`: left `Materialize`, right `Virtual` — "and it protects only what it names: the floor still selects the rest". The first assertion passes *vacuously* — `30-masters/a.mov` is `Materialize` because nothing was virtual at all, not because `never` won — which is why the second one has to be in the same test. |
| `a_profile_protection_still_wins_over_a_floor_that_selects_on_its_own` | **FAILS** | Second assertion, `resolve("40-media/a.mov", 4 MiB)`: left `Materialize`, right `Virtual` — "and the floor selects everything it did not name". The tier assertion below it is never reached; it would read `Profile` (no file spoke, `from_profile.says_something()` is true on the `!` line) against an expected `SizeFloor` — the precise confusion the separate variant exists to make unpassable. |
| `a_control_file_is_never_virtual_under_a_floor_that_selects_on_its_own` | **FAILS** | First assertion, `tier()`: left `Unset`, right `SizeFloor` — "the fixture really is a floor-only policy". That assertion is there for this reason: the control-file loop under it asserts `Materialize` and would pass vacuously in a policy that virtualizes nothing, so without a tier check the whole test would be dead weight. |
| `a_floor_never_widens_a_zone_a_pattern_file_already_named` | passes | Cannot fail: `.keepervirtual` names `40-media/**`, so `patterns` is non-empty and `floor_selects` is `false` **with or without** the fix — which is the claim. It fails a fix that computed the floor's authority before precedence, or from `profile.virtual_patterns.is_empty()` rather than the compiled set: `resolve("50-iso/big.iso", 4 MiB)` would then read `Virtual` against `Materialize`, a zone no source ever named silently dehydrating on upgrade. |
| `a_floor_of_zero_with_no_patterns_still_says_nothing` | passes | Cannot fail: with `virtual_over_bytes == 0` both spellings of `floor_selects` are `false`. It fails the naive fix `let floor_selects = patterns.is_empty();` — `tier()` would read `SizeFloor` against `Unset`, `authorizes_anything()` `true` against `false`, and `resolve(.., 10_000_000)` `Virtual` against `Materialize`. `0` is the default every profile ever written carries, so that fix is the owner's bug inverted and running on every folder in existence. |
| `a_path_outside_the_repository_is_still_never_virtual_under_a_floor` | passes | Cannot fail: it asserts `Materialize` only, and without the fix nothing is virtual. It fences the frame guard against a fix that answered from the floor before `is_inside_the_repository` — under a floor-only policy there is no pattern to mismatch, so that guard is the *only* thing standing between `/home/u/tgdrive/40-media/a.mov` and a `Virtual` answer, where before it was the second line of defence. |

The honest reading of that table: the story's behavioural claim is fenced by four tests, and the
three that cannot fail here are the ones that constrain the *shape* of the fix rather than its
presence. Two of them (`a_floor_never_widens…`, `a_floor_of_zero…`) each kill a specific plausible
wrong implementation, which is why they are worth their lines.

**The frontend half was mutated for real.** `vitest` needs neither `cargo` nor the shared
`target/`, so it was run: `add-folder-form.test.tsx` is 46 passed / 0 failed. Collapsing
`syncVirtualOverNote` to `return SYNC_VIRTUAL_OVER_MATCHED_NOTE;` — the old unconditional
sentence, which is exactly what the owner was shown — fails **two** tests: the new
`the floor's note is true in each of the three states` and story 56.14's
`says when the size floor is off, for every input that means off`. Restoring the branch returns
both to green. So the pair-cannot-contradict-itself claim is fenced by executed tests, not by
reasoning.

## Verification

**Commands:**
- `cargo test -p keeper-sync --lib lfs::virtual_policy` -- expected: the seven new tests pass;
  each was recorded failing before the change.
- `cargo clippy -p keeper-sync -p keeper-syncd -p keeper-core --all-targets -- -D warnings` --
  expected: clean.
- `cargo fmt --check` -- expected: clean.
- `bun run test src/components/sync/add-folder-form.test.tsx` -- expected: green including the
  three-state test.
- `bun run typecheck` / `bun run lint` -- expected: clean / baseline (4 warnings + 1 info).

**Manual checks (if no CLI):**
- `src-tauri/crates/keeper/src/sync_ipc.rs` cannot be compiled on this host (`gobject-sys`). It
  is doc-comment-only in this story; every touched symbol is reported for the macOS gate.

## Auto Run Result

Status: done. Two commits on `fix/56-15-a-clone-that-stopped-says-so`, not pushed: `866a339`
(the change) and `f519dbe` (the ten review patches). A sibling agent's story 56.15 commit
`363bf25` sits between them on the shared branch; the coordinator ships both stories as one PR.

**What was implemented.** A non-zero `virtualOverBytes` with an empty *effective* permissive
pattern set is now the selector: every LFS path at or above the floor may stay away, reported as
`VirtualPolicyTier::SizeFloor`. Protections and control files still win unconditionally, a zero
floor is still silence, and the floor never widens a zone some source already named. The folder
form's one unconditional sentence became four mutually exclusive ones plus a pair of lines saying
which of the floor and the LFS tracking size actually decides. `docs/sync.md` §3, §8, §9 and §17
were corrected, including two claims that had been false since story 56.10.

**The owner's configuration, unchanged, now does what he meant.** `tgdrive-light` —
`{"lfsMode":"materialize","lfsThresholdBytes":262144,"virtualPatterns":[],"virtualOverBytes":1048576}`
— keeps every file at or above 1 MiB as a placeholder and fetches it when he opens it. Proven by
`the_owners_stored_configuration_keeps_his_large_files_away`, whose fixture is that row.

**Files changed**
- `src-tauri/crates/keeper-sync/src/lfs/virtual_policy.rs` — `floor_selects`, the `SizeFloor`
  tier, `resolve`'s third gate, `authorizes_anything`, the frame guard's named-component rule,
  and eight new tests.
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` — `virtual_over_bytes`' doc; no code change.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — doc text on three fields; no code, field or wire
  change. Committed inside `363bf25` by agreement, since the shell crate's error arm forced that
  file into the sibling's commit.
- `src/components/sync/add-folder-form.tsx` — four floor sentences, two band lines, the
  positive-half mirror, and one byte-count helper shared with the save.
- `src/components/sync/add-folder-form.test.tsx` — three new tests, one renamed and extended.
- `dev/mock-shell.ts` — the `tgdrive-light` fixture, so the state can be looked at.
- `docs/sync.md` — §3 table, §8, §9 and §17.

**Review findings.** 10 patched (3 high, 6 medium, 1 low), 1 deferred, 0 rejected, 0 intent
gaps. Two findings were spec-caused and are argued in the Spec Change Log as patches rather than
a `bad_spec` loopback. The three high findings were worth the whole round: the form's sentence
contradicted the engine for a protections-only list, the floor's promise was false below the LFS
tracking threshold with the corrective line gated on the wrong side of the comparison, and the
change armed the release sweep for folders previously exempt while the spec asserted it could not.

**Verification performed** — every command run on this host, on the final tree:
- `cargo test -p keeper-core -p keeper-sync -p keeper-syncd` → **3596 passed, 0 failed, 1
  ignored** (baseline 3579; the delta is this story's 8 and the sibling story's tests).
- `cargo test -p keeper-sync --lib lfs::virtual_policy` → **46 passed, 0 failed**, all eight new
  tests named in the output.
- Mutation proof, executed twice and restored: neutering `floor_selects` → **41 passed, 5
  failed**; reverting the frame guard → **45 passed, 1 failed**. Both restores verified by an
  empty `git diff --stat` and `git status --porcelain`, then a green re-run. Printed assertions
  and `left`/`right` values are in the Design Notes.
- `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` →
  clean (keeper-sync confirmed re-checked, not cached).
- `cargo fmt --all --check` → clean.
- `bun run test` → **297 files, 4935 tests passed** (baseline 4932 + this story's 3).
- `bun run typecheck` → clean. `bun run lint` → 4 warnings + 1 info, exactly baseline.
- `git status --porcelain -- src/lib/ipc/gen` → empty; no binding drift, and none was possible
  because no wire field changed.

**Not verifiable here, for the macOS gate.** `src-tauri/crates/keeper` cannot link on Linux
(`gobject-sys`), so `bun run check:rust`'s `--workspace` clippy and `bun run test:rust`'s
`cargo-nextest` were replaced by the three-crate equivalents above. This story's symbols in that
crate are `SyncProfileVm::virtual_patterns`, `SyncProfileVm::virtual_over_bytes` and
`SyncProfileReq::virtual_over_bytes` — doc comments only, so a compile is a formality. The
sibling story's `SyncError::CheckoutUnfinished` arm in the same file is load-bearing and has never
been compiled anywhere; the PR body must name it.

**Residual risks.**
- The floor sentences are still not scoped to `lfsMode` beyond claiming only about files keeper
  tracks; under `pointerOnly` the floor is not the operative control. Deferred with the argument,
  because the honest fix scopes all three virtual controls at once.
- A folder carrying a positive floor and no permissive pattern gains a release sweep it did not
  have. Intended and now documented in three places, and the population is exactly the folders
  whose floor was a dead control — but it is a real behaviour change on upgrade, and the release
  path is reachable today only on Linux, where `open_file_state` can answer.
