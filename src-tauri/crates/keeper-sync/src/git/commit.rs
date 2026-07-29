//! Working tree → index → tree → commit (Story 24.4, AD-44).
//!
//! This is the write half of the engine, and it is entirely gitoxide: staging
//! and committing are local operations that must work with no network at all
//! (AD-49).
//!
//! Three things about gitoxide shape this module:
//!
//! * There is **no `write-tree`**. `Repository::index_from_tree` goes one way
//!   only, and the `tree-editor` feature is not enabled for this crate, so
//!   `write_tree_from_index` folds the sorted index into nested trees itself.
//! * **Index mutation is plumbing.** There is no `git add`; entries are pushed
//!   with `dangerously_push_entry`, which explicitly hands the caller
//!   responsibility for the sort invariant.
//! * **The tree-cache is not invalidated on write.** `gix_index::File::write`
//!   serializes the `tree` extension exactly as it was read, still marked
//!   valid, so a stale cache would make a later commit capture outdated
//!   subtree content. It is dropped before every write here.
//!
//! ## Precondition on content size (AD-46)
//!
//! [`stage_and_commit`] writes whatever bytes it finds on disk straight into
//! the object database. Content above the profile's LFS threshold **must
//! already have been replaced by its pointer** by the caller: gitoxide has no
//! streaming object read, and `Repository::write_blob_stream` is not streaming
//! either despite the name — it copies the whole reader into memory to hash it.
//! A 3 GB file committed here would be a 3 GB allocation and a permanently
//! bloated repository.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::{Path, PathBuf},
    time::Instant,
};

use gix::{
    bstr::BString,
    index::entry::{Flags, Mode, Stage, Stat},
};

use crate::{
    db::DeviceIdentity,
    error::{Result, SyncError},
    lfs::basic::{ProgressCoalescer, DEFAULT_PROGRESS_INTERVAL},
    profile::SyncProfile,
    provenance::{change_subject, commit_message, Provenance},
};

/// Observes staging as it walks the change set: `(files_done, path in flight)`.
///
/// `false` means the receiver is gone and reporting should stop, the same
/// contract [`crate::progress::ProgressSink`] and
/// [`crate::lfs::basic::TransferSink`] carry. Narrower than `ProgressSink` on
/// purpose: staging knows which path it is reading and how many it has passed,
/// and nothing else — profiles, phases and denominators belong to the engine.
pub type StagingSink<'a> = &'a dyn Fn(u64, &Path) -> bool;

/// Report one staging step, if anyone is listening and it is not too soon.
///
/// Coalesced at [`DEFAULT_PROGRESS_INTERVAL`] like every other progress
/// producer in the crate: staging ten thousand files is ten events a second,
/// not ten thousand events. The clock is only read when a sink exists, so a
/// caller that passes `None` pays nothing per file.
///
/// The last path of the change set is always reported, for the reason
/// `git::fetch`'s emitter always forwards its completion tick: a detail line
/// left naming the second of ten thousand files while the commit is written is
/// worse than one that updates a little less often.
fn report(
    sink: &mut Option<StagingSink<'_>>,
    coalescer: &mut ProgressCoalescer,
    files_done: u64,
    files_total: u64,
    current: &Path,
) {
    let Some(observer) = *sink else {
        return;
    };
    let last = files_done + 1 >= files_total;
    if !last && !coalescer.should_emit(Instant::now()) {
        return;
    }
    if !observer(files_done, current) {
        *sink = None;
    }
}

/// Paths to stage, all repository-relative.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StagedChange {
    /// Paths that are new to the index.
    pub added: Vec<PathBuf>,
    /// Tracked paths whose content or mode changed.
    pub modified: Vec<PathBuf>,
    /// Tracked paths to remove.
    pub deleted: Vec<PathBuf>,
    /// How big each path was when it was last measured (Story 34.6).
    ///
    /// A missing key means the size is not knowable, which is a real answer: a
    /// file can vanish between the scan and the commit, and a deletion only has
    /// a size while something still records one. Staging never reads this — it
    /// rides here because this is the last structure that still names the
    /// individual paths one commit moved, and the recently-synced list needs
    /// them named and measured together.
    pub sizes: BTreeMap<PathBuf, u64>,
}

