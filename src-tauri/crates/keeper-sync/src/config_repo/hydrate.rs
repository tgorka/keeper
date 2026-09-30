//! Materialising a directory of the config repository whose files are git-LFS
//! pointers (AD-341).
//!
//! The copy [`super::clone_or_fetch`] keeps is gix-only and knows nothing of
//! LFS, so a file the repository tracks through LFS arrives as its pointer.
//! [`hydrate_lfs_dir`] fetches the objects those pointers name through this
//! crate's own client — [`crate::lfs::batch`] for the negotiation,
//! [`crate::lfs::basic`] for the transfer and its sha256 + size check, the same
//! code drives use — and lays the content out under a destination outside the
//! copy, where the next hard reset of the copy cannot touch it.
//!
//! # What it promises
//!
//! * Only `<clone>/<rel_dir>` is read, and no symbolic link is followed on the
//!   way there or inside it.
//! * An object is published only after its digest and length matched its
//!   pointer, and every file is published by rename: a reader of `dest` sees
//!   the old file or the whole new one, never a torn one.
//! * A file already in place is not fetched again. [`STATE_FILE`] remembers
//!   which object each path holds, so a launch does not re-hash hundreds of
//!   megabytes of models to find that nothing changed; a path it does not
//!   remember is hashed once and adopted when it matches.
//! * Files in `dest` the source no longer names are left alone.
//! * Objects come from the LFS endpoint of the remote URL and nowhere else: a
//!   `.lfsconfig` in the repository is ignored, so the repository credential
//!   is never sent to a host someone with write access to it named.
//! * A set is either complete or not ready: [`COMPLETE_FILE`] is removed
//!   before the first change to `dest` and written last, after everything is
//!   in place, and [`hydration_is_current`] checks it against the copy — so a
//!   half-updated set (new encoder beside an old decoder) is never loaded.

use std::{
    collections::BTreeMap,
    fs::File,
    future::Future,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};

use super::{blocking, tree_path, RepoAuth, ORIGIN};
use crate::{
    error::{Result, SyncError},
    lfs::{
        basic::BasicTransfer,
        batch::{BatchClient, ObjectId},
        endpoint,
        pointer::{Pointer, MAX_POINTER_BYTES},
        store::LfsStore,
    },
};

/// The record, inside `dest`, of which object each hydrated path holds.
pub const STATE_FILE: &str = ".keeper-hydrate.json";

/// The completion marker, inside `dest`: `{ "digest": … }` over every file
/// the source directory names and what it holds, written last and only by a
/// run that placed everything. Its absence means `dest` may hold a mix of two
/// sets; see [`hydration_is_current`].
pub const COMPLETE_FILE: &str = ".keeper-models-complete.json";

/// The object store downloads land in before they are moved into place.
///
/// Inside `dest` so the move is a rename on one filesystem, and kept between
/// runs so an interrupted multi-hundred-megabyte download resumes from its
/// staged prefix instead of starting over.
const STORE_DIR: &str = ".keeper-hydrate-lfs";

/// How often a running transfer looks at the interrupt flag.
const INTERRUPT_POLL: Duration = Duration::from_millis(200);

const STATE_VERSION: u32 = 1;

/// What one hydration did. Every file the source names is counted exactly once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HydrateReport {
    /// Files written from LFS objects fetched by this run.
    pub downloaded: usize,
    /// Files that were not pointers and differed from `dest`, copied.
    pub copied: usize,
    /// Files already in place.
    pub skipped: usize,
    /// Bytes written by `downloaded` files.
    pub downloaded_bytes: u64,
    /// Bytes written by `copied` files.
    pub copied_bytes: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    version: u32,
    /// Path relative to `dest`, `/`-separated → the oid it holds.
    objects: BTreeMap<String, String>,
}

/// One file under the source directory.
struct Entry {
    /// Relative to the source directory (and so to `dest`), `/`-separated.
    rel: String,
    path: PathBuf,
    pointer: Option<Pointer>,
}

