---
title: 'Startup Recovery of Orphaned Segments'
type: 'feature'
created: '2026-07-19'
status: 'done'
baseline_revision: '69ac7f85808fe7b4f142eecdad936ed253bd4269'
final_revision: 'b52b05678a9f2598f56cea5fbe89d09005d97f24'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** A recorder/keeper/power crash leaves a session's `manifest.json` frozen at `status:"recording"` with an incomplete, possibly event-fed segment list — the terminal disk reconcile (17.2) never ran. Nothing marks such a session `recovered`, so its salvaged segments are never surfaced and Story 20.3's once-per-session recovery notice has no signal (FR-73, AD-37).

**Approach:** Add a best-effort recovery pass that runs at startup and before each new recording: scan the recordings base dir for sessions whose manifest is still `"recording"`, rebuild each ledger authoritatively from the on-disk `.mp4` files via 17.2's existing `reconcile_from_dir`, mark the manifest `"recovered"`, and atomically rewrite it. The `recovered` manifest on disk is the durable record 20.3 consumes; recovered files play as-is with no remux.

## Boundaries & Constraints

**Always:**
- Salvage runs `SessionManifest::reconcile_from_dir` (authoritative rebuild from disk) then `set_status(Recovered)` then atomic `write()` — never a bare status flip. Disk is the source of truth for the segment list, so a segment a stop-during-rotation `segmentClosed` suppression dropped, or the final segment that emits no `segmentClosed`, is still listed.
- Only act on manifests whose `status == Recording`; a `finalized`/`recovered`/`failed` manifest is terminal — read once and skipped, never rewritten.
- Best-effort and non-fatal everywhere: a missing base dir, an unreadable/corrupt manifest, or a per-folder salvage failure is logged (`tracing::warn`) and skipped, never aborting the scan and never failing startup or blocking `recording_start`.
- The core scan/recover code stays firewall-clean in `keeper-core::recording` (`std::fs` + serde only; no `std::process`, no Tauri, no Apple APIs). Base-dir derivation and thread/task placement live in the shell.
- Recovery is remux-free: it never rewrites or re-encodes the `.mp4` segments — it only rebuilds and rewrites `manifest.json`.
- At startup, run the pass **off the main thread** (a detached thread or async task, mirroring the notification-permission first-boot-hang fix) so a slow/large recordings volume never stalls boot.

**Block If:**
- 17.2's `reconcile_from_dir` / `write` / `ManifestStatus` contract turns out to be incompatible with loading a manifest from disk in a way that cannot be resolved additively (would reopen 17.2's locked schema) — HALT `blocked`.