impl StagedChange {
    /// Whether there is nothing at all to stage.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    /// Total number of affected paths.
    pub fn len(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }
}

/// At most this many paths are listed individually in a commit body.
///
/// A 10 000-file first sync would otherwise produce a commit message no tool
/// can display and every `git log` would scroll for minutes.
const MAX_LISTED_PATHS: usize = 50;

/// Stage `changes` and commit them on `HEAD`.
///
/// Returns `Ok(None)` when there is nothing to record — either because the
/// change set is empty, or because every staged path turned out to be
/// byte-identical to what `HEAD` already holds. An empty commit is never
/// created: it would be pushed, replicated to every peer and shown in the UI
/// while meaning nothing.
///
/// `progress` observes each path as it is reached; see [`StagingSink`].
///
/// See the module docs for the LFS precondition on `changes`.
pub fn stage_and_commit(
    repo: &gix::Repository,
    changes: &StagedChange,
    provenance: &Provenance,
    profile: &SyncProfile,
    author: &gix::actor::Signature,
    substitutions: &BTreeMap<PathBuf, Vec<u8>>,
    progress: Option<StagingSink<'_>>,
) -> Result<Option<gix::hash::ObjectId>> {
    if changes.is_empty() {
        return Ok(None);
    }
    let workdir = super::repo::workdir(repo)?;

    // An owned copy of the index: `index_or_empty` hands out a shared snapshot,
    // and for a repository that has never been staged it still carries the path
    // the index has to be written to.
    let shared = repo
        .index_or_empty()
        .map_err(|err| SyncError::Git(format!("could not read the index: {err}")))?;
    let mut index: gix::index::File = (**shared).clone();
    // Lookups are a binary search, so they are only valid over the region that
    // was sorted when we started. Everything pushed below lands after it.
    let sorted_len = index.entries().len();

    // `files_done` counts paths this loop has finished, so a report names the
    // path being read next to the number already behind it — which is exactly
    // the pairing the engine publishes before staging starts, so the stream
    // never contradicts its own opening frame.
    let mut sink = progress;
    let mut coalescer = ProgressCoalescer::new(DEFAULT_PROGRESS_INTERVAL);
    let mut files_done = 0u64;
    let files_total = changes.len() as u64;

    for rela in changes.added.iter().chain(changes.modified.iter()) {
        report(&mut sink, &mut coalescer, files_done, files_total, rela);
        let key = index_key(rela)?;
        let absolute = workdir.join(rela);
        let metadata = gix::index::fs::Metadata::from_path_no_follow(&absolute)
            .map_err(|source| SyncError::io("stat staged file", absolute.clone(), source))?;

        let (mode, content) = if metadata.is_symlink() {
            // A symlink's blob is its target, not the bytes it points at.
            let target = std::fs::read_link(&absolute)
                .map_err(|source| SyncError::io("read symlink", absolute.clone(), source))?;
            (
                Mode::SYMLINK,
                Vec::from(gix::path::into_bstr(target).into_owned()),
            )
        } else if metadata.is_file() {
            let mode = if metadata.is_executable() {
                Mode::FILE_EXECUTABLE
            } else {
                Mode::FILE
            };
            match substitutions.get(rela) {
                // An LFS-tracked path: the blob is the ~130-byte pointer while
                // the worktree keeps the real bytes. The entry's stat below is
                // still taken from the WORKTREE file, which is what makes
                // `gix::status` (and `git status`) call it unchanged without
                // reading gigabytes back — exactly how git+LFS itself works.
                Some(pointer) => (mode, pointer.clone()),
                None => {
                    let bytes = std::fs::read(&absolute).map_err(|source| {
                        SyncError::io("read staged file", absolute.clone(), source)
                    })?;
                    (mode, bytes)
                }
            }
        } else {
            // A fifo, socket or device has no meaning on a peer's filesystem;
            // this is one of the few conditions a human has to resolve.
            return Err(SyncError::InvalidPathForRemote {
                path: rela.clone(),
                reason: "only regular files and symlinks can be synchronized".to_owned(),
            });
        };

        let oid = repo
            .write_blob(&content)
            .map_err(|err| SyncError::Git(format!("could not write {}: {err}", rela.display())))?
            .detach();
        // A pre-epoch timestamp is the only way this fails; an all-zero stat
        // just makes the entry racily clean, so the next status re-reads it.
        let stat = Stat::from_fs(&metadata).unwrap_or_default();

        let existing = index.entry_index_by_path_and_stage_bounded(
            key.as_ref(),
            Stage::Unconflicted,
            sorted_len,
        );
        match existing {
            Some(idx) => {
                let entry = &mut index.entries_mut()[idx];
                entry.id = oid;
                entry.mode = mode;
                entry.stat = stat;
                // Re-adding a path that a previous pass marked for removal.
                entry.flags.remove(Flags::REMOVE);
            }
            None => index.dangerously_push_entry(stat, oid, Flags::empty(), mode, key.as_ref()),
        }
        files_done += 1;
    }

    if !changes.deleted.is_empty() {
        let mut doomed: BTreeSet<BString> = BTreeSet::new();
        for rela in &changes.deleted {
            // Removal itself is one bulk index pass, so this walk is the only
            // place a deletion can be named. Counting them here is what lets the
            // bar reach its denominator: the engine's total spans all three
            // groups, and a counter that stopped at added+modified would strand
            // it short on any pass that deleted anything.
            report(&mut sink, &mut coalescer, files_done, files_total, rela);
            doomed.insert(index_key(rela)?);
            files_done += 1;
        }
        // `BString: Borrow<BStr>`, so the callback's borrowed path is the key.
        index.remove_entries(|_, path, _| doomed.contains(path));
    }

    // `dangerously_push_entry` appended out of order and `remove_entries` left
    // gaps; the sort invariant has to hold before anything looks a path up or
    // serializes the index.
    index.sort_entries();
    // See the module docs: gix writes the tree-cache back verbatim, still
    // marked valid, so a cache built before these mutations would make a later
    // commit capture outdated subtrees.
    index.remove_tree();
    // The index is written before the commit on purpose: a crash here leaves a
    // staged-but-uncommitted state that the next run re-drives, which is what
    // NFR-24 asks for. The reverse order could lose the staging entirely.
    index
        .write(gix::index::write::Options::default())
        .map_err(|err| SyncError::Git(format!("could not write the index: {err}")))?;

    let tree_id = write_tree_from_index(repo, &index)?;
    let parent = super::repo::head_commit_id(repo)?;
    let parent_tree = match parent {
        Some(id) => Some(
            repo.find_commit(id)
                .map_err(|err| SyncError::Git(format!("could not read HEAD commit: {err}")))?
                .tree_id()
                .map_err(|err| SyncError::Git(format!("could not read HEAD tree: {err}")))?
                .detach(),
        ),
        None => None,
    };
    if parent_tree == Some(tree_id) {
        tracing::debug!(
            profile = profile.name,
            "staged paths matched HEAD exactly; no commit created"
        );
        return Ok(None);
    }

    let subject = change_subject(
        &profile.commit_subject_template,
        &profile.name,
        changes.added.len(),
        changes.modified.len(),
        changes.deleted.len(),
    );
    let message = commit_message(&subject, &change_body(changes), provenance);

    // `commit_as` takes `impl Into<SignatureRef>`, and `Signature` does *not*
    // convert into one: the borrowed form keeps the raw time string, which has
    // to be serialized into a buffer that outlives the call.
    let mut time_buf = gix::date::parse::TimeBuf::default();
    let signature = author.to_ref(&mut time_buf);
    let parents: Vec<gix::hash::ObjectId> = parent.into_iter().collect();

    // Checked here rather than on entry, and deliberately as late as possible:
    // this is the instant before the reference transaction opens, so it is the
    // moment at which the answer is most nearly still true. See
    // `repo::ensure_head_unlocked` — a branch lock somebody else holds does
    // not fail this call, it hangs it, on a full core, indefinitely.
    super::repo::ensure_head_unlocked(repo)?;

    let id = repo
        .commit_as(signature, signature, "HEAD", &message, tree_id, parents)
        .map_err(|err| SyncError::Git(format!("commit failed: {err}")))?
        .detach();
    Ok(Some(id))
}

