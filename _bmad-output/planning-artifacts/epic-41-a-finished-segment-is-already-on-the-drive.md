# Epic 41 — A finished segment is already on the drive

status: draft
created: 2026-08-05
altitude: epic
parent: Phase 6 (Recording × Sync), Epic 17 (rotation + ledger), Epic 26 (completeness gate), Epic 34 (sync you can see)
source: `product-inputs-recording-sync-2026-08-05.md` (the numbering spine), the divergent session in
`brainstorm-recording-sync-archive-2026-08-05/`, and a read-only survey of `keeper-sync`'s
`profile.rs`, `exclude.rs` and `engine.rs` against the recording driver sink in `keeper/src/ipc.rs`
binds: FR-130–FR-138, FR-146, NFR-31, NFR-32, NFR-34, AD-66–AD-70, UX-DR47–UX-DR49

## Why this epic exists

The owner's sentence was "make sure that when recording, the file will be synced after the proper
batch is saved on the drive". Everything hard about this phase is in that sentence, and it is not
the plumbing — it is the word *proper*.

keeper already refuses to commit a file it is not sure about, and it is right to: epic 26 built a
four-tier completeness gate precisely because a half-written file committed once is a corrupt file
forever. The gate's tier-2 answer to "is this done?" is quiescence — nothing has changed for
`settle_ms` (5 s by default). Applied to a recording, that is both too slow to feel immediate and,
worse, *epistemically wrong*: a file that has been quiet for five seconds is a file that might be
written again in the sixth.

The rotated segment is a different kind of object. `keeper-rec` closes it with `finishWriting`,
emits `SegmentClosed{index, path, bytes, track, ptsStart, ptsEnd}`, and never touches those bytes
again. It is not quiescent. It is *finished*. That claim is strictly stronger than the gate's, and
it comes from the only component entitled to make it — the process that owns the writer.

So this epic does not shorten a timer or add an exception. It adds an assertion API to the gate
(`StabilityGate::note_finished`, AD-67), narrow enough that only a producer holding a path it just
closed can reach it (FR-135), and it generalises an idea already in the tree: the Linux
`IN_CLOSE_WRITE` fast path (`note_close_write`, 1 s) is the same insight, currently available only
where the kernel volunteers it. macOS records; macOS should get it too.

Two more facts shape the epic:

1. **The seam already exists and is empty.** The `RecordingEvent::SegmentClosed` arm of the driver
   sink in `keeper/src/ipc.rs` writes the ledger and notifies nobody. That closure is the single
   integration point, and there is currently *zero* code anywhere linking recording to
   `keeper-sync` — grep finds only doc comments.
2. **The profile already has the right shape.** `SyncProfile` carries
   `notes: Option<NotesConfig> { subfolder, … }` with `NotesConfig::validate` refusing empty,
   absolute and escaping subfolders, and `SyncProfile::vault_root()` joining it. `RecordingsConfig`
   is that pattern applied a second time (AD-66) — a `#[serde(default)]` field on a JSON blob, so
   the migration is the serde attribute.

### Where we take a position

**Durability and publication are different promises, and the recorder only makes the first.**
Committing a closed segment is local, cheap and immediate; pushing a 2 GB LFS object is neither,
and doing it during the meeting eats the uplink the meeting runs on. So the commit happens at
close and the push happens on the profile's policy, defaulting to session end (FR-136, AD-70). The
UI says which of the two has happened in the recorder's own words, never git's (UX-DR48).

**`.partial` is the only in-progress marker we need.** The alternative — teaching the gate about
recording — spreads recording knowledge into the sync crate. A suffix does the same job with a
tier-0 name rule that already exists as a mechanism, is total (git sees a rename as add+delete, so
the suffix rule must be the thing that hides it), and keeps the sidecar's contract to one line of
change: write `<name>.mp4.partial`, rename on `finishWriting` (FR-133, AD-69).

**Sync is a consequence, not a checkbox.** If the destination resolves inside a synced profile then
recordings sync, and the settings surface states that as a fact rather than offering it as a
choice (UX-DR47). A second "also sync my recordings" toggle would be a second source of truth about
something the destination already decides.

