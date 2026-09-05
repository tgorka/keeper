//! The phone's history reads (Epic 66, Story 66.4, AD-198).
//!
//! The desktop answers "who changed this note", "what did it say at that
//! revision" and "what changed between these two" with `git log`, `git show`
//! and `git diff` — reads, and cheap ones, so the shell never needed an engine
//! API for them. A phone spawns nothing, so the same four questions are
//! answered here in-process, over the same repository, with gitoxide:
//!
//! * [`file_log`] — the commits that touched one path, newest first;
//! * [`recent_commits`] — the last N commits and which paths under a prefix
//!   each touched, which is what the unread projection is built from;
//! * [`blob_at`] — one path's bytes as of one revision;
//! * [`unified_diff`] — the `@@` hunks between two revisions, or a revision
//!   and the working tree, in the format `git diff --unified=3` prints;
//! * [`dirty_paths`] — the paths under a prefix whose bytes differ from `HEAD`.
//!
//! One deliberate difference from the desktop: [`file_log`] does not follow
//! renames (`git log --follow`). A note's identity is its ULID and survives a
//! rename already (FR-97); what stops at the rename is the list of older
//! revisions under the previous filename, and the phone says nothing about
//! them rather than guessing. A rename-detecting walk is `git`'s heuristic
//! over every commit's whole diff, and the phone reads history on a battery.
//!
//! Every function takes the repository's path and opens it read-only for the
//! call: history is a read, and holding a `gix::Repository` across the notes
//! registry's lifetime would pin an object store the reconciler never needs.

use std::{collections::HashSet, path::Path};

use gix::bstr::ByteSlice as _;

use super::repo::{open_read_only, status_paths};
use crate::error::{Result, SyncError};

/// One commit, as the desktop's `--format=%H%x1f%ct%x1f%B` printed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRevision {
    /// The full hex object id — the string the history panel and the restore
    /// verb hand back, so both name one object.
    pub id: String,
    /// Committer time in whole seconds since the epoch (`%ct`).
    pub committed_secs: i64,
    /// The whole message, subject line and trailers included (`%B`).
    pub message: String,
}

impl FileRevision {
    /// The subject: the first line of the message (`%s`).
    pub fn subject(&self) -> &str {
        self.message.lines().next().unwrap_or("").trim()
    }
}

/// One commit and the paths under the asked-for prefix it touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchedCommit {
    pub revision: FileRevision,
    /// Repository-relative, `/`-separated, prefix included — exactly what
    /// `git log --name-only` prints.
    pub paths: Vec<String>,
}

/// Open for reading, with the object cache a walk that decodes every commit's
/// tree wants.
fn open(repo_path: &Path) -> Result<gix::Repository> {
    let mut repo = open_read_only(repo_path, true)?;
    repo.object_cache_size_if_unset(4 * 1024 * 1024);
    Ok(repo)
}

fn walk_error(err: &dyn std::error::Error) -> SyncError {
    SyncError::Git(format!(
        "could not read the local history: {}",
        super::fetch::flatten(err)
    ))
}

/// `HEAD`'s commit, or `None` on an unborn branch — a repository that has
/// never committed has an honest empty history, not an error (AD-63).
fn head(repo: &gix::Repository) -> Result<Option<gix::ObjectId>> {
    super::repo::head_commit_id(repo)
}

/// The walk every reader here runs: from `HEAD`, newest first.
fn walk(repo: &gix::Repository) -> Result<Option<gix::revision::Walk<'_>>> {
    let Some(tip) = head(repo)? else {
        return Ok(None);
    };
    repo.rev_walk([tip])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map(Some)
        .map_err(|err| walk_error(&err))
}

fn revision_of(commit: &gix::Commit<'_>) -> Result<FileRevision> {
    let committed_secs = commit.time().map_err(|err| walk_error(&err))?.seconds;
    let message = commit
        .message_raw()
        .map_err(|err| walk_error(&err))?
        .to_str_lossy()
        .into_owned();
    Ok(FileRevision {
        id: commit.id().to_hex().to_string(),
        committed_secs,
        message,
    })
}

