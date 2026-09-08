# Lane: BMAD intent vs implementation (folder sync)


## Scope read

**Planning artifacts**
- `_bmad-output/planning-artifacts/research-sync-2026-07-25.md` — §1 (1–100), §2/§2.6 (104–423), §12 open questions (1731–1776), §13 sources (1795–1821)
- `_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SPINE.md:271-373` (AD-40 … AD-53 + the reliability/scale envelope paragraph)
- `_bmad-output/planning-artifacts/epics.md` — 1-160 (inventory head), 3469-3623 (Phase 4 head + FR-77…FR-93, NFR-23…NFR-26, epics 23–24), 3899-3963 (Epic 31 + Phase 4 Validation Summary)
- `_bmad-output/planning-artifacts/product-inputs.md` (whole), `product-inputs-recording-sync-2026-08-05.md` (whole), `product-inputs-sessions-2026-08-12.md:149-183`
- `_bmad-output/planning-artifacts/research-virtual-files-2026-08-22.md` + `research/virtual-files-2026-08-22/digests/r1–r4` (grep hits), `architecture/.../ARCHITECTURE-VIRTUAL-FILES.md` (grep hits), `prds/prd-keeper-2026-07-03/prd.md:99,558,946`
- `_bmad-output/implementation-artifacts/sprint-status.yaml:232-302`; `deferred-work.md` (full DW header/status extraction; entries 1578-1613 and 2028-2048 read in full); `implementation-artifacts/` file inventory (353 files, spec-per-epic histogram)
- `.bmad-loop/decisions.json` (whole, 191 lines)

**Docs**
- `docs/sync.md` — 1-93, 254-278, 1386-1473, 1917-1999, 2935-3030, plus grep-targeted lines (29-33, 261, 1445-1456, 2926, 3023)
- `docs/decisions.md` (D-1 … D-17), `docs/constraints-and-limitations.md` (whole), `docs/performance.md:1-78`

**Code spot-checks** (`src-tauri/crates/`)
- `keeper-sync/Cargo.toml:1-60`; `src/lib.rs:31-35`
- `keeper-sync/src/profile/mod.rs:55-64,131-160,995-1096,1205-1211`
- `keeper-sync/src/engine.rs:302-330,430-431,5155-5258,5583-5587,5740-5810,5980-6000,6244-6248,6852-6987,8644-8672,9095-9125,12245-12252,13005-13025`
- `keeper-sync/src/git/cli.rs:12-33,146-186,267-310,712-783,1442-1500,1763-1786`; `git/mod.rs:14-20`; `git/repo.rs:7-114,4734-4743`; `git/conflict.rs:1-70`
- `keeper-sync/src/db.rs:638-660,1447-1479,2140-2160,2325-2455`; `backoff.rs:1-80`; `stability.rs:1-70,740-800,1326-1347`; `watch.rs:1-6,259-262`; `exclude.rs:1-6`; `provenance.rs:1-6`; `progress.rs:1-6`; `platform.rs:1-6`; `volume.rs:9-53`; `anomaly.rs:1-40`; `credential.rs:43-46,167-169`; `tasks.rs:169-220`; `lfs/basic.rs:1-6`; `lfs/stage.rs:60-121,1300-1312,1610-1665`; `browse.rs:1465-1469`
- `keeper-sync/tests/*` (inventory + `lfs_roundtrip.rs:1-45`, `durability_matrix_stages.rs:1-41`)
- `keeper-syncd/src/commands.rs:25-90,232-451`
- `keeper/src/tray.rs:398-410,1875-1900,2075-2172`; `keeper/src/lib.rs:612-613`; `src/components/settings/settings-dialog.tsx:150-258`; `sync-git-row.tsx:1-30`
- `package.json:25-31`; `.github/workflows/ci.yml:39-49`; `release.yml:208-305`; `src-tauri/Cargo.toml:14`

---

## How it works (intent chain)

Folder sync is **Phase 4** of the keeper PRD, and it is the one phase the PRD does not itself specify: `prd.md:99` says *"FR-77 … FR-368 belong to Phases 4–7, specified in `epics.md` and the per-epic files rather than here"*. So the requirement spine is `epics.md:3495-3522` (FR-77…FR-93, NFR-23…NFR-26) and the decision spine is `ARCHITECTURE-SPINE.md:271-363` (AD-40…AD-53), both dated 2026-07-25 and both declaring `research-sync-2026-07-25.md` *"adopted, not relitigated"* (`epics.md:3477-3481`).

The route was locked before any story: `gix 0.86` owns clone/fetch/checkout/status/index/commit/attributes, and exactly five operations shell out to `git` (AD-41; `git/cli.rs:12-20`). LFS is mandatory above a threshold because gitoxide has no streaming object read (AD-46; `lfs/basic.rs:1-6`). Completeness is a four-tier gate of which only tier 4 is a proof (AD-45; `stability.rs:5-18`). Divergence produces conflict copies, never a prompt (AD-43; `git/conflict.rs:1-21`). An absent volume is never a deletion (AD-48; `volume.rs:9-53`). Everything network is a journaled unit under exponential backoff (AD-49; `db.rs:638-660`, `backoff.rs:29-41`). The engine is a crate that is both `tauri`-free and `keeper-core`-free, gated in CI (AD-40; `package.json:25-26`), and ships again as `keeper-syncd` (AD-52; `keeper-syncd/src/commands.rs:261-451`).

