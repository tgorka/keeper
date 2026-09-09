---
title: 'Story 70.2: offline is a word the folder reaches'
type: 'fix'
created: '2026-09-09'
status: 'review'
baseline_revision: '3cb3fd7'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-516, FR-517, FR-518, NFR-62 → AD-228; AD-49 (offline is a state, not a failure — made reachable), AD-53 (a credential never becomes a subprocess — made unconditional)
source: `review-sync-2026-09-08` lanes `engine.md` (F-engine-1, -2, -7), `db.md` (F-db-1, -2, -3), `pullpush.md` (F-PULLPUSH-3, -4, -5), `deps.md` (F-Deps-2)

<intent-contract>

## Intent

**Problem:** hesperia's remote was unreachable for six days and no folder ever said so. The transport's own wording — `tcp connect error: deadline has elapsed` — matches none of the ten needles `git::fetch::classify` had, so every attempt fell through to `SyncError::Git` (Transient, not `Network`), `record_failure` never wrote `Offline`, `remote_within_reach` never gated, and the tree was walked 1 371 times for a remote that could not take a byte. Four smaller faults share the shape "the engine has the word and never reaches it": the "failed N times in a row" counter is reset by the same `tick` that incremented it, so `NeedsAttention` cannot fire for journaled work; the offline transition is `debug!`, which no shipped build emits; a fetch has a 20 s connect timeout and nothing after it, and a `git` child has nothing at all, so a peer that accepts and goes silent parks the profile forever; and a fetch with no stored credential silently borrows whatever account the machine's `credential.helper` chain holds for that host.

**Approach:** classify by *type* before text (`reqwest::Error::is_connect()/is_timeout()` and a connection-shaped `io::ErrorKind` anywhere in the gix error chain), and extend the text list for the shim and for whatever type the chain does not carry. Bound the fetch with `tokio::time::timeout(FETCH_DEADLINE)` around the existing `spawn_blocking`, interrupting gix through a per-fetch flag and answering `Network`. Bound every `git` child with `GIT_DEADLINE` (spawn, poll, kill) and give every in-repository invocation `http.lowSpeedLimit/lowSpeedTime`. Move the counter reset to the two places a unit or a pass genuinely succeeds. Log the offline edge once each way at `info!` off a sticky per-profile set, the way `task_faults` keeps `note_task_outcome` honest. Open the repository for fetch with `credential.helper=` overridden in memory as `clone` already does, and install the static callback unconditionally so "no credential" is answered with a refusal that classifies `Auth`.

**Why the process-wide `interrupt` is not what the deadline sets.** `Engine::interrupt` is the shutdown flag; it is set after the supervisor loop exits and is never cleared. A deadline on one profile's fetch that set it would turn every later fetch and checkout in the process into `Cancelled` for the life of the process. The fetch therefore gets its own `Arc<AtomicBool>` — what gix reads as `should_interrupt` — seeded from the process flag, so nothing shutdown could say to a fetch today is lost (F-engine-11 records that shutdown cannot reach a running tick at all).

**Why `gitoxide.http.connectTimeout` is not set.** The key exists (`gix/src/config/tree/sections/gitoxide.rs:191`) and `gix/src/repository/config/transport.rs:267` reads it into `http::Options::connect_timeout` — which only the **curl** backend consumes (`gix-transport/src/client/blocking_io/http/curl/remote.rs:436`). keeper links `blocking-http-transport-reqwest-rust-tls`, and the reqwest backend hard-codes `.connect_timeout(20 s)` at `reqwest/remote.rs:64` and never reads the option. Setting the key would be an inert line claiming to do something; the tokio deadline is the bound that actually runs.

## Boundaries & Constraints

**Always:**
- `classify_error` answers `Cancelled` first, then whatever `classify_message` says (Auth and Diverged keep precedence), then the type walk, then `Git`.
- A type-classified failure names the kind in its `reason` so the log line says why it was called network when the text did not.
- `FETCH_DEADLINE` and `GIT_DEADLINE` are 10 minutes; a test overrides them through `#[cfg(test)]` setters, never by waiting.
- A `git` child past its deadline is killed **and reaped** before the error is returned; the error names the verb and is `Network`.
- `transient_failures` is incremented only in `record_failure`'s transient arm and reset only where a unit completes (`drain`'s `complete` site) or a pass succeeds (`mark_synced`).
- `sync offline` and `sync reachable again` are `info!`, once per onset each, keyed on a per-profile set; per-attempt lines stay `debug!`.
- `static_credential` is installed on every fetch and every clone; with no credential it answers `Get` with `Err(Quit)` so the handshake ends in `Failed to obtain credentials` → `Auth`.
- The fetch repository is opened with `credential.helper=` in memory (`open_for_fetch`), and `Credential`'s redacting `Debug` is unchanged.

