//! Repository lifecycle: open, clone, config enforcement, status
//! (Story 24.1, AD-41 / AD-47 / AD-48).
//!
//! Everything here is gitoxide. Two of these functions exist purely to close a
//! silent-failure mode that would otherwise corrupt a user's data, and both are
//! worth reading before touching:
//!
//! * [`open`] can be asked for `Trust::Full`. gitoxide derives trust from
//!   **directory ownership**, and a repository written by another machine — the
//!   normal case on a pendrive (AD-48) — comes back as `Trust::Reduced`. Under
//!   reduced trust gix **silently drops every repo-local `filter.*` driver**,
//!   with no error and no warning, so the LFS clean filter simply never runs
//!   and a multi-gigabyte file is committed raw into the object database
//!   (AD-46).
//! * [`enforce_local_config`] forces `index.sparse=false`. `gix::status`
//!   **hard-fails** with `TreeIndexDiff(IsSparse)` on a true sparse index, so a
//!   repository that plain `git` created with `index.sparse=true` is unusable
//!   to this engine until the key is set. A non-sparse index carrying
//!   `SKIP_WORKTREE` flags is fully understood, which is why AD-47 pairs cone
//!   sparse-checkout with a non-sparse index rather than avoiding sparsity.

use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime},
};

use crate::error::{Result, SyncError};
use crate::git::fetch::Credential;

/// How long an `index.lock` must sit untouched before it is debris.
///
/// git writes an index in well under a second, and keeper's own staging of a
/// six-figure file count still finishes far inside this window. The threshold
/// is not tuned for speed — it is deliberately far longer than any real write,
/// so the only locks it ever removes are ones nobody is holding.
const STALE_INDEX_LOCK: Duration = Duration::from_secs(60);

/// Open a managed repository.
///
/// Pass `trust_full` for a repository the engine put there itself — including
/// one on removable media owned by another uid. See the module docs: without it
/// gitoxide discards repo-local filter configuration without saying so.
pub fn open(path: &Path, trust_full: bool) -> Result<gix::Repository> {
    let mut options = gix::open::Options::default();
    if trust_full {
        options = options.with(gix::sec::Trust::Full);
    }
    let repo = gix::open_opts(path, options)
        .map_err(|err| SyncError::Git(format!("open failed: {err}")))?;
    release_stale_index_lock(repo.git_dir());
    Ok(repo)
}

/// Remove an `index.lock` left behind by a process that was killed.
///
/// A `SIGKILL` during an index write — a crash, an OOM kill, a machine losing
/// power — leaves the lock on disk with nobody holding it. git then refuses
/// every subsequent index write, so without this the profile never syncs again
/// and the cure is a human finding and deleting a file they have no reason to
/// know exists. That is exactly the unattended-recovery failure NFR-24 forbids.
///
/// A *fresh* lock is left strictly alone: it most likely belongs to a `git`
/// command the user is running by hand in the same folder, and stealing it
/// would corrupt their index to fix a problem that does not exist. The caller
/// sees the ordinary lock error instead, which is transient and retried.
///
/// Best-effort by design. If the removal races another process, or the clock
/// went backwards, the sync continues and fails on its own terms rather than
/// turning a cleanup into a hard error.
fn release_stale_index_lock(git_dir: &Path) {
    release_index_lock_older_than(git_dir, STALE_INDEX_LOCK);
}

/// The rule itself, with the threshold passed in so a test can exercise both
/// sides of the decision without manipulating file timestamps.
fn release_index_lock_older_than(git_dir: &Path, threshold: Duration) {
    let lock = git_dir.join("index.lock");
    let Ok(metadata) = std::fs::metadata(&lock) else {
        return;
    };
    if !metadata.is_file() {
        return;
    }
    // A modified time in the future means the clock moved, not that the lock is
    // new. Leaving it alone is the conservative reading.
    let Some(age) = metadata
        .modified()
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok())
    else {
        return;
    };
    if age < threshold {
        return;
    }
    match std::fs::remove_file(&lock) {
        // Logged, never silent: a repository that keeps needing this is a
        // machine that keeps dying mid-write, and that is worth seeing.
        Ok(()) => tracing::warn!(
            lock = %lock.display(),
            age_s = age.as_secs(),
            "released an index lock left behind by a killed run"
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(
            lock = %lock.display(),
            error = %err,
            "could not release a stale index lock"
        ),
    }
}

/// Does this clone failure mean the remote simply has no commits yet?
///
/// Pointing keeper at a repository that was just created in the forge is one
/// of the most ordinary things a user can do, and gitoxide reports it the same
/// way it reports a genuinely missing branch: a fetch that matched no ref.
/// There is no typed variant to match on, so the message is classified here,
/// once, next to the call that produces it — rather than letting a string
/// comparison leak into the engine.
///
/// Deliberately narrow. An auth failure, an unreachable host or a bad URL must
/// keep surfacing as itself; treating those as "empty remote" would silently
/// create an unrelated local history and push it somewhere unintended.
pub fn is_empty_remote(err: &SyncError) -> bool {
    let SyncError::Git(message) = err else {
        return false;
    };
    message.contains("didn't have any ref that matched")
        || message.contains("did not have any ref that matched")
}

