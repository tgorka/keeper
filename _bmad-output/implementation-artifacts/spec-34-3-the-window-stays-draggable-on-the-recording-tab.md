---
title: 'The window stays draggable on the Recording tab'
type: 'bugfix'
created: '2026-07-28'
status: 'review'
baseline_revision: '5c40a22'
---

<intent-contract>

## Intent

**Problem:** Recording is the only primary view where the window will not drag by its titlebar band. It renders nothing over the band — that was ruled out exhaustively in the epic (no `fixed`/`absolute` sibling above it, no pointer handlers, no `-webkit-app-region` anywhere in the repo). What is Recording-only is what it does to the *main thread* at the moment a drag begins:

1. It is the only view calling non-`async` `#[tauri::command]`s that do filesystem work. The Tauri v2 docs are explicit: "Commands without the `async` keyword are executed on the main thread unless defined with `#[tauri::command(async)]`" — and on macOS that is the same thread on which `startDragging` resolves to `performWindowDragWithEvent`. `recording_status` is polled at ~1 Hz for the whole session and, through `recording_snapshot`, `read_dir`s the session folder and `stat`s every segment on each tick. `recovered_sessions_list` is a `read_dir` plus a manifest load per subfolder. `recording_settings_get` is six `keeper.db` reads plus a destination probe, mounted three times per pane. `recording_session_summary` loads a manifest off a possibly-removable volume.
2. It is the only view that spawns sidecar processes on a timer *and* on every window `focus` — i.e. exactly when you click an unfocused titlebar to drag it. `recording-source-picker.tsx` re-enumerated sources on `focus` (a fresh `keeper-rec` each time) and `use-recording-permission.ts` re-probed on both `focus` *and* `visibilitychange`, which macOS fires back-to-back on a window return. One titlebar click cost two process launches.

**Approach:** Per AD-34-5, every recording command that touches the filesystem becomes `async`, which alone takes it off the main thread; and because a blocking body inside an `async` command then occupies a runtime worker instead (the reason `export_start` already hands its job to `tokio::task::spawn_blocking`), each such body is handed to the blocking pool through one new helper, `off_async_runtime`. `recording_snapshot` is split into its lock-held half (`live_snapshot`) and its disk half (`with_disk_figures`) so the async command path can compose them across a thread boundary while the tray and the quit gate keep the synchronous one-piece read. Per AD-34-6, both focus-driven refreshes move to the trailing edge of a coalescing window: a burst becomes one spawn, and no spawn lands on the mousedown that starts the drag.

The third mechanism the epic proposed — defaulting the shared `Select` wrapper to `modal={false}` — **cannot be implemented as specified**: Radix `Select` has no `modal` prop. The wrapper is hardened instead in the one way that is available to it, and the discrepancy is documented under Design Notes.

## Boundaries & Constraints

**Always:** Keep every command's name, argument list and success/error type identical — the TS bindings in `client.ts` already `await invoke(...)` for all seven, so `async`-ification is invisible to the frontend and no binding changes. Keep `recording_snapshot(&AppState) -> RecordingStatusVm` synchronous and `pub(crate)`: `lib.rs:222` (tray tick), `lib.rs:562` (`ExitRequested`) and `tray.rs:199` (Reveal) have no runtime to await on. Preserve Story 18.3's rule that the `recording_run` slot is never held across a blocking `read_dir`/`stat`. Preserve the byte-identical-figures guarantee between the tray line and the in-app banner. Keep the mount-time permission probe and the mount-time source enumeration immediate — only the *return-to-window* paths coalesce. Every command keeps a `Result` return type, which is what makes `State<'_, AppState>` legal in an `async` Tauri command.

**Block If:** (none — AD-34-5 and AD-34-6 settled the diagnosis; the `Select` premise failure was resolvable by verification rather than by a decision, see Design Notes)

