# Epic 14 Context: iOS Platform Behavior

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Make keeper an honest iOS citizen. Sync pauses cleanly when the app leaves the foreground and resumes instantly when it returns, all through one Rust lifecycle entry point — nothing anywhere pretends to be push or background delivery, and the UI and docs say so plainly. Foreground-local notifications and the app-icon badge work as far as iOS allows, and the phase's reliability bars are engineered in rather than hoped for: resume integrity (never a blank webview), memory hygiene under jetsam, flaky-network recovery without restarts, and a deliberate on-device storage posture (backup exclusion + file protection). This epic runs after the Epic 12 SM-7 gate, in parallel with Epic 13; verification is automated/Simulator-first, with on-device soaks folded into SM-8 dogfooding rather than blocking stories.

## Stories

- Story 14.1: Lifecycle Pause/Resume Through One Rust Entry
- Story 14.2: Honest No-Background-Sync Disclosure
- Story 14.3: Foreground Notifications and the All-Accounts Badge
- Story 14.4: Resume Integrity — Blank-Webview Guard and Stale-Resume Pill
- Story 14.5: Memory Hygiene Under Jetsam
- Story 14.6: Flaky-Network Resilience
- Story 14.7: Backup Exclusion and File Protection

## Requirements & Constraints

- **Lifecycle sync:** Backgrounding must stop the sliding-sync long-poll within seconds (graceful pause / offline mode) rather than letting it die mid-flight. Foregrounding renders cached state instantly and surfaces new messages within ~2 s on Wi-Fi. Both are Simulator-verifiable.
- **Honesty rule (hard constraint):** No surface anywhere may imply background delivery or push while the app is closed/suspended. This extends the existing desktop honesty rule. The badge must note it is not live while suspended. App copy and `docs/ios.md` limitations must match one-to-one so they never diverge.
- **Foreground notifications:** Post local notifications for new messages while the app is active, with identical content, preview toggle, and mute/mention-only semantics as desktop; suppress notifications for the currently-visible Chat. When OS notification permission is denied, show a persistent inline state with an Open Settings deep link, note the badge needs the same permission, and never self-re-prompt.
- **Badge:** Equals the Unified Inbox unread aggregate across all Accounts; refreshes on sync completion and foreground resume; never a live count while suspended.
- **Resume integrity:** Resuming (including overnight suspension) must never leave a blank or unresponsive webview; a reload guard detects a jettisoned webview process and restores the UI from cached state to the last stack level. Guard is automated-tested wherever process termination can be simulated, and continuously acceptance-tested from here on.
- **Memory hygiene:** On background / memory warning, drop droppable caches (image memory cache, media byte buffers); memory returns near baseline after backgrounding (Instruments-verified on Simulator). The large-media in-memory Range-slicing buffer stays capped with no unbounded growth under sustained seeking. A 24 h suspended soak with a large account must survive without a jetsam kill (SM-8 dogfooding, not story-blocking).
- **Flaky network:** Sync loop uses Simplified Sliding Sync offline mode with backoff, exits immediately on demand (foreground resume or pull-to-refresh), recovers unaided from airplane-mode and Wi-Fi↔cellular handovers, never blanks the UI, never spams toasts, and needs no app restart. Messages composed while disconnected must never be silently lost — they queue and dispatch on foreground reconnect (an already-elapsed undo window dispatches immediately).
- **Storage posture:** All database directories (SDK/crypto stores, `keeper.db`, `archive.db`) must carry the `isExcludedFromBackup` flag (verified by reading the resource value back in a test) and use file protection `NSFileProtectionCompleteUntilFirstUserAuthentication` — never `Complete`, so WAL access keeps a resumed sync loop working after screen lock (protection class asserted in code; lock-screen behavior validated in SM-8).

## Technical Decisions

