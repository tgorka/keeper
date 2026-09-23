//! The account's configuration repository: bytes in and out, nothing more
//! (AD-312).
//!
//! A person's settings live in one small git repository shared by all their
//! devices. This module keeps a local copy of it and publishes new files into
//! it. It knows nothing of what the files mean — the shell hands in the
//! paths to write, computed by `keeper-core` against the copy this module just
//! refreshed — and it is not a sync profile: no `sync.db` row, no watcher, no
//! journal. It uses the same free functions the engine does
//! ([`git::repo`], [`git::fetch`], [`git::push_http`]), gix-only, so it runs
//! unchanged on the phone.
//!
//! # The local copy is a cache
//!
//! [`clone_or_fetch`] always leaves the working tree and `refs/heads/<branch>`
//! exactly at `origin/<branch>` — a hard reset, not a merge. Nothing is ever
//! authored in the copy by hand, and a commit this module made that did not
//! reach the remote is discarded by the next refresh and re-planned against
//! what the remote holds then. That is what makes the retry in
//! [`commit_and_push`] honest: each attempt plans against the tip it commits
//! on, so a file another device created in the meantime is seen as existing
//! and left alone.
//!
//! # Credentials
//!
//! [`RepoAuth::Basic`] goes through gix's credential callback with the
//! username and password exactly as given. [`RepoAuth::Bearer`] is an
//! in-memory `http.extraHeader` on the handle that fetches — gix's reqwest
//! transport adds every configured extra header to each request it makes —
//! and an `Authorization: Bearer` header on the push. No helper, no process
//! argument, no userinfo in a URL, nothing written to `.git/config`.
//!
//! gix refuses to send a Basic pair to a plain `http://` remote; the bearer
//! header is sent as configured. Keeping secrets off clear-text remotes is the
//! descriptor's validation (https only, loopback excepted), not this module's.

use std::{
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use gix::hash::ObjectId;

use crate::{
    error::{Result, SyncError},
    git::{
        cli,
        fetch::{self, Credential, FetchOptions, TransferProgress},
        push_http::{self, HttpAuth},
        repo,
    },
};

/// How many times [`commit_and_push`] / [`move_and_push`] plan, commit and
/// push before a remote that keeps moving is reported instead of chased.
const MAX_ATTEMPTS: usize = 3;

/// The remote every copy tracks.
const ORIGIN: &str = "origin";

/// How the repository's host is authenticated.
#[derive(Clone)]
pub enum RepoAuth {
    /// A public repository, or one the host lets anybody read.
    None,
    /// HTTP Basic with this pair, verbatim.
    Basic { username: String, password: String },
    /// `Authorization: Bearer <token>`.
    Bearer(String),
}

impl std::fmt::Debug for RepoAuth {
    /// Hand-written so the secret never reaches a log line (NFR-26).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::None => "RepoAuth::None",
            Self::Basic { .. } => "RepoAuth::Basic(<redacted>)",
            Self::Bearer(_) => "RepoAuth::Bearer(<redacted>)",
        })
    }
}

impl RepoAuth {
    /// What gix's credential callback answers with.
    fn credential(&self) -> Option<Credential> {
        match self {
            Self::Basic { username, password } => Some(Credential {
                username: username.clone(),
                secret: password.clone(),
            }),
            Self::None | Self::Bearer(_) => None,
        }
    }

    /// The header line gix's HTTP transport adds to every request.
    fn extra_header(&self) -> Option<String> {
        match self {
            Self::Bearer(token) => Some(format!("Authorization: Bearer {token}")),
            Self::None | Self::Basic { .. } => None,
        }
    }

    fn http<'a>(&'a self, credential: Option<&'a Credential>) -> Option<HttpAuth<'a>> {
        match self {
            Self::None => None,
            Self::Basic { .. } => credential.map(HttpAuth::Basic),
            Self::Bearer(token) => Some(HttpAuth::Bearer(token)),
        }
    }
}

/// Which repository, which branch, and where the local copy lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSpec {
    pub url: String,
    pub branch: String,
    /// The clone root: the working tree, with `.git` inside it.
    pub dir: PathBuf,
}