**Never:**
- Never touch a session whose manifest is not `Recording`; never adopt or delete segment files; never remux.
- Never invent a new manifest schema field or bump `MANIFEST_VERSION` — this story reads/writes the existing shape.
- Do not build the recovery-notice UI, an app-state/settings signal, or a frontend query — Story 20.3 owns surfacing (it scans for `recovered` manifests). Do not add speculative plumbing for it.
- Do not rescan arbitrary historical destination folders — the pass covers the current effective destination dir only (an orphan left in a since-changed destination is out of scope; the files still play).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Stale session, segments on disk | folder with `manifest.json` `status:"recording"` + `screen-0000..0002.mp4` (ledger missing the final/ a suppressed segment) | `reconcile_from_dir` rebuilds `segments` from disk (bytes from `metadata`), status→`recovered`, atomic write; folder returned in the recovered list | No error expected |
| Stale session, crashed before first segment | `status:"recording"`, no `.mp4` files | segments→`[]`, status→`recovered`, written; returned | No error expected |
| Already-terminal manifest | `status:"finalized"`/`"recovered"`/`"failed"` | read once, skipped, file untouched, not in recovered list | No error expected |
| Corrupt / unreadable manifest.json | invalid JSON or unreadable file | warn + skip, file untouched, not recovered | Logged, non-fatal |
| Non-session entry | a stray file, or a subdir with no `manifest.json` | skipped | No error expected |
| Missing / unreadable base dir | base dir absent (first run) or `read_dir` errors | empty recovered list, no-op | Logged (unreadable) / silent (absent), non-fatal |
| Multiple orphans | two stale `recording` folders | both salvaged, both returned; one folder's failure doesn't stop the other | per-folder best-effort |
| Salvage write fails mid-scan | folder vanished / `write()` errors after reconcile | warn + skip that folder, continue scanning | Logged, non-fatal |
| Force-killed fragmented segment (Swift) | a fragmented `.mp4` truncated mid-fragment (un-flushed tail dropped) | `AVAsset` opens it and yields decodable video frames up to the last complete ~4 s fragment, no remux | n/a (assurance test) |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/recording.rs` -- Add `SessionManifest::load(folder: &Path) -> Result<Self, RecordingError>` (read `folder/manifest.json`, deserialize, set the runtime `folder` — the `#[serde(skip)]` field is empty after deserialize, so `reconcile_from_dir`/`write` need it set) and `recover_orphaned_sessions(base_dir: &Path, is_active: &dyn Fn(&Path) -> bool) -> Vec<PathBuf>` (iterate immediate subdirs; **skip symlinked entries** via the `DirEntry` file type; for each `Recording`-status manifest, call `is_active(&folder)` **immediately before** salvaging and skip a reserved/live folder; else `reconcile_from_dir` + `set_status(Recovered)` + `write`; collect recovered folders; best-effort per entry). Firewall-clean — `is_active` is a bare `&dyn Fn`, no shell/Apple types.
- `src-tauri/crates/keeper/src/ipc.rs` -- Add to `AppState` a reserved-live-folder set (e.g. `Mutex<HashSet<PathBuf>>`) and a recovery-scan `Mutex<()>`. `recording_start`: **reserve** the unique session folder (insert into the set) **before** `SessionManifest::create`, and remove it on every exit path (early error, terminal, quit-finalize — the same points that clear `recording_run`); after the destination gate computes `directory` and **before** the collision-suffix loop / `create`, take the recovery-scan lock and call `recover_orphaned_sessions(&directory, &is_active)` where `is_active` locks the reserved set and tests membership (bounded, best-effort, logged). Add a `pub(crate) fn recover_orphaned_recordings(state: &AppState)` shell helper that derives the base via the existing `effective_destination_dir`, takes the same recovery-scan lock, and calls the core pass with the same `is_active` predicate (used by startup).
- `src-tauri/crates/keeper/src/lib.rs` -- In `.setup(...)`, after the app state is available, spawn the startup recovery pass **off the main thread** (detached thread / `async_runtime::spawn`, like the off-thread notification-permission request) calling `ipc::recover_orphaned_recordings(...)`; log the recovered count. A failure never blocks boot. The live-folder reservation + recovery-scan lock (owned by `AppState`) make this thread safe against a concurrent `recording_start`.
- `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- Unit tests for every I/O-matrix row using unique temp dirs under `std::env::temp_dir()` (std::fs only, no `std::process`): stale→recovered w/ disk rebuild (incl. a final/suppressed segment appearing), crashed-before-first-segment, terminal-manifest skip, corrupt-manifest skip, non-session entry skip, missing/unreadable base dir, multiple orphans, a per-folder failure not aborting the scan, **an `is_active`-reserved `recording` folder left untouched (the live-session guard)**, and **a symlinked entry skipped**.
- `tools/keeper-rec/Tests/keeper-recTests/RecoveryTests.swift` (new) -- Induced-kill assurance test (AC2, FR-73): write a fragmented `.mp4` with the `FixtureSegments`/`Capture.swift` writer config (4 s `movieFragmentInterval`, `TempSessionDir` RAII) but **abandon the writer** (no `finishWriting`) to get the real crash shape, stop the writer before snapshotting the settled bytes, then truncate mid-fragment — **asserting the cut actually drops bytes (`cut < fileSize`)** — and assert `AVAsset` opens it and reads decodable, strictly-monotonic video frames up to the last complete fragment yet fewer than the clean control (a clean `writeFixtureSegment` fixture as the positive control). Give the fragment-flush wait a non-flaky budget. No SCK, no signing, no committed media.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- Add `SessionManifest::load` (read+deserialize `manifest.json`, set `folder`; `RecordingError::ManifestIo` on read/parse failure) and `recover_orphaned_sessions(base_dir, is_active: &dyn Fn(&Path) -> bool)`: `read_dir` the base (missing/unreadable → empty vec, best-effort), and for each immediate subdirectory (skip symlinked entries via the `DirEntry` file type) with a `manifest.json`, `load` it; if `status == ManifestStatus::Recording`, call `is_active(&folder)` immediately before salvaging and skip a reserved/live folder, else `reconcile_from_dir()` → `set_status(ManifestStatus::Recovered)` → `write()` and push the folder to the result; any per-entry error is `tracing::warn`+skip (never abort). Returns the recovered folders. Firewall-clean (std::fs + serde + a bare `&dyn Fn`).
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- Add to `AppState` a reserved-live-folder set (`Mutex<HashSet<PathBuf>>`) and a recovery-scan `Mutex<()>`. In `recording_start`, reserve the unique session folder before `SessionManifest::create` and remove it on every exit path (early error, terminal, quit-finalize — mirroring `recording_run` teardown); after the destination gate sets `directory` and before folder disambiguation/`create`, take the recovery-scan lock and invoke `recover_orphaned_sessions(&directory, &is_active)` (an `is_active` closure that locks the reserved set). Add `pub(crate) fn recover_orphaned_recordings(state)` deriving the base from `effective_destination_dir`, taking the recovery-scan lock, and delegating to the core pass with the same predicate (logging the count, swallowing errors).
- [x] `src-tauri/crates/keeper/src/lib.rs` -- In `.setup`, spawn the startup recovery pass off the main thread via `ipc::recover_orphaned_recordings`, logging the recovered count; non-fatal, never blocks boot (the reservation set + recovery-scan lock make it safe against a concurrent start).
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- Cover every I/O-matrix core row (stale→recovered disk rebuild incl. final/suppressed segment surfacing, crashed-before-first-segment, terminal skip, corrupt skip, non-session skip, missing/unreadable base, multiple orphans, per-folder failure isolation) plus the guard rows — **an `is_active`-reserved `recording` folder is left byte-for-byte untouched**, and **a symlinked base-dir entry is skipped** — with unique temp dirs and `std::fs` only; assert `dependency_firewall_holds` stays green.
- [x] `tools/keeper-rec/Tests/keeper-recTests/RecoveryTests.swift` -- Induced-kill assurance test: abandon the writer (no `finishWriting`), stop it, snapshot the settled crash-shape bytes, truncate mid-fragment (assert `cut < fileSize` so the kill really drops the un-flushed tail), and assert `AVAsset` reads strictly-monotonic frames up to the last complete ~4 s fragment yet fewer than the clean control, with no remux; use a non-flaky flush-wait budget.

**Acceptance Criteria:**
- Given an unfinalized session (stale `recording` manifest) from a recorder/keeper/power crash, when keeper starts up or is about to begin a new recording, then the recovery pass rebuilds the ledger from disk, marks the manifest `recovered`, leaves every segment file untouched and remux-free, and the orphaned tail fMP4 plays as-is (FR-73, AD-37).
- Given a manifest already at a terminal status (`finalized`/`recovered`/`failed`), when the recovery pass runs, then it is read once and left byte-for-byte unchanged (idempotent — a re-run recovers nothing new).
- Given a session the pass salvaged, when Story 20.3 later scans for recovery signals, then the `recovered` status persisted in that session's `manifest.json` is the durable record its once-per-session notice UI consumes (this story surfaces no UI; the live loud-failure notification is Story 18.4).
- Given the startup pass runs off the main thread, when the recordings volume is slow or holds many session folders, then app boot is not stalled and a scan failure never blocks startup.
- Given a recording session is live or starting (its folder reserved in `AppState`), when the startup or pre-record recovery pass runs concurrently and sees that folder's still-`recording` manifest, then the pass skips it and never rewrites it to `recovered` — the live session's manifest and ledger are left intact (the intent-contract's "never touch a non-orphaned session").
- Given `keeper-core`'s `dependency_firewall_holds` test, when the crate is tested, then the new recovery code in `recording.rs` carries no banned platform/process token (`std::fs` permitted) and the test passes.

## Spec Change Log

### 2026-07-19 — Review pass 1 (bad_spec loopback)

- **Triggering findings (both reviewers, deduplicated):**
  - `[high]` The recovery pass identified orphans **solely** by on-disk `status:"recording"`, which cannot distinguish a crashed orphan from a *live* session (a live session persists `recording` for its whole duration). The **detached** startup thread (and a `load`→`write` TOCTOU, plus a create-before-`RecordingRun`-install window in `recording_start`) could `reconcile_from_dir` + flip an actively-recording session's manifest to `recovered` mid-capture — corrupting a live session's durable record and violating the intent-contract's "never touch a non-orphaned session". The Design Notes' "at startup no session is live yet" argument was false for the detached thread.
  - `[low]` Two scans (startup thread + a fast pre-record start) could reconcile+`write` the same folder concurrently; `write()`'s fixed `.manifest.json.tmp` name means concurrent renames can interleave.
  - `[low]` `recover_orphaned_sessions` gated on `is_dir()`/`is_file()` which follow symlinks — a symlinked base-dir entry would be rewritten outside the destination tree.
  - `[medium]` Swift induced-kill test: if the snapshot has no un-flushed tail past the last `moof`, the cut is a no-op and the "fewer frames than control" assertion gives a false green; also the scratch file was copied before `cancelWriting`, racing an in-flight flush, and the 5 s flush-wait budget could flake red on a slow CI runner.
- **Amended (all OUTSIDE `<intent-contract>` — the contract's "never touch a non-orphaned session" already implied the guard):** Code Map, Tasks & Acceptance, and Design Notes now specify a shell-owned **live-folder reservation** (`recording_start` reserves its folder before `SessionManifest::create`, clears it on every exit path), an **`is_active` exclusion predicate** into `recover_orphaned_sessions` checked immediately before the salvage write (core stays firewall-clean — a bare `&dyn Fn`), a **recovery-scan mutex** serializing the two call sites, a **symlink skip**, and the two Swift test hardenings (assert `cut < fileSize`; settle the writer before the snapshot + non-flaky flush budget). Added guard test rows (reserved-folder untouched; symlinked entry skipped).
- **Known-bad state avoided:** a live recording's `manifest.json` silently overwritten with `recovered` mid-capture (a false recovery notice for an active session and a truncated ledger), a torn manifest from two racing scans, a manifest rewritten outside the destination tree, and a flaky/false-green induced-kill gate.
- **KEEP (must survive re-derivation):** `SessionManifest::load` (read+deserialize, set the `#[serde(skip)]` `folder`, secret-free `ManifestIo`); `recover_orphaned_sessions` reusing 17.2's disk-authoritative `reconcile_from_dir` (never a bare status flip; remux-free; only `Recording` acted on; terminal manifests idempotently untouched); best-effort non-fatal error handling (missing base = silent no-op, `NotFound` not warned, every per-entry failure warn+skip, never aborts/propagates/panics); firewall-clean core; the pre-record scan placed after the destination gate and before folder creation; the off-main-thread startup spawn; the effective-destination base-dir reuse (already-lazy `resolve_destination_dir`); the Swift abandon-the-writer crash-shape approach (a clean `finishWriting` consolidates fragments away) with `writeFixtureSegment` as the positive control; and the full core test matrix (all passed). Both Rust gates were green before this loopback — keep them green.