/// Bring `dest` up to date with `<clone_dir>/<rel_dir>`: pointer files become
/// their LFS objects, fetched from the LFS server of `remote_url` with
/// `auth`; other files are copied as they are.
///
/// `rel_dir` is repository-relative and must be plain (no `..`, not absolute,
/// not empty). The endpoint is derived from `remote_url` alone
/// (`<remote>.git/info/lfs`); the clone's `.lfsconfig` is never read, so the
/// credential only ever goes to the config repository's own host.
///
/// Objects are asked for in batches ([`crate::lfs::batch::DEFAULT_BATCH_SIZE`]
/// per request) and transferred a few at a time. One object failing does not
/// stop the others: everything that did arrive is placed and recorded, and the
/// first failure is returned, so the next run fetches only what is missing.
/// An object whose content does not match its pointer is
/// [`SyncError::Integrity`] and nothing is written for it.
///
/// [`COMPLETE_FILE`] is removed before `dest` is first changed and on any
/// failure or cancel, and written — last — only when every file is in place.
///
/// Blocking filesystem work runs through the module's `blocking` door, so this
/// is safe to await on a multi-threaded runtime or to `block_on` from the
/// blocking pool.
pub async fn hydrate_lfs_dir(
    client: &reqwest::Client,
    clone_dir: &Path,
    rel_dir: &str,
    remote_url: &str,
    auth: &RepoAuth,
    dest: &Path,
    interrupt: &AtomicBool,
) -> Result<HydrateReport> {
    let complete = dest.join(COMPLETE_FILE);
    let outcome = hydrate(
        client, clone_dir, rel_dir, remote_url, auth, dest, interrupt,
    )
    .await;
    if outcome.is_err() {
        if let Err(err) = unmark(&complete) {
            tracing::warn!(error = %err, "the models completion marker could not be removed");
        }
    }
    outcome
}

async fn hydrate(
    client: &reqwest::Client,
    clone_dir: &Path,
    rel_dir: &str,
    remote_url: &str,
    auth: &RepoAuth,
    dest: &Path,
    interrupt: &AtomicBool,
) -> Result<HydrateReport> {
    let rel_dir = tree_path(Path::new(rel_dir))?;
    let root = contained_dir(clone_dir, &rel_dir)?;
    let (entries, set) = blocking(|| -> Result<_> {
        let entries = walk(&root)?;
        let set = set_digest(&entries)?;
        Ok((entries, set))
    })?;
    std::fs::create_dir_all(dest)
        .map_err(|err| SyncError::io("create the hydration destination", dest, err))?;

    let complete = dest.join(COMPLETE_FILE);
    let state_path = dest.join(STATE_FILE);
    let previous = read_state(&state_path);
    let mut next = State {
        version: STATE_VERSION,
        objects: BTreeMap::new(),
    };
    let mut report = HydrateReport::default();
    // oid → (size, the entries that hold it). One object may back several
    // paths; it is asked for once.
    let mut wanted: BTreeMap<&str, (u64, Vec<&Entry>)> = BTreeMap::new();

    blocking(|| -> Result<()> {
        for entry in &entries {
            if interrupt.load(Ordering::Relaxed) {
                return Err(SyncError::Cancelled);
            }
            let target = dest.join(&entry.rel);
            match &entry.pointer {
                None => {
                    if same_content(&entry.path, &target)? {
                        report.skipped += 1;
                    } else {
                        unmark(&complete)?;
                        report.copied_bytes += copy_atomic(&entry.path, &target)?;
                        report.copied += 1;
                    }
                }
                Some(pointer) => {
                    let remembered = previous.objects.get(&entry.rel).map(String::as_str);
                    if holds(remembered, pointer, &target)? {
                        report.skipped += 1;
                        next.objects.insert(entry.rel.clone(), pointer.oid.clone());
                    } else if pointer.is_empty() {
                        // The empty pointer is its own content: nothing to ask
                        // a server for.
                        unmark(&complete)?;
                        write_atomic(&target, &[])?;
                        report.downloaded += 1;
                        next.objects.insert(entry.rel.clone(), pointer.oid.clone());
                    } else {
                        wanted
                            .entry(pointer.oid.as_str())
                            .or_insert_with(|| (pointer.size, Vec::new()))
                            .1
                            .push(entry);
                    }
                }
            }
        }
        Ok(())
    })?;

    let fetched = if wanted.is_empty() {
        Ok(())
    } else {
        unmark(&complete)?;
        fetch_and_place(
            client,
            remote_url,
            auth,
            dest,
            &wanted,
            &mut next,
            &mut report,
            interrupt,
        )
        .await
    };
    // Written whatever the fetch did: what was placed before a failure is in
    // place, and the next run should not fetch or hash it again.
    write_state(&state_path, &next)?;
    fetched?;
    let marker = serde_json::to_vec_pretty(&Complete { digest: set })
        .map_err(|err| SyncError::Config(format!("cannot encode the completion marker: {err}")))?;
    write_atomic(&complete, &marker)?;
    Ok(report)
}

/// What [`COMPLETE_FILE`] holds.
#[derive(Debug, Serialize, Deserialize)]
struct Complete {
    digest: String,
}

