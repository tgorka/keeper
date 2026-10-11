//! Moving keeper's data directory into the App Group container (Epic 98, story
//! 98.1 #6; the plan's Q3, the codemap's Q8).
//!
//! On the iPhone the data directory moves once, from the app's own container to
//! the App Group container the notification extension can also open. D-1 once
//! called that a path change; it is a data migration, because the bytes in the
//! old container still have to get to the new one, and a half-done move is two
//! directories neither of which holds an account. So the move is journaled:
//!
//! - **Once.** A finished move leaves [`MOVE_DONE`] in the destination; every
//!   later run sees it and moves nothing.
//! - **Resumable.** Before the first entry moves, [`MOVE_JOURNAL`] is written
//!   into the destination. A run that finds it finishes what an interrupted one
//!   started — the entries still in the old directory follow the ones already
//!   moved. Each entry moves by one `rename`, so an entry is in one place or
//!   the other, never half in each.
//! - **Never over someone else's files.** A destination that holds anything
//!   and no journal is refused — before anything is moved or marked, a fresh
//!   install included: keeper did not put it there, and moving into it (or
//!   marking it keeper's) could interleave two stores.
//! - **A refusal leaves the old directory whole and in use.** An entry that
//!   cannot move puts back the ones that did and removes the journal, and the
//!   refusal says which directory keeper keeps using. The one exception is a
//!   put-back that itself fails ([`MoveRefusal::Interrupted`]), which says so.
//! - **A name on both sides is a clash, not a retry.** It is refused before
//!   anything moves, the journal stays, and the refusal says the move will not
//!   finish by itself ([`MoveRefusal::Clash`]): which copy is right is not this
//!   module's guess to make.
//! - **Moved is moved.** Once every entry is in the destination, it is the
//!   data directory whether or not the done marker could be written; a missing
//!   marker is written by the next run, with nothing left to move.
//!
//! Pure filesystem, no platform: the iPhone's shell decides the two paths and
//! what to do with the answer (`IosPlatform::resolve` in the shell crate).

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

/// Written into the destination when the move is complete. Holds the old path,
/// for whoever reads the directory later.
pub const MOVE_DONE: &str = ".keeper-moved";

/// Written into the destination before the first entry moves, removed once
/// [`MOVE_DONE`] is written. Holds the old path.
pub const MOVE_JOURNAL: &str = ".keeper-moving";

/// What a run did, when keeper may use the destination afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    /// This run moved `entries` top-level entries (a resumed run counts only its
    /// own) and finished the move. Rarely the done marker could not be written:
    /// the journal then stays, and the next run writes the marker and moves
    /// nothing.
    Now { entries: usize },
    /// A previous run finished it; nothing was touched.
    Already,
    /// The old directory was absent or empty: a fresh install. The destination
    /// is marked as moved so a later run never treats keeper's own files there
    /// as someone else's.
    NothingToMove,
}