/// Clone `url` into `dest` on `branch`, authenticating with `credential`.
///
/// `index.sparse=false` is applied as an in-memory override for the clone
/// itself; [`enforce_local_config`] must still be called afterwards to make it
/// durable, because the override does not reach `.git/config`.
///
/// `credential` is threaded through the same static callback [`super::fetch`]
/// uses, and `credential.helper` is cleared for the duration. Both halves
/// matter. Without the callback gitoxide falls back to the OS credential helper
/// — which is not merely "unauthenticated", it is *authenticated as somebody
/// else*: whatever account the system git store happens to hold for that host,
/// regardless of which profile is syncing. That reads as a per-repository
/// failure (the stored account has access to one repo but not another) and
/// reports itself as a transport error that never mentions credentials.
/// Clearing the helper chain is what makes the profile's own secret the only
/// answer, so a profile that has no credential fails as unauthenticated instead
/// of silently borrowing one.
///
/// Blocking: gitoxide's HTTP transport has no async path, so callers on a tokio
/// runtime must wrap this in `spawn_blocking`.
pub fn clone(
    url: &str,
    dest: &Path,
    branch: &str,
    shallow_depth: Option<NonZeroU32>,
    credential: Option<&Credential>,
    interrupt: &AtomicBool,
) -> Result<gix::Repository> {
    let mut prepare = gix::prepare_clone(url, dest)
        .map_err(|err| SyncError::Config(format!("invalid remote URL: {err}")))?
        .with_in_memory_config_overrides(["index.sparse=false", "credential.helper="])
        .with_ref_name(Some(branch))
        .map_err(|err| SyncError::Config(format!("invalid branch name {branch:?}: {err}")))?;

    if let Some(depth) = shallow_depth {
        prepare = prepare.with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(depth));
    }

    if let Some(credential) = credential {
        // Cloned per connection rather than moved: `configure_connection` takes
        // an `FnMut` (gitoxide may reconnect, e.g. across a redirect) while the
        // callback it installs has to own its strings.
        let username = credential.username.clone();
        let secret = credential.secret.clone();
        prepare = prepare.configure_connection(move |connection| {
            let username = username.clone();
            let secret = secret.clone();
            // The closure's return type is gix's, and its 192-byte `Err` lives
            // in `gix_credentials::protocol::Error` — a foreign type we can
            // neither box nor shrink, and the callback signature is not ours to
            // change. Same allow, same reason, as in `super::fetch`.
            #[allow(clippy::result_large_err)]
            connection.set_credentials(move |action| {
                super::fetch::static_credential(&username, &secret, action)
            });
            Ok(())
        });
    }

    let host = host_of(url);
    let (mut checkout, _outcome) = prepare
        .fetch_then_checkout(gix::progress::Discard, interrupt)
        .map_err(|err| {
            super::fetch::classify("clone", &super::fetch::flatten(&err), &host, interrupt)
        })?;

    let (repo, _outcome) = checkout
        .main_worktree(gix::progress::Discard, interrupt)
        .map_err(|err| {
            cancelled_or(interrupt, || {
                SyncError::Git(format!("checkout failed: {err}"))
            })
        })?;

    enforce_local_config(&repo)?;
    Ok(repo)
}

/// The identity written into a managed repository that has none.
///
/// Deliberately not a person: it appears only in reflogs on hosts where no git
/// identity was ever configured, and claiming to be a human there would be a
/// lie. Commits keeper makes carry the real device signature instead.
const IDENTITY_NAME: &str = "keeper";
/// `.invalid` is reserved by RFC 2606 and can never be a deliverable address,
/// which is the honest way to say "no mailbox".
const IDENTITY_EMAIL: &str = "keeper@keeper.invalid";

/// Write `index.sparse=false` into the repository's own `.git/config`.
///
/// Mandatory for every managed repository — see the module docs for the
/// hard-failure this prevents. Idempotent: an existing value is overwritten
/// rather than appended, so repeated calls do not grow the file.
pub fn enforce_local_config(repo: &gix::Repository) -> Result<()> {
    enforce_local_config_with_filter(repo, None)
}