## Review Triage Log

### 2026-07-19 — Review pass 1
- intent_gap: 0
- bad_spec: 4: (high 1, medium 1, low 2)
- patch: 0
- defer: 1
- reject: 3
- addressed_findings:
  - `[high]` `[bad_spec]` Recovery could flip a *live* session's manifest to `recovered` (detached startup thread / load→write TOCTOU / create-before-reserve window) — spec now mandates a live-folder reservation + `is_active` exclusion predicate checked before the salvage write.
  - `[low]` `[bad_spec]` Two concurrent scans could race the same manifest's shared temp file — spec now serializes scans via a recovery-scan mutex.
  - `[low]` `[bad_spec]` Symlinked base-dir entry followed and rewritten outside the tree — spec now skips symlinked entries via the `DirEntry` file type.
  - `[medium]` `[bad_spec]` Swift induced-kill test could false-green on a no-op cut and flake on copy-before-cancel / a tight flush budget — spec now requires `cut < fileSize`, settling the writer before the snapshot, and a non-flaky flush wait.
  - Deferred (1): a session whose salvage permanently fails (e.g. a wedged read-only folder) stays `recording` and is re-scanned + re-warned on every startup and pre-record forever (no give-up marker).
  - Rejected (3): the `is_file()` pre-probe silent-skip of a stray (correct — `load` backstops real sessions with a warn); a stray `.manifest.json.tmp` left by a hard crash mid-`write` (pre-existing 17.2 `write()` behavior, self-heals on the next write); and pre-record start-latency growing O(session folders) (already tracked as a deferred-work item from the prior 17-3 analysis).

