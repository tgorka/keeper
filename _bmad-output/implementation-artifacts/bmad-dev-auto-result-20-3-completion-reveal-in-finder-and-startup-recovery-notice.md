---
status: ready-for-dev
story: '20.3'
slug: '20-3-completion-reveal-in-finder-and-startup-recovery-notice'
intended_spec_file: '_bmad-output/implementation-artifacts/spec-20-3-completion-reveal-in-finder-and-startup-recovery-notice.md'
blocking_condition: 'unmet upstream dependency — Story 17.3 (startup recovery of orphaned segments) is unimplemented, so no mechanism marks an interrupted session `recovered` for the FR-73 recovery-notice leg to surface'
---

# BMad Dev Auto Result — Story 20.3

Status: **blocked**

Blocking condition: The FR-73 recovery-notice leg of Story 20.3 depends on Story
17.3 ("Startup Recovery of Orphaned Segments"), which is **not implemented**
(`sprint-status.yaml`: `17-3-startup-recovery-of-orphaned-segments: backlog`; no
`spec-17-3-*.md` exists). Without 17.3 there is no code that scans the recordings
base dir at startup / pre-record, reconciles an interrupted (`recording`-status)
manifest to `recovered`, or exposes recovered orphaned sessions to the frontend —
so the second acceptance criterion cannot be met, and inventing that upstream
mechanism inside 20.3 is a scope/sequencing decision that requires the coordinator.

This halted in **step-02 (plan)** before any spec was written, so no code was
changed and the working tree is clean.

## Why this blocks (evidence)

Story 20.3 acceptance criteria (from `epics.md`):

1. **Completion leg (FR-71, FR-76, UX-DR34)** — on Stop/finalize, show a completion
   `Card` "Saved N segments · {size}" + session-folder path in `mono` + a primary
   **Reveal in Finder** (no preview/trim/share); tray returns to its exact prior
   configuration.
2. **Recovery-notice leg (FR-73, UX-DR34)** — "an interrupted session **(marked
   `recovered` by Story 17.3)** … when keeper starts or is about to begin a new
   recording … surfaces **once** as 'A recording was interrupted; N segments were
   saved' — the same card shape with a `bridge-degraded`-tinted edge, linking the
   folder — and recovered files play as-is with no remux."

Codebase state (verified 2026-07-19):

- **`Recovered` is reachable only within a live in-app stop.** The core state
  machine transitions `Stopping → Recovered` on a sidecar `RecordingEvent::Recovered`
  (`keeper-core/src/recording.rs:250`). This is a partial-salvage during an
  *already-open* recording session — it is **not** the cross-restart orphan case the
  AC describes ("when keeper starts or is about to begin a new recording").
- **No startup / pre-record orphan recovery exists.** `grep` for `recover_orphaned*`,
  `orphan`, a base-dir `read_dir`, or a `ManifestStatus::Recording`→`Recovered`
  reconciliation finds nothing. `recording_start` (`keeper/src/ipc.rs:3698`) has no
  pre-record recovery pass; `lib.rs` `setup()` performs no recordings-dir scan.
- **Story 17.3 was never specced or implemented.** No `spec-17-3-*.md` file;
  sprint status is `backlog`. The `deferred-work.md` entries citing
  `source_spec: spec-17-3-startup-recovery-of-orphaned-segments.md` (e.g. lines
  ~997, ~1002, ~1007) are **forward-looking items tracked *to* 17.3**, not evidence
  that 17.3 shipped — they describe what 17.3 *must* do.
- Epic 20 context confirms the dependency: "20.3 (Completion/recovery) depends on
  **Epic 17 (recovery/ledger)**, 16.6 (stop path), and 18.1 (tray restore)."

The completion leg is unaffected — it is fully buildable today (see below). Only the
recovery-notice leg is blocked, but the two share one card shape and one story, so the
story cannot be delivered whole against its stated ACs without 17.3.

## What IS ready (so a re-drive can move fast once unblocked)

Grounding gathered during planning — the completion leg needs no new backend:

- **View state + terminal snapshot:** `RecordingUiState` (`vm.rs`) already includes
  `finalized`/`recovered`/`failed`; `RecordingStatusVm` carries `state`,
  `output_path` (session **folder**), `segments_closed`, `on_disk_bytes`, `error`,
  `warning`. The 1 Hz poll in `use-recording-session.ts` stops on terminal and
  retains the last snapshot until `recording_acknowledge` clears it.
- **Completion UI gap:** today only a one-line "Saved to `{path}`" note renders at
  `recording-pane.tsx:170` (`FINALIZED_NOTE_PREFIX`). No completion Card, no segment
  count / size line, no Reveal-in-Finder button, no `recovered` rendering.
- **Reveal in Finder exists:** `reveal_path(path)` command
  (`keeper/src/ipc.rs:2037`, `tauri_plugin_opener::reveal_item_in_dir`, desktop-only,
  gated by the `revealInFileManager` capability — Story 5.5). The tray already uses
  the same primitive via `open_recordings_folder` (`tray.rs:158`). NOTE: an existing
  deferred item (deferred-work.md ~line 836) flags that the export dialog's Reveal
  button is un-gated on `revealInFileManager`; the 20.3 completion card should gate
  its Reveal on that capability from the start.
- **Tray restore exists (18.1/18.2):** `decide_presence` → `DropTray`/`restore_idle`
  (`tray.rs`) already returns the tray to its exact prior configuration at terminal;
  20.3 would verify, not rebuild, this.
- **`bridge-degraded` warning tint:** defined in `src/index.css` (`--color-bridge-degraded`,
  amber `#d97706`); used as `border-bridge-degraded/50 text-bridge-degraded` on the
  bridge-card badge — the pattern for the recovery card's warning edge.
- **Ambiguity to resolve in the spec (not blocking):** "N segments" — `segments_closed`
  is the closed-segment counter on the live snapshot; confirm it equals the
  authoritative `manifest.segments` screen-track count at finalize (single-segment,
  no-rotation sessions may report 0), and decide whether the count/size for a
  *recovered* session come from the persisted manifest (which 17.3 would reconcile).

## Recommended resolution (coordinator decision)

Pick one, then re-drive 20.3:

- **(A) Sequence 17.3 first (recommended).** Implement Story 17.3 (startup/pre-record
  orphan scan → `reconcile_from_dir` → mark `recovered` → expose recovered sessions to
  the frontend), then re-run 20.3 against both legs. Preserves story boundaries.
- **(B) Re-scope 20.3 to the completion leg only** (FR-71/FR-76), and split the FR-73
  recovery-notice into a follow-up gated on 17.3. Requires amending the epic/ACs.
- **(C) Explicitly authorize 20.3 to absorb 17.3's backend orphan-recovery.** Larger
  scope than the story as written; only choose if 17.3 will not be run standalone.

Re-drive with `/bmad-dev-auto 20-3-completion-reveal-in-finder-and-startup-recovery-notice`
once the dependency is resolved (or `/bmad-dev-auto 17-3-startup-recovery-of-orphaned-segments`
to build the prerequisite first).