**Never:** Do not change `recording_snapshot`'s signature (three callers outside this story's files). Do not touch `parse_req`, `sync_ipc.rs`, `engine.rs`, `sync-pane.tsx`, or any `SyncProfileReq`/`SyncActivityVm` field — other stories in this batch own those. Do not write `modal={false}` on a Radix `Select`: it is a TypeScript excess-property error and `SelectProvider` would silently drop a cast-through value, so it would be a placebo. Do not delete the focus refreshes outright — AD-34-6 says focus may still invalidate cached data. Do not make the mount probe wait.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| `recording_status` while live | a session in the slot, folder on disk | same VM as before: driver snapshot + read-time bytes + session cap; `read_dir`/`stat` on the blocking pool | a join failure (panicked body / runtime shutdown) surfaces as `Internal`, `retriable: false` |
| `recording_status` with no session | empty slot | `RecordingStatusVm::idle()`, returned without ever reaching the pool | no error path — nothing is spawned |
| `recording_status`, folder vanished mid-session | `output_path` points at a deleted folder | byte figures are 0, cap still stamped, state untouched | best-effort, never an error (Story 18.3) |
| `recording_acknowledge` on a terminal session | `Failed`/`Finalized`/`Recovered` slot | slot cleared, then the idle snapshot | as `recording_status` |
| `recording_acknowledge` on a live session | `Recording` slot | strict no-op, live snapshot returned | never a silent stop |
| `recording_stop` | live or already-settled session | one-shot trigger fired, `Ok(())`; idempotent | no blocking work, so no join failure possible |
| `recording_session_summary` | a folder with a readable manifest | manifest-authoritative `{screenSegmentCount, totalBytes, sessionFolder, title}` | a load failure is the same `IpcError` as before, so the card still falls back to folder + Reveal |
| `recovered_sessions_list` | destination with recovered / finalized / stray subfolders | only unacknowledged `Recovered` folders, sorted by basename | missing destination → `[]`; per-entry failure logged and skipped |
| `recording_settings_get` | any registry state | concrete, in-bounds, clamped VM with the effective destination | registry failures funnel through `to_ipc_error` unchanged |
| `recording_settings_set` | a VM to persist | six writes then the re-read, all in one blocking hop | a mid-sequence failure aborts before the re-read, exactly as before |
| Focus burst on the source picker | 4 `focus` events milliseconds apart | zero enumerations on the events; exactly one after `RECORDING_SOURCE_POLL_MS` | a failed enumeration leaves the prior list rendered |
| Return-to-window on the permission hook | `focus` + `visibilitychange` back-to-back | zero probes on the events; exactly one after `RETURN_PROBE_COALESCE_MS` | a failed probe degrades to the safe default (Start disabled) |
| Unmount inside a coalescing window | a probe/refresh queued, then teardown | the queued callback fires and is a no-op — no sidecar spawn after teardown | n/a |
| Genuine System Settings round-trip | grant made, user switches back seconds later | one probe, rows flip | unchanged |
| Any `Select` closes | option list dismissed | `Presence` unmounts on the same tick, so `DismissableLayer` restores `document.body.style.pointerEvents` at close | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/ipc.rs` -- new `off_async_runtime` (beside `to_ipc_error`); `recording_snapshot` split into `live_snapshot` + `with_disk_figures` with the new `recording_snapshot_off_runtime` async twin; `recording_stop`, `recording_status`, `recording_acknowledge`, `recording_session_summary` (both cfg arms), `recovered_sessions_list` (both cfg arms), `recording_settings_get` and `recording_settings_set` all `async`; new `read_recording_settings` shared by get and set; `acknowledge_recording` re-documented as the tray's synchronous path; one new test in `mod tests`.
- `src/components/recording/recording-source-picker.tsx` -- the `active` effect's `focus` listener (trailing-edge coalescing keyed on `RECORDING_SOURCE_POLL_MS`), its import, and the file doc comment.
- `src/hooks/use-recording-permission.ts` -- new exported `RETURN_PROBE_COALESCE_MS`; the mount effect's `focus` + `visibilitychange` listeners share one coalesced `probeOnReturn`; file doc comment.
- `src/components/ui/select.tsx` -- doc comment on `Select` recording Radix's unconditional modality; `SelectContent` loses its three `data-closed:*` exit-animation utilities and gains the doc comment explaining why.
- `src/components/recording/recording-source-picker.test.tsx` -- one new coalescing test.
- `src/hooks/use-recording-permission.test.tsx` -- the two return-to-window tests now advance the coalesce window; one new burst-coalescing test; the stale-probe test kicks its second probe via `refresh()`; the unmount test also proves a queued probe cannot spawn.

## Tasks & Acceptance

**Execution:**
- [x] `ipc.rs` -- Add `off_async_runtime<T, F>(body: F) -> Result<T, IpcError>`: `tokio::task::spawn_blocking(body).await` with the join error mapped through `to_ipc_error(CoreError::Internal(..))`. -- One place states the AD-34-5 rule, and six commands stop repeating a join-error mapping.
- [x] `ipc.rs` -- Split `recording_snapshot` into `live_snapshot(&Mutex<Option<RecordingRun>>) -> Option<(RecordingStatusVm, u32)>` and `with_disk_figures(RecordingStatusVm, u32) -> RecordingStatusVm`; keep the synchronous `recording_snapshot` composing them and add `recording_snapshot_off_runtime` composing them across the pool. -- Story 18.3's "never hold the slot across `read_dir`" becomes a signature rather than a comment, and both paths provably produce the same figures.
- [x] `ipc.rs` -- Make `recording_stop`, `recording_status` and `recording_acknowledge` `async`; the latter two go through `recording_snapshot_off_runtime`. `recording_acknowledge` now calls `acknowledge_recording_slot` directly, so `acknowledge_recording` remains as the tray's synchronous path. -- The ~1 Hz poll and the dismiss both leave the main thread.
- [x] `ipc.rs` -- Make both cfg arms of `recording_session_summary` and `recovered_sessions_list` `async`; the desktop bodies run their whole filesystem unit inside `off_async_runtime` (annotated closure return types, matching `engine.rs`'s `spawn_blocking` idiom). -- The two heaviest reads the pane issues leave both the main thread and the runtime workers.
- [x] `ipc.rs` -- Extract `read_recording_settings(&Path)`; make `recording_settings_get` `async` around it; make `recording_settings_set` `async` with its six writes *and* the re-read in one blocking closure. -- `set` had to change because it called `get`; doing it properly also keeps write-then-read on one thread.
- [x] `recording-source-picker.tsx` -- Replace the immediate `void refreshRecordingSources()` in `onFocus` with a monotonic-token schedule at `RECORDING_SOURCE_POLL_MS`; bump the token in the effect cleanup. -- A focus burst is one enumeration and none of it on the mousedown.
- [x] `use-recording-permission.ts` -- Add and export `RETURN_PROBE_COALESCE_MS = 500`; route both `focus` and `visibilitychange` through one coalesced `probeOnReturn`; bump the token in cleanup. -- The `focus`+`visibilitychange` pair macOS fires on a window return costs one sidecar spawn, after the click.
- [x] `ui/select.tsx` -- Drop `data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95` from `SelectContent`; document on `Select` that Radix `Select` has no `modal` prop and on `SelectContent` why the exit animation is gone. -- The body pointer-events lock can no longer outlive the closed state.
- [x] `ipc.rs::tests` -- `the_snapshot_halves_compose_into_one_authoritative_read`: empty slot yields `None`; a live slot yields the cloned snapshot plus the session cap (which the stored snapshot does not carry); the clone is immune to a later driver write; the disk half stamps the cap and reads real bytes, and zeroes them for a vanished folder.
- [x] `recording-source-picker.test.tsx` -- `does not enumerate on the focus event itself; a burst costs one read (AD-34-6)`.
- [x] `use-recording-permission.test.tsx` -- Advance the coalesce window in the visibility and focus tests; add `coalesces a return-to-window burst into one sidecar probe (AD-34-6)`; retarget the stale-probe test at `refresh()`; extend the unmount test to a probe queued before teardown.

**Acceptance Criteria:**
- Given the Recording pane is mounted, when any of `recording_status`, `recording_session_summary`, `recovered_sessions_list`, `recording_settings_get`, `recording_settings_set`, `recording_stop` or `recording_acknowledge` is invoked, then no filesystem syscall it performs runs on the main thread, and none occupies a runtime worker for the duration either.
- Given a burst of window `focus` events, when the coalescing window elapses, then at most one `keeper-rec` was spawned per source-picker interval and per permission interval, and none was spawned on the event itself.
- Given a hook or picker unmount with a refresh already queued, then no sidecar is spawned after teardown.
- Given a genuine return from System Settings, when the user switches back, then the permission rows still flip without a relaunch.
- Given any `Select` is dismissed, then `document.body.style.pointerEvents` is restored on the same tick as the close, not after an animation.
- Given the tray's ~1 Hz tick and the `recording_status` poll read at the same instant, then they still render byte-identical size, segment and meter figures.

## Design Notes

**Why both `async` and `spawn_blocking`.** They fix different halves. The Tauri v2 docs state the rule for each: "Commands without the `async` keyword are executed on the main thread"; "Async commands are executed on a separate async task using `async_runtime::spawn`". So the `async` keyword alone is what unblocks `performWindowDragWithEvent` — that is the drag bug. But an `async` command whose body is a synchronous `read_dir` then pins a runtime worker for its duration, which starves messaging, the sync engine and the recording driver task; `export_start` already routes around exactly that with `tokio::task::spawn_blocking` ("runs off the async runtime so it never blocks messaging (AD-11)"). `off_async_runtime` is a thin wrapper over that same call with the existing `to_ipc_error(CoreError::Internal(..))` mapping — not a second idiom, and not a second error taxonomy. The join error is non-retriable because the only ways to reach it are a panicked body or a runtime in shutdown.

**Why `recording_snapshot` is split rather than made async.** Three callers cannot await: the tray tick (`lib.rs:222`), the `ExitRequested` quit gate (`lib.rs:562`) and the tray's Reveal item (`tray.rs:199`). Changing its signature would mean editing two files this story does not own. The split is the better shape anyway: the old body carried a four-line comment explaining that the slot must be released before the disk I/O and relying on a scoped block to do it, whereas `live_snapshot` returning owned values makes that structural — the guard cannot escape it. It also solves the `Send` problem for free: a `std::sync::MutexGuard` held across an `.await` would make the command future `!Send` and fail to compile, and because `live_snapshot` is a separate non-`async` function its guard is never part of the future's state.

**Why `live_snapshot` takes the slot, not the state.** `acknowledge_recording_slot` already established that convention in this file for the same reason: the slot-level core is unit-testable with the existing `run_slot_in` helper, where an `AppState` is not constructible in a test. That is what makes the new test possible without a Tauri app.

**Why `recording_settings_set` changed even though the epic did not name it.** It called `recording_settings_get` for its re-read, so making the read `async` forced it. Leaving it non-`async` while extracting a synchronous helper would have kept six registry *writes* plus a destination probe on the main thread, in the same pane, in direct violation of AD-34-5's rule as written ("No `#[tauri::command]` does filesystem work on the main thread"). Doing it properly also let the writes and the re-read share one blocking hop, so nothing observes a half-written settings row from between them. This is the one scope decision the epic left open here.