/// Derive `(name, email)` for the git author of a profile's commits (AD-44).
///
/// The default email is a **non-routable** `sync@<device-id>.keeper.invalid`:
/// `.invalid` is reserved by RFC 2606 and can never resolve, so history stays
/// attributable to a device without publishing anyone's real address into a
/// repository that may be shared or made public.
pub fn author_for(profile: &SyncProfile, device: &DeviceIdentity) -> (String, String) {
    let fallback_email = format!("sync@{}.keeper.invalid", device.id.to_ascii_lowercase());
    let Some(raw) = profile
        .author_override
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return (
            sanitize_actor(&device.label),
            sanitize_actor(&fallback_email),
        );
    };

    if let Some((name, email)) = split_actor(raw) {
        let name = if name.is_empty() {
            device.label.as_str()
        } else {
            name
        };
        return (sanitize_actor(name), sanitize_actor(email));
    }
    if raw.contains('@') {
        // A bare address: keep the device label as the display name.
        (sanitize_actor(&device.label), sanitize_actor(raw))
    } else {
        // A bare display name: keep the non-routable address.
        (sanitize_actor(raw), sanitize_actor(&fallback_email))
    }
}

/// Split a `Name <email>` override.
fn split_actor(raw: &str) -> Option<(&str, &str)> {
    let open = raw.rfind('<')?;
    let close = raw.rfind('>')?;
    if close <= open {
        return None;
    }
    let email = raw[open + 1..close].trim();
    if email.is_empty() {
        return None;
    }
    Some((raw[..open].trim(), email))
}

