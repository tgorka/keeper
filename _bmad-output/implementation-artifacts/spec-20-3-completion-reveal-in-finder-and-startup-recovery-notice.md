---
title: 'Story 20.3: Completion / Reveal-in-Finder & Startup Recovery Notice'
type: 'feature'
created: '2026-07-19'
status: 'done'
baseline_revision: 'b26d399e38fba3c347d7800014f4d784715a9324'
final_revision: '1882243'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-20-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** When a recording finalizes today the Recording view shows only a one-line "Saved to {path}" caption (`recording-pane.tsx:170`) — no segment count, size, or Reveal button — and a crash-orphaned session recovered by Story 17.3 surfaces **nowhere**: 17.3 writes `status:"recovered"` into the session's `manifest.json` and discards the recovered-folder list at both call sites, so the durable signal is never read. The user has no honest end-of-session summary and no notice that an interrupted session was salvaged.

**Approach:** Render a completion `Card` on finalize ("Saved N segments · {size}" + session-folder path in mono + a primary **Reveal in Finder**, no preview/trim/share), and surface each unacknowledged `recovered` session **once** as the same card shape with a `bridge-degraded`-tinted warning edge ("A recording was interrupted; N segments were saved"). Both derive N and size from the authoritative on-disk manifest via a small read-only summary command; a persisted acknowledgement latch (registry `settings` table, mirroring the iOS-disclosure precedent) guarantees each recovered session surfaces exactly once. Recovered files play as-is — no remux. Tray restore is already owned by 18.1/18.2 and is verified, not rebuilt.

## Boundaries & Constraints

**Always:**
- "N segments" and "{size}" come from the manifest's authoritative segments — screen-track count `segments.iter().filter(|s| s.track == "screen").count()` and total bytes `segments.iter().map(|s| s.bytes).sum()`. **Never** use `RecordingStatusVm.segments_closed` for the count (it counts only *closed* segments — 0 for a single-segment session — and does not discriminate track).
- The completion and recovery cards are the **same card shape**; recovery uses a `bridge-degraded` warning edge (`border-bridge-degraded/50 text-bridge-degraded`, the `bridge-card` recipe). No preview, trim, share, upload, or cloud affordance anywhere on either card.
- The **Reveal in Finder** button is gated on `capabilities.revealInFileManager` (hidden when false) — it invokes the existing `reveal_path` command; recovered files are opened/revealed as-is with no remux or byte mutation.
- A recovered session surfaces **exactly once**: `recovered_sessions_list` excludes any session whose folder basename is in the persisted acknowledgement seen-set; dismissing/acknowledging a recovery card latches it. The latch is a one-way, best-effort registry `settings` write.
- New Rust surfacing code that reads/scans manifests stays firewall-clean in `keeper-core` (std::fs + serde only); all Tauri/command glue lives in the `keeper` shell. New commands are desktop-only (mobile stubs return `Unsupported`).
- All quality gates green: `bun run check`, `bun run check:rust`, `bun run test:rust`. No `.unwrap()`/`.expect()` in production paths; no `any` in TS; `import type` for type-only imports.

**Block If:**
- Surfacing recovered sessions cannot be done without mutating the wire-stable `ManifestStatus` enum or rewriting recovered manifests to a new status (the chosen design is a registry-`settings` seen-set — do not overload the manifest schema).
- The epic/AC's "N segments" is discovered to mean camera+screen combined rather than the screen track (the spec assumes screen-track count; rotation keeps screen==camera count, so screen is authoritative).

