# Brainstorm Intent — Recording destinations, sync integration, processable archive

Source: `_bmad-output/brainstorming/brainstorm-recording-sync-archive-2026-08-05/.memlog.md`
Session: autonomous ("Ideate for me"), 101 ideas / 7 techniques, converged by affinity clustering then MoSCoW.

## Intent

A `keeper` recording session must stop being a folder the app writes and forgets, and become an
addressable, self-describing record whose path, durability, searchability and meaning all derive from
one session identity. Concretely: date-first session folders nested by year under a configurable root,
recordings bound to a sync profile and subfolder (tgdrive), and each finalized segment reaching the
drive promptly instead of waiting on a quiescence timer. The unlocking insight is immutability — a
rotated segment is not "probably done", it is finished forever, which is a stronger claim than any
settle window can make, so syncing during recording is safe rather than a hack. Naming is a retrieval
feature, sync binding is durability, and the tag/note/archive layer is what makes the pile usable in
year three.

## Grounding — existing seams (verified by read-only scouts)

- Session folder is created host-side in Rust, in `keeper/src/ipc.rs` `recording_start`.
- The Swift sidecar only derives sibling segment paths from the one absolute path it is handed — so
  naming and nesting are a pure Rust change.
- `recording.destination_dir` already exists end to end: `keeper.db` settings k/v →
  `RecordingSettingsVm.destinationDir` → `RecordingDestinationControls`. The configurable ROOT is done;
  the NAME PATTERN is not.
- `SegmentClosed{index,path,bytes,track,ptsStart,ptsEnd}` lands in the driver sink in `ipc.rs` after the
  Swift writer's `finishWriting`; nothing is notified — that closure is the one seam for sync-on-finalize.
- `SyncProfile` already carries `notes: Option<NotesConfig>{subfolder, journal_template, cadence}`, and
  `Engine::watch_tap` already fans watcher events out to `notes_vault::start` — recordings should copy
  that shape, not invent one.
- No "this path is finished, sync it now" API exists: `StabilityGate::is_stable` is the only gate, and
  `note_close_write` (1 s window) is private and Linux-only.

## Scope — Must

- Path template rendering the whole relative path, with `{yyyy}` nesting in the default template.
- Recordings bound to a `SyncProfile` + subfolder (`profile.recordings.subfolder`), validated against
  overlap and escaping subpaths the way `NotesConfig::validate` already refuses `.obsidian`.
- `.partial` suffix while writing + atomic rename on close, so an in-progress file is never committed.
- Producer assert-finished so a closed segment syncs without waiting on quiescence.
- Commit immediately, push on a policy that never fights the meeting's uplink.

## Scope — Should

- A `recordings` table + FTS beside the message archive, plus a browser that is really an archive query
  (filter by tag, participant, date range, sync state).
- Note stub written at finalize, prefilled with title, date, participants and tags, saved next to the
  session and synced with it — the post-meeting minute is the only moment the user will type.
- One hierarchical tag vocabulary shared with notes (`notes/tags.rs` `TagTree`, `tag:` predicate), so a
  recording tagged `client/acme` appears beside a note tagged `client/acme`.
- Rename that moves the folder, rewrites the manifest and `git mv`s in one operation.

## Scope — Could

- Retention/prune of local media, pruning only objects the remote verifiably has.
- Per-tag routing of a session to a different sync profile (client repo vs personal, stays local).
- Chapters derived from the segment ledger's existing `ptsStart`/`ptsEnd`.
- Linking a session id to a Matrix room so transcript, chat and video share one timeline.

## Out of scope (won't, this pass)

- Transcription or any AI processing.
- Cross-device dedup.
- A public publishing lane.

## Key design decisions

- **Path token template, not booleans.** `{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}` renders the whole
  relative path; year nesting falls out of the DEFAULT TEMPLATE rather than a flag, and month depth
  (`{yyyy}/{mm}`) becomes an opt-in template edit. The template belongs to the destination, not the app,
  so a synced root and a local scratch root can differ.
- **Reuse the keeper-sync token vocabulary.** Same tokens as
  `DEFAULT_JOURNAL_TEMPLATE = journal/{yyyy}/{yyyy}-{mm}-{dd}.md` — one convention across notes and
  recordings. Time renders as `1432` / `14-32`, never `14:32` (illegal on exFAT; removable-volume sync
  makes a FAT pendrive a real destination). An untitled session collapses `{slug}` with no trailing
  separators and no "Untitled" litter.
- **`.partial` suffix + atomic rename is the completeness signal.** Tier-0 name exclusion already ignores
  it, and the exclusion must be the SUFFIX rule (cheap and total) because git sees a rename as add+delete.
- **Producer assertion, not a shorter timer.** `StabilityGate::note_finished(path)` lets an authoritative
  producer mark a path complete and skip the settle window. This is SAFE because a rotated segment is
  immutable forever — "finished" is a strictly stronger claim than "quiescent", not a weakened gate.
  Authority is narrow: only the producer owning the writer may assert, taking a path the recording
  driver just closed, never user input. This generalises the Linux `IN_CLOSE_WRITE` fast path
  (`note_close_write`, 1 s) from an OS signal to a first-class producer signal, macOS included.
- **Commit now / push on policy.** Commit + LFS-stage locally at once (cheap, durable on the same disk),
  defer the push to a bandwidth window or session end, so a 2 GB LFS object never eats the uplink the
  meeting is running on. Metadata always syncs; media syncs by policy (LFS `PointerOnly` on the laptop).
- **Append-only ledger vs write-once manifest.** `segments.ndjson` grows one line per rotation and never
  rewrites; `manifest.json` is written once at finalize. Rewriting the file the sync engine just
  committed would cause churn and self-echo. The ledger + archive row are the durable record; the
  manifest is a cache of it — stable, documented, versioned from day one, and never holding absolute
  paths, since a synced archive is opened on another machine by definition.
- **Immutable session id vs mutable display title.** Git wants stable paths, the user wants a friendly
  editable name; the id lives in the manifest and never changes, so a rename is a `git mv` of a folder
  whose identity is unchanged and the archive row keeps pointing at the same session. Session ids must
  be device-scoped.

## Risks (from Chaos Engineering)

- Two machines recording into the same synced folder collide on the same minute unless session ids are
  device-scoped; clock skew also mis-sorts a date-first name, so stamp both local time and a monotonic
  device sequence.
- The sync engine's own commit touches `.gitattributes` when the first mp4 appears — write the LFS rule
  at session start, or the working tree changes under the recorder mid-recording.
- Resumable LFS *upload* is the untested leg: a 40 GB day dying at 90%, or a battery dying with segments
  committed but not pushed, must resume via the `lfs_uploads_outstanding` gate before any new pointer is
  published.
- A push rejected by a protected branch must still mark the session locally durable — the tray says
  "recorded, not pushed", not a generic error.
- A pendrive unplugged mid-push pauses without deleting (absence semantics), but a recording still
  writing into an absent volume must fail loudly, never silently buffer; likewise a pull must never
  delete an active session folder.
- A folder renamed in Finder mid-recording splits a session (driver holds absolute paths) — detect via
  inode and refuse or follow; and a corrupted segment caught by verify-on-read must be quarantined with
  the session marked degraded, not silently synced.
