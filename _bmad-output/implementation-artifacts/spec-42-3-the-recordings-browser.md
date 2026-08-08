---
title: 'Story 42.3: The Recordings Browser'
type: 'feature'
created: '2026-08-08'
status: 'review'
blocking_condition: ''
baseline_revision: '623c3c2'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-42-the-recordings-archive.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-2-searchable-sessions.md'
---

<intent-contract>

## Intent

**Problem:** 42.1 made a session a row and 42.2 made it findable, and neither is reachable by a
person. Every recording keeper has ever made is still a folder you open in Finder and squint at.

**Approach:** one IPC command over 42.2's engine, and a surface that is a **search first** — the
filter row is above the fold, not behind a disclosure, because the answer to "where is that session"
is a query, not a scroll. Rows carry what identifies a session at a glance (title, date, duration,
size, tags, a durability glyph) and three actions that get you out of keeper and into the file.

## Boundaries & Constraints

**Always:**
- The engine stays in `keeper-core` and stays tauri-free. This story adds the `Vm` seam
  (`From<RecordingFilterVm> for RecordingFilter`) and the command, exactly as `search_archive` does
  over `fts.rs`.
- An absent `archive.db` is an empty result, never an error dialog — `search_archive`'s rule.
- A fresh **read-only** connection per query. WAL admits concurrent readers, so browsing never
  touches the writer and works offline.
- Capability gating is **absence**, at all three layers the app already gates at: the nav entry, the
  pane render, and the pane's own doc comment saying it is gated upstream. Reuse the existing
  `recording` flag — a browser for recordings you cannot make is not a surface, it is a puzzle.
- Two empty states, two different sentences, following `notes-empty-state.tsx`: nothing recorded yet
  is not the same fact as nothing matching this filter, and they must not look the same on screen.
- Debounced input, and the debounce is asserted, not assumed.
- Reveal must open the folder a session is in **now** — story 40.4 moves folders, and 42.1's row
  follows the session, so the path in the row is the current one and Reveal uses it.

**Block If:**
- `bun run bindings:check` cannot run to completion. It gates this story; it is also the one gate
  that cannot run on Linux, so the macOS gate is the arbiter.

