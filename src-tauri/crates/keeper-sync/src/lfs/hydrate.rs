//! Asking for one path's content, and the refusal that protects the bytes
//! already there (Story 56.3, FR-338).
//!
//! # Hydration is a verb, not a side effect
//!
//! There is no on-read hydration anywhere in keeper and there deliberately is
//! not going to be: nothing in this engine sits in front of `open(2)`, so a
//! virtual path becomes real only because something *asked*. This module is
//! the per-path decision behind that ask — what may be published, what is
//! already there, and what keeper will not touch — and it is shared verbatim
//! by both doors onto it (`keeper-syncd materialize` and the
//! `sync_materialize_entry` command), so neither can grow a policy of its own.
//!
//! # The refusal is named for the guarantee, not for the verb
//!
//! [`ContentRefusal`] says *keeper will not change the bytes at this path*.
//! That is the same sentence on both sides of the epic — materializing must not
//! overwrite a local modification, and releasing must not delete one — which is
//! why 56.4's five release refusals extend this enum rather than forking a
//! `ReleaseRefusal` beside it. It is a standalone enum with a hand-written
//! `Display`, the idiom [`crate::browse::BrowseRefusal`],
//! [`crate::files_write::WriteRefusal`], [`crate::export::ExportRefusal`] and
//! [`crate::file_serve::ServeRefusal`] already establish, and it is carried
//! across the transport by one [`crate::error::SyncError::Refused`] so that
//! growing the vocabulary costs no churn in the two exhaustive matches that
//! have to classify it.
//!
//! # Containment is `browse`'s rule, and only `browse`'s
//!
//! [`crate::browse::plain_segments`] is the single lexical containment test in
//! this crate (AD-65). Its verdict is carried into
//! [`ContentRefusal::Escapes`] unchanged, exactly as
//! [`crate::file_serve::ServeRefusal::Escapes`] carries it, so there is no
//! second rule here to fall out of step with the first.
//!
//! # The length tie, stated so it is chosen
//!
//! [`plan`] cannot tell "the content is already here" from "an edit that
//! happens to be exactly the content's length" without hashing a file that may
//! be gigabytes. It calls that case [`Plan::AlreadyHeld`] and writes nothing,
//! which is the direction that cannot lose bytes — the same trade
//! `prune::worktree_holds_content` already makes for the same reason. The
//! opposite reading would have this verb overwrite a same-length edit.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::browse::BrowseRefusal;
use crate::lfs::pointer::Pointer;
use crate::lfs::stage::{self, PendingSmudge};