**Why the coalescing is trailing-edge, and by monotonic token.** A leading-edge throttle would let the *first* focus after a quiet period run immediately — and that is precisely the focus delivered by the mousedown on an unfocused titlebar, so it would fix the burst case and miss the actual symptom. A trailing edge satisfies "at most one spawn per interval" for a burst *and* guarantees the spawn does not land on the click. The superseding mechanism is a monotonic counter rather than a stored timer handle because the permission hook already uses exactly that idiom for its in-flight probes (`seq`, "last-initiated wins"), it needs no handle type, and bumping the counter in the effect cleanup is a one-line way to make teardown outrun anything queued.

**Where the two intervals come from.** The source list is *already* re-enumerated every `RECORDING_SOURCE_POLL_MS` (3 s) for as long as the focus listener is bound — the effect starts the poll and the listener together — so a focus refresh can only ever anticipate the next tick. Coalescing it by exactly one poll interval means the focus path never adds a spawn the timer was not about to make anyway, and worst-case staleness the user sees is unchanged at 3 s. It is not deleted because it does still buy something: `setInterval` is throttled (and can be suspended) while the window is in the background, so the list may genuinely be stale on return, which is the case the focus path now exists to cover. The permission probe has no timer at all — it exists to catch a grant made in System Settings — so its window is sized against that human action: switching to System Settings, toggling a checkbox and switching back takes seconds, so `RETURN_PROBE_COALESCE_MS = 500` is imperceptible against it, while the `focus`/`visibilitychange` pair and any focus/blur burst are milliseconds apart and collapse completely.

