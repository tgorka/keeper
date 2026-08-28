# keeper surfaces a virtual-file feature must appear on
_agent: G2Surfaces · accessed: 2026-08-22_
_Repo grounding only. Every claim carries `path:line`. Read-only pass; nothing built, nothing run._

---

## 0. Verdict (read this first)

- **keeper already has most of the primitives.** A pointer-only checkout mode, an atomic pointer→bytes materializer, a durable per-path materialization ledger *with timestamps*, and a safe object-release path all exist and ship today. A virtual-file feature is mostly a matter of **wiring existing parts to a new policy + new surfaces**, not new machinery.
- **`LfsMode::PointerOnly` is the existing "do not download" switch**, but it is **per profile**, not per pattern (`src-tauri/crates/keeper-sync/src/profile/mod.rs:81`, `:743-747`). The product ask (a gitignore-like pattern file) is the missing selector; `lfs_never: Vec<String>` (`profile/mod.rs:813`) is the existing precedent for a per-profile glob list, and `subpaths[]` is the existing precedent for a *path filter that gates transfers, not just checkout* (`engine.rs:4984-4998`).
- **The 24h lazy release has an obvious host and an obvious clock.** `db::remember_materialized` writes `(profile_id, path, at_ms)` on every materialization (`db.rs:141-147`, `:324-338`) — `at_ms` is exactly "when this was last materialized". The only periodic engine in the product is the 1 Hz supervisor tick (`engine.rs:338`, `:1141-1156`, `:1185-1207`); the existing housekeeping hook is `mark_synced` → `prune_lfs_store` (`engine.rs:3052-3066`). A nightly sweep belongs beside that, not in a new scheduler.
- **There is no cron, no nightly timer, and no `.timer` unit anywhere in the repo.** `keeper-syncd` explicitly refuses timer-driven self-install (`commands.rs:275-283`, `docs/sync.md:885-890`). Any sweep must be tick-derived or an explicit `keeper-syncd` verb.
- **Two rendering paths already exist for "this file is not fully here"**: the Pending list (`PendingReason::Incoming { replacing }`, `engine.rs:5843-5862`; wire strings `sync_ipc.rs:1194-1212`; UI `sync-pane.tsx:461-469`) and the Files-tree per-row mark (`FilesSyncStatusVm`, `keeper-core/src/vm.rs:3812-3828`; UI `sync-status-mark.tsx:40-93`). A `virtual` / `materialized` state extends one enum in each.
- **OS-level placeholder presentation is the unbudgeted part.** keeper *reads* macOS `SF_DATALESS` (`stability.rs:65`, `:168-181`) but has never *produced* a placeholder. Producing one needs FFI, and the workspace denies `unsafe_code` outside the shell crate — with exactly one audited precedent (`docs/project-context.md:55-60`, `docs/constraints-and-limitations.md:83-92`, `keeper/src/ipc.rs:949`). `keeper-syncd` is Linux-only and unix-only by design (`keeper-syncd/src/main.rs:11-13`), so a macOS File-Provider extension cannot live in the daemon at all — **the primary environment (the server) can never get Finder-style semantics; it gets the pointer file, which is already what `PointerOnly` produces.**
- **A materialization is a git-index event, not just a file write.** Writing real bytes over a pointer invalidates the index stat and makes `git status` report the whole folder dirty; `git::repo::repair_index_stat` / `refresh_index_stat` exist precisely for this (`engine.rs:5061-5065`, `:5382-5392`, `git/repo.rs:1880-1912`). Any release path (bytes → pointer) carries the same obligation in reverse.

---

## 1. `keeper-syncd` — binary layout, verbs, config, tick, install, and existing periodic work

### 1.1 Binary layout

Four modules, no sync logic of its own:

- `src-tauri/crates/keeper-syncd/src/main.rs:21-24` — `mod commands; mod config; mod platform; mod update;`
- `main.rs:36-37` — `#[tokio::main] async fn main() -> ExitCode`
- `main.rs:44-53` — builds `LinuxPlatform`, the `SyncPlatform` impl
- `main.rs:55-56` — config path defaults to `platform.config_path()`, overridable by `--config` / `KEEPER_SYNCD_CONFIG`
- `main.rs:58-70` — startup order is load-bearing: **config is read before logging is initialised**, because `[daemon] logLevel` is an input to the logger. That is also why `init`, `doctor` and `logs` still work on a box whose config is broken.
- `main.rs:11-13` — **"Linux-first, unix-only."** *"Secret files are enforced by mode bits and `doctor` reads `/proc`, so this deliberately does not pretend to build for a platform that cannot express either."*
- `main.rs:4-9` — *"no forked policy here: every verb delegates to `keeper_sync::engine::Engine`, and this binary supplies only the three things a headless box needs — an XDG-shaped `SyncPlatform`, a configuration format, and a process lifecycle."*

Reinforced at `commands.rs:5-9`: *"There is deliberately **no sync logic here**… A second implementation of any of that on the CLI side would be exactly the divergence AD-52 exists to prevent."*

> **Implication for virtual files:** the materialize/release decision must live in `keeper-sync`; `keeper-syncd` may only expose a verb over it.

### 1.2 Subcommands (the place a new verb lands)

`Command` enum, `commands.rs:205-300`:

| Verb | Line | Notes |
|---|---|---|
| `Init { force }` | `:207-211` | writes a documented starter config |
| `Add(AddArgs)` | `:213` | carries every profile field the CLI can set |
| `List` | `:215` | |
| `Status { profile }` | `:217-220` | |
| `Sync { profile, once }` | `:222-228` | `--once` is the cron entry point |
| `Watch` | `:230` | **the systemd entry point** |
| `Pause` / `Resume` | `:232-241` | |
| `Verify { profile, repair, remote }` | `:243-264` | `--remote` asks the server whether it holds every object the pointers name |
| `Doctor` | `:266` | |
| `Logs { lines }` | `:268-273` | |
| `Update { check }` | `:275-283` | **"Never runs on a timer."** |
| `Lfs { direction }` | `:285-298` | hidden; the git clean/smudge filter |

Dispatch: `commands.rs:500-560`; `Command::Watch => run_supervisor(&printer, engine)` at `:547-550`.

Global flags: `--config/-c` (also `KEEPER_SYNCD_CONFIG`), `--json`, `--verbose/-v` — `commands.rs:180-199`.

Output contract: every command produces **both** a human rendering and a `--json` document, and both come out of one `Printer`, the only stdout writer in the crate (`commands.rs:9-15`).

Exit codes — **exhaustive by construction, no `_` arm**, because `Restart=on-failure` reads them (`commands.rs:44-54`, `:118-170`): `0` ok, `1` operational, `2` config, `3` prerequisite. `SyncError::Cancelled` and `SyncError::Busy` deliberately map to `0` (`commands.rs:133-142`).

> A `keeper-syncd materialize <profile> <path>` / `release <path>` / `sweep` trio would be added to this enum near `commands.rs:284` and dispatched near `:546`, each delegating to a new `Engine` method. A new `SyncError` variant forces a compile error in `sync_exit_code` (`commands.rs:120-124`) — that is the intended design, not an obstacle.

### 1.3 Config file

`src-tauri/crates/keeper-syncd/src/config.rs`:

- `:1-20` — TOML, **1:1 onto `SyncProfile`**, with the accepted key set *derived from the type at runtime* so it cannot drift when a field is added; **unknown keys are errors**; nothing is ever partially applied.
- `:44-48` — `DaemonConfig { daemon: DaemonSettings, profiles: Vec<SyncProfile> }`
- `:50-77` — `[daemon]`: `pollIntervalMs`, `logLevel`, `gitPath`; `#[serde(rename_all = "camelCase", deny_unknown_fields, default)]` with a snake_case `alias` on each key so neither spelling is silently ignored.
- `:34-39` — `LOG_LEVELS` allow-list; `MIN_POLL_INTERVAL_MS: u64 = 1_000`.
- `:92-104` — `RawDocument` also `deny_unknown_fields`, so a misspelled `[[profiles]]` (plural) section is caught rather than leaving the daemon with zero profiles.
- `:107-123` — `load()`; a missing file tells the operator to run `keeper-syncd init`, and parse errors are prefixed with the file path because `--config` and the env var are both in play.

