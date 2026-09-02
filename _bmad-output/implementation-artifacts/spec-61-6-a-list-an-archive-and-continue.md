---
title: 'Story 61.6: a list, an archive, and continue'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-381, FR-382 — AD-154; AD-27 consumed (a delete of an unknown id is refused, not a no-op); the recordings-archive list shape and the find-bar conventions reused
depends on: 61.4 (`bot_sessions`/`bot_messages`, `bots_session_open`, the 71-line `bot-session-list.tsx` this story rewrites)

<intent-contract>

## Intent

**Problem:** Hermes keeps real server-side sessions, and it is tempting to make them the store. They are the wrong truth: compression
mints a successor with a renamed title, the stored-response cache is 100 rows LRU, and Ollama has no session at all. **The list is the
archive, and continue means replay.**

**Approach:** `search_sessions` in `keeper-core::bots::session` orders by latest activity — the later of `updated_ms` and the newest
message — with `id DESC` as the tie-break; search is `LIKE` over titles and bodies, case-insensitive, wildcards escaped; archive is a
flag, delete is a local transaction that names its count. The title is minted locally by `mint_title`, and `bots_ipc.rs`' own
`title_from` was cut over to it. The list gets search above the fold, `Active | All | Archived` chips, inline rename, and
continue-by-click into `bots_session_open`'s replay.

## Boundaries & Constraints

**Always** (each invariant with the tests that pin it):
- Newest activity first: `MAX(updated_ms, COALESCE(MAX(m.created_ms), updated_ms)) DESC, id DESC`; a rename counts as activity; two
  indexes back it, `bot_sessions_by_activity (updated_ms DESC, id DESC)` and `bot_messages_by_time (session_id, created_ms)` —
  `the_session_list_orders_by_activity_with_and_without_messages`, `the_session_list_counts_a_rename_as_activity`,
  `the_session_list_breaks_a_tie_by_id`.
- Search: `LOWER(title) LIKE '%'||LOWER(?)||'%' ESCAPE '\'` OR the same over `bot_messages.content` via `EXISTS`; `escape_like` makes `%`
  and `_` literal; FTS5 is AD-12's answer if the archive ever needs it — `session_search_finds_a_word_that_is_only_in_a_body`,
  `session_search_is_case_insensitive_and_takes_wildcards_literally`, `session_search_with_no_text_returns_the_whole_scope`.
- `total` is counted by the same predicates with no `LIMIT`; page limit `0` → `DEFAULT_SESSION_PAGE` 50, clamped to `MAX_SESSION_PAGE`
  200; the count line shows Rust's total via `countLabel(shown.length, CONVERSATIONS, {of: total})`, never `rows.length` —
  `the_session_list_pages_and_still_reports_the_real_total`.
- Archive is reversible and keeps every message, with no dialog because it is undoable from the same menu —
  `archiving_a_session_is_reversible_and_keeps_every_message`, `archiving_a_session_that_is_not_there_says_so`.
- Delete is a local transaction — **no remote request** — behind an `AlertDialog` naming the conversation and its message count (the
  chain-of-custody rule); it removes its messages and only its messages; an unknown id is refused —
  `deleting_a_session_deletes_its_messages_and_only_its_messages`, `deleting_a_session_is_idempotent_and_reaches_the_archive`.
- The title is the first non-blank line of the first user message, bounded at `MAX_TITLE_CHARS`, `UNTITLED_SESSION` when empty; a rename
  goes through `mint_title` too; no second model call — `the_title_minter_takes_one_clean_line_from_the_first_message`.
- Only the open row that holds a `remote_session_id` shows it, with the plain sentence that the remote may have compressed it into a
  successor. Continue is a click: `onOpen` → `botsSessionOpen` replays from keeper's store; resume needs no endpoint. Two searches in
  flight resolve by a monotonic stale guard on both paths. The 22 cases in `bot-session-list.test.tsx` cover the chips, the 200 ms
  debounce and stale guard, rename (Enter/Escape/confirm), the delete dialog's wording, the remote-session sentence and the two empty sentences.

**Block If:** nothing. **Never:** a Hermes session as the store; a title from a model; a delete that reaches a remote;
`dangerouslySetInnerHTML`; platform sniffing; a new date vocabulary (relative dates through the existing `formatDraftAge`); a new dependency.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Order | sessions with and without messages; a rename | by latest activity, then `id DESC` | — |
| Search | a word only in a body; `50%`, `a_b`, mixed case; `""` | found; wildcards literal, case-insensitive; the whole scope | — |
| Page | limit 2 of 5 | 2 rows, `total: 5` | limit 0 → 50; > 200 → 200 |
| Archive / delete | toggle; confirm | reversible, messages kept; session and its messages gone, others untouched, idempotent | unknown id → says so / refused |
| Title | `"  \n hello world … (200 chars)"` | one clean line, ≤ `MAX_TITLE_CHARS` | blank → `UNTITLED_SESSION` |
| Stale results / empty | two searches in flight; no sessions or no match | the stale one dropped; two distinct sentences | — |