/// The blob a tree holds at `rel`, or `None` where the path is not in it.
fn entry_at(tree: &gix::Tree<'_>, rel: &str) -> Result<Option<gix::ObjectId>> {
    tree.lookup_entry_by_path(rel)
        .map(|entry| entry.map(|entry| entry.object_id()))
        .map_err(|err| walk_error(&err))
}

/// The first parent's tree, or the empty tree for a root commit.
fn parent_tree<'repo>(
    repo: &'repo gix::Repository,
    commit: &gix::Commit<'repo>,
) -> Result<gix::Tree<'repo>> {
    let Some(parent) = commit.parent_ids().next() else {
        return Ok(repo.empty_tree());
    };
    let object = parent.object().map_err(|err| walk_error(&err))?;
    let commit = object.try_into_commit().map_err(|err| walk_error(&err))?;
    commit.tree().map_err(|err| walk_error(&err))
}

/// The commits that changed `rel` — added it, rewrote it, or removed it —
/// newest first, at most `limit` of them.
///
/// A commit counts when the blob at `rel` differs from its first parent's,
/// which is what `git log -- <path>` reports on a linear history. A merge
/// commit that carries another side's change to the path is listed once, as
/// git lists it.
pub fn file_log(repo_path: &Path, rel: &str, limit: usize) -> Result<Vec<FileRevision>> {
    let repo = open(repo_path)?;
    let mut out = Vec::new();
    let Some(walk) = walk(&repo)? else {
        return Ok(out);
    };
    for info in walk {
        if out.len() >= limit {
            break;
        }
        let info = info.map_err(|err| walk_error(&err))?;
        let commit = info.object().map_err(|err| walk_error(&err))?;
        let here = entry_at(&commit.tree().map_err(|err| walk_error(&err))?, rel)?;
        let before = entry_at(&parent_tree(&repo, &commit)?, rel)?;
        if here != before {
            out.push(revision_of(&commit)?);
        }
    }
    Ok(out)
}

/// The last `limit` commits, newest first, each with the paths under `prefix`
/// it touched. A commit that touched nothing under the prefix is still
/// listed, with an empty path list, so `limit` counts commits and not hits —
/// the same window `git log -n<limit> --name-only -- <prefix>` bounds.
///
/// `prefix` is repository-relative with a trailing `/`, or empty for the
/// whole tree.
pub fn recent_commits(repo_path: &Path, prefix: &str, limit: usize) -> Result<Vec<TouchedCommit>> {
    let repo = open(repo_path)?;
    let mut out = Vec::new();
    let Some(walk) = walk(&repo)? else {
        return Ok(out);
    };
    for info in walk.take(limit) {
        let info = info.map_err(|err| walk_error(&err))?;
        let commit = info.object().map_err(|err| walk_error(&err))?;
        let tree = commit.tree().map_err(|err| walk_error(&err))?;
        let parent = parent_tree(&repo, &commit)?;
        let mut paths = Vec::new();
        parent
            .changes()
            .map_err(|err| walk_error(&err))?
            .options(|options| {
                options.track_path();
                // A rename is a deletion and an addition here, as
                // `--name-only` without `-M` lists it: both paths moved.
                options.track_rewrites(None);
            })
            .for_each_to_obtain_tree(&tree, |change| {
                // `--name-only` lists files. A directory that appears or
                // vanishes arrives here as its own entry, and is not one.
                if change.entry_mode().is_tree() {
                    return Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()));
                }
                let location = change.location().to_str_lossy();
                if location.starts_with(prefix) {
                    paths.push(location.into_owned());
                }
                Ok(std::ops::ControlFlow::Continue(()))
            })
            .map_err(|err| walk_error(&err))?;
        out.push(TouchedCommit {
            revision: revision_of(&commit)?,
            paths,
        });
    }
    Ok(out)
}