### 2026-07-19 — Review pass 2 (post-loopback)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 1, low 0)
- defer: 0
- reject: 10
- addressed_findings:
  - `[medium]` `[patch]` The pre-record recovery scan ran its O(session-folders) blocking `read_dir`/`stat` while holding the `recording_run` slot lock, violating the codebase invariant (documented at `recording_snapshot`) that the slot is never held across blocking `read_dir`/`stat` — so a slow/large recordings volume could stall stop/quit/tray during a Start. **Fixed:** hoisted `data_dir`/`directory` + the recovery-scan block to run BEFORE the `recording_run` start-guard is acquired (the guard's continuous busy-check→install hold, which is the single-capture-child guarantee, is unchanged; the new session's folder does not exist yet so the scan cannot see it). fmt+clippy clean, 975 Rust tests pass.
  - **Both reviewers confirmed the pass-1 live-session guard is correctly implemented and the corruption bug cannot recur** (reserve-before-create closes the window; the RAII `Drop` covers early-return/terminal/quit-abort with no live-but-unreserved or reserved-but-leaked window; `is_active` + the reservation share one mutex; core stays firewall-clean).
  - Rejected (10): raw-`PathBuf` `is_active` canonicalization gap (reserved key and scanned `entry.path()` are both built by joining onto the same in-process `directory`, so they are byte-equal — no real mismatch path); a symlinked `manifest.json` inside a real session dir (pathological tampering of app-owned files; the entry-level symlink skip covers the realistic case); the `is_file()` pre-probe TOCTOU (benign — `load` backstops); `version < MANIFEST_VERSION` forward-upgrade (only v1 exists; the `version > MANIFEST_VERSION` skip guards the real forward-incompat risk); reconcile-ok/write-fail "untested" (it IS exercised by `recover_isolates_a_per_folder_write_failure`); lock-order documented-in-prose-only (order is consistent scan→set at both sites; no cycle); the u32 collision-suffix saturation (pre-existing 17.2 code, pathological); startup-thread panic poisoning (`plain_lock` recovers poison by design; the thread is isolated); the process-local reservation vs a second keeper instance (single-instance is enforced elsewhere; the manifest schema may not carry a PID); and the Swift box-walker 64-bit largesize edge (test-only, only ever parses well-formed self-generated fixtures).

