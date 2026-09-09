//! Convergence without prompts (Story 24.7, AD-43).
//!
//! The product promise (FR-89) is that synchronization **never blocks on a
//! human decision to converge**. That rules out a modal, a queue of "resolve
//! these 400 conflicts" cards, and — for v1 — a three-way content merge, which
//! would silently write merge markers into a user's binary files.
//!
//! What is left is the Syncthing resolution shape: **the remote revision wins
//! the canonical path, and the local revision survives verbatim as a conflict
//! copy** committed as an ordinary tracked file, so every peer converges on the
//! same visible pair and either revision can be recovered with a rename.
//!
//! The one asymmetry worth stating out loud: **a modification always beats a
//! deletion.** A remote delete never removes a locally modified file and a
//! local delete never removes a remotely modified one, because a deletion
//! carries no content — losing it costs a rename, while losing an edit costs
//! the edit.
//!
//! Everything here is a pure function over two [`ChangeKind`]s. No filesystem,
//! no clock, no device lookup: the timestamp and the device label are passed
//! in, which is what makes the whole policy exhaustively testable.
//!
//! # Who calls it (Story 70.3, AD-229)
//!
//! For two epics the matrix had no production caller: the engine wrote a copy
//! for every path both sides had touched and let `-X theirs` decide the rest,
//! and `-X theirs` cannot decide modify/delete — measured, it exits 1 and
//! leaves `MERGE_HEAD`, after which every later merge exits 128. Now
//! `converge_with_conflict_copies` asks [`resolve`] twice: before the merge,
//! with each side's [`ChangeKind`] read from `git diff --name-status` against
//! the merge base, to decide which paths get a copy; and after it, with the
//! kinds read from the unmerged index stages ([`ChangeKind::from_stages`]),
//! to decide which side `git checkout --ours|--theirs` restores. One
//! function, two sources, so AD-43 is stated once.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Which revision of a diverged path is being referred to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The revision in this device's working tree.
    Local,
    /// The revision fetched from the remote.
    Remote,
}

/// What happened to one path on one side since the last common state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// The path did not exist before and does now.
    Added,
    /// The path existed before and its content or mode changed.
    Modified,
    /// The path existed before and is now gone.
    Deleted,
    /// This side did not touch the path.
    Unchanged,
}

impl ChangeKind {
    /// Whether this side holds bytes that exist nowhere else.
    ///
    /// This is the predicate the whole policy turns on: content can be lost,
    /// a deletion cannot.
    pub fn carries_content(self) -> bool {
        matches!(self, Self::Added | Self::Modified)
    }

    /// The kind one `git diff --name-status` letter stands for.
    ///
    /// `A` and `D` are the two the policy distinguishes; everything else —
    /// `M`, a type change `T`, a mode change — is content that exists on that
    /// side, which is all [`carries_content`](Self::carries_content) asks. The
    /// diff is run with `--no-renames`, so `R` and `C` never arrive: a rename
    /// is a `D` and an `A`, which is also how the merge sees it.
    pub fn from_status(letter: u8) -> Self {
        match letter {
            b'A' => Self::Added,
            b'D' => Self::Deleted,
            _ => Self::Modified,
        }
    }

    /// The kind an unmerged index entry's stages say one side did.
    ///
    /// Stage 1 is the merge base, stage 2 ours, stage 3 theirs; `in_base` is
    /// whether stage 1 exists and `on_side` whether this side's stage does.
    /// A side with no stage where the base had one deleted the path; a side
    /// with a stage where the base had none added it.
    pub fn from_stages(in_base: bool, on_side: bool) -> Self {
        match (in_base, on_side) {
            (false, true) => Self::Added,
            (true, true) => Self::Modified,
            (true, false) => Self::Deleted,
            (false, false) => Self::Unchanged,
        }
    }
}

/// What the engine does with one diverged path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Write the remote revision at the canonical path; the local side had
    /// nothing that would be lost.
    TakeRemote,
    /// Keep the local revision at the canonical path and propagate it; the
    /// remote side had nothing that would be lost.
    KeepLocal,
    /// Both sides changed. The remote revision takes the canonical path and the
    /// local revision is preserved beside it under `copy_name`.
    ConflictCopy {
        /// File name only, to be joined with the path's own parent directory.
        /// An `OsString` because the name is built from the path's own bytes
        /// and a non-UTF-8 file deserves a copy that is *its* copy.
        copy_name: OsString,
    },
    /// Nothing to do: the two sides already agree.
    Nothing,
}

