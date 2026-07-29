---
title: 'The tray glyph stops repainting itself'
type: 'bugfix'
created: '2026-07-28'
status: 'review'
baseline_revision: '5c40a22'
---

<intent-contract>

## Intent

**Problem:** Two independent defects made the menu-bar icon churn on a folder where nothing was happening.

1. `apply_sync_state` (`keeper/src/tray.rs`) ended every ~1 Hz tick by decoding the same PNG and calling `set_icon` + `set_icon_as_template`, and re-pushed an identical status string through `MenuItem::set_text`, forever. The recording renderer never did this — `render_recording` guards on the held `status_item` and `render_error` on `error_rendered`; the sync renderer was the outlier.
2. `Engine::collect_stable_changes` (`keeper-sync/src/engine.rs`) published `SyncPhase::Scanning` on entry. `Engine::publish` promotes any active phase to `ProfileState::Syncing`, and `refresh_pending` clears it back inside the same tick — so a walk that staged nothing still left the snapshot claiming the profile was busy for the duration of the walk. The tray samples that snapshot on its own timer, caught the window at random, and the 3-tick `BUSY_DWELL_TICKS` hold then pinned the busy glyph for roughly four seconds.

**Approach:** Per AD-34-1, a write happens on a transition, never on a tick. `TrayState` gains a `sync_memo` recording the glyph identity and the status text this tray was actually given; `sync_writes` diffs the composed tick against it and a tick that owes neither write returns before touching the OS. The glyph identity is the address of the `&'static [u8]` asset `sync_glyph` returned, so the transfer animation is compared frame by frame at no cost and nothing is decoded or hashed to make the comparison. Upstream, `collect_stable_changes` defers its `Scanning` publish until the walk knows it has work, so a fruitless scan publishes nothing at all.

## Boundaries & Constraints

**Always:** Keep the state→glyph mapping in `sync_glyph` and the smoothing in `dwelled` exactly as they are; `dwelled` is still called once per tick before the memo is consulted, so its hold still decays on schedule. Memoise only writes that actually landed. Reset the memo wherever the tray is rebuilt (`set_tray_presence`, `force_present`) or its rendering is displaced (`store_rendered_mode`, reached from `restore_idle`, `render_recording` and `render_error`). Preserve the file's lock discipline: no `TrayIcon`/`MenuItem` call runs with the tray guard held.

**Block If:** (none — the diagnosis and the two options for the engine half were settled in the epic)

