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

- **CI runner: GitHub `macos-latest` = Apple Silicon (`aarch64-apple-darwin`).**
  All CI-enforced gates below run on this runner, and every threshold is stated
  against an Apple Silicon baseline (it is also the only architecture keeper
  ships — see [`docs/release.md`](release.md)).
- **The runner is Apple Silicon, but not a fixed speed.** `macos-latest` boxes
  are shared, and the FTS gate saw the *same commit* pass at 150 ms and fail at
  266 ms on consecutive runs. NFR-2 therefore measures the runner before it
  judges keeper: it times a yardstick scan (a full, non-indexed aggregate over
  the same corpus — same SQLite, same pages, none of the code under test) and
  scales its budget by how much slower this box is than the reference. Every
  other gate stays absolute.
- **NFR-2 reference: an M1 MacBook Pro** (macOS 26.5, `cargo nextest`, dev
  profile — the same unoptimised build CI runs), where the yardstick's median is
  **32 ms** and search p95 lands at 123–126 ms against the flat 200 ms. That
  32 ms is `REFERENCE_SCAN` in the gate. Hardware at or above reference speed is
  held to the unscaled 200 ms; only a slower box earns proportional slack.
  The yardstick is calibrated within Apple Silicon, which is all CI runs: on an
  x86-64 Linux box it reads 2.7x slower while search is only 1.4x slower, so it
  over-grants there and the gate is correspondingly loose. Acceptable, because
  no CI job runs on one.
- Release-time manual measurements are taken on reference Apple Silicon hardware
  with a realistically seeded install (a 100k+-event archive; 1 and 5 configured
  accounts). Intel Macs are out of scope by design.

## Gate table