**Mechanism 3: the epic's premise is wrong, and this is defensive hardening either way.** Two separate statements need correcting.

*There is no `modal` prop on Radix `Select`.* `@radix-ui/react-select@2.3.5` (what `radix-ui@1.6.5` pins) declares the Root's entire surface as `SelectSharedProps = {children, open, defaultOpen, onOpenChange, dir, name, autoComplete, disabled, required, form}` plus `{value, defaultValue, onValueChange}`; `grep -i modal` over both `dist/index.d.mts` and `dist/index.mjs` returns zero matches. `modal` exists on `Dialog` (default `true`) and `Popover` (default `false`), never on `Select`. Because the wrapper types itself as `React.ComponentProps<typeof SelectPrimitive.Root>`, writing `modal={false}` is a compile error, and a cast-through value would be silently dropped. Shipping it would have been a placebo that looked like a fix.

*The leak the epic hypothesised cannot happen.* Radix `Select` is unconditionally modal: `SelectContentImpl` passes a hard-coded `disableOutsidePointerEvents` (literally `disableOutsidePointerEvents: true` in the dist, unlike `Dialog`, which gates it on `open`), and `DismissableLayer` sets `document.body.style.pointerEvents = "none"` behind a module-level ref-count. But the restore lives in that effect's **cleanup**, not in a close handler — so a `Content` that unmounts *while open* (its option list re-rendered away by the 3 s source poll, the whole `Select` unmounting, a route change) does restore the body. The stranding scenarios that do exist are duplicate resolved copies of `@radix-ui/react-dismissable-layer` (there is exactly one today) and third-party writes to `body.style.pointerEvents` — neither addressable from this wrapper.

