---
title: 'Export to JSON and Markdown'
type: 'feature'
created: '2026-07-05'
baseline_revision: '447cce00e952db400b7b9e05aedde53bdbee95e9'
final_revision: '158f1285f5bbb50ec217ae776c231f6cd1250b43'
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

**Problem:** Epic 5 persists every synced event to `archive.db` (5.1), keeps edit chains and redaction markers (5.2), and makes it searchable (5.3/5.4) — but there is no way to get history *out*. FR-35/AD-11/UX-DR11 promise a portable, provable export: any Chat / Account / everything, to lossless JSON and readable Markdown, as a background job that never blocks messaging and keeps working after sign-out.

**Approach:** Add a tauri-free `keeper-core::archive::export` module that reads `archive.db` **only** (via the existing read-only connection) and writes, for a chosen scope, a **lossless JSON** array (every archived row in scope; count matches the archive) plus a **chronological Markdown transcript** (sender, timestamp, final edited text resolved through the version chain, redaction stubs, media as relative file links). Drive it from a cancellable background job spawned off a new `export_start` command that streams `ExportProgressVm` (counts + terminal phase + output paths) over a `Channel`, with `export_cancel` setting a shared cancel flag. A new React Export dialog (scope picker → format checkboxes → include-media toggle → destination folder via the dialog plugin) shows a progress toast with Cancel and Reveal in Finder on success, and a **persistent** alert (not toast-only) on failure noting partial-file cleanup. Media bytes are copied **best-effort** via an injected resolver (kept out of `keeper-core`); the transcript/JSON never depend on it, so text export is complete and reproducible from `archive.db` alone.

## Boundaries & Constraints

**Always:**
- Export reads `archive.db` **only** for all transcript/JSON content — `open_readonly_archive_db(data_dir)`; never the SDK store, live session, or network (AD-11). This is what makes a signed-out Account exportable. `data_dir` comes from `state.platform.data_dir()`.
- **Scope** is one of: this Chat (`accountId`+`roomId`), this Account (`accountId`), or everything (all accounts). Rows are selected by scope from `events` and ordered `origin_ts ASC` (deterministic tie-break `inserted_ts ASC, event_id ASC`) for a chronological transcript.
- **JSON is lossless:** it contains every archived row within scope (redacted-but-retained rows and all edit-chain versions included), so the emitted event count equals the scoped archive row count (the provability guarantee). JSON carries full `media_json`/`content_json` verbatim.
- **Markdown is a readable transcript:** one entry per logical message (edit-chain **root**, deduped), in chronological order, each with sender, human timestamp, and the **final edited text** (resolve via `edit_chain` + `display_body_from_content`); a redacted message renders a stub (never the withheld content when `honorDeletions` is on); media renders as a **relative file link** under `media/` derived from `ArchiveMedia` metadata (e.g. `media/<event_id>-<sanitized_filename>`).
- **Background job, never blocks messaging:** the export runs on a blocking-safe task (`spawn_blocking`/dedicated thread — rusqlite is synchronous), streaming `ExportProgressVm` batches (running with counts → terminal `Completed`/`Cancelled`/`Failed`). `export_start` returns an `exportId` immediately; the writer task and messaging are untouched.
- **Cancel + cleanup:** each job has an `Arc<AtomicBool>` cancel flag in an `AppState` registry keyed by `exportId`; the loop checks it between events. On cancel **or** failure, partial output files/dir are deleted before the terminal batch, and the batch/UI states the partial-file cleanup honestly.
- **include-media** governs a **best-effort** byte copy into `<export>/media/`: resolvable bytes are copied (`mediaCopied`), unresolvable ones (uncached / signed-out) are skipped and **counted** (`mediaSkipped`) — never fatal. The Markdown link and JSON metadata are emitted regardless. The resolver is injected from the keeper layer so `keeper-core::archive::export` stays tauri-/session-free; `None` resolver ⇒ every media item counts as skipped.
- Rust: no `.unwrap()`/bare `.expect()` in prod paths; `?` + `thiserror` (`ArchiveError` → `CoreError` → `IpcError`); `tracing` with ids not content; new IPC types derive serde camelCase + `#[ts(export)]` into `src/lib/ipc/gen/`. TS: no `any`, `import type`, `import { open } from "@tauri-apps/plugin-dialog"` for the folder picker, generated types re-exported from `gen/`.

