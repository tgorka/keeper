//! Progress and status events (Story 29.1, AD-51).
//!
//! Two shapes, deliberately, because the codebase already proved both are
//! needed: a **stream** for a subscribed UI (the `export_start` precedent) and
//! a **polled snapshot** for the ~1 Hz tray tick, which must render correctly
//! when no webview is subscribed at all.
//!
//! The engine emits events through a sink type matching `keeper-core`'s
//! convention — `Box<dyn Fn(T) -> bool + Send + Sync>`, where returning `false`
//! means "stop producing". The Tauri shell wraps `Channel::send(..).is_ok()`;
//! `keeper-syncd` writes log lines; tests push into a `Vec`.

use serde::{Deserialize, Serialize};

use crate::profile::ProfileState;

/// A sink for progress events. `false` stops the producer.
pub type ProgressSink = Box<dyn Fn(SyncProgress) -> bool + Send + Sync>;

/// What the engine is doing right now.
///
/// Coarse on purpose: this drives a tray glyph and a status line, so a phase
/// exists only if a user would describe the work differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncPhase {
    Scanning,
    Fetching,
    Applying,
    Staging,
    Committing,
    Pushing,
    /// Large-object transfer, tracked separately because its byte counts dwarf
    /// everything else and a user reads it as a different activity.
    TransferringLfs,
    Verifying,
    Idle,
}

impl SyncPhase {
    /// Whether this phase should animate the tray glyph.
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// Short human label for the tray status line.
    pub fn label(self) -> &'static str {
        match self {
            Self::Scanning => "Scanning",
            Self::Fetching => "Fetching",
            Self::Applying => "Applying",
            Self::Staging => "Staging",
            Self::Committing => "Committing",
            Self::Pushing => "Pushing",
            Self::TransferringLfs => "Transferring",
            Self::Verifying => "Verifying",
            Self::Idle => "Idle",
        }
    }
}

/// One progress update for one profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub profile_id: String,
    pub profile_name: String,
    pub phase: SyncPhase,
    /// Files processed so far in this operation.
    pub files_done: u64,
    /// Total files, when known. Unknown during a streaming scan, and the UI
    /// must render an indeterminate meter rather than inventing a denominator.
    pub files_total: Option<u64>,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    /// The item currently being worked on, for the detail line. A repository
    /// -relative path, never an absolute one — absolute paths leak home
    /// directory names into logs and screenshots.
    pub current: Option<String>,
}

impl SyncProgress {
    pub fn idle(profile_id: impl Into<String>, profile_name: impl Into<String>) -> Self {
        Self {
            profile_id: profile_id.into(),
            profile_name: profile_name.into(),
            phase: SyncPhase::Idle,
            files_done: 0,
            files_total: None,
            bytes_done: 0,
            bytes_total: None,
            current: None,
        }
    }

    /// Completion in `[0.0, 1.0]`, or `None` when indeterminate.
    ///
    /// Prefers bytes over files: a 4 GB video and a 2 KB note are one file
    /// each, and a file-counted bar would sit at 50% for ten minutes.
    pub fn fraction(&self) -> Option<f64> {
        if let Some(total) = self.bytes_total.filter(|t| *t > 0) {
            return Some((self.bytes_done as f64 / total as f64).clamp(0.0, 1.0));
        }
        let total = self.files_total.filter(|t| *t > 0)?;
        Some((self.files_done as f64 / total as f64).clamp(0.0, 1.0))
    }
}

/// The polled snapshot the tray and any late-subscribing view read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub profile_id: String,
    pub profile_name: String,
    pub state: ProfileState,
    pub phase: SyncPhase,
    pub files_done: u64,
    pub files_total: Option<u64>,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    /// Units still queued, including deferred ones. Non-zero while offline,
    /// which is how the UI can say "12 changes waiting" honestly.
    pub pending: u32,
    /// Sticky warning, last-write-wins, cleared only by a clean run. Mirrors
    /// `RecordingStatusVm::warning` so the banner behaves identically.
    pub warning: Option<String>,
    /// Terminal error for this profile, if it stopped.
    pub error: Option<String>,
    /// Wall-clock ms of the last successful sync, if any.
    pub last_sync_ms: Option<i64>,
}

impl SyncStatus {
    pub fn idle(profile_id: impl Into<String>, profile_name: impl Into<String>) -> Self {
        Self {
            profile_id: profile_id.into(),
            profile_name: profile_name.into(),
            state: ProfileState::Idle,
            phase: SyncPhase::Idle,
            files_done: 0,
            files_total: None,
            bytes_done: 0,
            bytes_total: None,
            pending: 0,
            warning: None,
            error: None,
            last_sync_ms: None,
        }
    }
}

/// How the tray should render, given every profile at once.
///
/// A pure reduction so the icon decision is unit-testable and so two profiles
/// can never fight over the glyph (AD-51). The precedence is deliberate:
/// **problems outrank activity outrank calm** — a user must never miss a
/// warning because something else happened to be syncing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraySyncState {
    /// No profiles configured — the sync glyph is absent entirely.
    Absent,
    /// Configured and healthy, nothing in flight.
    Armed,
    /// At least one profile actively transferring.
    Active,
    /// Nothing wrong, but at least one profile cannot proceed (paused, or its
    /// volume is detached).
    Paused,
    /// At least one profile needs attention.
    Warning,
}