/// Strip characters that would corrupt a commit object.
///
/// git's actor line is `Name <email> <time>`: an `<`, `>` or newline inside
/// either field makes the object unparseable by every tool including git
/// itself, and `author_override` is user-supplied text.
fn sanitize_actor(value: &str) -> String {
    value
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | '\n' | '\r'))
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The commit body: a capped, marked list of what changed.
fn change_body(changes: &StagedChange) -> String {
    let total = changes.len();
    let mut body = String::with_capacity(64 * MAX_LISTED_PATHS.min(total));
    let mut listed = 0usize;

    'listing: for (marker, paths) in [
        ('+', &changes.added),
        ('~', &changes.modified),
        ('-', &changes.deleted),
    ] {
        for path in paths {
            if listed == MAX_LISTED_PATHS {
                break 'listing;
            }
            // A newline is a legal byte in a POSIX filename and would split the
            // body into a paragraph the reader cannot attribute.
            let shown = path.display().to_string().replace(['\n', '\r'], " ");
            let _ = writeln!(body, "{marker} {shown}");
            listed += 1;
        }
    }

    if total > listed {
        let _ = writeln!(body, "... and {} more", total - listed);
    }
    body
}

/// Repository-relative path to the slash-separated key the index uses.
fn index_key(rela: &std::path::Path) -> Result<BString> {
    if rela.is_absolute() {
        return Err(SyncError::InvalidPathForRemote {
            path: rela.to_path_buf(),
            reason: "staged paths must be repository-relative".to_owned(),
        });
    }
    if rela
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(SyncError::InvalidPathForRemote {
            path: rela.to_path_buf(),
            reason: "a staged path must not escape the repository".to_owned(),
        });
    }
    let raw = gix::path::try_into_bstr(rela).map_err(|_| SyncError::InvalidPathForRemote {
        path: rela.to_path_buf(),
        reason: "path is not valid UTF-8 and cannot be stored in a git index".to_owned(),
    })?;
    Ok(gix::path::to_unix_separators(raw).into_owned())
}

/// One directory that the tree builder currently has open: its name relative
/// to its parent, and the entries accumulated for it so far.
type TreeFrame = (Vec<u8>, Vec<gix::objs::tree::Entry>);