/// Whether `dest` holds, completely, the set `<clone_dir>/<rel_dir>` names
/// now: its [`COMPLETE_FILE`] records the digest this copy's directory has
/// today. `false` when the marker is absent or unreadable, or the copy
/// cannot be walked. Blocking: it hashes every non-pointer file of the set.
pub fn hydration_is_current(clone_dir: &Path, rel_dir: &str, dest: &Path) -> bool {
    let Some(recorded) = completion_digest(dest) else {
        return false;
    };
    let current = tree_path(Path::new(rel_dir))
        .and_then(|rel| contained_dir(clone_dir, &rel))
        .and_then(|root| walk(&root))
        .and_then(|entries| set_digest(&entries));
    current.is_ok_and(|digest| digest == recorded)
}

/// The set digest the last complete hydration of `dest` recorded, or `None`
/// while `dest` is not known to be complete — what a model loader keys on,
/// so a changed set is loaded afresh.
pub fn completion_digest(dest: &Path) -> Option<String> {
    let bytes = std::fs::read(dest.join(COMPLETE_FILE)).ok()?;
    serde_json::from_slice::<Complete>(&bytes)
        .ok()
        .map(|complete| complete.digest)
}

/// sha256 over `"<path>\t<oid>\n"` for every file of the set in path order:
/// a pointer's oid, or the sha256 of a file that is not one.
fn set_digest(entries: &[Entry]) -> Result<String> {
    let mut listing = String::new();
    for entry in entries {
        let oid = match &entry.pointer {
            Some(pointer) => pointer.oid.clone(),
            None => digest(&entry.path)?,
        };
        listing.push_str(&entry.rel);
        listing.push('\t');
        listing.push_str(&oid);
        listing.push('\n');
    }
    LfsStore::digest_of(listing.as_bytes()).map(|(oid, _)| oid)
}

/// Remove [`COMPLETE_FILE`]; absent already is fine.
fn unmark(complete: &Path) -> Result<()> {
    match std::fs::remove_file(complete) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(SyncError::io(
            "remove the models completion marker",
            complete,
            err,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
async fn fetch_and_place(
    client: &reqwest::Client,
    remote_url: &str,
    auth: &RepoAuth,
    dest: &Path,
    wanted: &BTreeMap<&str, (u64, Vec<&Entry>)>,
    next: &mut State,
    report: &mut HydrateReport,
    interrupt: &AtomicBool,
) -> Result<()> {
    let endpoint = endpoint::resolve(remote_url, None, ORIGIN)?;
    let authorization = auth.lfs_authorization();

    let objects: Vec<ObjectId> = wanted
        .iter()
        .map(|(oid, (size, _))| ObjectId::new(*oid, *size))
        .collect();
    let batch = BatchClient::new(client.clone(), endpoint, authorization.clone());
    let specs = until_interrupted(batch.download(&objects), interrupt).await??;

    let store = LfsStore::new(dest.join(STORE_DIR));
    let transfer = Arc::new(BasicTransfer::new(client.clone(), store.clone()));
    let results = until_interrupted(transfer.download_all(specs, authorization), interrupt).await?;

    let mut failure = None;
    let mut answered = 0usize;
    for (oid, result) in results {
        let Some((size, targets)) = wanted.get(oid.as_str()) else {
            // Only a panicked task loses its oid; `download_all` reports it
            // as a failure of its own.
            if let Err(err) = result {
                failure.get_or_insert(err);
            }
            continue;
        };
        answered += 1;
        if let Err(err) = result {
            tracing::warn!(oid = %oid, error = %err, "config repository object not hydrated");
            failure.get_or_insert(err);
            continue;
        }
        blocking(|| place(&store, &oid, targets, dest))?;
        report.downloaded += targets.len();
        report.downloaded_bytes += size * targets.len() as u64;
        for entry in targets {
            next.objects.insert(entry.rel.clone(), oid.clone());
        }
    }
    if let Some(err) = failure {
        return Err(err);
    }
    if answered != wanted.len() {
        return Err(SyncError::Config(format!(
            "the LFS server answered for {answered} of {} objects",
            wanted.len()
        )));
    }
    Ok(())
}

/// Run `work` to completion unless `interrupt` is raised first; dropping it
/// then cancels whatever it had in flight.
async fn until_interrupted<F: Future>(work: F, interrupt: &AtomicBool) -> Result<F::Output> {
    let mut work = std::pin::pin!(work);
    loop {
        if interrupt.load(Ordering::Relaxed) {
            return Err(SyncError::Cancelled);
        }
        tokio::select! {
            out = &mut work => return Ok(out),
            () = tokio::time::sleep(INTERRUPT_POLL) => {}
        }
    }
}

/// `<clone_dir>/<rel>`, refused when it or any directory on the way to it is
/// a symbolic link: the walk must not leave the copy.
fn contained_dir(clone_dir: &Path, rel: &str) -> Result<PathBuf> {
    let mut path = clone_dir.to_path_buf();
    for part in rel.split('/') {
        path.push(part);
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|err| SyncError::io("read the directory to hydrate", &path, err))?;
        if meta.file_type().is_symlink() {
            return Err(SyncError::InvalidPathForRemote {
                path: path.clone(),
                reason: "is a symbolic link; hydration never follows one".to_owned(),
            });
        }
    }
    Ok(path)
}

/// Every regular file below `root`, sorted by relative path. Symbolic links
/// and other non-regular entries are left out, never followed; so are names
/// that are not UTF-8 and the two names this module keeps in `dest` itself.
fn walk(root: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        let listing = std::fs::read_dir(&dir)
            .map_err(|err| SyncError::io("list a directory to hydrate", &dir, err))?;
        for item in listing {
            let item =
                item.map_err(|err| SyncError::io("list a directory to hydrate", &dir, err))?;
            let path = item.path();
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                tracing::warn!(path = %path.display(), "name is not UTF-8; not hydrated");
                continue;
            };
            if prefix.is_empty() && [STATE_FILE, STORE_DIR, COMPLETE_FILE].contains(&name.as_str())
            {
                tracing::warn!(name = %name, "name is reserved in the destination; not hydrated");
                continue;
            }
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            // `DirEntry::file_type` does not follow a symbolic link.
            let kind = item
                .file_type()
                .map_err(|err| SyncError::io("inspect a file to hydrate", &path, err))?;
            if kind.is_dir() {
                pending.push((path, rel));
            } else if kind.is_file() {
                let pointer = read_pointer(&path)?;
                entries.push(Entry { rel, path, pointer });
            } else {
                tracing::warn!(path = %rel, "not a regular file or directory; not hydrated");
            }
        }
    }
    entries.sort_unstable_by(|a, b| a.rel.cmp(&b.rel));
    Ok(entries)
}

