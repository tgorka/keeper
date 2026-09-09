# Lane: watcher, completeness gate, removable media, exclusions, concurrency


## Scope read

- `src-tauri/crates/keeper-sync/src/watch.rs:1-1095` (whole file, incl. tests)
- `src-tauri/crates/keeper-sync/src/stability.rs:1-1000` verbatim; `1000-1801` via test-name index + targeted reads
- `src-tauri/crates/keeper-sync/src/volume.rs:1-470`
- `src-tauri/crates/keeper-sync/src/openfiles.rs:1-141` (whole file — it is `RLIMIT_NOFILE`, not an open-file probe)
- `src-tauri/crates/keeper-sync/src/exclude.rs:1-330`
- `src-tauri/crates/keeper-sync/src/engine.rs`: `766-860`, `1440-1580`, `3330-3520`, `3640-3870`, `3870-4300`, `4300-4470`, `5380-5540`, `6623-6720`, `6990-7060`, `7100-7350`, `10000-10130`, `12090-12165`, `12240-12252`, `12790-12870`
- `src-tauri/crates/keeper-sync/src/git/repo.rs:1530-1620`, `1820-1990`, `WalkPolicy` `1619-1680`
- `src-tauri/crates/keeper-sync/src/git/commit.rs:130-330` (grep-scoped: deletion guards, blob read)
- `src-tauri/crates/keeper-sync/src/lfs/stage.rs:760-840`
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` (settle constants, `effective_settle_ms`)
- `docs/sync.md:122-260` (§4, §6), `docs/sync.md:3000-3031` (§20)
- `notify-8.2.0/src/fsevent.rs:35-250`, `:299-302`, `:375-395`; `notify-debouncer-full-0.7.0/src/lib.rs:195-400`, `:637-690`
- `../evidence-hesperia.md` (both revisions)

Non-goals honoured: walk cost itself (ScanPerf), LFS transfer/store/gc (LfsReview).

## How it works

One `FolderWatcher` per profile (`watch.rs:450`), armed on every supervisor tick by `ensure_watcher` (`engine.rs:3754`) after the volume gate (`engine.rs:3365`). `RecommendedWatcher` = FSEvents on macOS; notify creates the stream with `kFSEventStreamCreateFlagFileEvents | NoDefer`, latency `0.0`, `sinceWhen = SinceNow` (`fsevent.rs:299-302`) — per-file kinds, no coalescing delay, no history replay. `NoCache` is forced (`watch.rs:Backend`) because `FileIdMap::add_path` would `WalkDir` the whole tree at arm time. `notify-debouncer-full` (timeout 500 ms, tick = timeout/4) collapses per path; `fold_batch` (`watch.rs:318`) collapses again into ≤1 `WatchEvent` per path per batch, checks `need_rescan()` **before** the kind match (`:338`) — the Linux overflow event is pathless — and escalates to a root-path event (`:355`). A separate thread emits a root event every 15 min (`watch.rs:104`). The channel is unbounded, by argument (`watch.rs:449`).

`fold_watch_events` (`engine.rs:3936`) drains without blocking, drops tier-0-excluded paths against the profile's own `StabilityGate` (`:3981`), sets `watch_wake`, and computes `untracked_appeared` by asking the index (`:3995`). `scan_due` is `paced || watch_wake || settle_window_elapsed` (`engine.rs:3727`); `scan_and_enqueue` spends the wake *before* the walk (`engine.rs:12846`).

The gate (`stability.rs`) is tier 0 (globset) + iCloud `SF_DATALESS` + tier 2 (two equal `(size, mtime, ctime, inode)` samples ≥ W apart **and** mtime ≥ W old), with a 60 s ceiling checked first (`:446`). `Stable` ends the episode (`:604`). `note_finished` / `prime_stable` both call `declare_settled` (`:405`), which backdates the entry past the ceiling. Mid-episode entries mirror to `file_state` on every walk (`engine.rs:7272-7275`).

## Findings

### F-GATE-1 Nothing re-checks the volume between the walk and the commit; a detach mid-walk stages every tracked path as deleted — P0, effort S

Evidence: the volume gate runs once, at the top of the tick —
```rust
// engine.rs:3365
if !self.volume_ready(profile)? { self.drop_watcher(&profile.id); return Ok(()); }
```
and the walk that follows, seconds-to-a-minute later, feeds deletions straight through:
```rust
// engine.rs:7265
staged.deleted.extend(status.deleted.iter().cloned());
```
The only mass-deletion guard is `sorted_len == 0 && !changes.deleted.is_empty()` (`git/commit.rs:191`), which fires **only for an empty index** — the "checkout never finished" shape. A repository whose index was read before the detach has 155 626 entries, so that guard is inert, and the deletions arrive through the *index↔worktree* half of the walk (`git/repo.rs:1562`, `EntryStatus::Change(Removed)`), not the tree↔index half the guard's comment describes. `commit_local` (`engine.rs:6623`) re-checks nothing.

Why: this is precisely the catastrophe `volume.rs:1-10` and AD-48 exist to prevent, and the check-then-act gap is exactly the walk duration — on hesperia p50 1.1 s, p90 5.3 s, **max 60.8 s**, on a USB drive that is unplugged by hand. `do_push` re-checks (`engine.rs:4649`) so the delete-everything commit is not pushed in the same pass, but it is durable locally and ships on the next pass once the drive returns, followed by an add-everything commit; a peer that pulls between the two sees the folder emptied. **[INFERENCE]** I could not exercise the yank here, so the exact gix behaviour when the mountpoint disappears mid-walk (ENOENT per entry → `Removed`, vs. a hard failure or SIGBUS on the mmapped index) is unverified — but no code path makes the benign outcome *certain*, which is the defect.

Fix: in `collect_stable_changes`, after the walk and before returning, re-assert the volume for a `removable` profile — one `VolumeMarker::is_present(mount_root)` (`volume.rs:139`) — and return `SyncError::MediaAbsent` if the marker is gone; belt-and-braces, refuse in `stage_and_commit` when `changes.deleted.len()` is a large fraction of the index and the profile is removable.

### F-GATE-2 Tiers 3 and 4 have no caller on the commit path, so the gate's stated proof does not exist there — P1, effort M

Evidence: `open_writer_veto` (`stability.rs:699`) has **zero** production callers; `verify_while_reading` (`stability.rs:804`) has exactly one, inside the user-invoked audit verb (`engine.rs:12149`, in `verify`). The commit path reads non-LFS content with a bare
```rust
// git/commit.rs:241
let bytes = std::fs::read(&absolute).map_err(...)?;
```
with no `fstat` before/after. `docs/sync.md:136` and `:162` say otherwise: *"4 — verify-on-read | Re-stats the open descriptor before and after… this is the proof"*, *"Tier 4 is the only guarantee"*, and `stability.rs:16-19` builds the whole module's safety argument on it (*"the reason the other three are allowed to be approximations"*).

Why: tier 2 is documented as fallible (a writer that pauses > W looks finished), and the 60 s ceiling (`stability.rs:446`) *deliberately* commits files that never quiesce. Both were sold on tier 4 catching the resulting torn read. For an LFS-routed path `lfs::stage::clean` gives a partial equivalent — `fstat` on the open fd and a `written != size` refusal (`lfs/stage.rs:808-828`) — so the exposure is exactly the **non-LFS** set: everything under the 256 KiB threshold plus hesperia's 18-extension `lfsNever` list. An in-place rewrite (SQLite, a re-saved document) caught by the ceiling commits a Frankenstein blob, silently.

Fix: route the non-LFS staging read through `verify_while_reading` (or inline the same before/after `FileSample` comparison around `std::fs::read`) and treat `SyncError::Integrity` as "skip this path this pass"; call `open_writer_veto` from `is_stable` on Linux. If neither is wanted, delete the dead tier-3 function and correct §4's table and `stability.rs`'s module doc — the current text is a safety claim nothing implements.

### F-GATE-3 A "finished"/"primed" entry is exempt from the gate for its whole life, not just its next verdict — P1, effort S

Evidence: `declare_settled` backdates *both* clocks past the ceiling —
```rust
// stability.rs:411-419
let quiet_since_ms = now_ms.saturating_sub(ceiling);
self.entries.insert(path.to_path_buf(), Entry { last: sample,
    unchanged_since_ms: quiet_since_ms, pending_since_ms: quiet_since_ms, close_write: false });