/// Fold the index's stage-0 entries into nested tree objects and return the
/// root tree's id.
///
/// The index is sorted by raw path bytes, which is *exactly* git's tree order
/// once a directory is understood to sort as if it ended in `/` — `.` (0x2E)
/// and `-` (0x2D) sort before `/` (0x2F), and every ordinary name byte after
/// it. That equivalence is what lets a single forward pass close and open
/// directories as it goes, without ever holding more than one path of frames.
fn write_tree_from_index(
    repo: &gix::Repository,
    index: &gix::index::State,
) -> Result<gix::hash::ObjectId> {
    let backing = index.path_backing();
    // One frame per open directory; frame 0 is the root tree and has no name.
    let mut stack: Vec<TreeFrame> = vec![(Vec::new(), Vec::new())];

    for entry in index.entries() {
        // Conflicted stages never reach here (AD-43 resolves by copy, not by
        // merge), and a REMOVE-flagged entry is one the writer would skip.
        if entry.stage() != Stage::Unconflicted || entry.flags.contains(Flags::REMOVE) {
            continue;
        }
        let mode = entry.mode.to_tree_entry_mode().ok_or_else(|| {
            SyncError::Git(format!(
                "index entry has a mode git cannot store in a tree: {:o}",
                entry.mode.bits()
            ))
        })?;

        let mut rest: &[u8] = entry.path_in(backing);

        // Descend through the directories already open that this path shares.
        let mut matched = 0usize;
        while let Some(slash) = rest.iter().position(|byte| *byte == b'/') {
            match stack.get(matched + 1) {
                Some((open, _)) if open.as_slice() == &rest[..slash] => {
                    matched += 1;
                    rest = &rest[slash + 1..];
                }
                _ => break,
            }
        }
        // Everything deeper than the shared prefix is complete: write it out.
        while stack.len() > matched + 1 {
            fold_top(repo, &mut stack)?;
        }
        // Open whatever directories this path enters.
        while let Some(slash) = rest.iter().position(|byte| *byte == b'/') {
            stack.push((rest[..slash].to_vec(), Vec::new()));
            rest = &rest[slash + 1..];
        }

        let Some((_, entries)) = stack.last_mut() else {
            return Err(SyncError::Git(
                "tree builder lost its root frame".to_owned(),
            ));
        };
        entries.push(gix::objs::tree::Entry {
            mode,
            filename: BString::from(rest),
            oid: entry.id,
        });
    }

    while stack.len() > 1 {
        fold_top(repo, &mut stack)?;
    }
    let Some((_, root)) = stack.pop() else {
        return Err(SyncError::Git(
            "tree builder lost its root frame".to_owned(),
        ));
    };
    write_tree(repo, root)
}

/// Write the deepest open directory and record it in its parent.
fn fold_top(repo: &gix::Repository, stack: &mut Vec<TreeFrame>) -> Result<()> {
    let Some((name, entries)) = stack.pop() else {
        return Err(SyncError::Git("tree builder underflowed".to_owned()));
    };
    let id = write_tree(repo, entries)?;
    let Some((_, parent)) = stack.last_mut() else {
        return Err(SyncError::Git(
            "tree builder lost its root frame".to_owned(),
        ));
    };
    parent.push(gix::objs::tree::Entry {
        mode: gix::objs::tree::EntryKind::Tree.into(),
        filename: BString::from(name),
        oid: id,
    });
    Ok(())
}

/// Serialize one tree object.
fn write_tree(
    repo: &gix::Repository,
    mut entries: Vec<gix::objs::tree::Entry>,
) -> Result<gix::hash::ObjectId> {
    // The caller's order is already correct, but a mis-sorted tree object is
    // silently corrupt — git accepts it and then disagrees with itself about
    // the content — so the invariant is enforced rather than assumed.
    entries.sort();
    let tree = gix::objs::Tree { entries };
    Ok(repo
        .write_object(&tree)
        .map_err(|err| SyncError::Git(format!("could not write tree: {err}")))?
        .detach())
}

#[cfg(test)]
mod tests {
    /// No LFS substitution: the blob written is the worktree content, which is
    /// what every test here except the pointer ones expects.
    fn no_lfs() -> BTreeMap<PathBuf, Vec<u8>> {
        BTreeMap::new()
    }

    use super::*;
    use crate::provenance::SyncSource;

    fn signature() -> gix::actor::Signature {
        gix::actor::Signature {
            name: "Keeper".into(),
            email: "sync@01abc.keeper.invalid".into(),
            time: gix::date::Time::new(1_700_000_000, 0),
        }
    }