Downstream phases extended it rather than revised it: Epic 41 added the producer assertion and push policy (`product-inputs-recording-sync-2026-08-05.md`, AD-66…AD-70), Epic 56 added virtual files (AD-122…AD-134, `docs/decisions.md` D-2), Epic 57 added tasks (D-3), Epics 66/69 added the phone tier (D-15, D-16). **No later phase reopened NFR-23.**

---

## Intent → implementation matrix

Verdicts: **implemented** = stated behaviour exists and is exercised; **partial** = exists but a named leg is missing or unproven; **contradicted** = code does the opposite; **silently changed** = code does something else and no artifact records it.

| id | stated where | what the code does | verdict |
|---|---|---|---|
| FR-77 bidirectional sync, no manual git | `epics.md:3499` | `Engine::sync_once` (`engine.rs:9095`), `do_pull` (`:6097`), `do_push` (`:6852`) | implemented |
| FR-78 multiple concurrent profiles | `epics.md:3500` | `SyncProfile` + per-profile drain loop `engine.rs:5155` | implemented |
| FR-79 watch mode, per-profile direction | `epics.md:3501` | `watch.rs:1-6`; `SyncDirection::{pushes,pulls}` `profile/mod.rs:55-64` | implemented |
| FR-80 LFS above a threshold, auto-tracked | `epics.md:3502` | `lfs/stage.rs:1610-1665`, `ensure_attributes` `:692`, default 4 MiB `profile/mod.rs:142` | partial — no path back for blobs already in history (F-BMAD-11) |
| FR-81 sparse checkout of subpaths | `epics.md:3503` | `engine.rs:5987/5997` → `git/cli.rs:289/296` | implemented (verbs differ from doc — F-BMAD-14) |
| FR-82 worktree lanes + PR | `epics.md:3504` | branch switch `engine.rs:5784-5796`; PR unit `db.rs:1783`, `engine.rs:8644` | silently changed (F-BMAD-6); PR path untested (F-BMAD-4) |
| FR-83 removable media pauses, never deletes | `epics.md:3505` | `volume.rs:9-53`; `Retriability::Deferred` → `WorkState::Deferred` `engine.rs:5250` | implemented |
| FR-84 only complete files sync | `epics.md:3506` | `exclude.rs` (t0), `watch.rs` (t1), `stability.rs` (t2/t4), `openfiles.rs` (t3) | implemented |
| FR-85 checksum-verified end to end | `epics.md:3507` | tier 4 `stability.rs:793`; LFS oid verify `lfs/basic.rs`; `verify` verb `commands.rs:298` | implemented |
| FR-86 provenance per change | `epics.md:3508` | `provenance.rs:1-6` trailers | implemented (undocumented `@device` in subject — DW-142 open) |
| FR-87 progress visible | `epics.md:3509` | `progress.rs:1-6`; `tray.rs:1876/2076` driven from `lib.rs:612`; `settings-dialog.tsx:253` | implemented — §18 still denies it (F-BMAD-5) |
| FR-88 non-blocking warnings | `epics.md:3510` | `record_failure` `engine.rs:5271`; `problems()` incl. parked `:12717` | implemented |
| FR-89 conflict copies, no prompt | `epics.md:3511` | `git/conflict.rs:1-70` | implemented |
| FR-90 offline tolerance, durable queue | `epics.md:3512` | journal `db.rs`; `reschedule_after` `engine.rs:5238`; `backoff.rs` | implemented (field 615 attempts is in-design) |
| FR-91 structured, secret-free logging | `epics.md:3513` | `tracing`; `scrub_userinfo` `git/cli.rs:965` | partial — promised log-scan test absent (F-BMAD-9) |
| FR-92 standalone Linux CLI/daemon | `epics.md:3514` | `commands.rs:261-451`, all AD-52 verbs | implemented |
| FR-93 credentials in keychain, egress disclosed | `epics.md:3515` | `platform.rs:1-6`; `credential.rs:43`; `EgressKind::GitRemote` | implemented |
| **NFR-23** 100 000 files / 50 GB, bounded memory | `epics.md:3519`, spine `:363` | measured only at 100 000 files / **393 MB** (`sync.md:2988`); no harness; absent from `performance.md` | partial, and the bar is below the field folder (F-BMAD-1, F-BMAD-7) |
| **NFR-24** no data loss under kill -9 / unplug / network loss | `epics.md:3520` | `tests/durability_matrix_stages.rs` + `keeper-syncd/tests/durability_matrix.rs` | implemented, **satisfied with evidence** |
| **NFR-25** committed within quiescence + one tick | `epics.md:3521` | `profile/mod.rs:131-140`; commit leg on own cadence | implemented — nothing bounds walk cadence (F-BMAD-2) |
| **NFR-26** no secret/content in logs; no `unsafe` | `epics.md:3522` | `unsafe_code = "deny"` `Cargo.toml:14`; scrubbing tested `cli.rs:1486-1514` | partial — "enforced by the 23.6 log-scan test" (`epics.md:3936`) is false |
| AD-40 own crate, tauri-free and core-free | spine `:293` | `keeper-sync/Cargo.toml:8-14`; gates `package.json:25-26,31` | implemented |
| AD-41 gix + a five-verb `git` shim | spine `:298` | push/sparse/merge live; **worktree ×3 and gc have no production caller** (`cli.rs:268,275,282,305`) | partial — 2 of 5 dead (F-BMAD-3, F-BMAD-6) |
| AD-42 profile is the unit; journal is truth | spine `:303` | `sync.db` schema `db.rs`; serialized writer | implemented |
| AD-43 conflict copies; deletions never beat modifications | spine `:308` | `git/conflict.rs:12-17,57-70` | implemented |
| AD-44 provenance trailers + device ULID | spine `:313` | `provenance.rs` | implemented |
| AD-45 four tiers, none decides alone | spine `:318` | four modules; assertion still faces tier 4 (`stability.rs:1329`) | implemented |
| AD-46 LFS mandatory, first-party client | spine `:323` | `lfs/*`; 4 MiB default `profile/mod.rs:142` | implemented |
| AD-47 sparse with `index.sparse=false` forced | spine `:328` | forced at clone; field config confirms it | implemented |
| AD-48 absence is not deletion | spine `:333` | `volume.rs`, `Deferred`, 64 in-crate citations | implemented |
| AD-49 offline is normal; work is scheduled | spine `:338` | `backoff.rs` 2 s→600 s full jitter; `engine.rs:5238` | implemented |
| AD-50 lane = linked worktree + PR, never force-push | spine `:343` | `ensure_lane` switches branch `engine.rs:5784`; `worktree_add` unreferenced; PR `engine.rs:8644` | silently changed (F-BMAD-6) |
| AD-51 capability flag, glyph frames, one status line | spine `:348` | `settings-dialog.tsx:153,253`; `tray.rs:1876,2076` | implemented |
| AD-52 `keeper-syncd`, same engine | spine `:353` | every named verb incl. `doctor` `:421`, `logs` `:423` | implemented |
| AD-53 egress honesty + programmatic credentials | spine `:358` | `lfs/batch.rs:422`; `scrub_userinfo`; egress row tested | implemented |
| Story 31.1 AC: 100 000-file / **50 GB**, results in `docs/sync.md` | `epics.md:3911` | `sync.md:2988` records 393 MB | partial — AC silently narrowed |
| Story 31.4 Live Forgejo Integration (clone, bidi, LFS round trip, sparse, lane + PR) | `epics.md:3928-3932` | `sprint-status.yaml:298` = **in-progress**; no networked test exists | not done — yet §18 asserts its outcome (F-BMAD-4) |
| Story 31.6 phase acceptance | `epics.md:3939-3945` | `sprint-status.yaml:300` = done, ahead of 31.4 | contradicted ordering |
| Research §12 Q6 history-growth policy / "when does `git gc` run?" | research `:1769-1774` | never answered; no gc caller, no gc task kind (`tasks.rs:170-220`) | open, and the field hit it (F-BMAD-3) |
| Research §12 Q7 removable-media trust | research `:1775-1779` | answered implicitly by `trust_full = profile.removable` | silently changed (F-BMAD-15) |
| Research §12 Q5 "does a CLI ship at all?" | research `:1756-1758` | shipped and released (`release.yml:208-305`) | resolved |
| Research §12 Q2 tray arbitration | research `:1743-1750` | one composed decision, no second icon (`tray.rs:2076`) | resolved as option 1 |
| Research §12 Q8 bundle a `git` binary? | research `:1768-1772` | not bundled; gated on `capabilities.sync` | resolved |
| `core.fsmonitor` / `core.untrackedCache` / `feature.manyFiles` / index v4 | **nowhere** | zero hits in `_bmad-output`, `docs`, `src-tauri` (one unrelated Scalar quote) | never considered (F-BMAD-13) |
| Story-spec traceability, epics 23–31 | BMAD convention | 0 `spec-*.md` for 61 shipped stories (epic 32 has 1, epic 34 has 19) | contradicted (F-BMAD-12b) |