**Block If:**
- Producing a lossless JSON whose event count provably equals the scoped archive count is impossible without re-reading the SDK store or network (it must not be — every needed field is a column on `events`, readable through `open_readonly_archive_db`).

**Never:**
- No touching the SDK store, live `Client`, sync, or homeserver for transcript/JSON content (AD-11) — the only session-adjacent access is the optional, read-only, best-effort media byte copy, which degrades to skip-and-report and never gates export success.
- No changes to the ingestion writer, FTS engine (5.3), `SearchFilterVm`/`SearchHitVm`, timeline VMs, or the `archive.db` schema. No new `event_id`/mxc/key added to any streamed timeline VM.
- No archive-first pagination (5.6) or sign-out/delete-archive (5.7). No command-palette entry point (`⌘K` is Epic 9 — reachable from the conversation surface + search results only; palette entry deferred). No export of drafts/outbox/settings (`keeper.db`) — archive events only.
- No blocking the UI thread or messaging during export; no holding the whole export in memory beyond streaming needs for a 10k-message run.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Start Chat export | scope=chat, JSON+MD, dest chosen | job spawns, returns `exportId`; progress batches stream counts; both files written under a scope-named subfolder | — |
| Lossless JSON | 10k-message Chat | JSON array length == scoped `events` row count for that room; well-formed; parseable | — |
| MD transcript | mixed room (edits, redaction, media) | chronological; each root shows sender+timestamp+final edited text; redacted→stub; media→relative `media/…` link | — |
| Edit chain | original + 2 edits | one MD entry with the latest text; JSON retains all 3 rows (lossless) | — |
| Redaction, honorDeletions on | redacted-but-retained row | MD shows redaction stub, not content; JSON still includes the row (metadata) | — |
| include-media on, cached | media event, byte resolvable | byte copied to `media/…`; `mediaCopied++`; link resolves | resolver error ⇒ skip that item, `mediaSkipped++`, continue |
| include-media on, signed out | account signed out (no cache) | every media item `mediaSkipped++`; links/metadata still emitted; export completes | — |
| Cancel mid-run | `export_cancel(exportId)` | loop stops at next check; partial files deleted; terminal batch `Cancelled` | — |
| Failure (e.g. dest not writable) | dest dir read-only | terminal batch `Failed` with message; partial files cleaned; UI shows persistent alert (not toast) noting cleanup | mapped `IpcError`, `retriable` where applicable |
| Everything scope | scope=everything, multi-account | rows across all accounts, chronological; JSON count == total scoped rows | — |
| No matching rows | empty scope | valid empty JSON `[]` + empty-but-valid MD (header only); `Completed` with 0 counts | — |
| No format selected | JSON=false, MD=false | dialog blocks Start (≥1 format required) | validation in dialog, no call |
| Reveal in Finder | completed, click Reveal | `reveal_item_in_dir(outputPath)` opens Finder at the file | invalid path ⇒ mapped `IpcError`, no panic |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/archive/db.rs` -- ADD scoped export reads: `scoped_event_count(conn, &scope) -> i64` and a chronological root/event reader for a scope (reuse `edit_chain`, `retrievable_content`, `display_body_from_content`, `map_stored_event`; canonical SELECT + `ORDER BY origin_ts, inserted_ts, event_id`). Existing `open_readonly_archive_db` (line 207) is the connection source.
- `src-tauri/crates/keeper-core/src/archive/export/mod.rs` -- NEW orchestrator: `run_export(reader: &Connection, req: &ExportRequestVm, dest_root: &Path, progress: &dyn Fn(ExportProgressVm) -> bool, cancel: &AtomicBool, media: Option<&dyn Fn(&ArchiveMedia, &str) -> Option<Vec<u8>>>) -> Result<ExportOutcome, ArchiveError>`: create scope subfolder; stream roots chronologically; write JSON (lossless rows) + MD (transcript); best-effort media copy; check `cancel`; on cancel/error delete partial dir; emit progress. `honorDeletions` read once in the command via `keeper_core::archive::get_honor_remote_deletions(&data_dir)` (the same accessor `search_archive` uses) and passed into `retrievable_content`.
- `src-tauri/crates/keeper-core/src/archive/export/json.rs` -- NEW: pure lossless JSON serialization of scoped `StoredEvent` rows (all versions, redacted-retained included).
- `src-tauri/crates/keeper-core/src/archive/export/md.rs` -- NEW: pure Markdown transcript renderer (roots→final text, redaction stub, media link, sender/timestamp); unit-testable `render_markdown(events, media_links) -> String`.
- `src-tauri/crates/keeper-core/src/archive/mod.rs` -- `pub mod export;` + expose new db reads.
- `src-tauri/crates/keeper-core/src/vm.rs` -- NEW `ExportRequestVm` (scope kind + optional account/room ids + `{json,markdown}` flags + `includeMedia` + `destinationDir`), `ExportScopeKind` enum (`Chat`/`Account`/`Everything`), `ExportProgressVm` (`exportId`, `phase`, `messagesWritten`, `totalMessages?`, `mediaCopied`, `mediaSkipped`, `outputPaths`, `error?`), `ExportPhase` enum (`Running`/`Completed`/`Cancelled`/`Failed`) — all serde camelCase + `#[ts(export)]`.
- `src-tauri/crates/keeper-core/src/error.rs` -- `ArchiveError` variant(s) for export IO/serialization; ensure it maps through `CoreError`.
- `src-tauri/crates/keeper/src/ipc.rs` (+ `lib.rs`) -- `AppState` gains an export registry (`Mutex<HashMap<u64, Arc<AtomicBool>>>` + `AtomicU64` id counter). Commands: `export_start(state, request: ExportRequestVm, channel: Channel<ExportProgressVm>) -> Result<u64, IpcError>` (spawn blocking job, wrap `channel.send` as the sink, inject best-effort media resolver, register/deregister cancel flag), `export_cancel(state, export_id: u64) -> Result<(), IpcError>` (set flag), `reveal_path(path: String) -> Result<(), IpcError>` (`tauri_plugin_opener::reveal_item_in_dir`). Register all three in `generate_handler!`.
- `src/lib/ipc/client.ts` -- bindings: `startExport(request, onProgress): Promise<number>` (via `subscribe`), `cancelExport(exportId): Promise<void>`, `revealPath(path): Promise<void>`.
- `src/lib/stores/export.ts` -- NEW vanilla-zustand store: dialog open state + scope preset + live job state (`phase`, counts, `outputPaths`, `error`); `open(preset)`, `close()`, `applyProgress(vm)`; `useExportStore` hook (mirrors `search.ts`).
- `src/components/export/export-dialog.tsx` -- NEW surface: scope `RadioGroup` (this Chat / this Account / everything), format `Checkbox`es (JSON, Markdown; ≥1 required), include-media toggle, destination via `open({ directory: true })`, Start → `startExport`; running → `Progress` + counts + Cancel (`cancelExport`); success → Sonner toast with Reveal action (`revealPath`) + in-dialog Reveal; failure → **persistent** `AlertDialog`/inline alert (not toast-only) noting partial-file cleanup, retriable copy honored.
- `src/lib/stores/export.ts` consumers: `src/components/layout/conversation-pane.tsx` (or detail/header) -- add an Export affordance opening the dialog preset to the open Chat; `src/components/search/search-result-list.tsx` (or overlay) -- add an Export affordance opening the dialog preset from the current search scope (account/everything).
- `src/components/layout/app-shell.tsx` -- mount `<ExportDialog />`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/archive/db.rs` -- scoped export reads (`scoped_event_count` + chronological scoped reader) reusing edit-chain/retrievable/display helpers -- the archive-only data source.
- [x] `src-tauri/crates/keeper-core/src/archive/export/{mod,json,md}.rs` (new) + `archive/mod.rs` -- orchestrator (scope folder, stream, cancel, partial-cleanup, best-effort media), pure lossless JSON writer, pure Markdown transcript renderer -- the export engine.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` (+ `error.rs`) -- `ExportRequestVm`/`ExportScopeKind`/`ExportProgressVm`/`ExportPhase` (ts-rs) + `ArchiveError` export variants -- IPC contract + errors.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (+ `lib.rs`) -- `export_start`/`export_cancel`/`reveal_path` commands, `AppState` export registry, injected media resolver, exhaustive `IpcError` mapping, registered -- background job + cancel + reveal.
- [x] `src/lib/ipc/client.ts` -- `startExport`/`cancelExport`/`revealPath` bindings -- typed frontend access.
- [x] `src/lib/stores/export.ts` (new) -- dialog + live-job store (`open`/`close`/`applyProgress`) -- surface + progress plumbing.
- [x] `src/components/export/export-dialog.tsx` (new) + `app-shell.tsx` mount -- scope/format/include-media/destination, Start/Progress/Cancel, success toast + Reveal, persistent failure alert -- the export UI.
- [x] `src/components/layout/conversation-pane.tsx` + `src/components/search/search-result-list.tsx` -- Export entry points (Chat preset + search-scope preset) -- reachability.
- [x] Tests: Rust `archive/export` inline/integration tests over a temp `archive.db` (lossless JSON count==scoped rows; MD chronological + final-edited-text + redaction stub + media relative link; scope filtering chat/account/everything; cancel deletes partial output; `None` media resolver ⇒ all skipped); frontend `export-dialog.test.tsx` (scope/format/include-media → correct `ExportRequestVm`; ≥1-format gating; running→Progress+Cancel; completed→Reveal toast; failed→persistent alert not toast) + `export.ts` store test -- covers the I/O matrix.

