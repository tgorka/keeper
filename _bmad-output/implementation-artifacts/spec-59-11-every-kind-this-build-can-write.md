---
title: 'Story 59.11: every kind this build can write, and a guard that keeps the two lists one list'
type: 'feature'
created: '2026-09-01'
status: 'done'
baseline_revision: 'd3cb68a'
warnings: ['oversized']
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

<intent-contract>

## Intent

**Problem:** The owner, running v0.8.24 from this branch, went to create a task and found two kinds
offered where the build accepts three — *"nowy task - nie ma cli do wybory (jest tylko sync i
release)"*. `TaskKind::from_stored` has accepted `"verify"` since Story 59.9
(`keeper-sync/src/tasks.rs:236-243`); `TASK_KINDS` in `src/lib/stores/sync.ts:66` still reads
`["sync", "release"]`, so the one kind that *checks* rather than moves bytes is creatable from a
terminal and from nowhere else. That is the born-unreachable defect this epic exists to close, and
59.9 wrote the wiring down as a hand-off rather than shipping it, because `task-form.tsx` was being
rewritten by a sibling at the time.

The one-word edit is not the story. 59.9 named the real one in the same paragraph:
*"`TASK_KINDS` is a hand-maintained mirror of `TaskKind::from_stored` with **no mechanical guard**
tying the two together"* (`spec-59-9-…:326-329`). The two lists drifted **within one epic** of the
coupling being written down, and nothing went red. A story that adds `"verify"` and stops has fixed
one instance of a defect whose next instance is already scheduled: Epic 60 adds a kind.