```
`observe` resets `unchanged_since_ms` when the sample changes but **never** touches `pending_since_ms` (`stability.rs:243-259`), and `verdict` checks the ceiling first and returns before any other condition (`stability.rs:446-449`).

Why: the doc promises *"the next `verdict` returns `Stable`"* (`stability.rs:296`, `:333`) and *"tier 4 still reads every byte"*. Neither holds. Until a walk reaches that path and `is_stable` drops the entry, **any** content is `Stable` — including bytes demonstrably written after the assertion. The window is not short: `commit_walk_policy` (`engine.rs:1518`) answers `tracked_only` whenever the watcher is live and no untracked path appeared, so a freshly-renamed or freshly-created asserted file can stay invisible to the walk until the 15-minute `UNTRACKED_SWEEP_INTERVAL` (`engine.rs:452`). With F-GATE-2 there is no downstream check either. `note_finished_path`'s only authorisation is "inside `recordings_root`" (`engine.rs:4392-4420`), so any holder of a `FinishedTap` can assert a file it is still writing.

Fix: in `observe`, reset `pending_since_ms = now_ms` on a changed sample (a new episode began); the ceiling then measures the *current* bytes, and a prime/assert still clears on the first verdict as intended.

### F-GATE-4 A single continuously-written file arms a full status walk on every 1 Hz tick, for as long as the write lasts — P1, effort M

Evidence: `fold_watch_events` sets the wake for any surviving path (`engine.rs:4024`), `scan_due` is `paced || watch_wake_pending || settle_window_elapsed` (`engine.rs:3728`), and `settle_window_elapsed` stays true from the moment a held path's deadline passes until a walk collects it (`engine.rs:3736`). The debouncer expires one event per kind per path per drain (`notify-debouncer-full/src/lib.rs:236-253`, tick = 125 ms), so an appended file produces a fresh event several times a second, indefinitely. There is no rate limit on the watch-driven walk — the poll-driven one has `POLL_WALK_MIN_INTERVAL` of 60 s (`engine.rs:431`, `:12241`); the wake-driven one has nothing but the 1 Hz tick.

Why: this is hesperia's reported shape — **1371 walks, `scanned=155626` every time, one every 1–5 s in busy hours, p90 5.3 s, max 60.8 s** — on a USB volume, while a recording session runs. Every one of those walks re-`lstat`s 155 626 entries to learn something the gate already knows: the file is `Settling`, and `next_stable_ms` already names the exact instant that could change. The engine spends its most expensive operation on an answer it is holding.

Fix: two cheap, independent halves. (a) In `fold_watch_events`, do not set the wake for a path the gate already tracks whose `stable_at_ms` is still in the future — the deadline already schedules that walk. (b) Give the wake-driven walk a per-profile floor of `min(effective_settle_ms, a few seconds)`, the way the poll walk has one.

### F-GATE-5 A watcher that dies or goes deaf is never detected — P2, effort M

Evidence: `ensure_watcher` returns as soon as the map says `Live` with a matching root —
```rust
// engine.rs:3762-3768
Some(ProfileWatch::Live { watcher, .. })
    if watcher.root() == profile.local_path.as_path() => { return; }
