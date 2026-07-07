---
title: 'Performance and Reliability Release Gates'
type: 'feature'
created: '2026-07-06'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '953fea8d0807f487efb881cc9af140f938e5813c'
final_revision: 'b510279d440fa8eb4334a28a282525982a83f4fd'
context:
  - '{project-root}/docs/project-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** The PRD's hard performance/reliability numbers are asserted but only partly enforced. FTS latency (`archive_search_perf.rs`, 120k events, p95<200ms) and palette latency (`latency_under_100ms_at_10k_entries`, 10k chats) already gate in CI, but there is **no** crash-safety-under-process-kill test (NFR-8), no cold-start or idle-memory measurement (NFR-1/NFR-3), and no consolidated release-gate suite or reference-hardware doc — so regressions in the unguarded dimensions reach users and the SM-3/SM-4 sign-off has nothing concrete to check against.

**Approach:** Add the missing *automatable* gates — a real OS process-kill-mid-write durability test covering the three write paths (archive ingest, outbox insert, settings write), and a boot-time local-store-init budget test on a seeded 100k+-event archive — then consolidate every perf/reliability gate into a documented release-gate suite (`docs/performance.md`, reference hardware + gate table + assumption-tagged budgets) and extend the release checklist (`docs/release.md`) with the release-time manual measurements that cannot run unattended (full cold-start-to-interactive, idle memory measure-and-flag, live bridge-drop ≤60s).

## Boundaries & Constraints

