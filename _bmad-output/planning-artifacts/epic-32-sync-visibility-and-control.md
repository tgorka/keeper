# Epic 32 — Folder sync you can see and steer

status: draft
created: 2026-07-27
altitude: epic
parent: AD-40..AD-53 (Phase 4 folder sync spine)

## Why this epic exists

Folder sync works and is invisible. It lives in one Settings section, the tray says nothing
until a profile exists, and the pane can name a profile's state but never a *file*. A user
cannot answer the three questions every sync tool must answer:

1. Is it working right now, and on what?
2. What has it done to my files lately, and what is it about to do?
3. What went wrong, and what do I do about it?

Investigation (2026-07-27) found the data to answer them is mostly **produced and thrown away**,
not missing:

- `Engine::progress` (`engine.rs:1715-1719`) sets only `phase`. `files_done`, `bytes_done`,
  `current` are permanently `0`/`None`, so every counter the UI could show is a lie by omission.
  `BasicTransfer::with_sink` (`lfs/basic.rs:202`) exists and is never called; the fetch progress
  callback is a no-op closure (`engine.rs:816`).
- `StagedChange` (`git/commit.rs:50`) carries the exact added/modified/deleted paths and dies in
  `commit_local`, which returns `staged.len()` (`engine.rs:998-1013`).
- `converge_with_conflict_copies` returns the conflict-copy paths and `do_pull` keeps only the
  count (`engine.rs:927-933`). `SyncOutcome.conflicts` is never assigned, so the app's conflict
  list is always empty.
- Parked journal units carry `last_error` and are excluded from every query (`db.rs:452`), so a
  permanently-failed unit has no surface at all.

## Decisions

**AD-S1 — Sync is a primary view, not a Settings section.**
Binds: navigation. Prevents: sync detail competing with unrelated settings for one dialog.
Rule: a `sync` entry in `PrimaryView`, capability-gated exactly like Recording, desktop-only
(the phone shell renders no non-chat panes). Settings keeps profile *configuration*; the view owns
*activity, state and problems*.

**AD-S2 — Activity is durable, bounded, and paths-only.**
Binds: what "recently synced" means. Prevents: reconstructing history by parsing `git log`, and
unbounded growth. Rule: an `activity` table written where the truth already exists — at commit
(from `StagedChange`) and at conflict-copy time — holding `(profile_id, ts_ms, kind, path)` and
nothing else. Never file contents. Trimmed to the newest `ACTIVITY_CAP` rows per profile.

**AD-S3 — Pending is computed, never stored.**
Binds: what "waiting to sync" means. Prevents: a second source of truth that drifts from the
worktree. Rule: answered at query time from `RepoStatus` plus the quiescence gate's own
`file_state`, so it cannot disagree with what the next tick will do.

**AD-S4 — The conflict *policy* is not configurable.**
Binds: divergence handling. Prevents: the setting that loses data. Rule: remote keeps the
canonical path, the local version survives beside it as `.sync-conflict-<stamp>-<device>.<ext>`,
always, for everyone. Configurability is offered for *visibility* (list them, open them, dismiss
them), never for *resolution*. A "prefer mine / prefer theirs" switch is exactly how a sync tool
silently destroys work, and keeper's whole posture is that it never does.

**AD-S5 — Problems are a list, not a single string.**
Binds: failure surfacing. Prevents: one `warning: Option<String>` collapsing seven distinct
conditions and being cleared wholesale by one success. Rule: the view lists parked journal units
(with their `last_error` and a retry), the live warning/error, and conflict entries. Parked work
becomes visible and actionable.

**AD-S6 — Progress is measured, and the tray distinguishes transferring from working.**
Binds: tray + progress semantics. Prevents: a spinner that means nothing. Rule: the LFS transfer
sink and the fetch progress callback feed real byte counters into `SyncProgress`; the tray gains a
`Transferring` state that outranks `Active`. Recording still outranks all sync states and sync
still never forces tray presence (an unattended background folder must not summon a menu-bar item).