> A pattern file selecting virtual content has two candidate homes: (a) a new `SyncProfile` field — which **auto-propagates** into this TOML, the app's JSON profile, and the Tauri VM by construction; or (b) an in-repo dotfile like `.gitattributes`. Precedent supports both: (a) for configuration, (b) for repo-scoped rules — keeper already writes `.gitattributes` rules itself (`engine.rs:1923-1960`).

### 1.4 How the tick loop is scheduled

The daemon owns process lifecycle only; the loop belongs to the engine.

- `commands.rs:967-969` — `run_supervisor` creates a `tokio::sync::watch` shutdown channel and `tokio::spawn`s `engine.run(shutdown_rx)`.
- `commands.rs:971-975` — installs a SIGTERM handler; `:1008` sends `true`; `:1010` a bounded graceful finalize under `GRACEFUL_FINALIZE` (`commands.rs:56-62`, 10 s, *"identical to the app's `QUIT_FINALIZE_TIMEOUT`, because AD-52 requires the daemon's SIGTERM path to be the app's quit path: an in-flight push aborts **resumably**"*), then `ABORT_JOIN` 2 s (`commands.rs:64-69`) because `JoinHandle::abort` only *schedules* cancellation.
- Engine loop: `engine.rs:1141-1146` — `tokio::time::interval(Duration::from_millis(TICK_MS))` with `MissedTickBehavior::Delay` (*"after a long stall we want one catch-up tick, not a backlog of them fired back to back at a git server"*). `TICK_MS = 1_000` (`engine.rs:337-338`).
- `engine.rs:1149-1156` — a tick failure is logged, never fatal: *"one bad profile must not stop every other one."*
- `engine.rs:1185-1207` — `tick()`: drain finished assertions (before the enabled filter), `retain_watchers(&profiles)`, then `tick_profile` per enabled profile.
- `engine.rs:1209-1236` — `tick_profile()`: **volume gate first** (AD-48: *"a detached drive is indistinguishable from a mass deletion once you start walking the tree, so we never start walking"*), then the one-operation-per-profile reservation, then `ensure_watcher`, then `fold_watch_events`.
- **Scanning is paced independently of the tick**: `scan_is_due` (`engine.rs:1238-1267`) uses `profile.effective_poll_interval_ms()` and a `next_scan_ms` map (`engine.rs:523-533`). *"Queued work is drained every tick; discovering NEW work is paced, because a scan is a full re-stat of the tree, on a pendrive or over a network mount"* (`engine.rs:1241-1247`).
- Three reasons a walk happens: watcher wake, an elapsed settle window, or the paced backstop (`engine.rs:1269-1292`).

**The same loop runs in the app**: `keeper/src/sync.rs:406-425` — `start_supervisor` spawns `engine.run(stop_rx)` on the Tauri runtime; `:434-437` `stop_supervisor` sends the stop. **This is the single most important fact for a 24h release sweep: anything hung off the tick automatically exists on both hosts, with no second scheduler and no second policy.**

### 1.5 Install / keeping current (docs §12)

`docs/sync.md:822-906`:

- `:826-840` — the documented verb list.
- `:842-843` — XDG paths: `$XDG_CONFIG_HOME/keeper-sync/config.toml`, `$XDG_DATA_HOME/keeper-sync/sync.db`, `$XDG_STATE_HOME/keeper-sync/`.
- `:845-849` — TOML maps one-to-one onto a profile *"so a profile moves between the app and the daemon by copying a table. **Unknown keys are an error**"*.
- `:851-854` — secrets from an env var or a per-key file; a group- or world-readable secret file is **refused**, not warned about.
- `:856-858` — install the **user** service from `packaging/keeper-syncd.service`; SIGTERM performs a bounded graceful finalize.
- `:860-870` — prebuilt binary per target plus `.sha256`; `curl` + `sha256sum -c` + `install -m 0755`.
- `:872-878` — `keeper-syncd update --check` / `keeper-syncd update`.
- `:880-883` — `doctor` reports an available version as a **warning that never fails the run**.
- `:885-890` — **"It never installs by itself.** The daemon holds a durable journal and can be mid-push at any moment; swapping its binary on a timer is how a routine release becomes a corrupted transfer. The install is also not a restart: the file is replaced through a rename, and the running process keeps its old inode until you restart it."
- `:893-899` — integrity is a **checksum, not a signature**; the desktop app's updater verifies a minisign signature and is the stronger of the two.
- `:901-903` — only `linux-x86_64` and `macos-aarch64` are published today.
- `:1057-1059` (§18 Deliberate limitations) — restates the checksum-not-signature gap.

The unit itself, `src-tauri/crates/keeper-syncd/packaging/keeper-syncd.service`:

- header `:1-19` — **user unit, not a system one**, *"this daemon synchronizes the user's own files with the user's own git credentials. Running it as root would mean a service account writing into someone's home and holding their token."* `loginctl enable-linger $USER` to survive logout. Credentials never in `Environment=` (visible to `systemctl show`).
- `:24-28` — `After=network-online.target`, ordering only, deliberately **not** `Requires=`, because AD-49 makes offline the normal case.
- `:31-33` — `Type=simple`, `WorkingDirectory=%h`, `ExecStart=%h/.local/bin/keeper-syncd watch`
- `:35-41` — `Restart=on-failure`, `RestartSec=10`, `RestartPreventExitStatus=2 3` (*"Neither is fixed by trying again"*)
- `:43-50` — `KillSignal=SIGTERM`, `TimeoutStopSec=20`; must exceed the 10 s finalize plus the 2 s abort-join *"or systemd's SIGKILL would arrive first and defeat the whole mechanism."*
- `:52-73` — hardening deliberately minimal. `NoNewPrivileges=yes`, `PrivateTmp=yes`. **Not set on purpose:** `ProtectHome=` (*"it would hide the very directories this daemon synchronizes"*), `ProtectSystem=strict`/`ReadOnlyPaths=`, `PrivateNetwork=`.

> There is **no `.timer` unit** in `packaging/`. A nightly sweep as a systemd timer would be the product's first, and would sit against the "never on a timer" posture written at `docs/sync.md:885`. Tick-derived is the idiomatic choice; an explicit `keeper-syncd sweep` verb is the escape hatch for operators who want cron.

### 1.6 Existing periodic maintenance — the natural host for a release sweep

There is exactly **one** piece of recurring housekeeping, and it is not on a clock — it is on the *success edge*:

- `engine.rs:3020-3051` — doc for `mark_synced`: *"The prune lives HERE, and not at the end of `sync_once`, because `sync_once` is only one of the three ways a pass finishes. The ordinary watch-driven flow commits and enqueues a `Push`, which the journal drains through `execute` — a path that never reaches `sync_once`'s tail. Hooking the tail meant prune ran only on an explicit 'sync now', which is exactly the manual step it exists to avoid. **This function is the single point every path agrees means 'it worked'.**"* And `:3050-3051`: *"Never fatal. Reclaiming space is housekeeping, and a pass that did everything asked of it must not report failure because a delete did not."*
- `engine.rs:3052-3066` — the body: stamps `last_sync_ms`, then `if profile.lfs_prune_local { self.prune_lfs_store(profile) }`, warning on error.
- `engine.rs:5294-5316` — `prune_lfs_store`: open repo → `git::repo::tracked_paths` → `db::referenced_oids` (what the journal still owes) → `lfs::prune::plan(&repo, &local_path, &store, &tracked, &owed)` → `lfs::prune::release(&store, &releasable)`, logging `objects` and `bytes` reclaimed.
- Policy, `docs/sync.md:328-368`: on the originating machine every LFS file exists **twice** (measured: 215 GB worktree + 215 GB store on one 920 GB drive, `:330-336`). `lfsPruneLocal` is **on by default** (`:338-339`, and `profile/mod.rs:811-812` `#[serde(default = "default_true")]`). An object is released only when **all** hold (`:340-352`):
  1. the journal references no transfer for it — keeper's own durable record, not an inference from ref positions;
  2. **the worktree still holds the real content**, at the recorded length and not pointer text — *"This is what makes the release cheap to undo — the object is not the only local copy, the **file** is — and it is the condition `git lfs prune` cannot express"*;
  3. nothing else is running — after the upload queue has drained to quiescence and after the push, never between them.
  The honest trade at `:357-360`: *"the drive stops being self-sufficient. Every file is still there, but restoring one the worktree later loses now needs the network."*
  A carried-over migration marker exists for stores written before the default flipped (`db.rs:161-166`, `PRUNE_DEFAULT_MARKER`).