| # | Dimension (PRD) | Threshold | Enforcement point |
| --- | --- | --- | --- |
| NFR-1 | Cold start to interactive inbox | < 2 s (full) | **CI slice:** `src-tauri/crates/keeper-core/tests/cold_start_perf.rs::local_init_under_budget_at_100k_events` guards the deterministic offline **subset** (archive open + boot registry reads on a seeded ≥100k-event archive) under `LOCAL_INIT_BUDGET`. **Full figure:** release checklist (needs the webview + lazy per-account SDK activation). |
| NFR-2 | Offline FTS first results | < 200 ms p95 at 100k+ events | **CI:** `src-tauri/crates/keeper-core/tests/archive_search_perf.rs::search_p95_under_200ms_at_120k_events` (120k-event corpus, over the 100k+ threshold). Budget is machine-scaled: `allowed = 200 ms x max(1, scan / REFERENCE_SCAN)` — see "Reference hardware" above. |
| FR-48 | Command palette latency | ≤ 100 ms at 10k chats | **CI:** `src-tauri/crates/keeper-core/src/palette.rs::latency_under_100ms_at_10k_entries` (10k-entry index). |
| NFR-3 | Idle memory | ~500 MB with 5 accounts, ~300 MB with 1 account | **Release checklist — measure & flag.** See the assumption note below; these budgets are **not** hard CI gates. |
| NFR-8 | Crash safety (zero lost persisted events on kill) | Zero previously-committed rows lost; `PRAGMA integrity_check` = `ok` | **CI:** `src-tauri/crates/keeper-core/tests/crash_safety.rs` (real SIGKILL of a writing child for archive ingest, outbox insert, and settings write). |
| NFR-6 | Bridge-health drop reflected + notified | ≤ 60 s | **CI (logic):** the immediate disconnect-**notice** path in `src-tauri/crates/keeper-core/src/bridges/health.rs` (`disconnected_notice_flips_immediately_*`, `aggregator_notifies_once_on_transition_into_disconnected`) flips state with no debounce and notifies once. **No hard ≤ 60 s CI gate** covers the *silent-drop* (liveness-tick) case — its worst case can exceed 60 s (see below). **Live end-to-end:** release checklist. |
| NFR-22 | Segment handover gaplessness | Concatenated manifest PTS bounds monotonic; no boundary gap/overlap > one frame (`P = 1/frameRate`) | **CI:** the `keeper-rec` concat gate (`tools/keeper-rec/Tests/keeper-recTests/ConcatAssert*.swift`, Story 17.4) in the `recording` job — asserts per-boundary `ptsStart(k+1) − ptsEnd(k) ≈ P` from the session manifest's host-clock bounds plus intra-file PTS monotonicity via `AVAssetReader`, on fixtures generated on the runner (extends AD-21's measurement-hook discipline; negative controls prove the check is non-vacuous). |
| NFR-23 | Sync scale (200 000 files / 500 GB / 100 000 LFS objects, removable APFS) — bounded memory | Peak RSS bounded; a 2 GiB object handled in tens of MiB | **CI:** none. **Release checklist:** `docs/sync.md` §19 (the Linux `file://` envelope and the reference folder). The reference folder is the owner's, on hesperia; the number that matters is walk cost per event (NFR-61), not first-pass time. |
| NFR-24 | Sync durability (no data loss under `kill -9`, unplug, network loss) | Zero lost journaled units; the journal re-drives every interrupted unit | **CI:** `src-tauri/crates/keeper-sync/tests/durability_matrix_stages.rs` + `src-tauri/crates/keeper-syncd/tests/durability_matrix.rs` (real SIGKILL of the daemon at each stage boundary). |
| NFR-25 | Sync responsiveness (a settled change is committed within the quiescence window + one tick) | settle ≤ 5 s (10 s removable, 60 s ceiling) + 1 s | **CI:** the stability-gate suite in `src-tauri/crates/keeper-sync/src/stability.rs` and the tick-driven engine tests (`engine::tests`) that commit after settling. |
| NFR-26 | Sync secrecy (no credential or content in logs) | No userinfo, token or worktree byte in any log line | **CI (per site):** `git/cli.rs` scrub tests, `credential.rs`, `lfs/basic.rs`. **No whole-log scan test exists** — the Phase 4 summary's "23.6 log-scan test" was never written (review 2026-09-08, F-BMAD-9). |
| NFR-61 | Sync cadence (≤ 1 full-tree walk per minute under continuous single-file write; work ∝ change) | `status_walks` per hour on the reference folder | **CI:** `engine::tests` — a wake naming one path narrows the walk (`scanned ≤ 2`), a settling path's wake does not walk, two wakes inside the floor walk once, a live watcher backstops at 300 s, `sync_once` walks once. **Release checklist:** walks/hour on hesperia's `tgdrive` during a recording (§19). |
| NFR-62 | Sync silence (no network leg past its deadline without a log line; `Offline` within one poll) | fetch deadline 10 min, git-child deadline 10 min, LFS stall 90 s, connect 15 s | **CI:** `git::fetch::tests` (the hesperia connect-timeout string → `Network`), `git::cli::tests` (a child past its deadline is killed), `engine::tests` (a remote that accepts and never answers is `Offline` within the deadline; the transition logged once). |
| NFR-4 | Opening a conversation (chat switch < 150 ms) | First batch before any server read; a window that draws next to nothing fills itself in Rust | **CI (ordering, story 102.6):** `src-tauri/crates/keeper-core/src/timeline/open_tests.rs` — the first batch is sent while the header's server search is held; a window of claim renewals fills itself page by page and stops at 10 drawn items; a window that draws enough asks no page; a failed page is said once; a stalled page fails at its deadline; a window unloaded by a limited sync after reaching its start fills again; a Retry and "Load earlier messages" page on until something older shows; the header's search never meets a page of pagination (R509, R510). **Live (`#[ignore]`, delectra Synapse):** `tests/agents_matrix_live.rs::an_agent_room_behind_claims_shows_its_last_message_at_once` (220 claims after the last message, restarted warm cache: shown < 500 ms) and `::an_open_room_keeps_its_window_through_a_burst`. **Release checklist:** open timing on hesperia's agent rooms. |

All CI-enforced gates except NFR-22 run inside the existing required
`Rust (fmt, clippy, test)` cargo-nextest job — there is no separate perf CI job,
so a regression fails the same required check that already runs the 120k FTS
gate. NFR-22 runs in the `recording` job (`swift test` via
`scripts/test-keeper-rec.sh`), which `docs/release.md` lists among the required
status checks.

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

