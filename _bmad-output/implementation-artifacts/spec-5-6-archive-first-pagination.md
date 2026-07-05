---
title: 'Archive-First Pagination'
type: 'feature'
created: '2026-07-05'
baseline_revision: '24daeaf3a32004bbbe41c0150b4feb9998ae056f'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
final_revision: '47f8f7a2f7d1b99c7c4116f1bbd5bbdb2e77a880'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Story 3.9 delivered homeserver back-pagination, but keeper enables matrix-sdk's `.sqlite_store()` only for the state + crypto stores and never subscribes the SDK **event cache** early. So the persisted-event-cache leg is dormant: scrollback past the live sync window issues a homeserver `/messages` request immediately and fails offline. FR-17 promises keeper "back-paginates from the Local Archive first, then the Homeserver, seamlessly," and the architecture spine assigns this to the persisted event cache in the sdk dir (SPINE storage rule; FR-8–17 → `timeline`+SDK; NFR-1–4 → event cache) — not to `archive.db` (which owns FR-33–37 only).

**Approach:** Subscribe the client event cache once at account activation, **before `sync.start()`** (mirroring the archive-handler registration), so every synced batch persists continuously into the on-disk `SqliteEventCacheStore` that `.sqlite_store()` already provisions. `Timeline::paginate_backwards` then serves older events from local disk first — instant and offline — and only reaches the homeserver at the true gap. The existing Story 3.9 diff-stream and honest history-boundary carry the UX unchanged; no new IPC, VM, or `archive.db` read is introduced.

## Boundaries & Constraints

**Always:** Pagination stays in the SDK/`timeline` layer (SPINE: FR-8–17 → `timeline`+SDK; NFR-1–4 → event cache). The frontend remains a pure index-based mirror of the SDK Timeline diff stream (AD-9, AD-20) — ordering is never re-derived and events are never synthesized in TS. Older events continue to arrive as `PushFront`/`Insert` ops on the existing timeline channel. The event-cache `subscribe()` is idempotent and runs before `sync.start()` so the first sync batch is captured. `keeper-core` stays tauri-free; no `.unwrap()` in production paths.

**Block If:** FR-17's "Local Archive first" provably cannot be served by the SDK persistent event cache and genuinely requires feeding `archive.db` rows into the live SDK Timeline — that would violate AD-9/AD-19/AD-20 (SDK owns ordering; `unique_id`s are stable only within one Timeline instance) and there is no SDK API to inject external historical events. HALT rather than invent an event-injection path.