</intent-contract>

## Code Map

| file | state | what shipped |
|---|---|---|
| `src-tauri/crates/keeper-core/src/bots/session.rs` | +296 → 1217 | `mint_title`, `MAX_TITLE_CHARS`, `UNTITLED_SESSION`, `SessionScope{Live,All,Archived}`, `SessionQuery`, `SessionListRow{session, latest_activity_ms, message_count}`, `SessionPage{rows, total}`, `search_sessions`, `escape_like`, two indexes in `open()` |
| `src-tauri/crates/keeper-core/tests/bots_session_list.rs` | new, 497 | 12 tests over real temp SQLite |
| `src-tauri/crates/keeper-core/src/vm.rs` | +122 | `BotSessionScope`, `BotSessionQueryReq`, `BotSessionRowVm`, `BotSessionListVm` (+ compose / `to_query` / `to_scope`) |
| `src-tauri/crates/keeper/src/bots_ipc.rs`, `lib.rs` | +104/-33, +10 | `bots_sessions_search(req)`, `bots_session_rename(sessionId, title)`, `bots_session_archive(sessionId, archived)`, `bots_session_delete(sessionId)`, `session_vm`; `title_from` and its test cut over to `session::mint_title`; four registrations. **Not compiled on this host** |
| `src/components/bots/bot-session-list.tsx` (+ test) | rewritten, 71 → 485; new, 449 | search (`InputGroup`, 200 ms debounce, stale guard), scope chips as `aria-pressed`, count line, inline rename (the `space-row-menu` shape), archive, delete dialog, remote-session sentence, continue; 22 tests |
| `src/lib/ipc/client.ts`, `dev/mock-shell.ts`; `src/lib/ipc/gen/{BotSessionScope,BotSessionQueryReq,BotSessionRowVm,BotSessionListVm}.ts` | +72, +71; generated | four wrappers and types; four handlers (search really filters titles + bodies) and one archived fixture; ts-rs export during nextest |
| `src/components/bots/bots-pane.tsx` (+test) | +2, +4 | `onChanged={() => void refresh()}`, `onClosed={() => botsStore.getState().openConversation(null)}`; one mock entry for `botsSessionsSearch` |

DDL: no new columns — `archived` and `remote_session_id` already existed. Two new indexes as above.

## Tasks & Acceptance

**Execution:** [x] ordering, search, paging in core · [x] title minting moved from the shell into core · [x] the four commands · [x] the
list rewrite · [x] mock-shell handlers · [x] the two-line pane wiring. **Acceptance Criteria:** listed newest-first, searched over titles
and bodies, renamed, archived reversibly, deleted with a confirmation naming what happens to which object — **met**; resume replays from
keeper's store — **met** at the store level; a held Hermes `session_id` shown with the compression sentence — **met**; titles minted locally — **met**.

## Design Notes

**The title minter moved into core.** 61.4 had `title_from` in `bots_ipc.rs`; this story made it `session::mint_title` so the
first-message title and the rename verb share one bound, and deleted the shell's copy and its orphaned test.

## Deferred

A second model call to title a conversation — DW-211. A silent request to a paid endpoint is the surprise this app does not ship.

## Verification

- `cargo nextest run -p keeper-core -E 'binary(bots_session_list) + test(session)'` → 433 passed. `bunx vitest run
  src/components/bots/bot-session-list.test.tsx` → 22 passed. `cargo clippy -p keeper-core --all-targets -- -D warnings` clean; `bunx tsc
  --noEmit` no output; biome clean on five TS files. Coordinator's tree-wide gate: 4028 Rust / 5466 frontend tests green.

| Mutation | Test that failed |
|---|---|
| (a) `ORDER BY … DESC, id DESC` → `ASC, ASC` | `the_session_list_orders_by_activity_with_and_without_messages`, `the_session_list_counts_a_rename_as_activity`, `the_session_list_breaks_a_tie_by_id` (+ three collateral) |
| (b) archive-as-delete: `UPDATE bot_sessions SET archived` → `DELETE FROM bot_sessions` | `archiving_a_session_is_reversible_and_keeps_every_message` |
| (c) delete leaves messages: `… WHERE session_id = ?1 AND 0 = 1` | `deleting_a_session_deletes_its_messages_and_only_its_messages` |
| (d) `take(MAX_TITLE_CHARS - 1)` → `take(MAX_TITLE_CHARS + 40)` | `the_title_minter_takes_one_clean_line_from_the_first_message` — `left: 101, right: 60` |

Restored from a byte copy, `diff` reports identical, 433 passed. **Not verified here:** the four commands, `session_vm` and the `title_from →
mint_title` cutover never met a compiler — the `keeper` shell crate does not build on Linux; everything they call is compiled and tested in
core. "Resume works with the endpoint unreachable" and "delete issues no remote request" are argued structurally (no HTTP call exists on
either path) and asserted at the store, not observed against Ollama or Hermes. No browser, so `dev/mock-shell.ts`' handlers and the list at
narrow widths are unseen.
