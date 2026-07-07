# Performance and reliability release gates

This is the canonical, sign-off-ready suite of keeper's performance and
reliability gates. It enumerates every hard PRD number (the NFRs and FR-48), maps
each to its threshold and its enforcement point (a CI test that fails the build,
or a release-checklist measurement a human records), and records the reference
hardware the numbers are stated against. A maintainer signing off a release
checks every row here.

For the release-time manual measurements CI cannot run unattended, see
[`docs/release.md`](release.md) → "Perf and reliability sign-off (SM-3)".

## Reference hardware

- **CI runner / reference baseline: GitHub `macos-latest` = Apple Silicon
  (`aarch64-apple-darwin`).** All CI-enforced gates below run on this runner, and
  every threshold is stated against this reference Apple Silicon baseline (it is
  also the only architecture keeper ships — see [`docs/release.md`](release.md)).
- Release-time manual measurements are taken on reference Apple Silicon hardware
  with a realistically seeded install (a 100k+-event archive; 1 and 5 configured
  accounts). Intel Macs are out of scope by design.

## Gate table

| # | Dimension (PRD) | Threshold | Enforcement point |
| --- | --- | --- | --- |
| NFR-1 | Cold start to interactive inbox | < 2 s (full) | **CI slice:** `src-tauri/crates/keeper-core/tests/cold_start_perf.rs::local_init_under_budget_at_100k_events` guards the deterministic offline **subset** (archive open + boot registry reads on a seeded ≥100k-event archive) under `LOCAL_INIT_BUDGET`. **Full figure:** release checklist (needs the webview + lazy per-account SDK activation). |
| NFR-2 | Offline FTS first results | < 200 ms p95 at 100k+ events | **CI:** `src-tauri/crates/keeper-core/tests/archive_search_perf.rs::search_p95_under_200ms_at_120k_events` (120k-event corpus, over the 100k+ threshold). |
| FR-48 | Command palette latency | ≤ 100 ms at 10k chats | **CI:** `src-tauri/crates/keeper-core/src/palette.rs::latency_under_100ms_at_10k_entries` (10k-entry index). |
| NFR-3 | Idle memory | ~500 MB with 5 accounts, ~300 MB with 1 account | **Release checklist — measure & flag.** See the assumption note below; these budgets are **not** hard CI gates. |
| NFR-8 | Crash safety (zero lost persisted events on kill) | Zero previously-committed rows lost; `PRAGMA integrity_check` = `ok` | **CI:** `src-tauri/crates/keeper-core/tests/crash_safety.rs` (real SIGKILL of a writing child for archive ingest, outbox insert, and settings write). |
| NFR-6 | Bridge-health drop reflected + notified | ≤ 60 s | **CI (logic):** the immediate disconnect-**notice** path in `src-tauri/crates/keeper-core/src/bridges/health.rs` (`disconnected_notice_flips_immediately_*`, `aggregator_notifies_once_on_transition_into_disconnected`) flips state with no debounce and notifies once. **No hard ≤ 60 s CI gate** covers the *silent-drop* (liveness-tick) case — its worst case can exceed 60 s (see below). **Live end-to-end:** release checklist. |

All CI-enforced gates run inside the existing required `Rust (fmt, clippy, test)`
cargo-nextest job — there is no separate perf CI job, so a regression fails the
same required check that already runs the 120k FTS gate.

## Cold-start honesty (NFR-1)

"Cold start to interactive inbox" spans Rust init **and** webview render + lazy
per-account SDK activation (which needs the Keychain + network). The full 2 s
figure is therefore inherently a release-time measurement on reference hardware.

The CI gate (`cold_start_perf.rs`) deliberately guards only the one boot cost that
scales with archived data: opening a large WAL `archive.db` (idempotent
schema/migration/FTS-exists path) plus the registry reads a cold boot performs
(`list_accounts` + a couple of `get_setting`s) on a seeded ≥100k-event archive. It
is documented and named as a **subset** guard — never presented as the full
cold-start number. This mirrors how stories 11.1/11.2 wire a partial gate in CI
and leave the full validation on the release checklist.