```
and the only runtime error channel is discarded:
```rust
// watch.rs:473-477
Err(errors) => { for err in errors { tracing::warn!(root = %root.display(), %err, "watch error"); } }
```
`WATCH_REARM_INTERVAL_MS` (`engine.rs:735`) only paces retries of an arm that *failed*; `ProfileWatch::Failed` is never entered from a live watcher. Re-arming happens on exactly four events: root change, volume detach (`engine.rs:3368`), profile disable/remove, process restart.

Why: the failure modes are real and silent. FSEvents on an external volume depends on `/Volumes/<vol>/.fseventsd` — a volume carrying `.fseventsd/no_log`, or one whose fseventsd was reset, delivers nothing and raises nothing; keeper lists `.fseventsd` as tier-0 noise (`exclude.rs:101-102`) without ever asking whether it is functional. An inotify instance that hits `MaxFilesWatch` *after* arming, or an FSEvents stream invalidated by the volume going read-only, arrives as an `Err` batch and is logged at WARN into an 895 MB log. The 15-minute sweep (`watch.rs:104`) and the 15 s paced poll bound the damage, so this is a silent downgrade to the paced backstop, not data loss — but `warn_watch_degraded` (`engine.rs:3838`), which exists exactly to make that visible, is unreachable for it.

Fix: record `last_event_ms` per live watcher; on the periodic rescan tick, if the watcher has delivered nothing since it was armed *and* that tick's walk found changes, demote to `ProfileWatch::Failed` with the existing banner. Cheaper alternative: treat any `Err` batch as a demotion — the debouncer only surfaces backend errors there.

### F-GATE-6 The 26 MB index is opened and parsed inside the global `gates` mutex, once per busy tick — P2, effort S

Evidence:
```rust
// engine.rs:3977-3999
let mut gates = Self::lock(&self.gates);
...
index = self.open_repo(profile).ok().and_then(|repo| repo.index_or_empty().ok());
appeared = match (&index, path.strip_prefix(&profile.local_path)) { ... }
```
`gates` is one map for **all** profiles (`engine.rs:936`), and `collect_stable_changes` (`engine.rs:7183`), `settle_window_elapsed` (`engine.rs:3737`) and `refresh_pending` (`engine.rs:5372`) all queue behind it.

Why: hesperia's `.git/index` is 26.4 MB / 155 626 entries. Parsing it is not free, and it is paid inside a lock on every tick that drains a non-excluded event — to answer only "is this one path in the index". Every other profile's gate work serialises behind it. The comment two lines above shows the author reasoning carefully about the *watchers* lock and then doing the expensive thing under the other one.

Fix: hoist the index open out of the `gates` critical section (do the exclusion pass first, answer `appeared` outside the lock), and cache the parsed index per profile keyed on `.git/index` (mtime, size) — the walk on the same tick re-opens it anyway.

### F-GATE-7 `prime_moved_paths` does not mark the profile as having untracked arrivals, so the next walk can prune the primes it just made — P2, effort S

Evidence: `prime_moved_paths` records gate entries and mirrors them (`engine.rs:4260-4290`) but never touches `untracked_appeared`, which is what buys a `WalkPolicy::full()` (`engine.rs:1524-1528`). A moved-in file is by definition untracked, so `tracked_only` cannot see it, and the walk then prunes every entry it did not observe:
```rust
// engine.rs:7272
gate.retain(&observed);
```

Why: Story 40.4's point is that the deletion and the addition land in **one** commit so `git log --follow` works. If the primed destinations are absent from the first walk after the prime, their entries are deleted and the move splits after all. Today the watcher usually rescues it — a filesystem move raises Create events and `fold_watch_events` sets `appeared` (`engine.rs:3995`) — but that makes a correctness property depend on a watcher the rest of the code treats as best-effort, and `prime_moved_paths` is a public API callable for moves the watcher never saw (degraded watcher, `force_poll`, an event lost to debounce coalescing).

Fix: `Self::lock(&self.untracked_appeared).insert(profile.id.clone())` when `primed > 0`. One line, and the escape hatch stops depending on the watcher.

### F-GATE-8 `StabilityGate::forget_all` has no production caller, so a detach or pause leaves entries whose ceiling fires instantly on return — P2, effort S

Evidence: the API documents its own callers —
```rust
// stability.rs:617-621
/// Drop all state. Used when a profile is paused or its volume detaches:
/// windows measured against a clock from before the pause are meaningless.
pub fn forget_all(&mut self) { ... }
```
— and a crate-wide grep finds exactly one caller: its own unit test (`stability.rs:1642`). `tick_profile`'s detach arm drops the *watcher* only (`engine.rs:3368`); pause/remove drop the gate map entry (`engine.rs:1720`) but the durable `file_state` rows survive and are re-imported by `ensure_gate` (`engine.rs:4226-4229`).

Why: hesperia's drive was detached and re-attached on 09-07. After a re-attach, an entry whose `pending_since_ms` predates the detach is instantly past the 60 s ceiling (`stability.rs:446`), so the first observation returns `Stable` regardless of what the bytes are doing — the exact condition the doc says `forget_all` exists to prevent, and, with F-GATE-2, with nothing downstream to catch it.

Fix: call `forget_all` (and clear `file_state`) from the detach arm of `tick_profile`; or — better, and it subsumes F-GATE-3 — make `observe` restart `pending_since_ms` on a changed sample, which fixes both without a lifecycle hook anyone can forget.

### F-GATE-9 Watch paths are canonicalized by notify but stripped against the configured path; a symlinked root breaks anchored excludes and forces a full walk every batch — P2, effort S

Evidence: notify canonicalizes what it watches and reports canonical paths (`fsevent.rs:376-380`, `:392`), while both consumers strip the *configured* root: `StabilityGate::is_excluded` does `path.strip_prefix(&self.root).unwrap_or(path)` (`stability.rs:551`) and `fold_watch_events` does `path.strip_prefix(&profile.local_path)` with `_ => true` on failure (`engine.rs:3995-4003`).

Why: if `local_path` traverses a symlink (`/Users/x/Sync` → an external volume, `/tmp` → `/private/tmp` on macOS), *every* event path fails `strip_prefix`. Two silent consequences: anchored exclusions stop matching, so keeper's own `.git/**` and `.keeper-sync/**` writes each buy a wake and a walk — the exact pathology the Story 34.9 comment at `engine.rs:3900-3919` was written to kill; and `appeared` becomes `true` on every batch, so every tick takes `WalkPolicy::full()`, the directory scan the same file measures at 996 s on this folder. hesperia's three roots are real directories so it does not fire there; nothing prevents it. **[INFERENCE]** on the exact macOS symlink shapes; the mismatch itself is in the source. (Same class, lower impact: the gate keys on case-sensitive `PathBuf` while APFS runs `core.ignorecase=true`, so two spellings of one file can hold two gate entries and two `file_state` rows.)

Fix: canonicalize `local_path` once at profile load (or store the canonical root on the gate and strip against both), and make a `strip_prefix` failure a loud `debug!` rather than a silent "assume the worst".

### F-GATE-10 A nested repository is skipped forever with a log line the user never sees — P2, effort S

Evidence: `expand_untracked` files collapsed directories separately (`engine.rs:7009-7031`) and `report_collapsed` announces them with `tracing::warn!` (`engine.rs:7041-7057`) — never `self.warn(...)`, which is what puts a sentence on the profile card. The set is never pruned, deliberately, so it is said once per process.

Why: git reports a nested repository as one collapsed directory and keeper cannot stage it, so that folder and everything under it never syncs — not as a gitlink, not as content. Defensible, but invisible: the sibling `report_unreadable` (`engine.rs:7060`) *does* raise a UI warning for a comparable "stepped over your content" condition, on the stated principle that *"a folder that syncs while quietly omitting a file is the one outcome worse than a folder that stops"* (`engine.rs:7180-7183`). `docs/sync.md` never mentions nested repositories at all.

Fix: route the first sighting through `self.warn(&profile.id, &profile.name, …)` as well as the log, and add one line to `docs/sync.md` §4 or §20.

### F-GATE-11 `EventKind::Other` never reaches `fold_batch`, so `classify`'s "FSEvents vague kinds" branch and its test are unreachable — P2, effort S

Evidence: keeper asserts the branch matters —
```rust
// watch.rs:986-989
// FSEvents' deliberately vague kinds must not be discarded, or macOS
// would sync nothing at all.
assert_eq!(classify(&EventKind::Any), Interest::Touch);
assert_eq!(classify(&EventKind::Other), Interest::Touch);
```
but the debouncer drops it first: `EventKind::Other => { /* ignore meta events */ }` (`notify-debouncer-full/src/lib.rs:325-327`), reached only after the `need_rescan()` early return. And `EventKind::Any` is emitted by FSEvents *only in imprecise mode* (`fsevent.rs:128-132`), which notify never selects because it always sets `kFSEventStreamCreateFlagFileEvents` (`fsevent.rs:301`).

Why: the test reads as coverage of the macOS path and is not. Practical exposure is small — with `FileEvents` set, real changes arrive as `Create`/`Modify`/`Remove` — but the module's stated macOS safety net is a comment, and a future `notify` that emits `Other` for a real change would be dropped one layer below where anyone is looking.

Fix: rename the test to say it tests `classify` alone — the file already does this honestly for `EchoSuppressor` (`watch.rs:892-906`) — and note in the module doc that the debouncer, not `classify`, is the last filter for `Other`.

### F-GATE-12 Every process start is blind to what changed while it was down, and FSEvents will tell us — P2, effort M

Evidence: notify hardcodes `since_when: fs::kFSEventStreamEventIdSinceNow` (`fsevent.rs:299`) and exposes no way to set it. The compensating mechanism is the paced walk plus `poll_walk_policy`'s "first poll of a run" rule (`engine.rs:1482-1486`).

Why: FSEvents' whole differentiator over inotify is the persistent per-volume journal — a stored `FSEventStreamEventId` plus the volume identity lets a syncer ask "what changed since I last ran" and get an answer in milliseconds rather than a 155 626-entry walk. keeper already has both prerequisites: a durable per-profile store (`sync.db`) and a volume identity (`volume.rs`'s marker ULID and `profile.volume_id`). This is the largest available win in this lane for the 150 k-file target, and it is a `notify` limitation rather than a keeper bug.

Fix: not adoptable as-is — it needs `FSEventStreamCreate` with a stored `sinceWhen` plus `FSEventsGetCurrentEventId`, i.e. an upstream `notify` change or a small macOS-only watcher of keeper's own. Record the shape now; note that `kFSEventStreamEventFlagEventIdsWrapped` and a volume-identity change must both force a full walk.

### F-GATE-13 The walk's INFO line cannot answer "what are those 26 entries?" — P3, effort S

Evidence: `entries` is the raw yielded-item count and `scanned` the stat count (`git/repo.rs:1901-1903`, `watchdog.beats()` at `:1407`), and the line prints `added`/`modified`/`deleted` but **not** `untracked`. Under `WalkPolicy::tracked_only` — the steady state when a watcher is live (`engine.rs:1518-1528`) — `dirwalk_options` is `None` (`git/repo.rs:1745`), so no `DirectoryContents` item can be produced at all.

Why: with `added=modified=deleted=0` and no dirwalk, the 26/65 items can only be statuses `push_item` files into nothing — `EntryStatus::NeedsUpdate`, `IntentToAdd`, or a conflict (`git/repo.rs:1568-1572`). `NeedsUpdate` is the only one that recurs, and also the one that costs: it means the index's cached stat did not match and gix had to compare content. `persist_observed_stats` writes those back (`git/repo.rs:1921-1934`), so the alternation 26 / 65 / 0 is consistent with a set written back and re-dirtied — most plausibly the LFS-tracked paths `is_false_modification` exists to excuse (`engine.rs:7238`). **[INFERENCE]**: not provable from source, which *is* the finding — the log omits the field that would prove it. Separately, `file_state` holding 0 rows is consistent and correct: every candidate that round was `Excluded` or immediately `Stable`, and both leave no entry.

Fix: add `untracked = out.untracked.len()` and `needs_update = <entries_to_update>` to the `status walk finished` line. Two fields, and the next field report answers itself.

### F-GATE-14 Doc drift: tier 3 promises file descriptors it never had; `openfiles.rs` is not what the tier table implies — P3, effort S

Evidence: `docs/sync.md:135` — *"3 — open-writer veto | Linux only: `/proc/locks`, optionally open file descriptors"*. `stability.rs:699-737` reads `/proc/locks` and nothing else, and `openfiles.rs:1-30` is a `RLIMIT_NOFILE` raise for launchd-launched apps, not an open-file probe. §20 (`docs/sync.md:3000-3031`) lists *"macOS has no open-writer veto"* but not "tier 3 is never consulted anywhere" (F-GATE-2).

Why: §4 is the page a reader uses to decide how far to trust the gate, and two of its five rows currently overstate what runs.

Fix: strike "optionally open file descriptors"; state tier 3's and tier 4's true call sites (or wire them, per F-GATE-2) before the table claims them.

## The per-path gate, as a state machine

```mermaid
stateDiagram-v2
    [*] --> Unseen: path named by a walk or a watch event
    Unseen --> Excluded: is_excluded (tier 0, stability.rs:551)
    Excluded --> [*]: invisible - never staged, queued or counted
    Unseen --> Dataless: SF_DATALESS (stability.rs:571) - entry forgotten, user warned
    Unseen --> Vanished: FileSample::of returns None (stability.rs:588) - deletion staged unconditionally
    Unseen --> Held: stat failed - fail closed, no entry recorded
    Unseen --> Settling: first sample - Entry{unchanged_since=now, pending_since=now}
    Settling --> Settling: sample changed - unchanged_since=now, close_write cleared (pending_since NOT reset, F-GATE-3)
    Settling --> Settling: sample equal, window not yet elapsed
    Settling --> Stable: unchanged >= W AND mtime age >= W (stability.rs:458-475)
    Settling --> Stable: now - pending_since >= 60s ceiling (stability.rs:446) - forced
    Unseen --> Asserted: note_finished / prime_stable via declare_settled (stability.rs:405)
    Asserted --> Stable: ceiling condition already true on the very next verdict
    Stable --> [*]: entry dropped (stability.rs:604), path handed to the commit path with NO tier-3 veto and NO tier-4 verify (F-GATE-2)
    Settling --> [*]: gate.retain() prunes it if the next walk did not observe it (engine.rs:7272)
