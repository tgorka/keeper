---
title: "Product inputs — Recording × Sync (Phase 6)"
status: final
created: 2026-08-05
sources:
  - _bmad-output/brainstorming/brainstorm-recording-sync-archive-2026-08-05/.memlog.md
  - _bmad-output/brainstorming/brainstorm-recording-sync-archive-2026-08-05/brainstorm-intent.md
---

# Product inputs — Recording × Sync (Phase 6)

Stakeholder ask, session synthesis, and the numbering spine every Phase 6 document binds to.
This file is the contract between the epics and the stories: **numbers are allocated here and
nowhere else.**

## 1. Stakeholder ask (owner, 2026-08-05)

> Update the recording store to customize not only the main folder but also the folder name; by
> default make it start with the date so it will be sorted, and put it inside a year folder so it
> will not grow too much.
>
> Make it also support sync — choose which folder to sync with, and the folder inside (make some
> good choice for the tgdrive).
>
> Make sure that when recording, the file will be synced after the proper batch is saved on the
> drive.
>
> Look for other possible features relevant to combining recording and sync: I want the recordings
> and the sync archive to enter after (maybe process and integrate with notes and use tags).

## 2. Upstream synthesis

The divergent session (127 logged entries, 101 ideas across 7 techniques, autonomous stance) is in
`_bmad-output/brainstorming/brainstorm-recording-sync-archive-2026-08-05/.memlog.md`; the distilled
input is `brainstorm-intent.md` beside it. Three read-only scouts grounded the session in the code
before a single idea was generated, and their findings are the reason this phase is fifteen stories
and not fifty.

Five verdicts carry the phase:

1. **Naming is a retrieval feature, so it is a template, not a set of switches.** One token
   template renders the whole *relative path*; year nesting is what the default template happens to
   say, and month nesting is a template edit rather than a new option. The token vocabulary already
   exists in `keeper-sync` (`DEFAULT_JOURNAL_TEMPLATE = journal/{yyyy}/{yyyy}-{mm}-{dd}.md`) and is
   reused verbatim — one convention across notes and recordings.
2. **A recordings destination is a `SyncProfile` plus a subfolder**, exactly as a notes vault is
   (`NotesConfig { subfolder, … }`, `SyncProfile::vault_root()`). No second configuration store, no
   parallel path validator, no migration: profiles persist as one JSON blob per row, so
   `#[serde(default)]` *is* the migration (AD-54, reaffirmed).
3. **Immutability, not impatience, is what makes syncing during a recording safe.** A rotated
   segment is not "probably finished" — it is finished forever. That is a strictly stronger claim
   than the quiescence gate's, so the answer is a narrow producer assertion
   (`StabilityGate::note_finished`) that skips the settle window, never a shortened timer and never
   a weakened gate. The Linux `IN_CLOSE_WRITE` fast path (`note_close_write`, 1 s) is the same idea
   already in the tree, restricted to one OS; this generalises it to a first-class producer signal.
4. **Durability is local; publication is a policy.** Commit and LFS-stage the moment a segment
   closes — that is cheap, and it is the durability the owner asked for. Pushing a multi-gigabyte
   object over the uplink the meeting is running on is not, so the push runs on a policy whose
   default is session end.
5. **The archive is the point of the whole phase.** A manifest nobody reads is a dead end; the
   session becomes a row with full-text search, a tag vocabulary shared with notes, and a note stub
   written at the one moment the owner will ever type two sentences of context — the minute the
   recording stops.

## 3. Numbering spine (allocated here; do not renumber downstream)

Prior phases end at FR-124, NFR-30, AD-63, UX-DR44, Epic 39.

### 3.1 Functional requirements — FR-125 … FR-146