/// What a refresh found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// `origin/<branch>` as hex, which the local branch now equals; `None`
    /// for an empty remote.
    pub head: Option<String>,
    /// Whether the local branch moved.
    pub changed: bool,
    /// The remote has no `<branch>` yet; the first push creates it.
    pub empty_remote: bool,
}

/// One file to create, repository-relative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub rel: PathBuf,
    pub bytes: Vec<u8>,
}

/// Who a commit is by (author and committer alike).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    pub name: String,
    pub email: String,
}

/// What a publish did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushResult {
    /// The remote's branch is now at `head`.
    Pushed { head: String },
    /// There was nothing to write; nothing was committed or sent.
    NothingToDo,
}

/// Bring the copy at `spec.dir` to exactly `origin/<branch>`, creating it
/// first when there is none.
///
/// Blocking: gix's HTTP transport has no async path. See the module docs for
/// why this is a hard reset.
pub fn clone_or_fetch(
    spec: &RepoSpec,
    auth: &RepoAuth,
    interrupt: &AtomicBool,
) -> Result<SyncOutcome> {
    refresh(spec, auth, interrupt).map(|(_, outcome, _)| outcome)
}

/// Create the files `plan` asks for on top of the remote's tip and push them.
///
/// Each attempt refreshes the copy, calls `plan` with its working tree, and —
/// when `plan` returns anything — commits exactly those files as one commit
/// whose parent is `origin/<branch>` and pushes it. A push the remote refuses
/// as not a fast-forward (another device got there first) starts the next
/// attempt; after [`MAX_ATTEMPTS`] that refusal is returned.
///
/// Create-only: a `Write` naming a path the tip already holds is refused, as
/// is one that is absolute or climbs out with `..`, and one whose path — the
/// file or a directory on the way to it — the tip holds as a symbolic link.
/// `plan` is expected to have filtered these already; this is the second lock
/// on the door.
pub async fn commit_and_push<F>(
    client: &reqwest::Client,
    spec: &RepoSpec,
    auth: &RepoAuth,
    author: &Author,
    message: &str,
    plan: F,
    interrupt: &AtomicBool,
) -> Result<PushResult>
where
    F: Fn(&Path) -> Vec<Write> + Send + Sync,
{
    publish(
        client,
        spec,
        auth,
        author,
        message,
        interrupt,
        |repo, base, editor| {
            let mut any = false;
            for write in plan(&spec.dir) {
                let rel = tree_path(&write.rel)?;
                refuse_non_directories_on_the_way(repo, base, &rel)?;
                if let Some((mode, _)) = entry_at(repo, base, &rel)? {
                    return Err(SyncError::InvalidPathForRemote {
                        path: write.rel,
                        reason: if mode.is_link() {
                            "is a symbolic link in the repository; keeper does not write through \
                             links, and files there are never rewritten"
                        } else {
                            "already exists in the repository; files there are never rewritten"
                        }
                        .to_owned(),
                    });
                }
                let blob = repo
                    .write_blob(&write.bytes)
                    .map_err(|err| git_error("could not write a blob", &err))?;
                editor
                    .upsert(rel.as_str(), gix::object::tree::EntryKind::Blob, blob)
                    .map_err(|err| git_error("could not stage a file", &err))?;
                any = true;
            }
            Ok(any)
        },
    )
    .await
}

/// Rename files in the repository — a device's files when it is renamed — and
/// push the result, with [`commit_and_push`]'s attempts and refusals.
///
/// A move whose source is gone and whose destination exists is one another
/// attempt (or another device) already made, and is skipped; one whose
/// destination exists beside its source is refused rather than overwritten.
pub async fn move_and_push(
    client: &reqwest::Client,
    spec: &RepoSpec,
    auth: &RepoAuth,
    author: &Author,
    message: &str,
    moves: &[(PathBuf, PathBuf)],
    interrupt: &AtomicBool,
) -> Result<PushResult> {
    publish(
        client,
        spec,
        auth,
        author,
        message,
        interrupt,
        |repo, base, editor| {
            let mut any = false;
            for (from, to) in moves {
                let from_rel = tree_path(from)?;
                let to_rel = tree_path(to)?;
                refuse_non_directories_on_the_way(repo, base, &to_rel)?;
                let source = entry_at(repo, base, &from_rel)?;
                let target_exists = entry_at(repo, base, &to_rel)?.is_some();
                match (source, target_exists) {
                    (None, true) => continue,
                    (None, false) => {
                        return Err(SyncError::InvalidPathForRemote {
                            path: from.clone(),
                            reason: "is not in the repository, so it cannot be moved".to_owned(),
                        })
                    }
                    (Some(_), true) => {
                        return Err(SyncError::InvalidPathForRemote {
                            path: to.clone(),
                            reason:
                                "already exists in the repository; files there are never rewritten"
                                    .to_owned(),
                        })
                    }
                    (Some((mode, _)), false) if mode.is_tree() => {
                        return Err(SyncError::InvalidPathForRemote {
                            path: from.clone(),
                            reason: "is a directory; only files are moved".to_owned(),
                        })
                    }
                    (Some((mode, id)), false) => {
                        editor
                            .upsert(to_rel.as_str(), mode.kind(), id)
                            .and_then(|editor| editor.remove(from_rel.as_str()))
                            .map_err(|err| git_error("could not stage a move", &err))?;
                        any = true;
                    }
                }
            }
            Ok(any)
        },
    )
    .await
}