## The local socket, measured (123.1)

Story 123.1 measures keeper's local socket before 123.3 builds it: gRPC
(`tonic` 0.14) over a Unix socket against newline-delimited JSON over the
same socket and against the same call in-process, real MCP clients through
a stdio program on `rmcp`, and what another program of the same person can
already read of keeper. The spike's code lives on the throwaway branch
`spike/123-1`, never on `main`, and is kept as a git bundle with the story.

- **Base:** keeper `main` at `df74c488` (2026-11-24).
- **Hosts:** delectra (Linux amd64) and hesperia (macOS). The Linux dev
  container is used for no figure.

### Thresholds (committed before any figure)

For one connection, warm, and a 4 KiB call whose input is checked against
its JSON Schema before it is answered:

| # | Threshold | Hosts |
| --- | --- | --- |
| T1 | The wire adds at most **0.5 ms at p50** and **2 ms at p99** to the in-process call (gRPC over the socket minus the direct function call). | delectra, hesperia |
| T2 | gRPC's added p50 is at most **1.5 ×** NDJSON's added p50 over the same socket. | delectra, hesperia |
| T3 | The wire and `rmcp` add at most **12 MiB** to `keeper-cli`'s stripped release binary. | delectra, hesperia |
| T4 | The wire and `rmcp` add at most **8 MiB** to the app's stripped release binary. | hesperia |

A threshold is met only by a figure taken after the commit that wrote this
table. Each figure below names its run date beside this commit's.

### Figures

**hesperia** (macOS 27.0, arm64, `rustc` 1.99.1, `tonic` 0.14.9, `prost`
0.14.4, `rmcp` 3.10.0; spike `6e1b4f76` from base `df74c488`; run
2026-11-24 17:51–17:58 UTC, after the thresholds commit `c4dda2bf` of
17:18 UTC). The owner's own account, while he worked: load average 6 to 15
during the run, so the tails are pessimistic.

W1: the schema-checked echo, 10 000 warm calls, p50 / p99 in µs, one connection:

| Payload | gRPC, socket | NDJSON, socket | `tonic` over `duplex` | direct call |
| --- | --- | --- | --- | --- |
| 64 B | 32.9 / 64.1 | 8.0 / 16.1 | 14.8 / 28.6 | 0.1 / 0.2 |
| 4 KiB | 42.4 / 86.0 | 14.2 / 33.8 | 27.7 / 37.2 | 0.2 / 0.2 |
| 64 KiB | 302.0 / 537.0 | 196.8 / 340.9 | 135.8 / 258.6 | 0.9 / 1.2 |
| 1 MiB | 3 903 / 6 329 | 2 942 / 4 177 | 1 498 / 1 945 | 19.7 / 54.1 |

At 8 and 64 concurrent calls, gRPC shares one connection and NDJSON opens
one connection per caller. 4 KiB at 64 concurrent calls: gRPC 1 575 / 3 488,
NDJSON 233 / 974. One run of gRPC at 64 B and 64 concurrent calls broke with
`BrokenPipe` ("stream closed because of a broken pipe") and was recorded as
a failure, not a figure. 123.3 should look at this before it relies on one
shared connection. The full table is the run's `w1.json`.

W4: `keeper-cli`, stripped release binary: 10 055 296 B without the wire;
14 575 408 B with `tonic`/`prost`/`tower` and the input schema check
(`jsonschema`); 16 957 760 B with `rmcp` as well (+6.58 MiB). The app's
binary was not built on hesperia; it is owed.

W8: the three fixed tools' `tools` array is 590 B and 124 tokens
(`tiktoken-rs` 0.9.1, `o200k_base`). Each pinned candidate adds 222–243 B
(46–52 tokens). Two runs give identical bytes and SHA-256.

**delectra** (Linux 6.17.0-41-generic, x86_64, `rustc` 1.99.0, `tonic`
0.14.9, `prost` 0.14.4, `rmcp` 3.10.0; spike `6e1b4f76` from base
`df74c488`; run 2026-11-24 19:34–19:43 UTC, after the thresholds commit
`c4dda2bf` of 17:18 UTC). Command, on the spike branch:
`CLEAN=1 bash src-tauri/crates/spike-wire/scripts/run-linux-legs.sh <out>`.
Load average 0.29 at the start of W1.

