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

use keeper_paths::conflict::conflict_name;
use std::ffi::OsString;
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