**Never:**
- No preview/trim/share/upload/transcription surface on the recording completion or recovery cards.
- No remux, re-encode, or byte rewrite of recovered segment files; no new host contacted (zero-egress).
- Do not rebuild tray restore logic (18.1/18.2 owns `decide_presence`/`DropTray`/`restore_idle`) — only verify it restores prior config at the `Finalized`/`Recovered` terminal.
- Do not reuse `recording_acknowledge` (18.4, in-memory live-slot clear) for cross-restart recovery dedup — it cannot see prior-run orphans.
- Do not stream large payloads over IPC; the summary command returns only counts/bytes/folder-path scalars.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Finalize, single segment | manifest `finalized`, 1 screen segment | Completion card "Saved 1 segment · {size}", folder in mono, Reveal in Finder | manifest load fails → card still shows folder + Reveal; count/size omitted, logged |
| Finalize, N segments | manifest `finalized`, N screen segments (+optional camera) | "Saved N segments · {size}"; camera segments not counted | — |
| In-app terminal `recovered` | live stop salvaged (`RecordingUiState::Recovered`) | Same card, warning-tinted edge, "A recording was interrupted; N segments were saved", Reveal | as above |
| Cross-restart orphan, unacknowledged | `recovered` manifest in destination dir, basename not in seen-set | Recovery card surfaced once on the idle recording surface, warning edge, N/size/folder + Reveal | scan/load error → session skipped, logged, never throws |
| Orphan already acknowledged | basename in seen-set | Not listed; no card | — |
| Acknowledge recovery card | user dismisses | basename added to seen-set; never re-surfaces on later scans/restarts | write fails → best-effort; may reappear next scan, logged |
| Multiple orphans | several `recovered` manifests unacknowledged | Each surfaced, each independently dismissable | per-entry failure isolated |
| Empty/missing destination dir | no dir or no recovered manifests | `recovered_sessions_list` → `[]`; no cards | missing dir → `[]`, logged |
| Reveal with capability off | `revealInFileManager` false | Reveal button not rendered | — |
| Terminal tray state | state → `finalized`/`recovered` | Tray restores exact prior config (`DropTray` if forced-present, else `RestoreIdle`) | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/recording.rs` -- Add pure `SessionManifest` accessors `screen_segment_count(&self) -> u32` (`segments` filtered `track == "screen"`) and `total_bytes(&self) -> u64` (sum of all `segments[].bytes`). Firewall-clean. Reuse existing `load`. Unit-test both incl. single-segment and screen+camera.
- `src-tauri/crates/keeper-core/src/registry.rs` -- Add the acknowledgement seen-set under a new key `ui.recovered_sessions_acknowledged` (JSON array of session-folder basenames), mirroring the `ios_sync_disclosure_shown` latch (registry.rs:505): `get_recovered_sessions_acknowledged(data_dir) -> Result<Vec<String>, CoreError>` and `add_recovered_session_acknowledged(data_dir, session: &str) -> Result<(), CoreError>` (idempotent add via `get_setting`/`set_setting`). Unit-test round-trip + idempotence.
- `src-tauri/crates/keeper-core/src/vm.rs` -- Add `RecordingSummaryVm { session_folder: String, screen_segment_count: u32, total_bytes: u64 }` (`#[serde(rename_all = "camelCase")]`).
- `src-tauri/crates/keeper/src/ipc.rs` -- Add three desktop `#[tauri::command]`s (+ mobile `Unsupported` stubs): `recording_session_summary(folder: String) -> Result<RecordingSummaryVm, IpcError>` (`SessionManifest::load` → summary; used by the completion / in-app-recovered card); `recovered_sessions_list(state) -> Result<Vec<RecordingSummaryVm>, IpcError>` (scan `effective_destination_dir` immediate subdirs, `load` each `manifest.json`, keep `status == Recovered` AND basename not in the seen-set, map to summary, deterministic sort, best-effort skip on per-entry error); `recovered_session_acknowledge(state, folder: String) -> Result<(), IpcError>` (`add_recovered_session_acknowledged` on the basename). Reuse `effective_destination_dir(data_dir)`.
- `src-tauri/crates/keeper/src/lib.rs` -- Register the three new commands in the `tauri::generate_handler!` invoke handler.
- `src/lib/ipc/client.ts` -- Add typed wrappers `recordingSessionSummary(folder)`, `recoveredSessionsList()`, `recoveredSessionAcknowledge(folder)` + a `RecordingSummaryVm` type (`sessionFolder`, `screenSegmentCount`, `totalBytes`).
- `src/components/layout/recording-summary-card.tsx` (new) -- Shared card (shadcn `Card`) with `variant: "completion" | "recovered"`; props `sessionFolder`, `screenSegmentCount`, `totalBytes`, optional `onDismiss`. Renders "Saved N segment(s) · {size}" (completion) / "A recording was interrupted; N segment(s) were saved" (recovered, warning edge), folder in `font-mono`, a Reveal-in-Finder button gated on `capabilities.revealInFileManager` → `revealPath(sessionFolder)`. Size via `formatSize`/`bytesToWholeMb` from `recording-format.ts`. No preview/trim/share.
- `src/components/layout/recording-pane.tsx` -- Replace the `FINALIZED_NOTE_PREFIX` one-liner (line ~170) with the completion card on `state === "finalized"` (fetch summary via `recordingSessionSummary(status.outputPath)`); render the same card warning-tinted on the in-app `state === "recovered"` terminal; in the idle/pre-record state render one recovery card per unacknowledged recovered session, dismiss → `recoveredSessionAcknowledge`.
- `src/hooks/use-recovered-sessions.ts` (new) -- On mount (app start / recording surface) and after a session finalizes, call `recoveredSessionsList()` (only when `capabilities.recording`); expose `sessions` and an `acknowledge(folder)` that latches + drops the session from local state. Colocated test.
- Tests: `src/components/layout/recording-pane.test.tsx`, `recording-summary-card.test.tsx`, `use-recovered-sessions.test.ts`, and Rust command/accessor/registry tests.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- Add `screen_segment_count` (screen track only) and `total_bytes` (sum of **all** `segments[].bytes`, screen + camera) accessors + unit tests: single-segment session counts 1; a screen+camera manifest counts only the screen segments yet sums bytes across both.
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- Add `get_/add_recovered_session_acknowledged` (JSON-array seen-set under `ui.recovered_sessions_acknowledged`) + round-trip/idempotence tests.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- Add `RecordingSummaryVm` (camelCase).
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- Add `recording_session_summary`, `recovered_sessions_list`, `recovered_session_acknowledge` (desktop + mobile stubs); best-effort scan reusing `effective_destination_dir` + `SessionManifest::load`; command tests over temp dirs (recovered listed once, acknowledged excluded, terminal-non-recovered excluded, missing dir → `[]`).
- [x] `src-tauri/crates/keeper/src/lib.rs` -- Register the three commands in `generate_handler!`.
- [x] `src/lib/ipc/client.ts` -- Add the three TS wrappers + `RecordingSummaryVm` type.
- [x] `src/components/layout/recording-summary-card.tsx` -- New shared completion/recovered card; Reveal gated on `revealInFileManager`; size via `recording-format.ts`; unit test both variants + gated Reveal + dismiss.
- [x] `src/hooks/use-recovered-sessions.ts` -- New hook fetching + acknowledging recovered sessions behind `capabilities.recording`; test.
- [x] `src/components/layout/recording-pane.tsx` -- Wire completion card (finalized), warning card (in-app recovered), and idle-state recovery cards; extend `recording-pane.test.tsx` (completion card renders count/size/path/Reveal; recovered card warning edge + dismiss→ack; Reveal hidden when capability off).