/// As [`enforce_local_config`], additionally registering keeper as the `lfs`
/// clean/smudge filter when `filter_program` is given.
///
/// Registering the filter is what lets a human use plain `git` inside a synced
/// folder. Without it, the blob is a pointer while the worktree holds the real
/// bytes, so the moment git re-reads content — which it does whenever its stat
/// cache misses — it reports every LFS-tracked file as modified. keeper itself
/// tolerates that, but nobody running `git status` by hand should have to.
///
/// Single-invocation `clean`/`smudge` rather than the long-running
/// `filter.lfs.process` protocol: both git and gitoxide support it, it is far
/// less machinery, and the per-call cost only lands when git's stat cache
/// already missed.
///
/// `required` is deliberately left false. A worktree whose keeper binary has
/// moved must still be checkout-able — it would just get pointers, which is
/// recoverable, where a required filter would hard-fail every git command.
pub fn enforce_local_config_with_filter(
    repo: &gix::Repository,
    filter_program: Option<&Path>,
) -> Result<()> {
    let path = repo.git_dir().join("config");
    let mut config =
        gix::config::File::from_path_no_includes(path.clone(), gix::config::Source::Local)
            .map_err(|err| SyncError::Git(format!("could not read {}: {err}", path.display())))?;
    config
        .set_raw_value("index.sparse", "false")
        .map_err(|err| SyncError::Git(format!("could not set index.sparse: {err}")))?;

    // A fetch writes a reflog entry for the remote-tracking ref it moves, and
    // gitoxide refuses to write one without a committer identity. On a host
    // where nobody ever ran `git config --global user.email` — a fresh server
    // or container, which is precisely where a sync daemon gets installed —
    // every fetch fails with "reflog messages need a committer which isn't
    // set", long after the clone appeared to succeed.
    //
    // keeper's own commits are unaffected: they carry an explicit signature
    // built from the profile and device. This is only so git's local
    // bookkeeping has a name to write.
    //
    // Only filled when nothing else supplies one. A human's real identity,
    // from any scope, must keep winning inside a folder they also use by hand.
    if repo.committer().is_none() {
        config
            .set_raw_value("user.name", IDENTITY_NAME)
            .map_err(|err| SyncError::Git(format!("could not set user.name: {err}")))?;
        config
            .set_raw_value("user.email", IDENTITY_EMAIL)
            .map_err(|err| SyncError::Git(format!("could not set user.email: {err}")))?;
    }

    if let Some(program) = filter_program {
        let workdir = workdir(repo)?;
        // `%f` is git's placeholder for the path being filtered. The program
        // path is quoted because a user's install directory may contain spaces,
        // and git splits this value on whitespace.
        let quoted = program.display().to_string();
        let clean = format!("\"{quoted}\" lfs clean --repo \"{}\" %f", workdir.display());
        let smudge = format!(
            "\"{quoted}\" lfs smudge --repo \"{}\" %f",
            workdir.display()
        );
        config
            .set_raw_value("filter.lfs.clean", clean.as_str())
            .map_err(|err| SyncError::Git(format!("could not set filter.lfs.clean: {err}")))?;
        config
            .set_raw_value("filter.lfs.smudge", smudge.as_str())
            .map_err(|err| SyncError::Git(format!("could not set filter.lfs.smudge: {err}")))?;
        config
            .set_raw_value("filter.lfs.required", "false")
            .map_err(|err| SyncError::Git(format!("could not set filter.lfs.required: {err}")))?;
    }

    let parent = path
        .parent()
        .ok_or_else(|| SyncError::Config(format!("{} has no parent", path.display())))?;
    // Written tmp-then-rename: a torn `.git/config` is a bricked repository,
    // and this runs on a pendrive that can be unplugged mid-write (AD-48).
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|source| SyncError::io("stage .git/config", parent.to_path_buf(), source))?;
    config
        .write_to(&mut staged)
        .map_err(|source| SyncError::io("write .git/config", path.clone(), source))?;
    // A temp file is created 0600; keep whatever mode the repository already
    // used so a shared-group checkout does not silently become private.
    if let Ok(metadata) = std::fs::metadata(&path) {
        let _ = std::fs::set_permissions(staged.path(), metadata.permissions());
    }
    staged
        .persist(&path)
        .map_err(|err| SyncError::io("replace .git/config", path.clone(), err.error))?;

    Ok(())
}

/// Repository-relative paths that differ between `HEAD`, the index and the
/// working tree.
///
/// Each vector is sorted and deduplicated so a caller can diff two snapshots
/// and so tests are order-independent.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RepoStatus {
    /// Tracked paths that did not exist in `HEAD`.
    pub added: Vec<PathBuf>,
    /// Tracked paths whose content or mode changed.
    pub modified: Vec<PathBuf>,
    /// Tracked paths that are gone.
    pub deleted: Vec<PathBuf>,
    /// Paths on disk that git does not track.
    pub untracked: Vec<PathBuf>,
}

impl RepoStatus {
    /// Whether anything at all differs.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.modified.is_empty()
            && self.deleted.is_empty()
            && self.untracked.is_empty()
    }
}

