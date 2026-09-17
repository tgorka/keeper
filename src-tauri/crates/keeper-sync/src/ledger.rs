//! The run ledger's grammar: what a run's **file name** says, and how the next
//! run reads it without opening anything (Story 74.4, AD-252).
//!
//! # Why the name carries the mark
//!
//! A copy task walked its whole source on every pass, because nothing recorded
//! how far the last pass got. `task_runs` kept a one-line count; the per-file
//! record already existed — `keeper_sync::copy`'s report, rendered as plain
//! text — and was written into the destination where nothing ever read it. So
//! the one fact the next run needs was the one fact nobody wrote down.
//!
//! The owner's own design fixes that, and this module is its vocabulary: the
//! run leaves a file whose **name** answers "what changed since?", so the next
//! run lists a directory and stops. No database, no parse of a body, no I/O per
//! candidate run — a `read_dir` and a string compare.
//!
//! # The three rules that give a mark teeth
//!
//! 1. **The mark is the newest source modification time the run actually
//!    covered — never the wall clock.** A file written while the pass was
//!    walking is older than the clock at the end of it, so a clock mark would
//!    place that file behind the line and skip it *forever*. A covered-mtime
//!    mark can only ever be too small, which costs one extra comparison on the
//!    next pass and loses nothing.
//! 2. **A run that was not `Ok` never advances the mark** ([`latest_mark`]
//!    ignores `Partial` and `Failed`). A pass that died halfway has no honest
//!    high-water line: some of what it skipped it never looked at.
//! 3. **A configuration change invalidates every mark before it.** The name
//!    carries a fingerprint of the task's own configuration; a mark whose
//!    fingerprint differs describes a different job — a different source, a
//!    different destination — so it does not apply and the next pass is full.
//!    Silently honouring it is how a re-pointed copy task would quietly never
//!    copy its new source's history.
//!
//! # Firewall
//!
//! The grammar half of this module is pure: no clock, no filesystem, no
//! platform types. Instants arrive as `i64` epoch milliseconds and names
//! arrive as `&str`, so every rule below is provable from a string literal,
//! and the filesystem half further down cannot reach a decision the grammar
//! did not make.
//!
//! It lives in `keeper-sync` and not in `keeper-core` because this crate is
//! deliberately `keeper-core`-free (`Cargo.toml:10`, AD-40): the grammar is
//! read by the engine and by `keeper-syncd`, neither of which may link
//! matrix-sdk. The three trigger words and three verdict words therefore reach
//! the frontend as **strings**, the way `TaskVm.mode` already does — and
//! `keeper_core::tasks`' own comment on that mirror applies here too: a mirror
//! can drift, so the drift is handled (an unknown word is not a mark) rather
//! than assumed away.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::browse;
use crate::error::{Result, SyncError};

/// Every run file starts with this word, so one glob finds the ledger's own
/// files and leaves a person's notes in the same folder alone.
pub const RUN_FILE_PREFIX: &str = "run";

/// Markdown, because the body is for a person reading in five years with no
/// keeper installed — the same reasoning that put the copy log in plain text.
pub const RUN_FILE_SUFFIX: &str = ".md";

/// The separator between the name's fields. A hyphen, and therefore forbidden
/// inside every field: the stamp is digits and `T`/`Z`, the two words are from
/// closed sets, and the fingerprint is hex.
const FIELD_SEPARATOR: char = '-';

/// How many hex characters of the configuration digest the name carries.
///
/// Eight is 32 bits: enough that two configurations colliding is not something
/// a person will meet, and short enough that the name stays readable in a file
/// listing. A collision's cost is bounded anyway — it would make a mark apply
/// that should not have, i.e. one skipped pass, never a corrupted file.
pub const FINGERPRINT_LEN: usize = 8;

/// The byte joining fingerprint parts: ASCII unit separator, which cannot occur
/// in a path, a URL or a schedule expression.
///
/// Without a separator, `("ab", "c")` and `("a", "bc")` would digest
/// identically and a re-pointed task could inherit a mark that was never its
/// own.
const FINGERPRINT_JOIN: u8 = 0x1f;