/// The attempt loop both publishers share. `edit` stages its changes into an
/// editor over the tip's tree (`base`) and says whether it staged anything.
async fn publish<E>(
    client: &reqwest::Client,
    spec: &RepoSpec,
    auth: &RepoAuth,
    author: &Author,
    message: &str,
    interrupt: &AtomicBool,
    edit: E,
) -> Result<PushResult>
where
    E: Fn(&gix::Repository, ObjectId, &mut gix::object::tree::Editor<'_>) -> Result<bool>,
{
    let credential = auth.credential();
    let mut attempt = 0;
    loop {
        attempt += 1;
        if interrupt.load(Ordering::Relaxed) {
            return Err(SyncError::Cancelled);
        }
        let committed = blocking(|| -> Result<Option<(ObjectId, Option<ObjectId>)>> {
            let (repo, _, tip) = refresh(spec, auth, interrupt)?;
            let base = match tip {
                Some(id) => commit_tree(&repo, id)?,
                None => ObjectId::empty_tree(repo.object_hash()),
            };
            let mut editor = repo
                .edit_tree(base)
                .map_err(|err| git_error("could not read the remote's tree", &err))?;
            if !edit(&repo, base, &mut editor)? {
                return Ok(None);
            }
            let tree = editor
                .write()
                .map_err(|err| git_error("could not write the tree", &err))?
                .detach();
            if tree == base {
                return Ok(None);
            }
            let commit = new_commit(&repo, author, message, tree, tip)?;
            reset_hard(&repo, &spec.branch, Some(commit), interrupt)?;
            Ok(Some((commit, tip)))
        })?;
        let Some((commit, planned_on)) = committed else {
            return Ok(PushResult::NothingToDo);
        };

        let pushed = push_http::push_with_auth(
            client,
            &spec.dir,
            &spec.url,
            &spec.branch,
            auth.http(credential.as_ref()),
        )
        .await;
        let pushed = match pushed {
            // The advertisement promised a fast-forward and the server's
            // receive-pack still refused the update (`ng … reference already
            // exists` / `failed to update ref`): when the remote's tip is no
            // longer the one this attempt planned on, another device landed
            // in between — the race the client-side guard reports as
            // `Diverged`, only later. A tip that did not move leaves the
            // refusal as it was, and so does a check that cannot be made.
            Err(SyncError::Git(reason))
                if matches!(
                    blocking(|| remote_tip(spec, auth, interrupt)),
                    Ok(now) if now != planned_on
                ) =>
            {
                Err(SyncError::Diverged {
                    profile: cli::repo_label(&spec.dir),
                    reason,
                })
            }
            other => other,
        };
        match pushed {
            Ok(_) => {
                return Ok(PushResult::Pushed {
                    head: commit.to_string(),
                })
            }
            // The remote moved between the refresh and the push. The commit is
            // discarded by the next refresh and re-planned on the new tip.
            Err(SyncError::Diverged { .. }) if attempt < MAX_ATTEMPTS => {
                tracing::info!(attempt, "config repository moved during the push; retrying");
            }
            Err(err) => return Err(err),
        }
    }
}

/// Refuse `rel` when the tree holds a directory on the way to it as a symbolic
/// link or a file. Staging it would replace that entry with a directory —
/// rewriting what the repository holds — and the working tree the planner
/// looked at resolved the link to wherever it points.
fn refuse_non_directories_on_the_way(
    repo: &gix::Repository,
    tree: ObjectId,
    rel: &str,
) -> Result<()> {
    for (end, _) in rel.match_indices('/') {
        let prefix = &rel[..end];
        let reason = match entry_at(repo, tree, prefix)? {
            Some((mode, _)) if mode.is_tree() => continue,
            // Nothing deeper exists.
            None => return Ok(()),
            Some((mode, _)) if mode.is_link() => {
                format!(
                    "is a symbolic link in the repository; keeper does not write {rel} through it"
                )
            }
            Some(_) => format!(
                "is a file in the repository, so {rel} cannot be created inside it; files there \
                 are never rewritten"
            ),
        };
        return Err(SyncError::InvalidPathForRemote {
            path: PathBuf::from(prefix),
            reason,
        });
    }
    Ok(())
}

/// Open (or create) the copy, fetch, and hard-reset it to the remote's tip.
/// Returns the handle — still carrying the in-memory auth — the outcome, and
/// the tip.
fn refresh(
    spec: &RepoSpec,
    auth: &RepoAuth,
    interrupt: &AtomicBool,
) -> Result<(gix::Repository, SyncOutcome, Option<ObjectId>)> {
    let repo = open_with_auth(spec, auth)?;
    let before = repo::head_commit_id(&repo)?;
    let tip = fetch_tip(&repo, spec, auth, interrupt)?;

    reset_hard(&repo, &spec.branch, tip, interrupt)?;
    let outcome = SyncOutcome {
        head: tip.map(|id| id.to_string()),
        changed: before != tip,
        empty_remote: tip.is_none(),
    };
    Ok((repo, outcome, tip))
}

/// The remote's tip as a fresh fetch finds it; the working tree is untouched.
fn remote_tip(
    spec: &RepoSpec,
    auth: &RepoAuth,
    interrupt: &AtomicBool,
) -> Result<Option<ObjectId>> {
    fetch_tip(&open_with_auth(spec, auth)?, spec, auth, interrupt)
}

/// [`open_or_create`], carrying `auth`'s header in memory.
fn open_with_auth(spec: &RepoSpec, auth: &RepoAuth) -> Result<gix::Repository> {
    let mut repo = open_or_create(spec)?;
    if repo.committer().is_none() {
        with_fallback_committer(&mut repo)?;
    }
    if let Some(header) = auth.extra_header() {
        with_extra_header(&mut repo, &header)?;
    }
    Ok(repo)
}

/// Fetch `origin` and answer where `origin/<branch>` now is.
fn fetch_tip(
    repo: &gix::Repository,
    spec: &RepoSpec,
    auth: &RepoAuth,
    interrupt: &AtomicBool,
) -> Result<Option<ObjectId>> {
    let progress: TransferProgress = Arc::new(|_, _| {});
    fetch::fetch(
        repo,
        ORIGIN,
        &FetchOptions::default(),
        auth.credential().as_ref(),
        &progress,
        interrupt,
    )?;
    repo::resolve_reference(repo, &format!("refs/remotes/{ORIGIN}/{}", spec.branch))
}

/// The copy, opened for a fetch; created around `spec.dir` when absent.
fn open_or_create(spec: &RepoSpec) -> Result<gix::Repository> {
    let git_dir = spec.dir.join(".git");
    if git_dir.exists() {
        match repo::open_for_fetch(&spec.dir, false) {
            Ok(repo) => {
                repo::ensure_remote(&repo, &spec.url)?;
                return Ok(repo);
            }
            // A `.git` a killed first run left half-made; anything with history
            // in it is refused by `discard_unfinished_init` and the error stands.
            Err(err) => {
                if !repo::discard_unfinished_init(&git_dir)? {
                    return Err(err);
                }
            }
        }
    }
    std::fs::create_dir_all(&spec.dir)
        .map_err(|err| SyncError::io("create the config repository folder", &spec.dir, err))?;
    repo::adopt(&spec.dir, &spec.url, &spec.branch)?;
    repo::open_for_fetch(&spec.dir, false)
}

/// Give this handle a committer when no configuration supplies one.
///
/// Every fetch moves `origin/<branch>` and every reset moves the branch, and
/// gix writes a reflog entry for each — refusing without a committer
/// ("reflog messages need a committer which isn't set"). An iPhone has no git
/// identity at all, and neither does a Mac or a CI runner where nobody ran
/// `git config --global user.email`. The identity is the one managed drives
/// fall back to ([`repo::FALLBACK_IDENTITY_NAME`]); it reaches only reflogs,
/// because [`commit_and_push`] signs its commits with the person's [`Author`].
/// In memory, like the request header: the copy's `.git/config` stays as
/// `adopt` wrote it.
fn with_fallback_committer(repo: &mut gix::Repository) -> Result<()> {
    let what = "could not configure a committer";
    let mut identity = gix::config::File::new(gix::config::file::Metadata::api());
    let mut user = identity
        .section_mut_or_create_new("user", None)
        .map_err(|err| git_error(what, &err))?;
    user.push("name", repo::FALLBACK_IDENTITY_NAME)
        .map_err(|err| git_error(what, &err))?;
    user.push("email", repo::FALLBACK_IDENTITY_EMAIL)
        .map_err(|err| git_error(what, &err))?;
    drop(user);
    let mut snapshot = repo.config_snapshot_mut();
    snapshot
        .append(identity)
        .map_err(|err| git_error(what, &err))?;
    snapshot.commit().map_err(|err| git_error(what, &err))?;
    Ok(())
}

/// Add `header` as an `http.extraHeader` to this handle's configuration only.
///
/// Appended as its own API-sourced (fully trusted) section rather than set
/// through a key override: gix reads `http.extraHeader` through the trust
/// filter, and an override comment would carry the secret.
fn with_extra_header(repo: &mut gix::Repository, header: &str) -> Result<()> {
    let mut extra = gix::config::File::new(gix::config::file::Metadata::api());
    extra
        .section_mut_or_create_new("http", None)
        .map_err(|err| git_error("could not configure the request header", &err))?
        .push("extraHeader", header)
        .map_err(|err| git_error("could not configure the request header", &err))?;
    let mut snapshot = repo.config_snapshot_mut();
    snapshot
        .append(extra)
        .map_err(|err| git_error("could not configure the request header", &err))?;
    snapshot
        .commit()
        .map_err(|err| git_error("could not configure the request header", &err))?;
    Ok(())
}

/// Make the index, the working tree and `refs/heads/<branch>` equal `target`
/// — or empty and unborn for `None` — whatever they held before. Untracked
/// files the target does not name are left, as `git reset --hard` leaves them.
fn reset_hard(
    repo: &gix::Repository,
    branch: &str,
    target: Option<ObjectId>,
    interrupt: &AtomicBool,
) -> Result<()> {
    let workdir = repo::workdir(repo)?;
    let branch_ref = gix::refs::FullName::try_from(format!("refs/heads/{branch}"))
        .map_err(|err| SyncError::Config(format!("invalid branch name {branch:?}: {err}")))?;
    let tree = match target {
        Some(id) => commit_tree(repo, id)?,
        None => ObjectId::empty_tree(repo.object_hash()),
    };
    let current = repo
        .index_or_empty()
        .map_err(|err| SyncError::Git(format!("could not read the index: {err}")))?;
    let mut next = repo
        .index_from_tree(&tree)
        .map_err(|err| git_error("could not build the index for the remote's tip", &err))?;

    for (rela, ()) in current.entries_with_paths_by_filter_map(|rela, _| {
        next.entry_by_path(rela).is_none().then_some(())
    }) {
        let rela = gix::path::from_bstr(rela);
        // A directory on the way that is now a link is the new tip's (or a
        // half-finished checkout's) and points anywhere: the file the old index
        // names is not behind it, so the link itself goes and checkout puts back
        // whatever the target holds there.
        let path = linked_ancestor(&workdir, &rela).unwrap_or_else(|| workdir.join(&rela));
        match std::fs::remove_file(&path) {
            Ok(()) => prune_empty_parents(&workdir, &path),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(SyncError::io(
                    "remove a file the remote no longer has",
                    path,
                    err,
                ))
            }
        }
    }

    let mut options = repo
        .checkout_options(gix::worktree::stack::state::attributes::Source::IdMapping)
        .map_err(|err| SyncError::Git(format!("could not read checkout options: {err}")))?;
    options.destination_is_initially_empty = false;
    options.overwrite_existing = true;
    options.keep_going = false;
    let objects = repo
        .objects
        .clone()
        .into_arc()
        .map_err(|err| SyncError::io("open the object database", repo.git_dir(), err))?;
    let outcome = gix::worktree::state::checkout(
        &mut next,
        &workdir,
        objects,
        &gix::progress::Discard,
        &gix::progress::Discard,
        interrupt,
        options,
    )
    .map_err(|err| {
        if interrupt.load(Ordering::Relaxed) {
            SyncError::Cancelled
        } else {
            git_error("could not check out the remote's tip", &err)
        }
    })?;
    if interrupt.load(Ordering::Relaxed) {
        return Err(SyncError::Cancelled);
    }
    if let Some(first) = outcome.errors.first() {
        return Err(SyncError::Git(format!(
            "{} path(s) could not be written (first: {}: {})",
            outcome.errors.len(),
            first.path,
            first.error
        )));
    }
    if let Some(first) = outcome.collisions.first() {
        return Err(SyncError::Git(format!(
            "{} path(s) are blocked by something else on disk (first: {})",
            outcome.collisions.len(),
            first.path
        )));
    }
    next.write(gix::index::write::Options::default())
        .map_err(|err| SyncError::Git(format!("could not write the index: {err}")))?;

    // HEAD names the branch even while it is unborn, so the first commit on an
    // empty remote lands there.
    let head_is_branch = repo
        .head_name()
        .map_err(|err| SyncError::Git(format!("could not read HEAD: {err}")))?
        .is_some_and(|name| name == branch_ref);
    let mut edits = Vec::with_capacity(2);
    if !head_is_branch {
        edits.push(gix::refs::transaction::RefEdit {
            change: gix::refs::transaction::Change::Update {
                log: gix::refs::transaction::LogChange::default(),
                expected: gix::refs::transaction::PreviousValue::Any,
                new: gix::refs::Target::Symbolic(branch_ref.clone()),
            },
            name: gix::refs::FullName::try_from("HEAD")
                .map_err(|err| SyncError::Git(format!("HEAD is not a valid ref name: {err}")))?,
            deref: false,
        });
    }
    let existing = repo::resolve_reference(repo, branch_ref.as_bstr().to_string().as_str())?;
    match target {
        Some(id) if existing != Some(id) => edits.push(gix::refs::transaction::RefEdit {
            change: gix::refs::transaction::Change::Update {
                log: gix::refs::transaction::LogChange {
                    mode: gix::refs::transaction::RefLog::AndReference,
                    force_create_reflog: false,
                    message: "keeper: reset to the remote".into(),
                },
                expected: gix::refs::transaction::PreviousValue::Any,
                new: gix::refs::Target::Object(id),
            },
            name: branch_ref,
            deref: false,
        }),
        None if existing.is_some() => edits.push(gix::refs::transaction::RefEdit {
            change: gix::refs::transaction::Change::Delete {
                expected: gix::refs::transaction::PreviousValue::Any,
                log: gix::refs::transaction::RefLog::AndReference,
            },
            name: branch_ref,
            deref: false,
        }),
        _ => {}
    }
    if !edits.is_empty() {
        repo.edit_references(edits)
            .map_err(|err| git_error("could not move the branch", &err))?;
    }
    Ok(())
}

