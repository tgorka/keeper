---
title: '56.6 The checks stop calling the normal state a fault'
type: 'bugfix'
created: '2026-08-25'
status: 'done' # draft | ready-for-dev | in-progress | in-review | done | blocked
baseline_revision: '2629949f111dcf4deed2ed2d8719130f47f1893c'
final_revision: 'e42062d'
review_loop_iteration: 0
followup_review_recommended: true
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Epic 56 created a state its own checks read as damage. `Engine::verify` pushes every unmaterialized LFS path into `report.bad` on `!store.contains(...)` alone (`engine.rs:7725-7729`), so one policy over 10 000 paths is 10 000 failures and a non-zero exit for a cron wrapper — NFR-41 inverted. `copy.rs` is LFS-blind (`classify`, `copy.rs:805`): it copies ~130 bytes of pointer text, hashes them twice, and reports `CopyOutcome::Copied` with a matching digest — a *verified* copy of nothing, onto the pendrive that was supposed to be the second copy. And `republish_missing_objects` re-cleans the worktree file back into the store (`engine.rs:7638`) and compares the pointer text's own sha to the oid it stands for: a comparison that can never match, reached by writing a junk object into `<git-dir>/lfs/objects` to ask the question.

**Approach:** Correct all three where they are, from facts already in the tree, adding no vocabulary. The **index** says which paths carry a committed pointer, the compiled **`VirtualPolicy`** says which of those are authorized to stay away, and the **remote half stays untouched** — `audit_remote_objects` → `lfs::audit::serves` is the only half that can prove loss, and it is where the exit code already accounts for it. So `verify` earns silence from two local facts and reports everything else; a verified copy of a virtual path hydrates through `Engine::materialize_entry` when the caller hands the copy an engine and otherwise refuses by a `ContentRefusal` sentence; and the re-clean is gated by `stage::worktree_pointer` before `stage::clean` can write anything.

## Boundaries & Constraints