| id | requirement |
|---|---|
| FR-125 | The recording session folder name is a **path template** over the tokens `{yyyy} {yy} {mm} {dd} {HH} {MM} {SS} {title} {slug} {seq}`, rendering a *relative path* under the destination root. |
| FR-126 | The default template is `{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}` — date-first, therefore chronologically sorted by name, and nested one level per year. |
| FR-127 | The rendered path is filesystem-safe on APFS, exFAT and NTFS: no `:`, no leading/trailing separator or space, no `..`, never absolute; an empty `{slug}` collapses without leaving a separator or an "Untitled" placeholder. An *interior* component may collapse entirely and take its separator with it; the **final** component may not — a template whose last component is built only from collapsible tokens is rejected at parse (`OptionalLeaf`), because the rendered path *is* the session folder and a vanishing leaf would promote its parent into one. The final component, including any collision ordinal, is capped at 255 bytes and a title is truncated at a character boundary to fit. (Amended 2026-08-06: the original wording was silent on a fully collapsed leaf, which the story 40.1 review correctly escalated.) |
| FR-128 | A rendered path that already exists gains a numeric collision suffix; two sessions started in the same minute never share a folder. |
| FR-129 | The recording settings surface shows the template, its rendered preview for *now*, and rejects an invalid template inline rather than at record time. |
| FR-130 | A `SyncProfile` may be marked as a **recordings destination**: `recordings: Option<RecordingsConfig>` naming the subfolder (default `recordings/`), the media policy and the push policy. |
| FR-131 | The recording destination is either a plain directory (today's behaviour, unchanged) or a recordings-flagged profile plus its subfolder; choosing a profile is one control, and the effective root is shown resolved. |
| FR-132 | A recordings subfolder is validated the way a notes subfolder is: never empty, never absolute, never escaping the profile root, and never overlapping that profile's notes vault. |
| FR-133 | While a segment is being written it carries a `.partial` suffix, and becomes its final name by atomic rename at close; a `.partial` file is excluded from sync by a tier-0 suffix rule. |
| FR-134 | When a segment closes, the recorder **asserts** to the sync engine that this exact path is finished, and the engine may commit it without waiting for the settle window. |
| FR-135 | An assertion is only honoured for a path inside the asserting producer's own session folder; it is never reachable from user input or IPC. |
| FR-136 | A closed segment is committed (and LFS-staged) promptly; the **push** runs on the profile's push policy — `SessionEnd` by default, with `Immediate` and `Window` available. |
| FR-137 | The `.gitattributes` LFS rule covering the session's media extension is written once at session start, never mid-recording. |
| FR-138 | Every session carries a durability state — `local`, `committed`, `pushed`, `verified` — surfaced per session and reduced into the existing tray composition; a rejected push reads "recorded, not pushed", never a generic sync error. |
| FR-139 | A session is a **row**: `recordings` and `recording_segments` tables in `archive.db`, written at session start and completed at finalize, holding the immutable session id, the relative path, times, codec/resolution/fps, per-track segment ledger facts, and the durability state. |
| FR-140 | Session metadata (title, participants, note, tags, custom fields) is full-text searchable through an FTS5 index built the same way the message archive's is. |
| FR-141 | A **Recordings browser** lists sessions from that index, filterable by tag, participant, date range and durability state, and opens a session in Finder or plays it. |
| FR-142 | At finalize keeper writes a **note stub** beside the session — title, date, times, participants, tags and a link back to the session id — as an ordinary markdown file. |
| FR-143 | Recording tags resolve against the **same hierarchical tag vocabulary as notes** (`keeper-core/src/notes/tags.rs`), so `client/acme` means one thing in the app, with completion from the existing tree. |
| FR-144 | A session can be retitled after the fact; the folder moves, the manifest and the row are rewritten, and inside a synced tree the move is performed as a rename git can follow. |
| FR-145 | The session's immutable identity is a device-scoped id that never changes across retitles and moves; the manifest holds no absolute paths. |
| FR-146 | `manifest.json` is written once at finalize and is versioned; the running record is the append-only `segments.ndjson` ledger, one line per closed segment. |

### 3.2 Non-functional requirements — NFR-31 … NFR-35

| id | requirement |
|---|---|
| NFR-31 | Asserting a finished segment adds no measurable cost to the recording path: the sink stays non-blocking and a failed assertion degrades to the ordinary settle window, never to a dropped segment. |
| NFR-32 | A four-hour session (≈48 rotations, ≈24 GB) produces a bounded journal and a bounded commit count, and never rewrites a file the engine has already committed. |
| NFR-33 | The recordings index answers a tag- or text-filtered query over 10 000 sessions within the same budget the message archive holds for its own search. |
| NFR-34 | Recording continues correctly when the destination profile is offline, paused, absent (removable media) or rejected by the remote; durability degrades visibly, capture never does. |
| NFR-35 | Nothing in this phase makes `keeper-core` depend on `tauri` or on `keeper-sync`: `check:core-tauri-free` and `check:core-sync-free` stay green. |

### 3.3 Architecture decisions — AD-64 … AD-73

| id | decision |
|---|---|
| AD-64 | The path template is rendered in `keeper-core` as a pure function of (template, now, title, sequence); the shell supplies the clock and the filesystem. |
| AD-65 | The token vocabulary is shared with `keeper-sync`'s journal template rather than duplicated; one renderer, one documented token set. |
| AD-66 | A recordings destination is `RecordingsConfig` on `SyncProfile`, `#[serde(default)]`, validated at construction by the same rules as `NotesConfig::validate`. |
| AD-67 | `StabilityGate::note_finished(path)` is the producer-assertion API. It is additive to the four-tier gate, not a replacement: tier-0 exclusion and tier-4 verify-on-read still apply, and only the tier-2 quiescence wait is skipped. |
| AD-68 | The assertion crosses subsystems through the existing fan-out seam (`Engine::watch_tap`'s sibling direction), never by giving the recording code a handle to the git layer. |
| AD-69 | `.partial` is the in-progress marker, and its exclusion is a suffix rule in `BUILTIN_EXCLUDES` — cheap, total, and correct under the add+delete git sees for a rename. |
| AD-70 | Push policy lives on the profile, not in the recorder; the recorder asserts facts, the engine decides transport. |
| AD-71 | Recording rows live in `archive.db` beside the message archive, using its migration convention (`PRAGMA table_info` + `ALTER TABLE ADD COLUMN`) and a *separate* FTS5 table — the existing `events_fts` is external-content over `events` and must not be generalised. |
| AD-72 | The recording note stub is an ordinary markdown file written through the notes writer when the destination is a notes-flagged profile, and directly otherwise; recordings never author a second note format. |
| AD-73 | The session id is device-scoped (device id + ULID) so two machines recording into one synced folder in the same minute cannot collide. |

### 3.4 Experience decisions — UX-DR45 … UX-DR52

| id | decision |
|---|---|
| UX-DR45 | The template control shows a live rendered preview of the path that *would* be used right now — the preview is the documentation. |
| UX-DR46 | The destination control resolves to one line of truth: the absolute path recording will actually use, whether it came from a plain folder or a profile plus subfolder. |
| UX-DR47 | Sync is not a second toggle. If the destination is inside a synced profile, recordings sync; the UI states that consequence instead of offering it. |
| UX-DR48 | Durability is stated in the recorder's own words — "on this Mac", "committed", "on the drive", "verified" — not in git's. |
| UX-DR49 | A failed push while recording never interrupts the recording and never raises a modal; it downgrades the durability line and the tray glyph. |
| UX-DR50 | The Recordings browser is a search surface first and a list second: the filter row is above the fold. |
| UX-DR51 | The note stub is offered at stop with the cursor already in it, and dismissing it is one key; a stub the user never touches is deleted rather than left as litter. |
| UX-DR52 | Tag entry for a recording uses the same completion affordance as notes, drawing on the shared tag tree. |

### 3.5 Epics — 40 … 42

| epic | title | binds |
|---|---|---|
| 40 | A recording lands where you can find it | FR-125–FR-129, FR-144, FR-145, AD-64, AD-65, AD-73, UX-DR45, UX-DR46 |
| 41 | A finished segment is already on the drive | FR-130–FR-138, FR-146, NFR-31, NFR-32, NFR-34, AD-66–AD-70, UX-DR47–UX-DR49 |
| 42 | The recordings archive: searchable, tagged, noted | FR-139–FR-143, NFR-33, AD-71, AD-72, UX-DR50–UX-DR52 |

## 4. Out of scope this phase

- Transcription, summarisation, or any AI processing of recorded media.
- Cross-device deduplication of identical sessions.
- A public publishing lane for recordings.
- Retention and local pruning of verified LFS objects, per-tag routing to a second profile,
  chapter marks derived from the ledger, and linking a session to a Matrix room. All four were
  converged as "Could": they need a month of real use before their policy is chosen, and each is
  filed as deferred work rather than guessed at here.

## 5. Licensing pre-clearance

No new dependency is required. The template renderer, the FTS table and the tag lookup are all
built on crates already vendored (`serde`, `rusqlite` with the bundled FTS5 build, `ulid`,
`time`). The cargo-deny firewall is therefore untouched by this phase.