## Seeded 100k+-event corpus

Both the FTS gate (120k events) and the cold-start slice (100k events) seed a
corpus **over** the epic's 100k-event threshold by bulk-inserting in one
transaction and populating the FTS index once via the external-content `'rebuild'`
command (measuring query/open latency, not build throughput). This satisfies the
"enforce at the 100k+ corpus" requirement for both NFR-1's slice and NFR-2.

## Crash safety (NFR-8)

`crash_safety.rs` performs a **real OS process kill**, not an in-process `drop`. A
child re-invocation of the test binary (via `std::env::current_exe()`, gated by
the `KEEPER_CRASH_CHILD` env var so a normal `cargo nextest run` never recurses)
writes committed rows in a loop and prints + flushes `committed <id>` after each
commit. The parent reads those ids, `child.kill()`s the child (SIGKILL on Unix —
no `unsafe`, no new dependency) while writes are still in flight, then reopens each
DB and asserts:

- every id the child reported as committed survives (torn final write: a
  not-yet-committed row may be absent, but no previously-committed row is lost),
- `PRAGMA integrity_check` returns `ok`,
- for the archive path, the FTS index stays consistent with the indexed bodies
  (no orphaned/missing index rows; the FTS `'integrity-check'` passes).

All three local write paths are covered: archive ingest (`archive.db`), outbox
insert and settings write (`keeper.db`), both WAL-mode.

**Scope.** This is NFR-8's scope — survival of a killed *process* (unclean-WAL
recovery), not power-loss/fsync durability. A process kill leaves the written WAL
frames and the OS page cache intact, so a re-opener on the same machine sees every
committed frame regardless of the SQLite `synchronous` level; proving power-loss
durability would need a barrier-dropping/OS-crash harness, which is deliberately
out of scope.

## Bridge-health ≤60 s (NFR-6)

The ≤60 s bar rides **Leg 1** — the bot's management-room disconnect **notice**,
which flips session state immediately (no debounce) and fires exactly one
notification. This is the path the CI logic tests in `bridges/health.rs` cover.

The liveness-**tick** leg (debounce `DISCONNECT_DEBOUNCE_THRESHOLD` = 3 × a ping
reply timeout of up to 20 s × a tick interval clamped to ≤ 60 s) is a slower
backstop for a *silent* drop; its worst case can exceed 60 s, so there is **no**
universal "liveness-tick ≤ 60 s" guarantee to assert. The authoritative
end-to-end induced-drop check (a live bridge session dropped and observed
reflecting + notifying within 60 s) is a release-checklist item, because the
impure `HealthMonitor` shell cannot be driven against a live bot unattended in CI.

## Idle-memory budgets — ASSUMPTION, needs owner confirmation

> **The idle-memory numbers below are ASSUMPTIONS, not confirmed hard gates.**
> NFR-3 is a **measure-and-flag** requirement: idle RSS is recorded at release
> time and flagged if over budget, but it is **never** a silent hard-fail, and
> these budgets must be **confirmed by the project owner** on reference hardware
> before they can become enforced gates.

| Configuration | Assumed idle-memory budget |
| --- | --- |
| 1 account | ~300 MB |
| 5 accounts | ~500 MB |

Measuring idle memory requires a live session with the webview running, so it
stays on the release checklist ([`docs/release.md`](release.md)) rather than in
CI. Until the owner confirms these budgets against measured reference-hardware
numbers, treat them as targets to record against, not thresholds that fail a
release.

## What CI does NOT measure

CI never measures the **full** cold-start-to-interactive figure or idle memory —
both need the webview and, for cold start, a live per-account SDK activation
(Keychain + network). Those stay on the release checklist. CI also carries no
telemetry, analytics, or crash-reporting; the gates are pure offline tests.