**Never:** Do not read `archive.db` into the timeline or build a parallel keeper-owned prepend region above the SDK window (unavoidable overlap/dedup at the archive→homeserver handover; breaks AD-19). Do not add a pagination IPC command or VM (the 3.9 surface stands). Do not change the `archive.db` schema or any timeline VM. Do not let the boundary spin forever while offline.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Scroll back within persisted history | older events on disk in the event cache | events render immediately from disk, no `/messages`, seam invisible | none |
| Scroll past the persisted floor, online | cache exhausted, network up | one homeserver page; boundary shows homeserver loading; events prepend | fetch failure → retriable inline `error` boundary |
| Scroll past the persisted floor, offline | cache exhausted, offline | boundary states older history needs a connection and **stops** (no infinite spinner) | no network attempt |
| Reach room creation | homeserver start reached | boundary "This is the start of the conversation"; no further pagination | none |
| Restart, reopen a previously-synced room | persisted event cache on disk | prior history is present and scrollback is served from disk, offline | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/account.rs` — in `activate()` (the single restore/login funnel that registers the archive + redaction handlers before `sync.start()`, ~line 2386–2451), call `client.event_cache().subscribe()` right alongside those handlers and **before** the `SyncService` is built/started. Idempotent; the spawned `room_updates_task` is owned by the `Client` (aborts on logout with the sdk dir). This one call is the whole archive-first enablement. Add a doc comment tying it to FR-17 and the SPINE event-cache storage rule.
- `src-tauri/crates/keeper-core/src/timeline.rs` — no structural change. `paginate_backwards` (line 672) and `run_pagination_status_producer` (line 713) already drive the same diff stream + honest boundary; older events now resolve event-cache-first. Update the module/`paginate_backwards` doc note to say older events are served from the persisted event cache before the homeserver. Confirm `map_pagination_status` stays honest for local vs. network pagination.
- `src/components/chat/history-boundary.tsx` + `src/components/layout/conversation-pane.tsx` — verification only (no behavior change expected): the `paginating` / `offline` / `atStart` / `error` states already satisfy AC-2, and the `offline` row already stops without spinning. See Design Notes for why the "loads from your homeserver" copy stays.
- `src-tauri/crates/keeper-core/tests/event_cache_pagination.rs` (new) — integration test asserting the enablement invariant: a `Client` built on a temp `.sqlite_store()` reports `event_cache().has_subscribed() == false` before and `== true` after `subscribe()`, and a repeat `subscribe()` is a cheap no-op (the idempotency `activate()` relies on). Where a Story 1.8-style offline session harness is reachable, extend it to assert `has_subscribed()` after `activate()`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/account.rs` — subscribe the event cache in `activate()` before `sync.start()` (the archive-first enablement); doc-comment the FR-17/SPINE rationale.
- [x] `src-tauri/crates/keeper-core/src/timeline.rs` — doc note that back-pagination is now event-cache-first; confirm `map_pagination_status` honesty (no structural change).
- [x] `src/components/chat/history-boundary.tsx` + `src/components/layout/conversation-pane.tsx` — verify the offline-stop and homeserver-loading boundary states satisfy AC-2 (no code change unless a gap is found).
- [x] `src-tauri/crates/keeper-core/tests/event_cache_pagination.rs` (new) — test the `has_subscribed()` before/after `subscribe()` and idempotent-repeat invariant that `activate()` depends on.

**Acceptance Criteria:**
- Given a previously-synced Chat reopened while offline, when the user scrolls back, then older events render from the on-disk event cache with no homeserver request and the seam is invisible in normal use (FR-17, NFR-1).
- Given scrollback reaches the persisted floor while offline, then the boundary states older history needs a connection and stops; and when online it issues a single homeserver page behind the loading boundary (FR-17).
- Given account activation, then `client.event_cache().subscribe()` runs before `sync.start()` so every synced batch persists continuously — not only for rooms opened during this session.
- Given a 10k-event backscroll, then pagination stays freeze-free and scroll stays smooth (NFR-4): page size stays bounded, pages are disk-served and non-blocking, and the frontend windowing/diff-mirror is unchanged.

## Spec Change Log

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 0, low 2)
- defer: 3
- reject: 6
- addressed_findings:
  - `[low]` `[patch]` **`subscribe()` failure was fatal to the whole account activation.** The enablement mapped any `EventCacheError` to `AccountError::RestoreFailed` and `?`-propagated, so a (in matrix-sdk 0.18 effectively-unreachable) subscribe failure would abort a fully-usable account over a scrollback-cache enhancement. Changed to `if let Err(e) { tracing::warn!(...) }` + continue — degrades to homeserver-only back-pagination, matching the infallibly-registered archive/redaction handlers (enhancement, not precondition).
  - `[low]` `[patch]` **Ownership/teardown comment overstated the safety guarantee.** The comment claimed a single room-updates task "owned by the `Client` (aborted with the sdk dir on logout)." Tightened to state that `subscribe()` spawns multiple SDK-internal tasks aborted on the last `Client` clone dropping (not by an explicit `shutdown()` teardown), that `shutdown()` stops sync first (awaited) to quiesce the writer before dir deletion, and that tightening that ordering is tracked as deferred work.
  - Deferred (3): (1) event-cache tasks are the only SQLite-holding tasks not explicitly sequenced before `sign_out_cleanup`'s `remove_dir_all` — bounded today by `sync.stop().await` quiescing the writer and substantially pre-existing (lazy subscribe already spawned them), with no public SDK API to stop them; (2) automated behavioral coverage of activation-time persistence + archive-first `paginate_backwards`-from-disk (the test asserts only `has_subscribed()`); (3) persisted event-cache store growth/retention + duplication with `archive.db` now that all synced rooms persist from first sync.
  - Rejected (6): test temp-dir cleanup/collision (follows the existing `archive_search_perf.rs` convention; single test, negligible); `map_pagination_status` "stays honest" doc (documented deliberate decision — the SDK cannot distinguish a local vs. network page; behavior unchanged); before-`sync.start()` placement rationale (the conservative correct ordering, matching the archive handler); review-diff vs. committed-file mismatch (an artifact of the abbreviated review prompt, not the code); subscribe-then-later-activation-failure path (no `remove_dir_all` on the activation-error path, so no store-handle race); concurrent multi-account task count (inherent to AD-19 per-account supervision — each account already owns its own sync/producer tasks).