/// Why keeper will not change the bytes at one path.
///
/// Every variant is a state of the *request* rather than of the folder, which
/// is why they are separate from [`Plan`]: a caller that produces one has asked
/// for something that must not happen, and the sentence it renders is the one
/// the person who asked reads.
///
/// 56.4 adds `Open`, `UnprovenOnRemote`, `Pinned` and `AlreadyPointer` here
/// rather than to [`crate::error::SyncError`], for the reason this module's doc
/// gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentRefusal {
    /// The subpath is not a plain descendant of the profile root. Carries
    /// [`crate::browse::plain_segments`]'s own refusal, so the reason a log
    /// records is the reason the one containment rule gave.
    Escapes(BrowseRefusal),
    /// The profile's `lfs_mode` is [`crate::profile::LfsMode::Disabled`].
    ///
    /// Refused rather than performed, and this is the asymmetry with
    /// `Engine::lfs_files`: a *listing* deliberately ignores the mode, because
    /// pointers already committed are still pointers and answering "no LFS
    /// paths" was the assembled-twice divergence 56.2's review fixed. A
    /// *write* is the opposite case. Under `Disabled` nothing routes this path
    /// through the clean filter, so real bytes in the worktree are a content
    /// change the next commit would publish as a brand-new blob — which is how
    /// materializing under `Disabled` turns a 4 GB pointer into a 4 GB blob in
    /// the history.
    LfsDisabled {
        /// The profile's name, because the mode is a property of the folder and
        /// the folder is what a person would go and change.
        profile: String,
    },
    /// The folder is paused, so keeper is not touching its working tree at all.
    ///
    /// Both halves of the verb are unavailable while a profile is disabled and
    /// each fails differently, which is why this is refused rather than
    /// attempted: publishing inline would write into a working tree the user
    /// took keeper's hands off, and a queued unit would never be claimed,
    /// because the supervisor skips a disabled profile before it reaches the
    /// journal at all. Reporting "queued" for work nothing will run is the one
    /// answer worse than a refusal.
    Paused {
        /// The profile's name — pausing is a folder-level thing a person did
        /// and can undo.
        profile: String,
    },
    /// Nothing in the index records an LFS pointer for this path: a plain
    /// tracked file, a path git does not track, or a folder with no repository
    /// in it at all.
    ///
    /// The three collapse into one refusal on purpose. Every one of them means
    /// "there is no committed pointer here, so there is no content to ask
    /// for", and distinguishing them would invite the caller to *create* the
    /// missing thing — a `.git`, a tracking rule — which is somebody else's
    /// verb.
    NotTracked {
        /// The subpath as asked for, verbatim.
        path: String,
    },
    /// The path is tracked and outside the profile's `subpaths[]` cone, so its
    /// content is deliberately not kept on this machine (Story 27.2).
    OutsideSubpaths {
        /// The subpath as asked for, verbatim.
        path: String,
    },
    /// The index records a pointer and the worktree holds no file at all.
    ///
    /// Refused rather than checked out, because writing content here would
    /// silently undo a deletion the user has made and not yet committed. A
    /// checkout is `git`'s verb and a sync's, not this one's.
    Missing {
        /// The subpath as asked for, verbatim.
        path: String,
    },
    /// Something is at the path and it is not the pointer this folder
    /// committed, so publishing over it would destroy whatever it is.
    LocallyModified {
        /// The subpath as asked for, verbatim.
        path: String,
    },
}

impl std::fmt::Display for ContentRefusal {
    /// One sentence per refusal, written for the person who asked.
    ///
    /// Each names the guarantee rather than the mechanism: what is on disk
    /// stays on disk. These strings reach a user through
    /// `keeper-syncd`'s stderr and through the IPC envelope's `message`
    /// verbatim, so a mechanism word here would be a mechanism word in a
    /// dialog.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Escapes(refusal) => write!(f, "{refusal}"),
            Self::LfsDisabled { profile } => write!(
                f,
                "{profile} has large-file support turned off, so nothing routes this path \
                 through LFS; keeper will not write content over its pointer"
            ),
            Self::NotTracked { path } => write!(
                f,
                "\"{path}\" is not a large file this folder tracks, so there is no content to \
                 ask for"
            ),
            Self::OutsideSubpaths { path } => write!(
                f,
                "\"{path}\" is outside the paths this folder synchronizes, so its content is \
                 not kept on this machine"
            ),
            Self::Paused { profile } => write!(
                f,
                "{profile} is paused, so keeper is not writing anything into it; resume the \
                 folder and ask again"
            ),
            Self::Missing { path } => write!(
                f,
                "\"{path}\" is not in the folder any more; writing its content back would undo \
                 that deletion, so keeper will not"
            ),
            Self::LocallyModified { path } => write!(
                f,
                "\"{path}\" does not hold the pointer this folder committed, so keeper will not \
                 overwrite what is there"
            ),
        }
    }
}

impl From<BrowseRefusal> for ContentRefusal {
    /// Carry a containment refusal across without re-deriving it.
    ///
    /// The verdict is `browse`'s lexical rule and it already says what was
    /// wrong with the subpath; restating it here would be a second rule to keep
    /// in step with the first.
    fn from(refusal: BrowseRefusal) -> Self {
        Self::Escapes(refusal)
    }
}

/// What asking for one path's content did.
///
/// Three answers rather than a boolean, because "it is already here" and "it is
/// on its way" are different facts and a caller that folded them together could
/// not say which one it is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MaterializeOutcome {
    /// The object was already in this machine's store, so the content was
    /// published inline and the worktree holds the real bytes now.
    Materialized,
    /// The worktree was already holding the content; nothing was written.
    AlreadyMaterialized,
    /// The object is not here yet, so a transfer was queued. The running
    /// daemon delivers it; see [`Materialization::unit_id`].
    Queued,
}