/// Why keeper keeps using the old directory.
#[derive(Debug, thiserror::Error)]
pub enum MoveRefusal {
    #[error(
        "keeper did not move its data into {new} because that folder already holds files keeper \
         did not put there; it keeps using {old}"
    )]
    DestinationNotEmpty { old: PathBuf, new: PathBuf },
    #[error(
        "keeper could not move {entry} into {new} ({source}); it put back what it had moved and \
         keeps using {old}"
    )]
    RolledBack {
        old: PathBuf,
        new: PathBuf,
        entry: String,
        #[source]
        source: io::Error,
    },
    #[error(
        "keeper could not move {entry} into {new} ({source}) and could not put everything back, \
         so part of its data is still in {new}; it keeps using {old} and tries the move again the \
         next time it starts"
    )]
    Interrupted {
        old: PathBuf,
        new: PathBuf,
        entry: String,
        #[source]
        source: io::Error,
    },
    #[error(
        "keeper did not move its data into {new}: {entry} is in both {old} and {new}, and keeper \
         will not choose between them; it keeps using {old}, and the move will not finish on its \
         own while {entry} is in both"
    )]
    Clash {
        old: PathBuf,
        new: PathBuf,
        entry: String,
    },
    #[error("keeper could not prepare {path} for its data: {source}")]
    Prepare {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Move everything in `old` into `new`, once (module docs). `Ok` means use
/// `new` from now on; `Err` means keep using `old`, which every refusal but
/// [`MoveRefusal::Interrupted`] leaves whole.
pub fn move_into_group_container(old: &Path, new: &Path) -> Result<Moved, MoveRefusal> {
    run(old, new, Hook::None)
}

/// The failures a test cannot cause through the filesystem alone.
#[derive(Clone, Copy)]
enum Hook {
    None,
    /// The process dies after this many entries: the journal is written and
    /// some entries moved, exactly what a killed process leaves.
    #[cfg(test)]
    DieAfter(usize),
    /// The entry after this many cannot be renamed — what a device answers
    /// for, say, a cross-volume rename.
    #[cfg(test)]
    FailAfter(usize),
    /// Every entry moves, then the done marker cannot be written — what a
    /// full volume answers.
    #[cfg(test)]
    FailMarker,
}

impl Hook {
    /// The error the done marker's write answers instead, when this run
    /// simulates one.
    fn marker_failure(self) -> Option<io::Error> {
        #[cfg(test)]
        if matches!(self, Hook::FailMarker) {
            return Some(io::Error::other("simulated"));
        }
        None
    }
}

/// What a simulated death returns: the caller is gone, nothing reads it.
#[cfg(test)]
const DIED: Moved = Moved::Now {
    entries: usize::MAX,
};

fn run(old: &Path, new: &Path, hook: Hook) -> Result<Moved, MoveRefusal> {
    if new.join(MOVE_DONE).exists() {
        return Ok(Moved::Already);
    }
    let prepare = |path: &Path| {
        let path = path.to_path_buf();
        move |source| MoveRefusal::Prepare { path, source }
    };
    let pending = entries(old).map_err(prepare(old))?;
    if new.join(MOVE_JOURNAL).exists() {
        // A name already in the destination would be renamed onto. Answered
        // before anything moves: what an earlier run moved goes back, so
        // `old` is whole again, and the clash is not shuttled forward and
        // back on every later start.
        if let Some(name) = pending
            .iter()
            .find(|name| new.join(name).symlink_metadata().is_ok())
        {
            return Err(roll_back(old, new, name, already_there()));
        }
    } else {
        // Before anything is marked: a destination keeper did not fill is
        // never made keeper's, not even by a fresh install's marker.
        if !entries(new).map_err(prepare(new))?.is_empty() {
            return Err(MoveRefusal::DestinationNotEmpty {
                old: old.to_path_buf(),
                new: new.to_path_buf(),
            });
        }
        std::fs::create_dir_all(new).map_err(prepare(new))?;
        if pending.is_empty() {
            write_marker(new, MOVE_DONE, old).map_err(prepare(new))?;
            return Ok(Moved::NothingToMove);
        }
        write_marker(new, MOVE_JOURNAL, old).map_err(prepare(new))?;
    }

    let mut moved = 0;
    for name in pending {
        let to = new.join(&name);
        let result = match hook {
            #[cfg(test)]
            Hook::DieAfter(n) if n == moved => return Ok(DIED),
            #[cfg(test)]
            Hook::FailAfter(n) if n == moved => Err(io::Error::other("simulated")),
            _ if to.symlink_metadata().is_ok() => Err(already_there()),
            _ => std::fs::rename(old.join(&name), &to),
        };
        if let Err(source) = result {
            return Err(roll_back(old, new, &name, source));
        }
        moved += 1;
    }

    // Every entry is in `new` now, so it is the data directory whatever the
    // marker says: answering `Err` here would send keeper back to an emptied
    // `old`. A marker that cannot be written leaves the journal, and the next
    // run — nothing pending — writes it.
    let marked = match hook.marker_failure() {
        Some(err) => Err(err),
        None => write_marker(new, MOVE_DONE, old),
    };
    match marked {
        // Best effort from here: the move is complete and marked, and a
        // leftover journal or an empty old directory changes nothing a later
        // run decides.
        Ok(()) => {
            if let Err(err) = std::fs::remove_file(new.join(MOVE_JOURNAL)) {
                tracing::warn!(%err, "container move: could not remove the journal");
            }
        }
        Err(err) => tracing::warn!(
            %err,
            path = %new.display(),
            "container move: every entry moved but the done marker could not be written; the \
             journal stays and the next start writes it"
        ),
    }
    if let Err(err) = std::fs::remove_dir(old) {
        tracing::warn!(%err, path = %old.display(), "container move: could not remove the old directory");
    }
    Ok(Moved::Now { entries: moved })
}

fn already_there() -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        "the destination already has an entry of that name",
    )
}