/// Classify everything `gix::status` reports.
///
/// Both halves of the status are consumed: `HEAD`-to-index changes (already
/// staged) and index-to-worktree changes (not yet staged). A path can therefore
/// legitimately appear in two buckets — staged as added, then edited again —
/// and that is reported rather than collapsed.
///
/// # Untracked content is listed file by file, on purpose
///
/// gitoxide's dirwalk defaults to `CollapseDirectory`, which reports a
/// brand-new folder as ONE entry naming the directory. Expanding that by hand
/// meant re-walking the filesystem ourselves — and a hand-rolled walk does not
/// know about `.gitignore`, so an ignored file inside a NEW directory was
/// staged and pushed while the identical file one level up was correctly
/// skipped. A new folder holding `node_modules/`, build output or a `.env`
/// went to the remote in full.
///
/// Asking the dirwalk to emit matching entries individually hands that
/// judgement back to git, which is the only thing that reads every
/// `.gitignore`, `.git/info/exclude` and the global excludes file correctly.
/// It costs one entry per untracked file instead of one per directory — the
/// same paths the caller had to produce anyway.
pub fn status_paths(repo: &gix::Repository) -> Result<RepoStatus> {
    use gix::status::{index_worktree::Item as WorktreeItem, plumbing::index_as_worktree, Item};

    let platform = repo
        .status(gix::progress::Discard)
        .map_err(|err| SyncError::Git(format!("status failed: {err}")))?
        .index_worktree_options_mut(|options| {
            if let Some(dirwalk) = options.dirwalk_options.as_mut() {
                dirwalk.set_emit_untracked(gix::dir::walk::EmissionMode::Matching);
            }
        });
    let iter = platform
        .into_iter(None)
        .map_err(|err| SyncError::Git(format!("status failed: {err}")))?;

    let mut out = RepoStatus::default();
    for item in iter {
        let item = item.map_err(|err| SyncError::Git(format!("status failed: {err}")))?;
        match item {
            Item::TreeIndex(change) => match change {
                gix::diff::index::Change::Addition { location, .. } => {
                    out.added.push(to_path(location.as_ref()));
                }
                gix::diff::index::Change::Deletion { location, .. } => {
                    out.deleted.push(to_path(location.as_ref()));
                }
                gix::diff::index::Change::Modification { location, .. } => {
                    out.modified.push(to_path(location.as_ref()));
                }
                // A rename is a deletion at the source fused with an addition
                // at the destination; keeping both keeps the two vectors a
                // faithful description of what the tree has to become.
                gix::diff::index::Change::Rewrite {
                    source_location,
                    location,
                    ..
                } => {
                    out.deleted.push(to_path(source_location.as_ref()));
                    out.added.push(to_path(location.as_ref()));
                }
            },
            Item::IndexWorktree(WorktreeItem::Modification {
                rela_path, status, ..
            }) => match status {
                index_as_worktree::EntryStatus::Change(index_as_worktree::Change::Removed) => {
                    out.deleted.push(to_path(rela_path.as_ref()));
                }
                index_as_worktree::EntryStatus::Change(_) => {
                    out.modified.push(to_path(rela_path.as_ref()));
                }
                // A conflicted entry is a divergence the merge machinery never
                // produces here (AD-43 makes conflict copies instead), and
                // `NeedsUpdate` / `IntentToAdd` mean the content is unchanged.
                _ => {}
            },
            Item::IndexWorktree(WorktreeItem::DirectoryContents { entry, .. }) => {
                if entry.status == gix::dir::entry::Status::Untracked {
                    out.untracked.push(to_path(entry.rela_path.as_ref()));
                }
            }
            Item::IndexWorktree(WorktreeItem::Rewrite {
                source,
                dirwalk_entry,
                ..
            }) => {
                out.deleted.push(to_path(source.rela_path()));
                out.added.push(to_path(dirwalk_entry.rela_path.as_ref()));
            }
        }
    }

    for bucket in [
        &mut out.added,
        &mut out.modified,
        &mut out.deleted,
        &mut out.untracked,
    ] {
        bucket.sort();
        bucket.dedup();
    }
    Ok(out)
}

/// The commit `HEAD` resolves to, or `None` on an unborn branch.
///
/// A freshly initialized repository with no commits is an ordinary state, not
/// an error — every profile starts there.
pub fn head_commit_id(repo: &gix::Repository) -> Result<Option<gix::hash::ObjectId>> {
    let mut head = repo
        .head()
        .map_err(|err| SyncError::Git(format!("could not read HEAD: {err}")))?;
    let id = head
        .try_peel_to_id()
        .map_err(|err| SyncError::Git(format!("could not peel HEAD: {err}")))?;
    Ok(id.map(gix::Id::detach))
}

/// Whether the index or the working tree differs from `HEAD`.
///
/// Untracked files do **not** make a repository dirty, matching `git status`.
pub fn is_dirty(repo: &gix::Repository) -> Result<bool> {
    repo.is_dirty()
        .map_err(|err| SyncError::Git(format!("dirty check failed: {err}")))
}

