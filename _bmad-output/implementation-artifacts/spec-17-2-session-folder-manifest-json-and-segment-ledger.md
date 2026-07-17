---
title: 'Session Folder, manifest.json & Segment Ledger'
type: 'feature'
created: '2026-07-17'
status: 'done'
baseline_revision: 'aebd8a7acb817ec91dd52effb86f9f7e608d8fed'
final_revision: '5ada2c8'
review_loop_iteration: 1
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-17-context.md'
  - '{project-root}/docs/project-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Story 17.1 rotates a recording into multiple segment files and emits an enriched `segmentClosed{index,path,bytes,track}` per closed segment, but nothing turns those files into a self-describing session. Segments land loose in `~/Movies/keeper/`, the final segment (closed by `finalized`, never a `segmentClosed`) is nowhere recorded, and there is no manifest an external tool — or keeper's own future recovery — can read for a consistent picture.

**Approach:** Give `keeper-core::recording` ownership of a per-session folder `keeper-rec <local timestamp>/` containing the segment files plus an atomically-written `manifest.json` (capture target, devices, segment list, status), fed by a segment ledger that consumes the sidecar's `segmentClosed`/`state` events. On clean stop, reconcile the ledger against the folder's on-disk `.mp4` files so the manifest is complete (final segment + any `segmentClosed` the sidecar suppressed while stopping). No Swift change — the sidecar already emits the enriched events and writes segments into a given path.

## Boundaries & Constraints

**Always:**
- Write `manifest.json` **only** by writing a sibling temp file in the session folder then `std::fs::rename` over `manifest.json` (same directory → atomic on APFS); an external reader must never observe a torn or partial file.
- Session folder name is `keeper-rec <local timestamp>` (dots, not colons — filesystem-safe); segment files are `screen-####.mp4` (zero-padded, lexicographically = chronologically ordered). Folder name derivation + fs-safety validation live in `keeper-core::recording`.
- `keeper-core::recording` stays free of the firewall-banned tokens (`tauri`, `objc`, `objc2`, `ScreenCaptureKit`, `AVFoundation`, `CoreGraphics`, `tokio::process`, `std::process`); `std::fs` **is** permitted and used for the atomic write + reconcile. `dependency_firewall_holds` must still pass (it scans the whole file, tests included → no `std::process` even in test helpers).
- Manifest `status` maps non-terminal states (`Idle`/`Preflight`/`Recording`/`Rotating`/`Stopping`) → `"recording"`, `Finalized` → `"finalized"`, `Recovered` → `"recovered"`, `Failed` → `"failed"`. On clean Stop the persisted status transitions `recording → finalized`.
- At finalize the manifest's segment list is **authoritative from disk**: reconcile by scanning the folder so it includes the final segment (no `segmentClosed`) and any middle segment whose `segmentClosed` was suppressed by a stop landing mid-rotation (DW-992), sorted by index.
- Core is **time-agnostic**: the local-timestamp string is supplied by the shell (`chrono::Local`), never generated in `keeper-core`.
- The ledger stores each segment's **basename** (relative to the folder), so the manifest is portable and self-describing.

**Block If:**
- Feeding the ledger would require the sidecar to change its `segmentClosed`/`state` wire shape beyond 17.1's already-shipped enriched fields, or a `keeper-rec` `PROTOCOL_VERSION` bump — HALT `blocked` (that reopens 17.1's locked contract; this story is Rust-only).