**Acceptance Criteria:**
- Given the Export dialog opened from the detail/conversation surface or search results, when the user picks scope (this Chat / this Account / everything), formats (JSON, Markdown), include-media, and a destination, then the export runs as a background job reading `archive.db` only, streams a progress toast with counts and Cancel, and messaging is never blocked (FR-35, AD-11).
- Given a 10k-message Chat export, when it completes, then the JSON is well-formed and complete (emitted event count equals the scoped archive row count), the Markdown is a chronological transcript with sender, timestamp, final edited text, and media as relative file links, and the toast offers Reveal in Finder (FR-35).
- Given an export failure or a user cancel, then partial files are deleted and a **persistent** alert (not toast-only) appears in the Export surface noting the partial-file cleanup (UX-DR11).
- Given a signed-out Account whose archive is retained, when the user exports it, then JSON and Markdown are produced from `archive.db` with no live session (include-media degrades to skip-and-report), preserving the after-sign-out export guarantee; and no `event_id`/mxc is added to any streamed timeline VM and the `archive.db` schema is unchanged.

## Design Notes

**AD-11 boundary — why media is split out.** `archive.db` stores media **metadata** only (`ArchiveMedia`: mxc, mimetype, size, filename, thumbnail_mxc), never bytes; bytes live in the per-account SDK media cache under the SDK dir. AD-11 requires export to read `archive.db` only and never touch the SDK store/live session — that is exactly what lets a signed-out Account export. So the transcript and lossless JSON are derived **entirely from `archive.db`** (the durable, provable, after-sign-out-safe core), while the **include-media** byte copy is an explicitly separate, read-only, best-effort augmentation whose resolver is injected from the keeper layer (`keeper-core::archive::export` never links a `Client`). Unresolvable media (uncached, or signed-out — cache deleted with the SDK dir per 5.7) are skipped and **counted**, never fatal; the Markdown relative link and JSON metadata are emitted regardless. Net: text/JSON export is always complete; media bytes ride along when locally cached. (Residual: full session-free media byte inclusion is out of scope — log a deferred-work item.)