/// The working tree root.
///
/// A bare repository has none; that is a configuration error rather than a
/// panic, because a user can point a profile at one by mistake.
pub fn workdir(repo: &gix::Repository) -> Result<PathBuf> {
    repo.workdir().map(Path::to_path_buf).ok_or_else(|| {
        SyncError::Config(format!(
            "repository at {} is bare and has no working tree to synchronize",
            repo.git_dir().display()
        ))
    })
}

/// Repository-relative git path to a native path.
fn to_path(rela_path: &gix::bstr::BStr) -> PathBuf {
    gix::path::from_bstr(rela_path).into_owned()
}

/// Prefer `Cancelled` when the caller pulled the plug.
///
/// gitoxide surfaces an interruption as an ordinary transport error, and a
/// user-requested stop must never be reported as a failure (it would otherwise
/// be retried with backoff and shown as a warning).
/// The host a clone is talking to, for error messages.
///
/// `local` mirrors [`super::fetch`]'s answer for a `file://` or pendrive-to-
/// pendrive remote (AD-48), which genuinely has no host.
fn host_of(url: &str) -> String {
    gix::url::parse(url.as_bytes())
        .ok()
        .and_then(|parsed| parsed.host().map(str::to_owned))
        .unwrap_or_else(|| "local".to_owned())
}

fn cancelled_or(interrupt: &AtomicBool, otherwise: impl FnOnce() -> SyncError) -> SyncError {
    if interrupt.load(Ordering::Relaxed) {
        SyncError::Cancelled
    } else {
        otherwise()
    }
}

/// Initialize a repository inside a folder that already has content, and point
/// it at `remote_url`.
///
/// gitoxide's clone refuses a non-empty destination, which would make the most
/// ordinary request there is — "keep this folder I already have in sync" —
/// impossible. Adoption sidesteps that without touching a single existing file:
/// the repository is created empty around the content, the remote is attached,
/// and the caller's normal flow then commits what is there and reconciles it
/// with the remote through the usual divergence path.
///
/// `branch` becomes the initial HEAD, so the first commit lands where the
/// profile expects rather than on whatever the local git default happens to be.
pub fn adopt(root: &Path, remote_url: &str, branch: &str) -> Result<gix::Repository> {
    let repo = gix::init(root)
        .map_err(|err| SyncError::Git(format!("could not initialize {}: {err}", root.display())))?;

    // Point HEAD at the profile's branch before anything is committed. An
    // unborn branch is a plain symbolic ref, so this is just a file write.
    let head = format!("refs/heads/{branch}");
    repo.edit_reference(gix::refs::transaction::RefEdit {
        change: gix::refs::transaction::Change::Update {
            log: gix::refs::transaction::LogChange::default(),
            expected: gix::refs::transaction::PreviousValue::Any,
            new: gix::refs::Target::Symbolic(
                gix::refs::FullName::try_from(head.as_str())
                    .map_err(|err| SyncError::Config(format!("invalid branch name: {err}")))?,
            ),
        },
        name: gix::refs::FullName::try_from("HEAD")
            .map_err(|err| SyncError::Git(format!("HEAD is not a valid ref name: {err}")))?,
        deref: false,
    })
    .map_err(|err| SyncError::Git(format!("could not set HEAD: {err}")))?;

    let config_path = repo.git_dir().join("config");
    let mut config =
        gix::config::File::from_path_no_includes(config_path.clone(), gix::config::Source::Local)
            .map_err(|err| {
            SyncError::Git(format!("could not read {}: {err}", config_path.display()))
        })?;
    config
        .set_raw_value_by("remote", Some("origin".into()), "url", remote_url)
        .map_err(|err| SyncError::Git(format!("could not set remote.origin.url: {err}")))?;
    config
        .set_raw_value_by(
            "remote",
            Some("origin".into()),
            "fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        )
        .map_err(|err| SyncError::Git(format!("could not set remote.origin.fetch: {err}")))?;

    let parent = config_path
        .parent()
        .ok_or_else(|| SyncError::Config(format!("{} has no parent", config_path.display())))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|source| SyncError::io("stage .git/config", parent.to_path_buf(), source))?;
    config
        .write_to(&mut staged)
        .map_err(|source| SyncError::io("write .git/config", config_path.clone(), source))?;
    staged
        .persist(&config_path)
        .map_err(|err| SyncError::io("replace .git/config", config_path.clone(), err.error))?;

    // Reopen so the handle sees the remote and HEAD just written.
    open(root, false)
}