impl Resolution {
    /// Which side ends up at the canonical path, if either.
    ///
    /// Encodes AD-43's tie-break in one place: whenever both sides changed, the
    /// remote wins the name and the local revision moves aside.
    pub fn canonical(&self) -> Option<Side> {
        match self {
            Self::TakeRemote | Self::ConflictCopy { .. } => Some(Side::Remote),
            Self::KeepLocal => Some(Side::Local),
            Self::Nothing => None,
        }
    }
}

/// Decide what happens to `path` given how each side changed it.
///
/// `now_utc` is a pre-formatted `yyyymmdd-hhmmss` UTC stamp and `device` is this
/// machine's label; both are only consumed when a conflict copy is required.
pub fn resolve(
    local: ChangeKind,
    remote: ChangeKind,
    path: &Path,
    now_utc: &str,
    device: &str,
) -> Resolution {
    use ChangeKind::{Added, Deleted, Modified, Unchanged};

    match (local, remote) {
        // Nobody moved, or both moved the same way (a delete is a delete).
        (Unchanged, Unchanged) | (Deleted, Deleted) => Resolution::Nothing,

        // Only the remote moved. Its delete applies only because the file is
        // untouched here — that is the whole condition AD-43 puts on honouring
        // a remote deletion.
        (Unchanged, Added | Modified | Deleted) => Resolution::TakeRemote,

        // Only we moved.
        (Added | Modified | Deleted, Unchanged) => Resolution::KeepLocal,

        // A modification beats a deletion, in both directions: the side holding
        // bytes wins the path and the delete is simply not applied.
        (Added | Modified, Deleted) => Resolution::KeepLocal,
        (Deleted, Added | Modified) => Resolution::TakeRemote,

        // Both sides hold bytes. Neither may be discarded, so the remote takes
        // the canonical name and ours moves aside under a conflict name.
        (Added | Modified, Added | Modified) => Resolution::ConflictCopy {
            copy_name: conflict_name(path, now_utc, device),
        },
    }
}

/// Longest device label admitted into a conflict file name.
///
/// Filesystems cap a component at ~255 bytes; the stem, the marker and the
/// extension all have to fit beside the label, so it gets a hard budget.
const DEVICE_LABEL_CAP: usize = 32;
/// Longest timestamp admitted. A well-formed `yyyymmdd-hhmmss` is 15.
const TIMESTAMP_CAP: usize = 24;

/// Build `<stem>.sync-conflict-<yyyymmdd-hhmmss>-<device>.<ext>`.
///
/// Returns a **file name**, not a path: the caller joins it with the conflicted
/// path's own parent so the copy lands beside the original, which is where a
/// user will look for it.
///
/// The extension is preserved so the copy still opens in the same application,
/// and only the final `.` counts as the separator — `a.tar.gz` keeps `.gz`, and
/// a dotfile like `.bashrc` has no extension at all rather than an extension of
/// `bashrc`.
///
/// Built from the name's **bytes**, not from `to_string_lossy`: that rendering
/// maps `a\xFF.txt` and `a\xFE.txt` onto one string, so two distinct files
/// would have collapsed onto one copy name and the second would have overwritten
/// the first. `names.rs` states the rule — a lossy rendering is fine to *show*
/// and must never be used to *reach* — and a copy name is used to reach.
pub fn conflict_name(path: &Path, now_utc: &str, device: &str) -> OsString {
    build_name(path, &marker(now_utc, device))
}

/// [`conflict_name`] with an ordinal, for the second and later copies of one
/// path within one stamp: `<stem>.sync-conflict-<stamp>-<device>-<n>.<ext>`.
///
/// Two passes within one second contest the same path when the first pass'
/// merge was undone and retried, and `fs::copy` onto the first copy would have
/// truncated it. The engine opens every copy with `create_new` and counts up
/// from 2 on collision; the first copy keeps the plain name so the common case
/// reads as it always has.
pub fn conflict_name_numbered(path: &Path, now_utc: &str, device: &str, ordinal: u32) -> OsString {
    build_name(path, &format!("{}-{ordinal}", marker(now_utc, device)))
}

fn marker(now_utc: &str, device: &str) -> String {
    format!(
        "sync-conflict-{}-{}",
        sanitize_component(now_utc, TIMESTAMP_CAP),
        sanitize_component(device, DEVICE_LABEL_CAP)
    )
}