### 2026-07-19 — Review pass 3 (independent follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 0
- reject: 13
- addressed_findings:
  - none
- notes: The follow-up review the pass-2 lock-scope patch prompted (`followup_review_recommended: true`). Two fresh adversarial+edge-case reviewers (Opus) surfaced 13 deduplicated findings; **all rejected** — the review has converged. Both reviewers independently re-confirmed the pass-1 live-session guard is sound ("reserve-before-create ordering genuinely closes the create-window race"), matching pass 2. Rejections, by class: intent-contract-scoped-out (destination changed between crash and reboot strands orphans — the contract's Never clause: "an orphan left in a since-changed destination is out of scope"); already-rejected-in-pass-2 (`version < MANIFEST_VERSION` lower bound — only v1 exists; `is_active` path canonicalization — reserved key and scanned `entry.path()` are byte-equal, both joined onto the same in-process `directory`); confirmed-safe TOCTOU (`reserve()` outside `recovery_scan` — a folder is enumerable only after it exists, and existence implies a prior reserve-before-create; the Blind Hunter itself concluded it "cannot [happen] today"); already-tracked cost/noise (pre-record scan latency, boot-time O(folders) work, redundant startup+pre-record rescan, per-scan warn spam, idempotence-under-persistent-write-fault — all covered by the standing pass-1 deferred-work entry and the Design Notes "bounded, 0–1 orphans" note); speculative (a re-parented zombie sidecar reconciled mid-flush — the sidecar dies on EPIPE when its parent crashes, and a live writer is not the contract's "orphan"); and deliberately-scoped-by-spec (the shell reservation/`recovery_scan` concurrency is verified by review+prose, not integration tests, per the Code Map's core-only test matrix — both reviewers confirmed it correct by inspection). No new deferred-work entries (nothing deferred; the existing 17-3 wedged-orphan entry already captures the re-scan/re-warn noise and was left untouched).

## Design Notes

The heavy lifting already exists: 17.2's `reconcile_from_dir` is disk-authoritative (rebuilds `segments` from the `.mp4` files, bytes from `metadata`, preserving known PTS bounds by `(track, index)`), so a crashed session that never ran the terminal reconcile is fully salvaged just by loading its manifest and running the same path. The one missing seam is a **loader**: `SessionManifest`'s `folder` is `#[serde(skip)]`, so deserializing alone leaves `folder` empty and `reconcile_from_dir`/`write` would operate on the wrong path — `load` sets it from the passed folder.

Salvage flow per stale folder:
```text
load(folder)                    // read manifest.json, deserialize, set folder
if status != Recording { skip } // terminal manifests are truthful already
reconcile_from_dir()            // discard event-fed list, rebuild from disk
set_status(Recovered); write()  // atomic sibling-temp + rename, in place
```

**Base-dir finding already resolved.** The prior 17-3 review flagged an eager `unwrap_or(platform.data_dir()?)` fallback; the current base-dir derivation is already lazy — `resolve_destination_dir` uses `dirs::video_dir().unwrap_or_else(|| data_dir.to_path_buf()).join("keeper")` (ipc.rs), reached via `effective_destination_dir`. This story reuses that single source of truth for the scan root; no re-extraction needed.

**A `recording` manifest is NOT proof of an orphan — the scan MUST exclude the live session (review pass 1).** On-disk `status:"recording"` cannot by itself distinguish a crashed orphan from a session that is *currently recording* (a live session persists `status:"recording"` for its whole duration — mid-session writes keep it there; it only flips to a terminal state at finalize). The startup pass runs on a **detached** thread (intent-contract requires off-main-thread), so it can still be walking a slow/large recordings volume seconds after boot — *after* the webview loaded and the user clicked Record. Without a guard it would `load` that live session's manifest, see `Recording`, `reconcile_from_dir` a partial mid-capture snapshot, and `write` `recovered` over it — corrupting an active session's durable record and violating the intent-contract's "never touch a non-orphaned session". (`recording_start`'s own pre-record scan is race-free — it runs before the new folder exists — but the detached startup thread is not, and `RecordingRun` is only installed *after* `SessionManifest::create`, so a create-before-reserve window exists too.)