impl std::fmt::Display for MaterializeOutcome {
    /// The words a human rendering shows.
    ///
    /// Prose here, camelCase on the wire, and the two are deliberately not the
    /// same function. `LfsFileState`'s single-word variants let it use one
    /// string for both (`virtual`, `materialized`, `absent`); this enum's
    /// middle answer does not survive that rule — `alreadyMaterialized` in a
    /// sentence a person reads is a token that escaped from a JSON document.
    /// The wire form is `serde`'s, asserted against the key set in
    /// `keeper-syncd`'s renderer tests, so there is still exactly one spelling
    /// per surface.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::Materialized => "materialized",
            Self::AlreadyMaterialized => "already materialized",
            Self::Queued => "queued",
        })
    }
}

/// What one materialize request settled.
///
/// The size and the oid are the **pointer's**, always, for the reason
/// [`crate::lfs::listing::LfsFile`] states: for a virtual path the worktree's
/// `stat` is ~130 bytes of pointer text and the number a person asked about is
/// the one written inside it.
///
/// Deliberately not `Serialize`. `keeper-syncd`'s `--json` form promises
/// `unitId` is **absent** unless the outcome is
/// [`MaterializeOutcome::Queued`], and a derived serialization would emit
/// `unitId: null` instead — so the document is built by a renderer that takes
/// these fields as parameters, where a test can assert the key set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Materialization {
    /// Repository-relative and `/`-joined, the frame every other path in this
    /// crate is already in.
    pub path: String,
    /// The pointer's object id, bare hex.
    pub oid: String,
    /// The pointer's size — the honest number, never the pointer text's.
    pub size_bytes: u64,
    pub outcome: MaterializeOutcome,
    /// The journal row that will deliver the content, for
    /// [`MaterializeOutcome::Queued`] and for nothing else.
    ///
    /// It is the id of the **covering** unit, which for a repeat request is the
    /// row queued the first time: `db::enqueue_unique` deduplicates on the
    /// payload, so asking twice returns one id and queues one download.
    pub unit_id: Option<i64>,
}

/// What may be done with one path, once every refusal has been ruled out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// The worktree already holds content for this path. Nothing to write.
    AlreadyHeld,
    /// The worktree holds the committed pointer, so publishing the object it
    /// names is exactly the smudge [`crate::lfs::stage::materialize`] performs.
    Publish(PendingSmudge),
}

/// Decide what one path's worktree bytes allow, or refuse to touch them.
///
/// `indexed` is the pointer the **index** records for `rela` — the committed
/// truth — and the returned [`PendingSmudge`] carries that pointer rather than
/// the one parsed off the disk, so the object published is the one the folder
/// agreed on.
///
/// # One `stat`, then at most a kilobyte
///
/// A single [`std::fs::symlink_metadata`] settles the missing case, the
/// not-a-regular-file case and the length question;
/// [`stage::worktree_pointer`] reads at most
/// [`crate::lfs::pointer::MAX_POINTER_BYTES`] and only for a file inside that
/// window. A materialized four-gigabyte video therefore costs one `lstat` and
/// no read at all.
///
/// # Why a directory and a wrong-oid pointer give the same answer
///
/// Both are [`ContentRefusal::LocallyModified`], and that is not a shortcut:
/// each means *these are not the committed pointer's bytes*, which is the
/// entire question this function asks. A fifo, a socket, a symlink, a folder
/// and a valid pointer for some other object are all things a user put there,
/// and the guarantee is that keeper does not replace them. A `stat` that fails
/// for any reason other than absence lands here too, for the same reason: a
/// path keeper cannot even see is a path it must not overwrite.
///
/// Not [`stage::pending_smudges`], which is the sweep's shape and *silently
/// skips* a non-pointer. Skipping is right for a whole-tree pass and wrong
/// here: a request for one path must answer, and "I ignored it" is the answer
/// that would let this verb write nothing and report success.
pub fn plan(root: &Path, rela: &Path, indexed: &Pointer) -> Result<Plan, ContentRefusal> {
    let named = || rela.to_string_lossy().into_owned();
    let modified = || ContentRefusal::LocallyModified { path: named() };

    let absolute = root.join(rela);
    let metadata = match std::fs::symlink_metadata(&absolute) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ContentRefusal::Missing { path: named() });
        }
        Err(_) => return Err(modified()),
    };
    // `is_file` rather than `!is_dir`, for `worktree_pointer`'s reason: a fifo,
    // a socket or a device node has a length that is not a number of bytes
    // anyone can read out of it.
    if !metadata.is_file() {
        return Err(modified());
    }

    match stage::worktree_pointer(&absolute, &metadata) {
        // The ordinary virtual path: the worktree bytes ARE the committed
        // pointer (FR-331), so this is the one case there is content to publish.
        Some(found) if found.oid == indexed.oid => Ok(Plan::Publish(PendingSmudge {
            path: rela.to_path_buf(),
            pointer: indexed.clone(),
        })),
        // Pointer text for a different object: an edit, a stale checkout, or a
        // pointer somebody wrote by hand. Publishing `indexed`'s bytes over it
        // would discard it.
        Some(_) => Err(modified()),
        // Not pointer text. Length leads and the tie goes to writing nothing —
        // see the module doc.
        None if metadata.len() == indexed.size => Ok(Plan::AlreadyHeld),
        None => Err(modified()),
    }
}

