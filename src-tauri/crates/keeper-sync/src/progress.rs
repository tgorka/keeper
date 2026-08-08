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
use std::time::Instant;

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
    /// Large-object upload, tracked separately because its byte counts dwarf
    /// everything else and a user reads it as a different activity.
    ///
    /// Split from the download direction (rather than one `TransferringLfs`
    /// carrying a flag) because the tray renders the two differently and the
    /// phase is the only thing that reaches it: a state the icon must
    /// distinguish has to be distinguishable in the type.
    UploadingLfs,
    /// Large-object download. See [`Self::UploadingLfs`].
    DownloadingLfs,
    Verifying,
    Idle,
}

/// Which way bytes are moving, for the tray glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Up,
    Down,
}

impl SyncPhase {
    /// Whether this phase is doing anything at all.
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// Whether a producer measures this phase's bytes, and therefore whether a
    /// `bytes_per_second` figure can ever accompany it (Story 34.8, AD-34-13).
    ///
    /// This replaces an `is_transferring` defined as `direction().is_some()` and
    /// documented with "a push is bytes on the wire by any honest reading —
    /// for a folder of text files it is *all* of them". True as physics, false
    /// as a description of this program. `Pushing` is published by
    /// `Engine::do_push` carrying a **file** count and nothing else, because the
    /// push is `git push --porcelain` run through `git::cli::capture`, which
    /// collects a finished process's output: the byte counters exist only in a
    /// `--progress` stderr stream nothing in this crate reads. So the phase
    /// AD-34-13 most obviously describes was the one phase that could never
    /// satisfy it, and the predicate saying otherwise had no callers and no
    /// test — nothing could disagree with it, so nothing did.
    ///
    /// Two producers stamp the figure and they are the whole list:
    /// `Engine::fold_fetch_progress` under `Fetching`, and
    /// [`TransferTally::apply`], which the LFS legs drive under `UploadingLfs`
    /// and `DownloadingLfs`. Neither is taken on trust:
    /// `only_the_phases_with_a_byte_producer_claim_a_rate` asserts the set below
    /// and drives the tally, and `the_fetch_leg_stamps_a_rate_off_the_high_water_mark`
    /// in `engine.rs` drives the other. Gutting either producer, or widening
    /// this answer to a phase without one, fails a test.
    ///
    /// Deliberately **not** [`Self::direction`]: a push really is bytes going
    /// up and the tray's arrow is right to say so. "Which way" and "can we time
    /// it" are different questions, and the old predicate answered the second
    /// with the first. A `Pushing` folder therefore still draws a determinate
    /// bar off its file count — an honest meter — with no rate beside it.
    ///
    /// Total rather than a `matches!`, for [`Self::direction`]'s reason: a phase
    /// added later has to be classified here or the crate does not compile.
    pub fn carries_rate(self) -> bool {
        match self {
            Self::Fetching | Self::UploadingLfs | Self::DownloadingLfs => true,
            Self::Scanning
            | Self::Applying
            | Self::Staging
            | Self::Committing
            | Self::Pushing
            | Self::Verifying
            | Self::Idle => false,
        }
    }