**Final edited text without a schema change.** Reuse Story 5.2's `edit_chain(conn, account_id, event_id)` (root + `m.replace` versions ordered `origin_ts, inserted_ts, event_id`) and `display_body_from_content` to resolve the latest text for each transcript root; `retrievable_content(..., honor_deletions)` gates redacted content so a stub is shown, never withheld text. JSON stays lossless by emitting **all** scoped rows (every version + redacted-retained), so the count equals the archive.

**Cancel that actually interrupts.** rusqlite reads are synchronous, so the job runs on a blocking task and checks an `Arc<AtomicBool>` between events (drop-based cancellation can't interrupt a synchronous loop cleanly, unlike the subscription tasks). The flag lives in an `AppState` registry keyed by `exportId`; `export_cancel` sets it; the job removes itself on any terminal phase. Partial output is written under a single scope subfolder so cleanup on cancel/failure is one `remove_dir_all`.

**Reachability + reveal use existing plumbing.** Folder picking reuses `import { open } from "@tauri-apps/plugin-dialog"` with `{ directory: true }` (as composer.tsx already does for files; `dialog:allow-open` is granted). Reveal in Finder uses `tauri_plugin_opener::reveal_item_in_dir` — `opener:default` already grants `allow-reveal-item-in-dir`, so no capability change. Progress streams over `Channel<ExportProgressVm>` exactly like `room_list_subscribe`.

## Verification

**Commands:**
- `bun run check:rust` -- expected: `cargo fmt --check` + `clippy --all-targets -- -D warnings` clean (no `.unwrap()`, no warnings); `keeper-core` stays tauri-free.
- `bun run test:rust` -- expected: cargo-nextest green incl. new `archive/export` tests (lossless count, MD transcript/edit/redaction/media-link, scope filtering, cancel cleanup, None-resolver skip).
- `bun run check` -- expected: biome + tsc + vitest green incl. `export-dialog` + `export` store tests; new generated `ExportRequestVm`/`ExportProgressVm`/enum bindings typecheck; no `any`.
- `bun run check:all` -- expected: full gate (frontend + rust + `tauri build --no-bundle`) green; `bindings:check` shows only the new export types added and **no timeline VM / schema diff**.

## Spec Change Log

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 6: (high 2, medium 3, low 1)
- defer: 1
- reject: 16
- addressed_findings:
  - `[high]` `[patch]` **Redaction leak on an edited-then-redacted message.** `resolve_final_body` gated the honor-deletions stub on the *root* row's `redacted_ts` only. A remote redaction of the *edit* row (root's `redacted_ts` stays NULL) let the redacted-away latest edit's text leak into the shareable Markdown transcript despite the "honor remote deletions" policy. Now mirrors the frozen FTS engine's semantics exactly (`fts.rs`): root-redacted ⇒ whole-message stub; otherwise the current text is the latest **non-redacted** version. New test `honoring_shows_prior_version_not_a_redacted_latest_edit`.
  - `[high]` `[patch]` **Persistent failure alert defeated by closing the dialog mid-run.** The success toast and the UX-DR11-mandated persistent failure `AlertDialog` lived inside `ExportDialogInner`, which unmounts on close — but the footer offers "Close" while the background job runs. A failure after close surfaced nothing. Hoisted both terminal surfaces into an always-mounted `ExportJobWatcher` (toast fired from an effect, once per job), so they appear regardless of dialog open state. New regression test `still shows the persistent failure alert after the dialog is closed mid-run`.
  - `[medium]` `[patch]` **Provability invariant was a compiled-out `debug_assert`.** The "emitted JSON count == scoped archive count" guarantee was a `debug_assert_eq!` (no-op in release) over two separate reads that a concurrent ingest could diverge. Now both reads run in one read transaction (consistent WAL snapshot) and a real runtime check returns `Failed` on any mismatch.
  - `[medium]` `[patch]` **Cleanup could delete a pre-existing user folder.** The cancel/failure `remove_dir_all(scope_dir)` deleted the whole scope folder even if it (or a prior export's output / an unrelated same-named dir) pre-existed. Now records whether the folder pre-existed and only cleans up output it created. New test `cancel_preserves_a_preexisting_scope_folder`.
  - `[medium]` `[patch]` **Markdown transcript structure-forging from untrusted bodies.** Message bodies (remote-authored) were written raw, so a body could inject headings or forge a `**sender** — timestamp` entry in a shareable artifact. Bodies are now emitted as blockquotes (each line `> `-prefixed), containing block-level injection while staying readable. New test `body_is_blockquoted_so_it_cannot_forge_entry_structure`.
  - `[low]` `[patch]` **Media links dangled with no in-document disclosure.** With the resolver `None` (deferred), every media message emitted a `media/…` link to a non-existent file with no note. The transcript header now carries an honest note when media is referenced but not included. Strengthened `none_resolver_skips_all_media_but_emits_link`.
  - Deferred (1): the session-free media-byte resolver itself (injected `None` today) plus its copy-path hardening — filename-collision de-dup, `.`/`..` token rejection, per-file write-failure = skip-not-fail — bundled into one deferred-work entry (the copy path is dead until a resolver is wired; latent behind `None`).
  - Rejected as by-design / unreachable / negligible (16): orphan edits absent from the transcript (still lossless in JSON; narrow — original outside the archive window); `edit_chain` per-root N+1 (indexed seek; acceptable for a cancellable background job); Completed-vs-Running `total_messages` "divergence" (they agree at completion — `messages_written == roots.len()`); `reveal_path` accepting any path (reveal is benign and the input is app-controlled `outputPaths`); unbounded `destination_dir` (a native folder picker chooses it; the sharp edge — `remove_dir_all` — is patched); late `export_cancel` after completion (idempotent no-op; the Completed batch is honest); non-UTF-8 output path via `to_string_lossy` (macOS paths are UTF-8; reveal failure is caught); in-memory empty-archive schema drift / `db_path` TOCTOU (isolated never-synced path, low probability, benign); registered flag orphaned on a job-body panic (no panic sources — all `Result`s handled); `format_timestamp` on absurd/negative ts (cosmetic; `origin_ts` is a real server ts); `to_ipc_error` marking `ExportIo` retriable (that arm is effectively unreachable — setup errors are `Sqlite`; scope errors funnel to a `Failed` batch); frontend progress-ordering race before `startJob` (bounded by microtask ordering — the `await` continuation runs before macrotask channel messages); `parse_media` treating malformed `media_json` as no-media (our own ingest writes valid `ArchiveMedia`; degrades gracefully); registry `Mutex` poison (handled via `if let Ok(..)`, never `unwrap`); destination writability not pre-validated (surfaces honestly on the `Failed` batch); scope/preset id desync (chat/account radios are disabled when the preset lacks their ids).

### 2026-07-05 — Review pass (follow-up)

- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 0
- reject: 24
- addressed_findings:
  - `[medium]` `[patch]` **Partial output left behind on cancel/failure into a pre-existing folder.** The prior pass's fix for "cleanup could delete a pre-existing user folder" skipped cleanup *entirely* when the scope folder pre-existed (`run_export`, `!pre_existed` guard). But re-exporting the same scope to the same destination (folder already there from a prior run) then cancelling/failing left a partial `export.json`/`transcript.md`/`media/` behind — violating the "on cancel/failure partial files are deleted, honestly" contract and silently corrupting a previously-good export. Cleanup now removes only the three artifacts *this* export writes (`export.json`, `transcript.md`, `media/`) when the folder pre-existed, and still `remove_dir_all`s the whole folder only when it created it. New test `cancel_midrun_removes_partial_output_but_spares_preexisting_files` (writes JSON, cancels mid-run via the progress heartbeat, asserts the user sentinel survives while the partial JSON is cleaned).
  - `[low]` `[patch]` **Chat scope folder slug omitted the account id.** `scope_slug` keyed the Chat subfolder on `room_id` only (`chat-<room>`), while the Account scope keys on `account_id`. The same Matrix room synced under two of your accounts is stored as separate rows, so exporting both "this Chat" views into the same destination collided on one folder and silently overwrote one provable export with the other. Chat now keys on both (`chat-<account>-<room>`), consistent with the Account scope; affected test path literals updated.
  - Rejected (24): media copy-path hardening — filename-collision de-dup, `.`/`..` token rejection, per-file skip-not-fail, truncation collisions, byte-content tests (all latent behind the injected `None` resolver — dead code already captured by the standing deferred-work entry, not re-deferred); per-root `edit_chain` read outside the snapshot transaction (JSON provability count holds; MD is the readable, not the provable, artifact and snapshot/MD cross-consistency under concurrent same-room ingest at the export instant is not a promised invariant); `edit_chain` `m.replace`/`redacted_ts` ingest precondition (the established 5.2/5.3 model the FTS engine also relies on); empty-DB masks corrupt archive / `db_path` TOCTOU (app-controlled `data_dir`, empty→empty is the spec's "No matching rows" row); `total_messages` (roots) vs JSON row count (by-design: transcript=logical messages, JSON=lossless rows); `reveal_path` arbitrary path (benign reveal, app-controlled `outputPaths`); concurrent same-scope+same-dest exports clobber (deliberate rare action, no invariant beyond it); `format_timestamp` negative/huge ts (cosmetic; real server ts); empty-`{}` `media_json` dangling link (our ingest writes valid `ArchiveMedia`); success toast on empty export (by-design "No matching rows"); `applyProgress` terminal-not-sticky / out-of-order revert (Tauri `Channel` is FIFO; backend emits exactly one terminal batch then stops); `unchecked_transaction` "heavier than its guarantee" (provides exactly the count-consistency needed); no ≥1-format enforcement at the core boundary (spec places that validation in the dialog; empty-output core call is benign); `ExportError` no `Display`/`Error` impl (compiles, clippy-clean); stale re-Start note / `toastedId` not reset on `open()` (transient cosmetic; ids unique); `next_id` u64 wrap (unreachable); panic in the blocking job → stuck running (no panic sources, all `Result`s handled); partial-media note only when `copied==0` (unreachable today — `None` resolver ⇒ `copied` always 0); honor-deletions all-edits-redacted shows the unredacted root (correct: the root is retained, matches FTS semantics); empty-scope no `Running` heartbeat (cosmetic, indeterminate bar); chat `onStart` with null `roomId` (chat radio disabled when the preset lacks ids).

## Auto Run Result

Status: done

**Summary:** Delivered Story 5.5 — archive export to JSON and Markdown (FR-35, AD-11, UX-DR11). A new tauri-free `keeper-core::archive::export` module (`mod`/`json`/`md`) reads `archive.db` **only** (via `open_readonly_archive_db`, both scoped reads in one consistent read-transaction snapshot) and, for a chosen scope (this Chat / this Account / everything), writes a **lossless JSON** array (every archived row in scope — the emitted count is runtime-checked to equal the scoped archive count, the provability guarantee) plus a **chronological Markdown transcript** (one entry per logical message: sender, human UTC timestamp, final edited text resolved through the version chain, a redaction stub when honoring deletions, and media as relative `media/…` links). It runs as a cancellable background job: `export_start` registers an `Arc<AtomicBool>` cancel flag in an `AppState` registry, returns an `exportId` immediately, and spawns a `spawn_blocking` job that streams `ExportProgressVm` batches (Running heartbeats → one terminal Completed/Cancelled/Failed) over a `Channel`; `export_cancel` sets the flag (checked between events); on cancel/failure the scope folder is cleaned up — but only when the export created it, never a pre-existing user folder. A React Export dialog (scope radio, JSON/Markdown checkboxes with ≥1 required, include-media toggle, destination via the dialog plugin's directory picker) shows a live Progress bar + counts + Cancel; an always-mounted `ExportJobWatcher` fires the success toast (with Reveal in Finder via `reveal_item_in_dir`) and the persistent failure `AlertDialog` **independent of dialog open state**. Reachable from the conversation header and search results. Media byte inclusion is a best-effort, injected resolver (currently `None` — deferred), so the module never links a `Client`; text/JSON export is complete and works after sign-out.

**Files changed:**
- `src-tauri/crates/keeper-core/src/archive/export/{mod,json,md}.rs` (new) — orchestrator (scoped snapshot reads, chronological roots, cancel, single-folder cleanup that spares pre-existing dirs, best-effort injected media copy, runtime provability check), lossless JSON writer, pure Markdown transcript renderer (blockquoted bodies, honest media-omitted note, dependency-free UTC timestamps).
- `src-tauri/crates/keeper-core/src/archive/db.rs` — `ExportScope`, `scoped_event_count`, `scoped_events_chronological`, `open_empty_in_memory_archive_db` (never-synced empty export).
- `src-tauri/crates/keeper-core/src/archive/mod.rs` — `pub mod export;` + exports.
- `src-tauri/crates/keeper-core/src/{vm,error}.rs` — `ExportScopeKind`/`ExportRequestVm`/`ExportPhase`/`ExportProgressVm` (ts-rs) + `ArchiveError::ExportIo`.
- `src-tauri/crates/keeper/src/{ipc,lib}.rs` — `export_start`/`export_cancel`/`reveal_path` commands, `AppState` export registry, `None` media resolver, `IpcError` mapping, registered.
- `src/lib/ipc/client.ts` — `startExport`/`cancelExport`/`revealPath` bindings + 4 gen re-exports.
- `src/lib/stores/export.ts` (new) — dialog + live-job store.
- `src/components/export/export-dialog.tsx` (new) — dialog + always-mounted `ExportJobWatcher`; `src/components/layout/app-shell.tsx` mount.
- `src/components/layout/conversation-pane.tsx` + `src/components/search/search-overlay.tsx` — Export entry points.
- Tests: `archive/export` Rust tests (11) + `export-dialog.test.tsx` (9) + `export.test.ts` (8).

**Review findings:** 2 reviewers (adversarial Blind Hunter + Edge Case Hunter). Triage: 0 intent_gap, 0 bad_spec, 6 patch (high 2, medium 3, low 1), 1 defer, 16 reject. Patches: (1) redaction leak on an edited-then-redacted message — now mirrors FTS honor semantics (root-redacted ⇒ stub; else latest non-redacted version); (2) persistent failure alert/toast defeated by closing the dialog mid-run — hoisted to an always-mounted watcher; (3) provability invariant was a compiled-out `debug_assert` — now a consistent-snapshot read + real runtime check; (4) cleanup `remove_dir_all` could delete a pre-existing user folder — now spares folders it didn't create; (5) untrusted Markdown body could forge transcript structure — bodies blockquoted; (6) dangling media links — honest header note. Deferred: the session-free media-byte resolver + its copy-path hardening. See the Review Triage Log for the 16 rejects.

**Verification:** `bun run check:rust` → `cargo fmt --check` + `clippy --all-targets -D warnings` clean. `bun run test:rust` → cargo-nextest 408/408 (+4 export tests over the pre-review run). `bun run check` → biome + tsc + vitest 591/591 (+17 export tests) + `keeper-core` tauri-free invariant holds. `bun run tauri build --no-bundle` → release binary built (exit 0). `bindings:check` passes on content (only the 4 new export types added; **no timeline VM / `archive.db` schema diff**); its git-cleanliness assertion is satisfied once the story's files are committed (this run commits them).

**Residual risks:** (1) include-media copies nothing until the deferred session-free media resolver lands — every media item is skipped-and-counted with an honest in-transcript note; the copy path + counters exist and are unit-tested. (2) An "orphan edit" (an `m.replace` whose original is outside the archive window) is retained in the lossless JSON but not rendered as a transcript entry (rejected: narrow, and JSON stays complete). (3) `edit_chain` is queried once per transcript root (indexed seek) — acceptable for a cancellable background job with progress; a batched load would help only at extreme scale.

---

**Follow-up review pass (2026-07-05).** An independent Blind Hunter + Edge Case Hunter pass over the committed diff surfaced 2 patches (0 intent_gap, 0 bad_spec, 0 defer, 24 reject) — both in `keeper-core::archive::export::mod.rs`, both Rust-only, no IPC/schema/timeline-VM/frontend changes:
- **Partial-output cleanup into a pre-existing folder (medium).** The prior pass's fix that spares a pre-existing user folder was too coarse — it skipped cleanup entirely, so a cancelled/failed re-export left partial `export.json`/`transcript.md`/`media/` behind. Cleanup now deletes only the artifacts this export writes when the folder pre-existed (still `remove_dir_all`s a folder it created), honoring the delete-partial-output contract without wiping user data. New regression test.
- **Chat slug omitted the account id (low).** `chat-<room>` could collide across two accounts sharing a room; now `chat-<account>-<room>`, consistent with the Account scope.

Verification (follow-up): `cargo fmt --check` + `cargo clippy --workspace --all-targets -D warnings` clean; `cargo nextest -p keeper-core` 358/358 (incl. the new cleanup regression test). Frontend/keeper crate untouched, so their gates are unaffected. `followup_review_recommended: false` — two localized, low/medium, well-tested fixes in one module.
