# Epic 5 Context: Local Archive, Search & Export — History That Survives

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic delivers keeper's trust pillar: every event keeper ever syncs across every Account lands on the user's own disk in `archive.db`, so message history stops depending on any platform's retention. The archive is durable against remote rewrites (remote edits become inspectable version chains; remote deletions mark but never erase, subject to a user toggle), searchable offline with full-text search returning first results in under 200 ms even at 100k+ events, exportable losslessly to JSON and readably to Markdown, and it outlives sign-out so leaving an account never silently destroys history. This is the moment "the platform's rewrite loses to the user's local copy" — the concrete promise that history belongs to the person, not the network.

## Stories

- Story 5.1: Archive Ingestion Pipeline
- Story 5.2: Durability Against Remote Rewrites + Edit History
- Story 5.3: Offline Full-Text Search Engine
- Story 5.4: Search UI — Global and In-Chat
- Story 5.5: Export to JSON and Markdown
- Story 5.6: Archive-First Pagination
- Story 5.7: Archive Survives Sign-Out — and Deletes Only on Command

## Requirements & Constraints

- Every event visible in any timeline (including decrypted E2EE content and media metadata) must be persisted and remain queryable after restart with the network disabled — no silent loss.
- Edits are retained as version chains; the timeline shows the latest with an "Edited" caption while priors stay retrievable via search/export. Remote redactions/deletions are always honored in the timeline view (stub shown) but the pre-redaction content stays retrievable unless the user enables an explicit "Honor remote deletions locally" toggle.
- FTS: first results must return in under 200 ms (p95 over a standard query set) at 100k+ events, offline. This is a CI performance gate, not an aspiration. Search must be case-insensitive and CJK-capable; queries under 3 characters fall back to accelerated `LIKE`.
- Search accepts sender / Chat / Network / Account / date-range filters and returns identifiers sufficient to deep-link into a timeline at the exact matched message.
- Export runs as a background job, never blocking messaging, with progress and cancel. JSON must be lossless (event count matches the archive) and Markdown a chronological transcript with sender, timestamp, final edited text, and media as relative file links. A 10k-message export is the reference verification case.
- Sign-out keeps the archive by default; FTS and Export must keep working for a signed-out Account with no active session. Archive deletion is a separate, deliberate, per-account destructive action that touches only that Account's rows.
- Crash safety: WAL mode everywhere; recovery to a consistent state with zero lost persisted events.
- At-rest posture (honest disclosure required): `archive.db` and `keeper.db` ship without passphrase encryption in this version and rely on FileVault; this must be stated plainly in settings. (Passphrase encryption applies to SDK stores only, handled in Epic 2.)

## Technical Decisions

- All archive code lives in `keeper-core::archive` (`ingest`, version chains, `fts.rs`, `export/{json,md}.rs`). It is tauri-free; view models cross IPC as `Vm`-suffixed types in `keeper-core::vm`, serde camelCase, ts-rs exported to `src/lib/ipc/gen/`.
- Storage: one `archive.db` for all Accounts, keyed by `account_id`, holding events plus the FTS index. Distinct from `keeper.db` (drafts/outbox/settings/registries) and per-account `accounts/<account_id>/sdk/` SDK stores. All SQLite in WAL mode. Identifiers are ULID account ids; timestamps ms-epoch.
- Ingestion is a per-account archiver task consuming post-decryption events, appending normalized rows (event id, account, room, sender, origin ts, type, content JSON, media metadata). A single serialized writer task owns `archive.db` — this is the invariant preventing writer races.
- Durability rule: edits append a version chain; redactions/deletions mark, never erase. The "honor remote deletions" setting governs retention only; the timeline view always honors redaction regardless.
- FTS is an FTS5 external-content table over message text with `tokenize="trigram"`, indexed incrementally at ingest.
- Export reads `archive.db` only, as a background job — it never touches the SDK store or live session, which is what lets it work after sign-out.
- Logout deletes only the SDK dir and that account's Keychain entries; archive deletion is a separate explicit path (a targeted row/FTS delete for one account). Secrets live only in the macOS Keychain (service `dev.tgorka.keeper`).
- Errors follow `thiserror` per module → `CoreError` → `IpcError`; `tracing` only, ids not content. Log archive/sign-out actions with ids only.
- Windowing/ordering for timelines is computed in Rust; the TS side renders streamed VMs and never re-derives.

## UX & Interaction Patterns

- Search-highlight is a dedicated theme token (light/dark) used as a background tint behind matched terms and on in-timeline jump targets — never as borders or text color.
- Global search (`⌘⇧F`): query + filter chips (sender, Chat, Network, Account, date range); results grouped by Chat with matches tinted; header states "Searching your local archive"; works fully offline. Enter deep-links into the timeline at the match, highlighted for 2 s. In-chat search (`⌘F`) runs the same engine scoped to the open Chat. Cross-account result identity is disambiguated by account hue dot + Account name in result meta.
- Empty/no-result states: "No matches in your archive." with active filter chips shown for one-tap removal and the offline note kept visible; empty Archive view reads "Nothing archived. `E` archives a chat and keeps it searchable."
- Edit history: the "Edited" caption is clickable and opens an edit-history popover fed by the Local Archive showing prior versions with timestamps. Redacted events show a stub.
- Export dialog: scope picker (this Chat / this Account / everything) → format checkboxes (JSON, Markdown) → include-media toggle → destination. Progress toast with counts and Cancel; completion adds Reveal in Finder; failure is a persistent alert in the Export surface (not toast-only) noting partial-file cleanup. Reachable from the detail panel, search results, and the command palette.
- Settings → Archive & Storage carries the plain disclosures: keeper keeps local copies of remotely edited/deleted messages by default, this affects only this Mac, the "Honor remote deletions locally" toggle, and the honest FileVault-only at-rest note.
- Sign-out AlertDialog: default "Sign out, keep local archive"; separate explicit destructive "…and delete this Account's archive" requiring the Account name typed. Copy must note that content never synced-and-decrypted before sign-out is not recoverable.
- Voice: sentence case, no exclamation marks, honest state narration, consequence-naming; keyboard-operable throughout (search, export, settings all complete pointer-free).

## Cross-Story Dependencies

- Ingestion (5.1) is the foundation; durability/edit-history (5.2), FTS engine (5.3), archive-first pagination (5.6), and export (5.5) all build on it. Search UI (5.4) depends on the FTS engine (5.3). Sign-out survival (5.7) depends on FTS (5.3) and Export (5.5) existing so survival can be verified.
- Depends on Epic 3: ingestion consumes post-decryption events, so E2EE decryption and media handling must be in place first.
- Completes threads left open in earlier epics: FR-6 sign-out keep/delete-archive semantics (the destructive path deferred from Story 2.5's dialog), FR-11 edit history (timeline leg from Story 3.4), and FR-17 archive-first pagination (homeserver leg from Story 3.9).
- Downstream: Epic 8's post-dispatch delete falls back to Redaction, whose local retention is governed by this epic's durability rules.