The guard (both call sites):
- **Live-folder reservation (shell).** `AppState` carries a reserved-folder set; `recording_start` inserts its session folder **before** `SessionManifest::create` and removes it when the session ends (every exit path — early error, terminal, quit-finalize), so a folder is reserved for the entire span it could be live, including the create→`RecordingRun`-install window.
- **Exclusion predicate (core).** `recover_orphaned_sessions(base_dir, is_active: &dyn Fn(&Path) -> bool)` calls `is_active(&folder)` **immediately before** `set_status(Recovered)`/`write()` and skips (never mutates) any reserved folder. The predicate reads the reservation set under the same lock, so a session that reserves-then-creates is always seen as active. Core stays firewall-clean — a bare `&dyn Fn`, no shell/Apple types.
- **Serialize scans.** A single `AppState` recovery mutex is held around each scan so the startup thread and a fast pre-record start never reconcile+`write` the same folder concurrently (their `write()` shares the fixed `.manifest.json.tmp` name — concurrent renames could interleave).
- **Skip symlinked entries.** The loop uses the `DirEntry` file type (no extra syscall, does not follow) and skips any symlinked base-dir entry, so recovery never `write`s a manifest outside the destination tree.

The pass stays idempotent: a second run finds only `recovered`/terminal or reserved-live manifests and mutates nothing.

**Pre-record cost.** The pre-record scan is O(number of session folders) — one manifest read each, a reconcile+write only for the rare stale one. Acceptable and bounded today (sidecar spawn dominates; orphans are 0–1); bounding or moving it off the start path is a future optimization once dogfooding shows folder accumulation (tracked in `deferred-work.md`).

