//! The dock badge (Story 10.3, FR-53, AD-18).
//!
//! The dock badge is computed **in Rust** from the full cross-account unread/mention
//! state so it stays correct while the window is hidden (never from the windowed view
//! models, never in the webview). The pure [`badge_count`] arithmetic maps the current
//! [`DockBadgeMode`] plus the aggregate `(unread_rooms, mention_total)` to the number to
//! show (or `None` to clear); [`apply`] pushes that through the [`Platform`] port to the
//! OS dock. Both are unit-tested without a live `Client` or `AppHandle`.
//!
//! [`BadgeConfig`] holds the current mode as a lock so the Settings command can change
//! it live and the inbox merger reads it on every merged-state change. It lives on the
//! [`AccountManager`](crate::account::AccountManager) as an `Arc<BadgeConfig>` (not a
//! `static`), so there is no new global mutable state — mirroring
//! [`NotifyConfig`](crate::notify::NotifyConfig).

use std::sync::RwLock;

use crate::platform::Platform;
use crate::vm::DockBadgeMode;

/// The app-wide dock-badge mode (Story 10.3). Seeded once in
/// [`AccountManager::new`](crate::account::AccountManager) from the persisted registry
/// value (default [`DockBadgeMode::All`]) and read by the inbox merger on every
/// merged-state change to recompute the badge. The Settings command updates it live via
/// [`AccountManager::dock_badge_mode_set`](crate::account::AccountManager).
#[derive(Debug)]
pub struct BadgeConfig {
    mode: RwLock<DockBadgeMode>,
}

impl BadgeConfig {
    /// Construct with the given initial mode (seeded from the persisted registry value
    /// in [`AccountManager::new`](crate::account::AccountManager)).
    pub fn new(mode: DockBadgeMode) -> Self {
        Self {
            mode: RwLock::new(mode),
        }
    }

    /// The current dock-badge mode. A poisoned lock fails to the honest default
    /// ([`DockBadgeMode::All`]) rather than panicking — the badge is a comfort signal
    /// and must never abort the inbox path.
    pub fn mode(&self) -> DockBadgeMode {
        match self.mode.read() {
            Ok(mode) => *mode,
            Err(poisoned) => {
                tracing::warn!("badge-mode lock poisoned; falling back to All");
                *poisoned.into_inner()
            }
        }
    }

    /// Update the in-memory mode (the caller also persists it via
    /// [`registry::set_dock_badge_mode`](crate::registry::set_dock_badge_mode)). A
    /// poisoned lock still applies the write rather than panicking.
    pub fn set_mode(&self, mode: DockBadgeMode) {
        match self.mode.write() {
            Ok(mut guard) => *guard = mode,
            Err(poisoned) => {
                tracing::warn!("badge-mode lock poisoned; applying write anyway");
                *poisoned.into_inner() = mode;
            }
        }
    }
}

/// Pure dock-badge arithmetic (Story 10.3): map the mode + the cross-account aggregate
/// to the badge number, or `None` to clear. `All` badges the count of unread rooms;
/// `Mentions` badges the total unread-mention count; `Off` never badges. A zero total
/// clears the badge in every mode (`Some(0)` is never returned). This is the unit-tested
/// seam — no `Platform`, no `Client`.
pub fn badge_count(mode: DockBadgeMode, unread_rooms: u32, mention_total: u32) -> Option<u32> {
    match mode {
        DockBadgeMode::All => (unread_rooms > 0).then_some(unread_rooms),
        DockBadgeMode::Mentions => (mention_total > 0).then_some(mention_total),
        DockBadgeMode::Off => None,
    }
}

/// Compute the badge for `mode` + the aggregate and push it through the [`Platform`]
/// port to the OS dock (Story 10.3). A platform failure is logged at `warn` and
/// swallowed — the badge must never block or abort the inbox merge. An unset app handle
/// (headless / tests) is an honest no-op inside the port.
pub fn apply(platform: &dyn Platform, mode: DockBadgeMode, unread_rooms: u32, mention_total: u32) {
    let count = badge_count(mode, unread_rooms, mention_total);
    if let Err(error) = platform.set_badge_count(count) {
        tracing::warn!(%error, "could not set dock badge count");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_mode_badges_unread_room_count() {
        // `All` badges the number of unread rooms; the mention total is ignored.
        assert_eq!(badge_count(DockBadgeMode::All, 3, 0), Some(3));
        assert_eq!(badge_count(DockBadgeMode::All, 7, 42), Some(7));
    }

    #[test]
    fn mentions_mode_badges_mention_total() {
        // `Mentions` badges the summed mention count; the unread-room count is ignored.
        assert_eq!(badge_count(DockBadgeMode::Mentions, 0, 5), Some(5));
        assert_eq!(badge_count(DockBadgeMode::Mentions, 9, 2), Some(2));
    }

    #[test]
    fn off_mode_never_badges() {
        // `Off` clears the badge regardless of any unread/mention state.
        assert_eq!(badge_count(DockBadgeMode::Off, 0, 0), None);
        assert_eq!(badge_count(DockBadgeMode::Off, 12, 34), None);
    }

    #[test]
    fn zero_total_clears_badge_in_every_countable_mode() {
        // A zero total clears the badge (never `Some(0)`) in `All` and `Mentions`.
        assert_eq!(badge_count(DockBadgeMode::All, 0, 0), None);
        assert_eq!(badge_count(DockBadgeMode::All, 0, 9), None);
        assert_eq!(badge_count(DockBadgeMode::Mentions, 0, 0), None);
        assert_eq!(badge_count(DockBadgeMode::Mentions, 4, 0), None);
    }

    #[test]
    fn badge_config_round_trips_mode() {
        let config = BadgeConfig::new(DockBadgeMode::All);
        assert_eq!(config.mode(), DockBadgeMode::All);
        config.set_mode(DockBadgeMode::Mentions);
        assert_eq!(config.mode(), DockBadgeMode::Mentions);
        config.set_mode(DockBadgeMode::Off);
        assert_eq!(config.mode(), DockBadgeMode::Off);
    }
}