*What was hardened instead.* One real, in-repo exposure remains and it is fixable here. Because `disableOutsidePointerEvents` is a constant rather than `open`-gated, the body lock is held for as long as `Presence` keeps the closing content mounted — and `Presence` unmounts instantly only when the closed element computes `animation-name: none` (`presence.tsx`: `else if (currentAnimationName === 'none' || styles?.display === 'none') send('UNMOUNT')`). With `data-closed:animate-out` on the content, `position="popper"` would therefore hold `body { pointer-events: none }` for the whole ~100 ms exit animation *after* the list was dismissed: an invisible window in which the app is closed and a titlebar click silently does nothing, because Tauri's drag-region init script decides on `mousedown` whether `e.target` carries `data-tauri-drag-region` and with `body` unhittable the target is `<html>`. Removing the three `data-closed:*` utilities makes the closed state carry no `animation` declaration at all, so the lock is released on the same tick as the close, for every `position`. This costs nothing observable today: every `SelectContent` callsite in the app (three in `recording-advanced-controls.tsx`, one each in `recording-audio-controls.tsx`, `recording-webcam-controls.tsx`, `bbctl-panel.tsx`, two in `new-chat-dialog.tsx`, two in `add-folder-form.tsx`) uses the wrapper's default `position="item-aligned"`, which already suppresses both animations via `data-[align-trigger=true]:animate-none`. The rule exists so a future `popper` callsite cannot reintroduce the window.

**Which Selects would need modal behaviour.** The question the epic asked is moot — no callsite *can* opt out — but for the record none would want to: the only two in a modal container are the account and network pickers in `new-chat-dialog.tsx`, and a Radix `Dialog` already supplies both the scroll lock and the outside-pointer lock, with the nested `DismissableLayer` correctly regaining `pointer-events: auto` for its own portal.

**This mechanism is defensive hardening, not a confirmed root cause.** Nothing here was observed to strand `pointer-events: none`; the reasoning above in fact *rules out* the mechanism the epic suspected. Mechanisms 1 and 2 are the ones with a traced causal path to the symptom. Final confirmation for all three is dragging the real window on the Recording tab on macOS, which the parent agent performs — the epic's own acceptance: with the Recording tab active the window drags by its titlebar band as readily as on the Sync tab, and repeated open/close of the fps and mic Selects does not change that.

**One adjacent violation left alone, deliberately.** The tray's ~1 Hz tick calls the synchronous `recording_snapshot` from inside `tauri::async_runtime::spawn` (`lib.rs:212-235`), so while a session is live it does a `read_dir` plus a `stat` per segment on a runtime worker every second. That is not the main thread, so it is not the drag bug, and AD-34-5's rule is scoped to `#[tauri::command]`s — but it is the same starvation cost. `recording_snapshot_off_runtime` is exactly what that call site wants. It was not changed because `lib.rs` is append-only for this batch (five other agents are editing it) and the tick is outside this story's files.

## Verification

**Deliberately not run by me:** no `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt` or `bun run check` was executed for this story. Six agents were editing this worktree concurrently and the parent agent runs the full suite once at the end. Everything below is reasoning from source that was read, plus the three cheap fact-checks noted as commands.

