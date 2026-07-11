//! App-lifecycle entry for the keeper shell (Epic 14-1).
//!
//! This is the single Rust seam through which every app-lifecycle transition
//! (background / foreground) flows. On iOS the zero-native stopgap drives it
//! from the webview `visibilitychange` event; a later micro Swift
//! `UIApplication` plugin (`didEnterBackground` / `willEnterForeground`) will
//! call this SAME command — so the command signature is kept stable for that
//! upgrade path. Desktop never invokes it: the frontend attaches no listener on
//! the desktop tier, preserving Story 10.3 background operation.
//!
//! There is exactly one lifecycle command and no competing resume path:
//! foreground delegates to [`AccountManager::sync_now`] (the same sync-kick
//! pull-to-refresh uses, Story 13.6) and background delegates to
//! [`AccountManager::pause_all`] (graceful `SyncService::stop`, never teardown).
//!
//! Story 14.4 adds the stale-resume restart guard (matrix-rust-sdk#3935):
//! `sync_now()` is a bare idempotent `SyncService::start()` that no-ops while the
//! service still reports `Running` — exactly the stale-session trap after a long
//! suspension. `Background` records a **wall-clock** pause instant and, when the
//! suspension exceeded [`STALE_RESUME_THRESHOLD`], `Foreground` runs `pause_all()`
//! as a prelude so the shared kick becomes a real restart. One lifecycle truth:
//! the kick itself stays the identical `sync_now()`, and pull-to-refresh is
//! untouched (never a restart, never a second kick).
//!
//! [`AccountManager::sync_now`]: keeper_core::account::AccountManager::sync_now
//! [`AccountManager::pause_all`]: keeper_core::account::AccountManager::pause_all

use std::time::{Duration, SystemTime};

use keeper_core::vm::IpcError;
use serde::Deserialize;
use tauri::State;
use ts_rs::TS;

use crate::ipc::{slot_lock, slot_take, AppState};

/// How long a suspension must last before the foreground resume forces a full
/// sliding-sync restart (`pause_all()` before the shared `sync_now()` kick) to
/// defeat the matrix-rust-sdk#3935 stale-session edge. Below this, the bare
/// idempotent kick is enough (and cheaper — no long-poll teardown).
pub const STALE_RESUME_THRESHOLD: Duration = Duration::from_secs(120);

/// Pure stale-resume gate (Story 14.4): did the suspension recorded at `paused`
/// last at least `threshold` by `now`?
///
/// Operates on wall-clock [`SystemTime`] — deliberately NOT `Instant`, whose
/// Apple `mach_absolute_time` base does not advance while the device sleeps, so
/// an overnight suspension would measure near-zero and the restart would never
/// trip. The elapsed computation saturates: a backward clock jump (NTP) makes
/// `duration_since` fail, which reads as elapsed zero ⇒ not stale ⇒ the safe
/// bare kick. `None` (no pause recorded — e.g. a cold launch's first foreground)
/// is never stale.
pub fn should_restart_sync(
    paused: Option<SystemTime>,
    now: SystemTime,
    threshold: Duration,
) -> bool {
    match paused {
        Some(paused) => now.duration_since(paused).unwrap_or(Duration::ZERO) >= threshold,
        None => false,
    }
}

/// Which app-lifecycle transition the frontend is reporting (Epic 14-1).
///
/// `Foreground` = the app (re)entered the foreground → resume sync via the
/// idempotent [`AccountManager::sync_now`] kick. `Background` = the app left the
/// foreground → gracefully pause every live account's sync loop via
/// [`AccountManager::pause_all`]. Serializes to its lowercase name — the `phase`
/// argument of `app_lifecycle_changed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LifecyclePhase {
    /// The app (re)entered the foreground — resume sync.
    Foreground,
    /// The app left the foreground — gracefully pause sync.
    Background,
}