/// Put back every moved entry, so the old directory is whole again, then
/// remove the journal and the emptied destination.
///
/// A name found on both sides cannot be told apart from one keeper moved, so
/// it stays where it is: every other entry still goes back, the journal stays,
/// and the answer is a [`MoveRefusal::Clash`] naming it. The next run finds the
/// same name on both sides before it moves anything and answers the same,
/// with nothing left to put back — no entry is shuttled forward and back on
/// every start.
fn roll_back(old: &Path, new: &Path, failed: &OsStr, source: io::Error) -> MoveRefusal {
    let mut clashing = None;
    let put_back = entries(new).is_ok_and(|names| {
        names.into_iter().fold(true, |all, name| {
            let back = old.join(&name);
            if back.symlink_metadata().is_ok() {
                clashing.get_or_insert(name);
                return all;
            }
            let went = std::fs::rename(new.join(&name), &back).is_ok();
            all && went
        })
    });
    let (old, new) = (old.to_path_buf(), new.to_path_buf());
    if let Some(entry) = clashing.filter(|_| put_back) {
        return MoveRefusal::Clash {
            old,
            new,
            entry: entry.to_string_lossy().into_owned(),
        };
    }
    let entry = failed.to_string_lossy().into_owned();
    if put_back && std::fs::remove_file(new.join(MOVE_JOURNAL)).is_ok() {
        // An empty destination is what the next run expects to find.
        let _ = std::fs::remove_dir(&new);
        MoveRefusal::RolledBack {
            old,
            new,
            entry,
            source,
        }
    } else {
        MoveRefusal::Interrupted {
            old,
            new,
            entry,
            source,
        }
    }
}