**Block If:** nothing; every design question was answered by the gix source under `~/.cargo/git/checkouts/gitoxide-*/0ae3023`.

**Never:** set `Engine::interrupt` from a deadline; reset the counter in `tick`'s `Ok` arm; touch the merge verbs (70.3), `WalkPolicy` (70.1), `stability.rs` (70.5); log a credential.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| hesperia's exact text | `fetch from electra failed: An IO error occurred when talking to the server: error sending request for url (…): client error (Connect): tcp connect error: deadline has elapsed` | `SyncError::Network` | classified, not `Git` |
| type without text | an error whose Display is `boom` and whose source is `io::Error(TimedOut)` | `Network`, reason names `timed out` | classified by type |
| reqwest connect error | a real `reqwest::blocking` GET to a closed port | `network_cause` is `Some` | type walk finds it |
| Auth precedence | text carries `authentication failed` and the chain carries `ConnectionReset` | `Auth` | text first |
| counter through `tick()` | a fixture profile whose remote is a nonexistent `file://`; `tick()` until three units have failed | warning sticky, state `NeedsAttention`, one notification | reachable through the path |
| listener that never answers | remote `http://127.0.0.1:<port>/x.git`, socket accepted, no bytes; deadline 2 s | `Network` within the deadline; state `Offline` | the blocking thread is abandoned; the socket close at test end frees it |
| `git` child past deadline | fake `git` = `exec sleep 30`; deadline 300 ms | `Network` naming the verb, returned in well under 30 s, child reaped | killed |
| no credential, helper in config | 401 server; `.git/config` `credential.helper=!sh -c 'touch sentinel'`; no stored secret | `Auth`; sentinel absent | the callback refuses, gix never consults config |
| offline edge | Network failure ×2 then a completed unit | exactly one `sync offline`, one `sync reachable again` | once per onset |
| a pass that succeeds | `sync_once` Ok | counter and offline flag cleared by `mark_synced` | no second reset site |

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `src-tauri/crates/keeper-sync/src/git/fetch.rs` | `classify_error` (type walk via `network_cause`), `static_credential(Option<&Credential>, action)` answering `Quit` for none, unconditional install, `FETCH_DEADLINE`; tests |
| `src-tauri/crates/keeper-sync/src/git/cli.rs` | `GIT_DEADLINE`, `GitCli { deadline }` + `#[cfg(test)] with_deadline`, `capture` = spawn + capped reader threads + `try_wait` poll + kill; `repository_config_args` adds `http.lowSpeedLimit=1000`/`http.lowSpeedTime=60`; `NETWORK` needles + `tcp connect error`, `deadline has elapsed`, `timed out`, `connection closed`; `deadline_reason`; tests |
| `src-tauri/crates/keeper-sync/src/git/repo.rs` | `open_for_fetch` (in-memory `credential.helper=`); `clone` calls the new `static_credential` shape and `classify_error` |
| `src-tauri/crates/keeper-sync/src/engine.rs` | `fetch_deadline_ms` + `offline` fields; `record_failure` Network arm (`info!` on onset, per-attempt `debug!`); `tick` no longer resets the counter; `drain`'s `complete` site and `mark_synced` call `note_unit_succeeded`; `do_pull` opens with `open_for_fetch`, per-fetch interrupt, `tokio::time::timeout`; `sync_once_recording` loses its redundant reset; tests |
| `src-tauri/crates/keeper-sync/src/error.rs` | unchanged in shape — `Network { host, reason }` is the offline variant; noted for 70.3: `Refused` carries a `ContentRefusal`, not a string |

## Tasks & Acceptance

**Execution:**
- [x] `fetch.rs` — type-first classification (`classify_error`, `network_cause` incl. the `get_ref` step), unconditional credential callback answering `Quit`, `FETCH_DEADLINE`.
- [x] `cli.rs` — `GIT_DEADLINE` capture (spawn/poll/kill/reap, capped reader threads), low-speed limits, five new needles, `deadline_reason`.
- [x] `repo.rs` — `open_for_fetch`; `clone` on the new callback shape and `classify_error`.
- [x] `engine.rs` — deadline around the fetch (per-fetch interrupt), `note_unit_succeeded` at the two success sites, `tick` no longer resets, offline edge at `info!`, `do_pull` on `open_for_fetch`, `sync_once_recording`'s redundant reset removed.
- [x] tests for every matrix row; mutation proofs below.