**Acceptance Criteria:**
- Given a Stop, when the session finalizes, then the Recording view shows a completion `Card` — "Saved N segments · {size}" (N/size from the finalized manifest, not `segments_closed`) + the session-folder path in `mono` + a primary **Reveal in Finder** — with no preview/trim/share affordance, and the tray returns to its exact prior configuration (FR-71, FR-76, UX-DR34).
- Given an interrupted session marked `recovered` by Story 17.3, when keeper starts or is about to begin a new recording, then it surfaces **once** as "A recording was interrupted; N segments were saved" — the same card shape with a `bridge-degraded`-tinted edge, linking the folder — and recovered files are revealed/play as-is with no remux (FR-73, UX-DR34).
- Given a recovered session already acknowledged (its basename in the persisted seen-set), when the recovery scan re-runs on a later startup or pre-record, then it is not surfaced again.
- Given `revealInFileManager` is false, when either card renders, then the Reveal in Finder control is absent (capability-gated).
- Given `keeper-core`'s `dependency_firewall_holds` test, when the crate is tested, then the new `recording.rs`/`registry.rs` surfacing code carries no banned platform/process token and the test passes.

## Spec Change Log

No spec amendments — the review produced no `intent_gap` or `bad_spec` findings (no loopback). All addressed findings were localized `patch`es applied directly to the diff.

