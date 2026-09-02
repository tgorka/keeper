---
title: 'Story 61.4: a conversation you can have'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-378, FR-379 — AD-152, AD-146; AD-C7 consumed (one component, two modes); AD-27 consumed; the `Channel<T>` subscribe-and-return-an-id pattern with the DW-4 cleanup rule
depends on: 61.1, 61.2, 61.3 — the store, the wire and discovery are composed here; 61.5, 61.6 and 61.7 refine the surface this story builds

<intent-contract>

## Intent

**Problem:** keeper has four primary surfaces, and a store, a client and a discovery layer that nothing on screen reaches. **A fifth
surface, built the way the other four were.** **Approach:** `PrimaryView` gains `"bots"`; `CapabilitiesVm` gains `bots`, computed in the
shell as `cfg!(desktop)` and mirrored in the frozen store default and `dev/mock-shell.ts`; the sidebar entry is absent, not disabled,
where the flag is off; ⌘9 is bound by a hook cloned from `use-tasks-shortcut.ts`. Two new tables hold sessions and messages. The driver in
`bots_ipc.rs` persists the assistant row empty-and-partial **before** the request goes out; only `close_message` clears the flag. Settings gains a Providers section, one component in two modes.

## Boundaries & Constraints

**Always** (each invariant with the tests that pin it):
- `bots` is `cfg!(desktop)` and **not** `notes_available()` — written in `vm.rs`, `ipc.rs` and `use-bots-shortcut.ts`: a conversation is
  two tables in `keeper.db` and a URL behind the secret port, so gating on `sessions` would hide a working surface on every desktop whose
  `git` is older than the sync floor. The sidebar row and the pane are absent, not disabled, when the flag is off — *has NO sidebar row
  when the capability is off*, *has no sidebar row on a full sync machine whose bots flag is off*, *does not render the pane when the
  capability is off*, *offers the sidebar row when the capability is on*, *is a no-op on a full sync machine whose bots flag is off*,
  *opens with folder sync off, because a conversation needs no git*.
- The grant half is gated on `sync`: `botGrantOffer()` in `bot-grant-bar.tsx` is a pure exported function — sync off → absent; Hermes →
  refused with the one sentence (tool execution is server-side, no client tool route, `unattended_mode` defaults to deny); `tools ===
  false` → absent; `tools === true | null` → offered, with a warning on `null`. Unknown is never flattened to no. This is where the
  coordinator's 2026-09-02 correction lands.
- The assistant row is persisted before the request; deltas flush at a 512-byte threshold — not a timer (AD-62 refuses a second clock),
  not per delta; only `close_message` clears `partial`; the terminal `closed` event re-reads the stored row so the surface renders what
  was stored — `closing_a_bot_session_message_writes_its_metadata` (asserts `stored.partial`), *renders a streamed reply progressively*,
  *leaves a partial row with an honest caption when the stream dies*, *stops a streaming answer by its subscription id*.
- Stop fires the `CancelHandle` rather than aborting the task, so the driver unwinds through its own cancel path and writes the partial
  row; `bots_chat_stop` is idempotent. Retry replaces the named answer **and everything after it**, refuses a non-assistant row, and
  takes the message id explicitly so a Retry on a stale render cannot delete a row that arrived after it was drawn.
- The composer's gesture rules — *sends the trimmed text on Enter and clears the field*, *leaves Shift+Enter to the textarea and sends
  nothing*, *does not bind ⌘Enter, and does not swallow it either*, *ignores Enter mid-IME composition*, *refuses to send an empty or
  whitespace-only message*, *offers Stop instead of Send while an answer is arriving*, *says why it cannot send when there is nothing to send to*.
- Providers in Settings, one component in two modes — *lists an endpoint by HOST and discloses that it is private*, *says a row has no
  key stored, which is not a fault by itself*, *says so when the stored key has gone missing*, *adds an endpoint through an inline
  disclosure, never a dialog (AD-C7)*, *edits the same row through the same component, with the id present*, *a blank key field means
  unchanged, never cleared*, *renders Rust's own refusal sentence for a base URL it will not send to*, *prints the probe verdict in the
  app's own reachability vocabulary*, *confirms a removal by naming what happens to which object*.