/// Remove directories left empty by a deletion, up to (not including) `root`.
fn prune_empty_parents(root: &Path, removed: &Path) {
    let mut dir = removed.parent();
    while let Some(current) = dir {
        if current == root || std::fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}

/// The first directory between `root` and `rel`'s file that is a symbolic
/// link, if any. A component that is not there ends the walk: nothing below it
/// exists to be removed.
fn linked_ancestor(root: &Path, rel: &Path) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    let mut components = rel.components().peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            break;
        }
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return Some(path),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

/// Write a commit of `tree` on `parent` by `author`; references untouched.
fn new_commit(
    repo: &gix::Repository,
    author: &Author,
    message: &str,
    tree: ObjectId,
    parent: Option<ObjectId>,
) -> Result<ObjectId> {
    // `<`, `>` and line breaks would make the actor line unparseable.
    let clean = |value: &str| -> String {
        value
            .chars()
            .filter(|c| !matches!(c, '<' | '>' | '\n' | '\r'))
            .collect::<String>()
            .trim()
            .to_owned()
    };
    let signature = gix::actor::Signature {
        name: clean(&author.name).into(),
        email: clean(&author.email).into(),
        time: gix::date::Time::now_local_or_utc(),
    };
    let mut time_buf = gix::date::parse::TimeBuf::default();
    let signature = signature.to_ref(&mut time_buf);
    Ok(repo
        .new_commit_as(signature, signature, message, tree, parent)
        .map_err(|err| git_error("commit failed", &err))?
        .id)
}

