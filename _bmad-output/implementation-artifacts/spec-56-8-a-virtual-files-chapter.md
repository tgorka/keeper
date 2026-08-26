---
title: 'docs/sync.md grows a virtual-files chapter'
type: 'feature'
created: '2026-08-25'
status: 'done' # draft | ready-for-dev | in-progress | in-review | done | blocked
baseline_revision: 'ea967ad'
final_revision: 'c777281'
review_loop_iteration: 0
followup_review_recommended: true # 28 patches (2 high, 12 medium) across the whole chapter; the patched prose has not itself been reviewed
context: ['{project-root}/docs/sync.md', '{project-root}/docs/decisions.md']
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Epic 56 shipped virtual files across nine stories, and the only durable operator
documentation is scattered inside §12's daemon reference — so the states, the policy precedence, the
refusals, the two clocks, the TTL, the accepted `ls`/`du` cost and the closed Finder question have no
chapter a reader can be pointed at. Worse, four sentences elsewhere in `docs/sync.md` are now false.

**Approach:** Add `## 9. Virtual files` as a sibling of §8 (renumbering §§9–18 to §§10–19 and their
cross-references), covering the nine required points from the shipped code rather than the epic's
plan, and correct the sentences this epic falsified.

## Boundaries & Constraints

**Always:**
- Every symbol name, flag, literal, default and glyph is verified against this worktree's code. Where
  the shipped behaviour diverges from the epic's prose, **document the code** and record the
  divergence in Design Notes.
- Three shipped facts the chapter must state plainly rather than imply otherwise:
  1. `VirtualPolicy` has exactly two production call sites, both inside `Engine::verify`
     (`engine.rs:7778`, `:7860`). `materialize_pending` never consults it. So the policy
     **authorizes** and never instructs: what keeps content away on arrival is `lfsMode = pointerOnly`
     and `subpaths`, and the policy's job is to stop `verify` calling that state a fault.
  2. A malformed pattern is refused **where the policy is compiled** — inside `verify`, contained to
     that folder's line — not by a process-wide startup abort.
  3. `open_file_state` is overridden by **no** shipping platform, so `dehydrate` and the sweep refuse
     `OpenUnknown` on every real host today. §12 already says this; the chapter must not contradict it.
- Prose only. No behaviour change, no engine/UI edit.
- Keep §12's daemon reference authoritative for CLI detail; the chapter cross-references it rather
  than restating it.

