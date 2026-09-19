---
status: review
baseline_revision: a925c4b
final_revision: ''
---

# Story 76.6 — Meaning from the model you already have

<intent-contract>

## Problem

Words alone cannot retrieve a translation or related concept. Semantic search must not replace lexical evidence or introduce a new provider or downloaded weights.

## Approach

AD-264/265: store normalized f32 LE vectors beside chunks, scan a bounded top-k, normalize candidate pools independently, combine 0.45 lexical + 0.55 vector + 0.15 overlap, keep lexical candidates and admit meaning-only candidates at 0.18 or above. Collapse to best chunk per note.

## Always

Model-scoped vectors, transactional batch writes, cascade deletion, lexical fallback without vectors, deterministic score/note-id ordering. Preserve vectors only for unchanged embedding text.

## Block If

Invalid, zero-length, nonfinite or zero-norm vectors cannot be stored or searched. SQLite failures remain errors, not empty lists.

## Never

No ANN dependency, weights download, fabricated lexical marks on meaning-only hits, or low-confidence padding.

## I/O and edge-case matrix

| Input | Output / test |
| --- | --- |
| nonunit vectors and query | cosine of normalized values: `vectors_normalize` |
| model clear/change | zero stored vectors and pending chunks again: `vectors_clear` |
| mixed model/dimensions, k=0, top-k ties | bounded model-scoped ranked answer: `vectors_top_k` |
| NaN/zero vector in batch | error, transaction rollback: `vectors_reject_invalid` |
| changed paragraph, unchanged other chunk | only changed context loses vector: `vectors_survive_unchanged_chunks` |
| low lexical candidate | retained anchor: `fusion_anchor` |
| below-floor semantic candidate | omitted: `fusion_floor` |
| asymmetric pool strengths | 0.45/0.55 scores: `fusion_weights` |
| chunk in both pools | +0.15 even at normalized zero: `fusion_overlap` |
| several chunks same note, tied notes | best chunk, stable note id: `fusion_best_chunk` |
| empty vector pool | normalized lexical order/Words: `fusion_lexical_only` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notes/search_index.rs:1` — vector storage, cosine scan and pure fusion.
- `src-tauri/crates/keeper-core/src/notes/chunk.rs:1` — embedding context prefix.
- `src-tauri/crates/keeper-core/src/bots/store.rs:35` — connection policy precedent.

## Tasks & Acceptance

Acceptance, verbatim from the epic: with a chosen model, the query `podatki` lists a note that says `taxes` and never `podatki` **only** if its fused score clears the floor, labelled *matched by meaning* with no marks, below any note that says the word (mutation-proved on the pure fuser: the test fails when the anchor is removed — a lexical hit dropped — when the floor is removed — a below-floor vector-only hit shown — and when the weights are swapped); with the model cleared the same query answers the lexical list and the status line says meaning is off; choosing a different model empties `vectors` and the progress VM restarts from zero; a provider returning 404 to `/v1/embeddings` yields the refusal sentence in Settings and in the status line and a lexical answer, never an empty list; `capability_flags` on an Ollama model whose array says `embedding` reports `Some(true)`, on one that omits it `Some(false)`, on a Hermes model `None` (unit tests); the `Quirks` table's new row is pinned like `context_window_over_v1` (`quirks.rs:255-257`); NFR-65 and NFR-66 measured on hesperia against a loopback Ollama with the owner's vault, the numbers in the spec; `egress_list` before and after shows the same hosts and `docs/egress.md` is unchanged (NFR-67); the backfill, the subscribe command and the hybrid branch are **by inspection, awaiting CI macOS**.

## Design Notes

The amended contract orders tiers first: Both, Words, Meaning; the convex combination orders results within a tier. Every lexical candidate survives. Meaning-only candidates require raw cosine ≥ 0.5 and pool-normalized hybrid score ≥ 0.18; this combines a weak-match gate with model-relative scaling rather than relying on either alone. Degenerate pools normalize to one. Scores are not probabilities. A bounded BinaryHeap retains only k vector candidates while blobs are borrowed from SQLite rows.

- Provider discovery uses `Promise.allSettled`, preserving reachable models and naming each unreachable provider; a configured missing model remains explicit as “provider · model is unavailable”.
- Embedding picker is absent until capabilities hydrate and on the existing `useIsReducedCapabilityPlatform` phone tier (capability-derived, not OS sniffing or the tier-neutral sync flag).
- Shell wiring (by inspection): shared LazyLock HTTP client and last(model including provider,text) query memo avoid repeated setup/embedding; typed retryable failures have a 30-second deadline and terminal errors stay sticky.
- Shell wiring (by inspection): hash-aware PendingChunk writes use stored counts (zero enters cooldown), DB write errors stay sticky, meaning requires full coverage, and saving the same model does not reset progress.
- Core review fix: pending chunks carry text hashes; vector batches skip edited/deleted chunks and non-finite/zero vectors while returning the count actually stored.
- Core review fix: Unauthorized (401/403), Unsupported (404/405/501 or declared No), Transient (408/429/other 5xx), Transport and Malformed errors have distinct refusal sentences; only Transient and Transport are retryable.
- Core review fix: embedding task prefixes recognize model families case-insensitively, including namespaced/tagged E5 and Nomic IDs.
- Core review fix: blank provider or model fields read and persist as None (words only).

## Verification

Review-fix gate (2026-09-19): the combined `notes:: bots:: registry config::keys` keeper-core library filters passed **959 tests, 1 ignored, 1,794 filtered** after restoration. Mutation proof caught removal of tier ordering (`vec,lex,both,anchor` instead of `both,lex,anchor,vec`), removal of the raw-cosine gate (0.49 admitted), lost transient retryability (`is_retryable` false), blank-model filtering (Some instead of None), and stale-hash acceptance (2 stored instead of 1). Old `fusion_weights` score-order assertions and duplicate provider constants were removed rather than repinned. Earlier build-wave evidence below predates these amended contracts; provider network/macOS integration remains outside this core run.

Coordinator gate owed: `cargo nextest run -p keeper-core notes::chunk notes::search_index`. No cargo/bun ran in this lane. The isolated actual-source `rustc --test` proof passed 39/39 tests (28 new chunk/index tests and 11 existing matcher tests), including real-file SQLite normalization, rollback, clear, top-k/model/dimension filtering and vector reuse after a section edit. See spec 76-1 for the harness boundary and end-to-end non-test executable.

Mutation proof executed on isolated copies: removing the anchor by applying the floor to all candidates failed `fusion_anchor` at `lexical anchor`; removing the semantic floor failed `fusion_floor` because `low`/`mid` leaked into results; swapping 0.45/0.55 failed `fusion_weights` (`lex` ranked before expected `vec`). Together with the five lexical/offset mutants, all eight were caught; the final unmutated 39-test run passed. Production sources were never mutated. NFR-65/66 measurements, egress comparison, provider/shell integration and workspace gates remain coordinator/macOS work.

### Provider route and model settings

Code map: `keeper-core/src/bots/http.rs:81,131` owns client/auth policy;
`bots/mod.rs:297` owns endpoint routing; `bots/discover.rs:387` owns capability
tri-state; `bots/quirks.rs:61,198` owns provider differences; `vm.rs:5420` owns
the model wire shape; `registry.rs:1325` stores the selected provider/model.

The route sends only `{model,input:[...]}` to the existing endpoint. Research
§6.2 documents Ollama support; Hermes remains Unknown because its researched
route table says nothing either way. A 404 is HTTP refusal, not an empty success.
`EMBEDDINGS_UNSUPPORTED` supplies a safe sentence; endpoint URLs, tokens and
provider response text are never echoed. Model-family prefixes follow §6.3
(E5 query/passage; nomic search_query/search_document; otherwise none).
The caller chooses query versus passage; the route does not guess from content.
Normalization belongs to vector storage, not the transport.

| Provider input / boundary | Observable output / test |
| --- | --- |
| Model and two inputs | Exact model/string-array request body |
| Response rows out of order | Reordered by integer `index` |
| Missing `data`, bad JSON, duplicate/missing index | Malformed refusal |
| Empty, nonfinite or inconsistent vector dimensions | Malformed refusal |
| Non-2xx, including Hermes 404 | Typed HTTP refusal |
| Support::No | Unsupported before network |
| Unknown Hermes support | Probe permitted, never silently skipped |
| E5 / nomic / other model | Family-specific query/passage prefix |
| 32 / 33 inputs | Accepted / refused before network |
| Model chosen / cleared / corrupt | Round trip / None / None |

Provider verification owed to the coordinator: `bots::embed`, `bots::quirks`,
`bots::discover`, `registry`; parsing and body-building tests need no network.
The thin async route reuses the same pure validation functions. Unit test
mutation target: ordered response test must fail if index ordering is removed;
execution is not yet claimed.

The documentation-generation build type-checked these provider edits without
diagnostics but could not produce the unit-test binary: four `E0597` errors in
the concurrent search-index implementation stopped compilation. Main received
the failure artifact and the owed filters; provider tests and ordering mutation
remain unexecuted, not reported green.

### Settings implementation
`src/components/notes/search-settings.tsx:1` reads configured providers and their discovered models through existing Bots IPC. Select excludes only `embedding === false`; unknown models remain selectable with a warning (AD-151). None clears the stored model. Search-state refusal sentences appear here as well as in the bar. Matrix: true → offered; false → absent; null → offered with warning; None → null write; refused state → visible sentence. These settings never download a model or introduce another provider.

Settings proof: `search-settings.test.tsx` passes capability true/false/null selection behavior, the unknown-capability warning, None's null write, and rendering the provider refusal sentence. Included in the five-file 61-test run recorded in spec 76-2; generated-binding/typecheck and provider integration remain coordinator gates. The Settings section contains no hand-authored icon control; Select's generated chevron retains its house behavior.

### Shell wiring (by inspection)

By inspection, awaits CI macOS; none of this shell was compiled here. Gate: **Rust (fmt, clippy, test)** (`.github/workflows/ci.yml:28-52`, `macos-latest`), plus **iOS (compile check)**. NFR-65/66 timings and a real configured-provider round trip on hesperia are still owed.

- `src-tauri/crates/keeper/src/notes_vault.rs` (`embedding_model_changed`, `reconcile`, `publish_search_stats`, `embedding_endpoint`, `embed_tick`, `cadence_tick`): one bounded batch is dispatched per existing tick; its HTTP work runs outside the reconciler and returns through `Work::Embedded`. The reconciler can serve touched paths while HTTP runs. Generation changes discard stale results after note/model/provider changes. Terminal refusals stop batches until model/provider changes; transient failures retry after 30 seconds. A completed backfill does not re-query `search.db` on every idle tick.
- Endpoint assembly uses the existing configured provider and `resolve_token`; the client is core's redirect-refusing HTTP client, reused through the workspace reqwest dependency. No new host, model download or credential logging is introduced. Task ownership uses a weak work sender so the reconciler does not keep itself alive after the vault is removed.
- `src-tauri/crates/keeper/src/notes_ipc.rs:671` (`query_embedding`) uses the query prefix and a one-second timeout, falling back to lexical on failure or a model change during the await. `project_list` (`:490`) opens its read connection only after this await, then invokes `cosine_top_k`/`fuse` or `lexical_only`.
- `notes_embedding_model_get` (`:1221`), `notes_embedding_model_set` (`:1228`) use the registry; the setter queues vector clearing to the sole reconciler writer. `notes_subscribe_search` (`:5254`) streams an initial snapshot and subsequent watch changes, using the existing subscription cancellation registry. All commands are registered at `lib.rs:1302-1304`.

Matrix awaiting macOS execution: no model → words only; ready model → hybrid; slow query → words within timeout; provider 404 → refusal sentence and no further batches; provider change → retry; model change → cleared vectors/progress; changed note during HTTP → stale vectors discarded; full backfill → no idle search reads; queued touches → served during network wait. Core provider/fusion tests and mutations are recorded separately by their owners and do not establish this shell behavior.