/// The top-level entries of `dir`, sorted, minus the two markers; an absent
/// directory has none.
fn entries(dir: &Path) -> io::Result<Vec<OsString>> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut names = Vec::new();
    for entry in read {
        let name = entry?.file_name();
        if name != MOVE_JOURNAL && name != MOVE_DONE {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn write_marker(dir: &Path, name: &str, old: &Path) -> io::Result<()> {
    std::fs::write(dir.join(name), old.to_string_lossy().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A data directory shaped like an iPhone's: files, a nested store, a
    /// phone folder.
    fn populate(dir: &Path) {
        std::fs::create_dir_all(dir.join("accounts/01A/sdk")).expect("accounts");
        std::fs::write(
            dir.join("accounts/01A/sdk/matrix-sdk-state.sqlite3"),
            b"state",
        )
        .expect("store");
        std::fs::write(dir.join("keeper.db"), b"settings").expect("keeper.db");
        std::fs::write(dir.join("archive.db"), b"archive").expect("archive.db");
        std::fs::create_dir_all(dir.join("sync/01P/.git")).expect("folder");
        std::fs::write(dir.join("sync.db"), b"profiles").expect("sync.db");
    }

    /// Every file under `dir`, relative, with its bytes — the whole content a
    /// person would miss.
    fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in std::fs::read_dir(dir).expect("read dir") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    out.push((
                        path.strip_prefix(root).expect("under").to_path_buf(),
                        Vec::new(),
                    ));
                    walk(root, &path, out);
                } else {
                    let bytes = std::fs::read(&path).expect("read");
                    out.push((path.strip_prefix(root).expect("under").to_path_buf(), bytes));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.retain(|(path, _)| path.as_os_str() != MOVE_DONE && path.as_os_str() != MOVE_JOURNAL);
        out.sort();
        out
    }

    /// 98.1 #6, `container_move_is_journaled_and_idempotent`: everything moves
    /// once; a second run is a no-op; a run killed after half the entries is
    /// finished by the next; a run whose done marker cannot be written still
    /// answers the destination, and the next marks it; a destination keeper did
    /// not fill is refused and the old directory stays whole and in use.
    #[test]
    fn container_move_is_journaled_and_idempotent() {
        let root = tempfile::tempdir().expect("tempdir");
        let old = root
            .path()
            .join("app/Library/Application Support/dev.tgorka.keeper");
        let new = root
            .path()
            .join("group/Library/Application Support/dev.tgorka.keeper");
        populate(&old);
        let before = tree(&old);

        // Moves everything, once.
        assert_eq!(
            move_into_group_container(&old, &new).expect("move"),
            Moved::Now { entries: 5 }
        );
        assert_eq!(tree(&new), before);
        assert!(!old.exists(), "the emptied old directory is removed");
        assert!(!new.join(MOVE_JOURNAL).exists());

        // A second run is a no-op — even when the old directory is back with
        // files in it (an older build started once in between).
        std::fs::create_dir_all(&old).expect("old again");
        std::fs::write(old.join("keeper.db"), b"stale").expect("stale");
        assert_eq!(
            move_into_group_container(&old, &new).expect("again"),
            Moved::Already
        );
        assert_eq!(tree(&new), before);
        assert_eq!(
            std::fs::read(old.join("keeper.db")).expect("stale"),
            b"stale"
        );

        // A run killed after half the entries is finished by the next one.
        let old = root.path().join("killed/app/dev.tgorka.keeper");
        let new = root.path().join("killed/group/dev.tgorka.keeper");
        populate(&old);
        let before = tree(&old);
        assert_eq!(run(&old, &new, Hook::DieAfter(2)).expect("killed"), DIED);
        assert!(
            new.join(MOVE_JOURNAL).exists(),
            "the journal precedes every entry"
        );
        assert_eq!(entries(&new).expect("new").len(), 2);
        assert_eq!(entries(&old).expect("old").len(), 3);
        // The journal makes the half-full destination keeper's own.
        assert_eq!(
            move_into_group_container(&old, &new).expect("resume"),
            Moved::Now { entries: 3 }
        );
        assert_eq!(tree(&new), before);
        assert!(!new.join(MOVE_JOURNAL).exists() && new.join(MOVE_DONE).exists());

        // Every entry moved and the done marker could not be written: the
        // destination is the data directory all the same (the old one is
        // empty), and the next run — nothing pending — writes the marker.
        let old = root.path().join("unmarked/app/dev.tgorka.keeper");
        let new = root.path().join("unmarked/group/dev.tgorka.keeper");
        populate(&old);
        let before = tree(&old);
        assert_eq!(
            run(&old, &new, Hook::FailMarker).expect("moved, unmarked"),
            Moved::Now { entries: 5 }
        );
        assert_eq!(tree(&new), before);
        assert!(new.join(MOVE_JOURNAL).exists() && !new.join(MOVE_DONE).exists());
        assert_eq!(
            move_into_group_container(&old, &new).expect("marked"),
            Moved::Now { entries: 0 }
        );
        assert_eq!(tree(&new), before);
        assert!(!new.join(MOVE_JOURNAL).exists() && new.join(MOVE_DONE).exists());

        // A destination keeper did not fill is refused; nothing moves.
        let old = root.path().join("foreign/app/dev.tgorka.keeper");
        let new = root.path().join("foreign/group/dev.tgorka.keeper");
        populate(&old);
        let before = tree(&old);
        std::fs::create_dir_all(&new).expect("new");
        std::fs::write(new.join("keeper.db"), b"someone else's").expect("foreign");
        let refusal = move_into_group_container(&old, &new).expect_err("refused");
        assert!(
            matches!(refusal, MoveRefusal::DestinationNotEmpty { .. }),
            "{refusal}"
        );
        assert_eq!(tree(&old), before);
        assert_eq!(
            std::fs::read(new.join("keeper.db")).expect("kept"),
            b"someone else's"
        );
        assert!(!new.join(MOVE_DONE).exists() && !new.join(MOVE_JOURNAL).exists());
    }

    /// An entry that cannot move puts back the ones that did: the old
    /// directory is whole again, the destination is gone, and the next run
    /// starts over cleanly.
    #[test]
    fn an_entry_that_cannot_move_puts_back_the_ones_that_did() {
        let root = tempfile::tempdir().expect("tempdir");
        let old = root.path().join("app/dev.tgorka.keeper");
        let new = root.path().join("group/dev.tgorka.keeper");
        populate(&old);
        let before = tree(&old);

        let refusal = run(&old, &new, Hook::FailAfter(3)).expect_err("fails");
        assert!(
            matches!(refusal, MoveRefusal::RolledBack { .. }),
            "{refusal}"
        );
        assert_eq!(tree(&old), before);
        assert!(!new.exists(), "the destination is as the run found it");

        assert_eq!(
            move_into_group_container(&old, &new).expect("next launch"),
            Moved::Now { entries: 5 }
        );
        assert_eq!(tree(&new), before);
    }

    /// A name on both sides cannot be told apart from one keeper moved: it is
    /// a clash, answered before anything moves. What the killed run moved goes
    /// back, so the old directory is whole; the journal stays; a later start
    /// answers the same without moving anything; and once the clash is gone the
    /// next run moves everything.
    #[test]
    fn a_name_on_both_sides_is_a_clash_answered_before_anything_moves() {
        let root = tempfile::tempdir().expect("tempdir");
        let old = root.path().join("app/dev.tgorka.keeper");
        let new = root.path().join("group/dev.tgorka.keeper");
        populate(&old);
        let before = tree(&old);
        assert_eq!(run(&old, &new, Hook::DieAfter(2)).expect("killed"), DIED);
        std::fs::write(new.join("keeper.db"), b"clash").expect("clash");

        // `DieAfter(0)` dies at the first entry the loop would move: answering
        // at all proves nothing was moved forward to be put back again.
        for start in ["first", "every later"] {
            let refusal = run(&old, &new, Hook::DieAfter(0)).expect_err(start);
            assert!(
                matches!(&refusal, MoveRefusal::Clash { entry, .. } if entry == "keeper.db"),
                "{start} start: {refusal}"
            );
            assert_eq!(
                tree(&old),
                before,
                "{start} start: the old directory is whole"
            );
            assert_eq!(
                entries(&new).expect("new"),
                vec![OsString::from("keeper.db")]
            );
            assert!(new.join(MOVE_JOURNAL).exists());
        }

        std::fs::remove_file(new.join("keeper.db")).expect("clear the clash");
        assert_eq!(
            move_into_group_container(&old, &new).expect("resume"),
            Moved::Now { entries: 5 }
        );
        assert_eq!(tree(&new), before);
    }

    /// A fresh install marks an empty destination as keeper's; a destination
    /// that already holds files keeper did not put there is refused even with
    /// nothing to move, and is not marked.
    #[test]
    fn a_fresh_install_marks_an_empty_destination_and_refuses_a_foreign_one() {
        let root = tempfile::tempdir().expect("tempdir");
        let old = root.path().join("app/dev.tgorka.keeper");
        let new = root.path().join("group/dev.tgorka.keeper");
        assert_eq!(
            move_into_group_container(&old, &new).expect("fresh"),
            Moved::NothingToMove
        );
        // keeper's own files there later are never "someone else's".
        std::fs::write(new.join("keeper.db"), b"settings").expect("first run");
        populate(&old);
        assert_eq!(
            move_into_group_container(&old, &new).expect("again"),
            Moved::Already
        );

        let old = root.path().join("foreign/app/dev.tgorka.keeper");
        let new = root.path().join("foreign/group/dev.tgorka.keeper");
        std::fs::create_dir_all(&new).expect("new");
        std::fs::write(new.join("log.txt"), b"someone else's").expect("foreign");
        let refusal = move_into_group_container(&old, &new).expect_err("refused");
        assert!(
            matches!(refusal, MoveRefusal::DestinationNotEmpty { .. }),
            "{refusal}"
        );
        assert!(!new.join(MOVE_DONE).exists() && !new.join(MOVE_JOURNAL).exists());
    }
}