W1: the schema-checked echo, 10 000 warm calls, p50 / p99 in µs, one connection:

| Payload | gRPC, socket | NDJSON, socket | `tonic` over `duplex` | direct call |
| --- | --- | --- | --- | --- |
| 64 B | 172.9 / 336.9 | 37.0 / 80.1 | 107.5 / 190.0 | 0.1 / 0.1 |
| 4 KiB | 179.9 / 303.8 | 42.3 / 94.2 | 123.7 / 210.5 | 0.2 / 0.2 |
| 64 KiB | 277.8 / 539.2 | 144.8 / 257.6 | 244.9 / 432.4 | 1.1 / 2.4 |
| 1 MiB | 3 341 / 5 957 | 2 241 / 4 184 | 3 350 / 5 266 | 22.2 / 60.3 |

4 KiB at 64 concurrent calls: gRPC 2 453 / 4 491, NDJSON 265 / 736. 47 of
48 cells ran; the 48th, gRPC at 64 B and 64 concurrent calls, broke with the
same `BrokenPipe` as on hesperia and is recorded under `failures` in the
run's `w1.json`. The break is therefore seen on both hosts, not only under
hesperia's load.

W4: `keeper-cli`, stripped release binary: 11 769 488 B without the wire;
17 067 832 B with `tonic`/`prost`/`tower` and `jsonschema`; 19 720 952 B
with `rmcp` as well (+7.58 MiB). Build times: base 89 s after
`cargo clean`, then +gRPC 46 s and +`rmcp` 62 s incremental, and 6 s after
touching `spike-wire`. delectra builds through `sccache`, so the "clean"
build was sccache-warm, not cold. `cargo deny` is not installed there; the
crates added to `Cargo.lock` are listed in the run's `added-crates.txt`
without a licence check.

W7 (a), `rmcp`'s client only: in `Discover` mode the server answered
`2026-07-28`, in `Initialize` mode `2025-11-25`; both listed the three
tools and called `search_operations`, `describe_operation` and
`call_operation` through the socket, every line teed.

W8: identical to hesperia: the three fixed tools' `tools` array is 590 B and
124 tokens (`tiktoken-rs` 0.9.1, `o200k_base`); with one pinned candidate
833 B / 176 (`graph.query`), 819 B / 172 (`notes.search`), 812 B / 170
(`sessions.list`). Two runs give identical bytes and SHA-256.

W12, `bwrap` 0.11.0 against the spike's socket at
`~/.spike-123-1/wire/wire.sock`: with `--ro-bind / / --unshare-all` and the
home visible, **connected**; with `--tmpfs $HOME` as well, **refused**
(transport error). Not a gate (CR-9).

| # | hesperia | delectra |
| --- | --- | --- |
| T1 | **met**: +42.2 µs p50, +85.7 µs p99 | **met**: +179.8 µs p50, +303.6 µs p99 |
| T2 | **not met**: gRPC's added p50 is 3.01 × NDJSON's (42.2 / 14.0 µs) | **not met**: 4.27 × (179.8 / 42.1 µs) |
| T3 | **met**: +6.58 MiB | **met**: +7.58 MiB |
| T4 | not measured (app build owed) | — |

**Owed** (no figure is given for any of them):

- W4: the app build and T4 (hesperia); `cargo deny check licenses bans
  sources` over the added crates (both hosts).
- W7 (a) in Claude Code, W7 (b) in Claude Code, Cursor and VS Code, and
  Claude Code's `/context` figure for W8 (hesperia, after the owner signs
  Claude Code in).
- W11's Linux leg (delectra, behind the program's keeper-hq instance) and
  W11 (a)–(d) on the Mac.
- W12's Mac leg.
- The second-user legs: W9's two refusals on delectra and a second
  logged-in session on hesperia.
- W2, W3, W5, W6, W9's uid/gid/pid and W10 were run on neither host.