    /// Which way this phase is moving bytes, if it is moving any.
    ///
    /// The tray's whole direction story comes from here, so the mapping is
    /// deliberately total rather than a pair of `matches!` that could disagree
    /// with each other as phases are added.
    pub fn direction(self) -> Option<TransferDirection> {
        match self {
            Self::Fetching | Self::DownloadingLfs => Some(TransferDirection::Down),
            Self::Pushing | Self::UploadingLfs => Some(TransferDirection::Up),
            Self::Scanning
            | Self::Applying
            | Self::Staging
            | Self::Committing
            | Self::Verifying
            | Self::Idle => None,
        }
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
            // Both directions keep the word the line has always used: the
            // status line already names the profile and the byte counts, and
            // "Uploading 3 of 8" would be the third place one sync says which
            // way it is going.
            Self::UploadingLfs | Self::DownloadingLfs => "Transferring",
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
    /// Whole bytes per second, or `None` when no honest figure exists yet.
    ///
    /// Derived by [`RateMeter`], which never yields zero: a rate of nothing is
    /// the absence of a transfer, not a measurement of one, and the UI renders
    /// `None` as nothing rather than "0 B/s".
    pub bytes_per_second: Option<u64>,
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
            bytes_per_second: None,
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

/// Shortest window a rate may be measured over, in milliseconds.
///
/// Both byte producers sample at ~100 ms — `git::fetch::REPORT_INTERVAL_MS` and
/// `lfs::basic::DEFAULT_PROGRESS_INTERVAL` — and one sample can carry a whole
/// chunk that a buffered reader handed over at once. Dividing by 100 ms would
/// therefore report a burst as if it were the sustained rate, so a figure is
/// withheld until a full second of movement backs it.
const RATE_MIN_WINDOW_MS: u64 = 1_000;

/// How long a measurement window runs before it is reopened, in milliseconds.
///
/// The window has to close, or the figure becomes the average since the
/// transfer began: on a multi-gigabyte push that average would still read
/// "12 MB/s" minutes after the connection dropped to a crawl. Reopening every
/// two seconds keeps the number about the recent past while staying long
/// enough to be steady.
const RATE_WINDOW_MS: u64 = 2_000;

/// A transfer rate in whole bytes per second, measured over a rolling window.
///
/// Time is a parameter rather than a read of the clock, matching
/// `lfs::basic::ProgressCoalescer`, which is what makes the boundaries
/// testable.
///
/// Two rules make the figure honest, and both are expressed as `None`:
///
/// * **Not enough time.** One sample is a point, not a rate, and a window
///   shorter than [`RATE_MIN_WINDOW_MS`] is noise (see that constant).
/// * **Nothing moved.** A window that carries no bytes has no rate to report.
///   "0 B/s" would claim a measurement where there is only an idle wire — a
///   pack that finished arriving while its deltas resolve, or an object being
///   retried. So the meter never yields `Some(0)`: every `Some` is at least
///   1 B/s, which is what lets the UI render `None` as nothing without ever
///   having to special-case a zero.
///
/// The counter it is fed must be cumulative. Both feeds are monotonic by
/// construction — [`TransferTally`]'s per-object high-water marks, and the
/// `fetched` maximum on the fetch leg, where gitoxide's per-node counters
/// restart on every phase — and a counter that dropped anyway is absorbed by a
/// saturating subtraction: it reports no movement, so the meter falls silent
/// and re-anchors within one window instead of computing a negative rate.
#[derive(Debug, Default, Clone)]
pub struct RateMeter {
    /// When the open window started, and the byte count it started from.
    window: Option<(Instant, u64)>,
    /// The last figure measured over a full window, held while the next one
    /// fills so the display does not blink to nothing every two seconds.
    last: Option<u64>,
}

impl RateMeter {
    /// Fold in a cumulative byte count observed at `now` and return the rate.
    pub fn observe(&mut self, bytes: u64, now: Instant) -> Option<u64> {
        let Some((opened, opening_bytes)) = self.window else {
            self.window = Some((now, bytes));
            return None;
        };

        // Milliseconds, because a rate in whole bytes per second cannot resolve
        // anything finer and `u128` arithmetic buys nothing here.
        let elapsed = u64::try_from(now.saturating_duration_since(opened).as_millis())
            .unwrap_or(u64::MAX)
            .max(1);
        if elapsed < RATE_MIN_WINDOW_MS {
            return self.last;
        }

        let moved = bytes.saturating_sub(opening_bytes);
        // `elapsed` is at least `RATE_MIN_WINDOW_MS` here, so this divides by a
        // full second at minimum; `saturating_mul` keeps an absurd byte count
        // from wrapping on the way in.
        let rate = moved.saturating_mul(1_000) / elapsed;
        self.last = (rate > 0).then_some(rate);
        if elapsed >= RATE_WINDOW_MS {
            self.window = Some((now, bytes));
        }
        self.last
    }

    /// The current rate, without folding a new observation in.
    pub fn bytes_per_second(&self) -> Option<u64> {
        self.last
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
    rate: RateMeter,
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

    /// Fold one transfer event into the totals, observed now.
    pub fn fold(&mut self, event: &TransferEvent) {
        self.fold_at(event, Instant::now());
    }

    /// Fold one transfer event into the totals as of `now`.
    ///
    /// The clock is a parameter here for the same reason it is one on
    /// `lfs::basic::ProgressCoalescer::should_emit`: the rate's window
    /// boundaries are only testable if a test can choose when things happened.
    pub fn fold_at(&mut self, event: &TransferEvent, now: Instant) {
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
        // Every arm above is a byte observation, including the ones that move
        // nothing: a `Failed` that stalls the count for a second is exactly the
        // moment the rate should start falling.
        self.rate.observe(self.done, now);
    }

    /// Stamp the byte totals and the current rate onto a progress event.
    pub fn apply(&self, progress: &mut SyncProgress) {
        progress.bytes_done = self.done;
        progress.bytes_total = self.bytes_total();
        progress.bytes_per_second = self.rate.bytes_per_second();
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
    /// Paths the completeness gate is holding inside their quiescence window:
    /// seen, deliberately not yet queued, and therefore invisible to
    /// `pending`, which counts journal rows (AD-34-10). A folder with five
    /// thousand files still being written has no journal rows at all, which is
    /// how "up to date" came to be printed over it. Maintained by the scan,
    /// which is the only thing that knows.
    pub settling: u32,
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
            settling: 0,
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
    /// At least one profile is sending bytes and none is receiving any.
    Uploading,
    /// At least one profile is receiving bytes and none is sending any.
    Downloading,
    /// Both directions are in flight at once, across one profile or several.
    ///
    /// Its own state rather than a tie broken towards one direction: with four
    /// folders configured, "something is uploading and something is downloading"
    /// is the common case, and picking a winner would make the tray lie about
    /// half of it.
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
    let mut has_up = false;
    let mut has_down = false;
    let mut has_active = false;
    let mut has_paused = false;
    for s in statuses {
        if s.error.is_some() || s.warning.is_some() || s.state.is_warning() {
            has_warning = true;
        }
        match s.phase.direction() {
            Some(TransferDirection::Up) => has_up = true,
            Some(TransferDirection::Down) => has_down = true,
            None => {}
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
    // user is actually waiting for. Both directions at once outrank either alone
    // — it is the only answer that is true about every profile.
    if has_warning {
        TraySyncState::Warning
    } else if has_up && has_down {
        TraySyncState::Transferring
    } else if has_up {
        TraySyncState::Uploading
    } else if has_down {
        TraySyncState::Downloading
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
        // "up to date" is only honest when nothing is waiting for ANY reason
        // (AD-34-10). The two reasons are different facts and are worded
        // differently: a queued unit means work has been accepted but not yet
        // published, and a settling file means work has been *seen* and is
        // being deliberately held until its writer stops. Reporting either as
        // up to date is how a user comes to believe a file reached the server
        // when it did not — and the settling case is the one that used to be
        // invisible here, because it has no journal row to be counted by.
        //
        // The wording matches the Pending list's own "Waiting for writes to
        // stop", so the tray and the window never explain the same wait two
        // different ways.
        _ if status.pending > 0 && status.settling > 0 => format!(
            "{} — {} waiting to sync, {} waiting for writes to stop",
            status.profile_name, status.pending, status.settling
        ),
        _ if status.pending > 0 => {
            format!(
                "{} — {} waiting to sync",
                status.profile_name, status.pending
            )
        }
        _ if status.settling > 0 => format!(
            "{} — {} waiting for writes to stop",
            status.profile_name, status.settling
        ),
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
            TraySyncState::Downloading
        );

        let mut lfs = status("c");
        lfs.phase = SyncPhase::UploadingLfs;
        assert_eq!(
            tray_state(std::slice::from_ref(&lfs)),
            TraySyncState::Uploading
        );

        // One of each, across two profiles: the only honest answer names both.
        let mut down = status("e");
        down.phase = SyncPhase::DownloadingLfs;
        assert_eq!(
            tray_state(&[lfs.clone(), down]),
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
        fetching.phase = SyncPhase::DownloadingLfs;
        let mut paused = status("b");
        paused.state = ProfileState::Paused;
        assert_eq!(tray_state(&[fetching, paused]), TraySyncState::Downloading);
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

    /// `t0 + ms`, for reading a rate timeline as the wall clock it stands in for.
    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + std::time::Duration::from_millis(ms)
    }

    #[test]
    fn a_rate_waits_for_a_full_second_before_it_claims_one() {
        let t0 = Instant::now();
        let mut meter = RateMeter::default();

        // One sample is a point, not a rate.
        assert_eq!(meter.observe(0, t0), None);
        // A 100 ms sample is one producer tick, and one tick can be a whole
        // buffered chunk: 1 MB in 100 ms would read as 10 MB/s.
        assert_eq!(meter.observe(1_000_000, at(t0, 100)), None);

        assert_eq!(meter.observe(2_000_000, at(t0, 1_000)), Some(2_000_000));
        // Still inside the same window, so the figure widens rather than
        // restarting: 3 MB over 1.5 s.
        assert_eq!(meter.observe(3_000_000, at(t0, 1_500)), Some(2_000_000));
    }

    #[test]
    fn a_rate_is_never_zero_and_never_divides_by_no_time() {
        let t0 = Instant::now();
        let mut meter = RateMeter::default();

        // Two observations stamped the same instant. A naive elapsed of zero
        // here is exactly how a rate becomes infinite.
        assert_eq!(meter.observe(0, t0), None);
        assert_eq!(meter.observe(5_000, t0), None);

        // A real window, so the figures below are measured against something.
        assert_eq!(meter.observe(5_000, at(t0, 2_000)), Some(2_500));

        // The window reopened at 5 000 bytes, and nothing has moved since. That
        // is not a rate of zero — "0 B/s" would claim a measurement of an idle
        // wire — so there is nothing to report.
        assert_eq!(meter.observe(5_000, at(t0, 3_100)), None);
        // And 50 bytes in a minute rounds to under 1 B/s, which is the same
        // answer for the same reason.
        assert_eq!(meter.observe(5_050, at(t0, 62_000)), None);
    }

    #[test]
    fn a_counter_that_restarts_falls_silent_and_recovers() {
        // The shape of a retry that could not resume, and of a fetch phase
        // rolling over onto a node whose counter starts at zero.
        let t0 = Instant::now();
        let mut meter = RateMeter::default();
        assert_eq!(meter.observe(1_000_000, t0), None);
        assert_eq!(meter.observe(3_000_000, at(t0, 1_000)), Some(2_000_000));

        // Backwards. The subtraction saturates, so this is "nothing moved"
        // rather than a negative rate.
        assert_eq!(meter.observe(0, at(t0, 1_500)), None);
        // Past the window length, so the meter re-anchors on the lower count…
        assert_eq!(meter.observe(500_000, at(t0, 4_000)), None);
        // …and measures again from there.
        assert_eq!(meter.observe(1_500_000, at(t0, 5_000)), Some(1_000_000));
    }

    #[test]
    fn the_window_reopens_so_a_stall_stops_reporting_the_old_rate() {
        let t0 = Instant::now();
        let mut meter = RateMeter::default();
        assert_eq!(meter.observe(0, t0), None);
        assert_eq!(meter.observe(10_000_000, at(t0, 2_000)), Some(5_000_000));
        assert_eq!(meter.bytes_per_second(), Some(5_000_000));

        // Nothing more arrives. An average since the transfer began would still
        // be claiming 5 MB/s here.
        assert_eq!(meter.observe(10_000_000, at(t0, 3_000)), None);
        assert_eq!(meter.bytes_per_second(), None);
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
    fn the_tally_carries_a_rate_that_survives_a_retry() {
        let t0 = Instant::now();
        let mut tally = TransferTally::default();
        tally.fold_at(&started("a", 4_000_000), t0);
        tally.fold_at(
            &TransferEvent::Progress {
                oid: "a".to_owned(),
                bytes_done: 1_000_000,
            },
            at(t0, 1_000),
        );

        let mut progress = SyncProgress::idle("id", "n");
        tally.apply(&mut progress);
        assert_eq!(progress.bytes_per_second, Some(1_000_000));

        // The retry that `BasicTransfer` drives in place: this object's counter
        // restarts at zero while the tally's high-water mark holds. No new bytes
        // are landing, so after the window rolls there is no rate to report —
        // and at no point is there a zero one, which would read as a claim.
        for ms in [1_500, 4_000, 5_000] {
            tally.fold_at(
                &TransferEvent::Progress {
                    oid: "a".to_owned(),
                    bytes_done: 0,
                },
                at(t0, ms),
            );
            tally.apply(&mut progress);
            assert_ne!(progress.bytes_per_second, Some(0), "at {ms} ms");
        }
        assert_eq!(tally.bytes_done(), 1_000_000, "the mark never walked back");
        assert_eq!(
            progress.bytes_per_second, None,
            "a stalled retry has no rate"
        );
    }

    /// Story 34.8: [`SyncPhase::carries_rate`] is a claim about producers, so it
    /// is checked against them rather than restated.
    ///
    /// Its predecessor `is_transferring` asserted that `Pushing` carries a rate
    /// on the strength of a doc comment — with no callers and no test — so the
    /// phase an ordinary text folder spends its whole push in drew a bar with no
    /// rate while the vocabulary insisted otherwise, and the renderer test that
    /// looked like coverage hand-injected a `(pushing, 4.1 MB/s)` pair the engine
    /// cannot emit.
    ///
    /// The LFS half of the claim is driven here through the real producer.
    /// The fetch half is driven in `engine.rs`'s
    /// `the_fetch_leg_stamps_a_rate_off_the_high_water_mark`, next to the
    /// producer it exercises.
    #[test]
    fn only_the_phases_with_a_byte_producer_claim_a_rate() {
        for phase in [
            SyncPhase::Fetching,
            SyncPhase::UploadingLfs,
            SyncPhase::DownloadingLfs,
        ] {
            assert!(
                phase.carries_rate(),
                "{} has a producer that measures bytes",
                phase.label()
            );
        }
        for phase in [
            SyncPhase::Scanning,
            SyncPhase::Applying,
            SyncPhase::Staging,
            SyncPhase::Committing,
            SyncPhase::Pushing,
            SyncPhase::Verifying,
            SyncPhase::Idle,
        ] {
            assert!(
                !phase.carries_rate(),
                "nothing in this crate measures bytes in {}",
                phase.label()
            );
        }

        // A push is still bytes going up and the tray's arrow still says so.
        // "Which way" and "can we time it" are separate questions; answering
        // the second with the first is what produced the false claim.
        assert_eq!(
            SyncPhase::Pushing.direction(),
            Some(TransferDirection::Up),
            "a push has a direction even though nothing times it"
        );

        // And the producer behind the two phases that do claim one, driven
        // rather than described: 4 MB across a two-second window.
        let t0 = Instant::now();
        let mut tally = TransferTally::default();
        tally.fold_at(&started("a", 4_000_000), t0);
        tally.fold_at(
            &TransferEvent::Progress {
                oid: "a".to_owned(),
                bytes_done: 4_000_000,
            },
            at(t0, 2_000),
        );
        for phase in [SyncPhase::UploadingLfs, SyncPhase::DownloadingLfs] {
            let mut progress = SyncProgress::idle("id", "n");
            progress.phase = phase;
            tally.apply(&mut progress);
            assert_eq!(
                progress.bytes_per_second,
                Some(2_000_000),
                "{} claims a rate, so its producer has to stamp one",
                phase.label()
            );
        }
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
        s.phase = SyncPhase::UploadingLfs;
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

    /// AD-34-10, and the single most misleading string in the app before this.
    ///
    /// `pending` counts journal rows. A folder where five thousand files are
    /// mid-write has none, so the line printed "up to date" over it for as long
    /// as the writing lasted.
    #[test]
    fn a_folder_whose_files_are_still_being_written_is_never_up_to_date() {
        let mut s = status("tgdrive");
        s.state = ProfileState::Watching;
        s.settling = 5_000;
        assert_eq!(
            status_line(&s),
            "tgdrive — 5000 waiting for writes to stop",
            "no journal row exists, and the line must still tell the truth"
        );

        // Both kinds of wait at once: queued work must not hide the held files
        // (nor the reverse — each is a separate fact about the same folder).
        s.pending = 2;
        assert_eq!(
            status_line(&s),
            "tgdrive — 2 waiting to sync, 5000 waiting for writes to stop"
        );

        // Only once BOTH are zero is the claim honest.
        s.settling = 0;
        assert_eq!(status_line(&s), "tgdrive — 2 waiting to sync");
        s.pending = 0;
        assert_eq!(status_line(&s), "tgdrive — up to date");
    }

    /// An in-flight transfer already says what it is doing, and its own line is
    /// the more useful one — the settling count must not displace it.
    #[test]
    fn an_active_phase_still_outranks_the_settling_count() {
        let mut s = status("tgdrive");
        s.state = ProfileState::Syncing;
        s.phase = SyncPhase::Committing;
        s.settling = 7;
        assert!(
            status_line(&s).starts_with(SyncPhase::Committing.label()),
            "got {}",
            status_line(&s)
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