**Induced-kill test (AC2).** A fragmented MP4 (`movieFragmentInterval` ~4 s) written by `AVAssetWriter` is playable up to its last flushed `moof` fragment even if `finishWriting` never runs — that is the crash-safety property recovery relies on. The test reproduces it without capture hardware or signing by abandoning the writer mid-session (a clean `finishWriting` consolidates the fragments away, so it cannot model a crash), snapshotting the un-finalized on-disk crash shape, then truncating mid-fragment, and asserting `AVAsset` still decodes frames up to the last complete fragment (mirrors 17.4's generate-on-runner, no-committed-media approach). **Two robustness requirements (review pass 1):** (a) the cut must actually drop bytes — assert `cut < fileSize` (guard/throw if the snapshot has no un-flushed tail past the last `moof`, e.g. write enough frames past the last fragment boundary), otherwise the "fewer frames than the clean control" assertion can pass a no-op truncation and give a false green; (b) the snapshot must be read from a settled file — `cancelWriting()` (or otherwise stop the writer) before copying the bytes so the copy never races an in-flight fragment flush, and give the flush-wait a generous, non-flaky budget (or key it off writer readiness) so a slow CI runner never reds a correct implementation.

## Verification

**Commands:**
- `bun run test:rust` -- expected: new `load` + `recover_orphaned_sessions` unit tests pass (all I/O-matrix rows); `dependency_firewall_holds` green.
- `bun run check:rust` -- expected: `cargo fmt --check` + `clippy --all-targets -- -D warnings` clean; no `.unwrap()` in production paths (manifest/recovery I/O uses `?`/`RecordingError::ManifestIo` and best-effort `warn`+skip).
- `bash scripts/test-keeper-rec.sh` -- expected: the new `RecoveryTests` induced-kill assertion passes alongside the existing rotation + NFR-22 concat gate (fixtures generated on the runner; no signing).

**Manual checks (if no CLI):**
- Inspect a salvaged folder: `manifest.json` shows `"status":"recovered"` with `segments` matching the actual `screen-####.mp4` files on disk (final segment included), and the `.mp4` files are unchanged (same bytes/mtime — no remux).

## Auto Run Result

Status: **done**

### Summary
Added a best-effort startup + pre-record recovery pass that marks crash-orphaned Recording Sessions `recovered` so their salvaged segments are surfaced (FR-73, AD-37) and Story 20.3's once-per-session notice has a durable signal. `keeper-core::recording` gains `SessionManifest::load` (the missing loader — the `#[serde(skip)] folder` is empty after a plain deserialize, so `reconcile_from_dir`/`write` need it rebound) and `recover_orphaned_sessions(base_dir, is_active)`, which walks the recordings base dir, and for each session whose `manifest.json` is still `status:"recording"` (a crash froze it — 17.2's terminal reconcile never ran) rebuilds the ledger authoritatively from the on-disk `.mp4` files via the existing `reconcile_from_dir` (never a bare status flip; disk-authoritative bytes; final/suppressed segments surfaced), sets `recovered`, and atomically rewrites — remux-free (segment files untouched). Best-effort and non-fatal end to end (missing base = silent no-op; every per-entry failure warn+skip; never aborts, propagates, panics, or blocks). The shell runs it at startup off the main thread and before each `recording_start`; the `recovered` manifests on disk are the ONLY output (no UI, no app-state/settings signal — 20.3 owns surfacing). A Swift induced-kill assurance test proves a mid-fragment-truncated fragmented `.mp4` still plays up to the last complete ~4 s fragment.

