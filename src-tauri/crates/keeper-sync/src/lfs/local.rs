//! LFS objects over a filesystem remote (Story 34.18, AD-46, AD-48).
//!
//! # Why a second transport exists
//!
//! [`super::batch`] and [`super::basic`] speak to an LFS *server*. A remote that
//! is a filesystem path — a bare repository on a pendrive, an SMB mount, another
//! directory on the same disk — has no server and never will, so
//! [`super::endpoint::derive`] refuses it outright: "remote is a local path and
//! has no LFS server".
//!
//! That refusal was correct and, on its own, silently destructive. `git push`
//! copies objects into a local bare repository perfectly well, so a profile
//! synced to a pendrive published the *pointer* while its LFS object stayed in
//! the source machine's `.git/lfs`. `git ls-tree` on the remote then showed the
//! file present, which is exactly why it went unnoticed: what was present was
//! ~130 bytes of text naming content the pendrive did not have. Plug that
//! pendrive into another machine and the file is a stub.
//!
//! A pendrive is not a marginal case here — AD-48 makes removable media a
//! first-class profile, and "the video is too big for git" is the whole reason
//! LFS is in this engine. So the transport a filesystem remote needs is the one
//! thing it was missing, and it is a file copy:
//!
//! ```text
//! <worktree>/.git/lfs/objects/aa/bb/<oid>   <->   <remote>/lfs/objects/aa/bb/<oid>
//! ```
//!
//! This is what upstream git-lfs calls a *standalone transfer agent*
//! (`lfs.standalonetransferagent`), which exists for precisely this topology.
//! Both sides use the client layout, because both sides *are* clients — the
//! server-side sharding Forgejo uses does not enter into it (see
//! [`super::store::LfsStore::object_path`]).
//!
//! # What it is not
//!
//! Not a fallback for a failed HTTP transfer. A remote is either a filesystem
//! path or a URL, and the two never overlap, so this module and the batch client
//! are alternatives rather than a retry chain. An `ssh://` remote is a URL: it
//! goes through [`super::ssh`], not here, even when the host is this machine.

use std::path::{Path, PathBuf};

use crate::error::{Result, SyncError};
use crate::lfs::store::LfsStore;

/// The object store belonging to a filesystem remote, or `None` when the remote
/// is a URL.
///
/// `None` is the ordinary answer and never an error: it means "this remote has a
/// real LFS server, go and talk to it".
pub fn remote_store(remote_url: &str) -> Option<LfsStore> {
    let path = remote_path(remote_url)?;
    // A bare repository IS its own git directory, which is what a git remote
    // almost always is; a non-bare one keeps it in `.git`. Checking rather than
    // assuming matters because guessing wrong creates a second store that never
    // shares a byte with the one git itself would use.
    let git_dir = if path.join(".git").is_dir() {
        path.join(".git")
    } else {
        path
    };
    Some(LfsStore::in_git_dir(git_dir))
}

/// The filesystem path a remote names, if it names one.
///
/// The two guards are [`super::endpoint`]'s, for the same reasons: a `/` before
/// the colon means the colon is part of a path, and a single-character authority
/// is a Windows drive letter rather than a host.
fn remote_path(remote_url: &str) -> Option<PathBuf> {
    let trimmed = remote_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((scheme, rest)) = trimmed.split_once("://") {
        // `file://host/path` is legal and the host is empty or `localhost`;
        // anything else is a URL for somebody else to dial.
        if scheme != "file" {
            return None;
        }
        let idx = rest.find('/')?;
        let path = &rest[idx..];
        return Some(PathBuf::from(path));
    }
    match trimmed.split_once(':') {
        // scp-style `[user@]host:path` — a network remote.
        Some((authority, _)) if !authority.contains('/') && authority.len() > 1 => None,
        // Everything else is a path, including `C:/repos/thing.git`.
        _ => Some(PathBuf::from(trimmed)),
    }
}

/// Copy one object from `from` to `to`, verifying it as it lands.
///
/// Streaming and bounded: [`LfsStore::insert_verified`] hashes in the same pass
/// it writes and publishes by rename only once both the digest and the length
/// match, so a pendrive pulled mid-copy leaves nothing that could later be
/// mistaken for the object. Nothing here sizes a buffer from the object, which
/// is NFR-23's rule and the reason a 6 GB file is a 6 GB file rather than a 6 GB
/// allocation.
///
/// Blocking, and long-running by nature. Async callers MUST run it on
/// `tokio::task::spawn_blocking`, the same rule the rest of
/// [`super::store`] carries.
///
/// Returns the bytes moved, so the caller can report them as network traffic —
/// which, for a pendrive or an SMB mount, is exactly what they are.
pub fn transfer(from: &LfsStore, to: &LfsStore, oid: &str, size: u64) -> Result<u64> {
    if to.contains(oid, size) {
        // Already there. Not an error and not a no-op worth logging: an upload
        // re-driven from the journal after a crash lands here every time.
        return Ok(0);
    }
    let source = from.object_path(oid);
    let file = std::fs::File::open(&source).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            // Worth its own message: the store not having the object is a
            // different problem from the copy failing, and on the download side
            // it means the remote never received it — which is the very failure
            // this module exists to prevent, seen from the other end.
            return SyncError::Integrity {
                subject: format!("lfs object {oid}"),
                expected: format!("{size} bytes at {}", source.display()),
                actual: "absent".to_owned(),
            };
        }
        SyncError::io("open lfs object to copy", &source, err)
    })?;
    to.insert_verified(oid, file, size)?;
    Ok(size)
}