**AD-S7 — Credentials go to the Keychain through the app, never into a config file.**
Binds: auth. Prevents: a token in `config.toml` or `sync.db`. Rule: a `sync_set_credential`
command writes through the existing `SyncPlatform::secret_set` (already implemented, never called)
under `sync/<id>/credential`. The form takes a token, stores it, and never reads it back.

**AD-S8 — A dead knob is removed, not displayed.**
Binds: the config surface. Prevents: a setting that silently does nothing. Rule: `pollIntervalMs`
is deleted from `SyncProfile` and the daemon settings rather than exposed; the cadence is
`TICK_MS`. `authorOverride` — which does work — is promoted into the IPC types and the form.

**AD-S9 — Development mode is a documented, honest switch.**
Binds: diagnostics. Prevents: a debug toggle whose effect nobody can state. Rule: the existing
`debug_mode` switch also unlocks a Logs surface in the Sync view (tail of the app log, warnings
and errors first) and a "copy diagnostics" action. Engine warnings already reach
`~/Library/Logs/keeper/keeper.log`; the switch controls the file leg and the viewer, not whether
problems are recorded.

## Stories

**32.1 — Activity and pending, in the engine.**
`activity` table + `Engine::activity(profile, limit)` and `Engine::pending(profile)`. Commit path
records added/modified/deleted; conflict-copy path records conflicts. Trim to cap.
AC: after a sync that changes three files, `activity` returns exactly those three with kinds; after
touching a file inside its settle window, `pending` names it; the table never exceeds the cap.

**32.2 — Problems, in the engine.**
`Engine::problems(profile)` returning parked units (id, kind, attempts, last_error), the live
warning/error, and unresolved conflicts. `Engine::retry_parked(id)` returns a parked unit to pending.
AC: a parked unit appears with its error and disappears after a retry; a cleared warning leaves the
list; conflicts persist across a restart.

**32.3 — Real progress.**
Wire `BasicTransfer::with_sink` and the fetch progress callback into `SyncProgress`; set
`files_total`/`files_done` from the staged set. `SyncOutcome.conflicts`/`bytes` actually assigned.
AC: a transfer of a known-size object reports rising `bytes_done` with `bytes_total` set;
`syncFolderNow` returns the conflict paths it created.

**32.4 — IPC surface.**
`sync_activity`, `sync_pending`, `sync_problems`, `sync_retry_parked`, `sync_set_credential`,
`sync_clear_credential`, `sync_open_path`. `SyncProfileVm`/`Req` gain `authorOverride`; the
round-trip that silently resets `enabled` and `authorOverride` on save is fixed.
AC: saving a paused profile leaves it paused; a token set from the app authenticates a fetch.

**32.5 — The Sync view.**
Drawer entry + pane: profile cards with live state and progress, an Activity list (recent files,
newest first, kind icon, relative time), a Pending list, and a Problems section with conflicts and
parked work. Empty states that say what they mean.
AC: adding a folder shows it in the view without opening Settings; a file changed on disk appears
in Pending then moves to Activity after it syncs.

**32.6 — Tray and native menu.**
`Transferring` glyph + status line; a Sync section in the native menu whose items reflect live state.
AC: the tray shows transferring during an LFS transfer and returns to armed after; a live recording
still owns the tray throughout.

**32.7 — Config file and advanced knobs.**
Advanced disclosure exposes `settleMs`, `excludes`, `subpaths`, `tags`, `lane`, `authorOverride`.
A "Reload configuration" action re-reads the profile store and restarts the supervisor in place.
AC: an advanced value set in the form survives a save/reload round trip and reaches a commit trailer.

**32.8 — Logs and development mode.**
Log surface (tail, level filter, copy) gated on the debug switch; every engine `warn` reaches it.
AC: a forced failure appears in the log surface with its cause.

## Deferred

- The filesystem watcher (`watch.rs` has no production constructor; both hosts poll at 1 Hz) — DW-114.
- Per-file digests, which would make `verify` a real content check — DW-115's underlying question.
- `Keeper-Source` threading — DW-117.
- Push credentials: `GitCli::push` takes no credential and relies on the ambient helper or URL
  userinfo, so AD-S7 improves fetch/LFS/PR auth only.
