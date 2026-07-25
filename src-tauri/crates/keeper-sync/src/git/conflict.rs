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

use std::path::Path;

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
        copy_name: String,
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
pub fn conflict_name(path: &Path, now_utc: &str, device: &str) -> String {
    let marker = format!(
        "sync-conflict-{}-{}",
        sanitize_component(now_utc, TIMESTAMP_CAP),
        sanitize_component(device, DEVICE_LABEL_CAP)
    );

    let Some(name) = path.file_name() else {
        // A path with no final component (`/`, `..`) is not a file we could be
        // conflicting over, but returning a usable name beats panicking.
        return marker;
    };
    // Lossy on purpose: a non-UTF-8 name still deserves a conflict copy, and a
    // mangled-but-present stem is far more recoverable than dropping it.
    let name = name.to_string_lossy();

    match name.rfind('.') {
        // `idx > 0` keeps a leading dot from being read as a separator;
        // `idx + 1 < len` keeps a trailing dot from producing an empty suffix.
        Some(idx) if idx > 0 && idx + 1 < name.len() => {
            format!("{}.{marker}.{}", &name[..idx], &name[idx + 1..])
        }
        _ => format!("{name}.{marker}"),
    }
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
                        assert_ne!(copy_name.as_str(), "a.txt", "the copy must not collide");
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

    #[test]
    fn conflict_name_preserves_the_extension() {
        assert_eq!(
            conflict_name(Path::new("dir/a.txt"), TS, DEV),
            "a.sync-conflict-20260725-120000-laptop.txt"
        );
    }

    #[test]
    fn conflict_name_splits_a_multi_dot_name_at_the_last_dot() {
        assert_eq!(
            conflict_name(Path::new("a.tar.gz"), TS, DEV),
            "a.tar.sync-conflict-20260725-120000-laptop.gz"
        );
    }

    #[test]
    fn conflict_name_treats_a_dotfile_as_having_no_extension() {
        assert_eq!(
            conflict_name(Path::new(".bashrc"), TS, DEV),
            ".bashrc.sync-conflict-20260725-120000-laptop"
        );
    }

    #[test]
    fn conflict_name_handles_a_name_without_an_extension() {
        assert_eq!(
            conflict_name(Path::new("noext"), TS, DEV),
            "noext.sync-conflict-20260725-120000-laptop"
        );
    }

    #[test]
    fn conflict_name_never_lets_a_device_label_become_a_path() {
        let name = conflict_name(Path::new("a.txt"), TS, "work/box");
        assert_eq!(name, "a.sync-conflict-20260725-120000-work-box.txt");
        assert!(!name.contains('/'), "a separator would relocate the copy");

        let spaced = conflict_name(Path::new("a.txt"), TS, "my box");
        assert_eq!(spaced, "a.sync-conflict-20260725-120000-my-box.txt");
    }

    #[test]
    fn conflict_name_caps_a_hostile_device_label() {
        let long = "x".repeat(200);
        let name = conflict_name(Path::new("a.txt"), TS, &long);
        assert!(
            name.len() < 100,
            "a 200-char label must not blow the filename budget: {name}"
        );
    }

    #[test]
    fn conflict_name_survives_a_path_with_no_file_name() {
        let name = conflict_name(Path::new("/"), TS, DEV);
        assert_eq!(name, "sync-conflict-20260725-120000-laptop");
    }
}
