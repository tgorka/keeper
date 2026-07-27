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

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::lfs::basic::TransferEvent;
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
    /// Whether this phase is doing anything at all.
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// Whether bytes are actually crossing the network in this phase.
    ///
    /// Split out from [`Self::is_active`] because the tray animates only for
    /// real movement: a scan or a commit is work too, but animating over it
    /// promises a transfer that is not happening.
    pub fn is_transferring(self) -> bool {
        matches!(self, Self::Fetching | Self::TransferringLfs)
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

/// One object's contribution to a transfer's byte totals.
#[derive(Debug, Clone, Copy, Default)]
struct ObjectBytes {
    /// Size announced by [`TransferEvent::Started`].
    size: u64,
    /// High-water mark of what this object has reported moving.
    done: u64,
}

/// Byte totals folded from a stream of [`TransferEvent`]s.
///
/// Exists because the raw stream cannot be read as a total.
/// [`TransferEvent::Progress`] carries each object's OWN cumulative count while
/// up to `lfs::basic::DEFAULT_CONCURRENT_TRANSFERS` objects interleave: adding
/// the reported numbers re-counts the same bytes on every tick, and taking the
/// latest one alone makes the figure sawtooth as objects take turns. Keeping
/// one entry per oid and summing the *deltas* is the only reduction that is
/// both correct and monotonic.
///
/// Monotonic is a requirement, not a preference: `BasicTransfer` retries an
/// object in place, and a retry that cannot resume restarts that object's
/// counter at zero. A bar that walks backwards reads as a bug, so each object
/// contributes its high-water mark rather than its last report.
///
/// [`TransferEvent::Progress`]: crate::lfs::basic::TransferEvent::Progress
#[derive(Debug, Default, Clone)]
pub struct TransferTally {
    objects: HashMap<String, ObjectBytes>,
    total: u64,
    done: u64,
}

impl TransferTally {
    /// Sum of every announced size, or `None` when nothing has started yet.
    ///
    /// `None` rather than `Some(0)`: a zero denominator is indeterminate, and
    /// the UI must draw a spinner rather than an empty bar.
    pub fn bytes_total(&self) -> Option<u64> {
        (self.total > 0).then_some(self.total)
    }

    /// Bytes moved so far across every object in this transfer.
    pub fn bytes_done(&self) -> u64 {
        self.done
    }

    /// Fold one transfer event into the totals.
    pub fn fold(&mut self, event: &TransferEvent) {
        match event {
            TransferEvent::Started { oid, size } => {
                // A second `Started` for one oid is a re-driven journal unit,
                // not new work. Counting its size again would inflate the
                // denominator and strand the bar short of full forever.
                if let Entry::Vacant(slot) = self.objects.entry(oid.clone()) {
                    slot.insert(ObjectBytes {
                        size: *size,
                        done: 0,
                    });
                    self.total = self.total.saturating_add(*size);
                }
            }
            TransferEvent::Progress { oid, bytes_done } => self.advance(oid, *bytes_done),
            TransferEvent::Completed { oid } => {
                // The last `Progress` before completion is usually swallowed by
                // the coalescer, and an object smaller than one chunk may never
                // emit one at all, so completion is what actually retires an
                // object's remaining bytes.
                if let Some(size) = self.objects.get(oid).map(|object| object.size) {
                    self.advance(oid, size);
                }
            }
            // A failure deliberately changes nothing. The announced size stays
            // in the denominator because that work was real and shrinking it
            // would jump the bar forward; the partial count stays in the
            // numerator because those bytes did move and removing them would
            // jump it backwards. The remainder is simply never made up, which
            // is exactly what a failed transfer looks like.
            TransferEvent::Failed { .. } => {}
        }
    }

    /// Stamp the byte totals onto a progress event.
    pub fn apply(&self, progress: &mut SyncProgress) {
        progress.bytes_done = self.done;
        progress.bytes_total = self.bytes_total();
    }

    /// Raise one object's high-water mark, capped at its announced size.
    fn advance(&mut self, oid: &str, reported: u64) {
        let Some(object) = self.objects.get_mut(oid) else {
            // No `Started` for this oid, so there is no denominator to charge
            // it against; counting it anyway would push `bytes_done` past
            // `bytes_total` and drive `fraction` to a permanent 1.0.
            return;
        };
        let capped = if object.size > 0 {
            reported.min(object.size)
        } else {
            reported
        };
        if capped <= object.done {
            return;
        }
        self.done = self.done.saturating_add(capped - object.done);
        object.done = capped;
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
/// **problems outrank transfers outrank activity outrank calm** — a user must
/// never miss a warning because something else happened to be syncing, and a
/// profile that is merely scanning must never claim the animation that means
/// "bytes are moving right now".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraySyncState {
    /// No profiles configured — the sync glyph is absent entirely.
    Absent,
    /// Configured and healthy, nothing in flight.
    Armed,
    /// At least one profile is moving bytes over the network — a fetch, or an
    /// LFS transfer. Ranked above `Active` because when both are happening the
    /// user is waiting on the wire, and that is the thing worth animating.
    Transferring,
    /// At least one profile is working with nothing on the wire: a scan, a
    /// merge, a commit, a verify. Kept distinct from `Transferring` so the tray
    /// animates only when there is real movement to animate.
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
    let mut has_transfer = false;
    let mut has_active = false;
    let mut has_paused = false;
    for s in statuses {
        if s.error.is_some() || s.warning.is_some() || s.state.is_warning() {
            has_warning = true;
        }
        if s.phase.is_transferring() {
            has_transfer = true;
        }
        if s.state.is_active() || s.phase.is_active() {
            has_active = true;
        }
        if matches!(s.state, ProfileState::Paused) {
            has_paused = true;
        }
    }
    // Warning first: a problem must never be masked by concurrent activity.
    // Transfers next: of two busy profiles, the one on the wire is the one the
    // user is actually waiting for.
    if has_warning {
        TraySyncState::Warning
    } else if has_transfer {
        TraySyncState::Transferring
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
        // "up to date" is only honest when nothing is waiting. A queued unit
        // means work has been accepted but not yet published, and reporting
        // that as up to date is how a user comes to believe a file reached the
        // server when it did not.
        _ if status.pending > 0 => {
            format!(
                "{} — {} waiting to sync",
                status.profile_name, status.pending
            )
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
        // Committing is work with nothing on the wire, which is exactly what
        // separates `Active` from `Transferring`.
        let mut busy = status("a");
        busy.phase = SyncPhase::Committing;
        let mut paused = status("b");
        paused.state = ProfileState::Paused;
        assert_eq!(tray_state(&[busy, paused]), TraySyncState::Active);
    }

    #[test]
    fn a_transfer_outranks_other_activity_but_never_a_warning() {
        // Of two busy profiles the one on the wire wins: it is the one the user
        // is waiting for, and it is the only one an animation does not lie
        // about.
        let mut scanning = status("a");
        scanning.phase = SyncPhase::Scanning;
        let mut fetching = status("b");
        fetching.phase = SyncPhase::Fetching;
        assert_eq!(
            tray_state(&[scanning.clone(), fetching.clone()]),
            TraySyncState::Transferring
        );

        let mut lfs = status("c");
        lfs.phase = SyncPhase::TransferringLfs;
        assert_eq!(
            tray_state(std::slice::from_ref(&lfs)),
            TraySyncState::Transferring
        );

        // A problem still masks everything, transfers included.
        let mut broken = status("d");
        broken.warning = Some("rename required".into());
        assert_eq!(tray_state(&[fetching, broken]), TraySyncState::Warning);
    }

    #[test]
    fn a_transfer_outranks_a_paused_sibling() {
        let mut fetching = status("a");
        fetching.phase = SyncPhase::TransferringLfs;
        let mut paused = status("b");
        paused.state = ProfileState::Paused;
        assert_eq!(tray_state(&[fetching, paused]), TraySyncState::Transferring);
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

    fn started(oid: &str, size: u64) -> TransferEvent {
        TransferEvent::Started {
            oid: oid.to_owned(),
            size,
        }
    }

    #[test]
    fn interleaved_objects_never_double_count_and_never_go_backwards() {
        // Two concurrent objects each reporting their OWN cumulative count is
        // the shape `download_all` produces with up to eight in flight. Adding
        // the raw numbers would re-count on every tick; taking the latest one
        // would sawtooth between them.
        let mut tally = TransferTally::default();
        tally.fold(&started("a", 1_000));
        tally.fold(&started("b", 3_000));
        assert_eq!(tally.bytes_total(), Some(4_000));

        let script = [
            TransferEvent::Progress {
                oid: "a".to_owned(),
                bytes_done: 400,
            },
            TransferEvent::Progress {
                oid: "b".to_owned(),
                bytes_done: 1_500,
            },
            TransferEvent::Progress {
                oid: "a".to_owned(),
                bytes_done: 900,
            },
            // A retry that could not resume restarts this object's counter.
            // The high-water mark is what keeps the bar from walking backwards.
            TransferEvent::Progress {
                oid: "b".to_owned(),
                bytes_done: 0,
            },
            TransferEvent::Progress {
                oid: "b".to_owned(),
                bytes_done: 2_000,
            },
        ];
        let mut previous = 0;
        for event in &script {
            tally.fold(event);
            assert!(
                tally.bytes_done() >= previous,
                "{event:?} moved the total backwards: {previous} -> {}",
                tally.bytes_done()
            );
            previous = tally.bytes_done();
        }
        assert_eq!(tally.bytes_done(), 2_900, "900 of a plus 2000 of b");

        // Completion retires whatever the coalescer swallowed.
        tally.fold(&TransferEvent::Completed {
            oid: "a".to_owned(),
        });
        assert_eq!(tally.bytes_done(), 3_000);

        let mut progress = SyncProgress::idle("id", "n");
        tally.apply(&mut progress);
        assert_eq!(progress.bytes_total, Some(4_000));
        assert_eq!(progress.fraction(), Some(0.75));
    }

    #[test]
    fn a_failure_leaves_the_totals_consistent() {
        let mut tally = TransferTally::default();
        tally.fold(&started("a", 1_000));
        tally.fold(&started("b", 1_000));
        tally.fold(&TransferEvent::Progress {
            oid: "b".to_owned(),
            bytes_done: 250,
        });
        tally.fold(&TransferEvent::Failed {
            oid: "b".to_owned(),
            code: "network",
            error: "connection reset".to_owned(),
        });

        // The denominator keeps the failed object: that work was real, and
        // dropping it would jump the bar forward on a failure. The numerator
        // keeps its partial bytes: they did move, and dropping them would jump
        // the bar backwards.
        assert_eq!(tally.bytes_total(), Some(2_000));
        assert_eq!(tally.bytes_done(), 250);

        tally.fold(&TransferEvent::Completed {
            oid: "a".to_owned(),
        });
        assert_eq!(tally.bytes_done(), 1_250);
        let mut progress = SyncProgress::idle("id", "n");
        tally.apply(&mut progress);
        assert!(
            progress.bytes_done <= progress.bytes_total.unwrap_or(0),
            "a failure must never push the numerator past the denominator"
        );
        assert_eq!(progress.fraction(), Some(0.625));
    }

    #[test]
    fn a_re_driven_unit_does_not_inflate_the_denominator() {
        // The journal re-drives a unit after a crash, so the same oid can be
        // announced twice in one run. Counting its size twice would strand the
        // bar permanently short of full.
        let mut tally = TransferTally::default();
        tally.fold(&started("a", 500));
        tally.fold(&started("a", 500));
        assert_eq!(tally.bytes_total(), Some(500));

        // An `AlreadyPresent` upload completes without ever starting: nothing
        // was announced and nothing moved, so it must not be charged.
        tally.fold(&TransferEvent::Completed {
            oid: "never-started".to_owned(),
        });
        assert_eq!(tally.bytes_done(), 0);
        assert_eq!(tally.bytes_total(), Some(500));

        // Nothing has started at all: indeterminate, not "0 of 0".
        let empty = TransferTally::default();
        assert_eq!(empty.bytes_total(), None);
        let mut progress = SyncProgress::idle("id", "n");
        empty.apply(&mut progress);
        assert_eq!(progress.fraction(), None);
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
    fn a_profile_with_queued_work_is_never_reported_as_up_to_date() {
        // Saying "up to date (1 queued)" is how a user comes to believe a file
        // reached the server when it is still sitting in the journal.
        let mut s = status("tgdrive");
        s.state = ProfileState::Idle;
        s.pending = 3;
        assert_eq!(status_line(&s), "tgdrive — 3 waiting to sync");

        s.pending = 0;
        assert_eq!(status_line(&s), "tgdrive — up to date");
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