/// The entry at `rel` in `tree`, read from the object database — the tree
/// editor only sees subtrees it has already loaded.
fn entry_at(
    repo: &gix::Repository,
    tree: ObjectId,
    rel: &str,
) -> Result<Option<(gix::object::tree::EntryMode, ObjectId)>> {
    Ok(repo
        .find_tree(tree)
        .map_err(|err| git_error("could not read the remote's tree", &err))?
        .lookup_entry_by_path(rel)
        .map_err(|err| git_error("could not read the remote's tree", &err))?
        .map(|entry| (entry.mode(), entry.object_id())))
}

/// The tree `commit` records.
fn commit_tree(repo: &gix::Repository, commit: ObjectId) -> Result<ObjectId> {
    Ok(repo
        .find_commit(commit)
        .map_err(|err| git_error("could not read the remote's tip", &err))?
        .tree_id()
        .map_err(|err| git_error("could not read the remote's tree", &err))?
        .detach())
}

/// `rel` as a slash-separated tree path, refusing anything that is not a plain
/// relative path: absolute, `..`, `.`, a drive prefix, empty, or not UTF-8.
fn tree_path(rel: &Path) -> Result<String> {
    let refuse = |reason: &str| SyncError::InvalidPathForRemote {
        path: rel.to_path_buf(),
        reason: reason.to_owned(),
    };
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_str().ok_or_else(|| refuse("is not valid UTF-8"))?)
            }
            Component::ParentDir => return Err(refuse("must not climb out of the repository")),
            Component::RootDir | Component::Prefix(_) => {
                return Err(refuse("must be relative to the repository"))
            }
            Component::CurDir => return Err(refuse("must not contain `.`")),
        }
    }
    if parts.is_empty() {
        return Err(refuse("is empty"));
    }
    Ok(parts.join("/"))
}