**Never:**
- No startup/crash recovery, stale-`recording`-manifest scanning, or `recovered`-entry semantics (Story 17.3) — 17.2 only writes the schema and the clean `recording → finalized` path, and seeds the `recovered`/`failed` status mapping.
- No settings persistence / `keeper.db` reads (17.5); no concat-assert CI gate (17.4); no camera/microphone tracks or recovery-notice UI (Epic 19/20). `track` stays `"screen"`; devices `microphone`/`camera` are constant `false`.
- No change to `keeper-rec` Swift capture code or its protocol version.
- No new third-party crate (license firewall) — `std` + existing `serde`/`serde_json` only; **no** `tempfile` crate (use a same-dir temp name via `std::fs`). Do not block the runtime on large writes: the manifest is a few KB written a handful of times, so a synchronous `std::fs` write is acceptable — never stream media through it.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Session start | `recording_start`, local ts `"2026-07-17 14.23.45"` | `<base>/keeper-rec 2026-07-17 14.23.45/` created; `manifest.json` written atomically: status `"recording"`, captureTarget `{kind:"display",displayId:null}`, devices `{systemAudio:true,microphone:false,camera:false}`, `segments:[]` | Folder/manifest write failure → `RecordingError::ManifestIo`, session surfaces `failed` |
| Segment closes | `{"event":"segmentClosed","index":0,"path":".../screen-0000.mp4","bytes":N,"track":"screen"}` | Ledger appends `{index:0,file:"screen-0000.mp4",bytes:N,track:"screen"}`; manifest re-written atomically | Write failure surfaces as error; prior manifest intact |
| Clean stop / finalized | `state:"finalized"`; folder holds `screen-0000..K.mp4` on disk | Reconcile from disk: every `.mp4` becomes a ledger entry (bytes from `fs::metadata`), sorted by index — incl. the final segment; status `"finalized"`; atomic write | Dir read failure → error; last-known manifest stays on disk |
| Stop during rotation (DW-992) | ledger missing index k but `screen-000k.mp4` on disk at finalize | Disk reconcile backfills index k so the manifest is complete | No error expected |
| Torn-read guard | external reader polls `manifest.json` during a write | Reader always sees a complete, parseable manifest (temp+rename) — pre- or post-update, never partial | temp write fails → rename never happens; old file intact |
| Enriched parse vs state machine | enriched `segmentClosed` line | `parse_event` → `SegmentClosed{index, path:Some,bytes:Some,track:Some}`; `apply` bumps counter by 1, state unchanged | Bare `segmentClosed` (index only) → `path/bytes/track: None`, still legal |
| Folder-name fs-safety | ts `"2026-07-17 14.23.45"` | `"keeper-rec 2026-07-17 14.23.45"` — no `/`/`:`/control chars, lexicographically ordered | A separator/control char in the string → validation rejects it |
| Failure mid-session | `error` event before finalize | Manifest status persisted as `"failed"` (atomic write) | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/recording.rs` -- Enrich `RecordingEvent::SegmentClosed` with `path/bytes/track` + update `parse_event`; add the manifest subsystem: `SessionManifest` (owns folder path + data), `SegmentEntry`, `ManifestStatus` (serde), `session_folder_name`/fs-safety validation, atomic `write` (sibling temp + `rename`), and `reconcile_from_dir` — which at any terminal **rebuilds the segment list authoritatively from the on-disk `.mp4` files** (disk is the source of truth: bytes always from `metadata().len()`), best-effort (a stray/unreadable entry is skipped, never aborting the terminal write). Keep it firewall-clean.
- `src-tauri/crates/keeper-core/src/error.rs` -- Add a secret-free `RecordingError::ManifestIo(String)` variant for folder/manifest I/O failures.
- `src-tauri/crates/keeper/src/ipc.rs` -- `recording_start`: derive a **unique** session folder (disambiguate on collision — never adopt an existing directory), create it + the initial `recording` manifest, pass `<folder>/screen-0000.mp4` as `SessionParams.output_path`, own the `SessionManifest` in the driver task, update+atomic-write it per event, and reconcile+write on **every** terminal event; a mid-session manifest-write failure is logged only (never flips the live session to `failed`). Set the status VM `output_path` to the folder.
- `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- Unit tests for every I/O-matrix row: manifest serialization shape, folder-name derivation/validation, atomic write + `reconcile_from_dir` (via a unique temp dir under `std::env::temp_dir()` — **no** `std::process`), and enriched `parse_event`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- Enrich `RecordingEvent::SegmentClosed` to `{ index: u32, path: Option<String>, bytes: Option<u64>, track: Option<String> }` and extend `parse_event` to read the optional fields best-effort (index still required); update the one existing seam test to assert the enriched parse and add a bare-`segmentClosed` tolerance test; add `None` fields to existing `SegmentClosed{index}` literals -- carries the ledger data to the shell without a second parse path (17.1 pre-declared these additive). `apply`/`event_label` (matching `{ .. }`) are unchanged.
- [x] `src-tauri/crates/keeper-core/src/error.rs` -- Add `RecordingError::ManifestIo(String)` (secret-free message; never a path/token) and its `#[error(...)]`; it rolls up through the existing `CoreError::Recording` `#[from]` -- honest surfacing of folder/manifest write failures. (Precedes the manifest task, which returns it.)
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- Add `ManifestStatus` + `SegmentEntry` + `SessionManifest` (serde `Serialize`/`Deserialize`) modeling capture target, devices, `segments`, and `status`; `session_folder_name(local_ts) -> Result<String, RecordingError>` (`keeper-rec <ts>`) whose fs-safety validation rejects `/`, `\`, `:`, `NUL`, control chars, a leading dot, **and** an all-whitespace or trailing-`.`/trailing-space timestamp (some filesystems normalize the latter, diverging the folder basename from `manifest.session`); `SessionManifest::create(folder, target, devices)` (create the folder, failing if it already exists so a prior session is never adopted; + initial `recording` write), `record_segment(entry)` (append for the **live** incremental view during recording), `set_status(status)`, atomic `write()` (write `.<name>.tmp` in the folder, `rename` over `manifest.json`), and `reconcile_from_dir()` that at any terminal **rebuilds `segments` entirely from the on-disk `.mp4` files** (index from the stem's numeric run, `bytes` always from `metadata().len()` — the authoritative disk size, never a stale/zero event-fed value), skipping a non-segment or unreadable entry (log + continue, never abort), then sorts by `(index, file)` for determinism. `create`/`write`/`reconcile_from_dir` return `RecordingError::ManifestIo` on a real fs failure (no `.unwrap()`); a skipped stray entry is not a failure -- the schema + ledger + atomic write + disk-authoritative terminal reconcile that make the session self-describing and complete (FR-71, AD-33).
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- In `recording_start`: derive a **unique** folder from `directory` + `session_folder_name(ts)?` — if it already exists, disambiguate (append ` (2)`, ` (3)`, …) so a same-second sequential restart never reuses the prior session's folder; `SessionManifest::create(...)` it + the initial manifest; set `SessionParams.output_path = <folder>/screen-0000.mp4` and the VM `output_path` to the folder. Move the `SessionManifest` into the driver task; in the event sink, after `machine.apply(event)`, on `SegmentClosed` `record_segment` (live view) and on any state change `set_status`, then atomic-`write()`; on **every** terminal (`Finalized`/`Recovered`/`Failed`) call `reconcile_from_dir()` (rebuild from disk) before the final write. A mid-session `write()`/reconcile failure is logged via `tracing::warn` and **must not** change `machine` state or force the snapshot to `Failed` — the capture is still live and the single-child start-guard (which keys off the snapshot) must keep holding; the last good manifest stays on disk -- wires the ledger to live events and completes it from disk at every terminal.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- Unit-test the I/O-matrix rows plus the review-hardened behaviors: manifest JSON shape/round-trip, `session_folder_name` rejection (separator/control/leading-dot/all-whitespace/trailing-dot), atomic `write()` (temp then rename; parseable after), `create` refusing an existing folder, and `reconcile_from_dir` — final-segment inclusion, DW-992 backfill, **disk bytes overriding a zero/stale event-fed size**, a stray non-segment `.mp4` and an unreadable entry skipped without aborting, deterministic `(index, file)` sort, and reconcile driven from the `Recovered`/`Failed` terminals as well as `Finalized`. Use a fresh, uniquely-named temp dir under `std::env::temp_dir()` created/removed with `std::fs` -- proves the schema + reconcile without capture hardware and keeps the firewall (no `std::process`).

**Acceptance Criteria:**
- Given a started recording, when the session folder is created, then `<base>/keeper-rec <local ts>/manifest.json` exists with status `"recording"`, a display capture target, devices `{systemAudio:true, microphone:false, camera:false}`, and an initially-empty segment list, written atomically.
- Given a clean stop, when the session finalizes, then the manifest status is `"finalized"` and its segment list enumerates every `.mp4` in the folder — including the final segment that had no `segmentClosed` and any segment a mid-rotation stop suppressed — sorted by index, each entry `{index, file, bytes, track}`.
- Given an external reader polling `manifest.json` throughout the session, when any segment closes or the status changes, then every read returns a complete, parseable JSON manifest reflecting either the pre- or post-update state — never a partial file.
- Given `keeper-core`'s `dependency_firewall_holds` test, when the crate is tested, then `recording.rs` carries no banned platform/process token (`std::fs` permitted) and the test passes.

## Spec Change Log

### 2026-07-17 — Review pass 1 (bad_spec loopback)

- **Triggering findings:**
  - `[high]` A mid-session manifest-`write()` failure flipped the snapshot to `Failed` while capture was still live; the start-guard keys off the snapshot, so a transient write hiccup could spawn a **second** `keeper-rec` child (violates "never two capture children").
  - `[medium]` `reconcile_from_dir` ran only on `Finalized` and was all-or-nothing: `Recovered`/`Failed` terminals never completed from disk, and one unreadable dir entry aborted the terminal write, leaving `status:recording` stale on disk.
  - `[medium]` The reconcile merge strategy (dedupe by basename, keep event-fed `bytes`) froze a `bytes:0` fallback forever and could double-count a segment when the synthesized `screen-{index:04}.mp4` basename mismatched the real file.
  - `[medium]` Same-second sequential restart reused the prior session's folder (`create_dir_all` succeeds on an existing dir), cross-writing manifests and mixing segments.
  - `[low]` `sort_by_key(index)` was unstable across equal indices; `session_folder_name` accepted whitespace-only / trailing-dot timestamps some filesystems normalize.
- **Amended (outside `<intent-contract>`):** Code Map, Tasks & Acceptance, and Design Notes now specify: `reconcile_from_dir` **rebuilds `segments` entirely from disk** (bytes always from `metadata().len()`) on **every** terminal, best-effort (skip unreadable entries, never abort); a mid-session write failure is logged and never flips the live session to `Failed`; the session folder must be **unique** (disambiguate on collision, `create` refuses an existing dir); `session_folder_name` also rejects all-whitespace/trailing-dot/space; reconcile sorts by `(index, file)`; added tests for these.
- **Known-bad state avoided:** an incomplete or actively-wrong `manifest.json` (missing final/suppressed segment, frozen `bytes:0`, duplicated phantom entry, or two sessions' segments in one folder) — the exact failures the story exists to prevent — plus a double capture child from a benign write hiccup.
- **KEEP (must survive re-derivation):** the enriched `RecordingEvent::SegmentClosed{index, path:Option, bytes:Option, track:Option}` + best-effort `parse_event` + the rewritten seam test and the bare-tolerance test; `RecordingError::ManifestIo` with secret-free messages; the `SessionManifest`/`SegmentEntry`/`ManifestStatus`/`CaptureTarget`/`SessionDevices` schema (camelCase, basename `file`, `MANIFEST_VERSION`, lowercase status); the atomic sibling-temp-then-`rename` write; `session_folder_name` core derivation + fs-safety validation; `segment_index_from_stem`; the ipc.rs folder creation, `screen-0000.mp4` seed, VM `output_path` = folder, and per-event `record_segment` live view; the firewall-clean discipline (std::fs only, no `std::process` even in tests, unique temp dirs via `AtomicU64`). Both gates were green before this loopback — keep them green.

## Review Triage Log

### 2026-07-17 — Review pass
- intent_gap: 0
- bad_spec: 5: (high 1, medium 3, low 1)
- patch: 0
- defer: 0
- reject: 3
- addressed_findings:
  - `[high]` `[bad_spec]` Manifest-write failure flipped the live session to `Failed`, allowing a second capture child via the snapshot-keyed start-guard → spec now mandates a logged, non-fatal write failure that never changes session state.
  - `[medium]` `[bad_spec]` Reconcile ran only on `Finalized` and aborted on one bad entry → spec now rebuilds the segment list from disk best-effort on every terminal (`Finalized`/`Recovered`/`Failed`).
  - `[medium]` `[bad_spec]` Basename-dedupe merge froze `bytes:0` and could double-count on a synthesized-name mismatch → spec now rebuilds `segments` authoritatively from disk (bytes always from `metadata().len()`), event list is a live view only.
  - `[medium]` `[bad_spec]` Same-second restart reused the prior session's folder → spec now requires a unique folder (`create` refuses an existing dir; shell disambiguates on collision).
  - `[low]` `[bad_spec]` Unstable index sort + whitespace/trailing-dot timestamps accepted → spec now sorts by `(index, file)` and hardens `session_folder_name` validation.
  - Rejected (3): `file`-field path-traversal (reconcile uses `read_dir` basenames; sidecar path trusted; no downstream path-join in this story), u32 index overflow dropping a segment (unreachable — 4B+ segments; owned by 17.1's shipped `index:u32` contract), and the "ipc.rs sink untested" process note (the re-derived core tests now cover the reconcile/terminal behaviors).

### 2026-07-17 — Review pass 2 (post-loopback)
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 1, low 2)
- defer: 1
- reject: 7
- addressed_findings:
  - `[medium]` `[patch]` `reconcile_from_dir` ingested *any* `*.mp4` with a trailing digit run (a stray `IMG_1234.mp4`, or a future Epic-20 `camera-####.mp4` sharing the folder), polluting the authoritative screen-track ledger and allowing duplicate indices → now requires the `SEGMENT_STEM_PREFIX` (`screen-`) and matches the extension case-insensitively; the stray/prefix test was updated to assert only `screen-####.mp4` survive.
  - `[low]` `[patch]` `write()` left an orphaned `.manifest.json.tmp` in the session folder when the final `rename` failed → the rename-error path now removes the temp.
  - `[low]` `[patch]` a persistently-failing manifest write warned on every event across an hours-long session → the sink now warns at most once per session (still non-fatal).
  - Deferred (1): Story 17.3 startup recovery must run `reconcile_from_dir` (rebuild the ledger from disk), not merely flip status — a session that ended via a `run_session` error / crash never executed 17.2's terminal reconcile, so its manifest's segment list is incomplete.
  - Rejected (7): stale `recording` manifest on a `run_session` error (by design — it *is* the interrupted-session signal 17.3 keys on; the completeness note is the deferred item above); reconcile `read_dir` failure falling back to the event-fed view (documented best-effort; only reachable when the folder is already gone, where the write fails too); bidi/line-separator and non-ASCII-space timestamps (unreachable — the timestamp is the app's `chrono` `%Y-%m-%d %H.%M.%S`, only digits/`-`/space/`.`); dangling-symlink folder-name collision (extreme; yields an honest start error, no corruption); synthesized live-view basename cross-language coupling (live view only; the terminal reconcile reads real names); parent-dir left behind on a failed `create` (the parent is the intended durable dir); and `file_name()==None → session:""` (unreachable — `create_dir` fails first for such paths).

## Design Notes

Atomic write + ledger flow (all in `keeper-core::recording`, driven from the shell's event sink):
```text
create():   folder must not pre-exist (shell disambiguates on collision); manifest{status:recording, …, segments:[]}; write()
segmentClosed{i,path,bytes}: record_segment({index:i, file: basename(path), bytes, track})  // LIVE view only
state change:               status = map(state); write()
terminal (finalized|recovered|failed): reconcile_from_dir(); status = map(state); write()
write():    tmp = folder/".manifest.json.tmp"; fs::write(tmp, json); fs::rename(tmp, folder/"manifest.json")
```
`record_segment` gives external readers a *live* incremental view during recording, but the event-fed list can be wrong (a suppressed `segmentClosed`, a `bytes:None` fallback, a synthesized basename). So at **every terminal** `reconcile_from_dir` **discards the event-fed list and rebuilds `segments` entirely from the on-disk `.mp4` files** — index from the stem's numeric run, `bytes` always from `metadata().len()` (disk is authoritative; never a stale/zero event value) — then sorts by `(index, file)`. This makes the manifest complete and correct regardless of what the events said: it lists the final segment (no `segmentClosed`), backfills DW-992's suppressed middle segment, and repairs any wrong size. A stray/unreadable dir entry is skipped (logged), never aborting the terminal write. Reconcile runs on `Recovered`/`Failed` too (not just `Finalized`) so every terminal manifest is a truthful record. A mid-session `write()` I/O hiccup is logged and left non-fatal — the session is still live, so it must **not** flip the snapshot to `Failed` (that would defeat the single-child start-guard). Passing `screen-0000.mp4` as the first path lets 17.1's `nextSegmentPath` produce `screen-0001.mp4`… inside the folder with no Swift change.

Example `manifest.json` after a clean 2-segment session:
```json
{ "version": 1, "session": "keeper-rec 2026-07-17 14.23.45", "status": "finalized",
  "captureTarget": { "kind": "display", "displayId": null },
  "devices": { "systemAudio": true, "microphone": false, "camera": false },
  "segments": [ { "index": 0, "file": "screen-0000.mp4", "bytes": 524288000, "track": "screen" },
                { "index": 1, "file": "screen-0001.mp4", "bytes": 133169152, "track": "screen" } ] }
```

## Verification

**Commands:**
- `bun run test:rust` -- expected: new manifest/ledger/folder-name/reconcile tests + the updated enriched-`parse_event` seam test pass; `dependency_firewall_holds` still green.
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` clean (no `.unwrap()` in production paths — manifest I/O uses `?` with `RecordingError::ManifestIo`).

**Manual checks (if no CLI):**
- No Swift/`keeper-rec` change in this story, so `rec:build`/`rec:test` are not required; segment writing into the folder is exercised on signed hardware in later dogfooding.

## Auto Run Result

Status: **done**

### Summary
Gave `keeper-core::recording` ownership of a self-describing per-session folder. `recording_start` now derives a **unique** `~/Movies/keeper/keeper-rec <local ts>/` folder (disambiguated on collision) and creates it with an initial atomically-written `manifest.json` (schema version, session name, `status`, capture target, devices, empty segment list); the first segment path `screen-0000.mp4` seeds 17.1's sidecar rotation so segments land inside the folder with no Swift change. `RecordingEvent::SegmentClosed` is enriched to carry `path/bytes/track` (parsed best-effort; the state machine still only bumps its counter on `index`), feeding a **live** segment ledger the sink writes per event. At **every** terminal (`finalized`/`recovered`/`failed`) `reconcile_from_dir` discards the event-fed list and rebuilds `segments` authoritatively from the on-disk `screen-####.mp4` files (index from the stem, bytes always from `metadata().len()`), so the manifest lists the final segment (which emits no `segmentClosed`), backfills a stop-during-rotation-suppressed segment (DW-992), and repairs any wrong size — best-effort (a stray/unreadable entry is skipped, never aborting the write). Every write is atomic (sibling temp + `rename`) so an external reader never sees a torn file. A mid-session write failure is logged (once) and non-fatal — it never flips the live session to `failed`, preserving the single-capture-child guarantee. `keeper-core::recording` stays firewall-clean (`std::fs` + serde only; no platform/process tokens).

### Files changed
- `src-tauri/crates/keeper-core/src/recording.rs` — enriched `SegmentClosed{index,path,bytes,track}` + best-effort `parse_event`; manifest subsystem (`SessionManifest`, `SegmentEntry`, `ManifestStatus`, `CaptureTarget`, `SessionDevices`, `MANIFEST_VERSION`, `SEGMENT_STEM_PREFIX`), `session_folder_name` fs-safety validation, `segment_index_from_stem`, atomic `write` (temp+rename, cleans temp on rename failure), and `reconcile_from_dir` (disk-authoritative terminal rebuild, `screen-` prefix only); ~30 unit tests.
- `src-tauri/crates/keeper-core/src/error.rs` — secret-free `RecordingError::ManifestIo(String)`.
- `src-tauri/crates/keeper/src/ipc.rs` — `recording_start`: unique session folder + initial manifest, `screen-0000.mp4` seed, VM `output_path` = folder, driver-task sink drives `record_segment`/`set_status`/terminal `reconcile_from_dir`/atomic write, with a non-fatal warn-once write-failure path.
- `_bmad-output/implementation-artifacts/deferred-work.md` — one entry: 17.3 recovery must `reconcile_from_dir` when salvaging a crashed session.

### Review findings breakdown
- **Two reviewers × two passes** (adversarial + edge-case, Opus capability).
- **Pass 1 → bad_spec loopback (5 findings):** manifest-write failure could spawn a second capture child; reconcile ran only on `Finalized` and aborted on one bad entry; basename-dedupe froze `bytes:0` / double-counted; same-second restart reused the prior folder; unstable sort + weak timestamp validation. Spec amended (disk-authoritative rebuild on every terminal, non-fatal write, unique folder, hardened validation, `(index,file)` sort) and the implementation re-derived.
- **Pass 2 → 3 patches applied:** `reconcile_from_dir` now requires the `screen-` prefix (stray/future-`camera-` `*.mp4` no longer pollute the ledger; + case-insensitive extension); `write()` cleans an orphaned temp on rename failure; manifest-write failure warns at most once per session.
- **Deferred (1):** 17.3 startup recovery must rebuild the ledger from disk (a crash/`run_session`-error path never runs the terminal reconcile).
- **Rejected (10 across passes):** `file` path-traversal (trusted sidecar + `read_dir` basenames), u32 index overflow (unreachable; 17.1's shipped contract), stale-`recording`-on-crash (by design — the 17.3 recovery signal), best-effort reconcile fallback, bidi/NBSP timestamps (unreachable — app-generated chrono string), dangling-symlink collision (honest error, no corruption), live-view basename coupling, parent-dir side effect, empty-`session` defensive gap, and "sink untested" process note.

### Verification
- `bun run test:rust` — PASS (861/861; incl. the enriched-`parse_event` seam test, folder-name rejection matrix, atomic write, `create` refusing an existing folder, and the disk-authoritative reconcile across all three terminals + stray-skip/DW-992/final-segment/bytes-override rows; `dependency_firewall_holds` green).
- `bun run check:rust` — PASS (`cargo fmt --check` + `clippy --all-targets -D warnings`, zero warnings).
- No Swift/`keeper-rec` change (the sidecar already emits the enriched events), so `rec:build`/`rec:test` are not required.

### Residual risks
- True end-to-end folder/segment layout on real capture is exercised on dev-signed hardware in later dogfooding; 17.2's gate is compile + the pure/fs unit tests.
- A crashed session leaves a `status:"recording"` manifest with an incomplete segment list by design — Story 17.3 must reconcile it from disk on recovery (deferred entry recorded).
- The reconcile is time-agnostic and does not enforce contiguous/gap-free indices (DW-937 remains 17.x's concern); it lists whatever `screen-####.mp4` files exist.