**Always:**
- **The excuse is earned by two local facts, never one.** A pointer-without-object path is excused only when (a) the **index** carries a pointer for that path whose `oid` **and** `size` equal the worktree pointer's, and (b) the compiled policy answers `Virtualization::Virtual` for that repository-relative path at the pointer's declared size. `VirtualPolicy::resolve`'s own doc is the reason (`virtual_policy.rs:250`): *"a `Virtual` answer is an authorization, never an instruction"* — the authorization selects which paths *may* be excused; the index is what proves the bytes on disk are the committed pointer and not a stray pointer-shaped file a user dropped there. Either fact alone is a guess.
- **`verify` stays the half answerable without a network**, and the refusal to make it otherwise is load-bearing. `Engine::remote_serves` (`engine.rs:6690`) is one batch round trip **per object**, so taking the remote proof per path inside `verify` would answer NFR-41's own fixture — a folder with 10 000 virtual paths — with 10 000 round trips, and would break the documented contract at `docs/sync.md:316-317`. The proof belongs where it is already batched, index-driven and paid for once: `Engine::audit_remote_objects` behind `--remote`, whose `missing_total` has its own non-zero exit gate (`commands.rs:1260-1267`).
- **Everything unproven stays reported.** No index entry, no repository, an unreadable index, a policy that answers `Materialize`, a pointer whose oid the index does not carry, or an `oid`/`size` disagreement — each of those leaves the row in `bad`, word for word as today (`"LFS object {oid} is missing locally"`). The failure this story must not create is a check that went quiet.
- **A policy that cannot be compiled fails the verb, and never excuses anything.** `VirtualPolicy::compile` is `Result` and its refusal is `SyncError::Config` quoting the line (FR-329); `verify` propagates it. This is 56.5's `folder_config_is_faulted` direction restated: never read a permissive default out of a file the reader could not parse. `compile` runs **once** per `verify` call, before the walk, and is moved into the blocking closure — the struct's own doc forbids compiling per path (`virtual_policy.rs:126-129`). This is the **first production consumer** of `VirtualPolicy` (56.1's recorded deferred item).
- **`VerifyReport` gains exactly one field, `virtual_paths: u64`,** counting paths excused by both facts. `checked` keeps counting every file including those; `bad` keeps its meaning and its `(path, reason)` shape, so the desktop `sync_verify` command (`sync_ipc.rs:1573`) and the scheduler's `WorkKind::Verify` arm need no change. The daemon renders the new count in both forms because a suppressed row that is nowhere reported is indistinguishable from a check that stopped running.
- **The copy's default answer is a refusal by name, and hydration is a capability the caller supplies.** `copy.rs` gains one object-safe seam, `pub trait ContentSource { fn materialize(&self, absolute: &Path) -> Result<()>; }`, threaded as `Option<&dyn ContentSource>` through `copy_verified` → `copy_verified_hooked` → `plan_copy` → `classify`. `None` refuses every pointer path. This keeps `copy.rs`'s doc premise intact (`copy.rs:4-6`, "no profile, no journal, no state") and keeps AD-128's prohibition where it belongs: the module cannot hydrate on its own, and only an explicit user-initiated verb can.
- **`Engine` implements that seam, so the path→profile rule lives in the crate that compiles here.** `impl copy::ContentSource for Engine` resolves the absolute path against `db::list_profiles`' longest matching `local_path`, spells the remainder with `/`, and calls the existing `Engine::materialize_entry(&profile.id, &subpath)` — which is `pub fn`, blocking by contract (`engine.rs:6890-6893`), so no async enters `copy.rs`. No profile contains the path ⇒ `ContentRefusal::NotTracked`. The shell's only job is to hand the copy the engine it already has.
- **`MaterializeOutcome::Queued` is a refusal, not a success.** `Queued` means the object is not on this machine and a download unit was enqueued (`engine.rs:7057-7063`); a copy cannot wait for it, and treating it as hydrated is exactly the silent-pointer-copy bug in a new place. `Materialized` and `AlreadyMaterialized` are the only successes.
- **The pointer branch sits at plan time, in `classify`, beside the dataless-placeholder refusal it is the sibling of** (`copy.rs:824-831`) — the same idiom, the same `PlanItem::Refused { rel, reason }`, the reason string being the `ContentRefusal`'s own `Display` sentence (`hydrate.rs:237-241`), which is the sentence `keeper-syncd materialize` already prints. `stage::worktree_pointer(absolute, &meta)` costs nothing for an ordinary file and reuses the `Metadata` `classify` already binds (`copy.rs:806`). **After a successful hydration the file is re-stat'd** before `PlanItem::File { bytes }` is minted, or the plan's totals become a lie and `the_pre_walk_totals_match_what_the_report_accounts_for` (`copy.rs:1452`) says so.
- **The re-clean gate is placed so that `clean` is never reached, not so that its answer is ignored.** `stage::clean` streams the file into the store (`stage.rs:760`), so a gate after the call would still have deposited a junk object under the pointer text's own sha. The check is `stage::worktree_pointer` on the worktree file before the `clean` arm at `engine.rs:7638`; a path whose worktree bytes are the pointer naming this very object is classified `unrecoverable` **without** the read, the write or the comparison. The disposition is unchanged: `report.unrecoverable`, `continue`, one warning per object — because an object the server lacks and this machine does not hold **is** real loss, whatever the reason the bytes are not here.
- **The remote half is not touched at all.** No change to `audit_remote_objects`, `lfs::audit::{serves, report, RemoteAudit, MissingObject}`, `remote_serves`, or the two exit gates. `verify --remote` finding a pointer the server cannot serve is asserted by this story's tests and implemented by nobody in it — that is the point.
- **Durable docs that become false are corrected in the same change:** `docs/sync.md:316-317` (`verify` now excuses an authorized virtual path and says how many), the `sync_verify` doc sentence (`sync_ipc.rs:1566-1572`), and `SYNC_VERIFY_CLEAN_SENTENCE` (`sync-section.tsx:105-106`), which today promises "every large file's stored copy is present" — the sentence DW-1272 exists to keep honest. The virtual-files *chapter* remains 56.8's.

**Block If:**
- The distinction is judged to require a per-path remote round trip inside `Engine::verify`. That contradicts NFR-41's own fixture and the offline contract at `docs/sync.md:316-317`; the batched proof behind `--remote` is the epic's answer and a second one may not be invented unattended.

**Never:**
- No new crate, no new dependency, no new `SyncProfile` field (therefore **no** `sync_ipc.rs` `EXPRESSED`/`PRESERVED` classification and no `prior`-fixture value to add), no `meta` migration, no ledger column.
- No new Tauri command and no ts-rs type: `git status --porcelain -- src/lib/ipc/gen` stays empty and `src/test/command-registration.test.ts` is untouched.
- No `VirtualPolicy` cached on `Engine` and no policy consulted by `lfs::listing` — its module doc (`listing.rs:17-24`) states why the listing is disk-state and not policy, and that stays true.
- No new refusal enum: one variant joins `ContentRefusal`, per the standing rule at `hydrate.rs:16-24`.
- No change to `copy.rs`'s verification arithmetic, its progress, its log format, or `CopyOptions`' shape; no hydration triggered by anything other than a caller-supplied `ContentSource`.
- No `ls-files` change, no `dehydrate`/release change, no `prune` change, no `docs/sync.md` virtual-files chapter (56.8), no Files-row or delete-plan work (56.7).
- No test that sleeps, and no test that asserts the absence of a complaint by counting one path — the absence is asserted over a fixture of many.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Many virtual paths | 1 000 committed pointers, objects absent locally, all matched by `.keepervirtual` | `verify`: `bad` is **empty**; `virtual_paths == 1000`; `checked` counts them | No error expected |
| The unredeemable path beside them | one committed pointer in the same fixture that the policy answers `Materialize` for, object absent | that one path is the **only** `bad` row, reason unchanged | exit 1 from the daemon |
| Policy says virtual, index says nothing | a pointer-shaped file the index does not carry, path matches the patterns | reported in `bad` — an unauthorized guess is not an excuse | exit 1 |
| Policy says virtual, index carries a different oid | worktree pointer's oid ≠ the committed pointer's | reported in `bad` | exit 1 |
| No repository | folder with no `.git` | nothing is excused; every pointer-without-object stays `bad` | No error expected |
| Malformed `.keepervirtual` | one unparsable glob line | `verify` refuses before the walk, quoting the line | `SyncError::Config`, exit 1 |
| Materialized path | worktree holds the content, object in the store | untouched: `checked` only, as today | No error expected |
| Torn read | a plain file changing under the read | `bad`, unchanged (`verify_while_reading`) | No error expected |
| `verify --remote`, real loss | a policy-virtual path whose object the remote store does not hold | still reported by `audit_remote_objects` in `missing`; `missingOnRemote > 0` | exit 1 |
| `verify --remote`, intact | every object on the remote | `bad` empty, `missing` empty, `virtual_paths` reported | exit 0 |
| Verified copy, engine supplied | source tree with one virtual file whose object **is** in the local store | the file is hydrated then copied; destination bytes equal the content byte for byte; `CopyOutcome::Copied` with the content's digest | No error expected |
| Verified copy, no engine | same tree, `content: None` | that entry is `CopyOutcome::Failed` naming the pointer; the ordinary files beside it still copy; **no ~130-byte file at the destination** | refusal, job succeeds |
| Verified copy, object not here | virtual file whose object is in neither store | `materialize_entry` answers `Queued` ⇒ that entry refuses by name | refusal, job succeeds |
| Verified copy, path outside every profile | pointer-shaped file under no folder keeper syncs | refuses by name (`NotTracked`'s sentence) | refusal, job succeeds |
| Verified copy, folder busy | the owning profile is mid-sync | `SyncError::Busy`'s sentence becomes that entry's reason | refusal, job succeeds |
| Verified copy, plan totals | tree with one hydrated file | the report's `bytes` for it is the **content's** length, and the pre-walk total matches | No error expected |
| Re-clean, virtual path | `verify --remote --repair` over a virtual path the server lacks | listed `unrecoverable`, one warning; **the LFS store gains no object**, and the store's object count is unchanged | No error expected |
| Re-clean, real content | the machine that authored the content, object pruned from the store | re-cleaned and queued for upload exactly as today | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` — `VerifyReport` `:188-193` (**gains** `virtual_paths: u64`); `Engine::verify` `:7688-7753`, the blocking closure `:7695-7743` (captures only `root` today), the store `:7697`, **the branch to correct** `:7718-7734` with the condition at `:7725` and the push at `:7726-7729`; `read_head` `:8560`, `display_relative` `:8572`; `republish_missing_objects` `:7611-7685`, **the single `stage::clean` call site** `:7638` inside the `have` expression `:7634-7649`, the unrecoverable disposition `:7650-7653` and the warning `:7674-7683`; `audit_remote_objects` `:6566` and `remote_serves` `:6690` (**both unchanged**); `materialize_entry` `:6893-7109` — the `Queued` return `:7057-7063`, `AlreadyMaterialized` `:7006-7012`, `Materialized` `:7102-7108`, the reservation `:6955-6957`; `git::repo::open` usage precedent `:6976`. **Gains** `impl crate::copy::ContentSource for Engine`.
- `src-tauri/crates/keeper-sync/src/lfs/virtual_policy.rs` — `VirtualPolicy::compile(&SyncProfile) -> Result<Self>` `:160`; `resolve(&self, rela: &Path, size: u64) -> Virtualization` `:253` and its authorization-not-instruction doc `:250`; `Virtualization` `:90-96` (`Copy + PartialEq`); the compile-once doc `:126-129`; `VIRTUAL_PATTERN_FILE`. **Not modified.**
- `src-tauri/crates/keeper-sync/src/lfs/stage.rs` — `worktree_pointer(absolute, &Metadata) -> Option<Pointer>` `:955` (the documented "are these bytes the committed pointer" question, no repo needed); `indexed_pointers(&gix::Repository) -> BTreeMap<String, Pointer>` `:1009` (one index read for a whole walk); `index_key` `:784` (`pub(crate)`, git's spelling — the map's key); `clean` `:753-771` and its `insert_streaming` write `:760`. **Not modified.**
- `src-tauri/crates/keeper-sync/src/copy.rs` — module doc `:1-55` (the sync contract `:53-55`, the refusal rationale `:47-49`); `copy_verified` `:189-197`; `copy_verified_hooked` `:210` and its plan call `:221`; `PlanItem` `:697-706`; `plan_copy` `:714` and its two `classify` calls `:729`, `:781`; the totals fold `:784-794`; **`classify` `:805`**, its `meta` `:806`, the symlink refusal `:809-817`, the non-regular refusal `:818-823`, **the dataless refusal `:824-831`** (the sibling to copy), the `PlanItem::File` mint `:832-835`; `copy_one` `:307` and its re-stat `:318`; the verification comparisons `:456-465`, `:474-484`; inline tests `:1056`, `source_tree` `:1086`, `entry` `:1101`, the symlink-refusal test to model `:1504-1530`, the totals test `:1452`. **Gains** `ContentSource`, one `classify` arm and the threaded parameter.
- `src-tauri/crates/keeper-sync/src/lfs/hydrate.rs` — `ContentRefusal` `:72` and the extend-don't-fork rule `:16-24`; `NotTracked` `:117`, `UnprovenOnRemote` `:189`, `AlreadyPointer` `:208`; `Display` `:237`; `MaterializeOutcome` `:336`. **Gains** one variant for "this path holds only its pointer here".
- `src-tauri/crates/keeper-sync/src/db.rs` — `list_profiles` `:1090` (returns in-force profiles), `get_profile` `:1107`. **Not modified.**
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `Command::Verify` `:257-273`; `cmd_verify` `:1175-1268`, the human line `:1191-1196`, the JSON entry `:1200-1208`, the remote half `:1210-1245`, the repair call `:1229`, the two exit gates `:1254-1267`. **Gains** the `virtual` count in both renderings; the exit gates are untouched.
- `src-tauri/crates/keeper/src/copy_ipc.rs` — `copy_start` `:242-317`, the `CopyOptions` build `:265-267`, `spawn_blocking` `:272`, the `copy_verified` call `:277-278`. **Gains** the engine handoff (shell crate — macOS gate).
- `src-tauri/crates/keeper/src/sync_ipc.rs` — `sync_verify` `:1573-1583` and its doc `:1566-1572`; `engine_of` `:1063`, the move-the-`Arc`-into-`spawn_blocking` precedent `:2332-2337`. **Doc sentence only.**
- `src/components/settings/sync-section.tsx` — `SYNC_VERIFY_CLEAN_SENTENCE` `:105-106`, used `:596`. **Sentence only.**
- `docs/sync.md` — the verify section `:314-327`. **Two sentences.**
- `src-tauri/crates/keeper-sync/tests/dehydrate_entry.rs` — the harness to copy: `init_repo` `:64-71`, `profile` `:82`, `CONTENT_BYTES` `:57`, the filesystem-remote store seeding and the per-object 404 by deleting one object `:562-563`. `tests/virtual_policy.rs:80-101` — the committed-pointer fixture; `:17` `VIRTUAL_PATTERN_FILE` seeding. **New file** `tests/virtual_state_is_not_a_fault.rs`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` — add `VerifyReport::virtual_paths`; compile the policy once before `verify`'s closure and move it in with `root`, `removable` and the index map (`git::repo::open(...).ok()` → `stage::indexed_pointers`, defaulting to empty so a folder with no repository excuses nothing); replace the bare `!store.contains(...)` push with the two-fact excuse; document why the remote proof is not taken here.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` — gate the `stage::clean` arm in `republish_missing_objects` behind `stage::worktree_pointer`, so a path holding the pointer that names this object is `unrecoverable` without a read, a store write or a comparison.
- [x] `src-tauri/crates/keeper-sync/src/lfs/hydrate.rs` — add the one `ContentRefusal` variant and its `Display` sentence, written for the person who asked for the copy.
- [x] `src-tauri/crates/keeper-sync/src/copy.rs` — add `pub trait ContentSource`; thread `Option<&dyn ContentSource>` through `copy_verified`, `copy_verified_hooked`, `plan_copy` and `classify`; add the pointer arm after the dataless refusal (hydrate-then-re-stat, or refuse by name); extend the inline tests with a fake `ContentSource` for both branches and one asserting the plan totals after a hydration.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` — `impl copy::ContentSource for Engine`: longest-prefix profile resolution over `db::list_profiles`, `/`-joined remainder, `materialize_entry`, `Queued` mapped to the new refusal.
- [x] `src-tauri/crates/keeper-sync/tests/virtual_state_is_not_a_fault.rs` — **new**; the many-virtual fixture with one unredeemable path beside it, over a real repository, a real index and a real filesystem remote whose LFS store is seeded per object: verify's silence and its surviving report, the index and policy guards, `verify --remote` still finding loss, the store gaining no object from a repair pass, and a real end-to-end hydrating copy through `Engine`.
- [x] `src-tauri/crates/keeper-syncd/src/commands.rs` — render `virtual_paths` in the human line and the JSON entry; leave both exit gates untouched; extend the parse/render assertions that exist.
- [x] `src-tauri/crates/keeper/src/copy_ipc.rs` — obtain the process-wide engine and pass it as the copy's `ContentSource` (shell crate; report for the macOS gate).
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs`, `src/components/settings/sync-section.tsx`, `docs/sync.md` — correct the three sentences that become false.

**Acceptance Criteria:**
- Given a folder whose policy leaves 1 000 committed pointers virtual, when `verify` runs, then it reports no errors and no warnings for that fact and names how many paths are virtual.
- Given that same folder also holds one pointer the policy does not authorize whose object is absent, when `verify` runs, then that path is the only row reported and the daemon exits non-zero.
- Given a virtual path whose object the remote does not hold, when `verify --remote` runs, then it is reported and the exit code is non-zero.
- Given a verified copy of a virtual file, when the copy runs with an engine, then the destination holds the file's real bytes; when it runs without one, then the entry is refused by name and no ~130-byte file reaches the destination.
- Given a repair pass over a virtual path the server lacks, when it runs, then the LFS store gains no object and the path is still reported as beyond this machine.

## Spec Change Log

### 2026-08-25 — two implementation-shaped sentences superseded by the review, without a loopback

- **Triggering findings.** (a) Plan-time hydration was found to be unbounded, uncancellable, invisible and to run before the destination is proven writable: `classify` hydrated every pointer path during the pre-walk, where `cancel` is never read, no progress frame has been emitted, `create_dir_all(destination)` has not run, and a re-run with the default `replace_existing: false` would hydrate a whole zone in order to report `Collision`. (b) "The offline half excuses; the remote half condemns" was found to be **false for a filesystem remote**, because `audit_remote_objects` short-circuits one to `missing: []` without asking it anything — so for the external-drive setup both halves would have gone quiet.
- **What was amended.** Two sentences in the intent contract's **Always** list are superseded, and the contract text was deliberately **not** edited (it is read-only): (a) "The pointer branch sits at plan time, in `classify` … After a successful hydration the file is re-stat'd before `PlanItem::File { bytes }` is minted" — the *refusal* stays at plan time, the *hydration* moved to execute time, and the plan's `bytes` is now the pointer's declared size, which is the honest size FR-336 already requires of every keeper surface. The clause's own justification (the plan's totals must be the content's) is better served by the new shape than by the one it prescribed. (b) "The remote half is not touched at all" holds literally — `audit_remote_objects`, `lfs::audit::*` and `remote_serves` are byte-identical — but `verify` now takes a **third** local fact for a filesystem remote: when the remote's LFS store is a directory that is present, it must hold the object or the path stays reported.
- **Why this was a patch and not a loopback.** Both root causes are mechanism, not intent: the behaviour the contract asks for (never copy pointer text silently; never call the normal state a fault; never go quiet about real loss) is unchanged and strictly better served. Both fixes are confined to `copy.rs`/`engine.rs` plus their tests, and reverting a mutation-proved change of this size to re-derive two localized decisions would have risked the parts that were right for no gain in correctness.
- **KEEP — must survive any future re-derivation.** The two-fact excuse expressed as two named locals at one site; `report.checked` still counting excused paths; the store-object-set assertion on both sides of a repair pass (an assertion that the write did not happen, not that the answer was right); the thousand-path fixture; the loopback batch server that answers a real per-object 404; and `Queued`-is-a-refusal.

## Review Triage Log

### 2026-08-25 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 15: (high 4, medium 9, low 2)
- defer: 5: (high 0, medium 4, low 1)
- reject: 8: (high 0, medium 0, low 8)
- addressed_findings:
  - `[high]` `[patch]` `VirtualPolicy::compile`'s `?` sat between the `Verifying` publish and the tail `Idle` publish, so one typo in `.keepervirtual` left the tray spinning and a bar on the card until the process ended — Story 34.8 re-opened. The compile moved **inside** the blocking closure so its error travels the same path as `report?`; the test now asserts the phase is back to `Idle`.
  - `[high]` `[patch]` The story's own safety argument was false for a filesystem remote: `audit_remote_objects` reports one intact without asking it anything, so an object that vanished from the external drive would have been reported by neither half. `verify` now takes the drive's own per-object proof — free, a `stat` — as a third fact, and refuses to excuse anything at all when `lfs_mode` is `Disabled`. An unplugged drive is still absence and not failure (AD-48).
  - `[high]` `[patch]` Hydration moved from the pre-walk to execute time (`hydrate_then_stage`, between `copy_one` and `stage`): after the opening progress frame, after `create_dir_all(destination)`, after the collision decision and behind a `cancel` check. This also closes the plan→copy window — keeper's own release sweep can re-dehydrate a planned path, and the old shape would have streamed, hashed, verified and published ~130 bytes and called it `Copied` with the plan's byte count.
  - `[high]` `[patch]` A refused copy had already committed the machine to downloading the object: `materialize_entry`'s `Queued` branch enqueues, promotes and calls `wake_now`, which also resets 56.5's release-sweep rotation. The trait impl now asks the local store first and refuses without ever reaching the journal; the test asserts nothing is pending afterwards.
  - `[medium]` `[patch]` `copy_start` hard-failed every copy when the sync engine could not be built — and leaked a permanently-`Copying` job when it did, because the early return came after `register`. Resolved before `register`, `Err` logged and degraded to `None`, which is the graceful path this story already designed.
  - `[medium]` `[patch]` One folder's malformed policy aborted `verify` for every other folder and suppressed the JSON document entirely. Contained per profile: its own line, its own JSON entry, its own exit gate, and the loop continues.
  - `[medium]` `[patch]` Nothing tested the rendering the whole anti-quietness argument rests on. `verify_lines` / `verify_entry` extracted pure in `ls_files_lines`' shape, with four unit tests including the contained-failure entry.
  - `[medium]` `[patch]` `republish_missing_objects` had only a negative test, so widening the new gate would have killed re-publication silently. Added the positive case: a worktree holding the real content is still re-cleaned and queued, and the object is in the store afterwards.
  - `[medium]` `[patch]` Every new integration test could self-skip to green through `Engine::open(...).ok()?`, which has nothing to do with git availability. `expect` everywhere but the one documented git probe.
  - `[medium]` `[patch]` The owning-profile resolution was lexical, so a synced folder reached through a symlinked or differently-cased ancestor refused every large file with a sentence the user knows is false. Both ends canonicalized, as `browse::resolve` does (AD-59); a non-UTF-8 component now refuses instead of being rendered lossily into an index key.
  - `[medium]` `[patch]` The re-clean gate skipped `stage::clean` for **any** pointer-shaped file rather than the pointer naming that object, so a tracked file whose genuine content is pointer text was reported "beyond this machine" although its bytes were sitting right there. Narrowed to `pointer.oid == object.oid`, with a test for the distinction.
  - `[medium]` `[patch]` A store object present at the **wrong length** answered `contains == false` and was excused as virtual — real damage, silenced. The excuse now requires the object to be genuinely absent.
  - `[medium]` `[patch]` `verify` built an index map it provably could not use for every folder in the product. Skipped when no source said anything may stay away, which also keeps the ordinary folder from paying an index parse per verify — and keeps the new repository open off every folder that has no policy.
  - `[low]` `[patch]` The refusal named the path in two frames — a report row keyed `clip.mp4` carrying a sentence about `40-media/clip.mp4`. The copy now owns the naming and re-renders only `ContentNotHere`.
  - `[low]` `[patch]` Four sentences that were false: the probe's cost ("costs an ordinary file nothing" — it opens every file under 1 KiB), "committed `.keepervirtual`" (it is the worktree file, and a profile-tier list replaces it wholesale), `verify` "only reads" (it now opens the repository, which can clear a stale `index.lock`), and the trait doc's promise that every `Err` is a sentence for the user.

## Design Notes

**Why the excuse needs the index as well as the policy.** The policy answers a question about a *path pattern and a size*, and the size it is given comes from bytes on disk that a user could have written by hand. `VirtualPolicy::resolve` is pure and does no I/O by design (FR-328), so it cannot tell a checkout's committed pointer from a text file someone saved that happens to start with `version https://git-lfs...`. The index can, for the price of one read per pass, and the pair is the difference AD-129 asks for: *intentionally* virtual (the repository committed this pointer and the policy authorizes the content to stay away) versus *unredeemable* (anything else — including a pointer whose object is gone and whose path keeper was supposed to be holding).

**Why the batched remote proof is not taken in `verify`, and what is taken instead.**

```
verify              → index + policy + (the drive, when it is a drive) → offline, per path, no round trip
verify --remote     → audit_remote_objects → batched, index-driven, one round trip per few hundred
                    → lfs::audit::serves   → the answer that condemns a virtual path on a server
```

`remote_serves` is per object. NFR-41's fixture is 10 000 virtual paths, so composing it per path inside `verify` would turn the very case this story exists to make quiet into 10 000 batch requests — and would break the contract `docs/sync.md` states, that plain `verify` is the half answerable without a network. So the batched proof stays behind `--remote`, where `missing_total`'s own exit gate is separate from `bad_total`'s (`commands.rs`).

**But "the offline half excuses, the remote half condemns" is only true when the remote can be asked, and the review found the case where it cannot.** `audit_remote_objects` short-circuits a **filesystem** remote to `missing: []` without asking it anything, so for the external-drive setup — the "second copy" the docs sell — both halves would have gone quiet about an object that vanished from the drive. That remote is a directory, so the proof needs no network and no batch: when it resolves and its root exists, the excuse takes it as a third fact; when the drive is out, absence is never failure (AD-48) and the two local facts stand alone. A fourth condition falls out of the same reasoning: with `lfs_mode = Disabled` nothing will ever materialize a pointer, so calling its absent content normal is a promise nothing keeps.

**Why the copy refuses by default, hydrates only on request, and hydrates at execute time.** AD-128 refuses on-read hydration because "a `grep -r`, Spotlight, a backup agent, an antivirus scanner or a `du` walks the tree and hydrates everything". A copy is a read of a whole subtree, so `copy.rs` — which any of those callers could reach — must not be able to hydrate on its own: the capability arrives as a value, from a caller that knows a human asked. The desktop copy verb is exactly such a caller, and there a refusal would be the wrong answer: a user asking to copy a folder onto a pendrive is asking for the files, and handing them 130-byte stubs is the "present but empty" failure `lfs/local.rs` already fixed once for filesystem remotes. Hence the seam, and hence `Queued` being a refusal — the only honest success is bytes on disk now.

**The refusal is a plan-time answer and the hydration is an execute-time one**, which is the one shape the review changed. Everything a hydration must be answerable for exists only at execute time: `cancel` is readable there, the opening progress frame has been emitted so a four-minute fetch is not a UI stuck at `0/0`, `create_dir_all(destination)` has already proven the destination writable, and the collision decision has already been made so a re-run does not hydrate a whole zone in order to report `Collision`. The plan keeps its promise that the totals are facts by taking the **pointer's declared size** — the honest size FR-336 requires of every other keeper surface — so nothing has to move to know it. And because the question is asked again immediately before the read, the plan→copy window closes: keeper's own release sweep can re-dehydrate a planned path, and the alternative was streaming, hashing, verifying and publishing ~130 bytes and calling it `Copied` with the plan's byte count.

```rust
// copy.rs::hydrate_then_stage — after the re-stat, the collision decision and the cancel check
if crate::lfs::stage::worktree_pointer(src, &meta).is_none() {
    return stage(src, dst, publish, cancel, reporter, hook);   // every ordinary file
}
let Some(source) = content else { return Err(refused_content_not_here(src)) };
source.materialize(src)?;
let after = std::fs::symlink_metadata(src)?;                   // the content's mode and mtime
if crate::lfs::stage::worktree_pointer(src, &after).is_some() {
    return Err(refused_content_not_here(src));                 // Ok, and still a pointer
}
stage(src, dst, publish, cancel, reporter, hook)
```

**Why the re-clean gate goes before the call and not around its answer.** `stage::clean` is not a probe: it streams the file into the LFS store (`stage.rs:760`). Ignoring its answer for a pointer path would still leave a junk object under the pointer text's own sha in `<git-dir>/lfs/objects` — an object no pointer names, that `prune` will not remove for want of a reference, and that makes the "second local copy" accounting `docs/sync.md:329-350` describes quietly wrong. The gate is therefore a `worktree_pointer` question asked first, and the test asserts the store's object set is unchanged by a repair pass — the assertion that the write did not happen, not just that the answer was right.

**What is deliberately left as recorded deferred work.** Five entries, in `deferred-work.md`: the virtual count reaching `keeper-syncd` and not the desktop (56.7/56.9 own that surface, and this story's sentence rewording is the honest half); `verify` no longer being strictly read-only for a folder that carries a policy, since it opens the repository (narrowed to policy folders only, and stated in the code); a non-regular file still reaching `stage::clean`, which is pre-existing; a copy started mid-sync refusing per path on `SyncError::Busy` with no retry policy; and `LfsMode::PointerOnly` keeping a whole folder virtual by profile-wide configuration while the excuse is deliberately per path (AD-122). The pre-existing false-green in `audit_remote_objects` for `LfsMode::Disabled` is unchanged and no longer load-bearing, because a Disabled folder now excuses nothing.

## Verification

**Commands:**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` — expected: applied; `--check` clean afterwards.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — expected: clean.
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` — expected: green, **≥ 3448 passing** (the branch baseline), including the new `tests/virtual_state_is_not_a_fault.rs` and the new inline `copy` tests.
- `bun run typecheck` — expected: clean. `bun run lint` — expected: exactly the recorded baseline of 4 warnings + 1 info. `bun run test` — expected: 297 files / 4869 tests, unchanged except for whatever asserts the reworded sentence.
- `bun run check:core-tauri-free`, `check:core-sync-free`, `check:syncd-lean` — expected: pass; no dependency is added.
- **Mutation proof, seven guards, before finishing.** For each: remove or invert it, run that single test alone, confirm it **FAILS**, restore, and verify the restore by reading `git diff` (not from memory), then confirm it passes.
  1. the policy arm of verify's excuse → the many-virtual test
  2. the index arm of verify's excuse → the stray-pointer test
  3. verify's surviving `!store.contains(...)` report → the unauthorized-path test
  4. the copy's pointer arm with no source → the refuse-by-name test
  5. `Queued`-is-a-refusal → the object-not-here test
  6. the `worktree_pointer` gate before `stage::clean` → the store-gains-no-object test
  7. **the negative direction:** the real-loss detection behind `--remote` (`audit::report`'s missing push / `audit::serves`' strictness) → the `verify --remote` test, proving a fix that silenced the checks entirely would not pass.

**Manual checks (if no CLI):**
- `git status --porcelain -- src/lib/ipc/gen` — must be empty.
- `git diff --stat -- src-tauri/crates/keeper` — must show only `copy_ipc.rs` (the engine handoff) and `sync_ipc.rs` (one doc sentence), so the macOS gate has exactly one compilable change to confirm.
- Smoke-test the real `keeper-syncd` binary against a real repository with a filesystem remote: `verify` over a folder with a `.keepervirtual` in force reports `0 bad` and a virtual count and exits 0; the same folder with one unauthorized pointer exits 1 naming that path; `verify --remote` over an object removed from the remote store exits 1.

## Auto Run Result

Status: done

### What was implemented

Three shipped checks stopped reporting Epic 56's normal state as a fault, without any of them going quiet about real loss (FR-339, NFR-41, AD-129).

**`Engine::verify` now earns its silence from facts, and reports everything it cannot prove.** A pointer-without-object path is excused only when the **index** carries that exact committed pointer (oid *and* size), the compiled **`VirtualPolicy`** authorizes that path at the pointer's declared size, the object is **genuinely absent** rather than sitting there truncated, and — when the remote is a directory that is present — **the drive itself holds the object**. Anything unproven keeps the row it always had, word for word. This is the first production consumer of `VirtualPolicy`, so 56.1's malformed-pattern refusal (FR-329) becomes reachable by a user for the first time; it fails the verb quoting the line rather than being read as a permissive "nothing is virtual", and it fails it from inside the blocking closure so the folder is put back to `Idle` (Story 34.8's rule, which a config typo had become a new way to break). Excused paths are **counted**, not merely suppressed — `VerifyReport::virtual_paths`, rendered in both of `keeper-syncd verify`'s forms — because a row nobody reports anywhere is indistinguishable from a check that stopped running. A folder that could not be checked is now its own entry with its own exit gate instead of aborting the run and suppressing the JSON.

**The batched remote proof stays where it is paid for once.** `verify --remote`, `audit_remote_objects`, `lfs::audit::*` and `remote_serves` are byte-identical: composing a per-object round trip inside `verify` would answer NFR-41's own fixture with 10 000 batch requests and break the offline contract `docs/sync.md` states. The one hole the review found in that division of labour — a **filesystem** remote, which the audit short-circuits to "intact" without asking it anything — is closed on the offline side, where that remote is a directory and the proof costs a `stat`. An unplugged drive is absence, never failure (AD-48).

**A verified copy of a virtual file hydrates or refuses by name, and can no longer copy pointer text.** `copy.rs` gains one object-safe seam, `ContentSource`, threaded as `Option<&dyn ContentSource>`; `None` refuses every pointer path at plan time by the `ContentRefusal` sentence a user already reads from `keeper-syncd materialize`. `Engine` implements the seam — the path→profile rule lives in the crate that compiles on every host, resolves canonically (AD-59) and asks the local store **before** `materialize_entry`, so a refusal never commits the machine to downloading the zone it refused. The hydration itself happens at execute time, after the opening progress frame, after the destination is proven writable, after the collision decision and behind a `cancel` check; the plan's byte total is the **pointer's declared size**, the honest size FR-336 already requires everywhere else. The question is asked again immediately before the read, so the window in which keeper's own release sweep re-dehydrates a planned path can no longer produce a 130-byte stub marked `Copied`.

**The re-clean check no longer compares a pointer against content.** `stage::clean` streams the file into the LFS store, so the gate is asked *before* the call, not around its answer: a worktree holding the pointer that names **this** object is unrecoverable with no read, no store write and no comparison — while a pointer-shaped file naming a different oid is still re-cleaned and queued, because those bytes are recoverable.

### Files changed

- `src-tauri/crates/keeper-sync/src/engine.rs` — `VerifyReport::virtual_paths`; `verify`'s four-fact excuse, its policy compile inside the closure, and the folder-wide `excusable` gate that keeps a policy-free folder from paying an index read; the new `filesystem_remote_store` helper; `republish_missing_objects`' gate before `stage::clean`; `impl copy::ContentSource for Engine`.
- `src-tauri/crates/keeper-sync/src/copy.rs` — the `ContentSource` seam, the plan-time refusal, `hydrate_then_stage` at execute time, `entry_reason` so the copy owns the path frame of its refusals, and 8 new inline tests.
- `src-tauri/crates/keeper-sync/src/lfs/hydrate.rs` — `ContentRefusal::ContentNotHere`, one hand-written sentence, in the enum the standing rule says to extend.
- `src-tauri/crates/keeper-sync/tests/virtual_state_is_not_a_fault.rs` — **new**, 18 tests over a real repository, a real index, a real filesystem remote seeded per object and a real loopback batch server answering a per-object 404.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — pure `verify_lines`/`verify_entry` renderers with their own tests, the contained per-folder failure, and the third exit gate.
- `src-tauri/crates/keeper/src/copy_ipc.rs` — the engine handed to the copy as its `ContentSource`, resolved before the job is registered and degrading to `None` rather than failing a copy that has nothing to do with sync.
- `src-tauri/crates/keeper/src/sync_ipc.rs`, `src/components/settings/sync-section.tsx`, `docs/sync.md` — the three sentences that became false, corrected.

### Review findings

15 patches applied (4 high, 9 medium, 2 low), 5 deferred, 8 rejected, 0 intent gaps, 0 spec loopbacks. The four high-severity ones were all the same family: a check or a promise that had quietly stopped being true. A config typo re-opened Story 34.8's stuck `Verifying` phase; the story's own safety argument ("the remote half condemns") was false for the external-drive setup, so both halves would have gone silent about a real hole; the copy hydrated a whole zone during a pre-walk that reads no cancel flag, shows no progress and has not yet proven the destination writable; and a *refused* copy had already enqueued a download for every path it refused and reset 56.5's sweep rotation. Details in the Review Triage Log; two implementation-shaped sentences of the intent contract are recorded as superseded in the Spec Change Log.

### Verification performed

- `cargo fmt --all` applied, `--check` clean. `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — clean.
- Full Rust suite over the three buildable crates — **3475 passed, 0 failed, 1 ignored** (branch baseline 3448, +27).
- `bun run typecheck` clean; `bun run lint` 4 warnings + 1 info, exactly the baseline; `bun run test` 297 files / 4869 tests, exactly the baseline. All three dependency firewalls pass. `git status --porcelain -- src/lib/ipc/gen` empty. `git diff --stat -- src-tauri/crates/keeper` shows only `copy_ipc.rs` (body of `copy_start`) and `sync_ipc.rs` (a doc comment), so the macOS gate has one function to confirm.
- **16 guards mutation-proved**, each mutated away, its own test run alone and observed to FAIL, restored, the restore verified by reading `git diff`, then re-run green — including every guard the patch pass moved, whose earlier proofs no longer applied. The seven the spec named, in the order it named them:

  | correction | guard | test | fails without | passes with |
  |---|---|---|---|---|
  | verify | the policy arm (`resolve(..) == Virtual`) | `the_unauthorized_path_is_the_only_row_reported` | reported nothing where the unauthorized row belongs | ✓ |
  | verify | the index arm (`indexed.get(index_key)` oid+size) | `a_pointer_shaped_file_the_index_does_not_carry_is_still_reported` | excused a stray pointer-shaped file | ✓ |
  | verify | the surviving report (`!store.contains(..)` → `bad`) | `a_thousand_authorized_virtual_paths_are_not_a_fault` + `the_unauthorized_path_is_the_only_row_reported` | 1 000 rows appeared / the real row vanished | ✓ |
  | verify | the drive's own proof (P2's third fact) | `a_virtual_path_whose_object_left_the_remote_store_is_still_reported` | excused an object the present drive does not hold | ✓ |
  | verify | absent ≠ truncated (P12) | `an_object_present_at_the_wrong_length_stays_reported` | excused a short blob under the right name | ✓ |
  | copy | the refusal with no source | `a_pointer_path_with_no_source_is_refused_by_name_and_nothing_reaches_the_destination` | copied the pointer text and called it `Copied` | ✓ |
  | copy | the execute-time re-ask | `a_file_that_becomes_pointer_text_between_the_plan_and_the_copy_is_failed_not_copied` | copied the stub with the plan's byte count | ✓ |
  | copy | `Queued`/store-absent is a refusal | `a_copy_of_a_path_whose_object_is_not_here_refuses_by_name` | reported `Copied`, and enqueued a download | ✓ |
  | re-clean | the gate before `stage::clean` | `a_repair_pass_over_a_virtual_path_adds_no_object_to_the_store` | the store gained a junk object under the pointer text's own sha | ✓ |
  | re-clean | the gate names **this** object | `a_worktree_pointer_naming_another_object_is_re_cleaned_and_queued` | called recoverable bytes "beyond this machine" | ✓ |
  | **negative** | the real-loss detection behind `--remote` (`audit::report`'s missing push) | `verify_remote_still_reports_a_virtual_path_the_server_cannot_serve` | reported no loss at all — so a fix that silenced every check fails here | ✓ |

  Five further guards were proved the same way: the policy compiling inside the closure (phase back to `Idle`), the unplugged-drive filter, the `lfs_mode = Disabled` refusal to excuse, the canonical folder resolution, and the daemon renderer that must not drop an unchecked folder.
- **Smoke-tested with the real `keeper-syncd` binary** against a real repository, a real bare filesystem remote and a real `.keepervirtual`: `verify` over four authorized virtual paths and one unauthorized one prints `6 checked, 1 bad, 4 virtual`, names only the unauthorized path and exits 1; with that path removed, `0 bad, 4 virtual`, exit 0, and `--json` carries `"virtual": 4`; deleting one object from the **present** remote store brings that path back as `1 bad, 3 virtual`, exit 1; taking the whole remote away returns it to `0 bad, 4 virtual`, exit 0; and one unclosed character class in `.keepervirtual` prints `media: could not be checked: … unclosed character class; missing ']'` and exits 1 without touching any other folder.

### Residual risks

- **The count reaches the CLI and not the desktop.** `sync_verify` still returns only the `bad` rows, so the Settings pane shows the (reworded, no longer over-promising) clean sentence and no virtual count. Carrying it needs a wire type and a generated binding, which 56.7 owns; recorded in the ledger.
- **`verify` is no longer strictly read-only for a folder that carries a policy.** It opens the repository for the index, and `git::repo::open`'s door can clear a stale `index.lock` and rewrite `.git/config`. Narrowed so that a folder with no policy never opens it at all, stated in the code, and recorded.
- **An HTTP-remote folder's virtual paths are excused on local facts alone**, by design: the proof for those is the batched `--remote` pass, and a user who never runs it never learns that an object never reached the server. That is the pre-existing division of labour, now stated in `docs/sync.md`.
- **A copy started while its folder is syncing refuses per path** on `SyncError::Busy`, so the same copy run twice can produce different sets — safe (a named refusal, never a stub) but nondeterministic; recorded.
- **The copy seam is unverifiable end to end on this host.** The only production caller is the desktop shell, which cannot compile here; the integration test drives the real `Engine` and the real `copy_verified` instead, and `copy_start`'s 12 changed lines were written against the surrounding code read in full. The macOS gate has exactly that one function body to confirm.
- **`LfsMode::PointerOnly` still produces the old wall** for a folder that keeps every path virtual by profile-wide configuration with no per-path policy — pre-existing, and deliberately not folded into the excuse, because making the mode an authorization would tie the per-path selector back to the lever AD-122 exists to keep it away from. Recorded.