**Capture never degrades.** Every failure in this epic — offline remote, paused profile, absent
pendrive, rejected push, LFS upload dying at 90 % — downgrades *durability* and says so. None of
them stops the recorder, raises a modal, or drops a segment (NFR-34, UX-DR49).

## Stories

### Story 41.1: A Profile Can Say It Holds Recordings
**Rust-only (`keeper-sync`).** Bindings: no. Binds FR-130, FR-132, AD-66.

`SyncProfile` (`keeper-sync/src/profile.rs`) gains `recordings: Option<RecordingsConfig>` behind
`#[serde(default)]`, with `RecordingsConfig { subfolder: String, media: MediaPolicy, push:
PushPolicy }`, `DEFAULT_RECORDINGS_SUBFOLDER = "recordings"`, `MediaPolicy { Materialize,
PointerOnly }` and `PushPolicy { Immediate, SessionEnd, Window { quiet_from, quiet_to } }` with
`SessionEnd` the default. `RecordingsConfig::validate` mirrors `NotesConfig::validate` — never
empty, never absolute, never `..`, never escaping the profile root — and additionally refuses a
subfolder that overlaps that profile's notes vault in either direction. `SyncProfile::recordings_root()`
joins it. `SyncProfile::validate` calls into it, so an invalid config cannot be constructed.
AC: a `sync.db` blob written by 0.6.5 loads with `recordings: None` and no error line; a profile
with `recordings.subfolder = "../evil"` is rejected at construction with a typed `SyncError`, not at
use; a recordings subfolder equal to, inside, or containing the profile's notes subfolder is
rejected naming both; round-tripping a configured profile through the JSON blob preserves every
field including the push policy variant.

### Story 41.2: Choosing Where Recordings Live, Once
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 41.1, 40.2. Binds FR-131, UX-DR46, UX-DR47.

The recording destination becomes a resolved decision rather than a bare path:
`recording.destination_profile_id` joins `recording.destination_dir` in the `keeper.db` settings k/v,
and exactly one of them is in force. `RecordingSettingsVm` carries the choice plus the *resolved*
absolute root and the profile's name; the settings command refuses a profile id that is not
recordings-flagged, and refuses a plain directory that sits inside a synced profile's tree without
being that profile's recordings root (the ambiguous case that would otherwise sync by accident).
`RecordingDestinationControls` offers "a folder" or "a synced folder", and when a synced folder is
chosen it states the consequence — recordings here are committed and pushed by that profile — with
the resolved path on one line. Default choice when the owner has exactly one recordings-flagged
profile (the `tgdrive` case): that profile, subfolder `recordings/`, media `PointerOnly`, push
`SessionEnd`.
AC: `bun run bindings:check` passes; choosing a profile shows the resolved
`<local_path>/recordings` and persists across restart; selecting a non-flagged profile is impossible
in the UI and rejected by the command; pointing the plain-folder option inside a synced tree is
rejected with a message naming the profile it would have collided with; with no recordings-flagged
profile present the surface behaves exactly as it does today.

### Story 41.3: `.partial` While Writing, Final on Close
**Swift sidecar + `keeper-sync`.** Bindings: no. Binds FR-133, AD-69.

`keeper-rec` writes each segment as `<name>.<ext>.partial` and performs an atomic rename to
`<name>.<ext>` immediately after `finishWriting` completes, emitting `SegmentClosed` with the
*final* path. `keeper-sync`'s `BUILTIN_EXCLUDES` (`exclude.rs`) gains a `.partial` suffix rule, as a
suffix rather than a glob over a directory, so a partial file is invisible to `Engine::pending`, to
the commit path and to the activity feed. Startup recovery of orphaned segments (story 17.3) learns
the suffix: a `.partial` left by a crash is finalised or discarded by the existing recovery rules,
never committed.
AC: during a recording, `Engine::pending` for the destination profile never lists a `.partial` path,
asserted while a real rotation is in flight; a killed recorder leaves exactly one `.partial` and no
commit references it; recovery of that `.partial` produces either a finalised segment present in the
ledger or a removed file, and says which; the rename is atomic within the same directory (no copy
fallback) and the emitted `SegmentClosed.path` is the final name.

### Story 41.4: The Gate Learns the Word "Finished"
**Rust-only (`keeper-sync`).** Bindings: no. Binds FR-134, FR-135, NFR-31, AD-67, AD-68.