/// The repository-relative path a subpath's segments name.
///
/// Here rather than at the call site so both doors join the segments the same
/// way, and so nothing composes a path out of the raw subpath string after
/// [`crate::browse::plain_segments`] has vetted it — the join and the check
/// stay one step.
pub fn joined(segments: &[&std::ffi::OsStr]) -> PathBuf {
    segments.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pointer for `size` bytes of a recognisable object, with an oid that
    /// depends on `size` so two fixtures cannot accidentally share one.
    fn pointer(size: u64) -> Pointer {
        Pointer::new(format!("{size:064x}"), size)
    }

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    /// The path this verb exists for: pointer text on disk, the object named by
    /// the index, and something to publish.
    #[test]
    fn the_committed_pointer_is_the_one_case_there_is_content_to_publish() {
        let dir = temp();
        let indexed = pointer(4 * 1024 * 1024);
        std::fs::write(dir.path().join("clip.mp4"), indexed.render()).expect("check out");

        let plan = plan(dir.path(), Path::new("clip.mp4"), &indexed).expect("no refusal");
        assert_eq!(
            plan,
            Plan::Publish(PendingSmudge {
                path: PathBuf::from("clip.mp4"),
                pointer: indexed.clone(),
            }),
            "the smudge carries the INDEX's pointer, so the object published is \
             the one the folder committed"
        );
    }

    /// The length tie, asserted so the choice is a decision rather than a
    /// coincidence: content of exactly the pointer's size that is not pointer
    /// text is treated as already here, and nothing is written.
    #[test]
    fn content_of_the_pointers_own_length_is_already_held() {
        let dir = temp();
        let indexed = pointer(4_096);
        let content = vec![7u8; 4_096];
        std::fs::write(dir.path().join("clip.mp4"), &content).expect("materialized");

        assert_eq!(
            plan(dir.path(), Path::new("clip.mp4"), &indexed).expect("no refusal"),
            Plan::AlreadyHeld
        );
        assert_eq!(
            std::fs::read(dir.path().join("clip.mp4")).expect("read"),
            content,
            "nothing was written"
        );
    }

    /// A deletion the user has not committed yet must not be silently undone.
    #[test]
    fn an_absent_file_is_missing_and_not_something_to_check_out() {
        let dir = temp();
        assert_eq!(
            plan(dir.path(), Path::new("gone.mp4"), &pointer(10)).expect_err("refused"),
            ContentRefusal::Missing {
                path: "gone.mp4".to_owned()
            }
        );
    }

    /// A directory standing where the content should be is not the committed
    /// pointer's bytes, so it is not replaced.
    #[test]
    fn a_directory_at_the_path_is_never_replaced() {
        let dir = temp();
        std::fs::create_dir(dir.path().join("clip.mp4")).expect("mkdir");
        assert_eq!(
            plan(dir.path(), Path::new("clip.mp4"), &pointer(10)).expect_err("refused"),
            ContentRefusal::LocallyModified {
                path: "clip.mp4".to_owned()
            }
        );
        assert!(
            dir.path().join("clip.mp4").is_dir(),
            "and it is still a directory"
        );
    }

    /// Plain bytes of some other length are an edit, and an edit is what this
    /// refusal exists to protect.
    #[test]
    fn an_edited_file_is_refused_and_its_bytes_are_untouched() {
        let dir = temp();
        let edited = b"the user typed this\n";
        let target = dir.path().join("notes.bin");
        std::fs::write(&target, edited).expect("write");

        assert_eq!(
            plan(dir.path(), Path::new("notes.bin"), &pointer(4_096)).expect_err("refused"),
            ContentRefusal::LocallyModified {
                path: "notes.bin".to_owned()
            }
        );
        assert_eq!(
            std::fs::read(&target).expect("read"),
            edited,
            "a refusal writes nothing, which is the whole promise"
        );
    }

    /// Pointer text for a *different* object is the case a `is this a pointer?`
    /// test would wave through: it parses, it is canonical, and publishing the
    /// indexed object over it would discard whatever it names.
    #[test]
    fn pointer_text_for_another_object_is_refused() {
        let dir = temp();
        let other = pointer(9_999);
        std::fs::write(dir.path().join("clip.mp4"), other.render()).expect("write");

        assert_eq!(
            plan(dir.path(), Path::new("clip.mp4"), &pointer(4_096)).expect_err("refused"),
            ContentRefusal::LocallyModified {
                path: "clip.mp4".to_owned()
            }
        );
    }

    /// Every refusal renders a sentence that names the path or the folder, and
    /// none of them leaks a mechanism word.
    ///
    /// The messages reach a user verbatim — `keeper-syncd`'s stderr and the IPC
    /// envelope's `message` both take `to_string()` — so a refusal that did not
    /// say *which* file is a refusal nobody can act on.
    #[test]
    fn every_refusal_names_what_it_is_about() {
        let named: Vec<ContentRefusal> = vec![
            ContentRefusal::NotTracked {
                path: "40-media/clip.mp4".to_owned(),
            },
            ContentRefusal::OutsideSubpaths {
                path: "40-media/clip.mp4".to_owned(),
            },
            ContentRefusal::Missing {
                path: "40-media/clip.mp4".to_owned(),
            },
            ContentRefusal::LocallyModified {
                path: "40-media/clip.mp4".to_owned(),
            },
        ];
        for refusal in named {
            assert!(
                refusal.to_string().contains("40-media/clip.mp4"),
                "got: {refusal}"
            );
        }
        assert!(ContentRefusal::LfsDisabled {
            profile: "Field".to_owned()
        }
        .to_string()
        .contains("Field"));

        // The containment refusal is browse's own sentence, unchanged.
        let escape = BrowseRefusal::Escapes {
            subpath: "../etc/passwd".to_owned(),
        };
        assert_eq!(
            ContentRefusal::from(escape.clone()).to_string(),
            escape.to_string(),
            "carried across, not reworded — there is one containment rule"
        );
    }

    /// Prose for a person, camelCase for a consumer — and the two are the same
    /// three outcomes, in the same order.
    ///
    /// One enum still decides what happened; what differs is the spelling each
    /// surface expects. The pairing is asserted rather than left to two
    /// independent lists, because a variant added to one and not the other is
    /// how a `--json` consumer starts branching on a word no renderer emits.
    #[test]
    fn the_prose_word_and_the_wire_word_are_paired_per_outcome() {
        for (outcome, prose, wire) in [
            (
                MaterializeOutcome::Materialized,
                "materialized",
                "materialized",
            ),
            (
                MaterializeOutcome::AlreadyMaterialized,
                "already materialized",
                "alreadyMaterialized",
            ),
            (MaterializeOutcome::Queued, "queued", "queued"),
        ] {
            assert_eq!(outcome.to_string(), prose, "the sentence a person reads");
            assert_eq!(
                serde_json::to_value(outcome).expect("serialize"),
                serde_json::Value::String(wire.to_owned()),
                "and the token a consumer branches on"
            );
        }
    }
}