**Always:**
- New gates are plain Rust `#[test]`s in `keeper-core`, run by cargo-nextest in the existing **required** CI Rust job so a regression fails the build; reuse the established perf-test pattern (`Instant::now()` + budget assert; seed via the crate's public API as `archive_search_perf.rs`/`archive_durability.rs` do).
- The crash-safety test performs a **real OS process kill** (SIGKILL via `std::process::Child::kill()` — no `unsafe`, no new dependency) of a child that is *actively writing*, then reopens each DB in the parent and asserts every row the child reported as committed survives, `PRAGMA integrity_check` returns `ok`, and (archive) the FTS index stays consistent — for all three write paths.
- The cold-start CI gate measures only the deterministic, offline, Rust-measurable boot slice (`open_archive_db` + registry reads on a seeded ≥100k-event archive) against a documented local-init budget; it is named and documented as a **subset** guard, never presented as the full cold-start figure.
- The existing FTS (120k) and palette (10k) gates already pass — reference them as the enforcing tests in `docs/performance.md`; do not rewrite, weaken, or `#[ignore]` them. FTS already seeds >100k, satisfying "extend to the 100k+ corpus".
- Idle-memory budgets (~500 MB with 5 accounts, ~300 MB with 1) are **assumption-tagged**: measured and flagged-if-over on the release checklist, never a silent hard-fail, and documented as needing owner confirmation before becoming hard gates (NFR-3 = measure).
- English everywhere; bun only; Biome + rustfmt clean; no `.unwrap()`/bare `.expect()` in production paths (`expect` is fine in this test code); no AGPL/GPL/copyleft deps (`cargo deny` stays green); commit on the current branch only — no branch/push/history changes.

**Block If:**
- The child-kill harness cannot be expressed within cargo-nextest without `unsafe` or a new heavyweight dependency that fails `cargo deny` (e.g. no clean way to re-invoke the test binary as a writing child) — a tooling decision for a human.
- The seeded 100k+ cold-start slice already exceeds a defensible local-init regression budget (opening the archive is itself slow at scale) — a real perf defect needing a human perf/product decision, not a threshold to quietly relax.
- The shipped bridge grammars make even the immediate disconnect-**notice** path unable to surface+notify within 60 s (i.e. NFR-6's ≤60s mechanism is not actually met by the code) — a product/config decision.

**Never:**
- Never claim CI measures full cold-start-to-interactive or idle memory — those require the webview / a live session and stay on the release checklist. Never fabricate a universal "liveness-tick ≤60s" guarantee: the ≤60s bar rides the immediate disconnect-notice path (Leg 1, flips with no debounce); the tick leg (debounce 3 × up-to-20s ping timeout × tick interval clamped to ≤60s) is a slower backstop whose worst case can exceed 60s.
- Never weaken the existing FTS/palette gates to speed CI; never add telemetry/analytics/crash-reporting; never put Matrix/perf logic in TypeScript; never use npm/yarn/pnpm; never loosen the license allowlist to make a check pass.

## I/O & Edge-Case Matrix

Applies to the crash-safety durability test (child SIGKILLed while writing; parent reopens and asserts).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Archive ingest killed mid-batch | child inserts events into `archive.db`, printing each committed `event_id`, SIGKILLed after ≥K commits | on reopen, all K reported `event_id`s present; `integrity_check`=`ok`; FTS row-count consistent with indexed bodies | reopen succeeds, no panic |
| Outbox insert killed mid-loop | child `insert_outbox`es rows, printing committed ids, SIGKILLed after ≥K | all K reported ids present via `list_outbox_rows`; `integrity_check`=`ok` | reopen succeeds |
| Settings write killed mid-loop | child `set_setting`s keys, printing committed keys, SIGKILLed after ≥K | every reported key readable via `get_setting`; `integrity_check`=`ok` | reopen succeeds |
| Torn final write | child killed while a write is in flight (row not yet committed) | the uncommitted row may be absent; **no previously-committed row is lost or corrupted** | WAL integrity preserved |
| Cold-start local init at 100k+ | `archive.db` seeded with ≥100k events + registry with accounts/settings | `open_archive_db` + registry reads complete under the documented `LOCAL_INIT_BUDGET` | test fails build if over |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/tests/crash_safety.rs` -- **NEW** integration test: subprocess-SIGKILL durability for archive ingest, outbox insert, settings write.
- `src-tauri/crates/keeper-core/tests/cold_start_perf.rs` -- **NEW** integration test: boot-time local-store-init budget on a seeded ≥100k-event archive.
- `src-tauri/crates/keeper-core/tests/archive_search_perf.rs` -- EXISTING FTS gate (120k events, p95<200ms); reference only, do not modify.
- `src-tauri/crates/keeper-core/src/palette.rs` -- EXISTING palette gate `latency_under_100ms_at_10k_entries()` (~line 1131); reference only.
- `src-tauri/crates/keeper-core/src/archive/db.rs` -- `open_archive_db`/`open_readonly_archive_db`/`insert_event`/`db_path` (public; used by both new tests to seed/reopen).
- `src-tauri/crates/keeper-core/src/registry.rs` -- `set_setting`/`get_setting`/`insert_outbox`/`list_outbox_rows`/`list_accounts` (public; write paths under test).
- `src-tauri/crates/keeper-core/src/bridges/health.rs` -- bridge-health constants (`PING_REPLY_TIMEOUT`=20s, `DISCONNECT_DEBOUNCE_THRESHOLD`=3, tick clamp [1,60]) and the immediate-notice-flip tests that back the ≤60s claim; reference only.
- `docs/performance.md` -- **NEW** canonical perf/reliability release-gate suite: reference hardware, gate table, assumption-tagged budgets.
- `docs/release.md` -- extend the release checklist with the SM-3 manual perf/reliability measurements.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/tests/crash_safety.rs` -- NEW. Real-SIGKILL durability for all three write paths; unit-test **every** I/O-Matrix row. Parent spawns `std::env::current_exe()` with libtest args to run a child test that writes-until-killed, gated by a `KEEPER_CRASH_CHILD` env var so a normal nextest run does not recurse; parent reads committed ids from child stdout, `child.kill()`s after ≥K, reopens each DB, asserts zero loss + `PRAGMA integrity_check`=`ok`.
- [x] `src-tauri/crates/keeper-core/tests/cold_start_perf.rs` -- NEW. Seed `archive.db` with ≥100k events (reuse the bulk-insert + FTS-rebuild pattern from `archive_search_perf.rs`) and the registry with accounts/settings; time `open_archive_db` + registry reads; assert under `LOCAL_INIT_BUDGET` (500 ms; measured baseline ~18 ms). Named/commented as the CI-measurable *subset* of cold start.
- [x] `docs/performance.md` -- NEW. Reference hardware (GitHub `macos-latest` = Apple Silicon/aarch64 + the reference-Apple-Silicon note); a gate table mapping each number (cold start NFR-1, FTS NFR-2, palette FR-48, memory NFR-3, crash safety NFR-8, bridge-health NFR-6) to its threshold and enforcement point (CI test path *or* release-checklist); the assumption-tagged memory budgets flagged as needing owner confirmation; the seeded-100k+-corpus note.
- [x] `docs/release.md` -- Extend the release checklist with the release-time manual measurements CI cannot run unattended: full cold-start-to-interactive < 2 s, idle memory (measure & flag vs ~300/~500 MB), and a live induced bridge-drop reflected+notified ≤ 60 s; cross-link `docs/performance.md`.
- [x] Verify-only: confirm the existing FTS + palette gates still pass and are listed in `docs/performance.md`; no code change to them.

**Acceptance Criteria:**
- Given the perf/reliability gate suite runs under cargo-nextest in the required CI Rust job, when a change regresses archive-open, FTS, or palette latency past its budget, then the build fails (NFR-1 slice, NFR-2, FR-48).
- Given the crash-safety test, when the writing child is SIGKILLed mid-write for each of the three paths, then reopening each DB shows zero lost previously-committed rows and `PRAGMA integrity_check`=`ok` (NFR-8).
- Given `docs/performance.md`, when a maintainer signs off a release, then it enumerates every perf/reliability gate with its threshold and enforcement point, tags the idle-memory budgets as assumptions needing confirmation, and records the reference hardware (SM-4).
- Given `docs/release.md`, when the release checklist is followed, then it includes the manual measurements CI can't run unattended: full cold-start-to-interactive < 2 s, idle memory measure-and-flag vs ~300/~500 MB, and a live induced bridge-drop reflected+notified ≤ 60 s (SM-3, NFR-3, NFR-6).

## Spec Change Log

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 1, low 4)
- defer: 0
- reject: 15
- addressed_findings:
  - `[medium]` `[patch]` The parent read loop (`for line in reader.lines()`) could block **indefinitely** on a stalled child — a reliability gate that can itself hang CI. Rewrote it to drain the child's stdout on a helper thread feeding an mpsc channel, with a `recv_timeout(30s)` deadline so a wedged child becomes a bounded, diagnosable failure; made `child.kill()` lenient (no panic if the child already exited) and added the child's exit status to the assertion so a short read is reported as a writer/spawn fault, not a durability loss. Also set the child's `.stdin(Stdio::null())`, which cleared nextest's `LEAK` warnings (the child had inherited the parent test's stdin handle).
  - `[low]` `[patch]` The cold-start budget timed a single **cold-cache** open, risking a flaky failure of a *required* gate on noisy hosted CI. Added an untimed warm-up open so the timed run measures steady-state archive-open (the thing a real regression would slow).
  - `[low]` `[patch]` Temp dirs (including a 100k-event archive) leaked on any assertion unwind. Added an RAII `TempDirGuard` to both new test files so a failing gate never accumulates seeded DBs in the system temp dir.
  - `[low]` `[patch]` The crash-safety module doc and `docs/performance.md` implied fsync/power-loss durability. Scoped both to NFR-8's actual claim — survival of a killed *process* (unclean-WAL recovery) — and noted power-loss/fsync durability is deliberately out of scope (would need a barrier-dropping harness).
  - `[low]` `[patch]` The NFR-6 gate-table cell could read as ≤60s CI coverage. Made explicit that no hard ≤60s CI gate covers the *silent-drop* liveness-tick case (its worst case can exceed 60s); the ≤60s bar rides the immediate disconnect-notice path + the live release-checklist check.
- notes: Rejected 15 as refuted-by-code or by-design: the `stored_rows - indexed_docs <= 1` bound is correct for a single **synchronous** writer (insert→index→print sequentially on one connection — both reviewers ultimately concurred; already commented); `integrity_check` first-row check fails correctly on corruption (a corrupt DB's first row is an error line, not `ok`); the "torn case not forced" is inherent (a kill can't be deterministically landed mid-transaction — the asserted invariant holds either way, and forcing it needs fault injection out of scope); outbox/settings presence-not-value checks are sufficient (SQLite commit atomicity — a committed row's columns are intact, and `integrity_check` covers corruption); the per-call `open()` connection-churn "reduced fidelity" **is** the real production write path for `insert_outbox`/`set_setting`; `KILL_AFTER=200` is bounded and fast (≤0.5s observed); `--exact` matching zero tests fails safe (empty stream → the `>= KILL_AFTER` assert fails, not a false pass); the `KEEPER_CRASH_CHILD`-in-nextest-env inversion is unrealistic (keeper-specific var, set only on the child `Command`); pipe-full deadlock is benign (the kill immediately follows and the reader thread drains); `black_box` on the reads is harmless; the budget lumping archive-open + registry reads is by design (it guards the whole boot slice); the "required check" claim is accurate (the tests run in the Rust nextest job documented as required in `release.md`); the NFR-1 table cell already marks the CI slice a *subset* with the full figure on the release checklist; and temp-dir collision (pid+nanos, now also RAII-guarded) / `insert_account` uniqueness both rely on the guaranteed-clean unique temp dir. No `intent_gap` and no `bad_spec`: every real finding was an in-diff test/doc patch and the frozen `<intent-contract>` stands.

## Design Notes

**Real-crash harness (no unsafe, no new dep).** A real crash must be an OS kill, not an in-process `drop` (which reviewers correctly reject as not exercising fsync-on-commit durability). Pattern:

```rust
let exe = std::env::current_exe().expect("test exe");
let mut child = Command::new(&exe)
    .args(["--exact", "crash_child_archive", "--nocapture"]) // runs one child test
    .env("KEEPER_CRASH_CHILD", dir.path())                    // dir holding the DBs
    .stdout(Stdio::piped()).spawn().expect("spawn child");
// read child stdout lines "committed <id>" into `seen` until seen.len() >= K, then:
child.kill().expect("SIGKILL");   // std::process::Child::kill = SIGKILL on Unix, safe
let _ = child.wait();
let conn = open_archive_db(dir.path()).expect("reopen");       // parent reopens
// assert every id in `seen` is present, PRAGMA integrity_check == "ok"
```

The child test (`crash_child_archive`/`_outbox`/`_settings`) returns immediately when `KEEPER_CRASH_CHILD` is unset (so `cargo nextest run` never recurses); when set, it opens the real DB and writes in a loop, printing+flushing `committed <id>` after each committed row until killed. `current_exe()` under nextest is a standard libtest binary that accepts `--exact <name> --nocapture` when invoked directly. Keep K modest (e.g. 200) so the parent kills promptly while writes are still in flight.

**Cold-start honesty.** "Cold start to interactive inbox" spans Rust init **and** webview render + lazy per-account SDK activation (needs Keychain + network), so the full figure is inherently release-time on reference hardware. The CI gate guards the one deterministic offline slice that scales with data — opening a 100k+ `archive.db` (WAL) + registry reads — and is labelled as a subset, not the PRD's 2 s number. This mirrors 11.1/11.2: wire+partial-gate in CI, full validation on the release checklist.

**Bridge-health ≤60s.** Met by Leg 1 — the bot's management-room disconnect **notice** flips state immediately (no debounce) and fires one notification (already covered by `disconnected_notice_flips_immediately_*` / `aggregator_notifies_once_on_transition_into_disconnected`). The liveness-tick leg is a slower backstop for silent drops; do not assert a universal ≤60s over it. The authoritative end-to-end induced-drop check is a release-checklist item (SM-3) — the impure `HealthMonitor` shell cannot be driven against a live bot unattended.

**No separate CI job.** The new tests run inside the existing required `Rust (fmt, clippy, test)` nextest job (the 120k FTS test already runs there), so they gate PRs without doubling CI compile time; `docs/performance.md` names them as the release-gate suite.

## Verification

**Commands:**
- `bun run test:rust` -- expected: `crash_safety` (all matrix rows) + `cold_start_perf` pass alongside the existing `search_p95_under_200ms_at_120k_events` and `latency_under_100ms_at_10k_entries`; bindings unchanged (no new VM types).
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` clean (no `.unwrap()` in prod paths; test `expect` allowed).
- `bun run check` -- expected: biome + tsc + vitest still green (no frontend change).
- `cd src-tauri && cargo deny check licenses bans sources` -- expected: `ok` (no new dependencies introduced).
- Inspect `docs/performance.md` + `docs/release.md` -- expected: gate table maps every NFR to a threshold + enforcement point; memory budgets tagged assumption; SM-3 manual measurements listed.

**Manual checks (release-time, cannot run unattended):**
- On reference Apple Silicon with a seeded 100k+ archive: full cold-start-to-interactive < 2 s; idle RSS with 1 and 5 accounts recorded and flagged vs ~300/~500 MB; a live induced bridge-session drop reflected+notified within 60 s.

## Auto Run Result

Status: done

**Change summary.** Turned the PRD's perf/reliability numbers into an enforced, sign-off-ready release-gate suite. Added the two gates that were genuinely missing — a **crash-safety** durability test that SIGKILLs a real child process actively writing each of the three local write paths (archive ingest, outbox insert, settings write) and asserts zero previously-committed rows are lost + `PRAGMA integrity_check`=`ok` + FTS consistency (NFR-8), and a **cold-start** local-init budget test that seeds a ≥100k-event archive and gates the deterministic offline boot slice (archive open + registry reads) under 500 ms (NFR-1 CI subset). Consolidated every gate — including the already-passing FTS (120k, `archive_search_perf.rs`) and palette (10k, `latency_under_100ms_at_10k_entries`) tests — into `docs/performance.md` (reference hardware, gate table, assumption-tagged idle-memory budgets), and extended `docs/release.md` with the SM-3 release-time manual measurements CI cannot run unattended (full cold-start, idle memory measure-and-flag, live bridge-drop ≤60s). No new dependencies, no frontend/IPC/VM changes.

**Files changed.**
- `src-tauri/crates/keeper-core/tests/crash_safety.rs` (new) — real-SIGKILL durability gate for archive ingest / outbox / settings; thread-drained, deadline-bounded child harness; RAII temp cleanup.
- `src-tauri/crates/keeper-core/tests/cold_start_perf.rs` (new) — 100k-seeded local-init budget gate (`LOCAL_INIT_BUDGET`=500 ms, baseline ~18 ms) with warm-up; RAII temp cleanup.
- `docs/performance.md` (new) — canonical gate table (NFR-1/2/3/6/8 + FR-48 → threshold + enforcement point), reference hardware, cold-start/crash-safety/bridge-health honesty notes, assumption-tagged memory budgets.
- `docs/release.md` — new "Perf and reliability sign-off (SM-3)" release-checklist section.

**Review findings breakdown.** intent_gap 0, bad_spec 0. Patches applied 5 (medium 1, low 4): deadline-bounded/non-hanging + self-diagnosing crash-child harness (+ `.stdin(null)` clearing nextest LEAKs), cold-start warm-up against cold-cache flake, RAII temp-dir cleanup, fsync-vs-process-kill scope honesty in the comment + `docs/performance.md`, and an explicit "no hard ≤60s CI gate for the silent-drop case" note in the NFR-6 table cell. Deferred 0. Rejected 15 (see Review Triage Log) — chiefly the `stored-indexed ≤ 1` bound (correct for a single synchronous writer), integrity_check corruption detection, and the "torn case not forced" limitation (inherent to non-fault-injected crash testing).

**Verification performed (all green, re-run after patches).**
- `bun run test:rust` → **760 tests passed, 0 skipped, 0 leaky** — incl. the 6 `crash_safety` tests, `cold_start_perf::local_init_under_budget_at_100k_events` (~1.8 s), and the existing `search_p95_under_200ms_at_120k_events` (~13 s) + `latency_under_100ms_at_10k_entries`.
- `bun run check:rust` → rustfmt + clippy `-D warnings` clean.
- `bun run check` (biome + tsc + vitest, 947 tests) and `cargo deny check licenses bans sources` → confirmed green by the implementer and unaffected by the review patches (test-only Rust + docs; no deps, no frontend change).

**Residual risks.**
- The crash-safety gate proves **process-kill** survival (unclean-WAL recovery), not power-loss/fsync durability — now explicitly scoped in the code doc and `docs/performance.md`; a true power-loss test would need a barrier-dropping/OS-crash harness (out of scope for NFR-8).
- The CI cold-start gate is an honest **subset** (offline local store init); the full "< 2 s to interactive inbox" figure and idle memory (NFR-3) require the webview / a live session and remain release-checklist measurements on reference Apple Silicon (memory budgets are assumption-tagged, awaiting owner confirmation).
- NFR-6's ≤60s bar rides the immediate disconnect-notice path (CI logic tests); the silent-drop liveness-tick worst case can exceed 60s and the live end-to-end check is release-time only — both documented.
- `followup_review_recommended: false` — the review pass made only localized, verified test-infrastructure and documentation patches (no production code, IPC, or data-path changes), so an independent follow-up is not warranted.