**What was read and reasoned through:**
- The Tauri v2 "Calling Rust from the Frontend" docs, to verify AD-34-5's premise rather than assume it: "Commands without the `async` keyword are executed on the main thread unless defined with `#[tauri::command(async)]`" and "Async commands are executed on a separate async task using `async_runtime::spawn`". The same page's borrowed-argument caveat names `State<'_, Data>` and gives "wrap the return type in a `Result`" as the workaround that "works for all types" — every command changed here already returns `Result<_, IpcError>`, which is why the `State<'_, AppState>` parameters stay legal.
- `ipc.rs` around every changed command, plus `recording_snapshot`, `stop_active_recording`, `acknowledge_recording_slot`, `slot_lock`, `status_lock`, `effective_destination_dir` and `DesktopPlatform::data_dir`. `data_dir` is a `dirs::data_dir()` + `join` with no syscall of consequence, so it stays on the calling thread the way every other command does it; everything below it goes to the pool.
- `keeper-core/src/recording.rs:1755` and `:1803` to confirm `session_bytes_on_disk` / `current_segment_bytes_on_disk` are `read_dir` + `stat` loops — i.e. that `recording_status`, which the epic listed only as "polled at 1 Hz", genuinely does filesystem work through `recording_snapshot`.
- `grep` for every caller of `recording_snapshot` and `acknowledge_recording`: `lib.rs:222`, `lib.rs:562`, `tray.rs:191`, `tray.rs:199`. That is what fixed the shape — the synchronous form has to stay.
- `grep` for every Rust caller of the seven commands: only `recording_settings_set` → `recording_settings_get`. Tests call `scan_recovered_sessions` (still synchronous, now called from inside the blocking closure), never a command.
- `client.ts:1899-1991` to confirm all seven bindings already `await invoke(...)` and none needs a change; and `lib.rs:481-486`, where the invoke-handler entries are name-only and so unaffected by `async`.
- `Cargo.toml`: workspace `tokio` carries `rt-multi-thread` (hence `rt`, hence `spawn_blocking`), and workspace clippy is `all = warn` + `unwrap_used = warn` — the new code adds no `unwrap()`.
- `engine.rs`'s three awaited `spawn_blocking` sites, to copy the local convention of annotating the closure's return type (`move || -> Result<..>`) rather than leaning on inference through the generic.
- `recording-source.ts:114-170` for the store's in-flight dedupe and `RECORDING_SOURCE_POLL_MS`, and `use-recording-permission.ts`'s existing `seq` token, to size the intervals and to reuse the superseding idiom instead of inventing one. The store itself was not edited — the coalescing lives in the two files this story owns.
- Every `<Select` and `<SelectContent` callsite in `src/` (six files, eight instances) to establish that none passes `modal`, none passes `position`, and therefore that dropping the exit animation is observably inert today.
- `@radix-ui/react-select@2.3.5`'s published `dist/index.d.mts` and `dist/index.mjs`, plus `@radix-ui/react-presence`'s `dist/index.mjs`, for the modality and unmount findings above. A `librarian` subagent traced the same conclusions through `radix-ui/primitives` at tag `1.6.7` (whose select dist is byte-identical to 2.3.5), including `DismissableLayer`'s capture/restore ref-count.

**Fact-checks actually executed (no build, lint or test):**
- `grep -c -i modal` over `@radix-ui/react-select@2.3.5`'s `dist/index.d.mts` and `dist/index.mjs` -- both `0`, and the printed `SelectSharedProps`/`SelectProps` confirm the full prop surface.
- `grep -o "disableOutsidePointerEvents[^,)}]*"` over the same dist -- one hit, `disableOutsidePointerEvents: true`, i.e. a constant rather than an `open`-gated expression.
- A brace/paren/bracket balance pass over `ipc.rs` (whole file and each of the fourteen changed functions individually, spans auto-detected) and over all five changed TS/TSX files -- all zero. This catches truncation from the edits; it is not a substitute for `cargo check`.