/// The one Rust lifecycle entry (Epic 14-1): route an app-lifecycle transition
/// to the matching core operation.
///
/// `Foreground` runs exactly `state.accounts.sync_now().await` — the identical
/// call the `sync_now` command makes — so pull-to-refresh (Story 13.6) and
/// foreground resume converge on a single sync-kick and cannot diverge. When the
/// suspension recorded on `Background` exceeded [`STALE_RESUME_THRESHOLD`], a
/// `pause_all()` prelude turns that same kick into a full restart (Story 14.4,
/// matrix-rust-sdk#3935); the pause timestamp is *taken* (consumed) either way so
/// a later foreground without a new background can never re-trip it.
/// `Background` records the pause wall-clock instant — earliest-wins: only when
/// none is stored, so a duplicate/late `Background` can't shrink a long real
/// suspension into a short one — then runs `state.accounts.pause_all().await`,
/// gracefully stopping each live account's `SyncService` without tearing
/// anything down.
///
/// Both branches are best-effort and infallible (an empty/all-asleep account set
/// is a no-op), so this never returns an error in practice.
#[tauri::command]
pub async fn app_lifecycle_changed(
    state: State<'_, AppState>,
    phase: LifecyclePhase,
) -> Result<(), IpcError> {
    match phase {
        LifecyclePhase::Foreground => {
            // Take-not-read: the pause instant is consumed by exactly one resume.
            let paused_at = slot_take(&state.paused_at);
            if should_restart_sync(paused_at, SystemTime::now(), STALE_RESUME_THRESHOLD) {
                tracing::info!(
                    "stale resume: forcing a full sync restart (pause_all before the shared kick)"
                );
                // The restart prelude (Story 14.4): per-account `SyncService::stop()`
                // makes the shared kick below a real `start()` rather than a
                // stale-session no-op. Resume-only — pull-to-refresh never runs this.
                state.accounts.pause_all().await;
            }
            state.accounts.sync_now().await;
            // Re-assert the app-icon badge from the current aggregate now the app is
            // running again (Story 14.3, AD-20) — reuses the inbox merger's
            // `reapply_badge` (never a second count). Desktop never invokes this command;
            // on iOS the honest-no-op badge port makes this reach the OS. Best-effort.
            state.accounts.reassert_badge().await;
        }
        LifecyclePhase::Background => {
            // Earliest-wins (Story 14.4): lifecycle reports are not guaranteed
            // strictly alternating, so a duplicate/late `Background` must never
            // overwrite (and thereby shrink) an already-recorded suspension start.
            // Wall-clock `SystemTime`, never `Instant` — see `should_restart_sync`.
            {
                let mut slot = slot_lock(&state.paused_at);
                if slot.is_none() {
                    *slot = Some(SystemTime::now());
                }
            }
            state.accounts.pause_all().await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed, comfortably-post-epoch anchor so the boundary tests never depend on
    /// the host clock.
    fn anchor() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_750_000_000)
    }

    #[test]
    fn no_recorded_pause_is_never_stale() {
        assert!(!should_restart_sync(None, anchor(), STALE_RESUME_THRESHOLD));
    }

    #[test]
    fn below_the_threshold_takes_the_bare_kick() {
        let paused = anchor();
        let now = paused + (STALE_RESUME_THRESHOLD - Duration::from_secs(1));
        assert!(!should_restart_sync(
            Some(paused),
            now,
            STALE_RESUME_THRESHOLD
        ));
    }

    #[test]
    fn exactly_at_the_threshold_restarts() {
        let paused = anchor();
        let now = paused + STALE_RESUME_THRESHOLD;
        assert!(should_restart_sync(
            Some(paused),
            now,
            STALE_RESUME_THRESHOLD
        ));
    }

    #[test]
    fn above_the_threshold_restarts() {
        let paused = anchor();
        // The headline scenario: an overnight suspension.
        let now = paused + Duration::from_secs(8 * 60 * 60);
        assert!(should_restart_sync(
            Some(paused),
            now,
            STALE_RESUME_THRESHOLD
        ));
    }

    #[test]
    fn a_backward_clock_jump_saturates_to_not_stale() {
        // `now` BEFORE the recorded pause (an NTP step back): `duration_since`
        // fails, saturates to zero elapsed ⇒ not stale ⇒ the safe bare kick.
        let paused = anchor();
        let now = paused - Duration::from_secs(3600);
        assert!(!should_restart_sync(
            Some(paused),
            now,
            STALE_RESUME_THRESHOLD
        ));
    }
}