fn git_error(what: &str, err: &dyn std::error::Error) -> SyncError {
    SyncError::Git(format!("{what}: {}", fetch::flatten(err)))
}

/// Run blocking git work from async code without stalling a multi-threaded
/// runtime's other tasks; inline anywhere `block_in_place` would panic.
fn blocking<T>(work: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current().map(|handle| handle.runtime_flavor()) {
        Ok(tokio::runtime::RuntimeFlavor::MultiThread) => tokio::task::block_in_place(work),
        _ => work(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_relative_paths_become_tree_paths() {
        assert_eq!(
            tree_path(Path::new("alice/devices/mac.toml")).expect("plain"),
            "alice/devices/mac.toml"
        );
        for bad in [
            "../alice/keeper.toml",
            "alice/../../etc",
            "/etc/passwd",
            "./alice",
            "",
        ] {
            assert!(
                matches!(
                    tree_path(Path::new(bad)),
                    Err(SyncError::InvalidPathForRemote { .. })
                ),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn the_bearer_header_is_the_token_verbatim_and_basic_carries_no_header() {
        let bearer = RepoAuth::Bearer("eyJ.tok".to_owned());
        assert_eq!(
            bearer.extra_header().as_deref(),
            Some("Authorization: Bearer eyJ.tok")
        );
        assert!(bearer.credential().is_none());

        let basic = RepoAuth::Basic {
            username: "oauth2".to_owned(),
            password: "eyJ.tok".to_owned(),
        };
        assert_eq!(basic.extra_header(), None);
        let credential = basic.credential().expect("basic answers the callback");
        assert_eq!(
            (credential.username.as_str(), credential.secret.as_str()),
            ("oauth2", "eyJ.tok")
        );
        assert!(!format!("{basic:?}{bearer:?}").contains("eyJ"));
    }
}