**Tests added or changed:**
- `ipc.rs::tests::the_snapshot_halves_compose_into_one_authoritative_read` -- pins what each half owes. Fails if `live_snapshot` drops `run.segment_cap_mb` (the cap lives on the run, not the stored snapshot, so it must travel out of the lock or the meter loses its denominator once the read moves threads), if `with_disk_figures` stops stamping the cap or reading either byte figure, if the snapshot stops being a clone, or if a vanished folder starts erroring instead of yielding zeroes.
- `recording-source-picker.test.tsx::does not enumerate on the focus event itself; a burst costs one read (AD-34-6)` -- four `focus` events add no `listRecordingSources` call at all, and exactly one after `RECORDING_SOURCE_POLL_MS`. Fails immediately if the handler reverts to `void refreshRecordingSources()`. It calls `stopRecordingSourcePolling()` first so the count is about the focus path alone: the 3 s interval and the coalesced refresh would otherwise come due on the same tick, and whether the store's in-flight dedupe joined them or not would depend on how many microtasks the mock's promise chain took to settle — a real race in the *test*, not in the code.
- `use-recording-permission.test.tsx::coalesces a return-to-window burst into one sidecar probe (AD-34-6)` -- two `focus` and two `visibilitychange` events cost zero probes on the events and exactly one after the window, with nothing further queued afterwards. Fails if either listener probes eagerly or if they stop sharing one schedule.
- `use-recording-permission.test.tsx` unmount test -- now queues a probe *before* `unmount()` and advances past the window, so it fails if teardown stops outrunning queued work rather than only if a listener is left bound.
- The two existing return-to-window tests now advance the coalesce window explicitly (`vi.useFakeTimers({ shouldAdvanceTime: true })` + `advanceTimersByTimeAsync`, the pattern `sync-pane.test.tsx` and `copy-job.test.ts` already use) instead of passing incidentally inside `waitFor`'s default budget, and the visibility one asserts the probe count on both sides of the window.
- The stale-probe test now takes its newer token via `result.current.refresh()`. Token ordering is what that test is about; routing it through a coalesced `focus` would have made a timing detail load-bearing for an unrelated invariant.

**Commands for the caller to run:** `cargo test -p keeper`, `cargo clippy --all-targets`, `cargo fmt --check`, `bun run check`.

**Manual check still owed (from the epic, macOS-only):** on hesperia, with the Recording tab active, drag the window by its titlebar band and confirm it moves as readily as on the Sync tab — including while a session is live and the ~1 Hz poll is running, and immediately after repeatedly opening and closing the fps and mic Selects. This is the only confirmation that closes the story; the reasoning above narrows the cause to mechanisms 1 and 2 but does not observe the fix.

---

## Follow-up 2026-07-29 — the epic was wrong twice, and the real cause is an ACL grant

Everything above this heading is left as written. This section records what measurement found
afterwards, and what was changed on top.

### What the epic got wrong

**1. It is not Recording-specific.** Measured on macOS 26.5 (Tahoe, arm64) with the window pinned to
(200, 200, 1280x800) and its position read back from a full-screen capture: window **inactive** →
a click-drag on the band moves it; window **active** → it does not move. Both outcomes reproduce on
the **Chats** tab as well as the Recording tab (x stayed at 200 in both active runs). Chats spawns no
sidecar and calls no synchronous filesystem command, so mechanism A — main-thread stalls, the whole
premise of AD-34-5 as a *drag* fix — cannot explain the symptom. The `async`-ification above remains
correct work on its own terms (a `read_dir` on the main thread is a defect regardless), but it was
not the cause of what the user reported. The user perceived it as Recording-only; measurement says
every tab.

**2. The `pointer-events` theory is refuted, not merely unconfirmed.** With the window active, a real
foreground mouse click on a sidebar nav item lands and switches the view — impossible if
`document.body` were `pointer-events: none`. The drag also failed with no `Select` ever opened. The
`select.tsx` hardening stays (it is right on its own smaller merit: a body lock must never outlast
the open state), but its doc comment overstated the case as causation and now says the opposite.

### The measured rule, and the mechanism that produces it

`data-tauri-drag-region` is not a browser feature. It is a shim Tauri injects
(`crates/tauri/src/window/scripts/drag.js`): a **bubble-phase `document` `mousedown` listener** that
calls `window.__TAURI_INTERNALS__.invoke('plugin:window|start_dragging')` and **drops the returned
promise**. That command is ACL-gated, and `core:window:default` — the set this app inherits through
`core:default` — **does not contain `allow-start-dragging`**. Tauri's own custom-titlebar guide adds
`core:window:allow-start-dragging` as a separate, explicit line for exactly this reason. Neither
`capabilities/default.json` nor `capabilities/desktop.json` granted it.

So every webview-initiated drag this app has ever asked for was denied before it reached AppKit, and
denied *silently*, because the shim was not holding the promise that carried the denial. That
predicts the measured rule exactly:

- **Window inactive.** `WKWebView` does not accept the first mouse, so the click never reaches the
  DOM; AppKit activates and drags the window natively through the transparent title bar that
  `titleBarStyle: "Overlay"` keeps. No IPC, no ACL, no shim. **Moves.**