---

## Findings

### F-BMAD-1 The scale NFR was never re-authored for the folder it actually runs on — P1, effort M
**Evidence:** `epics.md:3519` — *"NFR-23: Scale — a 100 000-file / 50 GB profile reaches steady state with bounded memory"*; spine `:363` repeats it as an *"authored bar, owner sign-off at phase release"*. The field folder is 155 626 index entries, a 26.4 MB index, 1.02 GiB of packs, 4.8 GB of local LFS objects, on a 573 GiB-used volume. Nothing in `_bmad-output` names 150 000 files or 450 GB; the only artifact naming the real drive is another subsystem's — `product-inputs-sessions-2026-08-12.md:164`, *"no measurable idle CPU on a 400 GB drive"*.
**Why:** the bar is 1.56× under the file count and roughly an order of magnitude under the byte count of the only folder anyone syncs, so every downstream "steady state" claim is about a smaller machine. Phase 7 then authored an NFR against the real 400 GB drive without noticing it contradicted the phase-4 envelope sharing that disk.
**Fix:** re-author NFR-23 as *200 000 files / 500 GB / 100 000 LFS objects on removable USB APFS*, and record the field folder's shape (entries, index size, pack size, LFS object count) in `docs/sync.md` §19 as the reference profile.

### F-BMAD-2 No requirement bounds walk *cadence*, and §19's steady-state sentence is the assumption the field contradicts — P1, effort M
**Evidence:** `docs/sync.md:2996-2998` — *"Steady-state cost is dominated by one `lstat` per file, so a folder that is not changing is cheap to keep watched."* NFR-25 (`epics.md:3521`) bounds only latency. `docs/sync.md:1455-1456` is explicit: *"Nothing about syncing is paced by that floor: the commit leg has its own cadence and never consults this one."* Field: 1371 walks in one session, `scanned=155626` every time, p50 1.079 s / p90 5.287 s / max 60.828 s, one walk every 1–5 s (666 in a single hour).
**Why:** the design behaves exactly as written, and the written design is wrong at this scale — the folder *is* changing (one file), so the "cheap to keep watched" clause never applies and every event pays 155 626 `lstat`s. This makes the whole slow-scan symptom class a **specification** defect: nobody violated a requirement, because none exists.
**Fix:** add an NFR bounding amortised walk cost ("a folder under continuous single-file write costs at most one full-tree walk per minute and <2% of one core"), and make the commit leg consult a floor the way the Pending poll already does (`engine.rs:431`).