/// Why a run happened (AD-253).
///
/// The engine knows this at the moment it claims the task and, before this
/// story, threw it away — so a run a person asked for and a run the clock asked
/// for were indistinguishable afterwards. It is in the file name rather than
/// only in the database because the ledger has to survive the loss of
/// `sync.db`: a folder that syncs to another machine carries its own history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTrigger {
    /// Its schedule came due and this host's tick saw it first.
    Scheduled,
    /// A person pressed Run now.
    Requested,
    /// The host's own timer unit woke and ran it (`keeper-syncd`), which is a
    /// different fact from "its schedule came due while the app was open":
    /// AD-141's distinction, and the reason this is not folded into
    /// [`RunTrigger::Scheduled`].
    Timer,
}

impl RunTrigger {
    /// The wire and file-name spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Requested => "requested",
            Self::Timer => "timer",
        }
    }

    /// The inverse of [`RunTrigger::as_str`]; `None` for anything else, because
    /// a name this build cannot read must be skipped rather than guessed at
    /// (NFR-43's rule for an unrecognised stored spelling).
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "scheduled" => Some(Self::Scheduled),
            "requested" => Some(Self::Requested),
            "timer" => Some(Self::Timer),
            _ => None,
        }
    }
}

/// How a run ended, in the one word the next run needs.
///
/// Deliberately coarser than the outcome string a person reads: the only
/// question the *mechanism* asks of a past run is whether its high-water line
/// can be trusted, and that is a yes/no that `Partial` answers with "no".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunVerdict {
    /// Everything the run set out to do, it did. Only this advances the mark.
    Ok,
    /// It ran and some of it failed. The mark is not trustworthy: what it did
    /// not reach, it did not look at.
    Partial,
    /// It could not run, or died. Same reasoning, more so.
    Failed,
}

impl RunVerdict {
    /// The wire and file-name spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }

    /// The inverse of [`RunVerdict::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "ok" => Some(Self::Ok),
            "partial" => Some(Self::Partial),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    /// Whether a run with this verdict may advance the mark. Rule 2 of the
    /// module doc, in one place so no caller re-decides it.
    pub fn advances_mark(self) -> bool {
        matches!(self, Self::Ok)
    }
}

/// One run file's name, fully parsed — which is to say, the whole mark.
///
/// Field order in the rendered name is not cosmetic: the stamp comes first so
/// that a plain lexicographic sort of a directory listing is a sort by mark.
/// That is what lets the reader take the greatest matching name instead of
/// parsing every one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFileName {
    /// The newest source modification time this run covered, epoch ms. For a
    /// kind with no source to walk (a verify, a gc), the run's finish time —
    /// the same question, "up to when is this folder accounted for", asked of a
    /// job whose input is the folder itself.
    pub mark_ms: i64,
    /// Why the run happened.
    pub trigger: RunTrigger,
    /// How it ended.
    pub verdict: RunVerdict,
    /// [`FINGERPRINT_LEN`] lowercase hex of the task's configuration digest.
    pub fingerprint: String,
}

impl RunFileName {
    /// `run-<YYYYMMDDTHHMMSSZ>-<trigger>-<verdict>-<fp8>.md`.
    pub fn render(&self) -> String {
        format!(
            "{RUN_FILE_PREFIX}{FIELD_SEPARATOR}{}{FIELD_SEPARATOR}{}{FIELD_SEPARATOR}{}{FIELD_SEPARATOR}{}{RUN_FILE_SUFFIX}",
            stamp(self.mark_ms),
            self.trigger.as_str(),
            self.verdict.as_str(),
            self.fingerprint,
        )
    }

