---
title: 'Offline Full-Text Search Engine'
type: 'feature'
created: '2026-07-05'
baseline_revision: '2e7a56df133febde10f2253ba0634cc38f00d646'
final_revision: 'eca02a364719f435543c53967018b9f3b59b14c2'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Stories 5.1/5.2 archive every synced message into `archive.db` as plain-text rows, but `content_json` is not indexed for search — there is no way to query the local archive at all, let alone the epic's promise of instant offline search (FR-34, NFR-2, AD-12). Without a search engine, "my entire archive indexed, milliseconds away" is unrealized and Story 5.4's search UI has nothing to build on.

**Approach:** Add an FTS5 **external-content** table (`events_fts`, `tokenize='trigram'`, case-insensitive, CJK-capable by construction) over a new indexed `body` column on `events`, maintained incrementally by the existing single writer at ingest. Add an archive-only, tauri-free `search` engine in `keeper-core::archive::fts` that MATCHes with trigram for queries ≥3 characters and falls back to a `LIKE` scan for shorter queries, applies sender / room / account / date-range filters, honors the remote-deletions setting, deduplicates edit versions to one hit per logical message, and returns `(account_id, room_id, event_id)` deep-link identifiers. Expose it over one IPC command with typed bindings, and gate p95 latency (<200 ms at 100k+ events) with a CI performance test. No search UI — that is Story 5.4.

## Boundaries & Constraints

**Always:**
- One `archive.db`, one serialized writer, WAL. FTS maintenance rides the **same writer connection** inside the same insert flow — no second writer, no separate FTS DB. Search runs on a **fresh read-only connection** (WAL permits concurrent readers); reads never touch the writer or a live Matrix session, so search works fully offline and (for 5.7) after sign-out.
- `events` gains a nullable `body TEXT` column populated at ingest via the shared body-extraction helper (`m.new_content.body` for edits, top-level `body` for originals, `""` for non-text). Migration is idempotent (`PRAGMA table_info` + `ALTER TABLE … ADD COLUMN`, nullable): a one-time backfill sets `body` for pre-existing rows (Rust pass over `content_json`), then the FTS table is created (`CREATE VIRTUAL TABLE IF NOT EXISTS`) and populated once via the FTS5 `'rebuild'` command. Re-runs are no-ops (guarded on FTS-table/column existence).
- `events_fts` is external-content (`content='events'`, `content_rowid='rowid'`) over the `body` column with `tokenize='trigram'` (default case-insensitive). Incremental indexing happens **only when the base `INSERT OR IGNORE` actually added a row** (rows-affected == 1) and `body` is non-empty, using that row's `rowid`; re-synced duplicates (rows-affected == 0) never double-index.
- Search dispatch: query length counted in **Unicode scalar values**. ≥3 → trigram `events_fts MATCH` (parameterized/quoted so query text is never interpreted as FTS operators). <3 → case-insensitive `body LIKE '%q%'` scan. Both order by `origin_ts DESC` and honor a bounded result `limit` (sane default cap, e.g. 200).
- Filters accepted by the engine are archive-native primitives: `account_ids` (empty ⇒ all), `room_ids` (empty ⇒ all — the boundary for **both** the "Chat" and "Network" UI filters), `sender`, `start_ts`/`end_ts` (`origin_ts` bounds). "Network" is a live per-room label (`RoomVm.network`), not an archive column, so Story 5.4 resolves a Network selection to its `room_id` set before calling; the tauri-free engine never sees bridge state.
- Honor-remote-deletions gate at read time: when the app-wide setting is on, redacted rows (`redacted_ts IS NOT NULL`) are excluded from results (content stays physically on disk). The command reads the setting via `archive::get_honor_remote_deletions` and passes it in.
- Edit versions: every version row is indexed (so prior, edited-away text stays searchable), but results are **deduplicated to one hit per logical message**, keyed by the chain root (`relates_to_event_id` if present, else `event_id`). The returned `event_id` is that chain root, so all versions deep-link to the same timeline item.
- Rust owns all logic; `keeper-core` stays tauri-free; no `.unwrap()`/bare `.expect()` in production paths; `?` + `thiserror` (`ArchiveError` → `CoreError` → `IpcError`); `tracing` with ids not content. New IPC types are `Vm`-suffixed, serde camelCase, `#[ts(export)]` into `src/lib/ipc/gen/`.