/// The pointer `path` holds, if it is one. An empty file is a plain file here:
/// copying it is already exact.
fn read_pointer(path: &Path) -> Result<Option<Pointer>> {
    let file =
        File::open(path).map_err(|err| SyncError::io("read a file to hydrate", path, err))?;
    let mut head = Vec::with_capacity(MAX_POINTER_BYTES + 1);
    file.take(MAX_POINTER_BYTES as u64 + 1)
        .read_to_end(&mut head)
        .map_err(|err| SyncError::io("read a file to hydrate", path, err))?;
    if head.is_empty() || head.len() > MAX_POINTER_BYTES {
        return Ok(None);
    }
    Ok(Pointer::parse(&head))
}

/// Whether `target` already holds `pointer`'s object. A remembered oid with
/// the right length is believed without reading the file; anything else of
/// the right length is hashed.
fn holds(remembered: Option<&str>, pointer: &Pointer, target: &Path) -> Result<bool> {
    let Ok(meta) = std::fs::symlink_metadata(target) else {
        return Ok(false);
    };
    if !meta.is_file() || meta.len() != pointer.size {
        return Ok(false);
    }
    if remembered == Some(pointer.oid.as_str()) {
        return Ok(true);
    }
    Ok(digest(target)? == pointer.oid)
}

/// Whether `target` is a regular file with exactly `source`'s bytes.
fn same_content(source: &Path, target: &Path) -> Result<bool> {
    let (Ok(theirs), Ok(ours)) = (std::fs::metadata(source), std::fs::symlink_metadata(target))
    else {
        return Ok(false);
    };
    if !ours.is_file() || ours.len() != theirs.len() {
        return Ok(false);
    }
    Ok(digest(source)? == digest(target)?)
}

fn digest(path: &Path) -> Result<String> {
    let file = File::open(path).map_err(|err| SyncError::io("hash a hydrated file", path, err))?;
    LfsStore::digest_of(file).map(|(oid, _)| oid)
}