### F-BMAD-3 Research open question #6 (history growth / when `gc` runs) was never answered, and the `gc` shim is dead code — P1, effort M
**Evidence:** `research-sync-2026-07-25.md:1769-1774` — *"**History growth policy.** … pointer churn plus commit-per-change still grows the DAG. When does `git gc` run, on what trigger, and is there ever a history truncation / re-init path?"* No AD answers it. `GitCli::gc` (`git/cli.rs:305`) has one caller: a test (`cli.rs:1783`). `TaskKind` is `Sync | Release | Verify | Bot` (`tasks.rs:170-220`) and `docs/sync.md:1985-1991` states that vocabulary is deliberately closed. Field: 181 210 packed objects / 1.02 GiB in 6 packs behind 476 commits, `gc.auto` unset, gc run by an **external launchd job** the owner wrote.
**Why:** AD-41 counts gc among the five shelled-out verbs, so a reader concludes keeper runs it. It does not, and the one open research question about long-run growth is the one the owner answered with cron.
**Fix:** add a `maintain` task kind calling `GitCli::gc` behind the existing per-folder reservation and quiet-window, or delete `gc`/`worktree_*` and amend AD-41 to three verbs; then record the history-growth answer as a `docs/decisions.md` entry.

### F-BMAD-4 §18's "verified against real remotes" is backed by nothing in the repo, and the story that would have earned it is still open — P1, effort M
**Evidence:** `docs/sync.md:2937-2941` — *"the engine and the `keeper-syncd` daemon implement and verify §§1–8, §10, §11 and §13 against real git remotes, including a full LFS round trip (upload, peer clone, download, materialize) against a local LFS server and the review-lane airlock."* Against it: `tests/lfs_roundtrip.rs:10-11` — *"No network: the transfer layer has its own tests"*; no LFS-server harness under `scripts/`, `dev/` or `.github/workflows/`; no test references `WorkKind::OpenPullRequest`; `sync.md:2984` says the envelope used *"a `file://` remote"*. `sprint-status.yaml:298` — `31-4-live-forgejo-integration: in-progress`, whose AC (`epics.md:3928-3932`) is exactly *"clone, bidirectional sync, LFS round trip, sparse profile, lane + PR … byte-identical content and verified checksums"*, while `31-6` (phase acceptance) is `done` at `:300`.
**Why:** the phase-acceptance story closed while its live-integration gate stayed open, and the operator doc then asserted the open gate's outcome. The LFS wire path, the Forgejo quirk list and the lane/PR airlock have no automated proof and no recorded manual run — textbook "satisfied by assertion only".
**Fix:** downgrade §18 to what is proven (offline round-trip against `file://`), and either finish 31.4 with a checked-in Forgejo/`lfs-test-server` harness behind an env gate or reopen 31.6.

### F-BMAD-5 §18 understates the product: the §12 surfaces are wired — P2, effort S
**Evidence:** `docs/sync.md:2965-2968` — *"**§12 progress and warnings.** … the desktop app surfaces that render them are not wired up."* In the tree: `tray.rs:2076 apply_sync_state` is called each tick from `lib.rs:612-613`; glyph set `tray.rs:1876`; held status line `tray.rs:405-409`; `settings-dialog.tsx:253` renders `{sync && <SyncSection …>}`. §12 itself documents the shipped Activity list (`sync.md:1524`) and the 0.8.14 Pending change (`:1427`).
**Why:** an under-claiming operator doc invites re-implementation of a shipped surface and makes §18 useless as a status register.
**Fix:** delete the §12 bullet from §18's exceptions list; leave only the two genuinely-gated items.

### F-BMAD-6 AD-50's linked worktree silently became a branch switch — P2, effort M
**Evidence:** spine `:343-346` — *"the engine materializes a linked worktree (`git worktree add`, via the shim …)"* and *"The lane's worktree is pruned after the PR merges"*; `docs/sync.md:261` repeats it. Code: `engine.rs:5784-5796` `ensure_lane` → `git.ensure_branch(...)`. `worktree_add`/`worktree_remove`/`worktree_prune` (`git/cli.rs:268,275,282`) have no production caller — only the phone-refusal test at `cli.rs:1763-1774`.
**Why:** the airlock's stated guarantee ("an agent writes only there") is weaker than advertised — agent and human share one working tree, and nothing prunes. No DW row, story or decision records the change.
**Fix:** implement the linked worktree (the shim exists and is argument-vector tested) or amend AD-50 + §7 to "a dedicated branch in the profile's own worktree", deleting the three unused verbs.