/// One path's bytes as of `rev`, or `None` where that revision does not hold
/// the path. `rev` is anything `git rev-parse` accepts — a full or abbreviated
pub fn blob_at(repo_path: &Path, rev: &str, rel: &str) -> Result<Option<Vec<u8>>> {
    let repo = open(repo_path)?;
    let Ok(id) = repo.rev_parse_single(rev) else {
        return Ok(None);
    };
    let Ok(object) = id.object() else {
        return Ok(None);
    };
    let Ok(commit) = object.peel_to_kind(gix::object::Kind::Commit) else {
        return Ok(None);
    };
    let tree = commit
        .into_commit()
        .tree()
        .map_err(|err| walk_error(&err))?;
    let Some(entry) = tree
        .lookup_entry_by_path(rel)
        .map_err(|err| walk_error(&err))?
    else {
        return Ok(None);
    };
    let blob = entry.object().map_err(|err| walk_error(&err))?;
    Ok(Some(blob.detach().data))
}

/// The unified diff of `rel` from `from_rev` to `to_rev`, or to the working
/// tree when `to_rev` is `None` — the hunks and nothing else, as
/// `git diff --unified=3 <from> [<to>] -- <rel>` prints after its header.
///
/// A side that does not hold the path is the empty text, so a note added
/// after `from_rev` diffs as all additions, and one removed since as all
/// removals. Bytes that are not UTF-8 are read lossily: a note is UTF-8 by
/// construction, and a diff of a file that is not is a diff nobody will read.
pub fn unified_diff(
    repo_path: &Path,
    rel: &str,
    from_rev: &str,
    to_rev: Option<&str>,
) -> Result<String> {
    use gix::diff::blob::{
        unified_diff::{ConsumeBinaryHunk, ContextSize},
        Algorithm, InternedInput, UnifiedDiff,
    };

    let before = blob_at(repo_path, from_rev, rel)?.unwrap_or_default();
    let after = match to_rev {
        Some(rev) => blob_at(repo_path, rev, rel)?.unwrap_or_default(),
        None => std::fs::read(repo_path.join(rel)).unwrap_or_default(),
    };
    let before = String::from_utf8_lossy(&before);
    let after = String::from_utf8_lossy(&after);
    let input = InternedInput::new(before.as_ref(), after.as_ref());
    let diff = gix::diff::blob::diff_with_slider_heuristics(Algorithm::Histogram, &input);
    UnifiedDiff::new(
        &diff,
        &input,
        ConsumeBinaryHunk::new(String::new(), "\n"),
        ContextSize::symmetrical(3),
    )
    .consume()
    .map_err(|err| SyncError::Git(format!("could not render the diff: {err}")))
}