**Approach:** Add the spelling, then make this class of drift impossible in both directions with the
guard shape this repo already trusts — Story 59.7's `every_schedule_the_form_offers_is_one_this_
dialect_accepts` (`tasks.rs:1888-1925`), which is **Rust reading TypeScript**: it extracts the
literals from a `.ts` file and runs them through the real parser, with a floor on how many it found
so a renamed field cannot pass over nothing. Same shape, two directions, and the direction that was
violated here — a kind the build runs that the form does not offer — is the one that must fail
loudly. The picker's option text stays the stored spelling (AD-C7); what a person needs in order to
choose lands where the mode picker already puts it, in one note under the control.

## Boundaries & Constraints

**Always:**
- The option text is the **stored spelling itself**. `task-form.tsx:815-817` states the rule and its
  reason: the row's badge renders `task.kind` verbatim, `tasks list --json` prints this vocabulary,
  and *"two words for one stored value is exactly the drift AD-C7 forbids"*. A prettified label is
  therefore refused, and the note under the control — the `TASK_FORM_MODE_NOTE` idiom, one sentence
  per value, each leading with the spelling — is where a person is told what they are choosing.
- The note's three sentences are **taken, not invented**: they are `TaskKindArg`'s own per-value
  `--help` lines (`keeper-syncd/src/commands.rs:876-884`), which exist for precisely this problem —
  *"a bare `[possible values: …]` list cannot say which of the three a nightly schedule should be
  pointed at"* (`:871-874`) — corroborated by `docs/sync.md:1922-1926`'s kind table.
- `TASK_KINDS`' doc comment survives the edit intact. Its reasoning is still true: no derived union
  type accompanies it, because `db::decode_task` partitions on `kind` and sends every unreadable
  spelling to `TaskListingVm.unknown`, so an editable row's spelling is narrow by construction.
- The guard asserts **set equality**, not containment, and carries a floor on each extraction so
  that two simultaneously-broken extractions cannot agree about nothing.
- A kind that should deliberately never be offerable from the app must be a **named exception** in
  the guard, never a silent omission. See Design Notes: that set is empty today, and the guard says
  so in code.

**Block If:** the guard's second direction cannot be made to fail on a kind added to `from_stored`
and not offered — i.e. the extraction cannot be made to see the real match arms. Report and stop
rather than shipping a guard that only checks the direction that was already safe.

**Never:** add, mention, or build toward an exec/shell/command kind — that is Epic 60's first story,
deferred with an itemised bill agreed with the owner on 2026-08-31
(`ARCHITECTURE-SCHEDULED-TASKS.md:364-369`); add a derived union type for `TASK_KINDS`; render a
prettified option label; touch `src/components/layout/**` (owned by a sibling this wave);
re-implement in TypeScript any rule Rust owns; touch `_bmad-output/planning-artifacts/**`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| The report | the add form's kind picker | three options, values `sync`, `release`, `verify`, in that order | No error expected |
| The report, end to end | pick `verify`, Save | `syncTaskSave` receives `kind: "verify"` and nothing else changed | Rust's refusal shown verbatim, as for any save |
| Additive | pick `release`, Save | `kind: "release"` — unchanged from before this story | No error expected |
| Default | add form, nothing touched | `kind: "sync"`, the first option | No error expected |
| Edit | a stored `verify` task | the picker arrives holding `verify`, not the add form's default | No error expected |
| Copy | the note under the picker | names every spelling in `TASK_KINDS` | fails if a kind is offered with nothing said about it |
| Guard, direction 1 | a spelling in `TASK_KINDS` that `from_stored` refuses | red: "the form offers `…`, which this build cannot run" | test failure |
| Guard, direction 2 | a spelling `from_stored` accepts that `TASK_KINDS` omits | red: "this build runs `…`, which the form does not offer" | test failure — **the direction violated here** |
| Guard, extraction broke | a renamed constant or reshaped match | red on the floor, before any set is compared | test failure |

</intent-contract>

## Code Map

- `src/lib/stores/sync.ts:47-66` — `TASK_KINDS` becomes `["sync", "release", "verify"]`. The doc
  comment gains one paragraph naming the Rust-side guard, so the next reader finds the tie next to
  the list rather than only in a spec.
- `src/components/sync/task-form.tsx:134-135, 815-833` — `TASK_FORM_KIND_NOTE`, a new exported copy
  constant beside `TASK_FORM_MODE_NOTE`, rendered in a `<p>` under the kind picker exactly as the
  mode note is. The picker's markup and its AD-C7 comment are otherwise untouched.
- `src/components/sync/task-form.test.tsx` — three cases: the option list is `TASK_KINDS` in order,
  choosing `verify` sends `kind: "verify"` through `syncTaskSave`, choosing `release` still sends
  `kind: "release"`; plus the note names every offered spelling.
- `src-tauri/crates/keeper-sync/src/tasks.rs` — `mod tests` gains
  `every_kind_the_form_offers_is_one_this_build_runs_and_every_kind_it_runs_is_offered`, beside the
  59.7 guard it is modelled on. Nothing outside `mod tests` changes in this file.

**No generated bindings change, and that is checked rather than assumed.** No `#[ts(export)]` type
and no doc comment on a wire type is touched, so `src/lib/ipc/gen/**` is untouched. `TaskVm.kind`
and `TaskSaveReq.kind` remain `String`. `dev/mock-shell.ts` needs nothing either: its
`sync_task_save` echoes `req.kind` (`:2205`) rather than enumerating kinds.

**No shell-crate symbol is touched**, so the macOS gate has nothing new to link.

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/stores/sync.ts` — add `"verify"`; extend the doc with the guard that now ties the
  list to `from_stored`, leaving the no-union-type reasoning intact.
- [x] `src/components/sync/task-form.tsx` — `TASK_FORM_KIND_NOTE` sourced from `TaskKindArg`'s help
  lines, rendered under the picker; option text unchanged.
- [x] `src-tauri/crates/keeper-sync/src/tasks.rs` — the two-direction guard, with a floor per
  extraction and an explicit, empty, documented never-offered set.
- [x] `src/components/sync/task-form.test.tsx` — the frontend cases above (five as shipped: the
  review added the edit-mode one).

**Acceptance Criteria:**
- Given the app, when a person adds a task, then `verify` is one of the kinds they can choose and
  the save carries that spelling.
- Given a future kind added to `TaskKind::from_stored` and not offered by the form, when the Rust
  suite runs, then it fails and names the missing spelling.
- Given a spelling offered by the form that `from_stored` refuses, when the Rust suite runs, then it
  fails and names it.
- Given the existing two kinds, when they are chosen, then they behave exactly as before.

## Review Triage Log

### 2026-09-01 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 9: (high 2, medium 3, low 4)
- defer: 1: (high 0, medium 0, low 1)
- reject: 6: (high 0, medium 1, low 5)
- addressed_findings:
  - `[high]` `[patch]` Direction 2 recognised an arm only if it contained `=> Some(Self::`, so
    `Some(TaskKind::Verify)` — identical code, which rustfmt does not rewrite — was invisible and the
    exact 59.9 defect would have passed green. The filter now accepts any `"…" =>` line.
  - `[high]` `[patch]` Only the first literal of an arm was captured, so an alternation alias
    (`"verify" | "check" =>`) was a runnable spelling the guard never demanded. Every literal before
    the `=>` is now taken.
  - `[medium]` `[patch]` An acceptance path the line scan cannot read at all — a match guard, a
    `strip_prefix` — was silently absent rather than red. Added: the count of `=> Some(` arms may not
    exceed the spellings read off them.
  - `[medium]` `[patch]` The offered extraction skipped anything that was not a bare literal, so a
    spread or computed entry was an offered kind direction 1 never tested. Added: the array must be
    nothing but quoted literals, commas and whitespace.
  - `[medium]` `[patch]` A second inherent `impl TaskKind` block would be a door outside the
    extraction window. Added: the file may hold exactly one.
  - `[low]` `[patch]` `NEVER_OFFERED` entries were never validated, so a misspelled exemption would
    be inert while the kind it meant to withhold kept failing with advice contradicting the decision.
    Each entry must now name a spelling `from_stored` accepts.
  - `[low]` `[patch]` Order equality pinned the menu's order to the order `from_stored`'s arms happen
    to sit in — a no-op Rust refactor demanding a TypeScript edit. Relaxed to sorted-set equality;
    the claim worth keeping (the first option is the default) is asserted in `task-form.test.tsx`.
  - `[low]` `[patch]` The copy guard asserted a sentence *count*, which any future "e.g." would break
    while the copy stayed correct. Dropped; the per-kind sentence-leading loop is the whole claim.
  - `[low]` `[patch]` The new frontend cases were all add-mode. Added: a stored `verify` arrives in
    the edit form and survives the round trip — mutation-proved against `kind: task.kind`.
  - `[low]` `[defer]` The note's wording is *sourced* from `TaskKindArg`'s `--help` lines but nothing
    mechanically pins it there, so the two can drift into describing the same kind differently.
    Recorded in `deferred-work.md`.

**Rejected, with reasons.** Four findings were false-reds on source *formatting* — a single-quoted
array, extra spacing around `= [`, a type annotation on the declaration, a quoted word inside a
comment in the array. All are loud, self-diagnosing failures on edits biome would not make, and the
literal-only check added above turns the last of them into a message that says what is wrong. The
`>= 2` floors were called a false red on a future single-kind vocabulary: kinds are not removed —
a stored row must keep parsing — so that state does not arise. The cross-crate
`canonicalize().expect(…)` dependency on `src/lib/stores/sync.ts` is the 59.7 precedent's, chosen
deliberately, and is the only direction in which either claim can be checked at all. One finding —
that the test's name says *"the form offers"* while it reads the constant — was answered in the doc
comment rather than by a rename: the doc now names both links of the chain and which test owns which.

## Design Notes

### Why the option text does not become a friendly label

The invocation asked for *"a label a person can act on"*. The obvious reading — put "Verify stored
content" in the `<option>` — is refused by a rule already written at the control
(`task-form.tsx:815-817`) and asserted for the neighbouring picker
(`tasks-pane.test.tsx:2393-2396`): the badge on the row, `tasks list --json`, `tasks set --kind` and
this menu must all say one word, or a person reading their own task list cannot match it to the
thing they chose. The repo's answer to "the spelling is not self-explanatory" already exists one
control down: `TASK_FORM_MODE_NOTE` is one sentence per value under the mode picker. So the kind
picker gets its twin. The wording is lifted from `TaskKindArg`'s `--help` lines because those were
written for the identical problem in the identical vocabulary, which makes the two surfaces agree by
provenance instead of by coincidence.

### The never-offered set exists, and is empty

Every kind `from_stored` accepts is a kind a person may reasonably schedule, so nothing is withheld
today and the guard's exception set is `[]`. It is written down anyway, because the alternative
shape — a kind quietly absent from `TASK_KINDS` — is exactly the state this story is repairing, and
a guard that could be satisfied by editing a list is a guard with a back door. `update` is not a
counter-example: it is refused by `from_stored` itself (`docs/sync.md`'s *"`update` is not a task
kind and never will be"*), so it never enters the set the guard compares.

### The guard, and why it reads two files — and what policing the text costs

Direction 1 could have been checked without reading Rust source at all — `from_stored(literal)`
answers it. Direction 2 cannot: `from_stored` is a function from `&str`, its domain is not
enumerable, and Rust has no dependency-free way to list an enum's variants
(no `strum` in this workspace, `variant_count` unstable). A hand-written `TaskKind::ALL` would have
made the guard a mirror of a mirror — the same hand-maintained coupling one layer deeper, and the
layer that failed. Reading the match arms is the only version of direction 2 that checks rather than
restates, and it is the 59.7 guard's own method pointed at a second file.

The price is that direction 2 compares a **syntactic shadow** of `from_stored` rather than
`from_stored`, and the adversarial review's headline was a list of six ways that shadow could be
green while the two lists disagreed. Five are closed and each has an observed mutation behind it:
an arm spelled `Some(TaskKind::Verify)` instead of `Some(Self::Verify)` (the filter no longer names
a variant path — any `"…" =>` line is an arm); an alternation arm `"verify" | "check" =>` (every
literal before the `=>` is taken, not the first); an acceptance path the line scan cannot read at
all, such as a match guard (the count of `=> Some(` arms must not exceed the spellings read off
them, so an unreadable arm is a red rather than an absence); a non-literal entry in `TASK_KINDS`
such as a spread (the array must be *nothing but* quoted literals and commas, or the guard says so);
and a second inherent `impl TaskKind` block holding another door (the file may have exactly one).

The sixth is not closed here and is honest about it: a kind made runnable by a **trait** impl on
`TaskKind`. There is none today, `from_stored` is the sole parser, and `perform_task`'s exhaustive
match is what would surface one. It is stated at the test rather than papered over.

The order assertion was **relaxed** on review, in the opposite direction to everything else. Pinning
`TASK_KINDS`' order to the order `from_stored`'s arms happen to sit in makes a no-op Rust refactor
demand a TypeScript edit, and the claim worth keeping — *the first option is the default kind of
every task created afterwards* — is asserted where it is visible, in `task-form.test.tsx`'s
`expect(picker).toHaveValue("sync")`. The Rust side compares sorted sets, which still catches a
duplicate.

## Verification

**Commands:**
- `cargo test -p keeper-sync -p keeper-core -p keeper-syncd` — **3846 passed, 0 failed** (baseline
  3845 + this story's one guard).
- `cargo clippy -p keeper-sync --all-targets -- -D warnings` — clean.
- `cargo fmt -p keeper-sync -- --check` — clean.
- `bun run typecheck` — clean.
- `bun run lint` — 4 warnings + 1 info, exit 0 — the measured baseline exactly.
- `bun run test` — **302 files / 5162 tests passed**, twice consecutively.
- `bun run vitest run src/components/sync/task-form.test.tsx` — 50 passed (45 before this story).

**Mutation proof.** Ten inversions, every restore verified by reading `git diff --numstat` (the
three production files stay additions-only but for the one `TASK_KINDS` line) rather than from
memory:

| Mutation | Guard that failed | Observed |
|---|---|---|
| `"verify"` removed from `TASK_KINDS` — the literal Story 59.9 defect | the Rust guard, direction 2 | `this build runs "verify", which the form does not offer …` |
| …and the same mutation, frontend | 3 of the 5 new cases | `expected [ 'sync', 'release' ] to include 'verify'`; the save request assertion; the note's sentence loop |
| `"probe" => Some(Self::Verify)` added to `from_stored` | direction 2 | `this build runs "probe" …` — a genuinely NEW arm, which is the spec's Block-If cleared |
| `"teleport"` added to `TASK_KINDS` | direction 1 | `the form offers "teleport", which this build cannot run` |
| `"probe" => Some(TaskKind::Verify)` (variant path, not `Self`) | direction 2 | named `"probe"` — evasion 1 closed |
| `"verify" \| "check" => Some(Self::Verify)` | direction 2 | named `"check"` — evasion 2 closed |
| `v if v.eq_ignore_ascii_case("PROBE") => Some(…)` | the acceptance-count check | `has 4 arms that accept something but only 3 spellings could be read off them` |
| `TASK_KINDS = [… , ...EXTRA]` | the literal-only check | `holds something this guard cannot read as a plain string literal (", , , ...EXTRA" is left over)` |
| a second `impl TaskKind { … }` appended | the single-block check | `left: 2, right: 1` |
| `NEVER_OFFERED = ["verfy"]` | the exemption check | `NEVER_OFFERED names "verfy", which from_stored does not accept, so it withholds nothing` |
| `<option>` text prettified to `Run a {kind}` | the frontend option-list case | `expected [ 'Run a sync', … ] to deeply equal [ 'sync', 'release', 'verify' ]` |
| the `verify` sentence deleted from the note | the copy guard | the sentence-leading loop |
| `kind: task.kind` → `kind: "sync"` in the edit seed | the new edit-mode case, and 58.1's | an edit form that silently rewrites a check into a sync pass |

**Not run here:** `scripts/check-macos.sh` and anything that links the `keeper` shell crate. This
story touches **no shell-crate symbol**, so the macOS gate has nothing new to link.

**One observation, recorded because it is not attributable.** The first full `bun run test` of the
session reported `1 failed | 5161 passed`; the next two full runs reported `5162 passed` with no
failure and no failing file named. A sibling agent was landing frontend work into the same worktree
at that moment. It is not reproducible and not in any file this story touches.

## Auto Run Result

Status: done

**What was implemented.** The kind picker now offers every kind this build can write, and the two
lists that had to agree are tied together mechanically instead of by a doc comment. `TASK_KINDS`
gains `"verify"` — the one-line wiring Story 59.9 wrote down and deliberately left — and a note
under the picker says what each of the three does, in the CLI's own words, because AD-C7 keeps the
option text at the bare stored spelling. The story's substance is the guard: Rust reading
TypeScript, both directions, with five text-shadow evasions closed after review and the sixth named.

**Files changed**
- `src/lib/stores/sync.ts` — `TASK_KINDS = ["sync", "release", "verify"]`, and the doc now names the
  guard, the drift it is repairing, and the three tokens of the declaration the extraction anchors on.
- `src/components/sync/task-form.tsx` — `TASK_FORM_KIND_NOTE` and its `<p>`, additive only.
- `src-tauri/crates/keeper-sync/src/tasks.rs` — the guard, inside `mod tests`, additions only.
- `src/components/sync/task-form.test.tsx` — five cases: the option list, the `verify` save request,
  the `release` save request, the copy guard, and a stored `verify` surviving an edit round trip.

**Review findings:** 9 patches applied, 1 deferred, 6 rejected, 0 intent gaps, 0 spec loopbacks. See
the Review Triage Log; the deferred item is recorded in `deferred-work.md`.

**Residual risks.** Two, both stated at the test. A kind made runnable through a trait impl on
`TaskKind` is outside the guard's window — there is no such path today. And the guard checks
`TASK_KINDS`, not the rendered menu; the second link of that chain is
`task-form.test.tsx`'s option-list case, which each half's doc now names so neither is weakened in
ignorance of the other.