### Files changed
- `src-tauri/crates/keeper-core/src/recording.rs` — `SessionManifest::load` (secret-free `ManifestIo` on read/parse failure); `recover_orphaned_sessions(base_dir, is_active: &dyn Fn(&Path) -> bool)` (symlink-skip, live-folder guard, disk-authoritative reconcile reuse, best-effort); ~16 unit tests covering every I/O-matrix row plus the live-folder-reserved and symlinked-entry guard rows.
- `src-tauri/crates/keeper/src/ipc.rs` — `AppState.reserved_recording_folders` (`Arc<Mutex<HashSet<PathBuf>>>`) + `AppState.recovery_scan` (`Mutex<()>`); `LiveFolderReservation` RAII guard reserving the session folder before `SessionManifest::create` and unreserving on every exit path via `Drop`; pre-record scan run BEFORE the `recording_run` start-guard (so its O(folders) blocking scan never stalls stop/quit/tray); `pub(crate) fn recover_orphaned_recordings(state)` (startup helper, effective-destination base, best-effort).
- `src-tauri/crates/keeper/src/lib.rs` — `.setup` spawns the startup recovery pass on a detached `std::thread` (never blocks boot).
- `tools/keeper-rec/Tests/keeper-recTests/RecoveryTests.swift` (new) — induced-kill assurance test: abandon the writer (no `finishWriting`), settle + snapshot the crash-shape bytes, truncate mid-fragment (`cut < fileSize` asserted), assert `AVAsset` decodes strictly-monotonic frames up to the last complete fragment yet fewer than the clean control.
- `_bmad-output/implementation-artifacts/deferred-work.md` — one entry (permanently-wedged-orphan re-scan/re-warn noise).

### Review findings breakdown
- **Two reviewers (adversarial + edge-case, Opus) × two passes.**
- **Pass 1 → bad_spec loopback (1 high + 3 folded):** both reviewers independently found the recovery pass identified orphans SOLELY by on-disk `status:"recording"`, so the detached startup thread (or a load→write TOCTOU / create-before-reserve window) could flip a **live** recording's manifest to `recovered` mid-capture — corrupting an active session and violating the intent-contract's "never touch a non-orphaned session". Spec amended (live-folder reservation + `is_active` exclusion predicate + recovery-scan serialization mutex + symlink skip + two Swift test hardenings) and the code re-derived; 1 deferred, 3 rejected.
- **Pass 2 → 1 patch:** both reviewers confirmed the pass-1 guard is now correct and the corruption cannot recur. Remaining finding: the pre-record scan ran under the `recording_run` slot lock (an O(folders) blocking scan that could stall stop/quit/tray). **Patched** by moving the scan before the start-guard (guard continuity / single-child guarantee preserved). 0 deferred, 10 rejected.
- **Pass 3 → converged (independent follow-up):** the follow-up review the pass-2 post-panel patch recommended. Two fresh reviewers surfaced 13 deduplicated findings, **all rejected** — 0 intent_gap / 0 bad_spec / 0 patch / 0 defer. Both independently re-confirmed the live-session guard is sound. Rejections were intent-contract-scoped-out (destination-changed orphan stranding is an explicit Never clause), already-rejected-in-pass-2 (version lower-bound, `is_active` canonicalization), confirmed-safe (the `reserve()`-outside-`recovery_scan` TOCTOU cannot occur — existence implies prior reservation), already-tracked cost/noise (covered by the pass-1 deferred-work entry + the bounded-orphan Design Note), or deliberately spec-scoped (shell concurrency verified by review, not integration tests). `followup_review_recommended` lowered to **false**.

### Verification
- `bun run test:rust` — PASS (975/975; incl. the full recovery I/O matrix, the live-folder-reserved + symlinked-entry guard rows, `load` round-trip/error rows, and `dependency_firewall_holds`; re-run after the pass-2 patch).
- `bun run check:rust` — PASS (`cargo fmt --check` + `clippy --all-targets -- -D warnings`, zero warnings; no `.unwrap()` in production paths).
- `bash scripts/test-keeper-rec.sh` — PASS on the signed runner during implementation (49 tests, 0 failures, incl. the new `RecoveryTests` induced-kill + clean-control assertions). Not re-run for the pass-2 Rust-only lock-scope patch (no Swift change).

### Residual risks
- **followup_review_recommended: false** — the pass-3 independent follow-up review (two fresh reviewers over the recording-start critical section that the pass-2 lock-scope patch touched) converged with zero actionable findings; both reviewers re-confirmed the concurrency guard is sound. No further independent review is warranted.
- The live-session guard is process-local (single-instance is assumed enforced by Tauri); a second keeper process against the same destination is out of scope.
- A permanently-unsalvageable orphan (wedged read-only folder) is re-scanned + re-warned every startup/start with no give-up marker (deferred).
- Real end-to-end recovery on true crash output is exercised on dev-signed hardware in later dogfooding; this story's gate is the pure/fs unit tests + the Swift induced-kill assurance test.