## Design Notes

**Why the event cache, not `archive.db` (spine-forced).** The architecture spine assigns FR-8–17 pagination to `timeline`+SDK and NFR-1–4 to "windowing, event cache," and lists the event cache as a persisted matrix-sdk-sqlite store under `accounts/<id>/sdk/`; the `archive` module owns FR-33–37 only. Two hard invariants make `archive.db`-into-timeline impossible anyway: the frontend is a pure **index-based mirror** of the SDK-owned `Vector` (AD-9/AD-20), and SDK `unique_id`s are stable only within one `Timeline` instance (AD-19). A keeper-owned prepend of `archive.db` rows above the SDK window would collide at the archive→homeserver handover (the SDK's next `/messages` fetch starts from *its* oldest item and re-fetches the range the archive already showed → duplicates). So "Local Archive first" is realized by the SDK's persisted event cache, which the spine already places in the sdk dir. `.sqlite_store()` provisions the `SqliteEventCacheStore`; the only missing wire is subscribing early.

**Why early subscribe.** `EventCache::subscribe()` spawns one task listening to *all* room updates and writing them to the store. `TimelineBuilder::build()` calls it lazily (first timeline open), so today only rooms opened this session persist. Subscribing at activation before `sync.start()` — exactly as the archive handler is registered — closes the gap so any Chat has instant, offline scrollback later. It is idempotent (`get_or_init`), so the later lazy call is a no-op; the tasks live on the `Client` and are aborted when the account (and its sdk dir) is dropped on logout, keeping the "logout deletes the sdk dir, nothing else" rule intact.

**Boundary copy stays.** The SDK's `PaginationStatus` does not distinguish a local-cache page from a network page. In normal use a local page is a sub-perceptible flash, so the boundary effectively appears only when a page takes real time — a genuine homeserver fetch — where "Older history loads from your homeserver" is accurate. This satisfies AC-1 (invisible seam) and AC-2 (homeserver-loading indicator) without a dishonest local-vs-network guess.

## Verification

**Commands:**
- `bun run check:rust` — expected: `cargo fmt --check` + `clippy --all-targets -- -D warnings` clean; `keeper-core` stays tauri-free; no `.unwrap()`.
- `bun run test:rust` — expected: cargo-nextest green incl. new `event_cache_pagination` invariant test.
- `bun run check` — expected: biome + tsc + vitest green; no timeline VM or IPC binding diff.
- `bun run check:all` — expected: full gate green incl. `tauri build --no-bundle`; `bindings:check` shows **no** VM/schema change.

**Manual checks (real homeserver / offline, OQ-1 territory):**
- Scroll back a Chat → instant, no visible seam; drop the network → past the persisted floor the boundary reads "older history needs a connection" and stops; restart offline → prior history is still scrollable from disk; 10k-event backscroll stays freeze-free (NFR-4).

## Auto Run Result

Status: done

**Summary:** Delivered Story 5.6 — archive-first back-pagination (FR-17; NFR-1, NFR-4). The architecture spine assigns FR-8–17 pagination to `timeline`+SDK and NFR-1–4 to the persisted **event cache** in the sdk dir — not to `archive.db` (which owns FR-33–37 only), and which cannot feed the SDK-owned `Timeline` without violating AD-9/AD-19/AD-20. The enablement gap was concrete: `.sqlite_store()` already provisions a `SqliteEventCacheStore`, but the event cache was only subscribed lazily by `TimelineBuilder::build()` on first room open, so synced history wasn't persisted continuously and scrollback past the sync window hit the homeserver immediately (and failed offline). The fix subscribes the client event cache once at account activation, **before `sync.start()`** (alongside the archive/redaction handlers), so every synced batch persists to disk from the first sync; `Timeline::paginate_backwards` then serves older events from the local event cache first — instant and offline — reaching the homeserver only at the true gap, over the same Story 3.9 diff stream and honest history-boundary (unchanged). No new IPC, VM, or `archive.db` read; no schema or timeline-VM change. Post-review, the subscribe failure posture was softened from fatal-to-activation to warn-and-continue (archive-first is an enhancement, not a precondition), and the teardown/ownership comment was tightened.

**Files changed:**
- `src-tauri/crates/keeper-core/src/account.rs` — `activate()` subscribes the SDK event cache before `sync.start()`; a subscribe failure logs a warning and continues (degrades to homeserver-only pagination) rather than aborting activation; doc-commented with the FR-17/SPINE rationale and the teardown-ordering note.
- `src-tauri/crates/keeper-core/src/timeline.rs` — doc-only: module header + `paginate_backwards` note that back-pagination is now event-cache-first; `map_pagination_status` confirmed honest, no structural change.
- `src-tauri/crates/keeper-core/tests/event_cache_pagination.rs` (new) — integration test asserting the enablement invariant: `has_subscribed()` is false before and true after `subscribe()`, and a repeated `subscribe()` is an idempotent no-op (offline client on a temp `.sqlite_store()`).
- Frontend (`history-boundary.tsx`, `conversation-pane.tsx`) — verified only, no change: the `paginating`/`offline`/`atStart`/`error` boundary states already satisfy the homeserver-loading + offline-stop ACs.

**Review:** 1 pass, no intent_gap / no bad_spec (no loopback). Patches applied (2, both low): warn-and-continue on subscribe failure; corrected teardown/ownership comment. Deferred (3): event-cache task teardown ordering vs. sdk-dir deletion on logout (bounded by `sync.stop().await`, substantially pre-existing, no public SDK stop API); automated behavioral coverage of persistence + paginate-from-disk; persisted event-cache store growth/retention + duplication with `archive.db`. Rejected (6): test temp-dir convention, by-design pagination-status doc, before-sync placement, review-diff artifact, activation-error path, per-account task-count.

**Verification:** `bun run check:rust` — PASS (rustfmt + clippy `-D warnings` clean; keeper-core tauri-free; no `.unwrap()`). `bun run test:rust` — PASS (410/410, incl. new `event_cache_pagination`). `bun run check` — PASS (biome + tsc + vitest 591/591) per the implementation run; the post-review patch is Rust-only. Manual homeserver/offline scroll + 10k-smoothness checks remain OQ-1 / Epic 11 territory.

**Residual risks:** The FR-17 user-visible behavior (offline archive-first scrollback, invisible seam, 10k smoothness) is exercised manually, not by automation (deferred). Logout teardown ordering of the SDK event-cache tasks is bounded but not explicitly sequenced (deferred). Broadened on-disk persistence (all synced rooms) increases event-cache store growth and overlaps `archive.db` (deferred).