Other recurring behaviour, for completeness — none of it a maintenance timer:
- watcher re-arm backoff, `WATCH_REARM_INTERVAL_MS = 60_000` (`engine.rs:349-356`)
- watcher debounce plus *"the 15-minute sweep that covers a dropped event queue"* (`engine.rs:1351-1356`); `force_poll` is deliberately off because `notify::PollWatcher` re-stats the whole tree every 30 s, *slower* than keeper's own paced scan
- per-profile paced scan via `effective_poll_interval_ms` (`engine.rs:1264`)
- `release_satisfied_waits` on every drain (`engine.rs:2604-2641`) — re-reads conditions deferred units wait on; costs *"one indexed `COUNT(*)` per tick"*

**Design conclusion for §1:** a 24h lazy release should be a `release_stale_materializations(profile, now_ms)` invoked from `mark_synced` (or from `tick_profile` behind its own `next_release_ms` pacer mirroring `next_scan_ms`, `engine.rs:533`), reading `materialized.at_ms` and inheriting prune's condition-3 discipline. It gets the app host for free via `keeper/src/sync.rs:406`.

---

## 2. Tauri IPC — commands, channels, VMs, and where `materialize`/`release` go

### 2.1 Where sync commands live

`src-tauri/crates/keeper/src/sync_ipc.rs`, module doc `:1-11`: *"View models live here rather than in `keeper-core::vm` because sync is not part of the Matrix hexagon and `keeper-core` must never learn about it (AD-40) — the `lifecycle::LifecyclePhase` precedent for a shell-owned DTO. They still follow every `vm.rs` convention: serde `camelCase`, `#[ts(export)]` into `src/lib/ipc/gen/`, timestamps as `i64` ms… Every command is a thin projection over `keeper_sync::Engine`. Policy stays in the engine; this layer only translates types and maps errors into the one `IpcError` envelope the frontend already understands."*

