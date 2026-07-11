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

use keeper_core::vm::IpcError;
use serde::Deserialize;
use tauri::State;
use ts_rs::TS;

use crate::ipc::AppState;

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
/// foreground resume converge on a single sync-kick and cannot diverge.
/// `Background` runs `state.accounts.pause_all().await`, gracefully stopping
/// each live account's `SyncService` without tearing anything down.
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
            state.accounts.sync_now().await;
            // Re-assert the app-icon badge from the current aggregate now the app is
            // running again (Story 14.3, AD-20) — reuses the inbox merger's
            // `reapply_badge` (never a second count). Desktop never invokes this command;
            // on iOS the honest-no-op badge port makes this reach the OS. Best-effort.
            state.accounts.reassert_badge().await;
        }
        LifecyclePhase::Background => state.accounts.pause_all().await,
    }
    Ok(())
}