`StabilityGate` gains `note_finished(path, now_ms)`: an authoritative producer marks one absolute
path complete, and the next `collect_stable_changes` treats it as stable without waiting for the
settle window. Tier-0 exclusion and tier-4 verify-on-read still apply — this skips tier-2 only, and
the module doc says so. `Engine` exposes it as `note_finished_path(profile_id, path)` guarded so the
path must resolve inside that profile's `recordings_root()`; anything else is a typed error and a
`warn` line. The assertion is delivered through the same fan-out direction as `watch_tap` rather
than by handing the recorder an `Engine` handle (AD-68). An assertion for an unknown profile, a
disabled profile, or a path outside the recordings root degrades to the ordinary settle path — never
to an error the recorder must handle (NFR-31).
AC: a file written and immediately asserted is committed on the next tick without the 5 s settle
wait, proven by a test that fails if the tick count exceeds one; a path outside the recordings root
is refused and never becomes stable early; an excluded path (`.partial`) that is asserted stays
excluded; asserting the same path twice is idempotent; with the profile paused, the assertion is
recorded and honoured when it resumes rather than lost.

### Story 41.5: Committed at Close, Pushed on Policy
**Rust-only (`keeper` shell + `keeper-sync`).** Bindings: no. Depends on 41.2, 41.3, 41.4. Binds FR-136, FR-137, FR-146, NFR-32.

The `RecordingEvent::SegmentClosed` arm of the driver sink (`keeper/src/ipc.rs`) appends its line to
the append-only `segments.ndjson` ledger and asserts the finished path to the destination profile;
`manifest.json` is written once at finalize (FR-146), so the engine never re-commits a file it just
committed. The engine's `.gitattributes` LFS rule for the session's media extension is written at
session start (FR-137) — not on first commit — so the working tree does not change under a running
recorder. Push obeys `RecordingsConfig::push`: `Immediate` pushes per commit, `SessionEnd` pushes
once the finalize event lands, `Window` defers to the quiet hours. LFS staging is unchanged and
automatic (`lfs::stage::applies`, 4 MiB threshold), and `do_push`'s outstanding-object gate
(`lfs_uploads_outstanding`) continues to refuse publishing a pointer ahead of its object.
AC: a four-hour synthetic session (48 rotations) produces 48 commits, one `.gitattributes` write, one
`manifest.json` write and a bounded journal, asserted by counters not by inspection; with
`push = SessionEnd` no push occurs until finalize, and exactly one push does; with the remote
unreachable throughout, every segment is committed locally and the outstanding push drains on
reconnect without republishing pointers ahead of objects; a segment closed while the profile is
`MediaAbsent` is committed when the volume returns and never deleted in the meantime.

### Story 41.6: Durability You Can Read
**Rust + frontend.** Bindings: **yes**. Depends on 41.5. Binds FR-138, NFR-34, UX-DR48, UX-DR49.

Every session carries a durability state — `local`, `committed`, `pushed`, `verified` — derived from
the ledger and the engine's own knowledge, streamed to the recording surface and reduced into the
existing tray composition (recording still wins the icon; sync never forces presence). The active
recording banner gains one honest line: "on this Mac" → "committed" → "on the drive". A push
rejected by the remote reads "recorded, not pushed" with the reason available, never a generic sync
error and never a modal; the recording continues.
AC: `bun run bindings:check` passes; the banner's line advances through the states during a real
rotation and never regresses; killing the network mid-session leaves the banner at "committed" and
the tray in its warning glyph while recording continues to disk; a protected-branch rejection shows
"recorded, not pushed" and the session's state stays `committed`; on a build without the recording
capability none of this renders at all.

## Out of scope

- The archive row, the search index, the browser, the note stub and tags — all of epic 42.
- Retention or pruning of local objects, however tempting `PointerOnly` makes it: deferred by the
  MoSCoW verdict until there is a month of real usage to choose the policy from.
- Per-tag routing of a session to a second profile.
- Making `verified` mean a remote-side content check beyond what `Engine::verify` already does.
- Resumable LFS *upload*. It is a real gap named in the session's risk list, but it belongs to the
  LFS epic that owns the transfer adapter, not to the recorder that hands it a file.