/// Where an object would live under `remote`, for a log line or an error.
pub fn describe(store: &LfsStore, oid: &str) -> PathBuf {
    store.object_path(oid)
}

/// Is this path reachable right now?
///
/// A removable volume that is not mounted is [`SyncError::MediaAbsent`], not a
/// failure — AD-48's whole point — and the caller must be able to tell that
/// apart from a copy that went wrong.
pub fn is_reachable(store: &LfsStore) -> bool {
    // The `lfs` directory itself may legitimately not exist yet on a remote that
    // has never received an object, so the question is asked of its parent: the
    // git directory, which a `git push` to this remote has certainly created.
    store
        .root()
        .parent()
        .map(Path::is_dir)
        .unwrap_or_else(|| store.root().is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_remote_has_no_local_store_and_a_path_does() {
        for url in [
            "https://git.example/o/r.git",
            "http://git.example/o/r.git",
            "ssh://git@git.example/o/r.git",
            "git://git.example/o/r.git",
            // scp-style is a network remote, not a path with a colon in it.
            "git@git.example:o/r.git",
        ] {
            assert!(
                remote_store(url).is_none(),
                "{url} has a real LFS server to talk to"
            );
        }

        for (url, expected) in [
            ("/srv/git/r.git", "/srv/git/r.git/lfs"),
            ("file:///srv/git/r.git", "/srv/git/r.git/lfs"),
            // A drive letter is a path, which is the case the scp-style guard
            // exists to protect.
            ("C:/repos/r.git", "C:/repos/r.git/lfs"),
        ] {
            let store = remote_store(url).unwrap_or_else(|| panic!("{url} is a path"));
            assert_eq!(store.root(), Path::new(expected), "{url}");
        }
    }

    #[test]
    fn a_non_bare_remote_keeps_its_objects_under_dot_git() {
        // Both layouts occur in the wild, and picking the wrong one creates a
        // second store that never shares a byte with git's own.
        let dir = tempfile::tempdir().expect("tempdir");
        let bare = dir.path().join("bare.git");
        std::fs::create_dir_all(&bare).expect("bare");
        assert_eq!(
            remote_store(bare.to_str().expect("utf-8"))
                .expect("store")
                .root(),
            bare.join("lfs")
        );

        let checkout = dir.path().join("checkout");
        std::fs::create_dir_all(checkout.join(".git")).expect("worktree");
        assert_eq!(
            remote_store(checkout.to_str().expect("utf-8"))
                .expect("store")
                .root(),
            checkout.join(".git").join("lfs")
        );
    }

    #[test]
    fn an_object_copied_between_stores_arrives_whole_and_verified() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = LfsStore::in_git_dir(dir.path().join("work/.git"));
        let target = LfsStore::in_git_dir(dir.path().join("remote.git"));

        // Big enough to cross the chunked-copy boundary several times, so this
        // measures the streaming loop rather than a single read.
        let payload = vec![7u8; 400_000];
        let (oid, size) = source
            .insert_streaming(payload.as_slice())
            .expect("stage the object");
        assert_eq!(size, payload.len() as u64);

        assert_eq!(
            transfer(&source, &target, &oid, size).expect("copy"),
            size,
            "the bytes moved are reported, because on a pendrive they are traffic"
        );
        assert!(
            target.contains(&oid, size),
            "the object is on the remote — the actual content, not a pointer to it"
        );
        assert_eq!(
            std::fs::read(target.object_path(&oid)).expect("read back"),
            payload
        );

        // Idempotent, which the journal relies on: an upload re-driven after a
        // crash must not fail because it already succeeded.
        assert_eq!(
            transfer(&source, &target, &oid, size).expect("second copy"),
            0,
            "already present, so nothing moved"
        );
    }

    #[test]
    fn a_missing_source_object_is_named_rather_than_reported_as_an_io_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = LfsStore::in_git_dir(dir.path().join("work/.git"));
        let target = LfsStore::in_git_dir(dir.path().join("remote.git"));
        let err = transfer(&source, &target, &"a".repeat(64), 10).expect_err("nothing to copy");
        assert!(
            matches!(err, SyncError::Integrity { .. }),
            "an absent object is a fact about content, not a filesystem mishap: {err:?}"
        );
    }

    #[test]
    fn a_truncated_source_never_becomes_a_published_object() {
        // The half of `insert_verified` that matters here: a pendrive yanked
        // mid-write, or a partially restored backup, must not leave something
        // that a later `contains` accepts as the object.
        let dir = tempfile::tempdir().expect("tempdir");
        let source = LfsStore::in_git_dir(dir.path().join("work/.git"));
        let target = LfsStore::in_git_dir(dir.path().join("remote.git"));
        let (oid, size) = source
            .insert_streaming(&b"the whole thing"[..])
            .expect("stage");

        // Claim it is longer than it is, as a pointer for truncated content would.
        let err = transfer(&source, &target, &oid, size + 100).expect_err("length mismatch");
        assert!(matches!(err, SyncError::Integrity { .. }), "got {err:?}");
        assert!(
            !target.contains(&oid, size),
            "nothing is published, so a retry starts from zero rather than from \
             poisoned bytes"
        );
    }
}
