---
status: review
baseline_revision: a925c4b
final_revision: ''
---

# Story 76.1 — A search index beside the model

<intent-contract>

## Problem

The list searches only model metadata and a short snippet. The model must remain the files, but ranking a whole body needs a disposable derived index (AD-261).

## Approach

Heading-first, fence-aware paragraph chunks retain exact body byte ranges. A content-owning FTS5 table stores keeper-folded title/breadcrumb and text. One transaction replaces a note; unchanged hashes produce no writes. The reconciler owns the writer, fresh read-only connections own queries.

## Always

Use the existing `search::fold_str` and `search::find`; explicit integer chunk identities; foreign-key cascades; quoted AND terms with empty-AND OR fallback and a last-term prefix outside quotes. Keep head metadata searchable, excluding reserved fields.

## Block If

SQLite or filesystem operations fail: return a typed error rather than claiming an empty answer. Schema/vault mismatch discards only the derived database.

## Never

No second model, overlapping chunks, custom folding, FTS-generated display marks, new tokenizer, model weights, or changes to search-everywhere.

## I/O and edge-case matrix

| Input | Observable output / test |
| --- | --- |
| `notatkę`, `Łódź`; queries `notatke`, `Notatk`, `lodz`, `łódź` | folded body retrieval: `polish_fold_and_prefix` |
| separate chunks with separate query words | AND empty → OR results: `and_empty_uses_or` |
| first partial term, final partial term | only final term prefixes: `prefix_only_last_term` |
| punctuation-only term, embedded quote | no invalid MATCH / literal escaping: `punctuation_and_quotes` |
| 30-char note | one chunk: `short_note` |
| fence with blank lines and heading-looking code | one paragraph: `fence_keeps_blank_lines` |
| heading-only note | searchable exact text: `heading_only` |
| 10 000-char paragraph | bounded UTF-8-safe pieces: `oversized_paragraph` |
| Unicode body with headings/paragraphs | exact nonoverlapping slicing: `byte_ranges_are_exact` |
| tag/frontmatter absent from body | ordinal zero retrieval: `head_metadata` |
| same document twice | false, stable hash/rows/vectors: `replace_is_noop` |
| mismatched schema or vault | empty recreated database: `identity_mismatch_recreates` |
| delete and retain subset | no orphan FTS/vectors: `remove_and_retain` |
| overlapping query words | merged raw byte marks: `marks_merge` |
| `ł`, emoji, interior byte endpoints | rounded UTF-16 positions: `utf16_offsets` |
| match beyond beginning, budgeted excerpt | anchored text and re-based marks: `excerpt_rebases` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notes/search.rs:37` — shared matcher; `:262` folding.
- `src-tauri/crates/keeper-core/src/notes/index.rs:71` — reserved metadata prefix.
- `src-tauri/crates/keeper-core/src/archive/recordings_fts.rs:314` — content-owning FTS setup.
- `src-tauri/crates/keeper-core/src/archive/fts.rs:386` — unique real-file test database.
- `src-tauri/crates/keeper-core/src/notes/chunk.rs:1` — new byte-exact chunker.
- `src-tauri/crates/keeper-core/src/notes/search_index.rs:1` — new SQLite search store.

## Tasks & Acceptance

Acceptance, verbatim from the epic: a note whose body says `notatkę` is found by the query `notatke`, by `Notatk`, and by `łódź` when it says `Łódź` — and by nothing when the AND of two words has no chunk but the OR does, the OR results come back (mutation-proved: the tests fail when the fold is dropped from either side, when the prefix `*` is quoted, and when the OR fallback is removed); a fenced block containing a blank line is one chunk; a note of 30 characters is one chunk; `byte_start`/`byte_end` of every chunk slice the body back to its own text; the head chunk finds a note by a tag or a frontmatter value the body does not contain; `replace_note` twice with the same body leaves the row count unchanged and `text_hash` intact; opening a file whose `meta.schema` or `vault_id` differs deletes it and starts empty (all keeper-core, `cargo test -p keeper-core`, Linux); the reconciler wiring, the rebuild path and the cold-build timing are **by inspection, awaiting CI macOS** — NFR-63 and NFR-70 are measured on hesperia in the spec, with the number written down; `check:core-tauri-free`, `check:core-sync-free` and cargo-deny unchanged (NFR-68).

## Design Notes

AD-261…263; research §3.1, §3.2 and §4. Hard limits require scalar-boundary splitting when a single line exceeds 4 000 characters; otherwise prefer line boundaries. Small trailing chunks merge backward where the hard maximum permits. Hash embedding context as well as raw text so title/heading edits invalidate stale vectors. Content-owning FTS deletion is transactional with chunk deletion.

Main explicitly approved the hard-cap priority: a short tail stays separate when backward merging would exceed `CHUNK_MAX_CHARS`. Main also approved a deferred read transaction in `open_read_only`, so ranking and subsequent chunk reads observe one snapshot until the per-query connection is dropped.

- Review fix: short chunks merge only within the same heading; whitespace-only chunks are omitted.
- Review fix: corrupt, non-database, missing-meta, schema-mismatched and wrong-vault stores are discarded with one warning and recreated, including stale WAL/SHM sidecars.
- Review fix: lexical ranking selects the best chunk per note with a materialized BM25 calculation and a window rank; `LEXICAL_POOL` now caps at 1,000 notes rather than chunks.
- Review fix: pending embeddings carry their text hash; writes skip stale/deleted chunks and invalid vectors without poisoning other rows, returning the stored-row count.
- Review fix: schema 2 persists an optional opaque file stat key and exposes `note_stats`; identical content still refreshes a changed stat so cold scans can avoid body reads.
- Review fix: highlighting uses span-only matching without unused snippets; excerpt windows avoid allocating every character boundary, and collapse moves the winning row rather than cloning it.
- Shell review fix (by inspection, awaits macOS): cold fill skips unchanged body reads using persisted `size:mtime_ns:ino:path` stat keys (path catches inode-preserving renames); removals share one live-id set; rebuild derives sidecars and zeros counts; lexical refusal is independent of embedding refusal and clears after successful sync.

## Verification

Review-fix mutation proof (2026-09-19): removing heading isolation produced `["A"]` instead of `["A","B"]`; omitting whitespace rejection emitted a chunk; disabling recovery returned `file is not a database`; removing hash validation stored 2 rows instead of 1. The per-note ranking fixture was strengthened after an initial survivor (the second note's short body outranked duplicate sections); reverting ranking then returned only `{"many"}` instead of `{"many","other"}`. Every mutant was restored before the final gate.

Final review-fix gate: `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib -- notes:: bots:: registry config::keys` passed **959 tests, 1 ignored, 1,794 filtered** after restoring every mutant. Separate baselines passed 689 notes tests and 270 bots/registry/config tests; the focused search-index suite passed 28. This is keeper-core proof only; shell/macOS and project-wide gates remain coordinator-owned. The older build-wave evidence below is retained as history, not the current verification state.

Coordinator gate owed: `cargo nextest run -p keeper-core notes::chunk notes::search_index` from `src-tauri`. No cargo/bun ran in this lane. An isolated `rustc --test` harness imported the actual chunk, search-index and shared matcher sources, linked the existing rusqlite/blake3/tracing/thiserror artifacts, and ran **39 tests: 28 new, 11 existing; all passed**. The harness supplied only the existing `line_bounds` body and reserved-field constant as module context. This is real SQLite-file proof, not a workspace compile.

Eight independent mutants were compiled and tested on isolated source copies, never in the shared worktree. Dropping index folding failed `polish_fold_and_prefix` (`lodz`: 0 rather than 1 hit); dropping query folding failed the same test (`łódź`: 0 rather than 1); placing `*` inside quotes failed `Notatk`; removing OR fallback failed `and_empty_uses_or` (empty set instead of `{a,b}`); byte offsets failed `utf16_offsets` (`[[6,9],[1,5],[0,2]]` instead of `[[3,6],[0,3],[0,1]]`). The three fusion mutants are recorded in spec 76-6. The final unmutated suite again passed 39/39.

A separate non-test executable exercised create → replace/no-op → vector backfill → read-only lexical/cosine query → fuse → raw chunk → excerpt/marks/UTF-16 → delete/cascade. Output: `1 result, why=both, snippet="# Poland\nA notatkę from Łódź 🙂", UTF16=[[24, 28]]`. Temporary harnesses were removed after proof. `SearchIndex::chunk_text` is the sole API addition to the frozen contract, approved by Main's direct message. Optional `chunk_meta` was not added.

NFR-63/70, title-weight tuning against the owner's vault, cargo/clippy/formatting and the shell/macOS gates remain coordinator work; none is claimed by the isolated proof.

Additional concurrency row: writer replaces a note between ranking and snippet retrieval → `reader_keeps_ranked_chunk_snapshot` retains the old matching text while a fresh reader sees the replacement. Final isolated suite: **40/40 passed (29 new + 11 existing)**. Removing `BEGIN DEFERRED` on an isolated copy fails with `Some("replacement")` instead of `Some("old needle")`; this ninth mutant was caught. No production mutation occurred.

### Shell wiring (by inspection)

By inspection, awaits CI macOS; none of the shell changes was compiled here.
The authoritative gate is `.github/workflows/ci.yml:28-52`, **Rust (fmt, clippy, test)** on `macos-latest`; the iOS compile-check job additionally gates the phone build.

- `src-tauri/crates/keeper/src/notes_vault.rs:154` (`Vault::keeper_dir`), `:462` (`subscribe_search`), `:466` (`search_state`), `:535` (`rebuild`), `:714` (`spawn_reconciler`), `:751` (`reconcile`), `:913` (`publish_search_stats`), `:926` (`sync_search`), `:1260` (`apply_batch`): the reconciler owns the only writer, behind a task-owned mutex locked only inside `spawn_blocking`. No connection guard spans an await.
- Cold model publication remains first. The search pass then reads bounded files one at a time, avoiding retention of every vault body in memory. Incremental upserts reuse the exact text read by the existing stat-gated path; unchanged stats produce no search calls. An id change removes the old id before replacing the new one.
- Rebuild is queued to the sole writer, which drops its connection before deleting only `index.json`, `search.db`, `search.db-wal`, and `search.db-shm`; settings and trash are untouched.
- Search failures log ids, not note text or provider credentials, and do not prevent model publication. The independent watch stream reports an explicit unavailable sentence.

Shell matrix awaiting macOS execution: absent DB → cold catch-up after model publish; unchanged touch → no search read/write; changed body/id/removal → corresponding transaction; rescan → retain only current ids; rebuild → both disposable caches recreated; blocked database → model still publishes. Core index tests and their mutations belong to the core lane, not evidence that these shell paths ran. NFR-63 and NFR-70 measurements on hesperia remain owed.

The colocated shell regression `search_keeps_a_renamed_id_when_the_old_path_removal_arrives_last` (`notes_vault.rs:3517`) exercises cold indexing, replacement at the new path followed by removal of the old path with the same id, and true deletion. It must fail if the live-id removal guard in `sync_search` is deleted. Neither this test nor that mutation was run here: both await the macOS job. This specifically protects rename ordering, not merely method forwarding.
