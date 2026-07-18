# Epic 18 Context: Tray & Loud Failures — The Menu Bar Tells the Truth

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Make the macOS menu-bar tray the always-truthful surface for screen recording, and make every recording fault impossible to miss. This epic wires the tray's recording/warning/error states (elapsed time, segment, size, Stop, Open Recordings Folder), forces the tray visible during recording even when the user has opted out of the tray, ensures quitting never orphans a running recorder, adds an in-app twin of the tray (active-recording banner + segment meter) for when the user is looking at the app instead of the menu bar, wires a loud-failure triad (tray error + native notification + banner) so no recording ever fails silently, and adds a disk-space guard that warns and then gracefully stops-and-finalizes rather than letting a long recording die mid-write or fill the disk. It extends Story 10.3's opt-in tray and the AD-18 notification pipeline into the new Screen Recording phase (AD-33..39).

## Stories

- Story 18.1: Tray Recording State — Elapsed·Segment·Size, Stop, Open Folder
- Story 18.2: Forced Tray Presence & Honest Quit-While-Recording
- Story 18.3: In-App Active-Recording Banner & Segment Meter
- Story 18.4: Loud-Failure Triad — Tray Error + Notification + Banner
- Story 18.5: Disk-Space Guard — Warn & Graceful Stop-and-Finalize

## Requirements & Constraints

- The tray must reflect recording state within 1 s of start (`idle → recording → warning/error`), with a ~1 Hz-updating disabled menu line showing elapsed time, current segment index, and bytes written (e.g. "Recording — 12:34 · segment 3, 412 MB"), plus one-click **Stop Recording** and **Open Recordings Folder** menu items.
- Recording must **force the tray visible** even when the user's own opt-in tray toggle is off, and restore the exact prior tray configuration when recording stops — an invisible recording indicator is treated as a bug, not a degraded state.
- Quit-while-recording (⌘Q) must warn first, then run stop → flush → finalize the current segment, guarded by a kill-timeout so a hung sidecar is force-terminated rather than orphaned. This extends the existing quit-honesty behavior (background operation) to recording.
- macOS's own purple screen-recording indicator pill is system-owned and must never be touched or duplicated — keeper's tray only adds what the pill lacks (elapsed, segment, Stop, error states).
- Every started recording session must reach a user-visible terminal state: `finalized | recovered | failed` — no silent recording loss, extending the app's general no-silent-loss principle to capture.
- Recording faults (recorder crash/unexpected exit, writer stall, permission revocation, device loss, disk hard-floor) must surface within 5 s via all three channels at once: tray flips to error, a native notification is posted through the existing notification pipeline offering one-click restart, and (if the app is open) the in-app banner shows the error with its reason. Non-fatal warnings (e.g. mic hot-unplug, low disk) must persist until resolved or acknowledged — never a toast that silently disappears.
- Mic hot-unplug specifically must never abort the recording: video and system audio keep rolling, the mic track continues silence-filled, keeper attempts fallback to the system default input, and a persistent warning is raised.
- Disk-space guard: pre-start free-space validation (alongside folder exists/writable checks) must block start with an actionable error if space is already insufficient. During an active recording, free space crossing a warn threshold (authored default 10 GB) raises a persistent warning; crossing a hard floor (authored default 2 GB) triggers a graceful stop-and-finalize (never runs the volume to exhaustion, never dies mid-write). Both thresholds are product-owner sign-off items at phase release, not architecture blockers, and must be testable via a simulated low-free-space signal (no need to physically fill a disk).
- Induced-failure test coverage for this epic covers recorder-kill, writer-stall, and device-loss legs in automation; the live permission-revoke-mid-record leg is validated separately on real hardware at a later acceptance milestone (Story 20.6) and is out of scope here.
- Recording-red (the dedicated live-capture color token) is reserved exclusively for the record dot, the banner's edge/fill, and the tray error badge — never on buttons, body text, hover states, or general decoration, and it must stay visually distinct from the app's destructive/delete red. Reduced-motion settings keep the record dot steady, never pulsing.