- **Window active.** The click reaches the DOM, and the shim is the only path to a drag → denied →
  nothing happens, nothing is logged, nothing rejects loudly. **Does not move.**

It also predicts what the epic could not: tab-independence, macOS-version-independence, and the total
absence of any trace anywhere.

**A prediction that can be checked on the shipped 0.6.3 build, with no rebuild.** The same shim
listener also implements double-click-to-zoom, and it does that through
`plugin:window|internal_toggle_maximize` — which `core:window:default` **does** include. So with the
window active, on the current build: **double-clicking the band should zoom the window while
dragging it does nothing.** Same listener, same event, same IPC path; the only difference is which of
the two commands the ACL allows. If that holds, the diagnosis is confirmed before a single line is
compiled. If double-click does nothing either, the event is not reaching the shim at all and the
`handler never fired` row of the table below is where the next build's log will land.

### What changed

- `src-tauri/crates/keeper/capabilities/desktop.json` — grants `core:window:allow-start-dragging`.
  **This is the fix.** Desktop-scoped because window dragging is meaningless on iOS, and least
  privilege is what a capability file is for.
- `src-tauri/crates/keeper/src/ipc.rs` — new `titlebar_drag_report(stage, detail)` command: the
  frontend's only path into `~/Library/Logs/keeper/keeper.log`. `WARN`, deliberately — the file leg
  admits `WARN`/`ERROR` regardless of the debug-mode toggle, and that toggle is off by default, so an
  `INFO` line would exist only on a stderr nobody reads. Registered in `lib.rs` in the single
  `generate_handler!` list.
- `src/lib/ipc/client.ts` — `startWindowDragging()` (`getCurrentWindow().startDragging()`, i.e. the
  same `plugin:window|start_dragging`) and `titlebarDragReport()`.
- `src/lib/titlebar-drag.ts` (new) — `beginTitleBarDrag()`: issue the drag, then report `issued`,
  then `accepted` or `refused` with the refusal text verbatim.
- `src/components/layout/app-shell.tsx` — `onMouseDown` on both band columns: primary button, direct
  hit, opening click only; `preventDefault` + `stopPropagation` so exactly one `start_dragging` is
  issued per gesture and the outcome is attributable to it. Both columns keep
  `data-tauri-drag-region`: where the handler does not run, the shim behaves as before.
- Tests: `src/lib/titlebar-drag.test.ts` (5 — synchronous issue before any await, both outcomes, ACL
  string preserved verbatim, never rejects) and three added to `app-shell.test.tsx` (drag asked for
  on both columns; secondary/middle/double-click ignored; the document-level shim is bypassed, proven
  non-vacuously).

### What the next install tests, and how to read it

Two things can refuse this drag and only one of them was fixed here. The second — AppKit honouring
`performWindowDragWithEvent:` only while `NSApp.currentEvent` is still the mouse-down being
processed, which an asynchronous IPC hop cannot guarantee — is a race the frontend can narrow but not
close. That is what the log lines are for. `grep 'titlebar drag' ~/Library/Logs/keeper/keeper.log`
after dragging the band with the window **active**:

| Log | Window | Conclusion |
|-----|--------|------------|
| nothing at all | did not move | The handler never fired: the `mousedown` never reached React. Look at event delivery (band geometry, an overlay, first-mouse), not at the drag command. |
| `issued` only | did not move | The call went out and never came back — the Rust side never answered (`start_dragging` blocking the main thread inside the nested AppKit drag loop is the candidate). |
| `issued` + `REFUSED detail=…` | did not move | Read `detail`. If it still names `core:window:allow-start-dragging`, the capability edit did not reach the built app (stale `gen/schemas`, wrong capability file, unsigned rebuild). Anything else is a new, named failure. |
| `issued` + `accepted` | did not move | The ACL is fixed and AppKit declined anyway — the `currentEvent` race, i.e. the upstream Tahoe-plausible half. Next step is Rust-side: capture the mouse-down event and drag from it, or drop to `isMovableByWindowBackground`. |
| `issued` + `accepted` | **moved** | Fixed, and it was the ACL. Drop `titlebar_drag_report` and its frontend caller; keep the capability grant and the explicit handler (it is what makes the failure visible next time). |

**Not verified here:** `cargo build`/`clippy`/`nextest` cannot run on the Linux box (tauri needs
GTK), so the new command is reasoned-and-reread, not compiled. `bun run typecheck`, `bun run lint`
and `bun run test` were run: 145 files, 1655 tests, all passing.