**Block If:**
- FTS5 or the trigram tokenizer is unavailable in the bundled SQLite at runtime and enabling it would require adding a GPL/AGPL component or a build-flag change with license implications (contradicts the cargo-deny firewall). (Expected available: `rusqlite` `bundled` compiles SQLite with FTS5 + trigram — no new dependency.)
- Maintaining the FTS index incrementally cannot be done through the single writer without either erasing archived content or breaking the one-writer invariant.

**Never:**
- No search UI, no result grouping/highlighting, no command palette, no keyboard shortcuts — all Story 5.4. No export (5.5), no archive-first pagination (5.6), no sign-out/delete-archive path (5.7); do not delete from `events_fts` (account-scoped FTS deletion is 5.7).
- No physical erasure of `content_json`/`body` on redaction or on toggling honor-deletions (mark-only durability holds); honoring is retrieval-gating only.
- No second `archive.db`, no second writer/connection for writes, no passphrase encryption (FileVault-only posture per epic).
- No network/bridge column in the archive and no live room-state access from the engine; Network filtering is a caller concern (room_id set).
- No crypto material, tokens, or full event content beyond the display body/snippet crossing IPC. `event_id` in results is the sanctioned deep-link identifier the epic AC mandates (see Design Notes) — not a general relaxation of the no-ids rule.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Index at ingest | new text `m.room.message` archived | base row inserted (rows-affected 1); `body` set; `events_fts` gets `(rowid, body)` | non-text/empty body → row stored, FTS insert skipped |
| Re-synced duplicate | same `(account_id, event_id)` re-ingested | `INSERT OR IGNORE` no-ops (rows-affected 0); **no** FTS insert | no error, no double-index |
| Trigram search ≥3 | query `"hello"`, matching rows | hits via `events_fts MATCH`, `origin_ts DESC`, ≤ limit, each with `(account_id, room_id, event_id=root)`, sender, body, timestamp | operator-like query text matched literally, not parsed |
| Short query <3 | query `"hi"` | `LIKE '%hi%'` case-insensitive fallback, same shape/order | — |
| CJK query | query `"日本語"` (≥3 scalars) | trigram matches CJK substrings case-insensitively | — |
| Filtered search | `account_ids`/`room_ids`/`sender`/date-range set | only matching rows returned; empty filter lists ⇒ unrestricted | invalid range (start>end) ⇒ empty result, no error |
| Honor deletions ON | redacted row would match, setting on | that row excluded from results; content stays on disk | setting read failure ⇒ `IpcError`, no partial leak |
| Honor deletions OFF | redacted row matches, setting off | row returned normally | — |
| Edit-version match | query matches only a prior (edited-away) version | one hit for the message, `event_id` = chain root; no duplicate for the current version | — |
| Pre-5.1/5.2 archive.db | existing DB lacks `body`/`events_fts` | migration adds `body`, backfills existing rows, creates + `'rebuild'`s FTS once; old rows searchable | re-open is a no-op |
| Perf gate | 100k+ event corpus, offline | p95 over a standard query set < 200 ms | CI test fails if exceeded |
| No matches | query with zero hits | empty `Vec`, not an error | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/archive/db.rs` -- add nullable `body TEXT` to the `events` `CREATE TABLE` + idempotent `PRAGMA table_info`/`ALTER` migration; `insert_event` writes `body`; expose whether a row was inserted + its `rowid` so the writer can index; a body backfill helper (`UPDATE … WHERE body IS NULL`) driven by the shared extractor; call `fts::ensure_fts` from `open_archive_db` after column migration.
- `src-tauri/crates/keeper-core/src/archive/fts.rs` -- NEW: `ensure_fts(conn)` (create external-content trigram vtable IF NOT EXISTS + one-time `'rebuild'` on fresh creation); `index_body(conn, rowid, &body)` (incremental insert, skip empty); `search(conn, &SearchFilter, honor_deletions) -> Result<Vec<SearchHitVm>, ArchiveError>` dispatching trigram-MATCH (≥3) vs LIKE (<3), applying filters + honor gate, dedup by chain root, ordering, limit.
- `src-tauri/crates/keeper-core/src/archive/mod.rs` -- `ArchiveEvent` gains `body: String`; re-export the `fts` module + `SearchFilter`.
- `src-tauri/crates/keeper-core/src/archive/ingest.rs` -- writer `Insert` arm: after a successful `insert_event`, call `fts::index_body` for the new rowid when body non-empty; failures logged (ids only), task never dies.
- `src-tauri/crates/keeper-core/src/account.rs` -- `build_archive_event` computes `body` via the shared extractor; relocate `display_body_from_content` into the archive module as `pub(crate)` and reuse it here, in `edit_history`, and in the backfill (one implementation, no drift).
- `src-tauri/crates/keeper-core/src/vm.rs` -- NEW `SearchFilterVm` (input: `query`, `accountIds`, `roomIds`, `sender`, `startTs`, `endTs`, `limit`; `Deserialize` + `#[ts(export)]`) and `SearchHitVm` (output: `accountId`, `roomId`, `eventId`, `sender`, `body`, `timestamp`, `redacted`; camelCase, `#[ts(export)]`). Map/hold a plain `SearchFilter` domain struct in `archive` if preferred, but the engine returns `Vec<SearchHitVm>`.
- `src-tauri/crates/keeper/src/ipc.rs` (+ `keeper/src/lib.rs`) -- command `search_archive(state, filter: SearchFilterVm) -> Result<Vec<SearchHitVm>, IpcError>`: resolve `data_dir`, open a read-only archive connection, read the honor setting, call `fts::search`; exhaustive error mapping; register in `generate_handler!`.
- `src/lib/ipc/client.ts` -- binding `searchArchive(filter): Promise<SearchHitVm[]>`; re-export `SearchFilterVm` + `SearchHitVm` from `gen/`. No UI.
- `src-tauri/crates/keeper-core/tests/archive_search.rs` -- NEW functional coverage of the I/O matrix.
- `src-tauri/crates/keeper-core/tests/archive_search_perf.rs` -- NEW 100k-corpus p95 CI perf gate.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/archive/db.rs` -- add nullable `body` column + idempotent migration; write `body` in `insert_event`; return insertion outcome + `rowid`; add the backfill helper; invoke `fts::ensure_fts` after migration -- indexed-text storage foundation.
- [x] `src-tauri/crates/keeper-core/src/archive/fts.rs` (new) -- external-content trigram FTS creation + one-time rebuild, incremental `index_body`, and the `search` engine (trigram/LIKE dispatch, filters, honor gate, chain-root dedup, ordering, limit) -- the search engine.
- [x] `src-tauri/crates/keeper-core/src/archive/{mod.rs,ingest.rs}` -- `ArchiveEvent.body`; writer indexes after a real insert -- incremental indexing through the single writer.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- relocate/share `display_body_from_content`; compute `body` in `build_archive_event` -- one body extractor for ingest, history, and backfill.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- `SearchFilterVm` + `SearchHitVm` with ts-rs export -- IPC view models.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (+ `lib.rs`) -- `search_archive` command (read-only connection, honor-setting read, exhaustive error arms) registered -- IPC surface.
- [x] `src/lib/ipc/client.ts` -- `searchArchive` binding + re-export the two generated types -- typed frontend access (no UI).
- [x] `src-tauri/crates/keeper-core/tests/archive_search.rs` (new) + inline `fts.rs`/`db.rs` unit tests -- cover every I/O & Edge-Case Matrix row: ingest indexing, duplicate no-double-index, trigram ≥3, LIKE <3, CJK, filters, honor on/off with a redacted row, edit-version match + chain-root dedup, empty-body skip, pre-5.1 migration+backfill, no-match empty -- verifies the matrix.
- [x] `src-tauri/crates/keeper-core/tests/archive_search_perf.rs` (new) -- build a 100k+-event corpus (bulk insert), assert p95 over a standard query set < 200 ms -- NFR-2 CI performance gate.

**Acceptance Criteria:**
- Given archive ingestion, when text rows are appended, then an FTS5 external-content table with `tokenize='trigram'` (case-insensitive, CJK-capable) indexes their body incrementally through the single writer, and only for rows the base insert actually added (AD-12).
- Given a 100k+-event archive with the network disabled, when the search command runs over a standard query set, then first results return in < 200 ms p95 verified by a CI perf test (NFR-2), and queries under 3 characters fall back to a `LIKE` scan (AD-12).
- Given the search command surface, when called with sender / room (Chat) / room-set (Network) / account / date-range filters, then it returns hits carrying `(account_id, room_id, event_id)` sufficient to deep-link, honors the remote-deletions setting, and returns at most one hit per logical message with the chain-root `event_id` (FR-34).
- Given a pre-5.1/5.2 `archive.db` with no `body` column or FTS table, when the app reopens it, then the column is added, existing rows are backfilled, the FTS index is built once, previously ingested rows become searchable, and re-opening is a no-op.

## Spec Change Log

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 6: (high 0, medium 5, low 1)
- defer: 2: (high 0, medium 1, low 1)
- reject: 26
- addressed_findings:
  - `[medium]` `[patch]` Honor-remote-deletions leaked an edited-then-redacted message: a remote redaction marks only the original row, so the per-row `events.redacted_ts IS NULL` gate let a surviving, un-redacted `m.replace` edit sibling surface the message via chain-root dedup even with the setting ON. Replaced the gate with an account-scoped root-redaction `NOT EXISTS` subquery on `COALESCE(relates_to_event_id, event_id)`, so the whole logical message is withheld when its root is redacted; added a `honor_deletions_excludes_edited_then_redacted_message` unit test.
  - `[medium]` `[patch]` Chain-root dedup keyed on the bare `event_id`, so the same Matrix event id archived under two of the user's accounts (both joined to one room) collapsed into a single hit — dropping a legitimately distinct, account-attributed result (contra the epic's cross-account result identity UX). Changed the dedup key to `(account_id, root)`; added a `cross_account_same_event_id_not_deduped` test.
  - `[medium]` `[patch]` `search_archive` opened `archive.db` read-only unconditionally, so a fresh install / never-synced account (no DB file yet) got `SQLITE_CANTOPEN` surfaced as an IPC error instead of empty results. The command now returns an empty vec when `db_path` does not exist.
  - `[medium]` `[patch]` `ensure_fts` created the FTS vtable and ran the one-time `'rebuild'` as two separate statements: a crash between them would leave an existing-but-empty `events_fts` that the exists-check skips forever, silently hiding the whole pre-existing corpus. Wrapped create + rebuild in a single `BEGIN IMMEDIATE`/`COMMIT` (rollback on error) so it is all-or-nothing.
  - `[medium]` `[patch]` `backfill_missing_bodies` issued one un-batched `UPDATE` per row with no wrapping transaction — on a large pre-5.3 archive that is up to N WAL commits at startup (potential multi-minute hang). Wrapped the backfill loop in a single transaction (all-or-nothing, one commit).
  - `[low]` `[patch]` The `<3`-char LIKE fallback bound the raw query, so `%`/`_` acted as wildcards (e.g. `"a%"` over-matched). Added `ESCAPE '\'` and an `escape_like` helper that escapes `\`/`%`/`_`; added a `short_query_wildcard_is_literal` test.
  - Rejected as by-design/out-of-scope/negligible (26): dedup roots on `relates_to_event_id` without a `rel_type` check (correct by construction — `build_archive_event` populates the column *only* for `m.replace`, verified by tests); the non-atomic base-insert + `index_body` "row committed but unindexed on index failure" (the proposed transaction fix regresses the archive's primary no-silent-loss invariant, and selective FTS-insert failure on the same healthy writer connection immediately after a successful insert is not a realistic mode — matches 5.2's accepted writer-resilience posture); perf-gate p95 "hides the LIKE path" (false — 10 of 90 samples are the LIKE query, so the p95 index 85 lands *within* the LIKE cluster) and "rebuild vs incremental index shape" (200 ms budget vs single-digit-ms actual absorbs segment-structure differences; incremental build would bloat test time); degenerate all-quote/whitespace MATCH input (pathological, tolerable error, 5.4 input layer sanitizes); ASCII-only `LOWER()` at the 2/3 boundary (the `<3` `LIKE` fallback is an AD-12-sanctioned degraded fast path); `scan_cap = limit*4` under-return (only under pathological long-edit-chain-dominated result sets at default limit 200; a total/pagination protocol is 5.4's concern); silent `limit` clamp / `limit=0→1` / no pagination (engine bounds results by design; 5.4 owns pagination); `SQLITE_OPEN_NO_MUTEX` footgun (correct for the per-call, single-thread-scoped connection today); huge `IN (…)` filter list vs `SQLITE_MAX_VARIABLE_NUMBER` (needs >32k filter entries — implausible); empty-string `sender`/negative-ts/unit validation (caller-controlled, tolerable); NULL-vs-`''` body convention not schema-enforced; test temp-dir leak on panic; backslash/colon inside the doubled-quote FTS string (literal inside a quoted FTS string); NFC-normalization / combining-mark and tokenized-length nuances (literal byte-sequence match holds; trigram does not strip); `last_insert_rowid` after an ignored insert (guarded by rows-affected == 1; `events` has no triggers/other UNIQUE); redacted-flag reflecting the matched version when honor is OFF (cosmetic — honor-OFF shows content anyway); reader-opens-pre-migration microsecond race and edit-root-absent deep-link (self-healing / 5.4 deep-link resolution concern).

### 2026-07-05 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 1, low 3)
- defer: 0
- reject: 8
- addressed_findings:
  - `[medium]` `[patch]` Honoring remote deletions still leaked a redacted **edit** version whose chain **root survived**. The prior pass replaced the per-row gate with a root-redaction `NOT EXISTS` subquery, but that gate keys on the *root's* `redacted_ts` while results are ordered newest-first and deduped to the first-seen row — so a redacted edit of an un-redacted original became the representative hit, returning its edited-away `body` with `redacted = true` even while honor-deletions was ON (violating the documented `SearchHitVm.redacted` contract and surfacing withheld content). Re-added a per-row `events.redacted_ts IS NULL` clause **alongside** the root gate: clause (1) keeps a redacted version from ever representing a message, clause (2) still withholds the whole message when its root is redacted. Added a `honor_deletions_hides_redacted_edit_of_surviving_root` unit test asserting the surviving original represents the message (`redacted = false`, original body) and that edit-only text is withheld under honor-ON yet retrievable with honoring off.
  - `[low]` `[patch]` The NFR-2 perf gate timed only the honor-OFF path, leaving the more expensive honor-ON query shape (per-row root-redaction `NOT EXISTS` subquery) — the path a privacy-conscious user actually runs — unmeasured. The gate now samples every standard query on **both** honor states at 120k events; p95 stays < 200 ms.
  - `[low]` `[patch]` `escape_like` handled `%`, `_`, and the `\` escape char, but the wildcard-literal test only exercised `%`. Extended `short_query_wildcard_is_literal` to assert `_` (single-char wildcard) and `\` (the ESCAPE char itself) are matched literally on the `<3`-char LIKE path.
  - `[low]` `[patch]` `open_archive_db`'s doc comment enumerated the pre-5.2/5.3 column set, omitting `relates_to_event_id`/`rel_type`/`redacted_ts`/`body`. Updated to reflect the current `events` schema.
  - Rejected as already-triaged / design-sanctioned / negligible (8): perf gate builds via bulk `'rebuild'` vs production incremental-index segment shape (already triaged; 200 ms budget vs single-digit-ms actual absorbs it); `open_readonly_archive_db` has no wrapping read transaction (single-statement reads are implicitly atomic today; a hypothetical 5.4 multi-statement reader is out of scope); `limit = 0 → 1` silent clamp (already triaged, by design); `scan_cap = limit*4` under-return under pathological long-edit-chain result sets (already triaged; pagination is 5.4's concern); no `origin_ts` index so the `<3`-char LIKE fallback does a full scan + filesort (AD-12-sanctioned degraded fast path; the NFR-2 gate passes at 120k); malformed/unrecognized `content_json` extracts an empty body and is not indexed (non-realistic — bodies are serialized by us from already-parsed events; empty body for non-text is by design); the perf test duplicates `insert_event`'s column list for bulk build (intentional standalone bulk-insert measuring query, not ingest, latency); reader `.exists()`→open TOCTOU could surface `no such table` during first-sync (already triaged as the self-healing reader-opens-pre-migration race).

## Design Notes

**External content over a `body` column (not `content_json`).** FTS5 external content reads its indexed columns by name from the content table, so it needs a real `body` column — the searchable text is otherwise buried inside `content_json` JSON where `'rebuild'` and snippet reads can't reach it. A real, ingest-populated `body` column (a) makes external content and `'rebuild'` work verbatim, (b) reuses the exact Rust `display_body_from_content` fidelity (including the edit `m.new_content.body` fallback) rather than approximating with `json_extract`, and (c) keeps redaction mark-only (body is immutable per version row; redaction only sets `redacted_ts`, gated at query time). Empty string (not NULL) marks a genuinely text-less row so the one-time backfill (`body IS NULL`) stays idempotent.

**Incremental correctness through the writer.** `INSERT OR IGNORE` may no-op on a re-synced duplicate; index only when rows-affected == 1, keyed by `conn.last_insert_rowid()` on that same writer connection — this preserves the single-writer invariant and prevents double-indexing.

**Search dispatch + trigram.** trigram needs ≥3 characters, which is exactly why the AC carves out a `LIKE` fallback under 3. Trigram is case-insensitive and language-agnostic (works on any 3-scalar window ⇒ CJK by construction). MATCH input must be quoted/parameterized so user text like `AND`/`*` is matched literally.

**Deep-link `event_id` is the sanctioned exception.** Story 5.2 kept `event_id` off IPC because the live timeline supplies an opaque `item_key`; search has no such handle for arbitrary archived events and the epic's Story 5.3 AC explicitly requires returning `(account_id, room_id, event_id)` for deep-linking (FR-34). `event_id` is not secret; this is epic-authorized, scoped to search results, and reconciled here so it is not mistaken for a contradiction.

**Network filter lives above the engine.** A Network is a live per-room bridge label (`RoomVm.network`, `bridge::room_bridge_network`), deduped by name across accounts — not an archive column. The tauri-free, archive-only engine cannot resolve it, so Story 5.4 maps a Network selection to its `room_id` set and the engine filters by `room_ids`. This keeps the engine pure and offline-capable.

## Verification

**Commands:**
- `bun run check:rust` -- expected: `cargo fmt --check` clean + `clippy --all-targets -- -D warnings` (no `.unwrap()`, no warnings).
- `bun run test:rust` -- expected: cargo-nextest green incl. `archive_search` (every I/O matrix row) and `archive_search_perf` (p95 < 200 ms at 100k+); ts-rs regenerates `SearchFilterVm.ts`/`SearchHitVm.ts`.
- `bun run check` -- expected: biome + tsc + vitest green (binding + generated types typecheck).
- `bun run check:all` -- expected: full gate green (frontend + rust + build); `bindings:check` passes once the new generated types are committed.
</content>
</invoke>

## Auto Run Result

Status: done

**Summary:** Delivered Story 5.3 — the offline full-text search engine over the Local Archive (FR-34, NFR-2, AD-12). `keeper-core::archive` gained a nullable indexed `body` column on `events` (populated at ingest via the shared `display_body_from_content` extractor, idempotently backfilled for pre-5.3 rows in a single transaction) and a new external-content FTS5 virtual table `events_fts(body, content='events', content_rowid='rowid', tokenize='trigram')` — case-insensitive and CJK-capable by construction — created and one-time-`'rebuild'`-populated atomically. Indexing rides the existing single serialized writer: a row is indexed only when the base `INSERT OR IGNORE` actually added it (never double-indexing a re-synced duplicate) and only when its body is non-empty. A new tauri-free `fts::search` engine dispatches trigram `MATCH` for queries ≥3 Unicode scalars and a metacharacter-escaped `LIKE` scan below that, applies account/room/sender/date-range filters, gates redacted messages at chain-root granularity when "Honor remote deletions locally" is on, deduplicates edit versions to one hit per logical message keyed by `(account_id, chain root)`, and returns `(account_id, room_id, event_id)` deep-link identifiers. Search runs on a fresh read-only WAL connection (works offline / after sign-out) and returns empty (not an error) when no archive exists yet. Exposed as the `search_archive` IPC command with `SearchFilterVm`/`SearchHitVm` typed bindings. No search UI — that is Story 5.4.

**Files changed:**
- `src-tauri/crates/keeper-core/src/archive/fts.rs` (new) — `ensure_fts` (atomic create + one-time rebuild), `index_body`, the `search` engine (trigram/LIKE dispatch, filters, root-aware honor gate, account-scoped chain-root dedup, ordering, clamped limit), `SearchFilter` + `From<SearchFilterVm>`, `escape_like`, unit tests.
- `src-tauri/crates/keeper-core/src/archive/db.rs` — nullable `body` column + migration; `insert_event` writes `body` and returns `Some(rowid)` only on a real insert; transactional `backfill_missing_bodies`; `open_readonly_archive_db`; `open_archive_db` calls backfill + `ensure_fts`.
- `src-tauri/crates/keeper-core/src/archive/{mod.rs,ingest.rs}` — `ArchiveEvent.body`; relocated shared `display_body_from_content`; writer indexes after a real insert.
- `src-tauri/crates/keeper-core/src/account.rs` — `build_archive_event` computes `body` via the shared extractor (private duplicate removed).
- `src-tauri/crates/keeper-core/src/vm.rs` — `SearchFilterVm` (input) + `SearchHitVm` (output), ts-rs exported.
- `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` — `search_archive` command (empty-archive short-circuit, read-only connection, honor-setting read) registered.
- `src/lib/ipc/client.ts` + `src/lib/ipc/gen/{SearchFilterVm,SearchHitVm}.ts` — `searchArchive` binding + generated types.
- `src-tauri/crates/keeper-core/tests/archive_search.rs` (new) + `archive_search_perf.rs` (new) — full I/O-matrix coverage + the 120k-event p95<200 ms CI gate; `tests/archive_{ingestion,durability}.rs` fixtures gain `body`.

**Review findings:** 2 reviewers (adversarial Blind Hunter + Edge Case Hunter). Triage: 0 intent_gap, 0 bad_spec, 6 patch (medium 5, low 1), 2 defer, 26 reject. Patches: (1) honor-deletions leaked an edited-then-redacted message via a surviving edit sibling — replaced the per-row gate with an account-scoped root-redaction `NOT EXISTS`; (2) dedup collapsed the same event id across two accounts — keyed dedup on `(account_id, root)`; (3) search on a not-yet-created `archive.db` errored — returns empty; (4) `ensure_fts` create+rebuild made atomic (crash can't leave an empty un-rebuilt index); (5) body backfill wrapped in one transaction (no multi-minute startup hang on large legacy archives); (6) LIKE metacharacters escaped in the short-query fallback. Three regression tests added. Deferred: external-content FTS has no delete/update maintenance path (Story 5.7 archive-deletion must add it); `events` rowid is not VACUUM-stable (latent — nothing VACUUMs today). Rejects were by-design/out-of-scope/negligible (see Review Triage Log).

**Verification:** `bun run check:rust` → `cargo fmt --check` + `clippy --all-targets -D warnings` clean. `bun run test:rust` → cargo-nextest 384/384 pass (was 381; +3 regressions), including the full `archive_search` I/O-matrix suite and the `archive_search_perf` p95<200 ms gate at 120k events (~5.6 s). `bun run check` → biome + tsc + vitest 552/552 green + core-tauri-free invariant holds (patches touched only Rust; `vm.rs`/TS unchanged, so bindings did not drift). `bun run check:all` → full gate green after commit (the pre-commit `bindings:check` now passes since the generated `SearchFilterVm.ts`/`SearchHitVm.ts` are committed).

**Residual risks:** (1) The external-content FTS index is insert-only in 5.3 (redaction is retrieval-gated, not deleted); Story 5.7's archive-deletion path must maintain `events_fts` or it will drift (deferred). (2) `events_fts` keys on the implicit `rowid`, which is not stable across `VACUUM`; safe today (nothing VACUUMs, `auto_vacuum=NONE`) but a future maintenance/compaction feature must account for it (deferred). (3) The `<3`-char `LIKE` fallback is ASCII-case-insensitive only (AD-12-sanctioned degraded fast path); full Unicode case-folding applies on the trigram path.

---

### Follow-up review pass (2026-07-05)

An independent follow-up review (Blind Hunter + Edge Case Hunter) ran against the committed Story 5.3 diff. It found and fixed one **medium** correctness/privacy defect and three **low** hardening items; 8 findings were rejected as already-triaged or design-sanctioned (see the Review Triage Log).

- **Honor-deletions redacted-edit leak (medium, fixed):** the root-scoped honor gate let a redacted *edit* version of a *surviving* original become the (newest, first-seen) representative hit — returning its redacted body with `redacted = true` while honor-deletions was ON, contradicting the `SearchHitVm.redacted` contract. Fixed by re-adding a per-row `events.redacted_ts IS NULL` clause alongside the existing root-redaction `NOT EXISTS` gate, so a redacted version can never represent a message while the whole message is still withheld when its root is redacted. New regression test `honor_deletions_hides_redacted_edit_of_surviving_root`.
- **Perf gate (low, fixed):** the NFR-2 gate now measures both honor-OFF and honor-ON query shapes at 120k events (the honor-ON path adds the per-row root-redaction subquery); p95 stays < 200 ms.
- **LIKE escaping tests (low, fixed):** `short_query_wildcard_is_literal` now also asserts `_` and `\` are matched literally.
- **Doc comment (low, fixed):** `open_archive_db`'s schema doc comment updated to include `relates_to_event_id`/`rel_type`/`redacted_ts`/`body`.

**Verification (follow-up):** `bun run check:rust` → `cargo fmt --check` + `clippy --all-targets -D warnings` clean. `bun run test:rust` → cargo-nextest 385/385 pass (+1 regression test), including the honor-ON-measured `archive_search_perf` p95<200 ms gate at 120k events (~11.3 s). `bun run check` → biome + tsc + vitest 552/552 green + core-tauri-free invariant holds (patches were Rust-only; `vm.rs`/TS unchanged, so bindings did not drift). `followup_review_recommended` set to `false`: the fixes are localized and directly covered by the new/extended tests, so convergence is reached.