**Never:** Do not hash or decode a PNG to decide whether to repaint. Do not delay a genuine change by a tick — this is a diff, not a dwell. Do not stop `Transferring` from animating. Do not change `Engine::publish`'s promotion rule, and do not touch `refresh_pending`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Idle tick, nothing changed | memo matches glyph + line, sync menu installed | no `set_icon`, no `set_icon_as_template`, no `set_text`; returns early | n/a |
| Glyph changed, same words | `Armed` → `Warning`, identical line | `set_icon` only | decode/set failure leaves the memo unset, so the next tick retries |
| Words changed, same glyph | pending count moves under `Armed` | `set_text` only | a failed `set_text` leaves the memo unset; next tick retries |
| `Transferring` | frame advances every tick | icon written every tick — the ring keeps turning | as above |
| First tick after a tray is built | `sync_memo` is `None` | both writes; the sync menu is installed | menu build/install failure returns before touching the icon, exactly as before |
| Recording takes the tray | `render_recording` / `render_error` installs its menu | held sync line and memo dropped; sync re-installs after the recording ends | a failed menu install leaves both, so the transition is retried |
| Last profile removed | `TraySyncState::Absent` | `restore_idle` reinstalls the idle menu and drops the held line and memo together | a failed idle-menu install keeps both, so the teardown is retried |
| Scan stages nothing | clean tree, or every candidate still settling | no progress event, phase stays `Idle`, state is not promoted to `Syncing` | n/a |
| Scan stages something | a settled file | one `Scanning` event carrying `files_total` | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/tray.rs` -- `TrayState` (`sync_memo` field), `store_rendered_mode` (clears the sync rendering it displaces), new `SyncGlyphId` / `SyncMemo` / `SyncWrites` / `sync_writes`, `apply_sync_state` (diff + early return), new `push_sync_glyph`, `store_sync_item` → `store_sync_render`, `mod sync_tray_tests` (four new tests).
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `collect_stable_changes` (the `Scanning` publish moves from entry to the tail, guarded on `!staged.is_empty()`), plus the two doc comments on `next_scan_ms` and `scan_is_due` that named the old behaviour, plus one new test in `mod tests`.

## Tasks & Acceptance

**Execution:**
- [x] `tray.rs` -- Add `sync_memo: Option<SyncMemo>` to `TrayState` and `None` at both construction sites (`set_tray_presence`, `force_present`). -- The memo dies with the tray it describes, so a rebuilt menu-bar item is always painted once.
- [x] `tray.rs` -- Add `SyncGlyphId` (the asset address), `SyncMemo` (glyph + `Arc<str>` line), `SyncWrites` (`icon`/`text` + `is_empty`) and the pure `sync_writes`. -- The de-duplication becomes a value that can be asserted without a `TrayIcon`.
- [x] `tray.rs` -- `apply_sync_state`: compute `dwelled` and the glyph as before, then diff via `sync_writes` and return when nothing is owed; `set_text` only when the words changed; push the icon through `push_sync_glyph` only when the glyph changed; store the memo only for writes that landed. -- Removes the per-tick decode, `set_icon`, `set_icon_as_template` and `set_text`.
- [x] `tray.rs` -- `store_rendered_mode` also clears `sync_item` and `sync_memo`. -- A rendering swap un-installs the sync menu; keeping the held item would have the next sync tick `set_text` into a menu that is not installed.
- [x] `tray.rs` -- Extend `sync_tray_tests`: an unchanged tick writes nothing across five ticks; a changed glyph and changed words each write alone on the very next tick; `Transferring` writes on every frame; a tray with no memo, and a displaced menu, write again.
- [x] `engine.rs` -- Move the `Scanning` publish from the top of `collect_stable_changes` to its tail, behind `!staged.is_empty()`, carrying `files_total`. -- A scan that stages nothing no longer promotes the profile to `Syncing`.
- [x] `engine.rs` -- Add `a_scan_that_stages_nothing_reports_nothing`: two fruitless scans publish nothing and leave `phase == Idle` and the state un-promoted; the scan after the settle window publishes exactly one `Scanning`.

**Acceptance Criteria:**
- Given a tray showing a sync glyph, when a tick composes the same state and the same status line, then no `set_icon`, no `set_icon_as_template` and no `set_text` is performed.
- Given a memoised tray, when the glyph or the line genuinely changes, then the corresponding write happens on that very tick — no added latency, and the unchanged half is left alone.
- Given `TraySyncState::Transferring`, when the frame advances, then the icon is written on every tick and the ring keeps turning.
- Given the tray is rebuilt (presence toggled off→on) or its rendering displaced by recording, then the next sync tick writes again.
- Given a scan that stages nothing, when it completes, then no progress event was published and the profile was never promoted to `ProfileState::Syncing`.
- Given a scan that stages something, when it completes, then exactly one `Scanning` event is published before the caller commits or enqueues.

## Design Notes

**Why the glyph identity is an address.** The memo has to distinguish "the same picture" from "a different picture" on every tick. The state alone is not enough — `Transferring` equals `Transferring` while its four-frame ring advances, and a memo keyed on state would freeze the animation, which is the obvious way to break this change (hence a test for exactly that). Decoding the image, or hashing the PNG, costs more per tick than the repaint being avoided. `sync_glyph` already returns the one thing that fully determines the picture: which `&'static [u8]` asset. `SyncGlyphId::of` takes its address, so the identity is derived from the mapping rather than declared beside it and cannot drift from it. Two byte-identical assets that the linker chose to merge would compare equal, which is the right answer anyway — and it is why `Active` and `Transferring` at frame 0 share an identity: they are literally the same glyph, and there is nothing to repaint between them.

**Why the icon and the line are diffed apart.** They move on different cadences. A pending count changes while the glyph holds still, and the glyph escalates while the words stay put. Coupling them would reintroduce half the churn.

**Why the memo lives in `TrayState`.** Dropping the tray then forgets it for free, which is precisely the reset the story asks for: a rebuilt menu-bar item starts on the idle mark and must be painted once whatever the engine happens to be reporting. `Arc<str>` rather than `String` because the memo is cloned out of the slot on every tick, and the steady state — the one this exists to make free — must not allocate to discover it has nothing to do.

**Why only landed writes are memoised.** `render_recording` and `render_error` already work this way: a failed menu install leaves the flag unset so the next tick retries. If a failed `set_icon` were memoised, the tray would keep the wrong glyph until the state happened to change again.

**Why `store_rendered_mode` now clears the sync rendering.** It is called exactly on the three rendering transitions (recording install, error hold, idle restore), never on a steady tick — `apply_recording_state` only calls `restore_idle` when a recording line or the error hold is actually installed. Each of those swaps the whole menu, so the held sync `MenuItem` is no longer in any installed menu and the memo no longer describes what is on screen. Before this change the stale `sync_item` survived a recording, and the next sync tick would `set_text` into a detached item — the sync line simply never came back until the tray was rebuilt. The memo made that latent bug reachable (an unchanged tick would return early forever), so it is fixed here rather than left. Consequently the `Absent` branch no longer needs its own `store_sync_item(id, None)`: `restore_idle` clears both, and only once the idle menu is confirmed installed, so a failed teardown is retried instead of forgetting that the sync menu is still on screen.

