---
title: 'Story 18.1: Tray Recording State — Elapsed·Segment·Size, Stop, Open Folder'
type: 'feature'
created: '2026-07-17'
status: 'done'
baseline_revision: '6209fe5f3596a611d42462dae26a509e51b67e4a'
final_revision: '2b039ef700985f218003e91116e06ab2d1d18646'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-18-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** A live screen recording is invisible in the menu bar — the opt-in tray (Story 10.3) still shows only "Show keeper"/"Quit", so a running capture can be silently forgotten and there is no one-click Stop or way to reach the files.

**Approach:** When a session is live, drive the existing single mutex-guarded tray slot to a `recording` state: swap its icon to a record-dot badge, add a `~1 Hz`-refreshed disabled status line (`Recording — 12:34 · segment 3, 412 MB`), and add **Stop Recording** + **Open Recordings Folder** menu items. The tray is a pure renderer of the already-Rust-owned `RecordingStatusVm` snapshot; on any terminal state it restores the idle icon + menu.

## Boundaries & Constraints

**Always:**
- The tray renders only from the authoritative `RecordingStatusVm` snapshot (`AppState.recording_run`) + on-disk segment bytes — never invents, caches, or duplicates recording state.
- Recording state is reflected within 1 s of start; the status line refreshes on a ~1 Hz tick (elapsed advances every second).
- **Stop Recording** reuses the exact existing graceful-stop path (`recording_stop`'s one-shot trigger) so the current segment finalizes and the session reaches `finalized`; **Open Recordings Folder** reveals the session folder via the existing opener plugin.
- Everything tray-related stays best-effort (`warn` + continue) — the tray is a convenience, never load-bearing — matching the existing module.
- Segment size is summed from the session's own `screen-####.mp4` files on disk (same file-ownership rule as the manifest reconcile), so the figure grows live and matches the eventual manifest.

**Block If:**
- The graceful-stop path or `RecordingStatusVm` snapshot does not exist / cannot be reached from the tray (a foundational assumption of this story is false).

**Never:**
- Never touch, hide, or duplicate macOS's own purple screen-recording indicator pill.
- Never force the tray visible when the opt-in toggle is off, and never change tray presence/restore logic — that is Story 18.2. If no tray is present, the tick is a silent no-op.
- Never add or change any frontend/TypeScript surface, `RecordingStatusVm` fields, or the in-app banner (Story 18.3).
- No Pause affordance (deferred this phase). No warning/error tray variants (Story 18.4) beyond the plain `recording` state.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Bytes, normal | Folder with `screen-0000.mp4` (10), `screen-0001.mp4` (20) | `session_bytes_on_disk` → 30 | No error |
| Bytes, foreign files | Folder also holds `manifest.json`, `camera-0000.mp4`, `notes.mp4` | Only `screen-####.mp4` summed | Ignore non-segment files |
| Bytes, missing/empty folder | Folder absent or no segments | 0 | No error (best-effort) |
| Bytes, mid-write | Current segment file growing | Returns current on-disk length | No error |
| Elapsed format | 754 s | `"12:34"` | — |
| Elapsed format ≥ 1h | 3723 s | `"1:02:03"` | — |
| Size format | 431_800_000 bytes | `"412 MB"` (decimal MB, whole) | — |
| Size format ≥ 1000 MB | 1_290_000_000 bytes | `"1.2 GB"` | — |
| Status line compose | live, 2 closed, elapsed 754 s, 412 MB | `"Recording — 12:34 · segment 3, 412 MB"` | — |
| Status line, pre-capture | state `preflight`, no start instant | `"Starting…"` | No panic |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/tray.rs` -- the tray module: `static TRAY` slot, `build_tray`, `set_tray_presence`, `show_main_window`, `on_menu_event`. Extend with recording rendering + new menu items + status-line formatting.
- `src-tauri/crates/keeper/src/lib.rs` -- Tauri `.setup()` (`#[cfg(desktop)]` block ~124-141); spawn the ~1 Hz tick here holding the `AppHandle`.
- `src-tauri/crates/keeper/src/ipc.rs` -- `AppState.recording_run` (119), `RecordingRun`/`status`, `recording_stop` (3575) graceful-stop path to reuse, `slot_lock`/`status_lock`, `reveal_path`→`tauri_plugin_opener::reveal_item_in_dir` (~2006) pattern.
- `src-tauri/crates/keeper-core/src/vm.rs` -- `RecordingStatusVm` (2703): `{state, segments_closed, started_at_epoch_ms, output_path (folder), error}` — read only, unchanged.
- `src-tauri/crates/keeper-core/src/recording.rs` -- `SessionState` (40), `SEGMENT_STEM_PREFIX`/`segment_index_from_stem` (964) segment-file ownership rule; add `session_bytes_on_disk`.
- `src-tauri/crates/keeper/icons/` -- add the record-dot tray asset here.

## Tasks & Acceptance

**Execution (dependency order):**
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- add `pub fn session_bytes_on_disk(folder: &Path) -> u64` summing this session's `screen-####.mp4` file lengths (reuse `SEGMENT_STEM_PREFIX` + `segment_index_from_stem`; ignore foreign `*.mp4`, `manifest.json`, dirs); return 0 on a missing/unreadable folder. Unit-test the matrix bytes rows.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- factor `recording_stop`'s body into a reusable `pub(crate) fn stop_active_recording(state: &AppState)` called by both the command and the tray Stop item, so the tray fires the identical idempotent one-shot trigger. (Also added `pub(crate) recording_snapshot` shared by the poll command + tray, and made `epoch_ms_now` `pub(crate)`.)
- [x] `src-tauri/crates/keeper/icons/tray-recording.png` -- add a record-dot menu-bar badge asset for the recording state (recording-red dot). Generate the PNG during implementation. (44×44 RGBA, recording-red `#dc2626` dot.)
- [x] `src-tauri/crates/keeper/src/tray.rs` -- add pure formatters `format_elapsed`, `format_size`, `format_status_line` (unit-tested per matrix); add `STOP_ID`/`OPEN_FOLDER_ID` and extend `on_menu_event` (Stop → `stop_active_recording`; Open Folder → reveal the snapshot's `output_path` folder via `tauri_plugin_opener::reveal_item_in_dir`); add `pub fn apply_recording_state(app, snapshot)` that sets record-dot vs default icon (`TrayIcon::set_icon`), swaps the menu (recording = disabled status line + Stop + Open Folder + Show + Quit; idle = Show + Quit), and refreshes the disabled line each tick. No-op when the tray slot is empty. Best-effort (`warn`+continue). (Slot became `TrayState`; line refreshed via held `MenuItem::set_text`, no-flicker; `image-png` tauri feature added to `Cargo.toml`.)
- [x] `src-tauri/crates/keeper/src/lib.rs` -- in the desktop `.setup()`, spawn a `tokio::time::interval(Duration::from_secs(1))` task holding the `AppHandle`; each tick reads the `RecordingStatusVm` snapshot from `AppState.recording_run` and calls `tray::apply_recording_state`. (No explicit `run_on_main_thread`: Tauri tray/menu calls dispatch to main internally and the tray lock is never held across them.)

**Acceptance Criteria:**
- Given the opt-in tray is present and a recording starts, when the ~1 Hz tick runs, then within 1 s the tray icon shows the record-dot badge, the disabled status line reads like `Recording — 12:34 · segment 3, 412 MB` and advances every second, and the menu contains **Stop Recording** and **Open Recordings Folder**.
- Given a live recording, when the user chooses **Stop Recording**, then the same graceful-stop trigger fires (idempotent), the current segment finalizes, the session reaches `finalized`, and the tray restores its idle icon + Show/Quit menu.
- Given a live or finalized session, when the user chooses **Open Recordings Folder**, then the session folder (`output_path`) is revealed in Finder.
- Given the opt-in tray toggle is off (no tray slot), when a recording runs, then the tick is a silent no-op and no tray is forced (deferred to 18.2).
- Given macOS's own purple recording pill, then keeper never touches it — the tray only adds elapsed/segment/Stop/Open that the pill lacks.

## Design Notes

- **Single source of truth.** Elapsed = `now − started_at_epoch_ms`; segment index = `segments_closed + 1`; size = `session_bytes_on_disk(output_path)`. No new VM field, no TS changes — the tray reads exactly what the Recording view already polls, plus disk bytes (disk is already the manifest's authority via `reconcile_from_dir`). Size units: decimal MB (`10^6`) matching the `segment_mb` convention; one-decimal GB at ≥ 1000 MB.
- **Transitions.** Render the status line when `state ∈ {recording, rotating, stopping}` with a start instant; on `preflight`/no-start show `Starting…`; on any terminal or `idle`, restore the idle tray. Track the last-rendered mode so the icon/menu swap fires only on transition while the disabled line text refreshes every tick. Keep one tray icon: swap content with `TrayIcon::set_menu` on transition and update the line via the held `MenuItem`'s `set_text` (or rebuild the recording menu each tick — whichever Tauri v2 renders without flicker); the `on_menu_event` handler covers all four ids.

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core recording::` -- expected: `session_bytes_on_disk` matrix tests pass.
- `cd src-tauri && cargo test -p keeper tray::` -- expected: `format_elapsed`/`format_size`/`format_status_line` matrix tests pass.
- `cd src-tauri && cargo build -p keeper` -- expected: desktop build succeeds (tray + tick compile).
- `cd src-tauri && cargo clippy -p keeper -p keeper-core -- -D warnings` -- expected: no warnings.

**Manual checks (if no CLI):**
- Tray/menu-bar interaction (icon swap, live line, Stop finalizing, Open Folder revealing) is validated on real hardware at a later acceptance milestone; automated coverage here is the pure formatters + the bytes helper.

## Review Triage Log

### 2026-07-17 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 2, low 0)
- defer: 0
- reject: 19: (high 0, medium 0, low 19)
- addressed_findings:
  - `[medium]` `[patch]` Terminal→idle transition cleared the rendered-mode flag (`store_status_item(None)`) unconditionally even when `build_idle_menu`/`set_menu` failed, stranding the recording menu (Stop/Open) for the app lifetime. Fixed in `tray.rs`: build+install the idle menu FIRST and clear the flag only on success, else return so the next tick retries; the cosmetic icon restore moved after the flag-critical menu swap.
  - `[medium]` `[patch]` `RecordingStatusVm.output_path` doc contract was stale (said "the file being written") though it has held the session **folder** since Story 17.2 — a latent trap that would silently zero the tray size the moment anyone honored the documented file semantics. Corrected the struct-level and field-level doc comments in `vm.rs` to state it is the session folder.
- notes: Symlink / non-UTF-8 / trailing-digit (`screen-0001-copy2`) "miscount" findings were rejected — `session_bytes_on_disk` is byte-for-byte identical to `SessionManifest::reconcile_from_dir`, so parity with the manifest is intended. `reveal_item_in_dir` highlighting the folder in its parent is spec-conformant (AC says "reveals"). Forced-presence, Failed/warning tray variants (18.2/18.4), the per-tick re-stat cost, and the worker-thread main-dispatch block were rejected as out-of-scope or acceptable under the best-effort/never-load-bearing charter. Blind Hunter verified the lock discipline against Tauri 2.11.5 internals — no deadlock/crash introduced.

## Auto Run Result

Status: done

**Summary:** The macOS menu-bar tray now reflects a live screen recording. While a session is live, the existing single mutex-guarded tray flips to a record-dot icon with a ~1 Hz-refreshed disabled status line (`Recording — 12:34 · segment 3, 412 MB`) and adds **Stop Recording** + **Open Recordings Folder**; on any terminal/idle state it restores the idle Show/Quit tray. The tray is a pure renderer of the Rust-owned `RecordingStatusVm` snapshot (elapsed from `started_at_epoch_ms`, segment from `segments_closed + 1`, size summed live from the session folder on disk) — no new VM field and no TypeScript changes. Stop reuses the identical idempotent graceful-stop trigger as the `recording_stop` command.

**Files changed:**
- `src-tauri/crates/keeper-core/src/recording.rs` — new `session_bytes_on_disk` (parity with `reconcile_from_dir`) + 3 unit tests.
- `src-tauri/crates/keeper-core/src/vm.rs` — corrected stale `output_path` folder-vs-file doc comments (review patch).
- `src-tauri/crates/keeper/src/ipc.rs` — factored `stop_active_recording` + `recording_snapshot` (shared by command + tray), `epoch_ms_now` made `pub(crate)`.
- `src-tauri/crates/keeper/src/tray.rs` — recording rendering, Stop/Open menu items, pure formatters (+4 tests), `apply_recording_state`, deadlock-safe lock discipline, robust transition cleanup (review patch).
- `src-tauri/crates/keeper/src/lib.rs` — 1 Hz `tokio::time::interval` tick in desktop `.setup()`.
- `src-tauri/crates/keeper/Cargo.toml` (+ `Cargo.lock`) — `image-png` tauri feature for `Image::from_bytes`.
- `src-tauri/crates/keeper/icons/tray-recording.png` — new 44×44 RGBA record-dot asset.

**Review findings breakdown:** 2 patches applied (both medium, localized robustness + a stale-doc latent trap); 0 intent gaps; 0 bad-spec loopbacks; 0 deferred; 19 rejected (spec-conformant, out-of-scope 18.2/18.4, or acceptable under the best-effort charter). No deadlock/crash defects — lock discipline verified against Tauri internals.

**Verification:** `cargo test -p keeper-core recording::` → 50 passed; `cargo test -p keeper tray::` → 4 passed; `cargo build -p keeper` → success; `cargo clippy -p keeper -p keeper-core -- -D warnings` → clean. (One transient test failure was a temp-dir collision between two concurrent `cargo test` runs, not reproduced on a clean single run.)

**Residual risks:** GUI-level tray behavior (icon swap, live line refresh, Stop finalizing, Open Folder reveal) is not automatable here — deferred to real-hardware acceptance; automated coverage is the pure formatters + the bytes helper. On a rare menu-build failure the tray may briefly retry a transition on the next tick (best-effort, self-healing). `oversized` spec warning retained (multi-file cross-layer story).