**Acceptance Criteria:** see the matrix; each row is one test named in Verification.

## Design Notes

`classify` (text) stays as the shim's and the tests' entry; `classify_error` wraps it with the type walk and is what the three fetch sites and `clone` call. `network_cause` walks `source()` with `downcast_ref` for `reqwest::Error` and `std::io::Error`; gix wraps the reqwest error as `io::Error::other(err)` (`reqwest/remote.rs:203`), so the chain reaches it. HTTP 5xx is mapped by gix to `ConnectionAborted` and 401 to `PermissionDenied` — neither is in the kind set, deliberately.

The `git` child is read on two threads with `STDERR_CAP`-sized buffers so a megabyte of `push` output neither blocks the child on a full pipe nor lands in memory whole; the parent polls `try_wait` every 50 ms against the deadline.

## Verification

`cargo test -p keeper-sync --lib -- git::fetch::tests git::cli::tests::a_git_child git::cli::tests::a_repository_invocation git::cli::tests::transport_timeout three_failed_units a_remote_that_accepts a_unit_completing_after_offline a_fetch_with_no_credential` → 27 passed. `cargo check -p keeper-sync -p keeper-syncd --all-targets` → clean (with siblings' concurrent work in the tree).

| Mutation | Tests that failed | Observed |
|---|---|---|
| (1a) `classify_error`: type walk disabled | `a_connection_shaped_io_error_is_network_whatever_it_says` | 17 passed; 1 failed |
| (1b) needles `tcp connect error`, `deadline has elapsed` removed | `hesperias_connect_timeout_wording_is_network`, `cli::transport_timeout_wording_is_network` | 2 failed (`left: "git"`) |
| (1c) `network_cause`: `get_ref` step skipped | `a_reqwest_failure_is_found_through_gix_s_io_wrapper` | 1 failed |
| (2) fetch deadline pinned to `FETCH_DEADLINE` (setter ignored) | `a_remote_that_accepts_and_never_answers_is_offline_within_the_deadline` | failed on the elapsed assertion at 30.02 s; the folder still read `Offline` because gix's blocking reqwest client has its own default timeout and the type walk read `is_timeout()` |
| (3) `wait_within` never fires | `a_git_child_past_its_deadline_is_killed_and_classified_network` | failed after 30.02 s: the fake git returned `Ok` |
| (4a) reset restored in `tick`'s `Ok` arm | `three_failed_units_through_the_tick_reach_needs_attention` | failed: `None` ≠ `Some(1)`, "the count must survive the tick that made it" |
| (4b) `note_unit_succeeded` clears only the warning | `a_unit_completing_after_offline_logs_reachable_again_once` | failed: `sync reachable again` count 0 ≠ 1 |
| (5a) onset logged at `debug!` | `a_remote_that_accepts…`, `a_unit_completing_after_offline…` | both failed on the `sync offline` count |
| (6a) conditional callback **and** `open` instead of `open_for_fetch` | `a_fetch_with_no_credential_refuses_instead_of_asking_the_machine` | failed: "the machine's helper ran" (sentinel present) |
| (6b) conditional callback alone, override kept | none | **not caught** (2 runs): the override alone keeps the helper out; the unconditional callback is proven at unit level (`the_credential_callback_refuses_when_there_is_no_credential`) and buys determinism, not the sentinel |

Every mutation was one line, reverted by the same line; `fetch.rs` byte-identical to its pre-mutation copy, `cli.rs`/`engine.rs` identical in every line this story owns.

**Not verified here, and why.** hesperia's post-install measurement (card offline within one poll, `status_walks` flat) needs the Mac and a pf rule. The `keeper` shell crate was not touched. The abandoned blocking thread after a fetch deadline is documented, not measured. `gitoxide.http.connectTimeout` was deliberately **not** set — inert under the reqwest backend (see Intent). Observed during mutation (2): gix's `reqwest::blocking` client ends a silent fetch at 30 s on its own (reqwest's default blocking timeout; `[INFERENCE]` whether total or per-read — not checked in source before the budget ran out); the lane's "parks the profile forever" is too strong for the reqwest backend, and `FETCH_DEADLINE`'s doc comment says "no read timeout, no total" on the lane's authority — the coordinator may want that sentence softened.