- `bots_ipc.rs` carries no `#[cfg(desktop)]` and registers in the shared `invoke_handler` literal (`config_layers`' precedent); `to_ipc_error`
  widened to `pub(crate)` so there is one `CoreError → IpcError` mapping (AD-21). `BotStreamEvent` has **one** terminal arm, `closed{message,
  reason}`. Every `BotMessageVm` number is nullable, never zero-filled; `role`/`finishReason` are `String`. `BotConversation` windows from the
  first line via `useWindowedRows` (`src/components/ui/window-list.tsx`). No `PanelStrip` beside the pane; the refusal is stated in `app-shell.tsx` with Story 59.13's measured cost.

**Block If:** nothing. **Never:** `message-bubble.tsx` reused (Matrix-typed down to read receipts); a second IPC error mapping; a dialog for add; a token echoed to the form (`token: null` = unchanged, `clearToken` = delete).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Send | Enter with trimmed text | `bots_chat_send` → `opened` → `firstToken` → `delta`… → `closed` | Shift+Enter, ⌘Enter, mid-IME Enter, empty text send nothing |
| Stop / stream dies | streaming; endpoint gone | partial row kept, `partial: true`, honest caption; unreachable-remote vocabulary | `closed{reason}` |
| Provider add / edit | inline disclosure; same component with `id` | `BotProviderSaveReq{id: null,…}` / `{id,…}` | Rust's refusal sentence rendered verbatim |
| Secret gone / probe / remove | key vanished; Test; Remove | a fact, not a send-time fault; verdict in reachability vocabulary; confirmation names the object | — |

</intent-contract>

## Code Map

| file | state | what shipped |
|---|---|---|
| `src-tauri/crates/keeper-core/src/bots/session.rs` | new, 922 | `bot_sessions` + `bot_messages` (17 scalar columns per message, no JSON blob), CRUD following `store.rs`; 11 tests |
| `src-tauri/crates/keeper/src/bots_ipc.rs` | new, 1251 | 14 `#[tauri::command]`s (61.10 appended 4): streaming driver, cancel registry, progressive partial-row persistence. **Not compiled on this host** |
| `src-tauri/crates/keeper-core/src/vm.rs`; `bots/mod.rs` | edited | `CapabilitiesVm.bots` with its doc; `BotSessionVm`, `BotMessageVm`, `BotConversationVm`, `BotStreamEvent` (tagged on `kind`, payloads boxed), `BotChatSendReq`, `BotRetryReq`, `BotProviderSaveReq`, `BotSaveReq`; `pub mod session;` |
| `src-tauri/crates/keeper/src/ipc.rs`, `lib.rs` | edited | `bots: cfg!(desktop)`; `to_ipc_error` → `pub(crate)`; `mod bots_ipc;` + 18 registrations in the one literal |
| `src/components/bots/` | new | `bots-pane.tsx` 337, `bot-picker.tsx` 194, `bot-conversation.tsx` 107, `bot-message.tsx` 99, `bot-answer.tsx` 22 (61.5 rewrites), `bot-message-meta.tsx` 32, `bot-tool-call.tsx` 26, `bot-composer.tsx` 157, `bot-slash-menu.ts` 51, `bot-paste.ts` 44, `bot-session-list.tsx` 71 (61.6 rewrites), `bot-pins-strip.tsx` 61 (61.7 rewrites), `bot-grant-bar.tsx` 109, `bot-empty-state.tsx` 74 |
| `src/components/settings/bots-section.tsx`; `src/lib/stores/bots.ts`; `src/hooks/use-bots-shortcut.ts` | new, 638; 210; 79 | Providers add/test/edit/remove; the store; ⌘9 with the five guards in order and the no-dead-chord rule |
| `src/lib/stores/primary-view.ts`, `capabilities.ts`, `src/components/layout/sidebar-pane.tsx`, `app-shell.tsx`, `src/components/settings/settings-dialog.tsx`, `src/lib/ipc/client.ts`, `dev/mock-shell.ts` | edited | `"bots"`; `bots: false` default; `BOTS_VIEW` spliced before Settings; `useBotsShortcut()` + the branch; `{bots && <BotsSection/>}`; 14 typed wrappers; `capabilities.bots: true`, fixtures, 19 handlers incl. a real fake stream |
| tests | new | `bots-pane.test.tsx` 9, `bot-composer.test.tsx` 7, `bots-section.test.tsx` 14, `use-bots-shortcut.test.ts` 9; 13 sibling fixture files gained `bots` in their `CapabilitiesVm` literals |

Commands: `bots_providers_list` · `bots_provider_save(req)` · `bots_provider_remove(providerId)` · `bots_provider_probe` · `bots_models_list(providerId,
bot?)` · `bots_bot_probe(providerId, target)` · `bots_bots_list` · `bots_bot_save(req)` · `bots_bot_remove(botId)` · `bots_sessions_list(includeArchived)` ·
`bots_session_open(sessionId)` · `bots_chat_send(req, channel) -> subscription id` · `bots_message_retry(req, channel)` · `bots_chat_stop(subscriptionId)`.
`BotStreamEvent`: `opened{subscriptionId, session, user, assistant}` | `firstToken{afterMs}` | `delta{text}` | `reasoning{text}` | `toolCall{name}` | `closed{message, reason}`.

## Tasks & Acceptance

**Execution:** [x] `PrimaryView` member · [x] the flag in three places or typecheck fails · [x] sidebar absent-not-disabled · [x] ⌘9 hook
· [x] sessions/messages store · [x] the commands and the driver · [x] pane, composer, picker, empty state · [x] Settings Providers ·
[x] mock-shell fixtures and fake stream · [x] the fixture ripple. **Acceptance Criteria:** a surface at ⌘9 that narrates its own state
honestly — **met**; a provider added, tested and removed from Settings — **met**; streaming renders progressively, Stop stops, Retry
re-sends — **met** in jsdom against the mock stream; an unreachable endpoint uses the app's reachability vocabulary — **met**.

## Design Notes

**One refusal worth its sentence.** No `tempfile` was added; the session tests use the house scratch-dir helper carried from `bots/store.rs`. No new dependency anywhere in this story.

## Verification

- `bunx vitest run src/components/bots src/components/settings/bots-section.test.tsx src/hooks/use-bots-shortcut.test.ts` → 4 files, 39
  tests. `cargo nextest run -p keeper-core -E 'test(bot_session) or test(export_bindings_bot)'` → 31 passed. `bunx tsc --noEmit` clean;
  biome clean on 28 files. Coordinator's tree-wide gate: 4028 Rust / 5466 frontend tests green, lint, typecheck, fmt, clippy clean,
  `node scripts/check-design.mjs` at zero violations.

| Mutation | Test that failed | Observed |
|---|---|---|
| sidebar gate `...(bots ? [BOTS_VIEW] : [])` → `BOTS_VIEW,` | *has NO sidebar row when the capability is off*, *has no sidebar row on a full sync machine whose bots flag is off* | 2 failed / 7 |
| pane gate `bots && primaryView === "bots"` → `primaryView === "bots"` | *does not render the pane when the capability is off* | 1 failed / 8 |
| `session.rs` `close_message`: `i64::from(close.partial)` → `0i64` | `closing_a_bot_session_message_writes_its_metadata` — "a stopped answer is final and incomplete" | 1 failed / 10; restored, 31/31 |

**Not verified here.** The whole `keeper` shell crate: every line of `bots_ipc.rs`, the `mod` and 18 registrations, and `bots: cfg!(desktop)`
are written and never compiled on this Linux host — the `Channel<BotStreamEvent>` plumbing, the `LazyLock` stream registry,
`spawn_turn`/`drive`/`close`/`close_failed`/`emit_closed`, `replay`, the `BotsError → IpcError` mapping, and whether `pub(crate)
to_ipc_error` satisfies every caller. The 512-byte flush policy lives in that uncompiled driver; only the store half is tested. No real
endpoint, no real SSE socket, no real cancel of an in-flight stream: 61.2 owns those against its server, and this story's composition of
them is unproven end to end. No browser, no pixel; jsdom lays nothing out. A macOS `cargo check -p keeper` is owed before anything here is called reachable.