The full registered sync surface, `keeper/src/lib.rs:901-947` (desktop-only; `lib.rs:676-692` explains there is exactly **one** `invoke_handler` registration — Tauri's `invoke_handler` *assigns* rather than adds, so a second call would silently discard the first, and the test `exactly_one_invoke_handler_is_registered` fails the build if one is added back):

```
sync_profiles, sync_statuses, sync_profile_save, sync_profile_remove,
sync_profile_set_enabled, sync_folder_now, sync_verify, sync_rescan, sync_open_path,
sync_browse, sync_open_entry, sync_read_text, sync_read_document, sync_export_entry,
sync_write_entry, sync_delete_plan, sync_delete_entries, sync_create_entry,
sync_read_frontmatter, sync_write_frontmatter,
sync_subscribe_progress, sync_unsubscribe_progress,
sync_activity, sync_pending, sync_problems, sync_retry_parked,
sync_set_credential, sync_get_credential, sync_clear_credential,
sync_device, sync_device_set_label
```

Command definitions of interest: `sync_profiles` `:966`, `sync_statuses` `:976`, `sync_folder_now` `:1130`, `sync_activity` `:1153`, `sync_pending` `:1185`, `sync_problems` `:1228`, `sync_retry_parked` `:1268`, `sync_verify` `:1469`, `sync_rescan` `:1505`, `sync_open_path` `:1578`, `sync_browse` `:1846`.

A note worth carrying into any security review of a new command — `sync_ipc.rs:1323-1335`: *"**Every `#[tauri::command]` in the invoke handler is equally reachable.** `capabilities/*.json` gates plugin permissions, not these functions."*

### 2.2 Naming and shape conventions (a new pair must obey all of these)

1. **Verb naming**: `sync_<noun>` or `sync_<noun>_<verb>` — `sync_profile_save`, `sync_device_set_label`, `sync_folder_now`, `sync_retry_parked`. → `sync_materialize_entry` / `sync_release_entry` fit the existing `sync_*_entry` family (`sync_open_entry`, `sync_export_entry`, `sync_write_entry`, `sync_create_entry`).
2. **Argument order**: `state: tauri::State<'_, AppState>` first, then `id: String` (the profile id), then a repository-relative `subpath: String`. Never an absolute path — `sync_ipc.rs:1617-1619`: *"never an absolute one: absolute paths leak home-directory names into logs and screenshots."*
3. **Engine access**: through `engine_of(&state)` (`sync_ipc.rs:960-962`), which delegates to `crate::sync::engine(Arc::clone(&state.platform))` (`keeper/src/sync.rs:325`).
4. **Errors**: map through `sync_ipc_error(&err)` (`sync_ipc.rs:672`), projecting `SyncError` onto `IpcErrorCode`; each command's doc names the codes it can reject with (e.g. `:1415`, `:1428`).
5. **VMs**: `#[derive(Debug, Clone, Serialize, Deserialize, TS)]` + `#[ts(export)]` + `#[serde(rename_all = "camelCase")]`, generated into `src/lib/ipc/gen/`; `u64` annotated `#[ts(type = "number")]`, optional numbers `#[ts(type = "number | null")]`, timestamps `i64` ms (`sync_ipc.rs:6-7`, `:71-77`, `:215-219`).
6. **Wire enums are written out by hand**, never derived from serde, *"so the wire contract is visible at the boundary and cannot change under a rename"* — `activity_kind_str` `:47-56`, `delivery_str` `:60-68`, `lfs_str` `:625-629`, `state_str`/`phase_str` (referenced `:49-50`), and the reverse parse `:764-770`.
7. **The "effective value" rule (AD-34-8)**: a VM must be able to show the number actually **in force**, not only the pinned one — `settle_ms` / `effective_settle_ms` (`:89-102`), `poll_interval_ms` / `effective_poll_interval_ms` (`:104-113`), `recordings_subfolder` resolved in Rust *"so `DEFAULT_RECORDINGS_SUBFOLDER` is spelled once in the whole product"* (`:143-157`). **A virtual-file threshold or pattern set must follow this.**
8. **iOS parity**: sync is desktop-only and lives outside the flat `generate_handler!` literal via the `keeper_with_commands!` macro (`lib.rs:690-694`); other modules keep `#[cfg(not(desktop))]` twins so the handler list is identical per target (`notes_ipc.rs:26-28`, `sessions_ipc.rs:12-14`, `ipc.rs:1487-1491`).

### 2.3 The `Channel<T>` streams

The streaming convention, `keeper-sync/src/progress.rs:1-11`: *"Two shapes, deliberately… a **stream** for a subscribed UI (the `export_start` precedent) and a **polled snapshot** for the ~1 Hz tray tick, which must render correctly when no webview is subscribed at all… The Tauri shell wraps `Channel::send(..).is_ok()`; `keeper-syncd` writes log lines; tests push into a `Vec`."*

- `pub type ProgressSink = Box<dyn Fn(SyncProgress) -> bool + Send + Sync>` — `progress.rs:23`. **Returning `false` means "stop producing".**
- `sync_subscribe_progress(state, channel: tauri::ipc::Channel<SyncProgressVm>) -> Result<u64, IpcError>` — `sync_ipc.rs:1636-1657`. Body: `engine.subscribe(Box::new(move |event| { let vm = SyncProgressVm {…}; channel.send(vm).is_ok() }))`. Doc `:1630-1634`: *"Returns a subscription id. The engine drops a sink as soon as it returns `false`, which `Channel::send` does once the webview is gone — so a closed window unsubscribes itself and a reload cannot accumulate dead sinks."*
- `sync_unsubscribe_progress(state, id: u64)` — `sync_ipc.rs:1662-1668`; *"Unsubscribing an unknown id is a no-op, so a double-unsubscribe from a racing unmount is not an error."*
- Same pattern across the shell: `ipc.rs:1832` (bridge login), `:2036` (bridge health), `:2578` + `:2618` + `:2712` (**export — the job-scoped precedent, every event carrying its own `export_id`**), `notes_ipc.rs:2673` (note body), `:4276` (note changes), `:4422` (index progress), `sessions_ipc.rs:4600` (session search).

### 2.4 View-model types the frontend consumes

| VM | Line | Relevance to virtual files |
|---|---|---|
| `SyncProfileVm` | `sync_ipc.rs:75-170` (+ `From<&SyncProfile>` `:172-213`) | carries `excludes: Vec<String>` `:83`, `lfs_mode: String` `:86`, `lfs_threshold_bytes` `:88`. A virtual-file pattern list lands here. |
| `SyncStatusVm` | `:218-268` (+ `From` `:270-292`) | `pending`, `settling` `:246-252`, `queued_files`/`queued_bytes` `:254-259`, `warning`/`error`, `needs_attention`; `line` composed in Rust *"so the tray and the window can never word it differently"* `:232-236` |
| `SyncOutcomeVm` | `:299` | what one manual sync did; both raw counts and the composed sentence |
| `SyncActivityVm` | `:328` | Activity rows |
| **`SyncPendingVm`** | **`:374-397`** | `{ path, reason, sinceMs, sizeBytes }`; reason vocabulary `settling \| untracked \| modified \| added \| deleted \| incoming \| incomingUpdate` `:376-378` |
| `SyncParkedVm` | `:400` | |
| `SyncUnspellableVm` | `:426-434` | names that are not valid text |
| `SyncProblemsVm` | `:438-445` | `{warning, error, parked, conflicts, unspellable}` |
| `SyncDeviceVm` | `:1390` | |
| **`SyncProgressVm`** | **`:1604-1628`** | `{profileId, profileName, phase, filesDone, filesTotal, bytesDone, bytesTotal, current, fraction, bytesPerSecond}` |
| `FilesEntryVm` / `FilesEntrySyncVm` / `FilesSyncStatusVm` | `keeper-core/src/vm.rs:4025`, `:3839`, `:3812` | the Files-tree row and its sync mark |

**Where the new pair goes:** `src-tauri/crates/keeper/src/sync_ipc.rs`, registered in the sync block of `src-tauri/crates/keeper/src/lib.rs:901-947` — adjacent to `sync_open_entry` (`:914`) reads best: same noun, same containment rules. The backing engine methods belong beside `materialize_pending` (`engine.rs:4998`).

---

## 3. Frontend — where a per-file virtual/materialized badge + action attaches

### 3.1 The Sync pane and its Pending list

`src/components/layout/sync-pane.tsx` (2044 lines). Module doc `:1-58` — the pane answers three questions with one card per configured folder: header (state, the Rust-composed line, progress, row actions), then **Activity**, **Pending**, and a **Problems** section that exists only while something is wrong. Two carried-over rules: `SyncStatusVm.line` is rendered **verbatim** (`:41-45`) and *"Nothing here promises a finish time for a settling file. The quiet window restarts on every write, so the pane reports how long keeper has been waiting and stops there"* (`:45-47`).

- Section titles `:180-182` — `SYNC_ACTIVITY_TITLE`, `SYNC_PENDING_TITLE`, `SYNC_PROBLEMS_TITLE`.
- Copy `:192-208` — `SYNC_PENDING_EMPTY_SENTENCE` ("Nothing is waiting to sync."), `SYNC_SETTLING_SENTENCE`, `SYNC_SETTLING_NOTE`.
- **Direction words** `:439-440` — `SYNC_PENDING_INBOUND_WORD = "Coming in"`, `SYNC_PENDING_OUTBOUND_WORD = "Going out"`.
- **`PENDING_MARKS`** `:461-469` — `Record<string, {icon, word}>`, one per reason: `untracked`/`added` → `ArrowUpFromLine`, `modified` → `CircleArrowUp`, `deleted` → `CircleMinus`, `settling` → `Clock`, `incoming` → `ArrowDownToLine` ("New file · Coming in"), `incomingUpdate` → `CircleArrowDown` ("Changed · Coming in").
- `PENDING_REASONS` `:475-480` — the fixed phrase per reason (everything except `settling`, which is timed).
- **`syncPendingReason(pending, now = Date.now())`** `:522-534` — exported and unit-tested; settling reports elapsed wait, every other reason a fixed phrase, *"with an unrecognized one rendered as itself"* — so a new reason arriving from Rust degrades gracefully rather than blanking.
- **`PendingList`** `:1451-1541` — props `{profile, rows: SyncPendingVm[] | null, current: string | null}`. The row being transferred is sorted first (`:1456-1460`: *"this list runs to eighty-odd rows on a folder mid backlog, and the one thing happening right now was on none of the screens"*). Per row: `PENDING_MARKS[row.reason]` with `FileIcon` fallback (`:1489-1490`), screen-reader text (`:1508-1516`), `title={`${row.path} — ${syncPendingReason(row)}`}` (`:1521`), then `FoldToggle` and the conditional `SYNC_SETTLING_NOTE` (`:1537-1538`).
- Folding: `useFold` `:250-273` and `FoldToggle` `:280-300`, sizes from `syncListSizes()`.

**A `virtual` / `materialized` reason is a three-line change here** — one `PENDING_MARKS` entry, one `PENDING_REASONS` entry, one wire string in `sync_ipc.rs:1194-1212`. **An *action* on a row is not** — `PendingList` renders inert `<li>`s today; a Materialize/Release button would be the first interactive control in that list.

### 3.2 The Files tree — the better home for a per-file action

`src/components/layout/files-pane.tsx`:

- `FilesPane` `:688`; one directory per call via `syncBrowse(profileId, subpath)` `:835-838`, with listings and failures cached in state.
- **Row composition** `:1869-1872` — `{entry !== null && <SyncStatusMark sync={entry.sync} />}`, placed *between the name and the actions*, with the comment: *"A profile root has no entry of its own and takes no mark; its children answer for themselves."* Row actions (create, delete, attach-to-note, reveal) already follow — **this is the natural attachment point for a per-file Materialize/Release control.**
- Related surfaces reusing the same listing: `panel-strip.tsx:203` (resolves one file through `syncBrowse` on its parent folder), `export-controls` via `syncExportEntry`.

`src/components/layout/sync-status-mark.tsx`:
- `FILES_SYNC_MARK_LABEL: Record<FilesSyncStatusVm, string>` `:40-46` — `synced: "Synced"`, `waiting: "Waiting to sync"`, `excluded: "Excluded from sync"`, …
- `MARK_ICON` `:49-55` — *"One shape per state. The shape is what carries the distinction."* (`synced: Check`, `waiting: Clock`, `excluded: Ban`)
- `SyncStatusMark({ sync })` `:80-93` — `role="img"`, never focusable, `data-sync-status={sync.status}`, tone from `MARK_TONE`, label `sync.detail ?? FILES_SYNC_MARK_LABEL[sync.status]`.

**Backing Rust:** `FilesSyncStatusVm` (`keeper-core/src/vm.rs:3812-3828`) = `Synced | Waiting | Excluded | NotInRepository | Unknown`, with the design note `:3805-3810`: *"Deliberately keeper's own vocabulary and not git's. `staged`, `untracked` and `ahead` are answers to a question nobody browsing a folder is asking; the sentence in `FilesEntrySyncVm::detail` is where the specific reason goes, composed in Rust like every other sentence this surface renders."* And `:3799-3804` on why `unknown` exists: *"when the engine could not answer, every other value is a claim with nothing behind it, and the two available guesses are 'your work is safe' and 'keep waiting'. Neither is honest."* `FilesEntrySyncVm { status, detail: Option<String> }` `:3839-3847`, constructors `plain` / `explained` `:3850-3866`.

Critically, `sync_browse` **reads** the mark rather than recomputing it — `sync_ipc.rs:1690-1700`: *"**The sync mark is read, not recomputed** (Story 44.17, FR-173). `Engine::pending` is already the one derived answer to 'what has this folder not synced yet, and why'… Calling it here rather than asking git a second question is what keeps the two surfaces from ever wording the same file differently — and it is the reason a mark cannot become a second source of sync truth."* Plus `:1701-1703`: a folder whose repository is unreadable still lists, marked `Unknown` with the engine's own words.

> **Constraint this imposes:** adding a `Virtual` variant to `FilesSyncStatusVm` obliges `Engine::pending` (or a sibling engine derivation) to be its source. A UI-side inference — "the file is small and looks like pointer text, so draw it virtual" — would violate the stated invariant.

### 3.3 Stores and the IPC client

- `src/lib/stores/sync-detail.ts` — `pending: SyncPendingVm[] | null` `:108-109`, `problems` `:110-111`; one refresh fans out `syncActivity(id, listSizes.unfolded)` / `syncPending(id)` / `syncProblems(id)` in a single `Promise.all` `:235-238`. Also exports `startSyncProgressStream`, `syncLiveFraction`, `syncLiveRate`, `refreshSyncDetail(All)`, `retrySyncParked(All)` (imported at `sync-pane.tsx:150-162`).
- `src/lib/stores/sync.ts` — profile mirror and actions: `ensureSyncHydrated`, `isSyncStatusActive`, `startSyncStatusPolling`, `syncProfileNow`, `rescanSyncProfile`, `removeSyncProfile`, `setSyncProfileEnabled`, `syncErrorMessage` (imported `sync-pane.tsx:135-147`).
- `src/lib/ipc/client.ts:3477-3479` — `syncPending(id)` wraps `invoke<SyncPendingVm[]>("sync_pending", { id })`, with the rendering contract restated in its doc `:3468-3475`. Type re-exports `:249-253`, imports `:391-395`.
- Generated types: `src/lib/ipc/gen/SyncPendingVm.ts:6-27`, `src/lib/ipc/gen/SyncStatusVm.ts:29-35`.

---

## 4. Progress reporting — how a one-off user-triggered download would report

`src-tauri/crates/keeper-sync/src/progress.rs`:

- **Sink type**: `pub type ProgressSink = Box<dyn Fn(SyncProgress) -> bool + Send + Sync>` `:23`.
- **Phases** `SyncPhase` `:28-51`: `Scanning, Fetching, Applying, Staging, Committing, Pushing, UploadingLfs, DownloadingLfs, Verifying, Idle`. `:26-31`: *"Coarse on purpose: this drives a tray glyph and a status line, so a phase exists only if a user would describe the work differently."* Up and down are **separate variants**, not one variant with a flag, `:38-45`: *"because the tray renders the two differently and the phase is the only thing that reaches it: a state the icon must distinguish has to be distinguishable in the type."*
- `is_active` `:60-63`.
- `carries_rate` `:101-111` — **total match, no `_` arm** so a new phase must be classified or the crate does not compile; only `Fetching | UploadingLfs | DownloadingLfs` are `true`. The long doc `:66-100` explains that `Pushing` moves bytes but cannot report a rate, because the push is `git push --porcelain` through `git::cli::capture` and *"the byte counters exist only in a `--progress` stderr stream nothing in this crate reads."* Two producers stamp the figure: `Engine::fold_fetch_progress` under `Fetching`, and `TransferTally::apply` under the two LFS phases.
- `direction` `:116-125` — also total, *"so the mapping is deliberately total rather than a pair of `matches!` that could disagree with each other as phases are added."*
- `label` `:128-143` — both LFS directions render the single word `"Transferring"`, because the status line already names the profile and the byte counts.
- **Payload** `SyncProgress` `:146-172`: `profile_id, profile_name, phase, files_done, files_total: Option<u64>, bytes_done, bytes_total: Option<u64>, current: Option<String>, bytes_per_second: Option<u64>`. `files_total: None` ⇒ the UI must render an indeterminate meter *"rather than inventing a denominator"* `:154-157`. `current` is *"A repository-relative path, never an absolute one — absolute paths leak home directory names into logs and screenshots"* `:158-162`.
- `SyncProgress::idle(...)` `:175-188`.
- `fraction()` `:191-202` — prefers bytes over files: *"a 4 GB video and a 2 KB note are one file each, and a file-counted bar would sit at 50% for ten minutes."*
- **Rate honesty** `RateMeter` `:204-289`: `RATE_MIN_WINDOW_MS = 1_000` `:206-213` (both byte producers sample at ~100 ms, so a burst divided by 100 ms would misreport a sustained rate); `RATE_WINDOW_MS = 2_000` `:215-222` (the window must close or the figure becomes the average since the transfer began). The meter **never yields `Some(0)`** `:227-233`: *"'0 B/s' would claim a measurement where there is only an idle wire… which is what lets the UI render `None` as nothing without ever having to special-case a zero."* `observe(bytes, now)` `:237-264`, `bytes_per_second()` `:267-269`. Time is a parameter, not a clock read — *"which is what makes the boundaries testable."*
- `ObjectBytes` `:272-279` and `TransferTally` `:281-300` — the raw `TransferEvent` stream *"cannot be read as a total"* because `Progress` carries each object's own cumulative count while up to `lfs::basic::DEFAULT_CONCURRENT_TRANSFERS` objects interleave.
- `status_line(&SyncStatus)` composes the one sentence the tray and the window both render — consumed at `sync_ipc.rs:262` and imported by the daemon at `keeper-syncd/src/commands.rs:34`.

**Three sinks, one type**: Tauri → `channel.send(vm).is_ok()` (`sync_ipc.rs:1641-1656`); `keeper-syncd` → log lines (`progress.rs:9-11`); tests → `Vec`.

**How a user-triggered materialize would report — the existing precedent already fits:**

1. Publish `SyncPhase::DownloadingLfs` for the profile. The engine's publish idiom is `self.publish(self.progress(profile, phase))`, e.g. `engine.rs:3766` (Committing), `:5231`, `:5232` (back to `Idle` after `sync_once`).
2. `carries_rate()` is already `true` for that phase (`progress.rs:103`), so a `bytes_per_second` figure is legitimate and the `RateMeter` rules apply unchanged.
3. `current` = the repository-relative path being materialized.
4. `bytes_total` = the pointer's `size`, known **upfront** from the pointer itself — so the bar is determinate from the first byte, unlike a scan. `fraction()` then prefers bytes (`progress.rs:194-198`), which is the right behaviour for a single multi-GB object.
5. On completion, retire the claim via `clear_phase` (`engine.rs:1023-1027`), which is the authoritative move: `engine.rs:1000-1022` — *"The snapshot is the authoritative channel — the tray reads it with no webview attached, `keeper-syncd status` reads it from another process… clearing the snapshot is what actually retires the claim on every surface."*

**One structural caveat.** `SyncProgress` is keyed by **profile**, not by request (`progress.rs:149-150`). A user-triggered materialize running concurrently with a background sync would publish two activities into one per-profile slot, and the last writer wins. The `Channel<T>` + per-job id shape used by `export_start` — where every event carries its own `export_id` (`keeper/src/ipc.rs:2577-2618`, `:2712-2716`) — is the existing precedent for a **job-scoped** stream and is the better model if a materialize must show progress independent of the folder's own sync.

---

## 5. Platform ports, per-OS abstraction, and the FFI precedent

### 5.1 Two ports, deliberately separate

`keeper-sync/src/platform.rs:1-12`: *"`keeper-sync` reaches the OS only through this trait, exactly as `keeper-core` reaches it only through `keeper_core::platform::Platform`. It is a **separate** port rather than a reuse of that one because AD-40 keeps this crate free of `keeper-core` — otherwise `keeper-syncd` would link matrix-sdk on a headless server. Two implementations exist: the Tauri shell delegates to its existing `DesktopPlatform`, and `keeper-syncd` implements it directly against XDG paths and the OS keyring. A third, `TestPlatform`, lives here so unit tests never touch the real keychain or the real clock."*

**`SyncPlatform`** — `keeper-sync/src/platform.rs:19-110`, object-safe on purpose (`:20-23`: *"the engine holds `Arc<dyn SyncPlatform>` so a profile supervisor can be spawned without threading a generic through every type. Nothing here does real work — each method is a thin capability the host already has."*):

| Method | Line | Contract highlights |
|---|---|---|
| `data_dir()` | `:26` | where `sync.db` and engine-owned state live |
| `secret_get` / `secret_set` / `secret_delete` | `:33`, `:37`, `:40` | implementations MUST NOT write a secret where the engine's own persistence can see it (never `sync.db`, never `config.json`); deleting an absent secret succeeds |
| `notify(title, body)` | `:44` | best-effort **by contract**: a host with no notifier returns `Ok(())` rather than failing a sync |
| `now_ms()` | `:53` | wall-clock, injected *"so the quiescence gate (Story 26.3) and the scheduler (Story 26.6) are testable without sleeping"*; must be wall-clock not monotonic, because the scheduler reasons about time that passed while the process was not running |
| `utc_offset_minutes()` | `:73` | the one **provided** method; default via `gix`, overridable so a window test is the same test in Reykjavík and Auckland |
| `free_space(path)` | `:84` | `None` ⇒ callers **MUST** proceed (fail-open) — *"A sync that refuses to run because a statvfs failed is worse than one that runs out of space and says so"* |
| `git_program()` | `:104` | AD-41 hard prerequisite; must be **usable**, clearing `git::cli::MIN_GIT_MAJOR/MINOR` |
| `host_label()` | `:109` | provenance trailers and conflict filenames (AD-43/44) |

Helpers in the same file: `machine_utc_offset_minutes` `:126-128`, `civil_from_unix_ms` `:147-176` (hand-rolled because the crate deliberately has no `chrono`).

**Three implementations:**
- `TestPlatform` — same file `:178-300`, deliberately **in the library, not behind `#[cfg(test)]`**, *"so the sibling crates (`keeper`, `keeper-syncd`) can use it in their own tests without duplicating it"* `:180-184`. `advance_ms` `:225-228`, `set_utc_offset_minutes` `:238-241`, `without_git` `:210-213`.
- `LinuxPlatform` — `keeper-syncd/src/platform.rs:324`.
- `ShellSyncPlatform` — `keeper/src/sync.rs:51`, delegating each method to the `keeper-core` `Platform` (`:52-56`).

**`Platform`** (the app-side port) — `keeper-core/src/platform.rs:26+`, with `data_dir` `:29` and `exclude_from_backup` documented `:66-70`. Implementations: `DesktopPlatform` `keeper/src/ipc.rs:650`, `IosPlatform` `keeper/src/ipc.rs:821`.

### 5.2 What is already abstracted per-OS

Very little OS branching lives inside `keeper-sync`; the port absorbs it. The exceptions are compiled per-target rather than injected:

- `stability.rs:176-181` — `#[cfg(target_os = "macos")] fn metadata_is_dataless` using `std::os::macos::fs::MetadataExt::st_flags` (*"`st_flags` lives on the macOS-specific extension trait, not the unix one"*).
- `stability.rs:183-189` — the non-macOS arm returning `false`, with the reasoning: *"There is no `SF_DATALESS` equivalent on Linux or Windows: a FUSE-backed cloud mount either has the bytes or fails the read, and neither one is triggered by opening."*
- `stability.rs:155-161` — macOS has no tier 3 (also `docs/sync.md:155-161`).
- `keeper-syncd` is **unix-only by construction** (`main.rs:11-13`).
- `keeper-syncd/src/commands.rs:971` — `tokio::signal::unix::SignalKind::terminate`, another unix-only dependency.

### 5.3 Where macOS-specific / `unsafe` code is allowed

Policy — `docs/project-context.md:55-60`:

> `unsafe_code = "deny"` (workspace lint). In `keeper-core` and all business logic: no `unsafe`, ever. In the `keeper` **shell crate ONLY**, a narrowly-scoped, **function-level** `#[allow(unsafe_code)]` is permitted for platform FFI that has no safe binding (e.g. iOS `NSURLIsExcludedFromBackupKey` via objc2), under these conditions: **one function per concern, behind the `Platform` port, with a `// SAFETY:` comment citing the API contract, and listed in the audit inventory in `docs/constraints-and-limitations.md`.** (Coordinator [policy amendment])

Inventory — `docs/constraints-and-limitations.md:83-92`, *"Policy (2026-07-11): `unsafe_code` stays denied workspace-wide; the `keeper` shell crate may carry function-level, audited `#[allow(unsafe_code)]` exceptions for platform FFI with no safe binding. Current inventory:"* — followed by **exactly one entry**: iOS backup exclusion via objc2-foundation, behind `Platform::exclude_from_backup`, in `crates/keeper/src/ipc.rs`, story 14.7 (FR-65).

The implementation — `keeper/src/ipc.rs:938-952`: `#[allow(unsafe_code)] fn exclude_from_backup(&self, path: &Path)` inside `impl Platform for IosPlatform` (`ipc.rs:821`), with the objc2 types used *inside the method body only* *"so no iOS-only import leaks to the desktop compile (mirrors the 12.3 keychain pattern)"* (`:951-952`). Dependency justification lives in `keeper/Cargo.toml:120-124`.

**Consequences for an OS-level placeholder presentation:**

1. It cannot live in `keeper-sync` (workspace `unsafe_code = deny` plus the port discipline), and it cannot live in `keeper-syncd` (Linux/unix-only, `main.rs:11-13`). **So the server — the stated primary environment — gets no Finder-style placeholder at all; it gets the pointer file plus whatever metadata keeper chooses to expose.** That is not a shortfall to be engineered around; it is what `LfsMode::PointerOnly` already produces today.
2. On macOS it must be a function-level `#[allow(unsafe_code)]` in the **`keeper` shell crate**, behind a port method, with a `// SAFETY:` comment, plus an added line in `docs/constraints-and-limitations.md:88-92`.
3. A macOS File-Provider extension is a **separate bundle target**, for which the current inventory has no precedent. That is a genuinely new architectural commitment, not an extension of the existing exception — and `docs/project-context.md:60` records that the exception was a **coordinator decision**, so widening it is a governance step.
4. If a new capability is needed by the engine ("ask the host to present this path as a placeholder"), the idiomatic shape is a **provided** `SyncPlatform` method defaulting to a no-op — the `utc_offset_minutes` precedent (`platform.rs:59-75`) — so `LinuxPlatform` and `TestPlatform` need no change and the daemon degrades honestly.

---

## 6. Existing placeholder / dataless-file handling — what keeper already knows

### 6.1 Reading OS placeholders (macOS, today)

`keeper-sync/src/stability.rs`, module doc `:29-37`:

> **The iCloud hazard** — *"Orthogonal to all four tiers, and mandatory: on macOS a file may be a **dataless placeholder** whose bytes live in iCloud. `stat` is safe; `open` is not — opening one silently materializes it. A sync engine that hashes a Documents tree under iCloud Drive without checking `SF_DATALESS` drags the user's entire cloud library down over their network. **Every path into an `open` in this module goes through that check first.**"*

- `pub const SF_DATALESS: u32 = 0x4000_0000;` `:60-65` — macOS `bsd/sys/stat.h`; *"Defined on every platform so the constant is greppable and documented in one place; only the macOS build reads it."*
- `pub fn is_dataless(path: &Path) -> Result<bool>` `:164-174` — `symlink_metadata` only. *"Cheap (`lstat`) and safe: reading the flag does **not** materialize the file, whereas `open` does. Always `Ok(false)` off macOS."* `NotFound` ⇒ `Ok(false)`; any other error propagates as `SyncError::io("stat", …)`.
- `metadata_is_dataless` — macOS `:176-181`, non-macOS `:183-189`.
- `StabilityVerdict::Dataless` `:155-159` — *"A macOS iCloud placeholder. **Deliberately distinct from `Excluded`** so the caller can *warn* the user that a file is being skipped, rather than silently omitting content they can see in Finder."* (Siblings: `Vanished` `:155`, `Excluded` `:161`.)
- Gate ordering `:559-584` — tier 0, **then the iCloud guard, before anything that could open the file**; a *failed probe* is treated as `Settling`, not as safe: `tracing::warn!(… "dataless probe failed")`.
- A second guard immediately before opening `:820-829` — refusal reason *"dataless iCloud placeholder; opening it would materialize the file"*.

### 6.2 Where the engine surfaces it

`engine.rs:4195-4207` — on `StabilityVerdict::Dataless` the engine warns the user by name:

> *"Opening it would silently pull the whole object down from iCloud, so it is skipped and the user is told."* → message: `"{path} is a cloud placeholder and was skipped — download it locally to sync it"`.

Note `engine.rs:1044-1077`: this skipped-file count is explicitly one of the messages whose **text moves with the condition it describes**, which is why notification onset is keyed on `None → Some` on the snapshot's `warning`, never on wording — a text-keyed rule produced *"3 600 an hour for a single folder that stayed broken, which is the failure mode AD-51 calls a notification storm and which trains a user to turn keeper's notifications off."* The level (banner/tray) is rewritten every tick; only the native toast is edge-triggered.

> **A virtual-file feature that emits per-file warnings must obey the same edge-triggering rule.** A per-path message carrying a moving number is exactly the shape AD-51 forbids.

### 6.3 The copy engine

`keeper-sync/src/copy.rs:45-49` — the verified-copy path refuses FIFOs, sockets and device nodes *"and for macOS dataless iCloud placeholders (opening one silently materializes a file that may be gigabytes)"*. Implementation `:823-830` — `PlanItem::Refused { rel, reason: "a dataless iCloud placeholder; copying it would materialize the file from the network" }`. It imports `crate::stability::{is_dataless, FileSample}` at `:68` — one implementation, two consumers.

### 6.4 Docs

- `docs/sync.md:163-168` (§4 "iCloud placeholders"): *"On macOS, keeper checks `SF_DATALESS` before opening any file and skips placeholders. Opening a dataless file silently **materializes** it — without this check, syncing a folder under iCloud Drive would drag your entire cloud library onto the disk."*
- `docs/sync.md:153-161` ("Two honest gaps") — macOS has no tier 3.

**No `brtime` handling exists anywhere in the repository** — searched `keeper-sync/src`, `keeper-core/src`, `keeper/src` and `docs/`: zero hits. Nor is there any Windows placeholder / `FILE_ATTRIBUTE_RECALL_ON_*` handling.

### 6.5 keeper's OWN materialization machinery — the load-bearing discovery

keeper already implements an internal notion of *content present as a pointer that can become real bytes on demand.* It is not documented as a user feature, but it ships:

| Piece | Location | What it does |
|---|---|---|
| `LfsMode::PointerOnly` | `profile/mod.rs:81-90` | leaves LFS-tracked content as a pointer stub in the worktree **on purpose** (`LfsMode::Materialize` `:82-85` is the default: *"the only mode safe for multi-GB content: gitoxide has no streaming object read, so a raw 3 GB blob is a 3 GB allocation (AD-46)"*) |
| `MediaPolicy::Materialize / PointerOnly` | `profile/mod.rs:1809-1812` | the same choice expressed per media class, with a wire spelling test |
| `Engine::materialize_pending` | `engine.rs:4978-5065` | for each pending smudge: if `store.contains(&oid, size)` → `lfs::stage::materialize`, then `db::remember_materialized`; otherwise queue a transfer. *"a partial fetch materializes what it can and returns for the rest"* `:4981-4982`. Pointer-only mode short-circuits at `:5021-5026` and `:5037` |
| **`subpaths[]` filter the *transfer*, not just the checkout** | `engine.rs:4984-4998` | *"a pointer would pull down the gigabytes the profile exists to avoid"* — **the existing precedent for "do not download this content"**, and the closest thing today to the requested selector |
| Call sites | `engine.rs:4912-4913`, `:4972-4973`, `:5228-5229`, `:5994-5995` | after fetch, after a local-remote pull, in `sync_once`, and on demand |
| `lfs::stage::materialize` | `lfs/stage.rs:1108-1130` | writes to a **sibling temp file and renames**, so *"an interrupted materialization leaves the pointer intact rather than a truncated video: the operation is then simply retried. The staging name carries keeper's own `.keeper.*.tmp` prefix, which tier 0 already excludes, so the watcher cannot mistake it for user content."* Refuses with `SyncError::Integrity` if the store lacks the object `:1120-1121` |
| `materialized` table | `db.rs:141-147` | `(profile_id TEXT, path TEXT, at_ms INTEGER)` — the durable ledger |
| `db::remember_materialized` | `db.rs:322-338` | *"Called when an object is materialized, which is the one moment the fact is known. `INSERT OR REPLACE` because materializing again is the ordinary case — a second version arriving — and the newest timestamp is the useful one."* |
| `db::materialized_paths` | `db.rs:341-353` | the whole set for a profile, *"one statement beats a query per line"* |
| Index-stat repair | `engine.rs:5061-5065`, `:5359-5392`; `git/repo.rs:1878-1912` | after materialization *"The worktree files just changed size, so the index entries carry the pointer's stat and status would call every one of them"* modified. `refresh_index_stat` runs immediately after materialization; `repair_index_stat` re-stats *"every materialized LFS entry whose index stat has gone stale"*, reachable from `rescan` (log line: *"recheck: restored the index stat for materialized content"*). `git/repo.rs:1896-1900`: *"A checkout would re-materialize files that are already correct, cost the transfer again, and touch mtimes the user may be relying on. Re-stat is the smaller and truer repair."* |
| Pointer-vs-content discrimination | `git/repo.rs:1908-1912` | *"Only the entries whose worktree file is NOT a pointer: those are the materialized ones… An entry that still holds a pointer on disk is already consistent"* — the exact predicate a virtual/materialized badge needs |
| Pointer scan bounded to the index | `git/repo.rs:1822-1832` | *"Used to find checked-out LFS pointers that still need materializing: only a tracked path can hold one, so this bounds that scan to the index rather than walking the whole worktree"* — note the `U+FFFD` lesson (Story 47.2) |
| `lfs::prune::plan` / `release` | via `engine.rs:5299-5316`; policy `docs/sync.md:334-352` | the safe release path |
| Already-present short-circuit | `lfs/basic.rs:233-236` | *"Already materialized — a re-driven journal unit, or two profiles sharing content. Content addressing makes this unambiguous."* |

And the **pending derivation already distinguishes arrival from replacement** — `engine.rs:5836-5862`:

```rust
let held = self.with_db(|conn| db::materialized_paths(conn, profile_id)).unwrap_or_default();
for (label, oid, size_bytes) in self.with_db(|conn| db::queued_downloads(conn, profile_id))? {
    // An object whose path is not in the index cannot be named — it was
    // queued for a path since deleted.
    let path = label.unwrap_or_else(|| format!("LFS object {}…", &oid[..oid.len().min(12)]));
    if named.insert(path.clone()) {
        let replacing = held.contains(&path);
        out.push(PendingFile {
            path,
            reason: PendingReason::Incoming { size_bytes, replacing },
            size_bytes: Some(size_bytes),
        });
    }
}
```

…with the ordering note at `:5833-5836` (*"Added after the status buckets so `named` gives a local change precedence"*), and the wire mapping at `sync_ipc.rs:1196-1207`: *"Two words for one direction: an object that replaces content this machine holds is a different thing to look at from one that is simply arriving, and **only keeper's own record of what it has materialized can tell them apart.**"*

Related documented behaviour worth carrying into the design:
- **A pointer is never published ahead of its object** — `docs/sync.md:446-472`; `Engine::lfs_uploads_outstanding` `:3785-3789` costs one indexed `COUNT(*)` per push.
- **Working in the folder with plain `git`** — `docs/sync.md:533-593`; keeper registers itself as `filter.lfs.clean` / `filter.lfs.smudge`, and `keeper-syncd lfs clean|smudge` is that filter (`commands.rs:285-298`, served before anything else in `lib.rs:175`). **A virtual file that a plain `git checkout` would smudge into existence is a hole in any "do not download" policy — the filter is a second materialization path the feature must reason about.**
- **Verify** — `docs/sync.md:313-326`: `keeper-syncd verify` checks that every pointer in the worktree names an object *this machine* still has; `--remote` asks the server. On a deliberately virtualized tree the local half would report every virtual file as missing unless the check learns the new state.

---

## 7. Consolidated attachment map

| Concern | File to change | Anchor |
|---|---|---|
| Pattern-based "do not download" selector | `keeper-sync/src/profile/mod.rs` | beside `lfs_never` `:813`, `subpaths` `:743-747`, `lfs_mode` `:81` — auto-propagates to the daemon TOML, the app's JSON, and `SyncProfileVm` |
| Transfer-side filter precedent | `keeper-sync/src/engine.rs` | `materialize_pending`'s subpath filter `:4984-4998`; `keeper-sync/src/sparse.rs:10-43` for what a cone actually materializes |
| Materialize / release engine methods | `keeper-sync/src/engine.rs` | beside `materialize_pending` `:4998`, `prune_lfs_store` `:5296` |
| Atomic write discipline | `keeper-sync/src/lfs/stage.rs` | `materialize` `:1112-1130` (temp + rename, `.keeper.*.tmp`) |
| Index-stat obligation, both directions | `keeper-sync/src/git/repo.rs` | `refresh_index_stat` / `repair_index_stat` `:1878-1912`; engine call sites `:5061`, `:5382` |
| 24 h lazy release sweep | `keeper-sync/src/engine.rs` | `mark_synced` `:3052` (the success edge), or a `next_release_ms` pacer mirroring `next_scan_ms` `:533` / `scan_is_due` `:1254` |
| Release ledger + clock | `keeper-sync/src/db.rs` | `materialized` `:141`, `remember_materialized` `:324`, `materialized_paths` `:341` |
| Second materialization path to close | `keeper-syncd/src/commands.rs` + `keeper-sync/src/git/repo.rs:687-691` | the `filter.lfs.smudge` registration — plain `git checkout` can materialize |
| Daemon verbs | `keeper-syncd/src/commands.rs` | `Command` enum `:~284`, dispatch `:~546`, exit mapping `:120` |
| Daemon config keys | `keeper-syncd/src/config.rs` | derived from `SyncProfile` (free); `[daemon]` `:57` if process-scoped |
| Tauri commands | `keeper/src/sync_ipc.rs` | beside `sync_open_entry`; register in `keeper/src/lib.rs:901-947` |
| Progress for a one-off download | `keeper-sync/src/progress.rs` | `SyncPhase::DownloadingLfs` `:47`, `carries_rate` `:103`; job-scoped precedent `keeper/src/ipc.rs:2577-2618` |
| Pending badge | `engine.rs:5843` → `sync_ipc.rs:1194` → `sync-pane.tsx:461` | one enum arm, one wire string, one `PENDING_MARKS` entry |
| Files-tree badge + action | `keeper-core/src/vm.rs:3812` → `sync-status-mark.tsx:40,49` → `files-pane.tsx:1872` | the mark must be **derived in Rust** — `sync_ipc.rs:1690-1700` |
| Verify semantics | `keeper-syncd/src/commands.rs:243-264`; `docs/sync.md:313-326` | a virtualized tree must not read as data loss |
| macOS OS-level placeholder | `keeper/src/ipc.rs` (**shell crate only**) | policy `docs/project-context.md:55-60`; inventory `docs/constraints-and-limitations.md:83-92`; precedent `ipc.rs:938-952` |
| Notification discipline | `keeper-sync/src/engine.rs:1044-1077` | edge-triggered on presence, never on wording (AD-51) |
| Docs | `docs/sync.md` | §4 `:163`, §8 `:328`, §11 `:677`, §12 `:822`, §16 status `:1024`, §18 limitations `:1055` |

---

## 8. Open questions the grounding surfaced (not answered here)

1. **Per-profile vs per-pattern.** `LfsMode` is per profile (`profile/mod.rs:81`). A gitignore-like selector is a new axis: does it override `lfs_mode`, or apply only within `Materialize` mode? `lfs_never` (`:813`) is a glob list that *excludes from LFS entirely* — the new list means almost the opposite, so sharing its name or its plumbing would be a trap.
2. **Where do metadata (size / date / type / oid / "where it really is") come from when content is absent?** `size` and `oid` are in the pointer itself and cost nothing (`lfs/stage.rs` pointer type). **Date and "where it really is" are not in the pointer** — they need a new derivation (git log for the path) or a new ledger column. The `materialized` table currently has three columns (`db.rs:142-147`).
3. **Does a release rewrite the worktree file back to pointer text?** If it does, it **inverts `lfs::prune`'s condition 2** (*"the worktree still holds the real content"*, `docs/sync.md:344-348`), and the two features would fight over the same object — prune would stop releasing, or would release the only local copy. Release must therefore be one operation that de-materializes **and** prunes under a single set of conditions, not two independent policies.
4. **"Last use" is not observed today.** `materialized.at_ms` records *when materialized*, not *when last read* (`db.rs:324-327`). A true "24 h after last use" needs atime (unreliable; `noatime` is common) or an explicit access hook. **This is the single largest unbudgeted piece of the release ask** — "24 h after materialization" is implementable today; "24 h after last use" is not.
5. **Per-profile progress slot.** A user-triggered materialize concurrent with a background sync shares one `SyncProgress` slot (`progress.rs:149-150`). Either adopt job-scoped streaming (the `export_start` shape, `ipc.rs:2577`) or accept interleaving — deliberately, not by omission.
6. **The `filter.lfs.smudge` hole.** keeper owns the repository's smudge filter (`docs/sync.md:533-593`, `git/repo.rs:687-691`). A plain `git checkout` in a virtualized folder would invoke it and materialize content the policy said not to download. The filter must learn the policy, or the guarantee is only as strong as "nobody ran git by hand."
7. **`verify` on a virtualized tree.** `keeper-syncd verify` today reports a pointer whose object this machine lacks (`docs/sync.md:315-317`). Deliberate virtualization makes that the *normal* state, so verify must distinguish "virtual by policy" from "lost" — otherwise the one command that detects real data loss starts crying wolf.
8. **The server never gets Finder semantics.** `keeper-syncd` is Linux/unix-only (`main.rs:11-13`) and the unsafe-FFI exception is shell-crate-only (`docs/project-context.md:55-60`). On the stated primary environment, "visible in `ls`" means the pointer file — which `PointerOnly` already produces. The system-level presentation is a **desktop-macOS-only** ambition and should be scoped as such.