```

`W` = `effective_settle_ms`: 1 s after a Linux close-write, 5 s default, **10 s on a removable profile that never pinned its own value** (`profile/mod.rs:1089-1095`) — so hesperia's profiles, whose stored `settleMs` *is* the default 5000, actually run a 10 s window. Ceiling is 60 s and outranks everything (`profile/mod.rs:140`). That is the answer to "does a forever-appended log ever commit?": yes, every 60 s, and `is_stable` forgetting the entry is what stops it firing more often.

## Event → what the engine does

| notify / FSEvents event | reaches `fold_batch`? | `classify` | Engine effect |
| --- | --- | --- | --- |
| `Create(File/Folder/Other)` | yes | `Touch` | wake set unless tier-0 excluded; sets `untracked_appeared` if the path is not in the index → next walk is `WalkPolicy::full()` |
| `Modify(Data(*))`, `Modify(Metadata(Ownership/Extended/Any/Other))` | yes | `Touch` | wake set; walk stays `tracked_only` if the path is indexed |
| `Modify(Metadata(AccessTime))` | yes | `Ignore` | nothing — our own verify-on-read must not retrigger the gate (`watch.rs:262`) |
| `Modify(Name(From/To/Any))` | yes (debouncer splits `Any` on `exists()`, `lib.rs:301-311`) | `Touch` | both sides wake; no correlation attempted — `NoCache` is deliberate, `git status` decides. Rename→one-commit is handled out of band by `prime_moved_paths` (F-GATE-7) |
| `Modify(Name(Both))` (Linux) | **no** — debouncer ignores it and relies on `To`/`From` (`lib.rs:305`) | n/a | covered by the split events |
| `Remove(*)` | yes | `Touch` | wake set; the walk reports the deletion and `staged.deleted` takes it **unconditionally**, gate not applied (`engine.rs:7265`) — see F-GATE-1 |
| `Access(Close(Write))` (Linux `IN_CLOSE_WRITE`) | yes | `CloseWrite` | `gate.note_close_write` → window drops to 1 s. Never authorises a commit alone |
| `Access(*)` other (open, read, close-read) | yes | `Ignore` | nothing |
| `EventKind::Other` **without** the rescan flag | **no** — dropped by the debouncer (`lib.rs:325`) | (`Touch`, unreachable) | nothing — F-GATE-11 |
| `EventKind::Any` | yes if produced | `Touch` | wake; FSEvents produces it only in imprecise mode, which notify never selects (`fsevent.rs:128`) |
| `Flag::Rescan` — Linux `IN_Q_OVERFLOW` (pathless `Other`), macOS `MustScanSubDirs` (`fsevent.rs:116`) | yes; checked **before** the kind match and before `paths` (`watch.rs:338`) | n/a | root-path event emitted (`watch.rs:355`); the root is never excluded and `strip_prefix` yields `""`, which the index cannot hold → `appeared = true` → full walk. Debouncer keeps only the latest rescan and restarts its timer (`lib.rs:282-286`) |
| FSEvents `ROOT_CHANGED` / `MOUNT` / `UNMOUNT` | → `Modify(Name(From))` / `Create(Other)` / `Remove(Other)`, so yes | `Touch` | one wake. `ROOT_CHANGED` is in practice never delivered: it needs `kFSEventStreamCreateFlagWatchRoot`, which notify does not set (`fsevent.rs:301`). Volume presence is answered by the marker instead (`volume.rs:139`), which is the stronger check |
| `Err(errors)` batch (backend failure, `MaxFilesWatch` after arming) | delivered to the handler | n/a | **one `tracing::warn!` and nothing else** (`watch.rs:473-477`) — no rescan, no demotion, no banner (F-GATE-5) |
| Periodic sweep, every 15 min (`watch.rs:104`) | synthesised root event | n/a | same as a rescan: full walk. Also the only cover for a mount that notifies nothing |
| Arm failure at `start` | n/a | n/a | `ProfileWatch::Failed`, degraded banner naming the sysctl (`watch.rs:701`), retried every 60 s (`engine.rs:735`) |

## What is correct / well done

- **`need_rescan()` is checked before the kind match and before `paths`** (`watch.rs:338`), with a test asserting the Linux overflow event is pathless (`watch.rs:928-947`). This is the bug almost every watcher integration ships, and it is deliberately closed.
- **`NoCache` is named rather than inherited.** `RecommendedCache` is `NoCache` on Linux and `FileIdMap` elsewhere, so a green Linux suite would have hidden a `WalkDir` of 154 765 entries at every arm; the `Backend` doc says exactly that, with the measurement.
- **`retire` never blocks a caller** (`watch.rs:639`), citing notify's FSEvents `while CFRunLoopIsWaiting(...) { yield_now() }` spin by file and line, with a test that fails if a `join()` is reintroduced (`watch.rs:1053-1075`).
- **Tier 0 filters the wake, not only the walk** (`engine.rs:3981`), against the *same compiled set* the walk asks rather than a second copy — the fix for the `node_modules`-drives-a-walk-per-second pathology, applied at the seam where the decision is made.
- **The `Stable`-consumes-the-episode hazard is fixed at the place that fixes it.** `scan_and_enqueue` commits rather than merely looking and says why (`engine.rs:12827-12836`), with `reporting_stable_ends_the_episode_so_the_ceiling_cannot_fire_repeatedly` (`stability.rs:1102`) as the regression test. The memory of "a scan that walked and discarded destroyed the evidence" is closed and covered.
- **`next_stable_ms` / `stable_at_ms` / `tracked` share one definition** (`stability.rs:509-527`), after a divergence made a closed recording report as still being written; the count and the deadline can no longer disagree.
- **`volume.rs` is uniformly biased toward not destroying the marker**: existence test not a parse (`:139`), corrupt marker is an error not an overwrite (`:158`), newer schema preserved verbatim with a byte-comparing test (`:462`), atomic temp + `sync_all` + rename, `0644` for cross-uid removable media (`:400`).
- **`detect_mount_root` refuses a missing path and the filesystem root**, and `adopt_volume` additionally refuses the volume keeper's own data dir lives on (`engine.rs:5451-5470`) — the macOS data-volume case a `/` check alone would miss. Volume swap at the same path is `Foreign` → `NeedsAttention`, never silent adoption.
- **`prime_worktree_changes` is gated on `GitEngine::Gix`**, which is `cfg!(target_os = "ios")` (`git/cli.rs:118-122`), so "only a phone may declare its own tree settled" is enforced by the build rather than by caller discipline.
- **No `std::sync::Mutex` is held across an `.await` in this lane.** `fold_watch_events`, `collect_stable_changes`, `prime_moved_paths`, `note_finished_path`, `volume_ready`, `ensure_watcher` and `drain_finished_assertions` are synchronous, and every one that writes SQLite drops the gate lock first (`engine.rs:7274`, `:4277`, `:4428`). `Self::lock` is poison-tolerant by design (`engine.rs:1546`). The three channels are correctly asymmetric: unbounded watcher→engine (argued at `watch.rs:449` — a bounded one would either block the debouncer thread, causing the overflow it is meant to survive, or drop silently), bounded-lossy `watch_tap` (1024, `Lagged` means rescan), bounded-drop `finished_tap` (64, `try_send`, never blocks the capture path).

## Open questions I could not settle

1. **What the 26/65 entries actually are.** Narrowed to `NeedsUpdate` / `IntentToAdd` / conflict (F-GATE-13), most plausibly LFS-tracked racily-clean paths; not provable from source, and the log omits the field that would prove it. One `tracing` field closes this permanently.
2. **Which of the three `scan_due` reasons drove the 666 walks in hour `2026-09-08T21`.** The paced backstop (15 s) plus the poll (≤1/min) account for ~300/h, so ~366 came from `watch_wake` or `settle_window_elapsed` — yet hesperia's recording segments are written as `*.partial`, which tier 0 excludes (`exclude.rs:70`), so the wake should not have fired for the segment itself. Candidates I could not separate: segment-rotation renames onto final names, sidecar/manifest writes beside the segment, and directory-level FSEvents. Logging the reason in `scan_due` would answer it and costs one enum.
3. **Whether a mid-walk detach really produces `deleted = <every entry>` rather than a hard failure** (F-GATE-1). The guard is missing either way; only the blast radius is unverified. Reproducible on hesperia with a loop device or a spare stick.
4. **Whether `/Volumes/merope` carries a working `.fseventsd`.** If not, all three profiles have been on the paced backstop alone for the whole capture window and `ProfileWatch::Live` is a lie (F-GATE-5). One `ls /Volumes/merope/.fseventsd` plus one `touch` test on the box settles it.
5. **`core.fsmonitor` / `core.untrackedCache`** — out of scope here as walk cost (ScanPerf), but note the adjacency: keeper already runs the one long-lived process that could *be* the fsmonitor hook for these repositories, and F-GATE-12's FSEvents journal is the same mechanism seen from the other end.