- **One lifecycle entry (AD-30):** Lifecycle detection must enter Rust through a single lifecycle command in the shell (`lifecycle.rs`), driving `SyncService` pause on background and resume-with-immediate-sync on foreground. The zero-native stopgap is the webview `visibilitychange` event; the upgrade path is a micro Swift plugin on `UIApplication` notifications behind the same Rust entry — same entry point either way. Pull-to-refresh (Epic 13) must converge on the same sync-kick operation as foreground resume: one code path, no second lifecycle truth.
- **Snapshot-then-diff rendering (AD-8):** Every stream opens with a full snapshot/reset batch then diffs, so re-subscribing on resume is always safe. On foreground, render the cached zustand mirrors instantly, then reconcile via diff. The stale-resume path shows cached UI at once, kicks sync, and relies on a sync-loop restart guard for the known stale-session edge (matrix-rust-sdk#3935).
- **Notifications reuse (AD-18):** iOS notifications are foreground-local + badge-on-sync only, reusing the desktop rules engine and its visible-Chat suppression logic — no push infrastructure.
- **Badge source (AD-20):** The badge value is the Unified Inbox unread aggregate already computed by the `inbox` projection in Rust — never compute a second count.
- **Blank-webview guard (AD-30 / tauri#14371):** A reload guard for the blank-webview-on-resume bug is mandatory; it restores the UI from cached state. Track the upstream fix; incorporate Story 12.6's on-device findings.
- **Media buffers (AD-28):** `keeper-media://` runs unchanged on iOS; Range slicing stays in-memory with a capped buffer. Disk-backed streaming of large media is explicitly deferred work, not a phase requirement — the cap is the phase posture.
- **Secrets & store protection (AD-29):** Backup-exclusion and file-protection flags go through the existing `Platform` port. All account state stays under the single `Platform::data_dir()` root so a future App Group container move (NSE era) is a path change, not a migration.
- **No push this phase:** APNs push / NSE / background sync sit behind the paid-program decision gate and are out of scope. The single data-dir root keeps that future move cheap.

## UX & Interaction Patterns

- **Lifecycle disclosure card:** Shown once on iOS first run (Wizard Done step, or first Inbox render for an existing Account) and permanently in Settings → Notifications, with this exact copy: *"On iPhone, keeper syncs and notifies only while open. Close it and messages wait on your homeserver until you return — nothing is lost, and nothing here pretends to be push."* Voice rules apply.
- **Stale-resume pill:** On a stale foreground resume, render cached UI immediately and show a quiet "Connecting…" pill under the Inbox header that clears on the first sync response.
- **Offline pill:** On connectivity loss, an offline pill appears and clears with recovery — no toast spam, UI keeps rendering from the local mirror throughout.
- **Queued-send caption:** Messages backgrounded before dispatch carry the amber caption "Queued — sends when keeper is open and back online" and dispatch on foreground reconnect.
- **Permission-denied state:** Persistent inline "Notifications are off for keeper in iOS Settings." with an Open Settings deep link.

## Cross-Story Dependencies

- **14.1 is the spine:** Stories 14.2, 14.3, 14.4, 14.5, and 14.6 all depend on the single lifecycle entry from 14.1.
- **Epic 12 (SM-7 gate):** Precondition for the whole epic. 14.7 depends specifically on 12.3 (the `Platform::data_dir()` root); 14.5 depends on 12.4 (the media buffer cap); 14.4 folds in Story 12.6's on-device resume findings.
- **14.6 depends on 14.4** (shares resume/guard behavior).
- **Epic 13:** Runs in parallel; pull-to-refresh (Story 13.6) must reuse 14.1's sync-kick operation, and the Story 13.7 Archive & Storage disclosure must match 14.7's actual backup flagging.
- **Epic 15 / SM-8:** Overnight-suspension, 24 h jetsam soak, and on-device network scenarios (airplane-mode, Wi-Fi↔cellular) are SM-8 dogfooding checklist items with findings ledgered — not story-blocking device steps. `docs/ios.md` (Story 15.2) limitations copy must match 14.2 one-to-one.