    /// Read a name back, or `None` for anything that is not this grammar.
    ///
    /// Strict and total on purpose. The ledger folder is a folder in a person's
    /// drive: it will contain their notes, an editor's backup file, a
    /// `.DS_Store`, and one day a run file written by a newer keeper. Every one
    /// of those must read as "not a mark" rather than as a mark with a
    /// plausible-looking wrong value, because a wrong mark silently skips
    /// files.
    pub fn parse(name: &str) -> Option<Self> {
        let body = name.strip_suffix(RUN_FILE_SUFFIX)?;
        let rest = body.strip_prefix(RUN_FILE_PREFIX)?;
        let rest = rest.strip_prefix(FIELD_SEPARATOR)?;
        // Exactly four fields: a fifth would mean a grammar this build does not
        // know, and splitting loosely would let it pass as the one it does.
        let mut parts = rest.split(FIELD_SEPARATOR);
        let stamp_text = parts.next()?;
        let trigger = RunTrigger::parse(parts.next()?)?;
        let verdict = RunVerdict::parse(parts.next()?)?;
        let fingerprint = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        if fingerprint.len() != FINGERPRINT_LEN
            || !fingerprint
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        Some(Self {
            mark_ms: parse_stamp(stamp_text)?,
            trigger,
            verdict,
            fingerprint: fingerprint.to_owned(),
        })
    }
}