### F-BMAD-7 The canonical performance gate table has no row for any sync NFR — P2, effort S
**Evidence:** `docs/performance.md:1-8` claims to enumerate *"every hard PRD number (the NFRs and FR-48)"*; its table (`:41-49`) lists NFR-1, NFR-2, FR-48, NFR-3, NFR-8, NFR-6, NFR-22 and stops. NFR-23…NFR-26 appear nowhere in the file.
**Why:** the document a maintainer reads at release never mentions the subsystem walking 155 626 files every few seconds. NFR-23 has no enforcement point at all — not CI, not the checklist.
**Fix:** add four rows, marking honestly which are CI (NFR-24 genuinely is) and which are release-checklist.

### F-BMAD-8 125 engine tests silently skip when `git` is missing (DW-146, open) — P2, effort M
**Evidence:** DW-146 — *"Every `keeper-sync` engine test opens with `let Ok(engine) = Engine::open(..) else { return; };`, so on a machine without a usable `git` the whole family reports green having exercised nothing."* Counted: 125 such sites in `engine.rs`. `status: open`.
**Why:** it is the load-bearing qualifier on every "verified" claim in §18 and the Phase 4 Validation Summary. A shadowed or downgraded git turns the suite into a green no-op with no output change. [INFERENCE] CI does exercise them today: `ci.yml:49` runs nextest on a runner image that ships git.
**Fix:** DW-146's own second option — a helper that logs a loud skip and increments a counter, converted in one pass.

### F-BMAD-9 The log-scan test the phase acceptance credits does not exist — P2, effort S
**Evidence:** `epics.md:3936` — *"NFR-26 enforced by the 23.6 log-scan test …"*; Story 23.6's AC (`epics.md:3592-3594`) — *"a log-scan test asserts no credential-shaped token and no file content ever reaches a log line."* No such test exists in `src-tauri`; what exists is per-function coverage (`git/cli.rs:1486-1514`, `credential.rs:167-169`, `lfs/basic.rs:1385-1388`).
**Why:** the guards protect the sites someone remembered. NFR-26's claim is about *the log*; a new `tracing::warn!` interpolating a raw URL is exactly what the named test would catch. The field log is 895 MB with a 1.33 GB rotated sibling — nobody reads it by eye.
**Fix:** install a capturing `tracing` subscriber, drive a failing fetch/push/LFS batch against `https://user:token@host`, assert the buffer contains neither the userinfo nor any worktree byte.

### F-BMAD-10 No sync decision was ever recorded in either decision register — P2, effort S
**Evidence:** `.bmad-loop/decisions.json` holds 21 entries (DW-8 … DW-100), all `answered_at: 2026-07-06`, all Matrix-era; **no sync entry**. `docs/decisions.md` runs D-1 … D-17; D-2/D-3/D-15/D-16 are sync-adjacent, but nothing covers the store choice, the cadence, the scale envelope or history growth.
**Why:** `docs/decisions.md:5-6` exists so *"a reader can always trace a decision back to where it was made"*. The largest architectural commitment in the subsystem is traceable only to an owner sentence in an epics preamble.
**Fix:** write D-18 "the store is git, and what that costs", using the section below, with falsifiable assumptions and named revisit triggers.

### F-BMAD-11 FR-80 has no remediation path for content already in history as a blob — P2, effort M
**Evidence:** FR-80 (`epics.md:3502`) promises large content *"tracked, transferred and verified as LFS objects"*. The engine detects the violation per `anomaly.rs:14-23` and prints, every pass: *"files git carries as plain blobs that today's threshold would send to LFS … measured=files=600 bytes=1958664573 threshold=262144 … consequence=history keeps them as blobs for good"* (75 lines/session). The only doc acknowledgment is `sync.md:3023-3026` §20 item 6, which declines history rewriting.
**Why:** 1.96 GB of blobs is roughly twice the whole pack store (1.02 GiB) — the dominant growth term, and the same growth research §12 Q6 flagged. The warning is correct and actionable by nobody.
**Fix:** state the accepted cost in §20 with the anomaly's own numbers, and either add a documented opt-in `migrate --above=<size>` history rewrite or downgrade the anomaly to once-per-day.

### F-BMAD-12 The sync ledger holds 21 open entries, several of them the field's own symptoms — P2, effort S
**Evidence:** still `status: open`: DW-124 (nothing reconciles committed pointers against the remote), DW-125 (git probed 3× per `doctor`, TOCTOU), DW-126 (`ensure_activity_columns` race), DW-127 (0o600 into a shared remote store), DW-129 (unbounded `Vec` on the ssh LFS handshake), DW-130 (`LfsUploadPending` → `syncUnavailable`), DW-131, DW-133, **DW-134 (stale-lock recovery watches each lock 2 s at every `Repo::open`, four opens per pass)**, DW-135, DW-136, **DW-137 (a build writing into a gitignored-but-not-tier-0 dir still wakes the watcher)**, DW-139, DW-141, DW-142, DW-146, DW-148, DW-149, DW-161, DW-200, DW-206.
**Why:** DW-130 is visible on hesperia now (push-deferred rows), DW-134 is up to 8 s of lock-watching per pass on a folder that walks every 1–5 s, DW-137 is one of two mechanisms that could explain the cadence.
**Fix:** bundle DW-134 + DW-137 + DW-125 as a "walk and pass cost" sweep alongside F-BMAD-2's NFR; close DW-135/DW-139/DW-146 as one "the suite is evidence again" sweep.