**Block If:** a required fact cannot be verified from code (do not write it from the epic's prose).

**Never:** change any `.rs`, `.ts` or `.tsx` file; touch `src/lib/ipc/gen/`; run `gh`; push; fix a
defect found while writing (log it in `deferred-work.md` instead).

</intent-contract>

## Code Map

- `docs/sync.md` -- the chapter's home; §8 at :269-621 is the sibling and the voice to match; §12
  :850-1003 already documents the verbs and the sweep in operator detail; §2 table :66-83; §16 status
  :1189-1199; §18 limitations :1220-1244.
- `docs/decisions.md` -- **D-2** at :33-82 is the Finder/`ls` closure the chapter points at; :98 cites
  `docs/sync.md` §12 and must be renumbered to §13.
- `keeper-sync/src/lfs/virtual_policy.rs` -- `VIRTUAL_PATTERN_FILE = ".keepervirtual"` :68;
  `VirtualPolicy` :131; `compile` :159; wholesale positives :196-201; union protections :207-209;
  `VirtualPolicyTier` :109; precedence diagram :42-44; "a policy edit changes an *answer*, never a
  file" :11-19.
- `keeper-sync/src/exclude.rs` -- the refusal format `invalid {source} pattern {original:?}: {err}` :541.
- `keeper-sync/src/profile/folder.rs` -- `.keeper` :55, `keeper.toml` :58, `keeper.{host}.toml` :249;
  `virtualPatterns`/`virtualOverBytes`/`releaseTtlMs` all `Allowed` :138,:139,:144.
- `keeper-sync/src/profile/mod.rs` -- `virtual_patterns` :881, `virtual_over_bytes` :897,
  `release_ttl_ms` :923; `DEFAULT_RELEASE_TTL_MS` :159, `MIN_RELEASE_TTL_MS` :169,
  `RELEASE_TTL_CEILING_MS` :178; `effective_release_ttl_ms` :1089.
- `keeper-sync/src/lfs/hydrate.rs` -- `ContentRefusal` :72 with `Modified` :153, `Open` :162,
  `OpenUnknown` :175, `UnprovenOnRemote` :189, `Pinned` :198, `AlreadyPointer` :212; sentences :265-331.
- `keeper-sync/src/platform.rs` -- `OpenFileState` :25; `open_file_state` default `Unknown` :134;
  refuse-rather-than-guess doc :100-127.
- `keeper-sync/src/engine.rs` -- `RELEASE_BUDGET_OBJECTS = 32` :400, `RELEASE_BUDGET_BYTES = 1 GiB`
  :418, `RELEASE_LOOK_EVERY_MS = 3_600_000` :434; `release_is_due` :1623; `mark_synced` :3452;
  `release_expired` :6247; `remote_serves` :6702; `release_resolved` :7267; `release_due_at` :8743;
  `ReleaseSchedule` :8786.
- `keeper-sync/src/db.rs` -- `materialized` :148; `ensure_materialized_columns` :292 adding
  `last_used_ms`, `synced_at_ms`, `pinned`, `oid`, `size_bytes`, `local_origin`; `note_synced` :674;
  `note_local_authorship` :631.
- `keeper-sync/src/browse.rs` -- `EntrySyncStatus::Virtual` :307, `Materializing` :330,
  `Materialized` :353.
- `keeper-core/src/vm.rs` -- `FilesSyncStatusVm` :3822; `FilesReleaseVm.releases_after_ms` :4073;
  the `travels` `matches!` :4613-4626.
- `keeper-syncd/src/commands.rs` -- `LsFiles` :281 (`--remote` :290), `Materialize` :318,
  `Dehydrate` :324, `Pin` :343, `Unpin` :355; global `--json` :204.
- `src/components/layout/sync-status-mark.tsx` -- shape-vs-colour doc :5-11; label map :50, icon map
  :71, tone map :104.
- `src/components/layout/files-pane.tsx` -- action labels :311-313; the three gated actions :2202-2241.

## Tasks & Acceptance

**Execution:**
- [x] `docs/sync.md` -- insert `## 9. Virtual files` after §8 (before the `---` at :621-622) covering
      all nine points below -- the durable operator chapter this story exists for.
- [x] `docs/sync.md` -- renumber `## 9`…`## 18` to `## 10`…`## 19` and fix every internal
      cross-reference ≥ §9: :108 (§9→§10), :809 (§11→§12), :1105 (§9→§10), :1106 (§10→§11), :1171
      (§14→§15), :1192 (`§§1–10 and §12`→`§§1–11 and §13`), :1197 (§11→§12), :1232 (§12→§13) --
      a new chapter renumbers the ones after it, and a stale `§N` is a wrong pointer.
- [x] `docs/decisions.md` -- `:98` `docs/sync.md` §12 → §13 -- the only external §-number reference.
- [x] `docs/sync.md` -- §2 profile table: add `virtualPatterns` and `virtualOverBytes` rows with
      their shipped defaults (empty; `0` = no floor) -- the table is presented as the profile's field
      list and epic 56 added two fields to it.
- [x] `docs/sync.md` -- correct the four sentences epic 56 falsified: `:358-360` ("every LFS file
      exists twice"), `:381-382` ("Every file is still there"), `:566-569` ("`git checkout` restores
      real content"), and add the missing half to `lfsPruneLocal`'s condition 2 at `:373-377` (a
      pointer-text path is never a prune candidate, so the two features never contend for one byte).
- [x] `docs/sync.md` -- §16 status: state what epic 56 made real (the policy's verify excuse, the
      listing, the on-demand verb, the row states and the countdown) and what is not reachable on a
      production host (the release, for the `open_file_state` reason).
- [x] `docs/sync.md` -- §18: one bullet for the accepted cost (`ls -l`/`du`/third-party apps see
      ~130 bytes; no filesystem virtualization — D-2).
- [x] `_bmad-output/implementation-artifacts/deferred-work.md` -- log any defect found while writing;
      do not fix it here.

**The nine points, each verified:**
1. **Four row states** — `synced`, `virtual`, `materializing`, `materialized`, with the engine and
   wire enum names, the shipped labels ("Content not on this computer" / "Content arriving" /
   "Content on this computer"), the lucide glyphs (`Check`, `Cloud`, `ArrowDownToLine`, `HardDrive`),
   that shape carries the distinction and tone is emphasis only (four states share `text-faint`),
   that `materializing` is indeterminate with no percentage, and that all three new states **travel**
   in a delete confirmation.
2. **How a path becomes virtual** — `.keepervirtual` in the worktree (never `HEAD`), gitignore
   dialect, `!` protections unioned and positives replaced wholesale, `virtualOverBytes` floor;
   precedence ascending as shipped: `.keepervirtual` < stored profile row < `.keeper/keeper.toml` <
   `.keeper/keeper.<host>.toml`; a malformed pattern refused with the pattern quoted; **editing the
   policy never deletes a byte**; and the honest boundary of what the policy does today.
3. **The refusals** — six typed `ContentRefusal` variants for the five conditions (`Open` splits into
   `Open` and `OpenUnknown`), each a distinguishable typed error rather than a log line, plus the
   platform clause: no shipping platform can answer "is this file open" race-free, so keeper refuses.
4. **Both release clocks** — last use (`last_used_ms`, falling back to `at_ms`) for content that
   arrived from the remote; the sync-confirmed instant (`synced_at_ms`) for content this clone
   authored, `NULL` meaning not eligible at any age; why the second is stricter.
5. **The TTL and its budget** — `releaseTtlMs` default 24 h, per profile and per folder-TOML layer,
   `0` disables, out-of-range refused not clamped; `RELEASE_BUDGET_OBJECTS`/`RELEASE_BUDGET_BYTES`
   with the rotating cursor; the success edge and the hourly look, so a folder that never syncs never
   releases; a sweep failure never fails the sync.
6. **The verbs** — `ls-files [profile] [--remote]`, `materialize`, `dehydrate`, `pin`/`unpin`, global
   `--json`; the app's Materialize / Release / Pin row actions and which state offers which.
7. **What `ls`, `du` and third-party apps see** — ~130 bytes, and why that is the only representation
   `git status` tolerates: the pointer *is* the committed blob.
8. **Finder / `ls` integration is CLOSED** — with all four reasons, pointing at `docs/decisions.md`
   D-2; and no on-read hydration.
9. **One sentence** — automatic release on the success edge is the default; a scheduled or manual
   release is Epic 57's `tasks` verb.

**Acceptance Criteria:**
- Given a reader at `docs/sync.md`, when they read §9, then all nine points are covered with symbol
  names, flags and defaults that match the shipped code.
- Given `git diff`, when the story is done, then only `docs/*.md` and this spec changed — no `.rs`,
  no `.ts`, no `.tsx`.
- Given `grep -oE '§+[0-9]+' docs/sync.md docs/decisions.md`, when the renumber is done, then every
  reference resolves to the section it named before the insertion.
- Given `bun run lint`, `bun run typecheck`, `bun run test`, when run, then each is at the recorded
  baseline (lint: 4 warnings + 1 info; typecheck clean; 297 files / 4916 tests).
- Given `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check`, when run, then clean. No
  cargo test run is needed: no Rust changed.

## Spec Change Log

## Review Triage Log

### 2026-08-25 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 28: (high 2, medium 12, low 14)
- defer: 2: (medium 2)
- reject: 1: (low 1)
- addressed_findings:
  - `[high]` `[patch]` **The `lfsMode` gate was missing from the whole chapter.** Both reviewers found
    it independently. `release_mode_gate` (`engine.rs:8696-8710`) runs on the request door (`:7193`)
    and on the sweep (`:6274-6287`) before any per-path guard, returning `Ok` only for
    `LfsMode::PointerOnly`; `Materialize` — the default — refuses `AlwaysMaterializes` and `Disabled`
    refuses `LfsDisabled`. Six sentences were wrong or under-conditioned as a result: the refusal set
    (now seven refusals in two tiers), the platform paragraph (now conditioned on `pointerOnly`),
    "automatic release is the default", the §2 `virtualPatterns` and `lfsMode` rows, and §17's
    countdown claim. Fixed in all six places; recorded as Design Note 9 so a re-derivation keeps it.
  - `[high]` `[patch]` A higher-tier `virtualPatterns` list containing only `!` protections still
    triggers the wholesale override (`says_something` is `!patterns.is_empty() || !never.is_empty()`,
    `virtual_policy.rs:196`, `:377-379`) and installs an **empty** positive list, un-authorizing the
    committed zone until `verify` reports it as missing objects. The boundary is now stated.
  - `[medium]` `[patch]` The verify excuse is one of four free facts and is switched off entirely for
    `lfsMode = disabled` (`engine.rs:7790-7791`); §9 stated it unconditionally.
  - `[medium]` `[patch]` The hold-word list had four of five reasons; `ReleaseSchedule::LfsOff` added,
    with the note that three of them draw the same word `Kept`.
  - `[medium]` `[patch]` "First sight" is per **run**, not once in a folder's life — `next_release_ms`
    is in memory on the engine (`engine.rs:665`, `:1633-1641`).
  - `[medium]` `[patch]` The fail-closed decline covers any fault in **either** folder layer — parse
    failure, one unknown or refused key, a failed `validate()` — not only unreadability
    (`folder.rs:577-607`).
  - `[medium]` `[patch]` An unreadable (as opposed to absent) `.keepervirtual` is a config error that
    fails the folder's verify, with a different message from the quoted-pattern one
    (`virtual_policy.rs:162-178`).
  - `[medium]` `[patch]` "Every keeper listing … read from the index" was false for the Files view,
    which parses the pointer out of the worktree and only on the rungs it can change
    (`browse.rs:938-960`). Split into the two surfaces.
  - `[medium]` `[patch]` A `releaseTtlMs` between a minute and an hour is accepted but honoured no
    more often than hourly (`MIN_RELEASE_TTL_MS` vs `RELEASE_LOOK_EVERY_MS`); the chapter stated both
    halves and never joined them.
  - `[medium]` `[patch]` `AlreadyPointer` is the one refusal that is not inert, and its two callers
    differ: `dehydrate` refreshes the index stat, the sweep retracts the stale ledger row
    (`engine.rs:6196-6209`, `:7322-7338`).
  - `[medium]` `[patch]` §17's implemented range had been mechanically bumped to `§§1–11`, sweeping §9
    inside a claim its own carve-out bullet contradicts. Range restated as `§§1–8, §10, §11 and §13`.
  - `[medium]` `[patch]` "releasing … from a button is Epic 57's" was wrong — the **Release** row
    action ships today; only the *schedule* is Epic 57's.
  - `[medium]` `[patch]` The 1 GiB per-pass ceiling does not bound rows with no recorded size
    (`row.size_bytes.unwrap_or(0)`, `engine.rs:6331-6336`); such a pass is bounded by the 32-object
    count alone.
  - `[low]` `[patch]` Never-virtual list completed with `.gitattributes`, `.gitignore`, `.gitmodules`
    (`stage.rs:110`), including the reason a virtualized `.gitattributes` is the sharper hazard.
  - `[low]` `[patch]` A punctuation-only policy line (`!`, `/`, `!/`) is dropped silently rather than
    refused (`virtual_policy.rs:332-338`) — the case the chapter's own argument covers least.
  - `[low]` `[patch]` `Modified` is also the answer for a non-regular file, settled by one `lstat`
    before any hashing (`engine.rs:7310-7314`).
  - `[low]` `[patch]` `Pinned` is checked before the two expensive proofs, not "at the top" — three
    refusals precede it.
  - `[low]` `[patch]` The pre-rename guard's tuple degrades to size and mtime off unix
    (`stage.rs:1656-1657`).
  - `[low]` `[patch]` `ls-files --remote` fails the whole command when the server cannot be asked and
    emits no `--json` document at all (`commands.rs:1480-1504`).
  - `[low]` `[patch]` `materialize`'s queued transfer needs a running `watch` or the app; the command
    delivers nothing itself.
  - `[low]` `[patch]` The pin-on-the-wire sentence contradicted the chapter's own hold-word sentence;
    what is absent is a **boolean** a toggle could read, not the fact.
  - `[low]` `[patch]` §9 asserted "eight states" while documenting four. The other four are now named
    in one clause, plus the two consequences a reader gets wrong: only files reach the three new
    states (so a folder of virtual content reads `synced`), and exclusion is decided first (so an
    excluded virtual path draws the local-only delete confirmation).
  - `[low]` `[patch]` §9 stated the exit codes and then named §13 "the authority for all of it";
    delegation narrowed to the wording contract §13 actually owns.
  - `[low]` `[patch]` The quoted delete-confirmation placeholder is the **profile's** name
    (`{profile_name}`, `vm.rs:4654-4656`), not the folder's.
  - `[low]` `[patch]` Coordinator's own pass after the patch agent: removed an unclear
    "both halves of the verb" clause, removed a residual "never the worktree stat" over-claim that the
    next sentence contradicted, deleted one duplicated line, and reflowed three paragraphs.

**Why patch and not bad_spec.** The root cause of the two high findings is a missing *fact* in the
planning fact sheet, not a mis-specified approach: the corrections are additive clauses in identified
sentences, and the other ~350 lines of the chapter were verified correct by both reviewers
(`literals_confirmed` covers every symbol, glyph, quoted sentence, flag, default and the whole
precedence chain). A `bad_spec` loopback would have reverted verified prose to re-derive it, losing
that value for no gain. The missing facts are instead recorded as Design Notes 9–11 so any future
re-derivation carries them.

## Design Notes

**Shipped-vs-planned divergences the chapter documents as code and records here.**

1. **Precedence is not the epic's order.** The epic wrote "pattern file → `.keeper/keeper.toml` →
   `keeper.<host>.toml` → the profile". Shipped (`virtual_policy.rs:42-44`, `folder.rs:244-252`) the
   folder TOML layers outrank the stored profile row, and the per-host file lives **inside** `.keeper`:
   `.keepervirtual` < stored profile row < `.keeper/keeper.toml` < `.keeper/keeper.<host>.toml`.
2. **No startup refusal.** The epic said a malformed pattern "refuses at startup". `VirtualPolicy::compile`
   is called from one production site — inside `verify` (`engine.rs:7778`) — and `keeper-syncd`
   contains the failure to that folder's verify line (`commands.rs:1188-1192`) rather than aborting.
   The pattern is quoted (`exclude.rs:541`), which is the property that mattered.
3. **The policy authorizes; it does not yet gate a download.** `materialize_pending` gates on
   `LfsMode::Disabled`, the `subpaths` cone and `LfsMode::PointerOnly` only. FR-328's word "allowed"
   is exactly right and the module says so (`virtual_policy.rs:13-19`), but a reader would otherwise
   assume the pattern file stops a pull.
4. **The store-trust clause never shipped.** The epic's refusal (3) allowed "the local store holds it
   *and* the profile is configured to trust the store". No such setting exists; `engine.rs:7247-7250`
   records the opposite deliberately. The only local shortcut is a **filesystem remote's own** store.
5. **Five conditions, six typed names.** `Open` split into `Open` and `OpenUnknown`; 56.4's own spec
   records the correction. The chapter names six and says why.
6. **Two extra sweep gates the epic's summary omits:** `RELEASE_LOOK_EVERY_MS = 3_600_000` (so the
   sweep is asked at most hourly, and a profile's **first** successful sync releases nothing) and a
   fail-closed decline when `.keeper/keeper.toml` cannot be read.
7. **Two ceilings, not one:** 32 objects (checked before an attempt) and 1 GiB (checked after), with a
   rotating per-profile cursor so a refusing prefix cannot hide the rest.
8. **`VirtualPolicyTier` ships no wire surface.** The epic wanted a surface showing which tier is in
   force; `tier()`'s only consumer is verify's `excusable` gate. `ConfigTierVm` is an unrelated
   settings-layer type. The chapter therefore does not promise a tier display.
9. **The mode gate is the first refusal, and the planning inventory missed it.** Found by both
   reviewers, verified: `release_mode_gate` (`engine.rs:8696-8710`) runs on the request door
   (`:7193`) and on the sweep (`:6274-6287`) *before* any per-path guard, and returns `Ok` only for
   `LfsMode::PointerOnly` — `Materialize`, **the default** (`profile/mod.rs:82-86`), refuses with
   `AlwaysMaterializes` and `Disabled` with `LfsDisabled`. So `ContentRefusal` carries seven release
   refusals (its own doc counts them that way, `hydrate.rs:65-66`), and "nothing releases on a real
   host" is the platform's answer only in a `pointerOnly` folder; in an ordinary one it is a setting.
   Any re-derivation of this chapter MUST state the gate before the six per-path refusals — without
   it the chapter hands an operator the wrong diagnosis.
10. **The override predicate is `says_something`, not "a positive list".** A higher-tier
   `virtualPatterns` containing only `!` lines still sets `overrides` (`virtual_policy.rs:196`,
   `:377-379`) and installs an *empty* positive list, silently un-authorizing the committed zone.
11. **The Files view does not read the index.** It parses the pointer out of the worktree, and only
   on the rungs whose answer it can change (`browse.rs:938-960`); `ls-files` is the surface that
   reads the index (`lfs/listing.rs:95-100`). The two are not interchangeable in a sentence.

## Verification

**Commands:**
- `bun run lint` -- expected: baseline 4 warnings + 1 info, no new diagnostics.
- `bun run typecheck` -- expected: clean.
- `bun run test` -- expected: baseline 297 files / 4916 tests passing.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check` -- expected: clean (no Rust changed).
- `git diff --name-only` -- expected: only `docs/sync.md`, `docs/decisions.md` and this spec (plus
  `deferred-work.md` if anything was logged).
- `grep -oE '§+[0-9]+' docs/sync.md docs/decisions.md` -- expected: every reference resolves.

**Manual checks:**
- Read §9 against each of the nine points and confirm no sentence asserts something the code does not
  do — in particular nothing claiming the pattern file prevents a download, a startup abort, or a
  release that works on a production host.

## Auto Run Result

Status: done

**Implemented change.** `docs/sync.md` grew `## 9. Virtual files`, a sibling of §8 and the durable
operator chapter for everything epic 56 built: the four row states with the sentences and glyphs the
UI actually uses and the shape-not-colour rule; how a path becomes virtual (`.keepervirtual` in the
worktree, the gitignore dialect as implemented, the `virtualOverBytes` floor, the four-tier ascending
precedence, the quoted-pattern refusal, and — stated plainly — that the policy authorizes and never
instructs); the release refusals in two tiers, the folder's `lfsMode` gate ahead of six typed per-path
`ContentRefusal` variants, with the platform clause that keeper refuses rather than guesses; both
release clocks and which applies when; `releaseTtlMs`, both per-pass ceilings, the success-edge
trigger and its two extra gates; the daemon verbs and the app's row actions; the ~130 bytes `ls` and
`du` see and why the pointer is the only representation `git status` tolerates; the closed
Finder/`ls` question with all four reasons and a pointer at `docs/decisions.md` D-2; and one sentence
placing a scheduled release in Epic 57's `tasks` verb. §§9–18 renumbered to §§10–19 with every
cross-reference, and the sentences epic 56 falsified elsewhere in the manual were corrected.

**Files changed.**
- `docs/sync.md` — new §9 (≈400 lines); §§9–18 → §§10–19 and eight internal `§N` references; §2 gained
  `virtualPatterns` and `virtualOverBytes` rows and a default on `lfsMode`; four falsified sentences
  in §8 corrected; §17 restated; §19 gained the accepted-cost limitation.
- `docs/decisions.md` — one reference, `docs/sync.md` §12 → §13.
- `_bmad-output/implementation-artifacts/deferred-work.md` — two entries (both pre-existing §13
  defects surfaced while auditing §9's claims against it).
- `_bmad-output/implementation-artifacts/spec-56-8-a-virtual-files-chapter.md` — this spec.

**Review findings.** intent_gap 0, bad_spec 0, patch 28 (2 high, 12 medium, 14 low) all applied,
defer 2, reject 1. The two high findings were one systemic fact both reviewers found independently:
a release is gated on `lfsMode` before any per-path refusal, and only `pointerOnly` may let content
go — so the chapter's original "nothing releases on a real host" attributed to the platform was the
wrong diagnosis for the default folder. Six sentences plus two §2 table rows were re-conditioned. The
second high finding was that a higher-tier `virtualPatterns` list of only `!` protections silently
installs an empty positive list. Full breakdown in the Review Triage Log; the facts the planning pass
missed are recorded as Design Notes 9–11.

**Verification.**
- `bun run lint` — 712 files, 4 warnings + 1 info: baseline, no new diagnostics.
- `bun run typecheck` — clean.
- `bun run test` — 297 files / 4916 tests passed: baseline. Run twice, before and after the patch pass.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check` — clean. No cargo test run: no Rust
  changed, and no test reads either document.
- `grep -n '^## ' docs/sync.md` — 19 headings, `1.`…`19.`, no gap or duplicate.
- Every `§N` reference in `docs/sync.md` and `docs/decisions.md` re-checked line by line against the
  pre-insertion set; all resolve to the section they named before. No duplicated lines, no trailing
  whitespace.
- Both reviewers independently confirmed every symbol name, glyph, quoted user-facing sentence, CLI
  verb, flag, exit code, numeric default and the full precedence chain against the shipped code.

**Residual risks.**
- The patched prose has not itself been through a review pass; `followup_review_recommended: true`.
- §9 documents the shipped behaviour, which diverges from the epic's prose in eleven places (Design
  Notes). The most consequential for a reader: the pattern file authorizes but gates no download, and
  releasing requires `lfsMode = pointerOnly` and then still refuses on every real host. The chapter
  says all of this, but it means the epic's own acceptance language ("a folder may declare which paths
  are allowed to stay unmaterialized after a pull") describes an authorization rather than a
  selector — worth the ledger's attention rather than a docs fix.
- The two deferred entries are pre-existing §13 defects. Neither was fixed here: a docs branch scoped
  to §9 should not rewrite §13's exit-code contract, and one of the two has a behaviour-change repair.