**The engine half: defer, not retract.** The two options the epic left open were deferring the publish until there is something to report, or making the promotion conditional. Deferring is what shipped, for two reasons.

*Making the promotion conditional does not work as written.* `tray_state` treats a profile as active when `s.state.is_active() || s.phase.is_active()` (`progress.rs:318`), so leaving the `Scanning` phase in the snapshot while withholding the `Syncing` state would not hide the transient at all. Worse, `refresh_pending` only clears the phase when `snapshot.state == ProfileState::Syncing` (`engine.rs:721`) — withholding the promotion removes the only path that ever retires the phase, and the tray would then show activity forever. That would require editing `refresh_pending`, which stories 34.9/34.10 own.

*Announcing on entry and retracting at the end does not work either.* The window the tray can sample would be exactly as wide as it is today; the sampler is independent and the dwell amplifies whatever it catches. Only not publishing removes the race.

The trade this makes is deliberate and worth stating: a slow walk is now silent until it knows whether it has anything to say, where before it was busy from the first instant. Work is never left unannounced — a walk that finds something reports it one statement before the caller publishes `Committing` (`commit_local`) or enqueues the push (`scan_and_enqueue`) — but "I am looking" is no longer a claim the engine makes for its own sake. The published event carries `files_total` so the deferred report says how much was found rather than a bare "Scanning". The DW-116 scan pacing (`next_scan_ms`, `scan_is_due`) stands unchanged on the cost of the walk itself; its doc comments were amended because they cited the phase behaviour this change removes.

## Verification

**Deliberately not run by me:** no `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt` or `bun run check` was executed for this story. Six agents were editing this worktree concurrently and the parent agent runs the full suite once at the end. Everything below is reasoning from the source that was read, not observed output.

**What was read and reasoned through:**
- `tray.rs` in full for the sync and recording renderers, plus every `TrayState` construction site (`set_tray_presence:356`, `force_present:523`) and every `store_rendered_mode` caller (`render_recording`, `render_error`, `restore_idle`), to confirm the memo is reset on rebuild and on a rendering swap and *not* on a steady tick — `apply_recording_state`'s `RestoreIdle` arm calls `restore_idle` only when `status_item.is_some() || error_rendered`.
- `lib.rs:213-233` to confirm the tick's contract: 1 Hz, `frame` free-running and wrapping, sync applied straight after recording.
- `progress.rs:303-339` (`tray_state`) and `:359-405` (`status_line`), plus `engine.rs` `publish` / `refresh_pending` / `collect_stable_changes` / `commit_local` / `scan_and_enqueue`, to establish that the conditional-promotion option is unsound and that deferring leaves no phase stranded on the success path.
- `grep` for `SyncPhase::Scanning` across `src-tauri`: the only publisher was the line that moved; no test and no other crate asserts on it. `keeper/src/sync_ipc.rs:330` maps the variant for the UI and is unaffected.
- The existing engine test helpers (`engine`, `profile`, `adoptable`, `commit_after_settling`) and `a_file_inside_its_settle_window_is_pending_as_settling`, to model the two-pass settle idiom the new test uses.

**Tests added (four in `tray.rs::sync_tray_tests`, one in `engine.rs::tests`):**
- `an_unchanged_tick_writes_nothing_at_all` — five identical ticks after the first owe nothing. Fails if the diff is dropped.
- `a_change_reaches_the_tray_on_the_very_next_tick` — a changed glyph writes the icon alone; changed words write the line alone. Fails if the two are coupled or if a change is deferred.
- `a_transfer_still_animates_through_the_memo` — eight consecutive frames each demand an icon write. Fails if the identity is keyed on `TraySyncState` instead of the asset.
- `a_rebuilt_tray_is_painted_again` — no memo means both writes; a displaced menu means the line is reinstalled even when the words match.
- `engine.rs::a_scan_that_stages_nothing_reports_nothing` — a clean tree and a still-settling file publish nothing and leave the profile un-promoted; after the settle window the same scan publishes exactly one `Scanning`. Fails if the publish returns to the top of the function.

**Commands for the caller to run:** `cargo test -p keeper-sync -p keeper`, `cargo clippy --all-targets`, `cargo fmt --check`.

**Manual check still owed (from the epic):** on hesperia, watch the menu bar for 60 s with folders configured and idle — the glyph must not change at all — then touch a file in a synced folder and confirm the busy glyph appears within a tick and an LFS transfer still animates.