/// Reduce every profile's status to one tray state.
pub fn tray_state(statuses: &[SyncStatus]) -> TraySyncState {
    if statuses.is_empty() {
        return TraySyncState::Absent;
    }
    let mut has_warning = false;
    let mut has_active = false;
    let mut has_paused = false;
    for s in statuses {
        if s.error.is_some() || s.warning.is_some() || s.state.is_warning() {
            has_warning = true;
        }
        if s.state.is_active() || s.phase.is_active() {
            has_active = true;
        }
        if matches!(s.state, ProfileState::Paused) {
            has_paused = true;
        }
    }
    // Warning first: a problem must never be masked by concurrent activity.
    if has_warning {
        TraySyncState::Warning
    } else if has_active {
        TraySyncState::Active
    } else if has_paused {
        TraySyncState::Paused
    } else {
        TraySyncState::Armed
    }
}

/// Human byte formatting for the tray line, matching `tray.rs`'s existing
/// `format_size` shape so the two subsystems read alike.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// The tray's single status line (AD-51).
pub fn status_line(status: &SyncStatus) -> String {
    match status.state {
        ProfileState::MediaAbsent => format!("{} — drive not connected", status.profile_name),
        ProfileState::Paused => format!("{} — paused", status.profile_name),
        ProfileState::Offline if status.pending > 0 => {
            format!(
                "{} — offline, {} waiting",
                status.profile_name, status.pending
            )
        }
        ProfileState::Offline => format!("{} — offline", status.profile_name),
        ProfileState::NeedsAttention => format!(
            "{} — {}",
            status.profile_name,
            status.error.as_deref().unwrap_or("needs attention")
        ),
        _ if status.phase.is_active() => {
            let mut line = format!("{} {}", status.phase.label(), status.profile_name);
            if let Some(total) = status.files_total {
                line.push_str(&format!(" — {}/{} files", status.files_done, total));
            } else if status.files_done > 0 {
                line.push_str(&format!(" — {} files", status.files_done));
            }
            if let Some(total) = status.bytes_total.filter(|t| *t > 0) {
                line.push_str(&format!(
                    " · {} of {}",
                    format_bytes(status.bytes_done),
                    format_bytes(total)
                ));
            } else if status.bytes_done > 0 {
                line.push_str(&format!(" · {}", format_bytes(status.bytes_done)));
            }
            line
        }
        _ => format!("{} — up to date", status.profile_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(name: &str) -> SyncStatus {
        SyncStatus::idle("id", name)
    }

    #[test]
    fn no_profiles_means_no_sync_glyph_at_all() {
        assert_eq!(tray_state(&[]), TraySyncState::Absent);
    }

    #[test]
    fn a_warning_is_never_masked_by_concurrent_activity() {
        // The whole point of the precedence order.
        let mut busy = status("a");
        busy.phase = SyncPhase::Pushing;
        let mut broken = status("b");
        broken.warning = Some("rename required".into());
        assert_eq!(tray_state(&[busy, broken]), TraySyncState::Warning);
    }

    #[test]
    fn activity_outranks_paused() {
        let mut busy = status("a");
        busy.phase = SyncPhase::Fetching;
        let mut paused = status("b");
        paused.state = ProfileState::Paused;
        assert_eq!(tray_state(&[busy, paused]), TraySyncState::Active);
    }

    #[test]
    fn a_detached_volume_reads_as_a_warning_not_a_failure() {
        // AD-48: the user must be told, but nothing is broken.
        let mut absent = status("pendrive");
        absent.state = ProfileState::MediaAbsent;
        assert_eq!(
            tray_state(std::slice::from_ref(&absent)),
            TraySyncState::Warning
        );
        assert_eq!(status_line(&absent), "pendrive — drive not connected");
    }

    #[test]
    fn progress_prefers_bytes_because_files_lie_about_big_ones() {
        let mut p = SyncProgress::idle("id", "n");
        p.files_done = 1;
        p.files_total = Some(2);
        p.bytes_done = 100;
        p.bytes_total = Some(1_000);
        assert_eq!(p.fraction(), Some(0.1));
    }

    #[test]
    fn progress_is_indeterminate_when_no_total_is_known() {
        let mut p = SyncProgress::idle("id", "n");
        p.files_done = 7;
        assert_eq!(p.fraction(), None);
    }

    #[test]
    fn progress_never_exceeds_one_even_if_totals_are_stale() {
        let mut p = SyncProgress::idle("id", "n");
        p.bytes_done = 5_000;
        p.bytes_total = Some(1_000);
        assert_eq!(p.fraction(), Some(1.0));
    }

    #[test]
    fn a_zero_total_does_not_divide_by_zero() {
        let mut p = SyncProgress::idle("id", "n");
        p.bytes_total = Some(0);
        p.files_total = Some(0);
        assert_eq!(p.fraction(), None);
    }

    #[test]
    fn offline_status_says_how_much_is_waiting() {
        let mut s = status("tgdrive");
        s.state = ProfileState::Offline;
        s.pending = 12;
        assert_eq!(status_line(&s), "tgdrive — offline, 12 waiting");
    }

    #[test]
    fn an_active_line_carries_counts_and_bytes() {
        let mut s = status("tgdrive");
        s.state = ProfileState::Syncing;
        s.phase = SyncPhase::TransferringLfs;
        s.files_done = 42;
        s.files_total = Some(310);
        s.bytes_done = 1_288_490_188;
        s.bytes_total = Some(5_046_586_573);
        assert_eq!(
            status_line(&s),
            "Transferring tgdrive — 42/310 files · 1.2 GB of 4.7 GB"
        );
    }

    #[test]
    fn byte_formatting_crosses_units_cleanly() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }
}