**Never:**
- No new search engine. Everything goes through 42.2's `search_recordings`.
- No media player. Play hands the file to the system handler and stops caring.
- No tag normalisation or completion (42.5). Tags render and filter as stored.
- No note stub (42.4).
- No write path of any kind. This surface reads.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| No `archive.db` | first run | empty result, no error surfaced | none |
| Nothing recorded | archive open, zero rows | the "no recordings yet" sentence | none |
| Filter matches nothing | rows exist, filter excludes all | the "no matches for this filter" sentence, filters still on screen | none |
| Typing | a query typed at speed | ONE call after the debounce, not one per keystroke | none |
| Stale response | a slow query resolves after a newer one | the newer result wins; the stale one is discarded | none |
| Tag filter | a tag chip | list narrows through `search_recordings`, hierarchical per 42.2 | none |
| Session recorded during the session under test | a new row lands | it appears without an app restart | none |
| Retitled session | 40.4 moved the folder | Reveal opens the CURRENT folder, never the old path | honest error if gone |
| Reveal where unsupported | `revealInFileManager` off | the action is absent; the path renders as inert text | none |
| Play | a row's media file | handed to the system handler after a containment check | honest `IpcError` |
| Copy session id | a row | the immutable id on the clipboard, transient confirmation | swallowed |
| No recording capability | a build without it | the surface is ABSENT from the DOM, and so is its nav entry | none |
| A row with no title | untitled session | renders its date and folder, never a blank line | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` — `RecordingFilterVm` and `RecordingHitVm`, beside the
  existing `Recording*Vm` siblings. **Decision:** `vm.rs`, not a new `archive/vm.rs`. The codebase has
  two competing homes; every other recording VM is here, and splitting the family costs more than the
  newer pattern buys.
- `src-tauri/crates/keeper-core/src/archive/recordings_fts.rs` — `From<RecordingFilterVm> for
  RecordingFilter`, mirroring `fts.rs:63`.
- `src-tauri/crates/keeper/src/ipc.rs` — `search_recordings` command; a `recording_open_path` that
  hands a file to the system handler after a containment check, in the shape
  `notes_ipc.rs:2351-2355` established.
- `src-tauri/crates/keeper/src/lib.rs` — both commands in the single `invoke_handler` literal.
- `src/lib/ipc/client.ts` — typed wrappers.
- `src/lib/stores/primary-view.ts` — a `recordings` member. **Decision:** a sibling view, not a tab
  inside `RecordingPane`. The epic calls it a browser; a browser buried under the capture settings is
  a browser nobody opens.
- `src/components/recordings/` (new) — the pane, the row renderer, the empty state, and their tests.
- `src/components/layout/sidebar-pane.tsx`, `app-shell.tsx` — the gated nav entry and render.
- Read-only models: `search-panel.tsx` (debounce + stale guard), `notes-empty-state.tsx` (two states),
  `recording-summary-card.tsx` (reveal + freshness), `app-shell.test.tsx:130-138` (absence assertion).

## Tasks & Acceptance

**Execution:**
- [x] `RecordingFilterVm` / `RecordingHitVm` + the `From` seam.
- [x] `search_recordings` and `recording_open_path` commands, registered.
- [x] Client wrappers and generated bindings.
- [x] The pane: filter row above the fold, rows, per-row actions, two empty states.
- [x] Capability gating at nav, render and pane.
- [x] Tests: every matrix row, including the fake-timer debounce proof and the absent-from-the-DOM
      assertion.

**Acceptance Criteria:**
- `bun run bindings:check` passes.
- Filtering by a tag narrows the list without a round trip per keystroke, asserted with fake timers.
- A session recorded during the session under test appears without a restart.
- Reveal-in-Finder opens the real folder for a session whose folder was renamed by story 40.4.
- On a build without the recording capability the surface is absent from the DOM.

## Design Notes

**Two VM homes existed; this story picked one and said so.** Every other `Recording*Vm` lives in
`keeper-core/src/vm.rs`, and the newer per-subsystem `vm.rs` pattern (notes, sync) would have split
the family for no gain a reader of either file could feel.

**`RecordingHitVm` grew two fields the brief did not name, and both earn their place.**
`absolutePath`, because `revealPath` takes an absolute path and AD-65 forbids the frontend joining a
destination root to a subfolder — Rust composes it. `playablePath`, because `RecordingHit` carries
only the session FOLDER, and a Play button that opens a folder is Reveal wearing a different label;
it is the first screen segment (or the first segment of any track, for an audio-only session), and
`null` when a session has no segment row, in which case the action is absent rather than inert.

**The projection is a function, not a `From`.** `search_recording_vms(conn, filter, root)` wraps
42.2's untouched `search_recordings`, because two of the row's fields need the connection and one
needs the shell-resolved root, and no `From` can reach either. `archive::fts::search` already returns
`Vec<SearchHitVm>` directly, so keeper-core producing VMs is the existing convention, not a new one.

**Containment is both halves of the AD-59 idiom.** `recording_open_path` runs the lexical check
(under the root, every component `Normal`, so `..` cannot walk out) and THEN canonicalises both sides
and re-checks — which is what catches a symlink planted inside the recordings folder. A command that
opens any path the webview names is a file-disclosure primitive; a path that no longer resolves is
refused honestly rather than handed to the opener.

**A pre-42.1 `archive.db` browses as empty, not as an error.** Such a file has no `recordings` table
and nothing can create one on a read-only connection, so the query would raise `no such table`. To
the person holding it that is the same fact as an absent database, and it now reads the same way.

**Rows are a list, not a listbox.** `search-result-list.tsx` can use `role="option"` because its rows
are single activatable buttons; these rows host two or three, and an ARIA option may not contain
interactive descendants. `<ul aria-label="Recording sessions">` is the honest structure.

**The durability glyph reuses the live banner's four constants** rather than restating them, so the
banner and the row cannot word the same promise differently. An unknown word prints nothing.

**Tag choices come from the tags present in the current result set** — the message-search
sender-suggestion idiom. Tags that co-occur with what is on screen are exactly the tags that can
narrow it further; a global tag list would offer choices that empty the list.

**Refresh is undebounced and shares the stale guard.** That is what makes "a session recorded during
the session under test appears without a restart" something a person can demonstrate rather than a
sentence in a spec.