fn build_name(path: &Path, marker: &str) -> OsString {
    let Some(name) = path.file_name() else {
        // A path with no final component (`/`, `..`) is not a file we could be
        // conflicting over, but returning a usable name beats panicking.
        return OsString::from(marker);
    };
    let name = name_bytes(name);
    let mut out = Vec::with_capacity(name.len() + marker.len() + 2);
    match name.iter().rposition(|byte| *byte == b'.') {
        // `idx > 0` keeps a leading dot from being read as a separator;
        // `idx + 1 < len` keeps a trailing dot from producing an empty suffix.
        Some(idx) if idx > 0 && idx + 1 < name.len() => {
            out.extend_from_slice(&name[..idx]);
            out.push(b'.');
            out.extend_from_slice(marker.as_bytes());
            out.push(b'.');
            out.extend_from_slice(&name[idx + 1..]);
        }
        _ => {
            out.extend_from_slice(&name);
            out.push(b'.');
            out.extend_from_slice(marker.as_bytes());
        }
    }
    name_from_bytes(out)
}

/// The canonical path behind one of git's rescue names, if `path` is one.
///
/// When a merge finds a directory where the other side has a file (or two
/// different object types at one path), git cannot put both at the name and
/// writes the file as `<path>~<label>`, then `<path>~<label>_0`, `_1`, … if
/// that is taken — `label` being `HEAD` for ours and the merged ref with `/`
/// as `_` for theirs. Measured on git 2.53: the unmerged index entry is
/// recorded *at the rescue name*, not at the canonical one, so this is how
/// the engine finds out which path the conflict is really about and turns
/// the rescue into an ordinary conflict copy instead of leaving litter.
pub fn rescue_origin(path: &Path, label: &str) -> Option<PathBuf> {
    let name = name_bytes(path.file_name()?);
    let tilde = name.iter().rposition(|byte| *byte == b'~')?;
    let suffix = &name[tilde + 1..];
    let rest = suffix.strip_prefix(label.as_bytes())?;
    let ordinal_ok = rest.is_empty()
        || (rest.len() > 1 && rest[0] == b'_' && rest[1..].iter().all(u8::is_ascii_digit));
    if tilde == 0 || !ordinal_ok {
        return None;
    }
    let stem = name_from_bytes(name[..tilde].to_vec());
    Some(match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(stem),
        _ => PathBuf::from(stem),
    })
}

/// The label git uses in a rescue name for the side merged in.
pub fn rescue_label(reference: &str) -> String {
    reference.replace('/', "_")
}

#[cfg(unix)]
fn name_bytes(name: &OsStr) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt as _;
    std::borrow::Cow::Borrowed(name.as_bytes())
}

/// Off unix an `OsStr` is not bytes; the rendering is lossy, and the platform
/// that produced every non-UTF-8 name this crate has met is unix.
#[cfg(not(unix))]
fn name_bytes(name: &OsStr) -> std::borrow::Cow<'_, [u8]> {
    std::borrow::Cow::Owned(name.to_string_lossy().into_owned().into_bytes())
}

#[cfg(unix)]
fn name_from_bytes(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt as _;
    OsString::from_vec(bytes)
}