/// `YYYYMMDDTHHMMSSZ` — the instant as UTC, fixed width, sortable as text.
///
/// UTC, never local: a ledger travels between machines, and a stamp that means
/// a different instant depending on who reads it is not a mark. Fixed width
/// because a lexicographic sort of the directory listing has to be a
/// chronological sort, which a variable-width field would break.
///
/// The decomposition is the crate's own [`crate::platform::civil_from_unix_ms`]
/// — `keeper-sync` deliberately has no `chrono`, and that port's doc says why
/// one copy of a leap-year calculation is the only acceptable number. It says
/// nothing about zones, so the UTC guarantee is this function's: the `ms` it is
/// handed is an epoch instant, which is what `SyncPlatform::now_ms` and a
/// file's mtime both are.
pub fn stamp(ms: i64) -> String {
    let (year, month, day, hour, minute, second) = crate::platform::civil_from_unix_ms(ms);
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// The inverse of [`stamp`], to the second.
///
/// Milliseconds are deliberately not in the name: a filesystem's mtime
/// resolution varies, and a mark that claims more precision than the clock it
/// came from would put a file on the wrong side of the line. Truncating to the
/// second rounds *down* here — which is the safe direction, per rule 1.
pub fn parse_stamp(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 16 || bytes[8] != b'T' || bytes[15] != b'Z' {
        return None;
    }
    let field = |from: usize, to: usize| -> Option<i64> {
        let slice = text.get(from..to)?;
        if !slice.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        slice.parse::<i64>().ok()
    };
    let (year, month, day) = (field(0, 4)?, field(4, 6)?, field(6, 8)?);
    let (hour, minute, second) = (field(9, 11)?, field(11, 13)?, field(13, 15)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let ms =
        (unix_days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
            * 1_000;
    // The round trip is the real validation: 31 February parses field by field
    // and then names a day in March, so a name that does not render back to
    // itself is not this grammar. Cheaper and stricter than a month-length
    // table, and it also catches a year this arithmetic cannot represent.
    (stamp(ms) == text).then_some(ms)
}

/// Days since 1970-01-01 for a civil date — the inverse of
/// [`crate::platform::civil_from_unix_ms`]'s decomposition, and Howard
/// Hinnant's `days_from_civil` itself.
///
/// Written here rather than beside its inverse because the port is where time
/// *enters* the crate and nothing there needs the direction that turns words
/// back into an instant; this module needs it to read its own file names, and
/// the round-trip test above is what keeps the two halves honest about each
/// other.
fn unix_days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The year directory an instant belongs in.
///
/// Year subfolders exist so that no single directory grows without bound — the
/// same reason the recordings template and the sessions archive use them. The
/// year is the instant's **UTC** year for [`stamp`]'s reason: on 31 December a
/// local-time year would file two machines' runs under different folders and
/// the reader would miss one.
pub fn year_folder(ms: i64) -> String {
    let (year, ..) = crate::platform::civil_from_unix_ms(ms);
    format!("{year:04}")
}

/// A stable short digest of the parts of a task's configuration that decide
/// what a run would do.
///
/// SHA-256, truncated to [`FINGERPRINT_LEN`] hex. Not a security boundary —
/// nothing here defends against a chosen collision, and the cost of one is a
/// single skipped pass. What it must be is *stable across runs and machines*,
/// which is why the parts arrive as strings from the caller rather than being
/// derived from a struct's debug formatting: a field reordering must not
/// invalidate every mark in the ledger.
pub fn config_fingerprint(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            hasher.update([FINGERPRINT_JOIN]);
        }
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(FINGERPRINT_LEN);
    for byte in digest.iter() {
        if out.len() >= FINGERPRINT_LEN {
            break;
        }
        // Two hex characters per byte; `FINGERPRINT_LEN` is even, so this never
        // splits one.
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

/// The greatest mark among these file **names** that a run with this
/// configuration may use as its lower bound.
///
/// Names only — the caller passes a directory listing and opens nothing, which
/// is the property the whole design is for. `None` means "no mark applies": no
/// run yet, none that succeeded, or none that ran this configuration. Every one
/// of those answers the same way, because they all mean the next pass must be
/// full.
pub fn latest_mark(names: &[&str], fingerprint: &str) -> Option<i64> {
    names
        .iter()
        .filter_map(|name| RunFileName::parse(name))
        .filter(|run| run.verdict.advances_mark() && run.fingerprint == fingerprint)
        .map(|run| run.mark_ms)
        .max()
}

/// The per-task configuration file, written beside that task's year folders.
///
/// TOML and not JSON, because it is the same dialect as the folder file a
/// person may already have edited by hand (`.keeper/keeper.toml`), and because
/// it is written to be read: the copy configuration a run used, in the words
/// the CLI uses for the same fields.
pub const TASK_CONFIG_FILENAME: &str = "task.toml";

/// How many year directories back the mark reader looks.
///
/// Two, not one and not all: a run in January marks files modified in
/// December, so the newest year folder alone can miss the greatest mark by a
/// few days. Beyond two, a ledger that has been idle for a year has no mark
/// worth honouring anyway — and an unbounded walk would make the cheap
/// question expensive, which is the property this design exists for.
const YEARS_SCANNED: usize = 2;

/// Where one task's runs live, resolved from a profile's ledger subfolder.
///
/// Held as a type rather than passed as a `PathBuf` so that a caller cannot
/// accidentally hand the *profile* root to a writer: every path this module
/// composes starts here, and the one place it is built is
/// [`TaskLedger::resolve`], which is also the one place `plain_segments`
/// refuses a subfolder (AD-65 — no new path arithmetic anywhere else).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLedger {
    root: PathBuf,
}

impl TaskLedger {
    /// `<profile root>/<ledger subfolder>/<task id>`.
    ///
    /// Every segment of both the subfolder and the task id goes through
    /// [`browse::plain_segments`], so a `..`, a separator, a control character
    /// or an unspellable name is refused here rather than escaping the profile
    /// at write time. A task id is minted by keeper and is a ULID today, but
    /// it arrives here as a `&str` from a database column — which is exactly
    /// the kind of value that must not be trusted to be well-formed.
    pub fn resolve(profile_root: &Path, subfolder: &str, task_id: &str) -> Result<Self> {
        let mut root = profile_root.to_path_buf();
        for segment in browse::plain_segments(subfolder).map_err(|refusal| {
            SyncError::Config(format!("task ledger subfolder is unusable: {refusal}"))
        })? {
            root.push(segment);
        }
        for segment in browse::plain_segments(task_id).map_err(|refusal| {
            SyncError::Config(format!("task id is unusable as a folder name: {refusal}"))
        })? {
            root.push(segment);
        }
        Ok(Self { root })
    }

    /// The folder itself, for a surface that wants to name it to a person.
    pub fn path(&self) -> &Path {
        &self.root
    }

    /// The greatest mark a previous run of **this configuration** left, or
    /// `None` when the next pass must be a full one.
    ///
    /// Reads directory names only. A missing ledger folder, an unreadable one,
    /// a folder with no year directories and a folder whose every run failed
    /// all answer the same way, because they all mean the same thing to the
    /// caller: no lower bound applies. A ledger that cannot be read is
    /// therefore *safe* — it costs a full pass, never a skipped file.
    pub fn latest_mark(&self, fingerprint: &str) -> Option<i64> {
        let mut years: Vec<String> = std::fs::read_dir(&self.root)
            .ok()?
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.len() == 4 && name.bytes().all(|b| b.is_ascii_digit()))
            .collect();
        // Descending, so the two newest are the two scanned.
        years.sort_unstable_by(|a, b| b.cmp(a));
        years.truncate(YEARS_SCANNED);

        let mut best: Option<i64> = None;
        for year in years {
            let Ok(entries) = std::fs::read_dir(self.root.join(&year)) else {
                continue;
            };
            let names: Vec<String> = entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect();
            let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
            if let Some(mark) = latest_mark(&borrowed, fingerprint) {
                best = Some(best.map_or(mark, |current: i64| current.max(mark)));
            }
        }
        best
    }

    /// Write one run down, and return the file it landed in.
    ///
    /// Atomic the way every other file keeper puts in somebody's drive is
    /// atomic: a sibling temp then a rename, so a reader — including keeper's
    /// own mark reader, and including a sync pass walking the folder — never
    /// sees a half-written run, and a crash leaves no torn file behind under a
    /// name that parses.
    pub fn write_run(&self, name: &RunFileName, body: &str) -> Result<PathBuf> {
        let dir = self.root.join(year_folder(name.mark_ms));
        std::fs::create_dir_all(&dir)
            .map_err(|err| SyncError::io("create task ledger folder", &dir, err))?;
        let path = dir.join(name.render());
        // The temp's name deliberately does NOT parse as a run file (no `run-`
        // prefix, no `.md` suffix): if this process dies between the write and
        // the rename, what is left over must read as "not a mark" rather than
        // as a mark with a truncated body.
        let temp = dir.join(format!(".{}.writing", name.render()));
        std::fs::write(&temp, body)
            .map_err(|err| SyncError::io("write task ledger entry", &temp, err))?;
        if let Err(err) = std::fs::rename(&temp, &path) {
            let _ = std::fs::remove_file(&temp);
            return Err(SyncError::io("publish task ledger entry", &path, err));
        }
        Ok(path)
    }

    /// Keep the task's configuration file current, and say whether it changed.
    ///
    /// Written on every run rather than only at save time, for the same reason
    /// the run files exist at all: the folder has to be able to answer what
    /// this task *is* on a machine whose database never held it. Rewritten
    /// only when the content differs, so a folder that syncs does not get a
    /// commit per run for a file nobody edited.
    pub fn write_config(&self, body: &str) -> Result<bool> {
        std::fs::create_dir_all(&self.root)
            .map_err(|err| SyncError::io("create task ledger folder", &self.root, err))?;
        let path = self.root.join(TASK_CONFIG_FILENAME);
        if std::fs::read_to_string(&path).is_ok_and(|existing| existing == body) {
            return Ok(false);
        }
        let temp = self.root.join(format!(".{TASK_CONFIG_FILENAME}.writing"));
        std::fs::write(&temp, body)
            .map_err(|err| SyncError::io("write task configuration", &temp, err))?;
        if let Err(err) = std::fs::rename(&temp, &path) {
            let _ = std::fs::remove_file(&temp);
            return Err(SyncError::io("publish task configuration", &path, err));
        }
        Ok(true)
    }
}

/// The header a run file carries above the copy report's own lines.
///
/// Five facts, one per line, in the order somebody scanning a folder wants
/// them: what kind of run, why it happened, how it ended, what it marks, and
/// which configuration it ran. Every one of them is also in the file's name —
/// the body is where a person reads them, the name is where keeper does.
pub fn render_run_header(
    kind: &str,
    trigger: RunTrigger,
    verdict: RunVerdict,
    name: &RunFileName,
    detail: &str,
) -> String {
    format!(
        "keeper {kind} run\nwhy:         {}\nverdict:     {}\nmark:        {}\nconfig:      {}\ndetail:      {detail}\n\n",
        trigger.as_str(),
        verdict.as_str(),
        stamp(name.mark_ms),
        name.fingerprint,
    )
}

/// Whether a file name is one of this ledger's own run files.
///
/// Used by a surface listing the folder, so a person's own notes sitting beside
/// the runs are not rendered as runs. The prefix and suffix test is the cheap
/// half; `RunFileName::parse` is the honest one, and this is deliberately the
/// honest one.
pub fn is_run_file(name: &str) -> bool {
    name.starts_with(RUN_FILE_PREFIX) && name.ends_with(RUN_FILE_SUFFIX) && {
        RunFileName::parse(name).is_some()
    }
}

#[cfg(test)]
mod fs_tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "keeper-ledger-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("temp root");
        dir
    }

    fn run(mark_ms: i64, trigger: RunTrigger, verdict: RunVerdict, fp: &str) -> RunFileName {
        RunFileName {
            mark_ms,
            trigger,
            verdict,
            fingerprint: fp.to_owned(),
        }
    }

    #[test]
    fn a_subfolder_that_would_escape_the_profile_is_refused_not_normalised() {
        let root = temp_root("escape");
        for subfolder in ["../outside", "/absolute", "tasks/../..", ""] {
            let resolved = TaskLedger::resolve(&root, subfolder, "01TASK");
            if subfolder.is_empty() {
                // An empty subfolder resolves to the profile root itself, which
                // `plain_segments` allows; the profile's own validator is what
                // refuses it before a ledger is ever built (Story 74.3), and
                // this test records that division so neither side assumes the
                // other did it.
                assert!(resolved.is_ok());
                continue;
            }
            assert!(
                resolved.is_err(),
                "{subfolder} must be refused rather than cleaned up"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_mark_comes_from_names_across_the_two_newest_year_folders() {
        let root = temp_root("mark");
        let ledger = TaskLedger::resolve(&root, "tasks", "01TASK").expect("resolve");
        let fp = "3f2a91c4";
        // December's successful run, and January's failure: the newest year
        // folder alone would answer `None` and the next pass would re-walk the
        // whole tree for nothing.
        ledger
            .write_run(
                &run(1_767_225_599_000, RunTrigger::Scheduled, RunVerdict::Ok, fp),
                "december",
            )
            .expect("write december");
        ledger
            .write_run(
                &run(
                    1_767_225_600_000,
                    RunTrigger::Requested,
                    RunVerdict::Failed,
                    fp,
                ),
                "january",
            )
            .expect("write january");
        assert!(root.join("tasks/01TASK/2025").is_dir());
        assert!(root.join("tasks/01TASK/2026").is_dir());

        assert_eq!(
            ledger.latest_mark(fp),
            Some(1_767_225_599_000),
            "the greatest mark of a run that finished, wherever its year folder is"
        );
        assert_eq!(
            ledger.latest_mark("bbbbbbbb"),
            None,
            "a different configuration inherits nothing"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_or_unreadable_ledger_costs_a_full_pass_and_nothing_else() {
        let root = temp_root("absent");
        let ledger = TaskLedger::resolve(&root, "tasks", "01NOTHINGHERE").expect("resolve");
        assert_eq!(
            ledger.latest_mark("3f2a91c4"),
            None,
            "no folder, no mark, full pass — never a skipped file"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_published_run_leaves_no_temp_and_a_stray_temp_is_not_a_mark() {
        let root = temp_root("atomic");
        let ledger = TaskLedger::resolve(&root, "tasks", "01TASK").expect("resolve");
        let name = run(
            1_789_511_093_000,
            RunTrigger::Timer,
            RunVerdict::Ok,
            "0123abcd",
        );
        let path = ledger.write_run(&name, "body").expect("write");
        let dir = path.parent().expect("year folder");
        let names: Vec<String> = std::fs::read_dir(dir)
            .expect("listing")
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        assert_eq!(names, vec![name.render()], "no .writing residue survives");

        // What a crash between write and rename leaves behind must read as "not
        // a mark", or a truncated body would be honoured as a high-water line.
        let stray = dir.join(format!(".{}.writing", name.render()));
        std::fs::write(&stray, "torn").expect("stray");
        assert!(!is_run_file(&format!(".{}.writing", name.render())));
        assert_eq!(
            ledger.latest_mark("0123abcd"),
            Some(1_789_511_093_000),
            "the published run is still the only mark"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_configuration_file_is_rewritten_only_when_it_changed() {
        let root = temp_root("config");
        let ledger = TaskLedger::resolve(&root, "tasks", "01TASK").expect("resolve");
        assert!(ledger.write_config("kind = \"copy\"\n").expect("first"));
        assert!(
            !ledger.write_config("kind = \"copy\"\n").expect("second"),
            "an unchanged configuration must not produce a commit per run in a synced folder"
        );
        assert!(ledger.write_config("kind = \"sync\"\n").expect("third"));
        assert_eq!(
            std::fs::read_to_string(root.join("tasks/01TASK").join(TASK_CONFIG_FILENAME))
                .expect("read back"),
            "kind = \"sync\"\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_persons_own_file_in_the_ledger_folder_is_not_a_run() {
        for name in ["notes.md", "run-notes.md", ".DS_Store", "task.toml"] {
            assert!(!is_run_file(name), "{name} is not one of ours");
        }
        assert!(is_run_file("run-20260915T222453Z-requested-ok-3f2a91c4.md"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(mark_ms: i64, trigger: RunTrigger, verdict: RunVerdict, fp: &str) -> String {
        RunFileName {
            mark_ms,
            trigger,
            verdict,
            fingerprint: fp.to_owned(),
        }
        .render()
    }

    #[test]
    fn a_rendered_name_reads_back_as_the_run_it_described() {
        let original = RunFileName {
            mark_ms: 1_789_511_093_000,
            trigger: RunTrigger::Requested,
            verdict: RunVerdict::Ok,
            fingerprint: "3f2a91c4".to_owned(),
        };
        let rendered = original.render();
        assert_eq!(rendered, "run-20260915T222453Z-requested-ok-3f2a91c4.md");
        let parsed = RunFileName::parse(&rendered).expect("the grammar reads its own output");
        // Milliseconds are not in the name (see `parse_stamp`), so the mark
        // comes back truncated to the second — and downward, which is the safe
        // direction: a file modified in that second is looked at once more.
        assert_eq!(parsed.mark_ms, 1_789_511_093_000);
        assert_eq!(parsed.trigger, RunTrigger::Requested);
        assert_eq!(parsed.verdict, RunVerdict::Ok);
        assert_eq!(parsed.fingerprint, "3f2a91c4");
    }

    #[test]
    fn a_name_that_is_not_the_grammar_is_not_a_mark() {
        for candidate in [
            // A person's own file in their own folder.
            "notes.md",
            // The right shape, an unknown verdict — a newer keeper's word.
            "run-20260915T222453Z-requested-cancelled-3f2a91c4.md",
            // An unknown trigger.
            "run-20260915T222453Z-cron-ok-3f2a91c4.md",
            // A fifth field: a grammar this build does not know.
            "run-20260915T222453Z-requested-ok-3f2a91c4-extra.md",
            // A fingerprint that is not eight hex characters.
            "run-20260915T222453Z-requested-ok-3f2a91.md",
            "run-20260915T222453Z-requested-ok-3f2a91cg.md",
            // A stamp that is not a stamp.
            "run-yesterday-requested-ok-3f2a91c4.md",
            // An editor's backup of a real run file.
            "run-20260915T222453Z-requested-ok-3f2a91c4.md~",
            // The prefix without its separator.
            "runner-20260915T222453Z-requested-ok-3f2a91c4.md",
        ] {
            assert!(
                RunFileName::parse(candidate).is_none(),
                "{candidate} must not read as a mark"
            );
        }
    }

    #[test]
    fn the_greatest_successful_mark_wins_and_a_failure_never_counts() {
        let fp = "3f2a91c4";
        let names = [
            name(1_000_000_000_000, RunTrigger::Scheduled, RunVerdict::Ok, fp),
            // Later, but it died: what it did not reach, it did not look at.
            name(
                2_000_000_000_000,
                RunTrigger::Scheduled,
                RunVerdict::Failed,
                fp,
            ),
            // Later still, and only half of it worked.
            name(
                3_000_000_000_000,
                RunTrigger::Requested,
                RunVerdict::Partial,
                fp,
            ),
            name(1_500_000_000_000, RunTrigger::Timer, RunVerdict::Ok, fp),
        ];
        let listing: Vec<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(
            latest_mark(&listing, fp),
            // 1_500_000_000_000 to the second, which is the value itself here.
            Some(1_500_000_000_000),
            "only a run that finished may advance the line"
        );
    }

    #[test]
    fn a_mark_from_a_different_configuration_does_not_apply() {
        let names = [name(
            2_000_000_000_000,
            RunTrigger::Scheduled,
            RunVerdict::Ok,
            "aaaaaaaa",
        )];
        let listing: Vec<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(
            latest_mark(&listing, "bbbbbbbb"),
            None,
            "a re-pointed task must walk its new source, not inherit the old one's line"
        );
        assert_eq!(latest_mark(&listing, "aaaaaaaa"), Some(2_000_000_000_000));
        assert_eq!(
            latest_mark(&[], "aaaaaaaa"),
            None,
            "no runs yet answers the same way: the next pass is full"
        );
    }

    #[test]
    fn a_stamp_means_the_same_instant_on_every_machine() {
        // A leap day, and the instant one second before a UTC year boundary.
        for ms in [
            1_709_164_800_000_i64, // 2024-02-29T00:00:00Z
            1_767_225_599_000,     // 2025-12-31T23:59:59Z
            0,
            -86_400_000, // 1969-12-31T00:00:00Z — before the epoch
        ] {
            let text = stamp(ms);
            assert_eq!(
                parse_stamp(&text),
                Some(ms),
                "{text} must read back as the instant it was written from"
            );
        }
        assert_eq!(stamp(1_767_225_599_000), "20251231T235959Z");
    }

    #[test]
    fn the_year_folder_is_the_utc_year_so_two_machines_agree() {
        // 23:59:59 UTC on 31 December is already the next year in Warsaw and
        // still the old one in Los Angeles. Filing by local time would put two
        // clones' runs in different folders and the reader would miss one.
        assert_eq!(year_folder(1_767_225_599_000), "2025");
        assert_eq!(year_folder(1_767_225_600_000), "2026");
    }

    #[test]
    fn a_fingerprint_is_stable_and_moves_when_the_job_does() {
        let a = config_fingerprint(&["copy", "/src", "/dst", "replace=false"]);
        assert_eq!(
            a,
            config_fingerprint(&["copy", "/src", "/dst", "replace=false"]),
            "the same configuration must fingerprint identically on every run"
        );
        assert_eq!(a.len(), FINGERPRINT_LEN);
        assert!(
            a.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "the name's field is lowercase hex: {a}"
        );
        assert_ne!(
            a,
            config_fingerprint(&["copy", "/src", "/other", "replace=false"]),
            "a new destination is a different job"
        );
        assert_ne!(
            config_fingerprint(&["ab", "c"]),
            config_fingerprint(&["a", "bc"]),
            "parts are separated, or two configurations would share a mark"
        );
    }

    #[test]
    fn a_name_is_one_path_segment_no_filesystem_refuses() {
        // The writer composes this through `keeper_sync::browse::plain_segments`
        // (AD-65), which refuses separators, `..`, control characters and the
        // characters Windows forbids. A grammar that could produce one would
        // fail at the write, in the field, on somebody's drive.
        let forbidden = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
        for (ms, trigger, verdict) in [
            (0_i64, RunTrigger::Scheduled, RunVerdict::Ok),
            (
                1_789_511_093_999,
                RunTrigger::Requested,
                RunVerdict::Partial,
            ),
            (-1, RunTrigger::Timer, RunVerdict::Failed),
        ] {
            let rendered = name(ms, trigger, verdict, "0123abcd");
            assert!(
                !rendered
                    .chars()
                    .any(|c| forbidden.contains(&c) || c.is_control()),
                "{rendered} must be a plain path segment"
            );
            assert_ne!(rendered, "..");
            assert!(RunFileName::parse(&rendered).is_some());
        }
    }
}