/// Make sure a managed repository still has the `origin` its profile syncs with,
/// restoring it when it is gone. Returns whether it had to repair one.
///
/// [`adopt`] writes the remote right after `gix::init`, but those are two
/// separate filesystem steps. A process killed between them — SIGKILL, power
/// loss, an OOM kill during the very first sync — leaves a `.git` with no
/// remote at all. Every later run then takes the "repository already exists"
/// branch, never calls `adopt` again, and fails permanently with `remote
/// "origin" is not usable: The remote named "origin" did not exist`. The folder
/// looks synced, the commits are there, and nothing will ever be published
/// again: precisely the unpublished-forever loss NFR-24 forbids. Found by the
/// durability matrix, which reproduces it whenever the kill lands in that
/// window.
///
/// Repairs only a MISSING remote. One that exists but points somewhere else is
/// left exactly as it is: keeper shares these folders with plain `git`, so a
/// remote a human deliberately re-pointed is theirs, not a value keeper owns
/// the way it owns `index.sparse`.
pub fn ensure_remote(repo: &gix::Repository, remote_url: &str) -> Result<bool> {
    let config_path = repo.git_dir().join("config");
    let mut config =
        gix::config::File::from_path_no_includes(config_path.clone(), gix::config::Source::Local)
            .map_err(|err| {
            SyncError::Git(format!("could not read {}: {err}", config_path.display()))
        })?;
    let present = config
        .raw_value_by("remote", Some("origin".into()), "url")
        .map(|url| !url.is_empty())
        .unwrap_or(false);
    if present {
        return Ok(false);
    }

    config
        .set_raw_value_by("remote", Some("origin".into()), "url", remote_url)
        .map_err(|err| SyncError::Git(format!("could not restore remote.origin.url: {err}")))?;
    config
        .set_raw_value_by(
            "remote",
            Some("origin".into()),
            "fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        )
        .map_err(|err| SyncError::Git(format!("could not restore remote.origin.fetch: {err}")))?;

    let parent = config_path
        .parent()
        .ok_or_else(|| SyncError::Config(format!("{} has no parent", config_path.display())))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|source| SyncError::io("stage .git/config", parent.to_path_buf(), source))?;
    config
        .write_to(&mut staged)
        .map_err(|source| SyncError::io("write .git/config", config_path.clone(), source))?;
    staged
        .persist(&config_path)
        .map_err(|err| SyncError::io("replace .git/config", config_path.clone(), err.error))?;
    Ok(true)
}

/// Every path the index tracks, repository-relative.
///
/// Used to find checked-out LFS pointers that still need materializing: only a
/// tracked path can hold one, so this bounds that scan to the index rather than
/// walking the whole worktree.
pub fn tracked_paths(repo: &gix::Repository) -> Result<Vec<PathBuf>> {
    let index = repo
        .index_or_empty()
        .map_err(|err| SyncError::Git(format!("could not read the index: {err}")))?;
    Ok(index
        .entries()
        .iter()
        .map(|entry| PathBuf::from(entry.path(&index).to_string()))
        .collect())
}