### F-BMAD-12b 61 shipped stories across epics 23–31 have no story spec — P2, effort M
**Evidence:** `implementation-artifacts/` holds 353 files; grouping `spec-*.md` by epic yields **zero** for epics 23–31, 1 for epic 32, 19 for epic 34. `sprint-status.yaml:232-300` marks epics 23–30 `done` (61 story rows). DW-123 filed exactly this for six stories (34-14…34-19) and was closed `done 2026-07-29`; the ten-times-larger instance was never filed.
**Why:** the spec is the machine contract this audit checks code against. For the entire engine the only contract is epic prose and rustdoc — which is how ADs drifted (F-BMAD-6) with no artifact noticing.
**Fix:** do not back-fill 61 specs. File one ledger entry recording that epics 23–31 are spec-less by construction, and treat `docs/sync.md` + the AD block as their reviewable contract — which makes fixing §18 load-bearing rather than cosmetic.

### F-BMAD-13 `core.fsmonitor`, `core.untrackedCache` and the many-files knobs appear nowhere in the intent chain — P2, effort M
**Evidence:** a repo-wide search for `fsmonitor|untrackedCache|feature.manyFiles|splitIndex|index.threads` across `_bmad-output`, `docs` and `src-tauri/crates` returns one hit, an unrelated Microsoft-Scalar quote in a virtual-files digest (`r2-virtual-filesystems.md:99`). The gix capability matrix never raises them; no AD mentions them; the engine writes `index.sparse=false` and nothing else. Field config confirms none is set.
**Why:** git's own answers to "the status walk on a large tree is too expensive" are `core.untrackedCache` and `core.fsmonitor`. keeper cannot use `core.fsmonitor` for its own gix walk, but it already runs a watcher that is one by another name, and `core.untrackedCache` would help both keeper's walk and every foreign `git status` in that folder. Neither was considered, so neither was rejected with a reason.
**Fix:** add a decision evaluating `core.untrackedCache=true` + `index.version=4` on managed repositories, recording whether gix honours the untracked cache — and if it does not, say so, because the owner will keep asking.

### F-BMAD-14 Doc drift: the sparse verbs are `set|disable`, not `init|set|reapply` — P3, effort S
**Evidence:** `docs/sync.md:31` and spine AD-41 `:298` both say *"`git sparse-checkout init|set|reapply`"*. Code: `sparse_set` → `["sparse-checkout","set","--cone",…]` (`cli.rs:289,751`), `sparse_disable` → `["sparse-checkout","disable"]` (`:296,775`). `init`/`reapply` appear nowhere.
**Why:** trivial alone, but the same class as F-BMAD-6 in the same AD — the shim's advertised surface is not its real one.
**Fix:** one-line edit in both places.

### F-BMAD-15 Research §12 Q7 (removable-media trust) was answered by a flag, not a decision — P3, effort S
**Evidence:** research `:1775-1779` — *"§3.9's mitigation requires 'only after you've established the media is yours'. What establishes that … ? Getting this wrong silently commits multi-GB files raw."* Code answers with the profile flag: `git::repo::open(&profile.local_path, profile.removable)` at ~15 sites (`engine.rs:4938,6133,8104,10081,…`). Three sites hard-code `false` (`engine.rs:3591,9111,13017`); those read the index/pointers rather than running filters [INFERENCE], so the §3.9 hazard is not reachable through them.
**Why:** the answer is defensible and undocumented. §3.9 calls reduced trust a data-corruption hazard, and the thing between a user and it is a checkbox nobody is told is load-bearing.
**Fix:** record it in `docs/sync.md` §6 ("marking a profile removable is also what tells keeper to trust that repository's own `filter.*` config, without which LFS silently stops running there"), and make the hard-coded sites take `profile.removable` for uniformity.

---

## The git-vs-alternatives decision: what was actually recorded

**There is no recorded decision choosing git over rclone, Syncthing, git-annex, DVC, restic or a custom store.** git was a premise, entered as a given at the top of the phase:

> `epics.md:3471-3474` — *"Owner-requested phase: synchronize a local folder with a server over the **git protocol**, built on **gitoxide**, with git-LFS, worktrees, sparse checkout, multi-repo profiles, removable media, offline tolerance, tray progress, end-to-end checksums, provenance tagging, and a standalone Linux CLI."*

The research that followed was scoped to *how*, not *whether*: `research-sync-2026-07-25.md:9-14` scopes itself to *"a new keeper subsystem that synchronizes a user-chosen local folder against a git remote (Forgejo primary), carrying large binary files through git-LFS"*. Its §2 examined Syncthing, Nextcloud, rclone, gut-sync and Dropbox **for file-stability mechanics**; the cross-product conclusion (`:388-392`) is about quiescence gates, not stores.

The nearest thing to a store rationale is the Syncthing REJECT, and it is explicitly *post-hoc* — it justifies rejecting BEP **given** git:

> `research-sync-2026-07-25.md:216-221` — *"**REJECT:** the Block Exchange Protocol itself and the whole peer-to-peer/global-model architecture — **keeper syncs through a git remote, so the server is the global model and we inherit git's own causality (commit DAG) instead of inventing a vector clock.** Rejecting version vectors is what makes the ghost-counter class of failure structurally impossible for us, disputed report or not."*

git-annex, DVC and restic were surveyed a month later (`research-virtual-files-2026-08-22.md`) as **prior art for pointer design**, not candidate stores — emphatically so: `digests/r3-eviction-safety.md:52-53` — *"restic proves you can hold complete file metadata with zero worktree footprint — but only because restic never presents a browsable worktree. **It is the counter-example, not a model**, for a system whose whole point is that the user sees the file in `ls`."*

### The assumptions the choice carries, and which the evidence contradicts

| assumption, as stated | where | status against the field |
|---|---|---|
| *"a 100 000-file / 50 GB profile reaches steady state"* | `epics.md:3519` | **false at this scale** — 155 626 entries, ~450 GB, 26.4 MB index |
| *"Steady-state cost is dominated by one `lstat` per file, so a folder that is not changing is cheap to keep watched"* | `sync.md:2996-2998` | **contradicted in practice** — the folder always changes by one file, so the full 155 626-entry walk runs every 1–5 s, p90 5.3 s, max 60.8 s |
| *"LFS keeps blobs out of the ODB"* (implicit in AD-46) | spine `:323` | **partially false** — 600 files / 1.96 GB are blobs in history and cannot be removed; packs are 1.02 GiB |
| *"pointer churn plus commit-per-change still grows the DAG. When does `git gc` run?"* — flagged, unanswered | research `:1769-1774` | **answered in the field by a launchd cron the owner wrote**; keeper never calls gc |
| the server *is* the global model, so no vector clock is needed | research `:216-221` | **holds** — but the corollary bites: with the remote down 6 days, two clones of the same remote **on the same Mac** sit 7 days apart (`tgdrive` 3e66470 vs `tgdrive-light` 756612e) because there is no local peer path. A peer-to-peer design would have converged them. |
| a `git` binary is a reasonable hard prerequisite | `sync.md:19` | **holds** — git 2.52.0 present; capability gating works |
| gitoxide's absent push is survivable via the shim | spine `:298` | **holds on desktop**; D-16 had to write 931 lines of `push_http.rs` for the phone |
| the four-tier gate is sufficient for correctness | spine `:318` | **holds** — the field's near-miss (a 6.4 GB `.gz` truncated to 0 bytes) was refused by name, not committed |

**What is genuinely undecided and should be:** whether git's per-operation cost model — a full index + full worktree comparison per pass — is right for a folder that is 99.99% static and 0.01% append-heavy. That question was never asked, because the store was never chosen.

---

## Doc drift (doc line → code line)

| # | doc statement | doc | code | verdict |
|---|---|---|---|---|
| 1 | *"§12 … the desktop app surfaces that render them are not wired up"* | `sync.md:2965-2968` | `tray.rs:2076` via `lib.rs:612`; `settings-dialog.tsx:253` | **stale — shipped** |
| 2 | *"verify §§1–8, §10, §11, §13 against real git remotes … against a local LFS server"* | `sync.md:2937-2941` | `tests/lfs_roundtrip.rs:10` *"No network"*; no harness | **unsupported** |
| 3 | *"and the review-lane airlock"* (as verified) | `sync.md:2941` | no test touches `OpenPullRequest`; `engine.rs:8644` untested | **unsupported** |
| 4 | *"Keeper creates a linked worktree on a generated branch"* | `sync.md:261` | `engine.rs:5784-5796`; `cli.rs:268` unreferenced | **contradicted** |
| 5 | *"`git worktree add\|remove\|prune`"* as a shim operation | `sync.md:29` | `cli.rs:268,275,282` — no production caller | **contradicted** |
| 6 | *"`git gc` / `repack`"* as a shim operation | `sync.md:32` | `cli.rs:305`; only caller `cli.rs:1783` (test) | **contradicted** |
| 7 | *"`git sparse-checkout init\|set\|reapply`"* | `sync.md:31` | `cli.rs:289` (`set --cone`), `:296` (`disable`) | **drift** |
| 8 | *"`git gc` is available"* (§20 item 6) | `sync.md:3023-3024` | never invoked; no task kind `tasks.rs:170-220` | **misleading** |
| 9 | *"Run `git gc` on the repository"* (troubleshooting) | `sync.md:2926` | same | **honest but unowned** |
| 10 | *"100 000 files / 393 MB — first pass"* as the NFR-23 envelope | `sync.md:2988` | Story 31.1 AC demanded **50 GB** (`epics.md:3911`) | **narrowed** |
| 11 | *"a folder that is not changing is cheap to keep watched"* | `sync.md:2996-2998` | true per-walk; field runs 1371 walks/session | **assumption falsified** |
| 12 | *"Nothing about syncing is paced by that floor"* | `sync.md:1455-1456` | correct — `POLL_WALK_MIN_INTERVAL` `engine.rs:431` gates only the Pending poll | **accurate** (and the design defect, F-BMAD-2) |
| 13 | *"on a 155 625-entry folder … the unfloored poll walked the whole tree every ten to thirty seconds all day"* | `sync.md:1452-1455` | `engine.rs:12245-12251` | **accurate** |
| 14 | *"git ≥ 2.42"* | `sync.md:46` | `git/resolve.rs:74,279`, tested `:643` | **accurate** |
| 15 | *"`keeper-syncd doctor` … exits with code 3"* | `sync.md:47` | `commands.rs:68` `EXIT_PREREQUISITE = 3`; `Doctor` `:421` | **accurate** |
| 16 | `lfsThresholdBytes` default 4 MiB | `sync.md:79` | `profile/mod.rs:142` | **accurate** |
| 17 | `releaseTtlMs` 24 h, `lfsPruneLocal` true, `virtualOverBytes` 0 | `sync.md:82-84` | `profile/mod.rs:159,1051-1053` | **accurate** |
| 18 | settle 5 s / 10 s removable / 1 s close-write / 60 s ceiling | `sync.md` §4 | `profile/mod.rs:131-140`, `stability.rs:452` | **accurate** |
| 19 | daemon verbs `add … doctor, logs` | `sync.md` §13 / AD-52 `:353` | `commands.rs:261-451` — all present | **accurate** |
| 20 | *"A kind is a verb keeper already owns, and the vocabulary stays closed"* | `sync.md:1985` | `tasks.rs:170-220` | **accurate** (and why gc has no home) |
| 21 | *"only verify-on-read is a proof"* | `sync.md` §4 / `lib.rs:31-33` | `stability.rs:5-18,793` | **accurate** |
| 22 | conflict copy naming; deletions never beat modifications | `sync.md` §5 / spine `:311` | `git/conflict.rs:12-17,57-70` | **accurate** |