/// Move a verified object from the store to every path that names it: copies
/// for all but the last, a rename for the last so the common single-path case
/// never copies hundreds of megabytes.
fn place(store: &LfsStore, oid: &str, targets: &[&Entry], dest: &Path) -> Result<()> {
    let object = store.object_path(oid);
    let Some((last, rest)) = targets.split_last() else {
        return Ok(());
    };
    for entry in rest {
        copy_atomic(&object, &dest.join(&entry.rel))?;
    }
    let target = dest.join(&last.rel);
    create_parent(&target)?;
    if std::fs::rename(&object, &target).is_err() {
        // A destination subtree on another filesystem.
        copy_atomic(&object, &target)?;
        std::fs::remove_file(&object)
            .map_err(|err| SyncError::io("remove a placed LFS object", &object, err))?;
    }
    Ok(())
}

fn create_parent(target: &Path) -> Result<&Path> {
    let parent = target
        .parent()
        .ok_or_else(|| SyncError::InvalidPathForRemote {
            path: target.to_path_buf(),
            reason: "has no parent directory".to_owned(),
        })?;
    std::fs::create_dir_all(parent)
        .map_err(|err| SyncError::io("create a hydrated directory", parent, err))?;
    Ok(parent)
}

/// Copy `source` to `target` through a temporary file beside it.
fn copy_atomic(source: &Path, target: &Path) -> Result<u64> {
    let parent = create_parent(target)?;
    let mut from =
        File::open(source).map_err(|err| SyncError::io("read a file to hydrate", source, err))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|err| SyncError::io("stage a hydrated file", parent, err))?;
    let copied = std::io::copy(&mut from, staged.as_file_mut())
        .map_err(|err| SyncError::io("stage a hydrated file", target, err))?;
    publish(staged, target)?;
    Ok(copied)
}

fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = create_parent(target)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|err| SyncError::io("stage a hydrated file", parent, err))?;
    std::io::Write::write_all(staged.as_file_mut(), bytes)
        .map_err(|err| SyncError::io("stage a hydrated file", target, err))?;
    publish(staged, target)
}

fn publish(staged: tempfile::NamedTempFile, target: &Path) -> Result<()> {
    staged
        .as_file()
        .sync_all()
        .map_err(|err| SyncError::io("flush a hydrated file", target, err))?;
    staged
        .persist(target)
        .map_err(|err| SyncError::io("publish a hydrated file", target, err.error))?;
    Ok(())
}

/// The state a previous run left; empty when there is none or it cannot be
/// read — which costs one hash per file, never a wrong skip.
fn read_state(path: &Path) -> State {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return State::default(),
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "hydration state unreadable");
            return State::default();
        }
    };
    match serde_json::from_slice::<State>(&bytes) {
        Ok(state) if state.version == STATE_VERSION => state,
        Ok(_) => State::default(),
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "hydration state malformed");
            State::default()
        }
    }
}

fn write_state(path: &Path, state: &State) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|err| SyncError::Config(format!("cannot encode the hydration state: {err}")))?;
    write_atomic(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_auth_becomes_the_authorization_the_lfs_client_sends() {
        assert_eq!(RepoAuth::None.lfs_authorization(), None);
        assert_eq!(
            RepoAuth::Bearer("eyJ.tok".to_owned()).lfs_authorization(),
            Some("Bearer eyJ.tok".to_owned())
        );
        assert_eq!(
            RepoAuth::Basic {
                username: "oauth2".to_owned(),
                password: "pw".to_owned(),
            }
            .lfs_authorization(),
            // base64("oauth2:pw")
            Some("Basic b2F1dGgyOnB3".to_owned())
        );
    }

    #[test]
    fn a_pointer_is_told_from_content_by_its_bytes_not_its_name() {
        let dir = tempfile::tempdir().expect("dir");
        let pointer = dir.path().join("weights.bin");
        let oid = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";
        std::fs::write(&pointer, Pointer::new(oid, 12_345).render()).expect("write");
        let text = dir.path().join("models.toml");
        std::fs::write(&text, "[asr]\ndir = \"parakeet\"\n").expect("write");
        let empty = dir.path().join("empty");
        std::fs::write(&empty, "").expect("write");
        let big = dir.path().join("big.bin");
        // A pointer's text followed by more than a pointer may hold.
        let mut bytes = Pointer::new(oid, 1).render().into_bytes();
        bytes.resize(MAX_POINTER_BYTES + 1, b'\n');
        std::fs::write(&big, bytes).expect("write");

        let found = read_pointer(&pointer).expect("read").expect("a pointer");
        assert_eq!((found.oid.as_str(), found.size), (oid, 12_345));
        assert!(read_pointer(&text).expect("read").is_none());
        assert!(read_pointer(&empty).expect("read").is_none());
        assert!(read_pointer(&big).expect("read").is_none());
    }
}