## Review Triage Log

### 2026-07-19 — Review pass 1
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 1, low 3)
- defer: 0
- reject: 13
- addressed_findings:
  - `[medium]` `[patch]` Both reviewers: the completion / in-app-recovered card fabricated **"Saved 0 segments · 0 MB"** when the summary was `null` (a flash on every finalize during the async fetch, and permanent on a manifest-load failure) instead of the spec-mandated degraded shape. Made `RecordingSummaryCard`'s count/size nullable and rendered a figureless headline ("Recording saved" / "A recording was interrupted") + folder + Reveal when unavailable; the pane now passes `null` (not `0`). Added a card degraded test and a pane summary-fetch-failure test.
  - `[low]` `[patch]` `useRecordedSessionSummary` had no folder-change cancellation, so a slow fetch for a superseded folder could set a prior session's figures. Added a per-effect `stale` guard + clear-on-change.
  - `[low]` `[patch]` `get_recovered_sessions_acknowledged` silently `unwrap_or_default()`-swallowed a corrupt seen-set value (re-surfacing every recovered notice with no trace). Now logs `tracing::warn!` on parse failure before degrading to empty; added a corrupt-value registry test.
  - `[low]` `[patch]` `recovered_session_acknowledge` latched the full path as a "basename" when `file_name()` was `None`, diverging from the scan's basename read (an acknowledged card could re-surface). Now skips + logs when there is no basename.
  - Rejected (13): cross-destination basename collision and since-changed-destination orphan invisibility (explicitly out of scope per Story 17.3's intent contract — the scan is single-destination); the optimistic-drop-then-reappear on a failed latch (documented honest fallback; the seen-set remains the source of truth); the in-app-`recovered` terminal re-surfacing as a scan card after `outputPath` changes (low; a correct fix entangles 18.4's live-slot acknowledge semantics); disjoint count(screen)/size(all-tracks) scope, the sort-doc "basename" nuance, the hand-maintained TS twin, the summary command not re-`reconcile`-ing (the invariant holds — 17.2 reconciles at every terminal), no focus/foreground re-scan, `reveal_item_in_dir` parent-selection semantics, the below-fold `role=status` a11y note (the card is `role=status`), the mobile stub `Unsupported`-vs-`[]` asymmetry, and the byte-exact path-equality double-render filter (holds today).

## Design Notes