/// Re-stat `paths` and write the refreshed index.
///
/// Materializing an LFS object replaces a ~130-byte pointer with the real file,
/// so the entry's cached stat no longer describes what is on disk and `status`
/// would report every one of them as modified. Refreshing the stat — while
/// leaving the pointer blob as the entry's object — restores the invariant the
/// whole design rests on: pointer blob, worktree stat, clean status.
pub fn refresh_index_stat(repo: &gix::Repository, paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let workdir = workdir(repo)?;
    let shared = repo
        .index_or_empty()
        .map_err(|err| SyncError::Git(format!("could not read the index: {err}")))?;
    let mut index: gix::index::File = (**shared).clone();

    let wanted: std::collections::BTreeSet<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();

    let mut touched = false;
    for idx in 0..index.entries().len() {
        let key = {
            let state = &*index;
            let entry = &state.entries()[idx];
            entry.path(state).to_string()
        };
        if !wanted.contains(&key) {
            continue;
        }
        let absolute = workdir.join(&key);
        let Ok(metadata) = gix::index::fs::Metadata::from_path_no_follow(&absolute) else {
            continue;
        };
        // A pre-epoch timestamp is the only failure mode, and an all-zero stat
        // simply makes the entry racily clean so the next status re-reads it.
        if let Ok(stat) = gix::index::entry::Stat::from_fs(&metadata) {
            index.entries_mut()[idx].stat = stat;
            touched = true;
        }
    }
    if !touched {
        return Ok(());
    }
    index
        .write(gix::index::write::Options::default())
        .map_err(|err| SyncError::Git(format!("could not write the index: {err}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_stale_index_lock_is_released_but_a_fresh_one_is_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path();
        let lock = git_dir.join("index.lock");
        std::fs::write(&lock, b"").expect("write the lock");

        // A lock young enough that a live `git` command could still be holding
        // it. Stealing that would corrupt the user's index to fix a problem
        // nobody has.
        release_index_lock_older_than(git_dir, Duration::from_secs(3600));
        assert!(lock.exists(), "a lock somebody may still hold must survive");

        // The same file, judged against a threshold it is older than: debris
        // from a killed run, and the only thing standing between this profile
        // and never syncing again.
        release_index_lock_older_than(git_dir, Duration::ZERO);
        assert!(!lock.exists(), "a lock nobody holds must be released");
    }

    #[test]
    fn releasing_a_lock_that_was_never_there_is_not_an_error() {
        // Every repository open runs this, and the overwhelmingly common case
        // is that there is no lock at all.
        let dir = tempfile::tempdir().expect("tempdir");
        release_stale_index_lock(dir.path());
    }

    #[test]
    fn an_empty_remote_is_told_apart_from_a_real_failure() {
        // The narrowness is the point: misreading an auth failure as "empty"
        // would initialize an unrelated history and push it somewhere it does
        // not belong.
        assert!(is_empty_remote(&SyncError::Git(
            "clone failed: The remote didn't have any ref that matched 'main'".into()
        )));
        assert!(!is_empty_remote(&SyncError::Git(
            "clone failed: Credentials provided for \"ssh://host/r.git\" were not accepted".into()
        )));
        assert!(!is_empty_remote(&SyncError::Git(
            "clone failed: could not connect to host".into()
        )));
        assert!(!is_empty_remote(&SyncError::Config(
            "invalid remote URL".into()
        )));
    }

    use super::*;
    use crate::{
        git::commit::{stage_and_commit, StagedChange},
        provenance::{Provenance, SyncSource},
    };

    /// The durability hole this closes: `adopt` inits the repository and writes
    /// the remote as two separate steps, so a kill in between leaves a `.git`
    /// with no `origin` — and because `.git` then exists, `adopt` never runs
    /// again and every later sync dies with "the remote named origin did not
    /// exist". Reproduced by the durability matrix under load.
    #[test]
    fn a_repository_left_without_its_remote_is_repaired_on_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Exactly the half-initialized state a kill leaves: init ran, the config
        // write did not.
        let repo = gix::init(dir.path()).expect("init");

        assert!(
            ensure_remote(&repo, "https://example.org/x.git").expect("repair"),
            "a repository with no origin must be repaired"
        );
        let repo = open(dir.path(), false).expect("reopen");
        let url = repo
            .config_snapshot()
            .string("remote.origin.url")
            .expect("origin url");
        assert_eq!(url.to_string(), "https://example.org/x.git");

        assert!(
            !ensure_remote(&repo, "https://example.org/x.git").expect("second call"),
            "repair must be idempotent — a present remote is not rewritten"
        );
    }

    /// keeper shares these folders with plain `git`. A remote the human
    /// re-pointed on purpose is theirs, and silently restoring the profile's URL
    /// over it would be a surprise, not a repair.
    #[test]
    fn a_remote_pointing_elsewhere_is_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = adopt(dir.path(), "https://example.org/original.git", "main").expect("adopt");

        assert!(
            !ensure_remote(&repo, "https://example.org/profile.git").expect("no repair"),
            "an existing remote must not be rewritten"
        );
        let repo = open(dir.path(), false).expect("reopen");
        let url = repo
            .config_snapshot()
            .string("remote.origin.url")
            .expect("origin url");
        assert_eq!(url.to_string(), "https://example.org/original.git");
    }

    fn signature() -> gix::actor::Signature {
        gix::actor::Signature {
            name: "Keeper".into(),
            email: "sync@01ABC.keeper.invalid".into(),
            time: gix::date::Time::new(1_700_000_000, 0),
        }
    }

    fn provenance() -> Provenance {
        Provenance::new("fixture", "test-box", "01ABC", "localhost", SyncSource::Cli)
    }

    fn profile(root: &std::path::Path) -> crate::profile::SyncProfile {
        crate::profile::SyncProfile::new("01JFIXTURE", "fixture", root, "https://git.invalid/r.git")
    }

    /// A repository with `a.txt` and `b.txt` committed.
    fn repo_with_two_files() -> (tempfile::TempDir, gix::Repository) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        std::fs::write(dir.path().join("a.txt"), "alpha").expect("write a");
        std::fs::write(dir.path().join("b.txt"), "beta").expect("write b");
        let changes = StagedChange {
            added: vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")],
            ..StagedChange::default()
        };
        stage_and_commit(
            &repo,
            &changes,
            &provenance(),
            &profile(dir.path()),
            &signature(),
            &std::collections::BTreeMap::new(),
            None,
        )
        .expect("commit")
        .expect("a non-empty commit");
        // Re-open so the index snapshot is read fresh from disk, exactly as a
        // supervisor would on its next pass.
        let repo = open(dir.path(), true).expect("reopen");
        (dir, repo)
    }

    #[test]
    fn enforce_local_config_writes_the_key_that_keeps_status_working() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");

        enforce_local_config(&repo).expect("enforce");

        // Re-open so the assertion goes through the same path gix will use.
        let reopened = open(dir.path(), true).expect("reopen");
        assert_eq!(
            reopened.config_snapshot().boolean("index.sparse"),
            Some(false),
            "without this, gix::status hard-fails on a sparse index"
        );
    }

    #[test]
    fn enforce_local_config_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        enforce_local_config(&repo).expect("first");
        enforce_local_config(&repo).expect("second");

        let text = std::fs::read_to_string(dir.path().join(".git/config")).expect("read config");
        assert_eq!(
            text.matches("sparse").count(),
            1,
            "a repeated call must overwrite, not append: {text}"
        );
    }

    #[test]
    fn open_with_trust_full_reports_full_trust() {
        // Load-bearing for removable media: under the derived `Trust::Reduced`
        // gix drops repo-local `filter.*` and the LFS clean filter never runs.
        let dir = tempfile::tempdir().expect("tempdir");
        gix::init(dir.path()).expect("init");

        let repo = open(dir.path(), true).expect("open");
        assert_eq!(repo.git_dir_trust(), gix::sec::Trust::Full);
    }

    #[test]
    fn status_reports_untracked_files_and_nothing_else() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        std::fs::write(dir.path().join("new.txt"), "hello").expect("write");

        let status = status_paths(&repo).expect("status");
        assert_eq!(status.untracked, [PathBuf::from("new.txt")]);
        assert!(status.added.is_empty());
        assert!(status.modified.is_empty());
        assert!(status.deleted.is_empty());
    }

    #[test]
    fn status_classifies_a_modification_and_a_deletion_over_a_committed_fixture() {
        let (dir, repo) = repo_with_two_files();
        // Different length as well as different content, so the change is
        // visible from `lstat` alone and cannot be missed as racily clean.
        std::fs::write(dir.path().join("a.txt"), "alpha-changed").expect("modify");
        std::fs::remove_file(dir.path().join("b.txt")).expect("delete");

        let status = status_paths(&repo).expect("status");
        assert_eq!(status.modified, [PathBuf::from("a.txt")]);
        assert_eq!(status.deleted, [PathBuf::from("b.txt")]);
        assert!(status.untracked.is_empty());
    }

    #[test]
    fn a_clean_committed_repository_reports_no_changes() {
        let (_dir, repo) = repo_with_two_files();
        let status = status_paths(&repo).expect("status");
        assert!(status.is_empty(), "unexpected changes: {status:?}");
        assert!(!is_dirty(&repo).expect("dirty check"));
    }

    #[test]
    fn head_commit_id_is_none_on_an_unborn_branch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        assert_eq!(head_commit_id(&repo).expect("head"), None);
    }

    #[test]
    fn head_commit_id_follows_a_commit() {
        let (_dir, repo) = repo_with_two_files();
        assert!(head_commit_id(&repo).expect("head").is_some());
    }

    #[test]
    fn workdir_on_a_bare_repository_errors_instead_of_panicking() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init_bare(dir.path()).expect("init bare");

        let err = workdir(&repo).expect_err("a bare repo has no working tree");
        assert_eq!(err.code(), "config");
    }

    #[test]
    fn cloning_a_local_fixture_checks_out_and_pins_the_sparse_index_off() {
        // No network: the source is a bare repository in a tempdir, which
        // still exercises the real fetch/checkout path end to end.
        let source_dir = tempfile::tempdir().expect("tempdir");
        let source = gix::init_bare(source_dir.path()).expect("init bare");
        let blob = source.write_blob(b"alpha").expect("blob").detach();
        let tree = gix::objs::Tree {
            entries: vec![gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: "a.txt".into(),
                oid: blob,
            }],
        };
        let tree = source.write_object(&tree).expect("tree").detach();
        let mut buf = gix::date::parse::TimeBuf::default();
        let author = signature();
        let author = author.to_ref(&mut buf);
        source
            .commit_as(
                author,
                author,
                "HEAD",
                "root",
                tree,
                Vec::<gix::hash::ObjectId>::new(),
            )
            .expect("commit");
        let branch = source
            .head_name()
            .expect("head name")
            .expect("a born branch")
            .shorten()
            .to_string();

        let dest_dir = tempfile::tempdir().expect("tempdir");
        let dest = dest_dir.path().join("clone");
        let interrupt = AtomicBool::new(false);
        clone(
            &source_dir.path().to_string_lossy(),
            &dest,
            &branch,
            None,
            None,
            &interrupt,
        )
        .expect("clone from a local bare repository");

        assert_eq!(
            std::fs::read_to_string(dest.join("a.txt")).expect("checked out"),
            "alpha"
        );
        // Re-opened from disk on purpose: the clone-time override is only
        // in-memory, so reading the live handle would pass even if
        // `enforce_local_config` had written nothing.
        let reopened = open(&dest, true).expect("reopen");
        assert_eq!(
            reopened.config_snapshot().boolean("index.sparse"),
            Some(false),
            "clone must leave the sparse index disabled in .git/config"
        );
    }
}