---

## What is correct / well done

- **NFR-24 is the model of a satisfied-with-evidence NFR.** A deterministic per-boundary grid (`tests/durability_matrix_stages.rs:1-41`, holes documented as holes) plus a daemon-level real-SIGKILL sweep that *found two production repairs* (`repo.rs:892`, `:999`), with an explicit statement of what the sweep can never prove.
- **The four-tier gate is implemented as researched, including the expensive parts.** Tier 4 re-reads every byte even for a producer-asserted path (`stability.rs:1326-1345`), and reading a file is explicitly not a change so keeper's own verify pass cannot retrigger the gate (`watch.rs:259-262`).
- **The anomaly discipline** (`anomaly.rs:1-27`): a measurement, an expectation and a consequence, at `WARN` never `ERROR` — born from a four-day outage where every fact was discoverable and none was logged.
- **The `.gitattributes`-as-pointer class was found in the field and closed by construction** (`lfs/stage.rs:98-121,110`), with the same guard reused for the virtualization policy — the fix for the 1.3 M `gix_attributes` warnings in the evidence.
- **Credential posture is genuinely programmatic:** `scrub_userinfo` on every argument vector, stderr and push-report line (`cli.rs:613,470,409`; `push_http.rs:205-207`), sensitive `Authorization` values (`lfs/batch.rs:422`), redacting `Debug` impls (`credential.rs:43`, `fetch.rs:65-72`), no credential-helper subprocess.
- **The `Trust::Reduced` filter-drop hazard was taken seriously** — `git/repo.rs:7-14` documents it as the corruption path it is, and `open(path, trust_full)` exists solely to defeat it on removable media.
- **AD-40's crate firewall is enforced, not asserted** — two CI scripts (`package.json:25-26`) wired into `bun run check` (`:31`).
- **The offline/backoff design matches its own field behaviour.** 615 attempts over six days against a dead host is the design (`backoff.rs:34-40`), not a storm; parked units are excluded from "pending" yet included in "may a pointer be published" (`db.rs:2325-2380`) — a distinction most implementations get wrong.
- **Docs are unusually honest where current** — `sync.md:1424-1456` explains the Pending-list floor with the real 155 625-entry folder, its cost, and what the user loses by it.

---

## Open questions I could not settle

1. **Was NFR-23's 100 000 / 50 GB ever shown to the owner as a bar?** The spine calls it an *"authored bar, owner sign-off at phase release"* (`:363`), and epic 31 — which owns that sign-off — is still `in-progress`. No record of sign-off or refusal exists.
2. **Did Story 31.4 or 31.5 ever run?** 31.5 is `done` (`sprint-status.yaml:299`) and is human-in-the-loop (physical hosts, a real pendrive); 31.4 is `in-progress`. No result artifact exists for either (`bmad-dev-auto-result-*` covers epics 12/14/16/20 only), so I cannot tell whether 31.5's convergence/provenance ACs were observed or the row was flipped optimistically.
3. **Does gix honour `core.untrackedCache`?** F-BMAD-13's decision needs that answer; it is a gitoxide-source question outside this lane's read scope.
4. **Which mechanism produces the walk cadence** — DW-137's wake filter, `POLL_WALK_MIN_INTERVAL` not covering the commit leg, or the recording producer assertion. ScanPerf owns the attribution; my finding is only that no requirement forbids any of them.
5. **Why `tgdrive-light` re-ran its first checkout many times** (588 408 `collided (AlreadyExists)`, same path up to 12×). §2/AD-48 say adopting a non-empty folder is ordinary; whether the repeat is a retry loop or supervisor re-entry is a code question I did not chase.