**Why a summary command, not the live snapshot.** `RecordingStatusVm.segments_closed` is a rotation counter (bumped only on `SegmentClosed`, track-agnostic) — a single-segment session reports 0 and a camera session inflates it, so it cannot back "Saved N segments." The manifest is authoritative (17.2's `reconcile_from_dir` rebuilds `segments` from the on-disk `.mp4` files at every terminal, including the final never-closed segment). One tiny read-only command `recording_session_summary(folder)` loads that manifest and returns `{screenSegmentCount, totalBytes}`; the completion card calls it with `status.outputPath` (the session **folder**), keeping the hot live-poll state machine untouched. `recovered_sessions_list` reuses the same manifest→summary mapping over a directory scan.

**Surface the recovered list that 17.3 threw away.** 17.3's `recover_orphaned_recordings` computes the recovered folders then only logs the count (`ipc.rs:4411`, and the pre-record scan at ~`3826`). Rather than thread that in-memory list to the frontend (it also misses orphans from prior app runs), the frontend re-derives it from disk: `recovered_sessions_list` scans `effective_destination_dir` for `manifest.json` with `status:"recovered"`. Disk is the single source of truth and the scan is idempotent.

**"Exactly once" via a registry seen-set.** The only durable per-session signal is the manifest status, which never flips back — so a separate acknowledgement store is needed. Reuse the `settings` k/v table pattern of `ios_sync_disclosure_shown` (registry.rs:505), but keyed as a set: `ui.recovered_sessions_acknowledged` holds a JSON array of session-folder basenames. Acknowledging adds the basename; the list command filters them out. This handles multiple distinct recovered sessions and survives restarts, without overloading the wire-stable `ManifestStatus` enum (consumed by `recover_orphaned_sessions`'s own `status == Recording` gate) or reusing 18.4's in-memory `recording_acknowledge`.

Salvage/summary flow (per recovered folder, read-only):
```text
load(folder)                      // manifest.json already status:"recovered" (17.3)
if status != Recovered { skip }
if basename in seen-set { skip }  // already surfaced once
-> RecordingSummaryVm { folder, screen_segment_count, total_bytes }
```

**Tray is verified, not rebuilt.** `decide_presence` (tray.rs:342) already maps every terminal (incl. `Recovered`) to `DropTray` (forced-present) or `RestoreIdle`, keyed off the untouched `system.menu_bar_presence` setting. 20.3 relies on the existing tray decision tests; add an assertion only if a `Recovered`-terminal case is not already covered.

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc + vitest green, incl. the new card/hook/pane tests (completion count/size/path/Reveal, warning-edge recovery card, dismiss→ack, capability-gated Reveal).
- `bun run check:rust` -- expected: `cargo fmt --check` + `clippy --all-targets -- -D warnings` clean; no `.unwrap()` in production paths.
- `bun run test:rust` -- expected: new accessor/registry/command tests pass; `dependency_firewall_holds` green.

**Manual checks (if no CLI):**
- Finalize a session: completion card shows "Saved N segments · {size}" matching the on-disk `screen-####.mp4` files and the folder path; Reveal opens Finder at the folder. Inspect a `status:"recovered"` manifest folder: the recovery card appears once with the warning edge, and after dismissal never reappears (basename present in `ui.recovered_sessions_acknowledged`); segment `.mp4` bytes are unchanged (no remux).

## Auto Run Result

Status: **done**

### Summary
Delivered both legs of Story 20.3 against the now-unblocked Story 17.3 recovery mechanism. **Completion leg:** on `finalized` (and the in-app `recovered` terminal), the Recording view renders a shared `RecordingSummaryCard` — "Saved N segments · {size}" + the session folder in mono + a capability-gated **Reveal in Finder**, no preview/trim/share. N and size come from a read-only `recording_session_summary(folder)` command that loads the authoritative on-disk manifest (screen-track segment count + total bytes) — never the unreliable live `segments_closed` counter. **Recovery leg:** `recovered_sessions_list()` re-derives crash-recovered sessions from disk (scans the effective destination for `status:"recovered"` manifests whose basename isn't in a persisted seen-set) and surfaces each once as the same card shape with a `bridge-degraded` warning edge; `recovered_session_acknowledge(folder)` latches the basename into a new registry `settings` seen-set (`ui.recovered_sessions_acknowledged`) so it never re-surfaces. Recovered files are revealed as-is (no remux). All new `keeper-core` code is firewall-clean; commands are desktop-gated with mobile `Unsupported` stubs. Tray restore (18.1/18.2) is relied on, not rebuilt.

### Files changed
- `src-tauri/crates/keeper-core/src/recording.rs` — `SessionManifest::screen_segment_count()` / `total_bytes()` accessors + tests.
- `src-tauri/crates/keeper-core/src/registry.rs` — `get_/add_recovered_session_acknowledged` seen-set (JSON array under `ui.recovered_sessions_acknowledged`); logs + degrades to empty on a corrupt value; round-trip/idempotence + corrupt-value tests.
- `src-tauri/crates/keeper-core/src/vm.rs` — `RecordingSummaryVm { session_folder, screen_segment_count, total_bytes }` (camelCase).
- `src-tauri/crates/keeper/src/ipc.rs` — `recording_session_summary`, `recovered_sessions_list`, `recovered_session_acknowledge` commands (desktop + mobile stubs); pure `scan_recovered_sessions` helper; basename-guard on acknowledge; command/scan tests.
- `src-tauri/crates/keeper/src/lib.rs` — registered the three commands in `generate_handler!`.
- `src/lib/ipc/client.ts` — `RecordingSummaryVm` type + `recordingSessionSummary` / `recoveredSessionsList` / `recoveredSessionAcknowledge` wrappers.
- `src/components/layout/recording-summary-card.tsx` (new) — shared completion/recovered card; nullable count/size degrade; capability-gated Reveal; tests.
- `src/hooks/use-recorded-session-summary.ts` (new) — terminal-session summary fetch with stale-resolution guard.
- `src/hooks/use-recovered-sessions.ts` (new) — disk-scan + acknowledge hook behind `capabilities.recording`; test.
- `src/components/layout/recording-pane.tsx` — wired completion / in-app-recovered / idle-state recovery cards; removed the `FINALIZED_NOTE_PREFIX` one-liner; passes `null` (not `0`) when the summary is unavailable; extended tests (incl. the degraded path).

### Review findings breakdown
- **Two reviewers (Blind Hunter adversarial + Edge Case Hunter, Opus), 1 pass.** Both independently confirmed the firewall stays clean, no `.unwrap()` in new production paths, desktop-gating with mobile stubs, the count derives from the manifest screen track (not `segments_closed`), and recovered files are never remuxed.
- **4 patches applied** (1 medium, 3 low): the "0 segments · 0 MB" degraded-card fabrication (both reviewers); the summary-fetch stale-resolution race; the silent corrupt-seen-set reset; the `file_name()==None` acknowledge key divergence. Each is covered by a new/extended test.
- **13 rejected** — see the Review Triage Log. Chiefly: cross-destination / since-changed-destination orphan handling (explicitly out of scope per 17.3's intent contract), documented best-effort fallbacks, and cosmetic/doc nits.
- **0 intent_gap, 0 bad_spec, 0 defer.** `followup_review_recommended: false` — the fixes are localized, tested, and low-consequence; no independent follow-up warranted.

### Verification
- `bun run check:rust` — PASS (`cargo fmt --check` + `clippy --all-targets -- -D warnings`, zero warnings; no `.unwrap()` in production paths).
- `bun run test:rust` — PASS (982/982; incl. the new accessor, seen-set round-trip + corrupt-value, and command/scan tests, and `dependency_firewall_holds`).
- `bun run check` — PASS (biome clean, tsc clean, vitest 1450/1450 incl. the new card/hook/pane + degraded-path tests, and the `keeper-core` tauri-free firewall).

### Residual risks
- Recovery surfacing is single-destination and process-local (a since-changed recordings destination or a second keeper instance is out of scope — consistent with Story 17.3's intent contract).
- The in-app `recovered` terminal card and the cross-restart scan card can both surface the same salvaged session across separate app states (low; both honest; a unified acknowledgement-based dedup would touch 18.4's live-slot semantics — rejected this pass).
- End-to-end recovery against real crash output and the tray prior-config restore are exercised on dev-signed hardware in later dogfooding (20.5/20.6); this story's gate is the pure/fs unit tests + the frontend component/hook tests.