    fn provenance() -> Provenance {
        Provenance::new(
            "docs",
            "work laptop",
            "01ABC",
            "localhost",
            SyncSource::Watch,
        )
    }

    fn device() -> DeviceIdentity {
        DeviceIdentity {
            id: "01ABCDEF".to_owned(),
            label: "work laptop".to_owned(),
        }
    }

    fn profile() -> SyncProfile {
        SyncProfile::new("01P", "docs", "/tmp/docs", "https://git.example.com/x.git")
    }

    fn commit_files(
        dir: &std::path::Path,
        repo: &gix::Repository,
        files: &[(&str, &str)],
    ) -> Option<gix::hash::ObjectId> {
        let mut changes = StagedChange::default();
        for (name, content) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("mkdir");
            }
            std::fs::write(&path, content).expect("write");
            changes.added.push(PathBuf::from(name));
        }
        stage_and_commit(
            repo,
            &changes,
            &provenance(),
            &profile(),
            &signature(),
            &no_lfs(),
            None,
        )
        .expect("commit")
    }

    #[test]
    fn an_empty_change_set_never_produces_a_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        let got = stage_and_commit(
            &repo,
            &StagedChange::default(),
            &provenance(),
            &profile(),
            &signature(),
            &no_lfs(),
            None,
        )
        .expect("no error");
        assert_eq!(got, None);
    }

    #[test]
    fn a_commit_message_round_trips_through_provenance_parse() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        let id = commit_files(dir.path(), &repo, &[("a.txt", "alpha")]).expect("a commit");

        let commit = repo.find_commit(id).expect("find");
        let message = commit.message_raw_sloppy().to_string();
        let parsed = Provenance::parse(&message).expect("trailers are present");
        assert_eq!(parsed, provenance());
        assert!(
            message.starts_with("sync(docs): 1 added"),
            "unexpected subject: {message}"
        );
    }

    /// The end-to-end half of Story 34.5(b): a template stored on the profile
    /// has to be what `git log` shows, and the trailer block has to be
    /// untouched by it — provenance is not decoration.
    #[test]
    fn a_profiles_subject_template_is_what_the_commit_says() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        let mut templated = profile();
        templated.commit_subject_template = "backup {profile}: {changed} file(s)".to_owned();
        std::fs::write(dir.path().join("a.txt"), "alpha").expect("write");
        let changes = StagedChange {
            added: vec![PathBuf::from("a.txt")],
            ..StagedChange::default()
        };

        let id = stage_and_commit(
            &repo,
            &changes,
            &provenance(),
            &templated,
            &signature(),
            &no_lfs(),
            None,
        )
        .expect("commit")
        .expect("a non-empty commit");

        let message = repo
            .find_commit(id)
            .expect("find")
            .message_raw_sloppy()
            .to_string();
        let mut lines = message.lines();
        assert_eq!(lines.next(), Some("backup docs: 1 file(s)"));
        // The trailer block is not templatable, so it still parses whole.
        assert_eq!(
            Provenance::parse(&message).expect("trailers are present"),
            provenance()
        );
    }

    #[test]
    fn a_commit_records_the_content_in_nested_trees() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        commit_files(
            dir.path(),
            &repo,
            &[
                ("top.txt", "top"),
                ("a/b/deep.txt", "deep"),
                // Sorts between `a` and `a/…` in tree order; a builder that got
                // the directory ordering wrong would produce a corrupt tree.
                ("a.txt", "sibling"),
            ],
        )
        .expect("a commit");

        let head = repo.head_commit().expect("head");
        let tree = head.tree().expect("tree");
        for path in ["top.txt", "a.txt", "a/b/deep.txt"] {
            assert!(
                tree.lookup_entry_by_path(path).expect("lookup").is_some(),
                "{path} is missing from the committed tree"
            );
        }
        // Nothing is left staged once the commit exists.
        assert!(!crate::git::repo::is_dirty(&repo).expect("dirty check"));
    }

    #[test]
    fn restaging_identical_content_produces_no_second_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        commit_files(dir.path(), &repo, &[("a.txt", "alpha")]).expect("first commit");

        let again = StagedChange {
            modified: vec![PathBuf::from("a.txt")],
            ..StagedChange::default()
        };
        let got = stage_and_commit(
            &repo,
            &again,
            &provenance(),
            &profile(),
            &signature(),
            &no_lfs(),
            None,
        )
        .expect("no error");
        assert_eq!(got, None, "an unchanged file must not create a commit");
    }

    #[test]
    fn a_deletion_removes_the_path_from_the_next_tree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        commit_files(dir.path(), &repo, &[("a.txt", "alpha"), ("b.txt", "beta")])
            .expect("first commit");

        std::fs::remove_file(dir.path().join("b.txt")).expect("remove");
        let removal = StagedChange {
            deleted: vec![PathBuf::from("b.txt")],
            ..StagedChange::default()
        };
        stage_and_commit(
            &repo,
            &removal,
            &provenance(),
            &profile(),
            &signature(),
            &no_lfs(),
            None,
        )
        .expect("commit")
        .expect("a deletion is a change");

        let tree = repo.head_commit().expect("head").tree().expect("tree");
        assert!(tree
            .lookup_entry_by_path("a.txt")
            .expect("lookup")
            .is_some());
        assert!(tree
            .lookup_entry_by_path("b.txt")
            .expect("lookup")
            .is_none());
    }

    #[test]
    fn staging_names_each_path_and_counts_past_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        commit_files(dir.path(), &repo, &[("gone.txt", "bye")]).expect("first commit");
        for name in ["a.txt", "b.txt"] {
            std::fs::write(dir.path().join(name), name).expect("write");
        }
        std::fs::remove_file(dir.path().join("gone.txt")).expect("remove");

        let changes = StagedChange {
            added: vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")],
            deleted: vec![PathBuf::from("gone.txt")],
            ..StagedChange::default()
        };
        let seen = std::sync::Mutex::new(Vec::<(u64, PathBuf)>::new());
        let sink = |files_done: u64, current: &Path| -> bool {
            let mut log = seen.lock().expect("sink lock");
            log.push((files_done, current.to_owned()));
            true
        };
        stage_and_commit(
            &repo,
            &changes,
            &provenance(),
            &profile(),
            &signature(),
            &no_lfs(),
            Some(&sink),
        )
        .expect("commit")
        .expect("three paths are a change");

        let seen = seen.into_inner().expect("sink lock");
        // Two frames are guaranteed whatever the clock did: the coalescer always
        // passes the first call, and the last path is forced. Anything between
        // them depends on how long three tiny files took, which is not something
        // a test may assume.
        assert_eq!(
            seen.first(),
            Some(&(0, PathBuf::from("a.txt"))),
            "the first frame names the first path with nothing behind it"
        );
        assert_eq!(
            seen.last(),
            Some(&(2, PathBuf::from("gone.txt"))),
            "the last frame is the deletion, with both writes counted past it"
        );
        // Every frame moves forward, and no frame invents a path. A `current`
        // pinned to the first staged path — which is what the engine used to
        // publish once and never revise — fails here.
        let order = [
            PathBuf::from("a.txt"),
            PathBuf::from("b.txt"),
            PathBuf::from("gone.txt"),
        ];
        let mut previous: Option<u64> = None;
        for (files_done, current) in &seen {
            assert!(
                previous.is_none_or(|earlier| *files_done > earlier),
                "{files_done} did not advance past {previous:?}"
            );
            assert_eq!(
                order.get(*files_done as usize),
                Some(current),
                "frame {files_done} named the wrong path"
            );
            previous = Some(*files_done);
        }
        assert!(
            seen.len() <= order.len(),
            "staging must never report more frames than it has paths"
        );
    }

    #[test]
    fn a_sink_that_says_stop_is_not_called_again() {
        // The `ProgressSink` contract: `false` means the receiver is gone, so a
        // ten-thousand-path commit must not keep calling into a dead channel.
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        let mut changes = StagedChange::default();
        for index in 0..8 {
            let name = format!("f{index}.txt");
            std::fs::write(dir.path().join(&name), "x").expect("write");
            changes.added.push(PathBuf::from(name));
        }

        let calls = std::sync::atomic::AtomicUsize::new(0);
        let sink = |_: u64, _: &Path| -> bool {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            false
        };
        stage_and_commit(
            &repo,
            &changes,
            &provenance(),
            &profile(),
            &signature(),
            &no_lfs(),
            Some(&sink),
        )
        .expect("commit")
        .expect("eight paths are a change");

        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "the sink refused the first frame, so there is no second"
        );
    }

    #[test]
    fn a_staged_path_that_escapes_the_repository_is_refused() {
        let err = index_key(std::path::Path::new("../secrets")).expect_err("must reject");
        assert_eq!(err.code(), "invalidPath");
        let err = index_key(std::path::Path::new("/etc/passwd")).expect_err("must reject");
        assert_eq!(err.code(), "invalidPath");
    }

    #[test]
    fn a_large_change_list_is_summarised_rather_than_dumped() {
        let changes = StagedChange {
            added: (0..10_000)
                .map(|i| PathBuf::from(format!("f{i:05}.txt")))
                .collect(),
            ..StagedChange::default()
        };
        let body = change_body(&changes);

        assert_eq!(
            body.lines().filter(|l| l.starts_with("+ ")).count(),
            MAX_LISTED_PATHS
        );
        assert!(
            body.contains(&format!("... and {} more", 10_000 - MAX_LISTED_PATHS)),
            "the remainder must be summarised: {body}"
        );
        assert!(body.len() < 8_192, "body grew to {} bytes", body.len());
    }

    #[test]
    fn a_small_change_list_is_listed_in_full_with_markers() {
        let changes = StagedChange {
            added: vec![PathBuf::from("new.txt")],
            modified: vec![PathBuf::from("edited.txt")],
            deleted: vec![PathBuf::from("gone.txt")],
            ..StagedChange::default()
        };
        let body = change_body(&changes);
        assert_eq!(body, "+ new.txt\n~ edited.txt\n- gone.txt\n");
    }

    #[test]
    fn a_newline_in_a_path_cannot_split_the_commit_body() {
        let changes = StagedChange {
            added: vec![PathBuf::from("evil\nKeeper-Source: bot")],
            ..StagedChange::default()
        };
        let body = change_body(&changes);
        assert_eq!(body.lines().count(), 1, "body was split: {body:?}");
    }

    #[test]
    fn the_author_defaults_to_a_non_routable_device_address() {
        let (name, email) = author_for(&profile(), &device());
        assert_eq!(name, "work laptop");
        assert_eq!(email, "sync@01abcdef.keeper.invalid");
        assert!(
            email.ends_with(".keeper.invalid"),
            "the default must never be a routable address"
        );
    }

    #[test]
    fn the_author_override_is_honoured_in_both_shapes() {
        let mut with_full = profile();
        with_full.author_override = Some("Ada Lovelace <ada@example.com>".to_owned());
        assert_eq!(
            author_for(&with_full, &device()),
            ("Ada Lovelace".to_owned(), "ada@example.com".to_owned())
        );

        let mut with_email = profile();
        with_email.author_override = Some("ada@example.com".to_owned());
        assert_eq!(
            author_for(&with_email, &device()),
            ("work laptop".to_owned(), "ada@example.com".to_owned())
        );

        let mut with_name = profile();
        with_name.author_override = Some("Ada".to_owned());
        assert_eq!(
            author_for(&with_name, &device()),
            ("Ada".to_owned(), "sync@01abcdef.keeper.invalid".to_owned())
        );
    }

    #[test]
    fn a_blank_override_falls_back_to_the_default() {
        let mut blank = profile();
        blank.author_override = Some("   ".to_owned());
        assert_eq!(
            author_for(&blank, &device()),
            (
                "work laptop".to_owned(),
                "sync@01abcdef.keeper.invalid".to_owned()
            )
        );
    }

    #[test]
    fn an_override_cannot_smuggle_delimiters_into_the_actor_line() {
        let mut hostile = profile();
        hostile.author_override = Some("Bad <a@b.c> <x@y.z>\nmore".to_owned());
        let (name, email) = author_for(&hostile, &device());
        for field in [&name, &email] {
            assert!(
                !field.contains(['<', '>', '\n', '\r']),
                "delimiter survived: {field:?}"
            );
        }
    }
}