## Technical Decisions

- Ownership split: `keeper-rec` (the Swift sidecar) owns buffer-bounding, drop policy, and gapless segment rotation. `keeper-core::recording` owns the platform-free session state machine, manifest, segment ledger, recovery reconciliation, and the disk-guard **policy** (pre-start validation, warn/hard-floor decisions) — driven by free-space figures reported on the sidecar's `state` events, not measured directly by the core.
- The tray lives in `crates/keeper/src/tray.rs` behind a single mutex-guarded `TrayIcon` slot (desktop-only, `#[cfg(desktop)]`). Recording states are applied via `TrayIcon::set_icon` using shipped record-dot and warning-badge icon assets; the menu's elapsed/segment/size line is a disabled item updated on a ~1 Hz tick.
- Loud failures ride the existing AD-18 notification pipeline (`keeper-core::notify` → tauri-plugin-notification) rather than a new notification mechanism — recording faults are just another source feeding that pipeline, within the same 5 s delivery expectation.
- The UI (tray and in-app banner alike) is a pure renderer of the Rust-owned recording state machine fed by sidecar events (`state`, `segmentClosed`, `error`); no recording state is invented or duplicated in TypeScript/JS.
- This epic depends on Epic 16 (recording foundation) and Epic 17 (segment ledger/manifest, gapless rotation, finalize path) for the underlying session/segment data the tray and banner display, and for the finalize path the disk-guard's graceful stop relies on.

## UX & Interaction Patterns

- **Tray states**: `idle` (existing unchanged app icon) → `recording` (record-dot badge, bottom-right) → `warning` (amber outline badge) / `error` (filled recording-red badge). The elapsed/segment line uses monospace type in a disabled menu item.
- **Active-recording banner** (in-app twin of the tray, pinned to the top of the Recording view, persistent — never a toast): recording-red 3px left edge, record dot, "Recording", monospace "elapsed · segment · size" line, and a destructive-outline Stop button. Warning variant shows an amber edge with a persistent line that never auto-clears. Error variant is a filled recording-red banner naming the reason with a "Restart recording" action.
- **Segment meter**: a progress bar filling toward the configured segment size, captioned "segment N · 412 / 500 MB", resetting at each gapless rotation.
- Pause is explicitly deferred/out of scope this phase — absent from both tray and banner, not shown disabled.
- Accessibility: VoiceOver announces recording state assertively on start/stop/fault (treated as loss-risk events, like bridge health) — e.g. "Recording, 12 minutes 34 seconds, segment 3" — but elapsed time announces only on demand/state-change, never once per second. `Esc` must never stop a recording; stopping is always an explicit focusable action (destructive-by-omission guard). Tray items are real labelled menu items reachable via the standard macOS menu-bar-extra keyboard/VoiceOver path.
- Disk-floor stop copy example: "Recording stopped — low disk. N segments saved," paired with the banner/tray/notification triad, not a silent halt.

## Cross-Story Dependencies

- 18.1 is the foundation (tray recording state) that 18.2, 18.3, and 18.4 build on.
- 18.2 (forced presence, honest quit) depends on 18.1's tray plumbing.
- 18.3 (in-app banner) depends on 18.1 for the same state feed the tray uses.
- 18.4 (loud-failure triad) depends on both 18.1 (tray error state) and 18.3 (banner error variant) since all three surfaces must fire together.
- 18.5 (disk guard) depends on 18.3 and 18.4 (it reuses the warning/error banner and notification machinery) and on Epic 17's finalize path (graceful stop-and-finalize must reuse the same clean-finalize logic as a normal Stop).
- This epic extends Story 10.3 (the original opt-in tray) and the AD-18 notification pipeline rather than replacing them — existing non-recording tray/notification behavior must remain unchanged.