#[cfg(not(unix))]
fn name_from_bytes(bytes: Vec<u8>) -> OsString {
    OsString::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reduce a label to characters that are safe in a file name on every platform
/// we target, capped at `cap` characters.
///
/// A device label is user-editable text: a `/` in it would turn the conflict
/// copy into a path, and a `:` or `\` would make it unrepresentable on Windows.
fn sanitize_component(value: &str, cap: usize) -> String {
    let cleaned: String = value
        .chars()
        .take(cap)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    // An all-illegal label would otherwise collapse the marker into `--`.
    if cleaned.chars().all(|c| c == '-') {
        "device".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: &str = "20260725-120000";
    const DEV: &str = "laptop";

    fn resolve_at(local: ChangeKind, remote: ChangeKind) -> Resolution {
        resolve(local, remote, Path::new("dir/a.txt"), TS, DEV)
    }

    const ALL: [ChangeKind; 4] = [
        ChangeKind::Added,
        ChangeKind::Modified,
        ChangeKind::Deleted,
        ChangeKind::Unchanged,
    ];

    #[test]
    fn the_full_change_matrix_resolves_as_ad_43_specifies() {
        use ChangeKind::{Added, Deleted, Modified, Unchanged};
        let expected: [(ChangeKind, ChangeKind, Resolution); 16] = [
            (Unchanged, Unchanged, Resolution::Nothing),
            (Unchanged, Added, Resolution::TakeRemote),
            (Unchanged, Modified, Resolution::TakeRemote),
            (Unchanged, Deleted, Resolution::TakeRemote),
            (Added, Unchanged, Resolution::KeepLocal),
            (Modified, Unchanged, Resolution::KeepLocal),
            (Deleted, Unchanged, Resolution::KeepLocal),
            (Deleted, Deleted, Resolution::Nothing),
            // Modification beats deletion, both ways.
            (Added, Deleted, Resolution::KeepLocal),
            (Modified, Deleted, Resolution::KeepLocal),
            (Deleted, Added, Resolution::TakeRemote),
            (Deleted, Modified, Resolution::TakeRemote),
            // Both sides hold bytes.
            (
                Added,
                Added,
                Resolution::ConflictCopy {
                    copy_name: conflict_name(Path::new("dir/a.txt"), TS, DEV),
                },
            ),
            (
                Added,
                Modified,
                Resolution::ConflictCopy {
                    copy_name: conflict_name(Path::new("dir/a.txt"), TS, DEV),
                },
            ),
            (
                Modified,
                Added,
                Resolution::ConflictCopy {
                    copy_name: conflict_name(Path::new("dir/a.txt"), TS, DEV),
                },
            ),
            (
                Modified,
                Modified,
                Resolution::ConflictCopy {
                    copy_name: conflict_name(Path::new("dir/a.txt"), TS, DEV),
                },
            ),
        ];

        for (local, remote, want) in expected {
            assert_eq!(
                resolve_at(local, remote),
                want,
                "local {local:?} x remote {remote:?}"
            );
        }
    }

    #[test]
    fn no_pair_can_discard_content_that_exists_on_only_one_side() {
        for local in ALL {
            for remote in ALL {
                let got = resolve_at(local, remote);
                match (local.carries_content(), remote.carries_content()) {
                    // Both sides hold bytes: the only safe answer keeps both.
                    (true, true) => {
                        let Resolution::ConflictCopy { copy_name } = &got else {
                            panic!("{local:?} x {remote:?} discarded a revision: {got:?}");
                        };
                        assert!(!copy_name.is_empty());
                        assert_ne!(copy_name.as_os_str(), "a.txt", "the copy must not collide");
                    }
                    // Exactly one side holds bytes: that side must win the path.
                    (true, false) => assert_eq!(
                        got.canonical(),
                        Some(Side::Local),
                        "{local:?} x {remote:?} dropped the only surviving content"
                    ),
                    (false, true) => assert_eq!(
                        got.canonical(),
                        Some(Side::Remote),
                        "{local:?} x {remote:?} dropped the only surviving content"
                    ),
                    // Neither side holds bytes; nothing can be lost.
                    (false, false) => {}
                }
            }
        }
    }

    #[test]
    fn a_conflict_copy_always_leaves_the_canonical_path_to_the_remote() {
        assert_eq!(
            resolve_at(ChangeKind::Modified, ChangeKind::Modified).canonical(),
            Some(Side::Remote)
        );
        assert_eq!(Resolution::Nothing.canonical(), None);
    }

    /// The name as text, for assertions: every input here is UTF-8, so the
    /// rendering is exact.
    fn text(name: OsString) -> String {
        name.into_string()
            .expect("a UTF-8 input yields a UTF-8 name")
    }

    #[test]
    fn conflict_name_preserves_the_extension() {
        assert_eq!(
            text(conflict_name(Path::new("dir/a.txt"), TS, DEV)),
            "a.sync-conflict-20260725-120000-laptop.txt"
        );
    }

    #[test]
    fn conflict_name_splits_a_multi_dot_name_at_the_last_dot() {
        assert_eq!(
            text(conflict_name(Path::new("a.tar.gz"), TS, DEV)),
            "a.tar.sync-conflict-20260725-120000-laptop.gz"
        );
    }

    #[test]
    fn conflict_name_treats_a_dotfile_as_having_no_extension() {
        assert_eq!(
            text(conflict_name(Path::new(".bashrc"), TS, DEV)),
            ".bashrc.sync-conflict-20260725-120000-laptop"
        );
    }

    #[test]
    fn conflict_name_handles_a_name_without_an_extension() {
        assert_eq!(
            text(conflict_name(Path::new("noext"), TS, DEV)),
            "noext.sync-conflict-20260725-120000-laptop"
        );
    }

    #[test]
    fn conflict_name_never_lets_a_device_label_become_a_path() {
        let name = text(conflict_name(Path::new("a.txt"), TS, "work/box"));
        assert_eq!(name, "a.sync-conflict-20260725-120000-work-box.txt");
        assert!(!name.contains('/'), "a separator would relocate the copy");

        let spaced = text(conflict_name(Path::new("a.txt"), TS, "my box"));
        assert_eq!(spaced, "a.sync-conflict-20260725-120000-my-box.txt");
    }

    #[test]
    fn conflict_name_caps_a_hostile_device_label() {
        let long = "x".repeat(200);
        let name = text(conflict_name(Path::new("a.txt"), TS, &long));
        assert!(
            name.len() < 100,
            "a 200-char label must not blow the filename budget: {name}"
        );
    }

    #[test]
    fn conflict_name_survives_a_path_with_no_file_name() {
        let name = text(conflict_name(Path::new("/"), TS, DEV));
        assert_eq!(name, "sync-conflict-20260725-120000-laptop");
    }

    #[test]
    fn a_numbered_copy_keeps_the_ordinal_inside_the_marker() {
        assert_eq!(
            text(conflict_name_numbered(Path::new("a.txt"), TS, DEV, 2)),
            "a.sync-conflict-20260725-120000-laptop-2.txt"
        );
        assert_eq!(
            text(conflict_name_numbered(Path::new(".bashrc"), TS, DEV, 3)),
            ".bashrc.sync-conflict-20260725-120000-laptop-3"
        );
    }

    #[cfg(unix)]
    #[test]
    fn two_names_that_render_alike_get_two_different_copy_names() {
        // The bug `names.rs` documents: `to_string_lossy` maps both onto one
        // string, and one copy name means one of the two files is lost.
        use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
        let ff = PathBuf::from(OsString::from_vec(b"a\xFF.txt".to_vec()));
        let fe = PathBuf::from(OsString::from_vec(b"a\xFE.txt".to_vec()));
        let ff_copy = conflict_name(&ff, TS, DEV);
        let fe_copy = conflict_name(&fe, TS, DEV);
        assert_ne!(ff_copy, fe_copy);
        assert_eq!(
            ff_copy.as_bytes(),
            b"a\xFF.sync-conflict-20260725-120000-laptop.txt"
        );
    }

    #[test]
    fn a_rescue_name_resolves_to_its_canonical_path_and_nothing_else_does() {
        let label = rescue_label("refs/remotes/origin/main");
        assert_eq!(label, "refs_remotes_origin_main");
        assert_eq!(
            rescue_origin(Path::new("dir/d~HEAD"), "HEAD"),
            Some(PathBuf::from("dir/d"))
        );
        assert_eq!(
            rescue_origin(Path::new("d~HEAD_0"), "HEAD"),
            Some(PathBuf::from("d"))
        );
        assert_eq!(
            rescue_origin(Path::new("d~refs_remotes_origin_main_12"), &label),
            Some(PathBuf::from("d"))
        );
        // The other side's label is not this side's rescue.
        assert_eq!(rescue_origin(Path::new("d~HEAD"), &label), None);
        // A `~` a user typed, a suffix that is not an ordinal, an empty stem.
        assert_eq!(rescue_origin(Path::new("backup~HEADS"), "HEAD"), None);
        assert_eq!(rescue_origin(Path::new("d~HEAD_x"), "HEAD"), None);
        assert_eq!(rescue_origin(Path::new("~HEAD"), "HEAD"), None);
        assert_eq!(rescue_origin(Path::new("plain.txt"), "HEAD"), None);
    }

    #[test]
    fn stages_and_status_letters_map_onto_the_four_kinds() {
        assert_eq!(ChangeKind::from_status(b'A'), ChangeKind::Added);
        assert_eq!(ChangeKind::from_status(b'D'), ChangeKind::Deleted);
        assert_eq!(ChangeKind::from_status(b'M'), ChangeKind::Modified);
        assert_eq!(ChangeKind::from_status(b'T'), ChangeKind::Modified);
        assert_eq!(ChangeKind::from_stages(false, true), ChangeKind::Added);
        assert_eq!(ChangeKind::from_stages(true, true), ChangeKind::Modified);
        assert_eq!(ChangeKind::from_stages(true, false), ChangeKind::Deleted);
        assert_eq!(ChangeKind::from_stages(false, false), ChangeKind::Unchanged);
    }
}