/// Every path under `prefix` whose bytes on disk are not what `HEAD` holds:
/// added, modified, deleted, or never tracked at all. Repository-relative,
/// `/`-separated, prefix included — the paths `git status --porcelain` names.
pub fn dirty_paths(repo_path: &Path, prefix: &str) -> Result<HashSet<String>> {
    let repo = open(repo_path)?;
    let status = status_paths(&repo)?;
    Ok(status
        .added
        .iter()
        .chain(&status.modified)
        .chain(&status.deleted)
        .chain(&status.untracked)
        .map(|path| {
            path.components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .filter(|path| path.starts_with(prefix))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, process::Command};

    use super::*;

    /// A `git` that reads no configuration but the repository's own, with a
    /// fixed identity and a clock that ticks by the second so commit times
    /// order the way the walk sorts them. `None` where no `git` is on `PATH`.
    struct Repo {
        dir: PathBuf,
        tick: std::cell::Cell<i64>,
    }

    impl Repo {
        fn init(dir: &Path) -> Option<Self> {
            let repo = Self {
                dir: dir.to_path_buf(),
                tick: std::cell::Cell::new(1_700_000_000),
            };
            repo.try_git(&["init", "-q", "-b", "main"]).then_some(repo)
        }

        fn command(&self) -> Command {
            let stamp = format!("{} +0000", self.tick.get());
            let mut command = Command::new("git");
            command
                .current_dir(&self.dir)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .env("GIT_AUTHOR_DATE", &stamp)
                .env("GIT_COMMITTER_DATE", &stamp);
            command
        }

        fn try_git(&self, args: &[&str]) -> bool {
            self.command()
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        }

        fn git(&self, args: &[&str]) -> String {
            let output = self.command().args(args).output().expect("git");
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, text).expect("write");
        }

        /// Commit everything with `message`, one second later than the last.
        fn commit(&self, message: &str) -> String {
            self.tick.set(self.tick.get() + 1);
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "--allow-empty", "-m", message]);
            self.git(&["rev-parse", "HEAD"])
        }
    }

    /// Three commits: the note is added, an unrelated file is added, the note
    /// is rewritten. The log names the first and the third, newest first,
    /// with the ids, times and whole messages `git log` would print.
    #[test]
    fn a_file_log_names_the_commits_that_changed_the_path_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("notes/a.md", "one\n");
        let added = repo.commit("add a\n\nKeeper-Device: mac\n");
        repo.write("notes/b.md", "other\n");
        repo.commit("add b");
        repo.write("notes/a.md", "one, revised\n");
        let revised = repo.commit("revise a");

        let log = file_log(dir.path(), "notes/a.md", 10).expect("log");
        assert_eq!(
            log.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec![revised.as_str(), added.as_str()]
        );
        assert_eq!(log[0].subject(), "revise a");
        assert_eq!(log[1].message, "add a\n\nKeeper-Device: mac\n");
        assert!(log[0].committed_secs > log[1].committed_secs);
        // Bounded, newest kept.
        assert_eq!(
            file_log(dir.path(), "notes/a.md", 1).expect("log"),
            vec![log[0].clone()]
        );
        // A path nobody committed, and a repository with no commit at all.
        assert!(file_log(dir.path(), "notes/none.md", 10)
            .expect("log")
            .is_empty());
        let empty = tempfile::tempdir().expect("tempdir");
        let Some(_) = Repo::init(empty.path()) else {
            return;
        };
        assert!(file_log(empty.path(), "notes/a.md", 10)
            .expect("an unborn branch is an empty history")
            .is_empty());
    }

    /// A deletion is a change to the path too, and it comes back as such.
    #[test]
    fn a_removal_is_a_revision_of_the_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("notes/a.md", "one\n");
        repo.commit("add a");
        std::fs::remove_file(dir.path().join("notes/a.md")).expect("rm");
        let removed = repo.commit("remove a");
        let log = file_log(dir.path(), "notes/a.md", 10).expect("log");
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].id, removed);
        assert_eq!(
            blob_at(dir.path(), &removed, "notes/a.md").expect("read"),
            None,
            "the revision that removed it does not hold it"
        );
    }

    /// The unread projection's window: every commit, newest first, with the
    /// paths under the vault subfolder each touched — and only those.
    #[test]
    fn recent_commits_name_the_paths_under_the_prefix_each_touched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("notes/a.md", "one\n");
        repo.write("README.md", "outside the vault\n");
        let first = repo.commit("first");
        repo.write("notes/a.md", "one, revised\n");
        repo.write("notes/deep/c.md", "deep\n");
        let second = repo.commit("second");
        repo.write("README.md", "still outside\n");
        let third = repo.commit("third");

        let recent = recent_commits(dir.path(), "notes/", 10).expect("recent");
        let ids: Vec<&str> = recent.iter().map(|c| c.revision.id.as_str()).collect();
        assert_eq!(ids, vec![third.as_str(), second.as_str(), first.as_str()]);
        assert!(recent[0].paths.is_empty(), "the README is not under notes/");
        let mut second_paths = recent[1].paths.clone();
        second_paths.sort();
        assert_eq!(second_paths, vec!["notes/a.md", "notes/deep/c.md"]);
        assert_eq!(recent[2].paths, vec!["notes/a.md"]);
        assert_eq!(
            recent_commits(dir.path(), "notes/", 2)
                .expect("recent")
                .len(),
            2,
            "the limit counts commits"
        );
    }

    /// `blob_at` reads what `git show <rev>:<path>` prints, for a full id,
    /// an abbreviated one and `HEAD`; a revision that names nothing is `None`.
    #[test]
    fn a_blob_at_a_revision_is_the_bytes_git_show_prints() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("notes/a.md", "one\n");
        let first = repo.commit("first");
        repo.write("notes/a.md", "two\n");
        repo.commit("second");
        assert_eq!(
            blob_at(dir.path(), &first, "notes/a.md").expect("read"),
            Some(b"one\n".to_vec())
        );
        assert_eq!(
            blob_at(dir.path(), &first[..10], "notes/a.md").expect("read"),
            Some(b"one\n".to_vec())
        );
        assert_eq!(
            blob_at(dir.path(), "HEAD", "notes/a.md").expect("read"),
            Some(b"two\n".to_vec())
        );
        assert_eq!(
            blob_at(
                dir.path(),
                "0000000000000000000000000000000000000000",
                "notes/a.md"
            )
            .expect("a revision that is not there reads as nothing"),
            None
        );
    }

    /// The hunks are what `git diff --unified=3` prints — the same header
    /// arithmetic, the same prefixes — between two revisions and against the
    /// working tree.
    #[test]
    fn a_unified_diff_matches_git_diff() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        let body: String = (1..=12).map(|n| format!("line {n}\n")).collect();
        repo.write("notes/a.md", &body);
        let first = repo.commit("first");
        let edited = body.replace("line 6\n", "line six\n");
        repo.write("notes/a.md", &edited);
        let second = repo.commit("second");
        // Uncommitted on top, for the working-tree half.
        repo.write("notes/a.md", &format!("{edited}line 13\n"));

        let ours = unified_diff(dir.path(), "notes/a.md", &first, Some(&second)).expect("diff");
        let theirs = repo.git(&[
            "diff",
            "--no-color",
            "--unified=3",
            &first,
            &second,
            "--",
            "notes/a.md",
        ]);
        // git appends the enclosing "function" line after the second `@@`;
        // the hunk parser on the far side reads the four numbers and ignores
        // it, so the comparison does too.
        let theirs_hunks: String = theirs
            .lines()
            .skip_while(|line| !line.starts_with("@@"))
            .map(|line| match line.strip_prefix("@@ ") {
                Some(header) => format!("@@ {} @@\n", header.split(" @@").next().unwrap_or("")),
                None => format!("{line}\n"),
            })
            .collect();
        assert_eq!(ours, theirs_hunks);
        assert!(ours.starts_with("@@ -3,7 +3,7 @@"));

        let working = unified_diff(dir.path(), "notes/a.md", &second, None).expect("diff");
        assert!(working.contains("+line 13"), "{working}");
        let added = unified_diff(dir.path(), "notes/new.md", &first, None).expect("diff");
        assert!(added.is_empty(), "a path on neither side has no hunks");
        std::fs::write(dir.path().join("notes/new.md"), "fresh\n").expect("write");
        let added = unified_diff(dir.path(), "notes/new.md", &first, None).expect("diff");
        // gitoxide spells an empty side `-1,0` where git spells it `-0,0`;
        // the four numbers parse the same on the far side.
        assert_eq!(added, "@@ -1,0 +1,1 @@\n+fresh\n");
    }

    /// The dirty set names what `git status` names under the prefix and
    /// nothing outside it.
    #[test]
    fn dirty_paths_are_the_status_under_the_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(repo) = Repo::init(dir.path()) else {
            return;
        };
        repo.write("notes/kept.md", "kept\n");
        repo.write("notes/edited.md", "before\n");
        repo.write("notes/gone.md", "gone\n");
        repo.write("README.md", "outside\n");
        repo.commit("first");
        repo.write("notes/edited.md", "after\n");
        repo.write("notes/new.md", "new\n");
        repo.write("README.md", "outside, edited\n");
        std::fs::remove_file(dir.path().join("notes/gone.md")).expect("rm");

        let mut dirty: Vec<String> = dirty_paths(dir.path(), "notes/")
            .expect("status")
            .into_iter()
            .collect();
        dirty.sort();
        assert_eq!(
            dirty,
            vec!["notes/edited.md", "notes/gone.md", "notes/new.md"]
        );
    }
}
