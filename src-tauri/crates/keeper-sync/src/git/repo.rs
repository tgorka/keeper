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
//!   [`clear_worktree_sparse_override`] is the other half of that guarantee:
//!   `.git/config` is not the last word once `git sparse-checkout` has switched
//!   the repository to worktree-scoped configuration.

use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    time::{Duration, Instant, SystemTime},
};

use crate::error::{Result, SyncError};
use crate::git::fetch::Credential;
use crate::lfs::pointer::{Pointer, MAX_POINTER_BYTES};

/// How long an `index.lock` must sit untouched before it is debris.
///
/// git writes an index in well under a second, and keeper's own staging of a
/// six-figure file count still finishes far inside this window. The threshold
/// is not tuned for speed — it is deliberately far longer than any real write,
/// so the only locks it ever removes are ones nobody is holding.
const STALE_INDEX_LOCK: Duration = Duration::from_secs(60);

/// How long a reference lock must sit untouched before it is debris.
///
/// git states its own bound: `core.filesRefLockTimeout` defaults to 100 ms,
/// which is how long git is prepared to *wait* for a loose reference lock
/// somebody else holds before giving up, and gitoxide uses the same default.
/// That is git saying a live holder releases in a fraction of a second — which
/// it does, because publishing a loose reference is a 41-byte write and a
/// rename. Two seconds is twenty times that bound, with room for a filesystem
/// having a bad day, and the window is only ever spent in full on a lock that
/// nobody is holding.
const STALE_REF_LOCK: Duration = Duration::from_secs(2);

/// How often [`release_ref_lock_if_abandoned`] re-stats the lock it is
/// watching.
///
/// Short, because the overwhelmingly likely reason to find a reference lock at
/// all is a `git` command running right now, and that case must cost one poll
/// rather than the whole window.
const REF_LOCK_POLL: Duration = Duration::from_millis(25);

/// Open a managed repository.
///
/// Pass `trust_full` for a repository the engine put there itself — including
/// one on removable media owned by another uid. See the module docs: without it
/// gitoxide discards repo-local filter configuration without saying so.
///
/// **This door does housekeeping.** It deletes an `index.lock` older than
/// [`STALE_INDEX_LOCK`], polls for and clears abandoned loose-ref locks, and
/// rewrites the merged configuration to drop a foreign `filter.lfs.process`
/// (the DW-140 and DW-206 guards). Every one of those is a repair, and every
/// leg that goes on to WRITE wants them. A caller that only means to read
/// should use [`open_read_only`] instead.
pub fn open(path: &Path, trust_full: bool) -> Result<gix::Repository> {
    let mut repo = open_read_only(path, trust_full)?;
    release_stale_index_lock(repo.git_dir());
    release_stale_ref_locks(repo.git_dir());
    drop_foreign_lfs_driver(&mut repo)?;
    Ok(repo)
}

/// Open a managed repository **without repairing anything** (Story 56.14).
///
/// [`open`] minus its three housekeeping calls, and nothing else: the same
/// `trust_full` semantics, the same error text, the same `gix::open_opts`.
///
/// # Why a second door rather than a flag
///
/// Because the difference is a promise to the rest of the machine, not a
/// tuning knob. `Engine::verify` is the one pass that takes no per-profile
/// reservation, so it is the likeliest of all of them to be running beside a
/// keeper commit or a person's own `git` — and the repairs are exactly the
/// operations that are dangerous next to a live writer: deleting an
/// `index.lock` that is 61 seconds old and genuinely held, or committing a
/// config snapshot while another process is editing `.git/config`. A check
/// that repairs what it is checking is not a check.
///
/// The cost dropped with them is not incidental either:
/// [`release_stale_ref_locks`] walks all of `refs/` and polls for up to
/// [`STALE_REF_LOCK`] per lock it finds.
///
/// **A caller that will write must not use this.** Reading an index, a tree or
/// a blob needs none of the three; staging, committing, fetching and pushing
/// all do, and `drop_foreign_lfs_driver` in particular is what stops another
/// installation's LFS filter from reaching this checkout.
pub fn open_read_only(path: &Path, trust_full: bool) -> Result<gix::Repository> {
    let mut options = gix::open::Options::default();
    if trust_full {
        options = options.with(gix::sec::Trust::Full);
    }
    gix::open_opts(path, options)
        .map_err(|err| SyncError::Git(format!("open failed: {}", super::fetch::flatten(&err))))
}

/// Remove every `filter "lfs"` driver that is not this repository's own from the
/// merged, in-memory configuration.
///
/// # The failure this ends
///
/// `git lfs install` — the same command whose hooks [`super::cli`] neutralizes —
/// writes a driver into the user's **global** config:
///
/// ```text
/// [filter "lfs"]
///     process  = git-lfs filter-process
///     clean    = git-lfs clean -- %f
///     smudge   = git-lfs smudge -- %f
///     required = true
/// ```
///
/// gitoxide collects one driver per configuration *section* and then takes the
/// **first** whose name matches (`gix::filter::extract_drivers` feeding
/// `gix_filter::pipeline::util::extract_driver`). Sections arrive in scope order,
/// so a global `filter "lfs"` precedes the local one
/// [`enforce_local_config_with_filter`] writes — and keys are **not** merged
/// across the two the way git merges them. The global section is therefore the
/// whole answer: keeper's `clean`/`smudge` are never consulted, its
/// `required = false` never applies, and `process` wins over both.
///
/// `process` is the long-running filter protocol, and gitoxide's launch of it
/// fails hard whatever `required` says (`gix_filter::driver::State::
/// maybe_launch_process` returns `ProcessHandshake` before any driver leniency is
/// consulted). On a desktop launch `PATH` is Finder's, so `/bin/sh -c
/// "git-lfs filter-process"` finds no `git-lfs`, exits, and the handshake read
/// hits EOF:
///
/// ```text
/// status failed: IO error while writing blob or reading file metadata or
/// changing filetype: Process handshake with command … "/bin/sh" "-c"
/// "git-lfs filter-process" "sh" failed: Failed to read or write to the
/// process: failed to fill whole buffer
/// ```
///
/// The trigger is any content re-read of an LFS entry, measured against gix
/// 0.86: `index_as_worktree` falls through to a content comparison whenever the
/// entry's stat tuple stops matching, and `FastEq` streams that content — through
/// the filter — as long as the SIZE still agrees, which for an LFS entry it
/// normally does (the entry's stat is the worktree file's, AD-46). A recording
/// whose mtime moved after the last index write is exactly that shape, and so is
/// the racily-clean case.
///
/// Field measurement (2026-08-13 → 2026-08-16, 90 430 identical lines in one
/// user's log): the state is self-perpetuating. `status` fails, so nothing
/// commits, so the index is never rewritten, so the same entry is re-read on the
/// next pass — no retry, restart or reinstall can clear it.
///
/// # Why the whole section goes
///
/// keeper *is* the LFS implementation for a folder it manages: it writes the
/// pointers, owns the object store under `.git/lfs`, uploads through its own
/// journal and prunes objects it has replicated. A driver belonging to another
/// installation is wrong here even when its binary is present — the same
/// reasoning [`super::cli`] applies to git-lfs's hooks, one layer down. Only the
/// repository's own scopes (`.git/config`, `.git/config.worktree`) may name the
/// `lfs` driver.
///
/// Nothing on disk is touched: the surgery is on the merged snapshot this
/// `Repository` handle holds, so a user's `~/.gitconfig` keeps working for every
/// repository keeper does not manage.
pub fn drop_foreign_lfs_driver(repo: &mut gix::Repository) -> Result<()> {
    // Reading first keeps the common case free: committing a snapshot re-reads
    // every value and clears the repository's caches, and this runs on every
    // open, which is once per sync pass.
    if !has_foreign_lfs_driver(repo) {
        return Ok(());
    }
    let mut snapshot = repo.config_snapshot_mut();
    // A loop, not one call: `remove_section_filter` removes the last match, and
    // a machine can carry several (system *and* global, or two global files
    // reached through an `include`).
    while snapshot
        .remove_section_filter("filter", Some("lfs".into()), |meta| {
            !is_repository_scope(meta)
        })
        .is_some()
    {}
    snapshot
        .commit()
        .map_err(|err| SyncError::Git(format!("could not drop a foreign lfs filter: {err}")))?;
    Ok(())
}

/// Whether any `filter "lfs"` section comes from outside this repository.
fn has_foreign_lfs_driver(repo: &gix::Repository) -> bool {
    repo.config_snapshot()
        .sections_by_name("filter")
        .into_iter()
        .flatten()
        .any(|section| {
            section
                .header()
                .subsection_name()
                .is_some_and(|name| name == "lfs")
                && !is_repository_scope(section.meta())
        })
}

/// `.git/config` and `.git/config.worktree`, and nothing else.
///
/// The scope, not the trust level: a global file is perfectly trustworthy and
/// still has no business naming the `lfs` driver for a folder keeper manages.
fn is_repository_scope(meta: &gix::config::file::Metadata) -> bool {
    meta.source.kind() == gix::config::source::Kind::Repository
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

/// Remove reference locks left behind by a process that was killed.
///
/// The same wound as the `index.lock` above, one layer further in and with a
/// worse ending. `gix::Repository::commit_as` publishes a commit by running a
/// reference transaction over `HEAD`, which locks `HEAD` and then the branch
/// it points at. A `SIGKILL` anywhere in that window — a crash, an OOM kill, a
/// laptop lid closing on a pendrive — leaves `HEAD.lock` or
/// `refs/heads/<branch>.lock` on disk with nobody holding it, and from then on
/// *every* commit in that folder fails with `A lock could not be obtained for
/// reference "HEAD"`. Work keeps being committed nowhere and published never,
/// the folder reports itself as merely failing, and the cure is a human
/// deleting a file they have no reason to know exists. That is the
/// unattended-recovery failure NFR-24 forbids, and the durability matrix
/// caught it twice: once as twelve committed changes that never reached the
/// remote, and once as a run that stopped making progress for ninety-six
/// minutes (see `.config/nextest.toml`).
///
/// **Waiting longer cannot fix this.** gitoxide already retries acquisition
/// with quadratic backoff, bounded by `core.filesRefLockTimeout` — 100 ms by
/// default. That is the right answer for a lock a live writer holds and no
/// answer at all for one whose owner is dead: there is nobody left to release
/// it, so every timeout, however generous, expires against the same file.
/// Raising it only turns "fails at once, forever" into "hangs first, then
/// fails, forever".
///
/// # Telling a dead process's lock from a live writer's
///
/// The lock file names no owner — no pid, no host, nothing to ask whether the
/// holder still exists — and keeper cannot assume it is the only writer
/// either, because these folders are meant to be usable with plain `git`. The
/// holder may be the user's own `git commit`, a GUI, or a second keeper
/// process. What *is* observable is whether the lock is moving, so that is
/// what this tests: the lock is watched for [`STALE_REF_LOCK`] and released
/// only if it was neither rewritten nor let go while we looked. A live writer
/// finishes inside a single poll and its lock is then left to it, released or
/// replaced, either way untouched by us.
///
/// The window is measured on our own clock rather than read off the lock's
/// mtime. A file timestamp is a claim by whichever machine wrote it, and on
/// removable media (AD-48) that is routinely not this one; elapsed time we
/// measured ourselves needs no such trust, and a clock that jumps cannot make
/// this either eager or blind.
///
/// And if the judgement is ever wrong, it still cannot corrupt anything. A
/// reference is published by renaming its lock over it, so a writer whose lock
/// we removed fails its own rename with `ENOENT` and reports an error while
/// the reference keeps the value it already had. A torn or half-written
/// reference is not a state this can produce — the worst case is somebody
/// else's write failing loudly, not silently landing wrong.
///
/// git itself never does any of this, and is right not to: git is run by a
/// person at a terminal, so it can print "another git process seems to be
/// running" and let them decide. keeper is a daemon syncing a folder nobody is
/// looking at. There is no one to read the message, which is the whole reason
/// NFR-24 exists.
///
/// `packed-refs.lock` is deliberately **not** included. It is the one
/// reference lock a legitimate operation holds for a long time — `git gc` and
/// `git pack-refs` hold it while rewriting every reference in the repository,
/// which is not bounded by the 100 ms above — and keeper's own commit and
/// fetch paths never create one, so nothing observed has ever stranded on it.
/// Covering it would put a guess on the same footing as a measurement.
///
/// Best-effort throughout, like its index-lock neighbour: a removal that races
/// another process, or a directory that will not be read, leaves the sync to
/// fail on its own terms rather than turning a cleanup into a hard error.
fn release_stale_ref_locks(git_dir: &Path) {
    release_ref_locks_unheld_for(git_dir, STALE_REF_LOCK);
}

/// The rule itself, with the window passed in so a test can exercise both
/// sides of the decision without waiting out the real one.
fn release_ref_locks_unheld_for(git_dir: &Path, window: Duration) {
    for lock in loose_ref_locks(git_dir) {
        release_ref_lock_if_abandoned(&lock, window);
    }
}

/// Every `*.lock` sitting beside a loose reference: `HEAD` itself, and
/// anything under `refs/`.
///
/// The whole set is collected before any of it is watched, so the walk reads
/// one consistent picture of the reference store rather than re-reading
/// directories that its own removals have changed.
fn loose_ref_locks(git_dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let head = git_dir.join("HEAD.lock");
    if head.is_file() {
        found.push(head);
    }
    let mut stack = vec![git_dir.join("refs")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // The entry's own type, not the target's, so a symlink can never
            // walk this out of the reference store. git forbids a reference
            // whose name ends in `.lock`, so the extension is unambiguous.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "lock") {
                found.push(path);
            }
        }
    }
    found
}

/// Watch one lock for `window` and release it only if nothing touched it.
///
/// Returns as soon as the lock proves itself alive, so the cost of being
/// careful is paid by abandoned locks and not by the user's own `git`.
fn release_ref_lock_if_abandoned(lock: &Path, window: Duration) {
    let Some(before) = lock_identity(lock) else {
        return;
    };
    let started = Instant::now();
    while let Some(remaining) = window.checked_sub(started.elapsed()) {
        std::thread::sleep(remaining.min(REF_LOCK_POLL));
        match lock_identity(lock) {
            // Let go while we watched: a live writer did exactly what one
            // does, and there was never anything here to recover.
            None => return,
            // Rewritten, or replaced by a second writer's: either way somebody
            // is using this and it is not ours to take.
            Some(after) if after != before => return,
            Some(_) => {}
        }
    }
    match std::fs::remove_file(lock) {
        // Logged, never silent. Quietly repairing somebody's git directory is
        // not a repair, it is a surprise; and a folder that keeps needing this
        // is a machine that keeps dying mid-write, which is worth seeing.
        Ok(()) => tracing::warn!(
            lock = %lock.display(),
            watched_ms = %window.as_millis(),
            "released a reference lock left behind by a killed run"
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(
            lock = %lock.display(),
            error = %err,
            "could not release a stale reference lock"
        ),
    }
}

/// Enough of a lock file to notice that somebody rewrote or replaced it.
///
/// Length and modification time rather than content: a reference lock is
/// written once and renamed away, so any change at all is proof of a writer,
/// and reading the bytes would race that writer for no extra information. An
/// unreadable stat is reported as absent, which is the conservative answer —
/// it ends the watch without removing anything.
fn lock_identity(lock: &Path) -> Option<(u64, SystemTime)> {
    let metadata = std::fs::metadata(lock).ok()?;
    if !metadata.is_file() {
        return None;
    }
    Some((metadata.len(), metadata.modified().ok()?))
}

/// The path gitoxide locks while it updates `reference`.
///
/// `.lock` is appended to the whole name rather than swapped in as an
/// extension, matching `gix_lock::acquire`'s own suffix rule — which is the
/// only version that gets a reference like `refs/tags/v1.0` right.
fn reference_lock_path(git_dir: &Path, reference: &gix::bstr::BStr) -> PathBuf {
    let mut path = git_dir
        .join(gix::path::from_bstr(reference))
        .into_os_string();
    path.push(".lock");
    PathBuf::from(path)
}

/// Refuse to start a reference transaction that somebody else is already in.
///
/// This is a decision about whether to *attempt* a commit, not a second lock
/// implementation: gitoxide still acquires the real lock, and this only
/// declines to call it when we can already see that doing so is pointless.
///
/// It exists because the failure is not merely pointless, it is unbounded.
/// `commit_as("HEAD", …)` derefs into two reference edits — `HEAD`, and the
/// branch it resolves to — and `gix-ref` 0.66.0 builds the name for a failed
/// acquisition by walking the edit's parent chain
/// (`store/file/transaction/prepare.rs:398-410`) without ever advancing its
/// cursor once it reaches the root. So which lock is held decides what
/// happens: a held `HEAD.lock` fails the ROOT edit, the loop body never runs,
/// and gitoxide returns the ordinary error; a held branch lock fails a CHILD
/// edit and gitoxide spins on a full core, forever, with no error and no
/// output. On a user's machine that is a wedged, hot process for as long as
/// their `git commit` holds the lock — and [`release_stale_ref_locks`] cannot
/// help there, because a lock a live writer holds is precisely the one it must
/// leave alone. Not entering the transaction is the only remedy available
/// while the defect lives upstream.
///
/// A refused pass is a normal outcome, not a fault: nothing is staged away,
/// the working tree still holds the change, and the failure is transient so
/// the scheduler retries it after backoff. A human's commit takes a moment,
/// so in practice the next pass simply succeeds.
///
/// **The check-then-act window is real and is not closed here.** A writer can
/// still take the lock between this call and gitoxide's acquisition. What this
/// removes is the whole *duration* of somebody else's hold — seconds for a
/// person typing a commit message, unbounded for debris — leaving only the few
/// microseconds between the two calls. Closing it entirely would need
/// gitoxide to expose its own acquisition, which it does not, so this is the
/// bound rather than the cure.
pub fn ensure_head_unlocked(repo: &gix::Repository) -> Result<()> {
    let git_dir = repo.git_dir();
    // Both references the transaction touches, named in the order it locks
    // them. An unborn branch has no name yet and cannot be locked.
    let branch = repo.head_name().ok().flatten();
    let references = [
        Some(gix::bstr::BStr::new("HEAD")),
        branch.as_ref().map(|name| name.as_bstr()),
    ];
    for reference in references.into_iter().flatten() {
        if reference_lock_path(git_dir, reference).is_file() {
            return Err(SyncError::Git(format!(
                "{reference} is being updated by another process; nothing was committed"
            )));
        }
    }
    Ok(())
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

    let (mut repo, _outcome) = checkout
        .main_worktree(gix::progress::Discard, interrupt)
        .map_err(|err| {
            cancelled_or(interrupt, || {
                SyncError::Git(format!("checkout failed: {}", super::fetch::flatten(&err)))
            })
        })?;

    enforce_local_config(&repo)?;
    // The checkout above is the one filtered operation keeper cannot protect
    // this way — `gix::clone::PrepareCheckout` hands out no mutable repository
    // (DW-206). Everything the caller does with this handle afterwards is.
    drop_foreign_lfs_driver(&mut repo)?;
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
    enforce_local_config_with_filter(repo, None, false)
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
/// All three keys are written, and `filter.lfs.process` is the load-bearing one
/// (DW-140). git prefers a `process` driver over a `clean`/`smudge` pair
/// *whatever scope each was defined in*, so the repository-local pair this
/// function used to write alone was silently outranked by the
/// `filter.lfs.process` that `git lfs install` leaves in `~/.gitconfig` — on
/// every machine that has ever had the real git-lfs. The pair is still written
/// because it costs one line and is what a `git` old enough to lack process
/// filters would use; the local `process` key is what actually takes effect.
///
/// # Why this is not what [`drop_foreign_lfs_driver`] already does (DW-206)
///
/// The two halves look like the same fix and are not, because they defend
/// different gits. `drop_foreign_lfs_driver` performs surgery on the merged
/// snapshot **this `Repository` handle holds** — its own doc says nothing on
/// disk is touched — so it settles what *gitoxide* consults, in this process.
/// keeper also shells out: `merge`, `push`, `checkout` and `sparse-checkout`
/// are the git binary (see the module docs), and that binary re-reads
/// `~/.gitconfig` for itself. An in-memory removal cannot reach it.
///
/// Measured, on the config state DW-206 leaves behind — local `clean`/`smudge`,
/// no local `process`, global `filter.lfs.process = git-lfs filter-process`:
/// `git add` staged a **git-lfs** pointer, not the output of keeper's `clean`.
/// The global driver still won. Writing a repository-scoped `process` key is
/// what out-ranks it, because git *does* merge keys across scopes with the
/// narrowest winning — which is the same rule, read from the other side, that
/// let the global one beat a local `clean`/`smudge` in the first place.
///
/// A foreign `process` — what `git lfs install --local` leaves here — is still
/// stripped, for DW-206's reason: keeper owns this section. The strip runs
/// *before* the write, so it removes theirs and never ours.
///
/// `required` is deliberately left false, and the reasoning changed rather than
/// survived: it is not that a failure is harmless, it is that
/// [`crate::lfs::filter::run_process`] no longer *has* the failure mode that
/// made `false` dangerous. A per-path refusal is reported as `status=error` and
/// the process stays up, so git falls back for one path instead of emptying the
/// rest of the checkout. What `false` still buys is the original case: a
/// worktree whose keeper binary moved must remain checkout-able, as pointers,
/// rather than hard-failing every git command in the folder.
pub fn enforce_local_config_with_filter(
    repo: &gix::Repository,
    filter_program: Option<&Path>,
    serves_process: bool,
) -> Result<()> {
    let path = repo.git_dir().join("config");
    let mut config = read_config(&path, gix::config::Source::Local)?;
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
        // Strip first, write second (DW-206 + DW-140). Every `process` key here
        // is somebody else's — `git lfs install --local` writes one, and keeper
        // owns this section — and clearing them before the write is what lets
        // the write below be the only one left. Doing it the other way round
        // would delete the key this function exists to install. Several
        // `[filter "lfs"]` sections in one file are legal, so every one of them
        // is stripped rather than the last; a file with no such section yet
        // yields no ids and the loop does nothing.
        let ids: Vec<_> = config
            .sections_and_ids()
            .filter(|(section, _)| {
                section.header().name() == "filter"
                    && section
                        .header()
                        .subsection_name()
                        .is_some_and(|name| name == "lfs")
            })
            .map(|(_, id)| id)
            .collect();
        for id in ids {
            if let Some(mut section) = config.section_mut_by_id(id) {
                while section.remove("process").is_some() {}
            }
        }

        config
            .set_raw_value("filter.lfs.clean", clean.as_str())
            .map_err(|err| SyncError::Git(format!("could not set filter.lfs.clean: {err}")))?;
        config
            .set_raw_value("filter.lfs.smudge", smudge.as_str())
            .map_err(|err| SyncError::Git(format!("could not set filter.lfs.smudge: {err}")))?;
        // Only when the program has been *asked* and answered — see
        // [`crate::lfs::filter::serves_process`]. A `process` key naming a binary
        // that cannot serve it is worse than none at all: gitoxide's launch
        // failure is hard whatever `required` says, so the folder stops syncing
        // rather than degrading to pointers.
        if serves_process {
            // No `%f`: the long-running protocol names each path in-band.
            let process = format!(
                "\"{quoted}\" lfs filter-process --repo \"{}\"",
                workdir.display()
            );
            config
                .set_raw_value("filter.lfs.process", process.as_str())
                .map_err(|err| {
                    SyncError::Git(format!("could not set filter.lfs.process: {err}"))
                })?;
        }
        config
            .set_raw_value("filter.lfs.required", "false")
            .map_err(|err| SyncError::Git(format!("could not set filter.lfs.required: {err}")))?;
    }

    write_config_atomically(&config, &path)?;

    Ok(())
}

/// Read one git configuration file, without following its include directives.
///
/// `source` is what decides precedence when gitoxide later merges scopes, so it
/// has to name the file honestly: `Local` for `.git/config`, `Worktree` for
/// `.git/config.worktree`.
fn read_config(path: &Path, source: gix::config::Source) -> Result<gix::config::File> {
    gix::config::File::from_path_no_includes(path.to_owned(), source)
        .map_err(|err| SyncError::Git(format!("could not read {}: {err}", path.display())))
}

/// Replace a git configuration file with `config`, atomically.
///
/// Written tmp-then-rename: a torn `.git/config` is a bricked repository, and
/// this runs on a pendrive that can be unplugged mid-write (AD-48).
fn write_config_atomically(config: &gix::config::File, path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| SyncError::Config(format!("{} has no parent", path.display())))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|source| SyncError::io("stage a git config", parent.to_path_buf(), source))?;
    config
        .write_to(&mut staged)
        .map_err(|source| SyncError::io("write a git config", path.to_path_buf(), source))?;
    // A temp file is created 0600; keep whatever mode the repository already
    // used so a shared-group checkout does not silently become private.
    if let Ok(metadata) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(staged.path(), metadata.permissions());
    }
    staged
        .persist(path)
        .map_err(|err| SyncError::io("replace a git config", path.to_path_buf(), err.error))?;
    Ok(())
}

/// The `.git/info/sparse-checkout` patterns in force, or `None` when the
/// repository is not in sparse mode at all (Story 27.2).
///
/// The pattern *grammar* is deliberately not interpreted here: it belongs with
/// [`crate::sparse`], which is also what decides the cone that produced it. This
/// answers only the two questions gitoxide can — is sparse checkout switched on,
/// and what does the file say.
///
/// "Switched on" is read from the effective configuration rather than from the
/// file's existence, because `git sparse-checkout disable` leaves the pattern
/// file exactly where it is and only flips `core.sparseCheckout` to false. A
/// repository that was ever sparse keeps a stale, complete-looking pattern file
/// forever, and reading that file alone would report a full checkout as narrow.
pub fn sparse_patterns(repo: &gix::Repository) -> Result<Option<String>> {
    if repo.config_snapshot().boolean("core.sparseCheckout") != Some(true) {
        return Ok(None);
    }
    let path = repo.git_dir().join("info").join("sparse-checkout");
    match std::fs::read_to_string(&path) {
        Ok(patterns) => Ok(Some(patterns)),
        // Sparse mode on with no patterns at all is a broken repository, not a
        // full checkout. Reported as an empty cone so the caller re-applies the
        // profile's own instead of concluding there is nothing to do.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Some(String::new())),
        Err(err) => Err(SyncError::io(
            "read the sparse-checkout patterns",
            path,
            err,
        )),
    }
}

/// Clear an `index.sparse` that a worktree-scoped configuration is shadowing,
/// reporting whether anything had to change (Story 27.2, AD-47).
///
/// # Why `.git/config` is not the last word
///
/// `git sparse-checkout` switches the repository to **worktree-scoped
/// configuration**: it writes `extensions.worktreeConfig = true` into
/// `.git/config` and puts `core.sparseCheckout` into `.git/config.worktree`.
/// That second file is loaded *after* `.git/config` by git and by gitoxide
/// alike, so whatever it names wins.
///
/// `index.sparse` is one of the things it can name. `git sparse-checkout set
/// --sparse-index` writes `index.sparse = true` there, and from that moment the
/// `false` [`enforce_local_config`] wrote into `.git/config` is dead text: the
/// effective value is `true`, git builds a genuinely sparse index, and
/// `gix::status` hard-fails with `TreeIndexDiff(IsSparse)` on every sync
/// afterwards — the exact failure the module docs open with, reached by a route
/// that leaves the enforced key sitting there looking correct.
///
/// keeper's own shim never passes `--sparse-index`. But keeper does not own
/// these folders alone: a human running plain `git` inside a synced folder is
/// the documented case — [`enforce_local_config_with_filter`] registers the LFS
/// filter for precisely that person — and one such command is all it takes. So
/// AD-47's invariant is enforced where it is actually decided, not only where it
/// was written.
///
/// The key is set to `false` rather than removed, which is also what git's own
/// `sparse-checkout disable` writes there.
pub fn clear_worktree_sparse_override(repo: &gix::Repository) -> Result<bool> {
    let git_dir = repo.git_dir();
    let path = git_dir.join("config.worktree");
    if !path.exists() {
        return Ok(false);
    }
    // The file is inert unless the extension names it, and enabling that
    // extension is git's decision to make, never keeper's.
    let local = read_config(&git_dir.join("config"), gix::config::Source::Local)?;
    if local.boolean("extensions.worktreeConfig").ok().flatten() != Some(true) {
        return Ok(false);
    }

    let mut config = read_config(&path, gix::config::Source::Worktree)?;
    let shadowing = match config.boolean("index.sparse") {
        // Not named here, so nothing is shadowed.
        Ok(None) => false,
        Ok(Some(sparse)) => sparse,
        // Present but unparseable. git will not accept it either, and it is
        // certainly not the `false` this engine requires.
        Err(_) => true,
    };
    if !shadowing {
        return Ok(false);
    }

    config
        .set_raw_value("index.sparse", "false")
        .map_err(|err| SyncError::Git(format!("could not set index.sparse: {err}")))?;
    write_config_atomically(&config, &path)?;
    Ok(true)
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
    /// Tracked paths whose state could not be determined, and why.
    ///
    /// Empty in every ordinary pass. A non-empty vector means the status is a
    /// report about the *rest* of the folder: these paths were stepped over so
    /// the others could be answered at all. See [`status_paths`].
    pub unreadable: Vec<UnreadablePath>,
}

impl RepoStatus {
    /// Whether anything at all differs.
    ///
    /// Deliberately blind to [`Self::unreadable`]: a path nobody can read is
    /// not a change to synchronize, and answering "yes, something differs"
    /// because of one would send the commit path off to stage a file it cannot
    /// open. The condition is reported, not converged.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.modified.is_empty()
            && self.deleted.is_empty()
            && self.untracked.is_empty()
    }
}

/// A tracked path the engine could not read, and the reason it could not.
///
/// `reason` is an errno rendering — "Permission denied (os error 13)" — never
/// file content, so it is safe in a log and in the UI (AD-21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadablePath {
    /// Repository-relative.
    pub path: PathBuf,
    /// What the filesystem said.
    pub reason: String,
}

impl std::fmt::Display for UnreadablePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.path.display(), self.reason)
    }
}

/// How many unreadable paths one pass will step over before giving up.
///
/// A handful of them is a permissions accident and worth working around. A
/// thousand is a different fault — an unmounted subtree, a revoked group, a
/// failing disk — and quietly synchronizing "everything except those thousand
/// files" would be a worse answer than refusing, because it looks like success.
const MAX_UNREADABLE_SKIPPED: usize = 32;

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
/// # One unreadable file does not cost the folder its synchronization
///
/// gitoxide reports a per-entry IO failure by aborting the whole walk: the
/// error surfaces only when the worker thread is joined, after every successful
/// item has already been yielded, so there is no "skip it and keep going" for
/// the caller to take. One file whose read fails therefore used to fail every
/// pass — and because `collect_stable_changes` propagates that, a single
/// unreadable path stalled the entire profile indefinitely. Two machines hit it
/// in the field within a day of each other, one of them for sixteen consecutive
/// passes with nothing else syncing.
///
/// That is the failure NFR-24 and FR-89 exist to forbid: convergence must not
/// wait on a human. And it is not an inherent property of the operation: given
/// the same unreadable tracked file, plain `git status` reports it as modified
/// and carries on. keeper was strictly less robust than the tool it wraps, on a
/// file git itself shrugs at. So a failing status is not the answer here, it is
/// the question. [`unreadable_tracked_paths`] finds which paths cannot be read, and
/// the walk is repeated with those excluded by pathspec, which gitoxide honours
/// before it ever opens them. The result describes the rest of the folder
/// truthfully and names what it had to step over in [`RepoStatus::unreadable`],
/// so the engine can raise it with the user while everything else converges.
///
/// The fallback matters as much as the mechanism: when the diagnosis finds
/// nothing — a file that opens but fails mid-read, a disk failing under the
/// hash — the original error is returned unchanged rather than a guess.
/// # The diagnosis is remembered, because it costs a walk of the whole index
///
/// Finding the bad path means one `lstat` and one `open` per tracked entry:
/// about six seconds on the 154 000-file profile this was found on, and it is a
/// removable USB volume. The condition persists until a human fixes it, and the
/// durability probe asks for a status roughly once a second while a recording
/// runs — so re-diagnosing per call would peg a core and thrash the disk for as
/// long as the file stayed broken.
///
/// So the answer is memoized per repository. A pass with a remembered set
/// excludes it up front and never scans at all; only a status that fails
/// *anyway* pays for a fresh walk. The memo is keyed by git directory rather
/// than by profile because being unreadable is a property of the disk, not of
/// whoever is asking.
///
/// It re-verifies before it trusts itself: every remembered path is re-checked
/// each pass — at most [`MAX_UNREADABLE_SKIPPED`] of them, so the check is
/// bounded — and one that has become readable is dropped, which is what lets a
/// file return to synchronization the moment its permissions are restored,
/// with no restart and nothing for the user to press.
pub fn status_paths(repo: &gix::Repository) -> Result<RepoStatus> {
    // No reporter, so the interval cannot matter: the walk asks nothing. And no
    // claim, so it changes nothing either — see [`WalkPolicy::read_only`].
    status_paths_reported(repo, None, Duration::MAX, WalkPolicy::read_only())
}

/// [`status_paths`], plus a way for a slow walk to say how far it has got.
///
/// The walk is the one step that can run for minutes with nothing on screen:
/// on a 155 000-file folder on a USB volume it stats every tracked file, and
/// until now it published exactly nothing while doing so - the pane read
/// `Idle - N waiting to sync` and the owner had no way to tell a folder that
/// was working from one that was wedged. `report` is called at most once a
/// second with `(items produced, index entries)`; see [`WalkReport`].
pub fn status_paths_reported(
    repo: &gix::Repository,
    report: Option<WalkReport<'_>>,
    interval: Duration,
    policy: WalkPolicy,
) -> Result<RepoStatus> {
    let known = still_unreadable(repo, remembered_unreadable(repo));
    let skip: Vec<PathBuf> = known.iter().map(|item| item.path.clone()).collect();

    // `WalkReport` is a shared reference and therefore `Copy`, which is the
    // point: the retry arm below needs the same reporter, and a `&mut dyn
    // FnMut` could not be handed to both walks.
    let status = match status_paths_excluding(repo, &skip, report, interval, policy) {
        Ok(mut status) => {
            status.unreadable = known;
            status
        }
        Err(first) => {
            let found = unreadable_tracked_paths(repo);
            if found.is_empty() {
                // Nothing to blame, so nothing to work around. The caller gets
                // the real error rather than a story about a path we invented.
                remember_unreadable(repo, &[]);
                return Err(first);
            }
            if found.len() > MAX_UNREADABLE_SKIPPED {
                tracing::warn!(
                    count = found.len(),
                    "too many unreadable paths to step over; reporting the failure instead"
                );
                remember_unreadable(repo, &[]);
                return Err(first);
            }
            let skip: Vec<PathBuf> = found.iter().map(|item| item.path.clone()).collect();
            let mut status = status_paths_excluding(repo, &skip, report, interval, policy)?;
            for item in &found {
                tracing::warn!(path = %item.path.display(), reason = %item.reason,
                    "this file could not be read; the rest of the folder was synchronized without it");
            }
            status.unreadable = found;
            status
        }
    };

    remember_unreadable(repo, &status.unreadable);
    Ok(status)
}

/// Unreadable paths already diagnosed for a repository, keyed by git directory.
///
/// A process-wide memo rather than engine state on purpose: all three callers
/// of [`status_paths`] would otherwise have to thread and agree on the same
/// set, and the thing being remembered belongs to the repository either way.
/// Bounded twice over — one entry per repository this process has synchronized,
/// each holding at most [`MAX_UNREADABLE_SKIPPED`] paths.
static UNREADABLE_MEMO: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, Vec<UnreadablePath>>>,
> = std::sync::OnceLock::new();

fn unreadable_memo(
) -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, Vec<UnreadablePath>>> {
    UNREADABLE_MEMO.get_or_init(Default::default)
}

fn remembered_unreadable(repo: &gix::Repository) -> Vec<UnreadablePath> {
    match unreadable_memo().lock() {
        Ok(memo) => memo.get(repo.git_dir()).cloned().unwrap_or_default(),
        // A poisoned memo is a cache miss, never a failed sync: the worst it
        // costs is the walk it existed to avoid.
        Err(_) => Vec::new(),
    }
}

fn remember_unreadable(repo: &gix::Repository, unreadable: &[UnreadablePath]) {
    let Ok(mut memo) = unreadable_memo().lock() else {
        return;
    };
    if unreadable.is_empty() {
        memo.remove(repo.git_dir());
    } else {
        memo.insert(repo.git_dir().to_path_buf(), unreadable.to_vec());
    }
}

/// Which of `known` still cannot be read, with a refreshed reason.
fn still_unreadable(repo: &gix::Repository, known: Vec<UnreadablePath>) -> Vec<UnreadablePath> {
    if known.is_empty() {
        return known;
    }
    let Ok(workdir) = workdir(repo) else {
        return Vec::new();
    };
    known
        .into_iter()
        .filter_map(|item| {
            why_unreadable(&workdir.join(&item.path)).map(|reason| UnreadablePath {
                path: item.path,
                reason,
            })
        })
        .collect()
}

/// How far the walk has got through the index, as gix counts it.
///
/// # Why the emitted items are not a liveness signal
///
/// A status walk emits only what CHANGED. Every entry it examines and finds
/// clean produces nothing at all, so once a worktree's dirty entries run out
/// part-way through the index the walk goes legitimately silent for the whole
/// remainder of the pass — and a watchdog reading emission as liveness kills a
/// healthy walk.
///
/// **Measured, not reasoned about.** On a 155 662-entry folder mid-migration
/// the same pass finished three times at ~4 300 s while 73 542 entries were
/// dirty, then — as the repair sweep converted 500 of them per pass — was
/// abandoned 61 consecutive times, every time at 2 912 s with exactly 600 s of
/// silence and roughly 1 400 s of honest work still to do. The folder had no
/// stall; the proxy did.
///
/// gix increments this once per index entry it has finished comparing, *after*
/// whatever filter conversion that entry needed. So it moves while the walk is
/// quietly scanning and freezes when a conversion deadlocks, which is exactly
/// the distinction emission cannot make.
#[derive(Clone, Debug)]
struct ScannedEntries {
    counter: gix::progress::StepShared,
}

impl gix::progress::Count for ScannedEntries {
    fn set(&self, step: gix::progress::Step) {
        self.counter.store(step, Ordering::Relaxed);
    }

    fn step(&self) -> gix::progress::Step {
        self.counter.load(Ordering::Relaxed)
    }

    fn inc_by(&self, step: gix::progress::Step) {
        self.counter.fetch_add(step, Ordering::Relaxed);
    }

    /// The whole reason this type exists rather than `gix::progress::Discard`:
    /// `Discard::counter()` hands out a fresh `Arc` per call, so gix counts
    /// into an atomic nobody else holds and the caller can never read it.
    fn counter(&self) -> gix::progress::StepShared {
        std::sync::Arc::clone(&self.counter)
    }
}

impl gix::progress::Progress for ScannedEntries {
    /// Deliberately not a reset. gix calls this with the index size before it
    /// takes the counter, and a fresh instance is armed per walk, so the only
    /// thing a reset could do here is race the watchdog to zero.
    fn init(&mut self, _max: Option<gix::progress::Step>, _unit: Option<gix::progress::Unit>) {}

    fn set_name(&mut self, _name: String) {}

    fn name(&self) -> Option<String> {
        None
    }

    fn id(&self) -> gix::progress::Id {
        gix::progress::UNKNOWN
    }

    fn message(&self, _level: gix::progress::MessageLevel, _message: String) {}
}

/// Bytes sitting in the LFS scratch directory right now.
///
/// A conversion in flight is a temp file growing in here, so a total that
/// changes between two looks means the filter is working. Summed rather than
/// counted because one file growing is the case that matters, and a count would
/// stay at 1 for the entire 550 s it takes.
///
/// Errors are silence, not zero-and-panic: an unreadable or absent scratch
/// directory means this signal has nothing to say, and the other two decide.
/// The cost is one `read_dir` per poll over a directory that holds a handful of
/// entries, against a poll measured in seconds.
fn scratch_bytes(dir: Option<&Path>) -> u64 {
    let Some(dir) = dir else {
        return 0;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|entry| entry.ok()?.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
}

/// Abandons a status walk that has stopped making progress.
///
/// # Why a watchdog and not a total timeout
///
/// A status pass has no honest upper bound. It converts every filtered file it
/// suspects of having changed, and one of those can be a gigabyte of video that
/// takes minutes on an external disk. A pass that is *slow* is doing its job; a
/// pass that has stopped moving is not, and only the second is worth killing.
/// So the clock is reset by progress, and it fires on stillness.
///
/// # What counts as progress
///
/// Three signals, any of which resets the clock: an item coming out of the
/// walk, gix's own count of index entries compared (see [`ScannedEntries`]),
/// and bytes accumulating in the LFS scratch directory.
///
/// The second is load-bearing for a clean tree — a walk with nothing left to
/// report is still working, and for most of a large index that is the only
/// signal there is.
///
/// The third is load-bearing for ONE BIG FILE, and it is the reason this guard
/// stopped killing walks on a folder full of video. gix increments its entry
/// counter *after* the conversion an entry needed, so a single 5 GB file
/// streaming through the filter moves neither of the first two signals for its
/// whole duration. Measured on the field folder 2026-08-28: five entries of
/// 4.3-5.3 GB each, ~9.6 MB/s on that USB volume, so 450-550 s per file with a
/// 600 s limit — and under any competing load, every pass died on them. Eight
/// consecutive walks were abandoned "after 46 entries and 1450s", each having
/// done 850 s of honest work first, and because the pass never completed the
/// objects were never published and the folder never drained.
///
/// Watching the scratch bytes is not a proxy for that work; it IS that work.
/// keeper's filter writes the object it is hashing into `.git/lfs/tmp` before
/// renaming it into the store, so the byte total moving means the conversion is
/// moving. A deadlocked filter writes nothing, which is the distinction the
/// first two signals cannot make and this one can.
///
/// # Why it interrupts rather than just reporting
///
/// gix polls the flag from inside the walk, so setting it unwinds the threads
/// that are stuck and returns control here. A watchdog that only logged would
/// leave the folder exactly as stuck as before while claiming to have noticed —
/// which is the failure mode this whole change exists to remove.
struct StatusWatchdog {
    heartbeat: std::sync::Arc<AtomicU64>,
    scanned: gix::progress::StepShared,
    started: Instant,
}

impl StatusWatchdog {
    fn arm(
        interrupt: std::sync::Arc<AtomicBool>,
        heartbeat: std::sync::Arc<AtomicU64>,
        scanned: gix::progress::StepShared,
        scratch: Option<PathBuf>,
    ) -> Self {
        Self::arm_with(
            interrupt,
            heartbeat,
            scanned,
            scratch,
            WATCHDOG_POLL,
            STATUS_SILENCE_LIMIT,
        )
    }

    /// The intervals as parameters, so the behaviour that matters — firing on
    /// stillness, staying quiet under load — is testable in milliseconds
    /// instead of in the ten minutes production waits.
    fn arm_with(
        interrupt: std::sync::Arc<AtomicBool>,
        heartbeat: std::sync::Arc<AtomicU64>,
        scanned: gix::progress::StepShared,
        scratch: Option<PathBuf>,
        poll: Duration,
        limit: Duration,
    ) -> Self {
        let watcher_interrupt = std::sync::Arc::clone(&interrupt);
        let watcher_heartbeat = std::sync::Arc::clone(&heartbeat);
        let watcher_scanned = std::sync::Arc::clone(&scanned);
        // Detached: it observes three atomics and owns nothing the walk needs,
        // so there is no join to get wrong and no lock it can hold. It ends
        // when the walk does, because the walk stops beating and the flag it
        // sets is read once more before anyone looks at it.
        std::thread::Builder::new()
            .name("keeper-status-watchdog".into())
            .spawn(move || {
                let mut last_beat = 0u64;
                let mut last_scan = 0usize;
                let mut last_moved = scratch_bytes(scratch.as_deref());
                let mut quiet = Duration::ZERO;
                loop {
                    std::thread::sleep(poll);
                    if watcher_interrupt.load(Ordering::Relaxed) {
                        return;
                    }
                    let beat = watcher_heartbeat.load(Ordering::Relaxed);
                    if beat == u64::MAX {
                        // The walk finished and said so; nothing left to watch.
                        return;
                    }
                    let scan = watcher_scanned.load(Ordering::Relaxed);
                    let moved = scratch_bytes(scratch.as_deref());
                    // Any signal is progress. The second is why a clean tree's
                    // quiet pass survives; the third is why a 5 GB file does.
                    if beat != last_beat || scan != last_scan || moved != last_moved {
                        last_beat = beat;
                        last_scan = scan;
                        last_moved = moved;
                        quiet = Duration::ZERO;
                        continue;
                    }
                    quiet += poll;
                    if quiet >= limit {
                        tracing::error!(
                            items = beat,
                            scanned = scan,
                            scratch_bytes = moved,
                            silent_for_s = quiet.as_secs(),
                            "the status walk stopped making progress; abandoning it"
                        );
                        watcher_interrupt.store(true, Ordering::Relaxed);
                        return;
                    }
                    // Said once per minute rather than once per poll: a long
                    // honest conversion should leave a trail, not a flood.
                    if quiet.as_secs().is_multiple_of(60) {
                        tracing::info!(
                            items = beat,
                            scanned = scan,
                            scratch_bytes = moved,
                            silent_for_s = quiet.as_secs(),
                            "the status walk is still working on one file"
                        );
                    }
                }
            })
            .ok();
        // `interrupt` is not kept: the watcher owns the clone that sets it and
        // the caller owns the clone that reads it, so a third handle here would
        // be a field nobody touches.
        drop(interrupt);
        Self {
            heartbeat,
            scanned,
            started: Instant::now(),
        }
    }

    /// One item came out of the walk. Answers how many have, so the caller
    /// needs no counter of its own.
    fn beat(&self) -> u64 {
        self.heartbeat.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// How many items the walk has produced.
    fn beats(&self) -> u64 {
        self.heartbeat.load(Ordering::Relaxed)
    }

    /// How many index entries the walk has compared. See [`ScannedEntries`]:
    /// this is the number that keeps moving over a clean tree, and the one a
    /// person reads to know whether a quiet pass was working.
    fn scans(&self) -> usize {
        self.scanned.load(Ordering::Relaxed)
    }

    /// How long the walk has been running.
    fn elapsed_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    /// The sentence a caller shows when the walk was abandoned.
    ///
    /// It names both counts, because "status failed" without them cannot tell
    /// "stuck on the first file" from "stuck on the last one", and that
    /// difference is the whole of the next person's investigation. Two numbers
    /// rather than one: the changes found say how much of the pass's *output*
    /// survived, and the files checked say where in the index it stopped —
    /// which are different questions on a tree that is mostly clean.
    fn silence_error(&self, seen: u64) -> SyncError {
        SyncError::Git(format!(
            "the folder's status scan stopped responding after {seen} entries and \
             {}s, having checked {} files, so keeper abandoned it rather than \
             waiting. This is usually a stalled content filter; the next pass \
             will try again.",
            self.started.elapsed().as_secs(),
            self.scans()
        ))
    }
}

impl Drop for StatusWatchdog {
    fn drop(&mut self) {
        // Tell the watcher the walk is over, whichever way it ended.
        self.heartbeat.store(u64::MAX, Ordering::Relaxed);
    }
}

/// Hand back the walk's result, or refuse it because the walk was abandoned.
///
/// **The refusal is the point.** An interrupt does not make gix yield an error
/// — it makes the iterator END, which from the loop is indistinguishable from a
/// walk that finished. Without this check the caller receives a status computed
/// from whatever was reached before the stall and treats it as a description of
/// the whole worktree; `commit_local` would then commit that reading. Observed
/// on the first run of this guard against a real stalled folder: 19 entries of
/// a 567-file tree, offered as `added=0 modified=1 deleted=2`.
fn finish_walk(
    interrupt: &AtomicBool,
    watchdog: &StatusWatchdog,
    out: RepoStatus,
) -> Result<RepoStatus> {
    if interrupt.load(Ordering::Relaxed) {
        return Err(watchdog.silence_error(watchdog.beats()));
    }
    Ok(out)
}

/// How often the watchdog looks. Short enough to be responsive, long enough to
/// cost nothing over a pass that runs for minutes.
const WATCHDOG_POLL: Duration = Duration::from_secs(5);

/// How long the walk may go without moving at all before it is abandoned.
///
/// Not a performance budget — a liveness one. A status pass over a large
/// worktree can legitimately spend minutes on one file: converting a gigabyte
/// of video through the LFS filter is real work, and killing it because it is
/// slow would be a bug of its own. What it may never do is stop *both* emitting
/// items and comparing index entries, which is what a deadlocked filter looks
/// like from here.
///
/// Ten minutes is far longer than the slowest honest file and far shorter than
/// the four days a stalled folder went unnoticed in the field.
const STATUS_SILENCE_LIMIT: Duration = Duration::from_secs(600);

/// The most files this walk converts at once.
///
/// **The number that took a vault down for four days.** `thread_limit: None`
/// means "one thread per core", and each thread that meets a filtered file
/// holds an LFS filter conversion open while it streams the file through. On a
/// worktree of `*.mov` recordings that produced 55 conversions in flight at
/// once against a much smaller pool of filter processes — every one of them
/// blocked, none able to finish, no error, no progress, forever.
///
/// Four is enough to keep a disk busy and small enough that the conversions in
/// flight cannot outnumber the workers that serve them. A status pass is not
/// the hot path; it runs before a pull and once per scan.
const STATUS_THREAD_LIMIT: usize = 4;

/// The ceiling is the fix, so it is enforced where it cannot be argued with
/// rather than in a test somebody can delete: raising this back to "one per
/// core" is what deadlocked 55 conversions against a smaller filter pool.
const _: () = assert!(STATUS_THREAD_LIMIT >= 1 && STATUS_THREAD_LIMIT <= 8);

/// What a walk in progress can say about itself: index entries compared, and
/// the number of index entries there are.
///
/// **Both numbers count the same thing**, which is the whole point and was not
/// true before. The numerator used to be items *emitted* — a walk's changed
/// paths — against a denominator of index entries. On a folder mid-LFS
/// migration that reads as `9113/155662` and creeps, because it is measuring
/// how many differences have been found, not how far the walk has got; and it
/// stops dead for the whole of any stretch where the walk finds nothing, which
/// on a mostly clean tree is most of the pass.
///
/// `scanned` is what gix itself counts through the index (see `ScannedEntries`),
/// so the pair is monotonic, bounded by its own denominator, and moves at the
/// rate the walk is actually working. It is what turns ten silent minutes into
/// "checked 41 000 of 155 000 files" and means it.
///
/// `Sync`, because the ticker that publishes it runs beside the walk rather
/// than inside it — see `status_paths_excluding`.
pub type WalkReport<'a> = &'a (dyn Fn(u64, u64) + Sync);

/// File one walked item into the bucket that describes what has to happen.
///
/// Lifted out of the loop so the loop can live inside a `thread::scope` without
/// eighty lines of match arms moving one indent to the right; the classification
/// is unchanged.
fn push_item(out: &mut RepoStatus, item: gix::status::Item) {
    use gix::status::{index_worktree::Item as WorktreeItem, plumbing::index_as_worktree, Item};

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
            // A rename is a deletion at the source fused with an addition at
            // the destination; keeping both keeps the two vectors a faithful
            // description of what the tree has to become.
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

/// Whether a finished walk still owes one closing progress report.
///
/// Two rules meet here, and they pull in opposite directions.
///
/// A walk that never reached its first tick must stay silent: publishing the
/// scanning phase on every poll of an idle folder is what made the menu-bar
/// glyph flip once a second, and the report exists for the ten-minute walk, not
/// the ten-millisecond one. That is the `spoke` half — a ticker that did speak
/// must not leave the last figure it happened to catch as the final word, or
/// the bar stops short of its own end (the macOS gate saw `[198, 382, 296,
/// 392]` against a denominator of 400).
///
/// A ZERO interval is the opposite case and always owes the report. It means
/// "report every item", which the ticker cannot deliver: it sleeps a clamped
/// millisecond before its first look, and a small walk has already set `done`
/// by then. Without this arm every caller of `Engine::report_every_walk_item`
/// is racing its own fixture — `a_pending_poll_publishes_the_progress_of_its_own_walk`
/// failed on the macOS gate for exactly this reason while passing on Linux,
/// with an empty event stream and a walk that had genuinely run.
fn owes_closing_report(spoke: bool, interval: Duration) -> bool {
    spoke || interval.is_zero()
}

/// What a walk does besides answering the question it was asked.
///
/// Two decisions, both of which cost or save whole minutes on a large folder,
/// and neither of which the walk can make for itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkPolicy {
    /// Write the stat data the walk observed back into the index.
    ///
    /// gitoxide hands these over and expects the caller to save them:
    /// `EntryStatus::NeedsUpdate(stat)` is collected into
    /// `gix::status::Outcome`, whose own documentation says that without the
    /// write-back "subsequent `status` operations will take longer to
    /// complete". Dropping them is what made tgdrive's pane a permanent
    /// "Scanning": measured on that folder, 60 280 of 155 662 index entries
    /// carried no stat at all, so every pass re-read 25.4 GB — 24.2 GB of it
    /// back through the LFS clean filter — to learn what the previous pass had
    /// already learned and thrown away.
    ///
    /// Only for a caller that holds the folder's walk claim. The write is the
    /// walk's own index with stat fields replaced, so a leg that staged
    /// something after the walk started would have that work overwritten.
    pub persist_stats: bool,
    /// Walk the directories looking for files git does not track.
    ///
    /// This is the expensive half of a walk and it is not proportional to what
    /// changed: it `lstat`s the whole tree every time. Measured on tgdrive,
    /// `find . -type f` over 157 490 files takes 996 s on that USB volume, and
    /// that is the floor for a pass that asks the question at all.
    ///
    /// The commit leg must ask it — an untracked file is exactly what it exists
    /// to publish. A five-second UI poll does not: a new file reaches the
    /// Pending list through the watcher and the stability gate long before any
    /// walk would find it, so the poll asks only about entries the index
    /// already names, and sweeps for untracked ones on the cadence in
    /// `Engine::UNTRACKED_SWEEP_INTERVAL`.
    pub find_untracked: bool,
}

impl WalkPolicy {
    /// Everything: find untracked files, and save what was learned.
    pub const fn full() -> Self {
        Self {
            persist_stats: true,
            find_untracked: true,
        }
    }

    /// Only what the index already names — no directory walk — but still save
    /// the stats, because that is what makes the next pass cheaper.
    pub const fn tracked_only() -> Self {
        Self {
            persist_stats: true,
            find_untracked: false,
        }
    }

    /// Answer the question and change nothing. For callers that do not hold the
    /// walk claim, and for every test that asserts on a walk's output rather
    /// than on its effect.
    pub const fn read_only() -> Self {
        Self {
            persist_stats: false,
            find_untracked: true,
        }
    }
}

/// The walk, with the pace its caller reports at.
///
/// `skip` is held out of the walk entirely. The exclusions are spelled
/// `:(exclude,literal)<path>`: `literal` because a synced folder is full of
/// names that are also glob syntax — `[2026]`, `*`, `?` all occur in real user
/// content — and a pattern that quietly matched more than the one path it names
/// would hide unrelated files from every pass.
///
/// `interval` is the publisher's policy, not git's: see the engine's
/// `WALK_REPORT_INTERVAL`. `Duration::MAX` with no reporter is the silent case.
///
/// `policy` decides the two things that dominate the cost on a large folder:
/// whether the directories are walked at all, and whether what was learned is
/// written down. See [`WalkPolicy`].
fn status_paths_excluding(
    repo: &gix::Repository,
    skip: &[PathBuf],
    report: Option<WalkReport<'_>>,
    interval: Duration,
    policy: WalkPolicy,
) -> Result<RepoStatus> {
    // `flatten`, not `{err}`: a status that trips over one unreadable tracked
    // file reports "IO error while writing blob or reading file metadata or
    // changing filetype" in its top frame, and the errno that says *why* —
    // permission denied, EOF from a file rewritten mid-read — is two `source()`
    // hops down. gix never names the path, so the cause is all there is.
    // The walk is told when to give up before it is told what to look for: a
    // pass that cannot be abandoned is a pass that can hang a folder forever,
    // which is what this function did in the field (DW: a vault went four days
    // without a fetch, `running` in the journal, no error, no progress).
    let interrupt = std::sync::Arc::new(AtomicBool::new(false));
    let heartbeat = std::sync::Arc::new(AtomicU64::new(0));
    // gix counts its own way through the index into this; the watchdog reads it
    // so a pass with nothing left to report still counts as alive. See
    // `ScannedEntries` for the 61 healthy walks that were killed without it.
    let scanned: gix::progress::StepShared = std::sync::Arc::new(AtomicUsize::new(0));
    // Where keeper's own filter writes what it is hashing. The walk's third
    // liveness signal, and the only one that moves while one 5 GB entry
    // converts: see [`StatusWatchdog`].
    let scratch = crate::lfs::store::LfsStore::in_git_dir(repo.git_dir()).tmp_dir();
    let watchdog = StatusWatchdog::arm(
        std::sync::Arc::clone(&interrupt),
        std::sync::Arc::clone(&heartbeat),
        std::sync::Arc::clone(&scanned),
        Some(scratch),
    );

    let platform = repo
        .status(ScannedEntries {
            counter: std::sync::Arc::clone(&scanned),
        })
        .map_err(|err| SyncError::Git(format!("status failed: {}", super::fetch::flatten(&err))))?
        .should_interrupt_owned(std::sync::Arc::clone(&interrupt))
        .index_worktree_options_mut(|options| {
            if policy.find_untracked {
                if let Some(dirwalk) = options.dirwalk_options.as_mut() {
                    dirwalk.set_emit_untracked(gix::dir::walk::EmissionMode::Matching);
                }
            } else {
                // `None` is how gix is told not to walk the directories at all.
                // Clearing it rather than filtering the emissions is the whole
                // point: the cost is the `lstat` of every entry in the tree,
                // not the reporting of the few that are untracked.
                options.dirwalk_options = None;
            }
            // See `STATUS_THREAD_LIMIT`: unbounded parallelism here is what
            // produced 55 simultaneous LFS conversions against a smaller pool
            // of filter processes and deadlocked every one of them.
            options.thread_limit = Some(STATUS_THREAD_LIMIT);
        });
    let patterns: Vec<gix::bstr::BString> = skip
        .iter()
        .map(|path| {
            let mut pattern = gix::bstr::BString::from(":(exclude,literal)");
            pattern.extend_from_slice(&gix::path::into_bstr(path.as_path()));
            pattern
        })
        .collect();
    let mut iter = platform
        .into_iter(patterns)
        .map_err(|err| SyncError::Git(format!("status failed: {}", super::fetch::flatten(&err))))?;

    // The denominator, read once: the index is already mapped by the walk
    // itself, and re-asking per item would put a load behind a counter whose
    // whole purpose is to cost nothing.
    let entries = repo
        .index_or_empty()
        .map(|index| index.entries().len() as u64)
        .unwrap_or(0);

    // Progress comes off a clock, not off the items.
    //
    // It used to be published from inside this loop, which meant it could only
    // move when the walk *emitted* something. A pass whose dirty entries run
    // out part-way then compares tens of thousands of clean files in complete
    // silence — the same stretch that used to get healthy walks killed — and
    // the pane froze on whatever number the last emission left behind. This is
    // also why the number the owner saw was items emitted rather than entries
    // compared: inside the loop, the emission count was the only one that was
    // guaranteed to have moved.
    //
    // Scoped, so the ticker can borrow the caller's reporter rather than
    // forcing every call site through an `Arc`. It wakes on a short slice so
    // the scope's join never adds a report interval to the end of a fast walk:
    // `neuradrive` finishes in 150 ms and runs every few seconds.
    let done = std::sync::Arc::new(AtomicBool::new(false));
    // Whether the ticker ever spoke, which is one half of
    // [`owes_closing_report`] - the rule that decides whether this walk still
    // owes a final figure when it returns.
    let spoke = std::sync::Arc::new(AtomicBool::new(false));
    let mut out = RepoStatus::default();
    let walked = std::thread::scope(|scope| -> Result<RepoStatus> {
        if let Some(report) = report {
            let ticking = std::sync::Arc::clone(&scanned);
            let ticking_done = std::sync::Arc::clone(&done);
            let ticking_spoke = std::sync::Arc::clone(&spoke);
            scope.spawn(move || {
                // The wake is short so the scope's join never adds a report
                // interval to the end of a fast walk, and it never exceeds the
                // interval itself so a test can ask for a millisecond cadence
                // and get one.
                let slice = interval.clamp(Duration::from_millis(1), Duration::from_millis(20));
                let mut waited = Duration::ZERO;
                loop {
                    std::thread::sleep(slice);
                    if ticking_done.load(Ordering::Relaxed) {
                        return;
                    }
                    waited += slice;
                    if waited >= interval {
                        waited = Duration::ZERO;
                        ticking_spoke.store(true, Ordering::Relaxed);
                        report(ticking.load(Ordering::Relaxed) as u64, entries);
                    }
                }
            });
        }
        // `done` is set on every exit from the walk, including the error one:
        // a ticker left running would hold the scope open forever.
        let walked = (|| -> Result<()> {
            for item in iter.by_ref() {
                // Before the item is inspected: a walk that is producing
                // anything at all is alive, whatever the item turns out to be.
                // The heartbeat is the count, so there is no second counter to
                // keep in step with it.
                let seen = watchdog.beat();
                let item = item.map_err(|err| {
                    // An interrupt usually ends the iterator rather than
                    // surfacing here, which is why the real guard is after the
                    // loop — but if it ever does surface, the reason the walk
                    // stopped is the useful sentence, not gix's inner error.
                    if interrupt.load(Ordering::Relaxed) {
                        return watchdog.silence_error(seen);
                    }
                    SyncError::Git(format!("status failed: {}", super::fetch::flatten(&err)))
                })?;
                push_item(&mut out, item);
            }
            Ok(())
        })();
        done.store(true, Ordering::Relaxed);
        // The closing figure, from this thread now the ticker has been told to
        // stop, so there is exactly one last report and no race for it.
        if let Some(report) = report {
            if owes_closing_report(spoke.load(Ordering::Relaxed), interval) {
                report(scanned.load(Ordering::Relaxed) as u64, entries);
            }
        }
        walked.map(|()| std::mem::take(&mut out))
    })?;
    let mut out = walked;

    // The step gitoxide leaves to the caller, and the one keeper used to skip.
    //
    // Before `finish_walk`, because an interrupted walk still learned something
    // true about every entry it did reach — and after the scope, because the
    // iterator has to be finished for its outcome to exist at all.
    if policy.persist_stats {
        persist_observed_stats(repo, iter);
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
    // **After the loop, not inside it.** An interrupt does not make gix yield an
    // error — it makes the iterator END, which is indistinguishable from a walk
    // that finished. Returning here would hand the caller a status computed
    // from the entries reached before the stall and let `commit_local` act on
    // it as though it described the whole worktree. Observed on the very first
    // run of this guard: 19 entries of a 567-file tree, reported as
    // `added=0 modified=1 deleted=2`.
    let out = finish_walk(&interrupt, &watchdog, out)?;

    // The shape of the pass, once, at INFO. A folder that later stalls is
    // diagnosed by comparing this line between runs — how many entries, how
    // long — and its absence is what made the field failure unreadable.
    //
    // Named, because two profiles produce interleaved lines and one of them may
    // hold 641 files while the other holds 155 662: `elapsed_ms=108` next to
    // `elapsed_ms=3044748` is unreadable without knowing which folder each
    // belongs to, and that cost real time during the field diagnosis. The
    // worktree's own directory name is the profile's name in every case that
    // matters and needs nothing threaded through to obtain.
    tracing::info!(
        folder = repo
            .workdir()
            .and_then(|dir| dir.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        entries = watchdog.beats(),
        scanned = watchdog.scans(),
        elapsed_ms = watchdog.elapsed_ms(),
        added = out.added.len(),
        modified = out.modified.len(),
        deleted = out.deleted.len(),
        "status walk finished"
    );
    Ok(out)
}

/// Save the stat data a finished walk observed, so the next one can answer from
/// `lstat` instead of reading the file again.
///
/// gitoxide collects these while it walks — `EntryStatus::NeedsUpdate(stat)`,
/// gathered into `gix::status::Outcome` — and then leaves the write to the
/// caller, saying so in its own documentation: without it, "subsequent `status`
/// operations will take longer to complete".
///
/// Best-effort, and quiet about the ordinary cases. `into_outcome` returns
/// `None` for a walk that ended early, which is not a failure: the pass was
/// interrupted and there is nothing to save. A failed write is worth a line,
/// because the folder will keep paying for it, but it is not worth failing a
/// pass that has already produced a correct answer.
fn persist_observed_stats(repo: &gix::Repository, iter: gix::status::Iter) {
    let Some(mut outcome) = iter.into_outcome() else {
        return;
    };
    if !outcome.has_changes() {
        return;
    }
    let observed = outcome
        .index_worktree
        .tracked_file_modification
        .entries_to_update;
    match outcome.write_changes() {
        Some(Ok(())) => tracing::info!(
            folder = repo
                .workdir()
                .and_then(|dir| dir.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            entries = observed,
            "wrote the stat data this walk observed into the index"
        ),
        Some(Err(err)) => tracing::warn!(
            error = %err,
            entries = observed,
            "could not write the walk's stat data back; the next pass will re-read those files"
        ),
        None => {}
    }
}

/// Tracked paths that cannot be read right now, and what the filesystem said.
///
/// Only ever reached after a status has already failed, which is what makes the
/// cost defensible: it is one `lstat` and one `open` per tracked entry, no
/// content is read, and nothing calls it on a healthy repository.
///
/// # What it can and cannot catch
///
/// An `open` proves the file can be *started*, not finished. A disk failing
/// mid-file, or a file truncated between the stat and the read, still fails the
/// status and is invisible here — deliberately, because the alternative is
/// reading every tracked byte to find out. [`status_paths`] returns the
/// original error when this finds nothing, so an undiagnosable failure stays
/// exactly as loud as it was.
///
/// A missing path is not unreadable: a deleted file is an ordinary change and
/// status reports it as one. Only an error that is *not* `NotFound` counts.
fn unreadable_tracked_paths(repo: &gix::Repository) -> Vec<UnreadablePath> {
    let Ok(workdir) = workdir(repo) else {
        return Vec::new();
    };
    let Ok(index) = repo.index_or_empty() else {
        return Vec::new();
    };
    let state = &*index;

    let mut out: Vec<UnreadablePath> = Vec::new();
    for entry in state.entries() {
        let rela = to_path(entry.path(state));
        if let Some(reason) = why_unreadable(&workdir.join(&rela)) {
            out.push(UnreadablePath { path: rela, reason });
            // One past the ceiling is enough to prove the ceiling was breached,
            // and stops a failing volume from being walked to its end.
            if out.len() > MAX_UNREADABLE_SKIPPED {
                break;
            }
        }
    }
    out
}

/// Why `absolute` cannot be read, or `None` if it can.
fn why_unreadable(absolute: &Path) -> Option<String> {
    let metadata = match std::fs::symlink_metadata(absolute) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => return Some(err.to_string()),
    };

    if metadata.is_symlink() {
        // The blob of a symlink is its target, so reading the link is the whole
        // of what staging it needs.
        return match std::fs::read_link(absolute) {
            Ok(_) => None,
            Err(err) => Some(err.to_string()),
        };
    }
    if !metadata.is_file() {
        // A directory or device where a file is tracked is a different
        // condition with its own error, and not one an exclusion would fix.
        return None;
    }
    match std::fs::File::open(absolute) {
        Ok(_) => None,
        // Gone between the two syscalls: an ordinary deletion, not a fault.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => Some(err.to_string()),
    }
}

/// The commit `HEAD` resolves to, or `None` on an unborn branch.
///
/// A freshly initialized repository with no commits is an ordinary state, not
/// an error — every profile starts there.
pub fn head_commit_id(repo: &gix::Repository) -> Result<Option<gix::hash::ObjectId>> {
    let mut head = repo.head().map_err(|err| {
        SyncError::Git(format!(
            "could not read HEAD: {}",
            super::fetch::flatten(&err)
        ))
    })?;
    let id = head.try_peel_to_id().map_err(|err| {
        SyncError::Git(format!(
            "could not peel HEAD: {}",
            super::fetch::flatten(&err)
        ))
    })?;
    Ok(id.map(gix::Id::detach))
}

/// Whether this repository's index holds no entries at all.
///
/// A repository that has never been staged answers `true` and so does one
/// whose index file is missing; both are the same fact and neither is an
/// error. See [`checkout_is_unfinished`] for the condition this is half of.
pub fn index_is_unpopulated(repo: &gix::Repository) -> Result<bool> {
    let index = repo
        .index_or_empty()
        .map_err(|err| SyncError::Git(format!("could not read the index: {err}")))?;
    Ok(index.entries().is_empty())
}

/// How many entries `.git/index` claims, read from its 12-byte header
/// (Story 56.15).
///
/// `0` for an index that is absent, truncated, or not a `DIRC` file at all —
/// all of which mean the same thing to the only caller: this repository has no
/// working copy recorded.
///
/// # Why not `index_or_empty`
///
/// Because the supervisor asks once per second per profile, and
/// [`index_is_unpopulated`] maps and *parses* the whole file to answer: on the
/// folder this was written for that is 155 625 entries and roughly 10 MB, to
/// learn a number the header states in four bytes. The header is `DIRC`, a
/// big-endian version, and a big-endian entry count, and git has never written
/// it otherwise.
///
/// This is a *screen*, never the verdict: a non-zero answer ends the question,
/// and a zero answer sends the caller to [`checkout_is_unfinished`], which
/// opens the repository and asks properly.
pub fn index_entry_count(git_dir: &Path) -> u32 {
    use std::io::Read as _;

    let Ok(mut file) = std::fs::File::open(git_dir.join("index")) else {
        return 0;
    };
    let mut header = [0u8; 12];
    if file.read_exact(&mut header).is_err() || &header[..4] != b"DIRC" {
        return 0;
    }
    u32::from_be_bytes([header[8], header[9], header[10], header[11]])
}

/// Whether this repository is a checkout that never finished: `HEAD` holds a
/// tree with content in it, and the index holds nothing (Story 56.15).
///
/// # Why this exact shape, and why it cannot be anything else
///
/// git never leaves a repository here. A checkout writes the index and the
/// worktree together; a `git rm -r .` writes an index with the removals in it,
/// not an index with no entries. The only ways in are a clone or a checkout
/// killed between the fetch and [`gix::clone::PrepareCheckout::main_worktree`]
/// writing the index, and somebody deleting `.git/index` by hand. Both mean
/// the same thing: **this working copy was never made**.
///
/// It matters because of what the status walk does with it. `gix::status`
/// diffs `HEAD`'s tree against the index before it looks at the worktree at
/// all, so every path in `HEAD` comes back as
/// `gix::diff::index::Change::Deletion` — see [`push_item`], which files those
/// into `RepoStatus::deleted`. On the folder this was written for that is
/// 155 625 deletions per pass, out of an index that could not contribute one,
/// which is exactly the arithmetic the field log shows:
/// `entries=155625 scanned=0 … deleted=155625` — `scanned` counts index
/// entries compared against the worktree, and an empty index compares none.
///
/// The `HEAD` tree is required to be non-empty so that a repository whose
/// first commit is genuinely empty is not accused of anything.
pub fn checkout_is_unfinished(repo: &gix::Repository) -> Result<bool> {
    let Some(head) = head_commit_id(repo)? else {
        // Unborn: nothing has been checked out because nothing exists yet.
        return Ok(false);
    };
    if !index_is_unpopulated(repo)? {
        return Ok(false);
    }
    let commit = repo.find_commit(head).map_err(|err| {
        SyncError::Git(format!(
            "could not read the HEAD commit: {}",
            super::fetch::flatten(&err)
        ))
    })?;
    let tree = commit.tree().map_err(|err| {
        SyncError::Git(format!(
            "could not read the HEAD tree: {}",
            super::fetch::flatten(&err)
        ))
    })?;
    let has_content = tree.iter().next().is_some();
    Ok(has_content)
}

/// What [`restore_missing_checkout`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutRepair {
    /// Paths written into the worktree from `HEAD` because nothing was there.
    pub restored: usize,
    /// Paths left exactly as they were, because something already was.
    pub kept: usize,
    /// Why this repair is not finished, when it is not. `None` means the
    /// index was written and the working copy is whole.
    pub unfinished: Option<String>,
}

/// Finish a checkout that stopped, **writing only what is missing**
/// (Story 56.15).
///
/// The repair for [`checkout_is_unfinished`]: build the index `HEAD` implies,
/// write into the worktree every path that is not there, and save the index —
/// but only once every entry provably has a file behind it.
///
/// # Refuse first, repair second — and why this order is the safety argument
///
/// Repairing is what the owner wants and refusing is what is safe, so this
/// does both, in an order a later caller cannot reverse.
///
/// * The **refusal** ([`super::commit::stage_and_commit`]'s empty-index guard)
///   is unconditional and lives at the one place a commit is made. It does not
///   consult this function and this function cannot switch it off. A repair
///   that half-worked, crashed, or was never reached therefore leaves the
///   refusal standing.
/// * The **repair** only ever ADDS. `destination_is_initially_empty` is set
///   `true`, which makes `gix_worktree_state` open every destination with
///   `create_new` — so a path that already exists fails with `AlreadyExists`
///   and is recorded as a collision rather than truncated. That is the whole
///   safety property, and the OS enforces it rather than a check here: with
///   `destination_is_initially_empty = false` the same call opens with
///   `create(true).truncate(true)` and would overwrite a user's files with
///   whatever `HEAD` happens to hold. There is no freshness filter anywhere in
///   that code path — every entry is written — so `overwrite_existing = false`
///   alone does **not** protect the bytes, whatever its own doc suggests.
/// * The index is written **last, and only when the repair is whole**. An
///   index built from `HEAD` names every path; a worktree that received only
///   half of them would then read the other half as deleted, which is the very
///   catastrophe this exists to prevent. So an interrupt, an IO error, or a
///   collision that is not "a file is already here" all leave the index
///   untouched and the repository exactly as it was — refused, and retried on
///   the next pass.
///
/// The clone that produces this state only ever runs against an EMPTY
/// destination (`Engine::open_repo` checks), so in the ordinary case every
/// file this writes is one keeper's own checkout was supposed to have written
/// from this same commit. A collision means somebody put something there
/// since, and leaving it alone is the only defensible answer.
pub fn restore_missing_checkout(
    repo: &gix::Repository,
    interrupt: &AtomicBool,
) -> Result<CheckoutRepair> {
    let workdir = workdir(repo)?;
    let Some(head) = head_commit_id(repo)? else {
        return Ok(CheckoutRepair {
            restored: 0,
            kept: 0,
            unfinished: Some("this folder has no commits to restore from".to_owned()),
        });
    };
    let tree = repo
        .find_commit(head)
        .map_err(|err| {
            SyncError::Git(format!(
                "could not read the HEAD commit: {}",
                super::fetch::flatten(&err)
            ))
        })?
        .tree_id()
        .map_err(|err| SyncError::Git(format!("could not read the HEAD tree: {err}")))?
        .detach();
    let mut index = repo
        .index_from_tree(&tree)
        .map_err(|err| SyncError::Git(format!("could not rebuild the index from HEAD: {err}")))?;

    // What is already on disk, decided BEFORE the checkout rather than by
    // colliding with it (Story 56.15 follow-up).
    //
    // # Why the order is the whole bug
    //
    // `gix_worktree_state::checkout::entry` filters first and opens the
    // destination second: for a path routed through `filter.lfs.process` it
    // asks the filter for the file's content, and only then calls `open_file`,
    // which fails with `AlreadyExists` when something is already standing
    // there. The `?` on that call drops the filter's response **undrained** —
    // and unlike the delayed path, which sinks it explicitly, the immediate
    // path does not. The filter process is then stuck mid-write on a pipe
    // nobody is reading, the next request blocks writing to its stdin, and
    // both sides sit there.
    //
    // Measured on the owner's `/Users/tgorka/tgdrive`, whose worktree holds
    // 155 112 of `HEAD`'s 155 625 paths, so nearly every entry collided: the
    // checkout emitted thousands of collisions in under a second and then
    // stopped dead for **exactly 900 s** — `lfs::filter::REQUEST_LIMIT`, the
    // watchdog killing the wedged filter — over and over, each stall costing
    // the paths in flight. That is what "52 052 of its files could not be
    // written (Failed to invoke 'smudge' command)" was, and why this folder
    // could not repair itself for three days.
    //
    // Asking `lstat` first costs one syscall per entry and removes the
    // precondition entirely: the filter is never asked for content that is
    // going to be thrown away. It also turns the repair from 155 625 filter
    // round trips into one per genuinely missing file — 513 of them here.
    //
    // `SKIP_WORKTREE` is how the checkout is told to leave an entry alone
    // (`chunk.rs` skips those before it does anything else). The flags are
    // cleared again before the index is written: an index carrying them says
    // "sparse checkout, do not materialize", and a status walk treats such
    // entries as unchanged — which on this folder would mean 155 112 files
    // that silently stop syncing.
    let mut kept = 0usize;
    let mut blocked: Vec<gix::bstr::BString> = Vec::new();
    let mut standing: Vec<usize> = Vec::new();
    for (position, (entry, rela)) in index.entries_mut_with_paths().enumerate() {
        let path = workdir.join(gix::path::from_bstr(rela));
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            // Absent, which is the only thing this repair exists to write.
            continue;
        };
        if metadata.is_file() || metadata.is_symlink() {
            kept += 1;
        } else {
            // A directory where `HEAD` holds a file. The path stays as it is,
            // so an index entry claiming it would be false — and the very next
            // status walk would read that as a deletion.
            blocked.push(rela.into());
        }
        entry.flags |= gix::index::entry::Flags::SKIP_WORKTREE;
        standing.push(position);
    }

    let mut options = repo
        .checkout_options(gix::worktree::stack::state::attributes::Source::IdMapping)
        .map_err(|err| SyncError::Git(format!("could not read checkout options: {err}")))?;
    // The two lines the whole safety argument rests on. See the doc above.
    // They stay even though nothing standing on disk now reaches the checkout:
    // a file created between the `lstat` above and the write below is a race
    // this must still lose safely rather than overwrite.
    options.destination_is_initially_empty = true;
    options.overwrite_existing = false;
    // One failure must not abandon the rest: a repair that stops at the first
    // unreadable path leaves a worktree *more* incomplete than it found, and
    // the errors are counted rather than raised so the decision below is taken
    // over all of them at once.
    options.keep_going = true;

    let objects = repo
        .objects
        .clone()
        .into_arc()
        .map_err(|err| SyncError::io("open the object database", repo.git_dir(), err))?;
    let outcome = gix::worktree::state::checkout(
        &mut index,
        &workdir,
        objects,
        &gix::progress::Discard,
        &gix::progress::Discard,
        interrupt,
        options,
    )
    .map_err(|err| {
        SyncError::Git(format!(
            "could not finish the checkout: {}",
            super::fetch::flatten(&err)
        ))
    })?;

    // A collision can still happen — something created between the `lstat`
    // above and the write here — and it is classified the same way, for the
    // same reason: the index entry about to claim the path is only true if a
    // FILE is what is standing there. A directory where `HEAD` holds a file
    // collides with `AlreadyExists` exactly like a real file does, and an
    // index written over it leaves an entry whose worktree object is a
    // directory, which the very next status walk reads as a deletion.
    for collision in &outcome.collisions {
        let rela: &gix::bstr::BStr = collision.path.as_ref();
        let path = workdir.join(gix::path::from_bstr(rela));
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file() || meta.is_symlink()) {
            kept += 1;
        } else {
            blocked.push(collision.path.clone());
        }
    }
    let unfinished = if interrupt.load(Ordering::Relaxed) {
        Some("it was interrupted before every file was written".to_owned())
    } else if let Some(first) = outcome.errors.first() {
        Some(format!(
            "{} of its files could not be written (first: {}: {})",
            outcome.errors.len(),
            first.path,
            first.error
        ))
    } else {
        blocked.first().map(|first| {
            format!(
                "{} of its paths are blocked by something else on disk (first: {})",
                blocked.len(),
                first
            )
        })
    };
    if unfinished.is_none() {
        // Only the flags this function set, and only now: see the note above
        // for what a written `SKIP_WORKTREE` would mean to the next walk.
        // Positions, because the entries have not been re-sorted — the
        // checkout does not touch their order.
        let entries = index.entries_mut();
        for position in &standing {
            entries[*position]
                .flags
                .remove(gix::index::entry::Flags::SKIP_WORKTREE);
        }
        index
            .write(gix::index::write::Options::default())
            .map_err(|err| SyncError::Git(format!("could not write the restored index: {err}")))?;
    }
    Ok(CheckoutRepair {
        // `files_updated` counts every entry the chunk loop saw, including the
        // ones it skipped for `SKIP_WORKTREE`, so both have to come off it for
        // this to mean "files this repair actually wrote".
        restored: outcome
            .files_updated
            .saturating_sub(standing.len())
            .saturating_sub(outcome.collisions.len()),
        kept,
        unfinished,
    })
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
    let mut config = read_config(&config_path, gix::config::Source::Local)?;
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

    write_config_atomically(&config, &config_path)?;

    // Reopen so the handle sees the remote and HEAD just written.
    open(root, false)
}

/// Everything `gix::init` writes into a fresh `.git`, and nothing else.
///
/// Taken from `gix::create::into`, which lays them down in this order: `info/`,
/// `hooks/`, `objects/`, `refs/`, then `HEAD`, `description` and `config`. A
/// `.git` holding any name outside this list got further than an init did,
/// whatever else is wrong with it.
const INIT_SCAFFOLD: &[&str] = &[
    "HEAD",
    "config",
    "description",
    "hooks",
    "info",
    "objects",
    "refs",
];

/// Discard a `.git` that keeper began creating and never finished, so the
/// caller can create it properly. Returns whether it removed one.
///
/// [`adopt`] starts with `gix::init`, and `gix::init` is a sequence of
/// filesystem steps, not an atomic one: it writes `info/`, `hooks/`,
/// `objects/` and `refs/` before it writes `HEAD`, and `config` after that. A
/// `SIGKILL` inside that sequence leaves a `.git` directory that exists and is
/// not a repository. From then on [`crate::engine::Engine`] takes its
/// "repository already exists" branch — precisely *because* `.git` exists —
/// never calls `adopt` again, and fails every single sync forever with
/// `does not appear to be a git repository` or `.git/config could not be
/// read`. The folder is stranded exactly as an abandoned reference lock
/// strands it, and for the same reason: nothing on the recovery path knows
/// this state exists. Found by the durability matrix, which reproduces it
/// whenever a kill lands in the first few milliseconds of the very first sync.
///
/// # This finishes keeper's own init; it never repairs a repository
///
/// The distinction is the whole safety argument, because being wrong here
/// destroys somebody's history. Removing a `.git` is only defensible when
/// there is provably nothing in it to lose, so that is what is checked rather
/// than assumed: every name at the top level must be one `gix::init` itself
/// writes, and `refs/` and `objects/` — the only two places history can be —
/// must contain nothing but the empty directories it leaves behind. One
/// reference, one object, one `index`, one `logs/`, one `packed-refs`, a
/// `modules/`, a `worktrees/`, a `.git` that is a file pointing into somebody
/// else's repository: any of them and this refuses, loudly, and the caller's
/// original error stands. A stranded folder that says so is recoverable by a
/// human. A deleted history is not.
///
/// The user's own files are never involved either way: they live in the
/// working tree, and only `.git` is touched.
pub fn discard_unfinished_init(git_dir: &Path) -> Result<bool> {
    if let Some(evidence) = signs_of_real_history(git_dir) {
        tracing::warn!(
            git_dir = %git_dir.display(),
            evidence,
            "refusing to re-create a repository that is not an unfinished init; \
             this folder needs a human"
        );
        return Ok(false);
    }
    // Logged before the removal, not after: if this is ever the wrong call,
    // the line that says what was about to happen is the only evidence left.
    tracing::warn!(
        git_dir = %git_dir.display(),
        "discarding a half-made `.git` that provably holds no history"
    );
    std::fs::remove_dir_all(git_dir)
        .map_err(|source| SyncError::io("discard an unfinished repository", git_dir, source))?;
    Ok(true)
}

/// The first sign that `git_dir` holds something a finished repository would
/// hold, or `None` when there is provably nothing in it to lose.
///
/// Anything that cannot be established counts as a sign. "I could not read it"
/// and "there is nothing there" must never collapse into the same answer when
/// the answer authorizes a deletion.
fn signs_of_real_history(git_dir: &Path) -> Option<String> {
    match std::fs::symlink_metadata(git_dir) {
        Ok(metadata) if metadata.is_dir() => {}
        // A `.git` that is a file is a linked worktree or a submodule pointing
        // into another repository, and a symlink is somebody's deliberate
        // arrangement. Neither is an init of ours that stopped halfway.
        Ok(_) => return Some("`.git` is not a directory".to_owned()),
        Err(err) => return Some(format!("`.git` could not be read: {err}")),
    }

    let entries = match std::fs::read_dir(git_dir) {
        Ok(entries) => entries,
        Err(err) => return Some(format!("`.git` could not be listed: {err}")),
    };
    for entry in entries {
        let Ok(entry) = entry else {
            return Some("`.git` holds an entry that could not be read".to_owned());
        };
        match entry.file_name().to_str() {
            Some(name) if INIT_SCAFFOLD.contains(&name) => {}
            Some(name) => return Some(format!("`.git/{name}` is present")),
            None => return Some("`.git` holds an entry with a non-UTF-8 name".to_owned()),
        }
    }

    // `gix::init` leaves both of these holding empty directories and nothing
    // else, so a single entry in either is the entire answer.
    for place in ["refs", "objects"] {
        if let Some(found) = first_entry_under(&git_dir.join(place)) {
            return Some(found);
        }
    }
    None
}

/// Describe the first non-directory anywhere under `dir`, or `None` if it holds
/// only (possibly nested) empty directories.
///
/// A directory that is absent is not a sign — a kill early enough that
/// `gix::init` never created it leaves nothing behind either. A directory that
/// cannot be read *is* a sign, for the reason above.
fn first_entry_under(dir: &Path) -> Option<String> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Some(format!("{} could not be read: {err}", current.display())),
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return Some(format!("{} holds an unreadable entry", current.display()));
            };
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(entry.path()),
                _ => return Some(format!("{} exists", entry.path().display())),
            }
        }
    }
    None
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
    let mut config = read_config(&config_path, gix::config::Source::Local)?;
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

    write_config_atomically(&config, &config_path)?;
    Ok(true)
}

/// Every path the index tracks, repository-relative.
///
/// Used to find checked-out LFS pointers that still need materializing: only a
/// tracked path can hold one, so this bounds that scan to the index rather than
/// walking the whole worktree.
///
/// **Byte-exact.** This used to build each path with `BStr::to_string`, which
/// is a lossy UTF-8 decode: a tracked file whose name is not valid UTF-8 came
/// back as a path with `U+FFFD` in it, which names a different file or no file
/// at all — so the materialization scan silently skipped it, or, in a folder
/// that also held a file genuinely named with `U+FFFD`, stat'd and rewrote the
/// wrong one. [`gix::path::from_bstr`] is the conversion [`to_path`] already
/// uses for status output and it keeps the bytes (Story 47.2).
pub fn tracked_paths(repo: &gix::Repository) -> Result<Vec<PathBuf>> {
    let index = repo
        .index_or_empty()
        .map_err(|err| SyncError::Git(format!("could not read the index: {err}")))?;
    Ok(index
        .entries()
        .iter()
        .map(|entry| to_path(entry.path(&index)))
        .collect())
}

/// Tracked paths whose worktree file could still be a checked-out pointer.
///
/// [`tracked_paths`] bounds the materialization sweep to the index, and that was
/// enough while the answer was cheap to use. It is not: the caller stats — and
/// for anything small enough, reads — every path it is handed, so on a folder of
/// 154,765 entries on a USB volume the sweep costs about ten minutes of
/// filesystem I/O to find the handful of pointers actually waiting. That runs
/// inside the profile's one-operation-at-a-time reservation, so for those ten
/// minutes nothing else about the folder can happen at all.
///
/// The index already knows the answer. Its entries carry the `stat` git recorded
/// at checkout, and a checked-out pointer is a file of at most
/// [`crate::lfs::pointer::MAX_POINTER_BYTES`]; anything larger on disk is
/// content, not a stub. So the size the index remembers is a filter that costs
/// no I/O whatsoever, and one that cannot lose a pointer: a pointer only ever
/// reaches the worktree through a checkout, and a checkout is exactly the moment
/// git records its size here.
///
/// A materialized file whose stat has been refreshed to its real length is
/// therefore excluded, which is the entire point — it is also, by definition,
/// the file that no longer needs materializing.
pub fn pointer_sized_tracked_paths(repo: &gix::Repository, max_bytes: u32) -> Result<Vec<PathBuf>> {
    let index = repo
        .index_or_empty()
        .map_err(|err| SyncError::Git(format!("could not read the index: {err}")))?;
    Ok(index
        .entries()
        .iter()
        .filter(|entry| entry.stat.size <= max_bytes)
        .map(|entry| to_path(entry.path(&index)))
        .collect())
}

/// Tracked paths git is carrying that keeper cannot spell (Story 47.2).
///
/// The index is the only complete inventory of a repository that costs no
/// filesystem walk — it is already resident — and it is the one place a file
/// that was committed long ago and has not changed since can still be seen. A
/// status walk cannot answer this: a tracked, unmodified file is exactly the
/// case that reports nothing, and it was the case the owner hit.
///
/// Allocates only for the offenders. [`crate::names::UnspellableName::of_bytes`]
/// answers `None` after a UTF-8 validation and nothing else, so a repository
/// with a hundred thousand ordinary paths pays one scan of the index's own
/// bytes and builds no strings at all.
pub fn unspellable_tracked_paths(
    repo: &gix::Repository,
) -> Result<Vec<crate::names::UnspellableName>> {
    let index = repo
        .index_or_empty()
        .map_err(|err| SyncError::Git(format!("could not read the index: {err}")))?;
    let mut out: Vec<crate::names::UnspellableName> = index
        .entries()
        .iter()
        .filter_map(|entry| crate::names::UnspellableName::of_bytes(entry.path(&index)))
        .collect();
    // The index is sorted by raw path bytes, which is not the order the
    // escaped renderings sort in; a report that reshuffles between polls reads
    // as churn. Dedup because one path can hold several unmerged stages.
    out.sort();
    out.dedup();
    Ok(out)
}

/// Restore the pointer-blob / worktree-stat invariant across the whole index.
///
/// # The failure this repairs
///
/// [`refresh_index_stat`] is called in exactly one place: immediately after a
/// materialization, on the paths that materialization touched. That is correct
/// and it is not enough. A run that dies between writing the real files and
/// refreshing their stat — the `Too many open files` exhaustion that a
/// Finder-launched app hit for days is one way — leaves the index describing
/// pointers and the worktree holding gigabytes. Nothing ever calls it again for
/// those paths, so the invariant stays broken for the life of the folder.
///
/// What that costs is not cosmetic. Every entry whose stat disagrees is a file
/// `status` must convert through the LFS filter to compare, so a routine status
/// pass turns into streaming the entire worktree through a filter pipeline —
/// which is how one vault reached 93 GB of conversions and stopped syncing
/// entirely.
///
/// # Why it re-stats rather than re-checks out
///
/// The bytes on disk are right; only the index's memory of them is stale. A
/// checkout would re-materialize files that are already correct, cost the
/// transfer again, and touch mtimes the user may be relying on. Re-stat is the
/// smaller and truer repair: it changes what git *remembers*, not what the user
/// *has*.
///
/// Answers how many entries it corrected, so a caller can say whether there was
/// anything to repair — "nothing was wrong" and "I fixed 500 files" are
/// different sentences and a button that cannot tell them apart teaches nothing.
pub fn repair_index_stat(repo: &gix::Repository) -> Result<usize> {
    let paths = tracked_paths(repo)?;
    let workdir = workdir(repo)?;
    // Only the entries whose worktree file is NOT a pointer: those are the
    // materialized ones, and they are the only ones whose stat can have been
    // left describing something else. An entry that still holds a pointer on
    // disk is already consistent and re-stating it would be a write for
    // nothing.
    let stale: Vec<PathBuf> = paths
        .into_iter()
        .filter(|rel| {
            let full = workdir.join(rel);
            let Ok(meta) = std::fs::symlink_metadata(&full) else {
                return false;
            };
            if !meta.is_file() {
                return false;
            }
            // A pointer is tiny by construction. Anything larger cannot be one,
            // and reading the head of every small file is cheap next to the
            // conversion this exists to avoid.
            if meta.len() >= MAX_POINTER_BYTES as u64 {
                return true;
            }
            let Ok(head) = std::fs::read(&full) else {
                return false;
            };
            Pointer::parse(&head).is_none()
        })
        .collect();
    let corrected = stale.len();
    refresh_index_stat(repo, &stale)?;
    Ok(corrected)
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
        .map_err(|err| {
            SyncError::Git(format!(
                "could not write the index: {}",
                super::fetch::flatten(&err)
            ))
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    /// The watchdog fires on STILLNESS, not on duration and not on silence.
    ///
    /// This is the whole distinction the field failures turned on, twice. A
    /// status pass converting a gigabyte of video is slow and healthy; a pass
    /// scanning a clean tree is *silent* and healthy; only a pass that has
    /// stopped doing both is stuck. A guard that could not tell them apart
    /// would either kill honest work or keep waiting on a deadlock — both worse
    /// than no guard at all, because both look like a decision.
    const FAST_POLL: Duration = Duration::from_millis(20);
    const FAST_LIMIT: Duration = Duration::from_millis(200);

    /// A fresh counter of index entries compared, for the tests that drive one
    /// by hand.
    fn scan_counter() -> gix::progress::StepShared {
        Arc::new(AtomicUsize::new(0))
    }

    #[test]
    fn a_walk_that_keeps_producing_is_never_interrupted() {
        let interrupt = Arc::new(AtomicBool::new(false));
        let heartbeat = Arc::new(AtomicU64::new(0));
        let watchdog = StatusWatchdog::arm_with(
            Arc::clone(&interrupt),
            Arc::clone(&heartbeat),
            scan_counter(),
            None,
            FAST_POLL,
            FAST_LIMIT,
        );

        // Several times the silence limit, spent beating slowly enough that a
        // duration-based guard would have fired long ago.
        let until = Instant::now() + FAST_LIMIT * 4;
        while Instant::now() < until {
            watchdog.beat();
            std::thread::sleep(FAST_POLL / 2);
        }

        assert!(
            !interrupt.load(Ordering::Relaxed),
            "a walk that is producing items is alive, however long it takes"
        );
        assert!(watchdog.beats() > 0, "the beats are the item count");
    }

    /// A walk with nothing left to report is still a walk.
    ///
    /// The regression this pins is the one that killed 61 consecutive healthy
    /// passes on a 155 662-entry folder: emission stopped when the dirty
    /// entries ran out, roughly 1 400 s before the pass would have finished,
    /// and the guard read that as a deadlocked filter. Nothing beats here at
    /// all — only gix's own count of entries compared moves.
    /// One 5 GB file converting is not a stall (2026-08-28).
    ///
    /// The field shape this exists for: the walk reaches a 4.3-5.3 GB LFS entry,
    /// keeper's filter streams it at ~9.6 MB/s, and for the 450-550 s that takes
    /// gix emits nothing and increments no entry counter, because it counts an
    /// entry *after* its conversion. Eight consecutive passes on that folder
    /// were abandoned with 600 s of "silence" while the filter was writing the
    /// whole time, so the objects never published and the folder never drained.
    ///
    /// Here the only thing moving is bytes in the scratch directory — no beats,
    /// no scans, exactly as production looked.
    #[test]
    fn a_filter_streaming_one_huge_file_is_not_a_stall() {
        let dir = tempfile::tempdir().expect("tempdir");
        let scratch = dir.path().join("lfs-tmp");
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let blob = scratch.join("keeper-lfs-abc123");
        std::fs::write(&blob, b"").expect("open the scratch file");

        let interrupt = Arc::new(AtomicBool::new(false));
        let heartbeat = Arc::new(AtomicU64::new(0));
        let _watchdog = StatusWatchdog::arm_with(
            Arc::clone(&interrupt),
            Arc::clone(&heartbeat),
            scan_counter(),
            Some(scratch.clone()),
            FAST_POLL,
            FAST_LIMIT,
        );

        // Several times the silence limit, spent only growing the temp file.
        let until = Instant::now() + FAST_LIMIT * 4;
        let mut written = 0u64;
        while Instant::now() < until {
            written += 1;
            std::fs::write(&blob, vec![b'x'; written as usize * 512]).expect("grow the scratch");
            std::thread::sleep(FAST_POLL / 2);
        }

        assert!(
            !interrupt.load(Ordering::Relaxed),
            "a filter writing bytes is working, however long one file takes"
        );
    }

    /// And the guard still fires when the scratch directory is there but nothing
    /// is being written into it — a filter that deadlocked rather than one that
    /// is slow. Same fixture as the test above, minus the writing.
    ///
    /// Waits for the flag rather than sleeping a fixed span, the way
    /// [`a_walk_that_goes_still_is_interrupted`] does. Sleeping `FAST_LIMIT * 3`
    /// asserted something stronger than the guard promises: not merely that
    /// stillness sets the flag, but that the watchdog thread is SCHEDULED
    /// promptly enough to notice within 600 ms of wall clock. On a CI runner
    /// carrying four thousand tests at once it is not, and this test was the one
    /// red check on `main`. The assertion below is unchanged and no weaker — a
    /// guard that never fires still fails, five seconds later.
    #[test]
    fn a_dead_filter_with_a_scratch_dir_is_still_interrupted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let scratch = dir.path().join("lfs-tmp");
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        std::fs::write(scratch.join("keeper-lfs-abc123"), vec![b'x'; 4096]).expect("scratch file");

        let interrupt = Arc::new(AtomicBool::new(false));
        let _watchdog = StatusWatchdog::arm_with(
            Arc::clone(&interrupt),
            Arc::new(AtomicU64::new(0)),
            scan_counter(),
            Some(scratch),
            FAST_POLL,
            FAST_LIMIT,
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !interrupt.load(Ordering::Relaxed) {
            std::thread::sleep(FAST_POLL);
        }

        assert!(
            interrupt.load(Ordering::Relaxed),
            "a scratch file that stopped growing is a stalled filter, not work"
        );
    }

    #[test]
    fn a_walk_that_only_scans_is_never_interrupted() {
        let interrupt = Arc::new(AtomicBool::new(false));
        let scanned = scan_counter();
        let watchdog = StatusWatchdog::arm_with(
            Arc::clone(&interrupt),
            Arc::new(AtomicU64::new(0)),
            Arc::clone(&scanned),
            None,
            FAST_POLL,
            FAST_LIMIT,
        );

        let until = Instant::now() + FAST_LIMIT * 4;
        while Instant::now() < until {
            scanned.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(FAST_POLL / 2);
        }

        assert!(
            !interrupt.load(Ordering::Relaxed),
            "a pass comparing index entries is working, even when every one of \
             them is clean and it therefore emits nothing"
        );
        assert!(
            watchdog.scans() > 0,
            "the scan count is what proved it alive"
        );
        assert_eq!(watchdog.beats(), 0, "and it did so with no item emitted");
    }

    /// The atomic gix counts into MUST be the one the watchdog reads.
    ///
    /// This is the half a unit test of the loop cannot reach, and the half that
    /// silently does nothing when it is wrong. `gix::progress::Discard` — what
    /// this walk passed before — implements `counter()` as
    /// `Arc::new(AtomicUsize::default())`, a FRESH handle per call: gix counts
    /// diligently, into an atomic nobody else holds. Wired that way the guard
    /// still compiles, still fires, and its new liveness signal is simply
    /// always zero. So the number is asserted end to end, against real gix,
    /// over a real repository.
    #[test]
    fn gix_counts_index_entries_into_the_atomic_the_watchdog_reads() {
        let (_dir, repo) = repo_with_two_files();
        let scanned = scan_counter();

        let platform = repo
            .status(ScannedEntries {
                counter: Arc::clone(&scanned),
            })
            .expect("status platform");
        for item in platform
            .into_iter(Vec::<gix::bstr::BString>::new())
            .expect("status iterator")
        {
            item.expect("status item");
        }

        assert_eq!(
            scanned.load(Ordering::Relaxed),
            2,
            "both committed entries were compared and neither was counted where \
             the watchdog could see it"
        );
    }

    /// The behaviour the whole change exists for: a walk that stops moving is
    /// abandoned rather than waited on. Without this the guard is decoration.
    #[test]
    fn a_walk_that_goes_still_is_interrupted() {
        let interrupt = Arc::new(AtomicBool::new(false));
        let heartbeat = Arc::new(AtomicU64::new(0));
        let watchdog = StatusWatchdog::arm_with(
            Arc::clone(&interrupt),
            Arc::clone(&heartbeat),
            scan_counter(),
            None,
            FAST_POLL,
            FAST_LIMIT,
        );
        // One item, then nothing at all — the shape of a deadlocked conversion:
        // no further emission AND no further entry compared.
        watchdog.beat();

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !interrupt.load(Ordering::Relaxed) {
            std::thread::sleep(FAST_POLL);
        }

        assert!(
            interrupt.load(Ordering::Relaxed),
            "stillness past the limit has to set the flag gix polls, or the walk \
             hangs exactly as it did in the field"
        );
    }

    /// The counter is the item count, and the caller needs no second one.
    #[test]
    fn a_beat_answers_how_many_have_been_seen() {
        let watchdog = StatusWatchdog::arm(
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
            scan_counter(),
            None,
        );
        assert_eq!(watchdog.beat(), 1);
        assert_eq!(watchdog.beat(), 2);
        assert_eq!(watchdog.beats(), 2);
    }

    /// The filter that keeps the materialization sweep off the filesystem.
    ///
    /// A pointer is at most `MAX_POINTER_BYTES` on disk, and git records what it
    /// checked out, so the index alone separates "might still be a stub" from
    /// "is content". Handing the caller every tracked path instead cost about
    /// ten minutes of stats and reads per sweep on a 154,765-entry folder on a
    /// USB volume — inside the profile's reservation, with ready work queued
    /// behind it.
    #[test]
    fn only_pointer_sized_entries_are_offered_as_smudge_candidates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let git = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .expect("git")
                    .success(),
                "git {args:?}"
            );
        };
        git(&["init", "-q", "-b", "main"]);
        // A stub the size a pointer is, and content the size content is.
        std::fs::write(root.join("stub.mp4"), vec![b'x'; 130]).expect("write");
        std::fs::write(root.join("content.mp4"), vec![b'y'; 8_192]).expect("write");
        git(&["add", "-A"]);

        let repo = open(root, false).expect("open");
        let candidates = pointer_sized_tracked_paths(&repo, 1_024).expect("candidates");

        assert_eq!(
            candidates,
            vec![PathBuf::from("stub.mp4")],
            "only the entry small enough to be a pointer is worth a stat"
        );
        assert_eq!(
            tracked_paths(&repo).expect("tracked").len(),
            2,
            "and the unfiltered inventory still holds both, so this is a filter and not a loss"
        );
    }

    /// A walk that was abandoned must not be mistaken for one that finished.
    ///
    /// The regression this pins cost nothing only because it was caught on the
    /// guard's first real run: gix ends the iterator on interrupt instead of
    /// erroring, so the partial result reached the caller looking complete.
    #[test]
    fn an_abandoned_walk_is_refused_rather_than_returned_short() {
        let watchdog = StatusWatchdog::arm_with(
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(19)),
            scan_counter(),
            None,
            FAST_POLL,
            Duration::from_secs(3600),
        );
        let partial = RepoStatus {
            deleted: vec![PathBuf::from("a"), PathBuf::from("b")],
            ..RepoStatus::default()
        };

        let interrupted = AtomicBool::new(true);
        let refused = finish_walk(&interrupted, &watchdog, partial.clone());
        assert!(
            refused.is_err(),
            "a truncated status offered as a whole one is how a stall becomes a \
             commit of deletions nobody made"
        );

        let ran = AtomicBool::new(false);
        let kept = finish_walk(&ran, &watchdog, partial).expect("a completed walk is returned");
        assert_eq!(kept.deleted.len(), 2, "an honest walk keeps its findings");
    }

    /// Dropping the guard tells the watcher to stop, so a finished walk leaves
    /// no thread behind polling atomics for ten minutes.
    #[test]
    fn a_finished_walk_releases_its_watcher() {
        let heartbeat = Arc::new(AtomicU64::new(0));
        {
            let _watchdog = StatusWatchdog::arm(
                Arc::new(AtomicBool::new(false)),
                Arc::clone(&heartbeat),
                scan_counter(),
                None,
            );
        }
        assert_eq!(
            heartbeat.load(Ordering::Relaxed),
            u64::MAX,
            "the sentinel is how the watcher learns the walk is over"
        );
    }

    /// The sentence a stalled scan produces has to name what was reached.
    ///
    /// "status failed" cannot tell "stuck on the first file" from "stuck on the
    /// last one", and that difference is the whole of the next investigation.
    #[test]
    fn the_refusal_names_what_it_got_through() {
        let scanned = scan_counter();
        scanned.store(41_000, Ordering::Relaxed);
        let watchdog = StatusWatchdog::arm(
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
            Arc::clone(&scanned),
            None,
        );
        let message = watchdog.silence_error(4_217).to_string();

        assert!(
            message.contains("4217"),
            "the count is the diagnosis: {message}"
        );
        assert!(
            message.contains("41000"),
            "and so is how far into the index it got, which on a mostly-clean \
             tree is the only number that separates 'stuck at the start' from \
             'nearly done': {message}"
        );
        assert!(
            message.contains("stopped responding"),
            "it has to say the scan stopped, not that it failed: {message}"
        );
        assert!(
            message.contains("try again"),
            "and that the folder is not lost: {message}"
        );
    }

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
        // Every repository open runs both of these, and the overwhelmingly
        // common case is that there is no lock at all.
        let dir = tempfile::tempdir().expect("tempdir");
        release_stale_index_lock(dir.path());
        release_stale_ref_locks(dir.path());
    }

    /// Plant the debris a `SIGKILL` inside a reference transaction leaves, at
    /// each of the three places gitoxide can leave it.
    fn plant_ref_locks(git_dir: &std::path::Path) -> Vec<PathBuf> {
        let locks = [
            git_dir.join("HEAD.lock"),
            git_dir.join("refs/heads/main.lock"),
            git_dir.join("refs/remotes/origin/main.lock"),
        ];
        for lock in &locks {
            let parent = lock.parent().expect("a lock always has a parent");
            std::fs::create_dir_all(parent).expect("reference directory");
            // 41 bytes, the shape gitoxide writes: an object id and a newline.
            std::fs::write(lock, b"9e2f04c0c1a70cb9e0e2b2d2f4a8e6c3d1b5a7f9\n")
                .expect("plant the lock");
        }
        locks.into_iter().collect()
    }

    #[test]
    fn a_reference_lock_left_by_a_killed_run_is_released() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path();
        let locks = plant_ref_locks(git_dir);
        // A real reference beside each one: recovery removes the debris and
        // must not so much as glance at what it was shadowing.
        let reference = git_dir.join("refs/heads/main");
        std::fs::write(&reference, b"kept\n").expect("write the reference");

        release_ref_locks_unheld_for(git_dir, Duration::ZERO);

        for lock in &locks {
            assert!(
                !lock.exists(),
                "a lock nobody holds must be released: {}",
                lock.display()
            );
        }
        assert_eq!(
            std::fs::read(&reference).expect("the reference must survive"),
            b"kept\n",
            "recovery removes locks, never references"
        );
    }

    #[test]
    fn a_reference_lock_is_watched_before_it_is_broken() {
        // The property that keeps this from degenerating into "delete every
        // `.lock`": a lock is only debris once it has been observed doing
        // nothing for the whole window, so the call cannot return sooner.
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path();
        let lock = git_dir.join("HEAD.lock");
        std::fs::write(&lock, b"held\n").expect("plant the lock");

        let window = Duration::from_millis(300);
        let started = Instant::now();
        release_ref_locks_unheld_for(git_dir, window);

        assert!(
            started.elapsed() >= window,
            "a lock must be watched for the whole window, not deleted on sight"
        );
        assert!(!lock.exists(), "a lock nobody touched must be released");
    }

    #[test]
    fn a_reference_lock_a_live_writer_is_using_is_left_alone() {
        // The direction that matters most. A human running `git commit` in a
        // synced folder holds this exact file, and breaking it would corrupt
        // their write to fix a problem nobody has. A writer that is working
        // touches its lock; that is the signal, and it must win.
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path();
        let lock = git_dir.join("HEAD.lock");
        std::fs::write(&lock, b"w").expect("plant the lock");

        let writing = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let writer = {
            let lock = lock.clone();
            let writing = std::sync::Arc::clone(&writing);
            std::thread::spawn(move || {
                let mut written = 1usize;
                while writing.load(Ordering::Relaxed) {
                    written += 1;
                    // Growing the file changes both halves of the identity, so
                    // this cannot pass by a filesystem with a coarse clock.
                    let _ = std::fs::write(&lock, vec![b'w'; written]);
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
        };

        // A window far longer than the writer's cadence: the call still has to
        // give up on it, and give up early.
        let started = Instant::now();
        release_ref_locks_unheld_for(git_dir, Duration::from_secs(30));
        let waited = started.elapsed();
        writing.store(false, Ordering::Relaxed);
        writer.join().expect("the writer thread");

        assert!(
            lock.exists(),
            "a lock somebody is still writing must survive"
        );
        assert!(
            waited < Duration::from_secs(5),
            "proof of a live writer must end the watch at once, not run it out; waited {waited:?}"
        );
    }

    #[test]
    fn a_reference_lock_that_is_let_go_mid_watch_is_not_chased() {
        // The ordinary contention case: keeper opened the repository while
        // somebody else's reference update was in flight. It completes, its
        // lock disappears, and there is nothing to recover — least of all a
        // lock the *next* writer took in the meantime.
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path();
        let lock = git_dir.join("refs/heads/main.lock");
        std::fs::create_dir_all(lock.parent().expect("parent")).expect("reference directory");
        std::fs::write(&lock, b"first\n").expect("plant the lock");

        let releasing = {
            let lock = lock.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(60));
                std::fs::remove_file(&lock).expect("the writer releases its lock");
                std::thread::sleep(Duration::from_millis(40));
                std::fs::write(&lock, b"second\n").expect("a second writer takes it");
            })
        };

        release_ref_locks_unheld_for(git_dir, Duration::from_secs(30));
        releasing.join().expect("the writer thread");

        assert!(
            lock.exists(),
            "the second writer's lock must not be collected by a watch that \
             started before it existed"
        );
    }

    #[test]
    fn a_packed_refs_lock_is_never_touched() {
        // Deliberate scope, pinned so it survives a tidy-up. `git gc` and
        // `git pack-refs` hold this one while rewriting every reference in the
        // repository, which is not bounded by the fraction of a second that
        // justifies the window above, and keeper's own paths never create it.
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path();
        let lock = git_dir.join("packed-refs.lock");
        std::fs::write(&lock, b"").expect("plant the lock");

        release_ref_locks_unheld_for(git_dir, Duration::ZERO);

        assert!(
            lock.exists(),
            "packed-refs is out of scope and must be left exactly as it was"
        );
    }

    #[test]
    fn a_commit_is_refused_while_the_reference_it_would_move_is_locked() {
        // Not an optimization. A held branch lock does not fail
        // `commit_as`, it hangs it on a full core indefinitely (gix-ref
        // 0.66.0, `prepare.rs:398-410`), so refusing to enter the transaction
        // is the only bound available while that lives upstream.
        let (dir, repo) = repo_with_two_files();
        ensure_head_unlocked(&repo).expect("an unlocked repository must be usable");

        let branch = repo
            .head_name()
            .expect("head")
            .expect("a born branch")
            .as_bstr()
            .to_string();
        let branch_lock = repo.git_dir().join(format!("{branch}.lock"));
        std::fs::write(&branch_lock, b"").expect("plant the branch lock");
        assert!(
            ensure_head_unlocked(&repo).is_err(),
            "a locked branch must stop the commit before gitoxide is called"
        );
        std::fs::remove_file(&branch_lock).expect("the writer finishes");
        ensure_head_unlocked(&repo).expect("a released lock must unblock the commit");

        let head_lock = repo.git_dir().join("HEAD.lock");
        std::fs::write(&head_lock, b"").expect("plant the HEAD lock");
        assert!(
            ensure_head_unlocked(&repo).is_err(),
            "a locked HEAD must stop the commit too"
        );
        drop(dir);
    }

    /// The `.git` a kill leaves when it lands inside `gix::init`: every
    /// directory and template that init writes before `HEAD`, and nothing else.
    fn unfinished_init(root: &std::path::Path) -> PathBuf {
        let git_dir = root.join(".git");
        for dir in [
            "info",
            "hooks",
            "objects/info",
            "objects/pack",
            "refs/heads",
            "refs/tags",
        ] {
            std::fs::create_dir_all(git_dir.join(dir)).expect("init scaffold");
        }
        std::fs::write(git_dir.join("info/exclude"), b"").expect("info/exclude");
        std::fs::write(git_dir.join("hooks/pre-commit.sample"), b"#!/bin/sh\n").expect("a hook");
        git_dir
    }

    #[test]
    fn a_repository_holding_history_is_never_discarded() {
        // The test that matters most: being wrong here deletes somebody's
        // work. This is a real repository wearing the exact symptom an
        // unfinished init wears — `.git` exists and will not open — and the
        // only thing that may tell them apart is what is inside.
        let (dir, repo) = repo_with_two_files();
        let git_dir = repo.git_dir().to_path_buf();
        drop(repo);
        std::fs::remove_file(git_dir.join("HEAD")).expect("break the repository");

        assert!(
            !discard_unfinished_init(&git_dir).expect("classify"),
            "a repository with commits in it must never be discarded"
        );
        assert!(
            git_dir.join("objects").exists() && git_dir.join("refs/heads").exists(),
            "the refusal must leave every byte where it was"
        );
        drop(dir);
    }

    #[test]
    fn every_sign_of_a_finished_repository_refuses_the_discard() {
        // One sign each, planted into an otherwise pristine half-made `.git`.
        // Each is something `gix::init` never writes, so each means somebody
        // — git, keeper or a person — got further than an init did here.
        for sign in [
            "index",
            "packed-refs",
            "ORIG_HEAD",
            "logs/HEAD",
            "refs/heads/main",
            "refs/remotes/origin/main",
            "objects/ab/cdef0123456789",
            "objects/pack/pack-abc.pack",
            "modules/vendored/config",
            "worktrees/other/HEAD",
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let git_dir = unfinished_init(dir.path());
            let planted = git_dir.join(sign);
            std::fs::create_dir_all(planted.parent().expect("a parent")).expect("sign directory");
            std::fs::write(&planted, b"x").expect("plant the sign");

            assert!(
                !discard_unfinished_init(&git_dir).expect("classify"),
                "{sign} must stop the discard"
            );
            assert!(planted.exists(), "{sign} must still be there afterwards");
        }
    }

    #[test]
    fn a_git_file_pointing_at_another_repository_is_never_discarded() {
        // A linked worktree or a submodule: `.git` is a file naming somebody
        // else's object store, and removing it detaches a repository that is
        // working perfectly well.
        let dir = tempfile::tempdir().expect("tempdir");
        let git_dir = dir.path().join(".git");
        std::fs::write(&git_dir, b"gitdir: ../elsewhere/.git/worktrees/here\n")
            .expect("write the gitdir pointer");

        assert!(
            !discard_unfinished_init(&git_dir).expect("classify"),
            "a gitdir pointer is not an unfinished init"
        );
        assert!(git_dir.exists(), "the pointer must survive");
    }

    #[test]
    fn an_init_that_never_finished_is_discarded_and_the_worktree_is_untouched() {
        // Both halves of the window `gix::init` leaves open: killed before it
        // wrote `HEAD`, and killed after `HEAD` but before `config`. Either
        // way there is no history, and leaving the directory in place strands
        // the folder for good.
        for head in [None, Some(&b"ref: refs/heads/main\n"[..])] {
            let dir = tempfile::tempdir().expect("tempdir");
            let theirs = dir.path().join("their-work.txt");
            std::fs::write(&theirs, b"never keeper's to delete").expect("worktree file");
            let git_dir = unfinished_init(dir.path());
            if let Some(head) = head {
                std::fs::write(git_dir.join("HEAD"), head).expect("HEAD");
            }

            assert!(
                discard_unfinished_init(&git_dir).expect("classify"),
                "a half-made repository must be cleared so it can be made properly"
            );
            assert!(!git_dir.exists(), "the half-made directory must be gone");
            assert_eq!(
                std::fs::read(&theirs).expect("the user's file must survive"),
                b"never keeper's to delete",
                "only `.git` is ever touched"
            );
        }
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

    /// The load-bearing safety property of the repair (Story 56.15): it writes
    /// what is MISSING and never a byte over what is there.
    ///
    /// The owner's folder holds 16 GB beside an index that holds nothing. A
    /// repair that re-checked-out the whole tree would replace every one of
    /// those files with whatever `HEAD` happens to carry — which is not a
    /// deletion, but is the same class of loss, and it is what
    /// `gix_worktree_state::checkout` does by default: there is no freshness
    /// filter in that path, and `overwrite_existing = false` alone still opens
    /// with `create(true).truncate(true)`.
    #[test]
    fn a_repair_writes_what_is_missing_and_overwrites_nothing() {
        let (dir, _fixture) = repo_with_two_files();
        // The interrupted-checkout state, plus the thing that makes it
        // dangerous: one tracked path holds bytes that are NOT what HEAD says.
        std::fs::remove_file(dir.path().join(".git/index")).expect("drop the index");
        std::fs::remove_file(dir.path().join("b.txt")).expect("drop one worktree file");
        std::fs::write(dir.path().join("a.txt"), "the owner's own bytes").expect("diverge a");
        let repo = open(dir.path(), true).expect("reopen without an index");
        assert!(checkout_is_unfinished(&repo).expect("classify"));

        let interrupt = AtomicBool::new(false);
        let repair = restore_missing_checkout(&repo, &interrupt).expect("repair");
        assert_eq!(
            repair.unfinished, None,
            "a plain file collision is not a block"
        );
        assert_eq!(
            repair.kept, 1,
            "the path that was already there was left alone"
        );

        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).expect("a.txt"),
            "the owner's own bytes",
            "NOT OVERWRITTEN: the whole reason this is safe to run unattended"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.txt")).expect("b.txt"),
            "beta",
            "and the missing one is restored from the commit that already exists here"
        );
        let repo = open(dir.path(), true).expect("reopen");
        assert!(
            !index_is_unpopulated(&repo).expect("index"),
            "the index is written, so the next walk stops reading a mass deletion"
        );
    }

    /// The filter is never asked for content the repair is going to discard.
    ///
    /// `gix_worktree_state::checkout::entry` filters first and opens the
    /// destination second, and the `?` on that open drops the filter's
    /// response **undrained** when the path already exists — unlike the
    /// delayed path, which sinks it explicitly. A long-running
    /// `filter.<driver>.process` is then wedged mid-write on a pipe nobody
    /// reads, and the next request blocks writing to its stdin.
    ///
    /// On the owner's `/Users/tgorka/tgdrive`, where 155 112 of `HEAD`'s
    /// 155 625 paths were already on disk, that produced stalls of **exactly
    /// 900 s** — `lfs::filter::REQUEST_LIMIT` killing the wedged filter — one
    /// after another, and a repair that could not finish in three days.
    ///
    /// So the assertion is about invocations, not about output: a path that is
    /// already there must not reach the filter at all.
    #[test]
    fn a_repair_never_runs_the_filter_for_a_path_that_is_already_there() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        // Both outside the worktree, so neither becomes an untracked file the
        // checkout has an opinion about.
        let marker = root.join(".git/smudge-invocations");
        let script = root.join(".git/smudge.sh");
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .status()
                .expect("git");
            assert!(status.success(), "git {args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(root.join(".gitattributes"), "*.txt filter=mark\n").expect("attributes");
        std::fs::write(root.join("here.txt"), "already on disk").expect("write here");
        std::fs::write(root.join("gone.txt"), "will be deleted").expect("write gone");
        git(&["-c", "filter.mark.clean=cat", "add", "."]);
        git(&[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "-c",
            "filter.mark.clean=cat",
            "commit",
            "-qm",
            "seed",
        ]);
        // A smudge that records every invocation and is otherwise `cat`. The
        // count is the whole assertion.
        //
        // A script rather than an inline `sh -c`: git config treats `;` as the
        // start of a comment outside quotes, so an inline command containing
        // one is silently truncated and the filter fails to run at all —
        // which reads exactly like the thing this test is asserting.
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho ran >> {}\ncat\n", marker.display()),
        )
        .expect("write the smudge script");
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("make it executable");
        let config = format!(
            "[filter \"mark\"]\n\tclean = cat\n\tsmudge = {}\n\trequired = false\n",
            script.display()
        );
        let mut existing = std::fs::read_to_string(root.join(".git/config")).expect("config");
        existing.push_str(&config);
        std::fs::write(root.join(".git/config"), existing).expect("write config");

        // The state the repair is for, with one path of each kind: `here.txt`
        // standing on disk (must be left alone, and must not be filtered) and
        // `gone.txt` missing (must be restored, and may be filtered).
        std::fs::remove_file(root.join(".git/index")).expect("drop the index");
        std::fs::remove_file(root.join("gone.txt")).expect("drop one file");

        let repo = open(root, false).expect("reopen without an index");
        assert!(checkout_is_unfinished(&repo).expect("classify"));
        let interrupt = AtomicBool::new(false);
        let repair = restore_missing_checkout(&repo, &interrupt).expect("repair");

        assert_eq!(repair.unfinished, None, "nothing here blocks the repair");
        // These two numbers are the deterministic half of the assertion: with
        // the classification removed they come back as `restored: 0, kept: 4`,
        // because every present path reaches the checkout, collides, and is
        // counted a second time by the collision pass.
        assert_eq!(repair.restored, 1, "only the missing file was written");
        assert_eq!(
            repair.kept, 2,
            "`here.txt` and `.gitattributes` were already there"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("here.txt")).expect("here.txt"),
            "already on disk",
            "NOT OVERWRITTEN, which is the property the old collision path also had"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("gone.txt")).expect("gone.txt"),
            "will be deleted",
            "and the missing one is back from the commit"
        );

        let invocations = std::fs::read_to_string(&marker).unwrap_or_default();
        assert_eq!(
            invocations.lines().count(),
            1,
            "exactly one smudge — for the missing path. A second one means the \
             filter was asked for content that was then thrown away, which is \
             what wedges a long-running filter process: {invocations:?}"
        );

        // And the flags this repair sets must never reach disk: an index
        // carrying `SKIP_WORKTREE` says "sparse checkout, do not materialize",
        // and a status walk reads those entries as unchanged forever.
        let written = gix::index::File::at(
            repo.index_path(),
            repo.object_hash(),
            false,
            Default::default(),
        )
        .expect("the repair wrote an index");
        assert!(
            written.entries().iter().all(|entry| !entry
                .flags
                .contains(gix::index::entry::Flags::SKIP_WORKTREE)),
            "a written SKIP_WORKTREE would stop those paths syncing, silently"
        );
    }

    /// And when it cannot finish, it leaves the index alone — because an index
    /// naming paths the worktree does not hold IS the mass deletion.
    #[test]
    fn a_repair_that_cannot_finish_writes_no_index() {
        let (dir, _repo) = repo_with_two_files();
        std::fs::remove_file(dir.path().join(".git/index")).expect("drop the index");
        std::fs::remove_file(dir.path().join("a.txt")).expect("drop a");
        std::fs::remove_file(dir.path().join("b.txt")).expect("drop b");
        // A directory where HEAD holds a file: the write is refused and the
        // path stays absent as a file, so an index claiming it would be false.
        std::fs::create_dir(dir.path().join("a.txt")).expect("block a");
        let repo = open(dir.path(), true).expect("reopen");

        let interrupt = AtomicBool::new(false);
        let repair = restore_missing_checkout(&repo, &interrupt).expect("repair");
        assert!(
            repair
                .unfinished
                .as_deref()
                .is_some_and(|why| why.contains("blocked")),
            "it has to say why, got: {:?}",
            repair.unfinished
        );
        assert!(
            !dir.path().join(".git/index").exists(),
            "NO INDEX: half a checkout plus a full index is exactly the state \
             that reads as a mass deletion"
        );
    }

    /// Zero the stat data of one index entry, the way a plumbing write leaves
    /// it (`git update-index --cacheinfo`, and whatever produced 60 280 such
    /// entries in the field folder).
    fn strip_stat(repo: &gix::Repository, rela: &str) {
        let mut index = gix::index::File::at(
            repo.index_path(),
            repo.object_hash(),
            false,
            Default::default(),
        )
        .expect("read the index from disk");
        let position = index
            .entry_index_by_path(rela.into())
            .expect("the fixture path is in the index");
        index.entries_mut()[position].stat = gix::index::entry::Stat::default();
        index
            .write(gix::index::write::Options::default())
            .expect("write the stat-less index");
    }

    /// The stat data of one index entry, read fresh from disk.
    fn stat_of(root: &std::path::Path, rela: &str) -> gix::index::entry::Stat {
        let repo = open(root, true).expect("reopen");
        let index = repo.index().expect("index");
        let position = index
            .entry_index_by_path(rela.into())
            .expect("the fixture path is in the index");
        index.entries()[position].stat
    }

    /// A walk that is allowed to save what it learned, saves it.
    ///
    /// This is the whole of DW's "Scanning 151578/155662 forever". An entry
    /// with no stat cannot be answered by `lstat`, so every pass reads the file
    /// and — for an LFS path — runs it back through the clean filter. gitoxide
    /// hands the observed stat over and leaves the write to the caller; keeper
    /// dropped it, so the next pass re-learned the same thing. Measured on the
    /// field folder: 60 280 of 155 662 entries, 25.4 GB re-read per walk.
    #[test]
    fn a_persisting_walk_writes_the_stat_it_observed() {
        let (dir, _repo) = repo_with_two_files();
        strip_stat(&open(dir.path(), true).expect("reopen"), "a.txt");
        assert_eq!(
            stat_of(dir.path(), "a.txt").mtime.secs,
            0,
            "the fixture must start stat-less, or this test proves nothing"
        );

        let repo = open(dir.path(), true).expect("reopen");
        let status =
            status_paths_reported(&repo, None, Duration::MAX, WalkPolicy::full()).expect("status");
        assert!(
            status.modified.is_empty(),
            "the file matches its blob; a stat-less entry is not a modified one: {status:?}"
        );

        assert_ne!(
            stat_of(dir.path(), "a.txt").mtime.secs,
            0,
            "the walk read the file and must have written down what it found"
        );
    }

    /// And the commit that follows the walk does not revert it.
    ///
    /// The trap this pins: `stage_and_commit` used to clone the handle's
    /// *cached* index snapshot, and a pass loads that cache early — the walk
    /// reads the index for its own denominator. So the stats the walk had just
    /// written were reverted by the very same pass, on every folder that
    /// actually committed something, and the saving would have shown up only on
    /// idle passes.
    #[test]
    fn a_commit_after_a_persisting_walk_keeps_the_stats_the_walk_wrote() {
        let (dir, _repo) = repo_with_two_files();
        strip_stat(&open(dir.path(), true).expect("reopen"), "a.txt");

        // One handle for the whole pass, exactly as `commit_local` uses it.
        let repo = open(dir.path(), true).expect("reopen");
        status_paths_reported(&repo, None, Duration::MAX, WalkPolicy::full()).expect("status");
        let after_walk = stat_of(dir.path(), "a.txt").mtime.secs;
        assert_ne!(after_walk, 0, "the walk must have written the stat first");

        std::fs::write(dir.path().join("c.txt"), "gamma").expect("write");
        stage_and_commit(
            &repo,
            &StagedChange {
                added: vec![PathBuf::from("c.txt")],
                ..StagedChange::default()
            },
            &provenance(),
            &profile(dir.path()),
            &signature(),
            &std::collections::BTreeMap::new(),
            None,
        )
        .expect("commit")
        .expect("a non-empty commit");

        assert_eq!(
            stat_of(dir.path(), "a.txt").mtime.secs,
            after_walk,
            "staging wrote a stale index and threw the walk's work away"
        );
    }

    /// And a walk that was not asked to save anything changes nothing.
    ///
    /// The read-only policy is what every caller without the folder's walk
    /// claim gets, because the write is the walk's own index with stat fields
    /// replaced: a leg that staged something after the walk started would have
    /// that work overwritten.
    #[test]
    fn a_read_only_walk_leaves_the_index_exactly_as_it_found_it() {
        let (dir, _repo) = repo_with_two_files();
        strip_stat(&open(dir.path(), true).expect("reopen"), "a.txt");
        let before = std::fs::read(dir.path().join(".git/index")).expect("read index");

        let repo = open(dir.path(), true).expect("reopen");
        status_paths_reported(&repo, None, Duration::MAX, WalkPolicy::read_only()).expect("status");

        assert_eq!(
            std::fs::read(dir.path().join(".git/index")).expect("read index"),
            before,
            "a read-only walk must not rewrite the index at all"
        );
    }

    /// The poll's cheap walk: index entries yes, directory scan no.
    ///
    /// The directory scan is the half that costs the whole tree whatever
    /// changed — 996 s of `lstat` on the field folder, per pass. What it buys
    /// is untracked paths, which a five-second poll does not need: the watcher
    /// and the stability gate name a new file long before a walk finds it.
    #[test]
    fn a_tracked_only_walk_skips_the_untracked_search_and_still_reports_changes() {
        let (dir, _repo) = repo_with_two_files();
        std::fs::write(dir.path().join("a.txt"), "alpha-changed").expect("modify");
        std::fs::write(dir.path().join("new.txt"), "untracked").expect("write");

        let repo = open(dir.path(), true).expect("reopen");
        let full =
            status_paths_reported(&repo, None, Duration::MAX, WalkPolicy::full()).expect("status");
        assert_eq!(
            full.untracked,
            [PathBuf::from("new.txt")],
            "the full policy is what finds an untracked file: {full:?}"
        );

        let repo = open(dir.path(), true).expect("reopen");
        let tracked = status_paths_reported(&repo, None, Duration::MAX, WalkPolicy::tracked_only())
            .expect("status");
        assert!(
            tracked.untracked.is_empty(),
            "the poll's walk must not have walked the directories: {tracked:?}"
        );
        assert_eq!(
            tracked.modified,
            [PathBuf::from("a.txt")],
            "and it must still answer the question the index can answer: {tracked:?}"
        );
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

    /// The key that decides whether keeper's filter is ever reached (DW-140).
    ///
    /// git prefers `filter.<drv>.process` over a `clean`/`smudge` pair
    /// *whatever scope each was defined in*, so a global one — which
    /// `git lfs install` writes into `~/.gitconfig` on every machine that has
    /// ever had the real client — outranks the repository-local pair keeper
    /// used to register alone. Writing a local `process` key is the only way to
    /// win that comparison, and it is the whole reason this registration
    /// exists.
    #[test]
    fn the_filter_registration_claims_the_key_that_actually_takes_effect() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");

        enforce_local_config_with_filter(
            &repo,
            Some(Path::new("/Applications/keeper.app/keeper")),
            true,
        )
        .expect("enforce");

        // Read back through gix rather than off the raw file: the on-disk form
        // escapes the quotes around the program path, and asserting on that
        // spelling would test the encoder instead of the registration.
        let reopened = open(dir.path(), true).expect("reopen");
        let snapshot = reopened.config_snapshot();
        let process = snapshot
            .string("filter.lfs.process")
            .expect("the long-running form must be registered, not only clean/smudge")
            .to_string();
        assert_eq!(
            process,
            "\"/Applications/keeper.app/keeper\" lfs filter-process --repo \"".to_owned()
                + &dir.path().display().to_string()
                + "\""
        );
        // No `%f`: the protocol names each path in-band, and a stray
        // placeholder would arrive as an argument the filter never asked for.
        assert!(!process.contains("%f"), "{process}");
        // The single-shot pair stays: it costs one line and is what a git old
        // enough to lack process filters would use.
        assert!(snapshot
            .string("filter.lfs.clean")
            .is_some_and(|value| value.to_string().contains("lfs clean")));
        assert!(snapshot
            .string("filter.lfs.smudge")
            .is_some_and(|value| value.to_string().contains("lfs smudge")));
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

    /// The `[filter "lfs"]` section `git lfs install --local` leaves in
    /// `.git/config`, with the long-running driver keeper cannot answer.
    fn with_local_lfs_process(root: &std::path::Path) {
        let config = root.join(".git/config");
        let mut text = std::fs::read_to_string(&config).expect("read config");
        text.push_str("[filter \"lfs\"]\n\tprocess = git-lfs filter-process\n\trequired = true\n");
        std::fs::write(&config, text).expect("write config");
    }

    /// Somebody else's long-running driver is replaced by keeper's own, not
    /// merely deleted (DW-206 refined by DW-140).
    ///
    /// The original of this test asserted no `process` key survived at all,
    /// which was right while keeper had none to offer: gitoxide takes `process`
    /// in preference to the pair below it and fails hard when it cannot be
    /// launched, whatever `required` says, so a driver keeper could not answer
    /// made the two keys it had just written unreachable. Now keeper *can*
    /// answer one, and leaving the slot empty is what loses: git prefers a
    /// `process` driver from any scope, so an empty local slot hands every
    /// filtered operation of the git binary back to whatever `~/.gitconfig`
    /// names. Both halves are asserted below — theirs is gone, ours is there.
    #[test]
    fn registering_the_filter_replaces_a_foreign_process_driver_with_keepers_own() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        with_local_lfs_process(dir.path());

        enforce_local_config_with_filter(&repo, Some(std::path::Path::new("/opt/keeper")), true)
            .expect("enforce");

        let reopened = open(dir.path(), true).expect("reopen");
        let config = reopened.config_snapshot();
        let process = config
            .string("filter.lfs.process")
            .expect("keeper's own driver must occupy the slot")
            .to_string();
        assert!(
            process.contains("/opt/keeper") && process.contains("lfs filter-process"),
            "the surviving driver has to be keeper's: {process}"
        );
        assert!(
            !process.contains("git-lfs"),
            "no trace of the driver that was here: {process}"
        );
        // Exactly one, so the strip really ran rather than the write landing
        // beside a survivor in another section.
        let text = std::fs::read_to_string(dir.path().join(".git/config")).expect("read config");
        assert_eq!(text.matches("process = ").count(), 1, "{text}");
        assert!(
            config
                .string("filter.lfs.clean")
                .is_some_and(|value| value.to_string().contains("lfs clean")),
            "the clean filter has to survive"
        );
        assert_eq!(config.boolean("filter.lfs.required"), Some(false));
    }

    #[test]
    fn a_foreign_lfs_driver_is_dropped_and_the_local_one_is_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        // Registered without a long-running driver of keeper's own, which is
        // what this test needs to arrange: the foreign one has to be the only
        // `process` in the merged config, or it is not the one gix would launch
        // and the fixture is not the situation being tested. The case where
        // keeper *has* registered one is
        // `keepers_own_process_driver_survives_a_foreign_section`.
        enforce_local_config_with_filter(&repo, Some(std::path::Path::new("/opt/keeper")), false)
            .expect("enforce");
        let mut repo = open(dir.path(), true).expect("reopen");

        // A global `[filter "lfs"]`, in the position a real one occupies: ahead
        // of the repository's own section, because scopes merge in precedence
        // order and gitoxide answers with the FIRST driver of that name.
        let foreign = gix::config::File::from_bytes_owned(
            &mut b"[filter \"lfs\"]\n\tprocess = git-lfs filter-process\n".to_vec(),
            gix::config::file::Metadata::from(gix::config::Source::User),
            Default::default(),
        )
        .expect("parse the foreign config");
        {
            let mut snapshot = repo.config_snapshot_mut();
            let mut merged = foreign;
            merged
                .append(snapshot.clone())
                .expect("merge the repository's own scopes after it");
            *snapshot = merged;
            snapshot.commit().expect("commit the doctored config");
        }
        assert_eq!(
            repo.config_snapshot().string("filter.lfs.process"),
            Some("git-lfs filter-process".into()),
            "arranged: the foreign driver is what gix would launch"
        );

        drop_foreign_lfs_driver(&mut repo).expect("drop");

        let config = repo.config_snapshot();
        assert_eq!(
            config.string("filter.lfs.process"),
            None,
            "a driver from outside the repository may not decide how it is filtered"
        );
        assert!(
            config
                .string("filter.lfs.clean")
                .is_some_and(|value| value.to_string().contains("lfs clean")),
            "the repository's own registration is what must remain"
        );
    }

    /// The two halves composed: keeper's own long-running driver must survive
    /// the surgery that removes everybody else's (DW-206 + DW-140).
    ///
    /// `drop_foreign_lfs_driver` filters on **scope**, not on content, so this
    /// is really asking whether the key `enforce_local_config_with_filter`
    /// writes lands in a repository scope. If it ever did not, the drop would
    /// take keeper's own driver with it and every filtered operation would fall
    /// back to whatever `~/.gitconfig` names — the exact silence DW-140 is
    /// about, reintroduced by the fix for DW-206.
    #[test]
    fn keepers_own_process_driver_survives_a_foreign_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        enforce_local_config_with_filter(&repo, Some(std::path::Path::new("/opt/keeper")), true)
            .expect("enforce");
        let mut repo = open(dir.path(), true).expect("reopen");

        let foreign = gix::config::File::from_bytes_owned(
            &mut b"[filter \"lfs\"]\n\tprocess = git-lfs filter-process\n".to_vec(),
            gix::config::file::Metadata::from(gix::config::Source::User),
            Default::default(),
        )
        .expect("parse the foreign config");
        {
            let mut snapshot = repo.config_snapshot_mut();
            let mut merged = foreign;
            merged
                .append(snapshot.clone())
                .expect("merge the repository's own scopes after it");
            *snapshot = merged;
            snapshot.commit().expect("commit the doctored config");
        }

        drop_foreign_lfs_driver(&mut repo).expect("drop");

        let config = repo.config_snapshot();
        let surviving = config
            .string("filter.lfs.process")
            .map(|value| value.to_string())
            .expect("keeper's own driver must outlive the drop");
        assert!(
            surviving.contains("/opt/keeper") && surviving.contains("lfs filter-process"),
            "what survived has to be keeper's: {surviving}"
        );
        assert!(
            !surviving.contains("git-lfs"),
            "and the foreign one has to be gone: {surviving}"
        );
    }

    /// Put a repository into the worktree-scoped configuration state
    /// `git sparse-checkout` leaves behind, with `index.sparse` set to
    /// `declared` in `.git/config.worktree`.
    fn with_worktree_config(root: &std::path::Path, declared: Option<&str>) {
        let config = root.join(".git/config");
        let mut text = std::fs::read_to_string(&config).expect("read config");
        text.push_str("[extensions]\n\tworktreeConfig = true\n");
        std::fs::write(&config, text).expect("write config");
        let mut worktree = String::from("[core]\n\tsparseCheckout = true\n");
        if let Some(declared) = declared {
            worktree.push_str(&format!("[index]\n\tsparse = {declared}\n"));
        }
        std::fs::write(root.join(".git/config.worktree"), worktree).expect("write worktree config");
    }

    #[test]
    fn an_index_sparse_a_worktree_config_shadows_is_cleared() {
        // The whole point of the function: `.git/config` says false, the
        // worktree scope says true, and true is what gix and git both act on —
        // so `gix::status` hard-fails until this runs.
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        enforce_local_config(&repo).expect("enforce");
        with_worktree_config(dir.path(), Some("true"));

        let reopened = open(dir.path(), true).expect("reopen");
        assert_eq!(
            reopened.config_snapshot().boolean("index.sparse"),
            Some(true),
            "the fixture must reproduce the shadow, or this test proves nothing"
        );

        assert!(
            clear_worktree_sparse_override(&reopened).expect("clear"),
            "a shadowing true must be reported as changed"
        );

        let reopened = open(dir.path(), true).expect("reopen after the fix");
        assert_eq!(
            reopened.config_snapshot().boolean("index.sparse"),
            Some(false),
            "the effective value is what AD-47 constrains, not the one in .git/config"
        );
        assert_eq!(
            reopened.config_snapshot().boolean("core.sparseCheckout"),
            Some(true),
            "the sparse checkout itself must survive: only the index format is keeper's"
        );
    }

    #[test]
    fn a_worktree_config_that_is_not_shadowing_is_left_untouched() {
        // `sparse-checkout disable` writes `index.sparse = false` here itself,
        // and every rewrite of this file is a write to a pendrive that may be
        // pulled mid-sync (AD-48). Nothing is rewritten unless it is wrong.
        for declared in [None, Some("false")] {
            let dir = tempfile::tempdir().expect("tempdir");
            let repo = gix::init(dir.path()).expect("init");
            enforce_local_config(&repo).expect("enforce");
            with_worktree_config(dir.path(), declared);
            let path = dir.path().join(".git/config.worktree");
            let before = std::fs::read_to_string(&path).expect("read worktree config");

            let reopened = open(dir.path(), true).expect("reopen");
            assert!(
                !clear_worktree_sparse_override(&reopened).expect("clear"),
                "{declared:?} shadows nothing and must report no change"
            );
            assert_eq!(
                std::fs::read_to_string(&path).expect("read worktree config"),
                before,
                "{declared:?} must not have been rewritten"
            );
        }
    }

    #[test]
    fn a_worktree_config_the_extension_does_not_enable_is_inert() {
        // Without `extensions.worktreeConfig` neither git nor gix ever loads the
        // file, so nothing is shadowed and enabling the extension to "fix" a
        // value nobody reads would be keeper making the repository sparse-aware
        // on its own initiative.
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        enforce_local_config(&repo).expect("enforce");
        std::fs::write(
            dir.path().join(".git/config.worktree"),
            "[index]\n\tsparse = true\n",
        )
        .expect("write worktree config");

        let reopened = open(dir.path(), true).expect("reopen");
        assert!(!clear_worktree_sparse_override(&reopened).expect("clear"));
        assert_eq!(
            reopened.config_snapshot().boolean("index.sparse"),
            Some(false),
            "an unreferenced file cannot have changed the effective value"
        );
    }

    #[test]
    fn sparse_patterns_are_reported_only_while_sparse_checkout_is_switched_on() {
        // `sparse-checkout disable` leaves the pattern file exactly where it
        // was. Reading the file alone would report a full checkout as narrow
        // forever, and the cone would never be re-applied after a re-widening.
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        enforce_local_config(&repo).expect("enforce");
        let info = dir.path().join(".git/info");
        std::fs::create_dir_all(&info).expect("info dir");
        std::fs::write(info.join("sparse-checkout"), "/*\n!/*/\n/docs/\n").expect("patterns");

        let reopened = open(dir.path(), true).expect("reopen");
        let stale = sparse_patterns(&reopened).expect("read");
        assert_eq!(stale, None, "a stale pattern file is not a sparse checkout");

        with_worktree_config(dir.path(), None);
        let reopened = open(dir.path(), true).expect("reopen");
        let patterns = sparse_patterns(&reopened).expect("read");
        assert_eq!(patterns.as_deref(), Some("/*\n!/*/\n/docs/\n"));
    }

    #[test]
    fn sparse_mode_with_no_pattern_file_reads_as_an_empty_cone_not_a_full_checkout() {
        // Reported as `Some("")` so the caller re-applies the profile's cone.
        // `None` would mean "full checkout, nothing to do", and the repository
        // would sit in a broken sparse state indefinitely.
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        enforce_local_config(&repo).expect("enforce");
        with_worktree_config(dir.path(), None);

        let reopened = open(dir.path(), true).expect("reopen");
        let patterns = sparse_patterns(&reopened).expect("read");
        assert_eq!(patterns.as_deref(), Some(""));
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

    /// Age a file by `by`, so the sweep's own threshold decides.
    ///
    /// `std::fs::FileTimes` rather than a new dev-dependency or a shelled-out
    /// `touch`, whose date syntax differs between GNU and BSD and would make
    /// this test pass on Linux and fail on the macOS host.
    fn backdate(path: &Path, by: Duration) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open the lock to set its times");
        let when = SystemTime::now() - by;
        file.set_times(std::fs::FileTimes::new().set_modified(when))
            .expect("backdate the mtime");
    }

    /// [`open_read_only`] repairs nothing, and the stale-`index.lock` sweep is
    /// the half of that promise you can see from outside (Story 56.14).
    ///
    /// `Engine::verify` is the one pass that takes no per-profile reservation,
    /// so it is the likeliest of them all to be running beside a keeper commit
    /// or beside a person's own `git` — and deleting an `index.lock` that is
    /// genuinely held is exactly the repair that is dangerous next to a live
    /// writer. A check that repairs what it is checking is not a check.
    /// Without the read-only door, `verify` opened through [`open`] and did all
    /// three repairs: the index lock, the loose-ref locks, and the config
    /// rewrite that drops a foreign LFS driver.
    ///
    /// Without the fix `open_read_only` *is* `open`, so the first assertion
    /// reads a file that is no longer there — the lock is deleted by the very
    /// call that promised to touch nothing. The second half is the positive
    /// control: the same lock, aged the same way, IS removed by `open`, which
    /// is what stops the first half passing merely because the lock was too
    /// young for the threshold to fire.
    #[test]
    fn open_read_only_leaves_a_stale_index_lock_that_open_would_remove() {
        let dir = tempfile::tempdir().expect("tempdir");
        gix::init(dir.path()).expect("init");
        let lock = dir.path().join(".git").join("index.lock");
        std::fs::write(&lock, b"").expect("plant an index lock");
        // Comfortably past the threshold, so a coarse filesystem clock cannot
        // put the fixture on the wrong side of the decision.
        backdate(&lock, STALE_INDEX_LOCK + Duration::from_secs(60));

        let _read_only = open_read_only(dir.path(), true).expect("open read-only");
        assert!(
            lock.exists(),
            "the read-only door must repair nothing, and a lock well past \
             STALE_INDEX_LOCK is the repair that is easiest to see"
        );

        let _repaired = open(dir.path(), true).expect("open");
        assert!(
            !lock.exists(),
            "the writing door still sweeps it, or the assertion above proves \
             only that the lock was too young"
        );
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

    /// A walk that finishes inside the pacing interval says nothing.
    ///
    /// This half is not an optimisation, it is the constraint: publishing the
    /// scanning phase on every poll of an idle folder is what made the menu-bar
    /// glyph flip once a second, and the report exists for the ten-minute walk,
    /// not the ten-millisecond one.
    #[test]
    fn a_walk_that_finishes_immediately_reports_no_progress() {
        let (dir, repo) = repo_with_two_files();
        // The walk MUST produce items, or this test would pass for the wrong
        // reason: a clean tree emits nothing and there is no pacing to prove.
        std::fs::write(dir.path().join("c.txt"), "untracked").expect("write");
        std::fs::write(dir.path().join("a.txt"), "alpha-changed").expect("modify");
        let seen = std::sync::Mutex::new(Vec::new());
        let report = |done: u64, total: u64| seen.lock().expect("lock").push((done, total));

        status_paths_reported(
            &repo,
            Some(&report),
            Duration::from_secs(1),
            WalkPolicy::read_only(),
        )
        .expect("status");
        assert!(
            seen.lock().expect("lock").is_empty(),
            "a fast walk must stay silent, got {:?}",
            seen.lock().expect("lock")
        );
    }

    /// And a ZERO interval reports even when the walk beats the ticker to it.
    ///
    /// Same fixture as the silence guard above, so the two differ in exactly
    /// one input: the interval. `Duration::ZERO` is what
    /// `Engine::report_every_walk_item` asks for, and the ticker cannot serve
    /// it — it sleeps a clamped millisecond before its first look, by which
    /// time a two-file walk has finished and set `done`. Every engine-level
    /// test that subscribes to `Scanning` progress rests on this closing
    /// report; without it those tests assert that the machine is slow.
    #[test]
    fn a_zero_interval_walk_reports_its_closing_figure() {
        let (dir, repo) = repo_with_two_files();
        std::fs::write(dir.path().join("c.txt"), "untracked").expect("write");
        std::fs::write(dir.path().join("a.txt"), "alpha-changed").expect("modify");
        let seen = std::sync::Mutex::new(Vec::new());
        let report = |done: u64, total: u64| seen.lock().expect("lock").push((done, total));

        status_paths_reported(
            &repo,
            Some(&report),
            Duration::ZERO,
            WalkPolicy::read_only(),
        )
        .expect("status");
        let seen = seen.lock().expect("lock").clone();
        let last = seen.last().copied().expect("a zero interval reports");
        assert!(
            last.0 > 0 && last.1 > 0,
            "the closing report is the pair the UI renders: {seen:?}"
        );
        assert_eq!(
            last.1, 2,
            "the denominator is the index entry count: {seen:?}"
        );
    }

    /// The rule itself, which is where the mutation is detectable.
    ///
    /// The walk above cannot carry this claim on its own: whether a two-entry
    /// walk beats the ticker's first millisecond is a property of the machine,
    /// so on a slow enough box it passes with the closing report deleted. This
    /// is the same rule with the timing removed.
    #[test]
    fn the_closing_report_is_owed_when_the_ticker_was_silent_only_at_zero() {
        assert!(
            owes_closing_report(true, Duration::from_secs(1)),
            "a ticker that spoke must not leave a mid-walk figure as the final word"
        );
        assert!(
            !owes_closing_report(false, Duration::from_secs(1)),
            "a paced walk that stayed under one interval must stay silent"
        );
        assert!(
            owes_closing_report(false, Duration::ZERO),
            "report-every-item cannot depend on the ticker winning a race"
        );
        assert!(owes_closing_report(true, Duration::ZERO));
    }

    /// And a walk that does take time reports the pair the UI renders.
    ///
    /// The fixture is the failure, not a convenience: 3 000 committed files
    /// that are all **clean**, plus one untracked path. The walk therefore
    /// emits exactly one item and compares 3 000 index entries.
    ///
    /// Against that, the two candidate designs give opposite answers:
    ///
    /// * published from inside the item loop, counting emissions — **one**
    ///   report, reading `1`, for the whole pass. That is what the owner was
    ///   looking at when a 155 662-entry folder sat on `9113/155662`.
    /// * published off a clock, counting entries — a report per interval,
    ///   climbing to 3 000, which is what the walk is actually doing.
    #[test]
    fn a_slow_walk_reports_index_entries_compared_not_items_emitted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        let mut added = Vec::with_capacity(3_000);
        for i in 0..3_000 {
            let name = format!("f{i:05}.txt");
            std::fs::write(dir.path().join(&name), b"x").expect("write");
            added.push(PathBuf::from(name));
        }
        stage_and_commit(
            &repo,
            &StagedChange {
                added,
                ..StagedChange::default()
            },
            &provenance(),
            &profile(dir.path()),
            &signature(),
            &std::collections::BTreeMap::new(),
            None,
        )
        .expect("commit")
        .expect("a non-empty commit");
        let repo = gix::open(dir.path()).expect("reopen");
        // The one emission. Everything else the walk touches is clean, so a
        // reporter tied to emissions has nothing further to say.
        std::fs::write(dir.path().join("untracked.txt"), "u").expect("write");

        let seen = std::sync::Mutex::new(Vec::new());
        let report = |done: u64, total: u64| seen.lock().expect("lock").push((done, total));

        // ZERO, not a millisecond cadence: whether a 3 000-entry walk outlasts
        // a tick is a property of the machine, and this test is about WHAT is
        // counted, not about whether the clock got a turn. At ZERO the closing
        // report is owed unconditionally, so the assertions below run on every
        // machine — and an emissions-based numerator still fails them.
        status_paths_excluding(
            &repo,
            &[],
            Some(&report),
            Duration::ZERO,
            WalkPolicy::read_only(),
        )
        .expect("status");

        let seen = seen.lock().expect("lock").clone();
        assert!(
            !seen.is_empty(),
            "3 000 entries were compared and the walk said nothing"
        );
        // The denominator is the index, which holds the committed files and not
        // the untracked one.
        assert!(
            seen.iter().all(|pair| pair.1 == 3_000),
            "the denominator is the index entry count: {:?}",
            &seen[..seen.len().min(8)]
        );
        // Drawn against that denominator, so it can never pass it. An emission
        // count could: the untracked path is emitted and is not an index entry.
        assert!(
            seen.iter().all(|pair| pair.0 <= pair.1),
            "the numerator overran its own denominator: {:?}",
            &seen[..seen.len().min(8)]
        );
        // Counts up. Not *strictly*: a tick that lands between two comparisons
        // honestly repeats the figure rather than inventing movement.
        assert!(
            seen.windows(2).all(|pair| pair[1].0 >= pair[0].0),
            "the count walked backwards: {:?}",
            &seen[..seen.len().min(8)]
        );
        // The regression, stated as a number: one emission happened, so a
        // loop-driven reporter could not have got past it.
        let reached = seen.iter().map(|pair| pair.0).max().unwrap_or(0);
        assert!(
            reached > 1,
            "the count never got past the single emitted item ({reached}), which \
             is exactly the freeze this reports off a clock to avoid: {:?}",
            &seen[..seen.len().min(8)]
        );
    }

    /// A walk that reported at all reports where it stopped.
    ///
    /// The ticker fires on its own cadence, so the walk almost never ends on
    /// one: whatever figure the last tick happened to catch would otherwise be
    /// the final word, and the pane would sit a few thousand entries short of
    /// the end with no way to tell "finished" from "stalled near the end". The
    /// macOS gate saw exactly that — `[198, 382, 296, 392]` against a
    /// denominator of 400.
    ///
    /// **The claim is conditional on purpose.** How many ticks an 8 000-entry
    /// walk outlasts is a property of the machine, not of this code: the same
    /// fixture produced four reports on Linux and fewer than two on the macOS
    /// gate, where the walk finished inside a single 5 ms slice. Demanding a
    /// tick count is how the previous revision of this test failed CI while
    /// the behaviour it guards was correct. What must hold on every machine is
    /// that the LAST word is the denominator whenever there was a word at all;
    /// `the_closing_report_is_owed_when_the_ticker_was_silent_only_at_zero`
    /// carries the rule itself, with the clock taken out.
    #[test]
    fn a_walk_that_reported_says_where_it_stopped() {
        const ENTRIES: usize = 8_000;
        const CADENCE: Duration = Duration::from_millis(1);
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        let mut added = Vec::with_capacity(ENTRIES);
        for i in 0..ENTRIES {
            let name = format!("f{i:05}.txt");
            std::fs::write(dir.path().join(&name), b"x").expect("write");
            added.push(PathBuf::from(name));
        }
        stage_and_commit(
            &repo,
            &StagedChange {
                added,
                ..StagedChange::default()
            },
            &provenance(),
            &profile(dir.path()),
            &signature(),
            &std::collections::BTreeMap::new(),
            None,
        )
        .expect("commit")
        .expect("a non-empty commit");
        let repo = gix::open(dir.path()).expect("reopen");

        let seen = std::sync::Mutex::new(Vec::new());
        let report = |done: u64, total: u64| seen.lock().expect("lock").push((done, total));
        status_paths_excluding(&repo, &[], Some(&report), CADENCE, WalkPolicy::read_only())
            .expect("status");

        let seen = seen.lock().expect("lock").clone();
        assert!(
            seen.windows(2).all(|pair| pair[1].0 >= pair[0].0),
            "the count walked backwards: {seen:?}"
        );
        if let Some(last) = seen.last().copied() {
            assert_eq!(
                last,
                (ENTRIES as u64, ENTRIES as u64),
                "the walk compared every entry and its last word said otherwise: {seen:?}"
            );
        }
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

    /// Story 47.2 — the index scan must keep path bytes, not decode them.
    ///
    /// `tracked_paths` used to build each path with `BStr::to_string`, a lossy
    /// UTF-8 decode. The resulting `PathBuf` names a different file or no file
    /// at all, and its one caller is the LFS materialization scan, which
    /// `lstat`s and rewrites what it is given.
    #[cfg(unix)]
    #[test]
    fn the_index_scan_keeps_the_bytes_of_a_name_that_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        // Raw bytes, because no string literal can express this name.
        let odd = std::ffi::OsString::from_vec(b"doc-\xffepuap.txt".to_vec());
        if crate::names::create_unspellable(dir.path(), b"doc-\xffepuap.txt").is_none() {
            eprintln!("{}", crate::names::UNSPELLABLE_UNAVAILABLE);
            return;
        }
        std::fs::write(dir.path().join("ordinary.txt"), b"x").expect("write");

        let status = status_paths(&repo).expect("status");
        let changes = crate::git::commit::StagedChange {
            added: status.untracked.clone(),
            ..Default::default()
        };
        crate::git::commit::stage_and_commit(
            &repo,
            &changes,
            &crate::provenance::Provenance::new(
                "p",
                "dev",
                "01",
                "host",
                crate::provenance::SyncSource::Manual,
            ),
            &profile(dir.path()),
            &signature(),
            &std::collections::BTreeMap::new(),
            None,
        )
        .expect("git carries path bytes, so this commits");

        let tracked = tracked_paths(&repo).expect("tracked");
        assert!(
            tracked.iter().any(|p| p.as_os_str() == odd),
            "the path must come back byte-identical; got {tracked:?}"
        );
        // The specific way it used to be wrong: the lossy form is a path that
        // reaches nothing, and a caller that stats it silently does nothing.
        assert!(
            !dir.path().join("doc-\u{FFFD}epuap.txt").exists(),
            "the lossy rendering names no file, which is why decoding lost it"
        );

        // And the same index answers the report question, for the ordinary
        // file too — an inventory that flagged everything would be useless.
        let found = unspellable_tracked_paths(&repo).expect("scan");
        assert_eq!(
            found.iter().map(|n| n.escaped.as_str()).collect::<Vec<_>>(),
            vec!["doc-\\xffepuap.txt"]
        );
    }

    /// A repository of ordinary names has nothing to report, and a folder that
    /// is not a repository yet has no index to read.
    #[test]
    fn an_ordinary_repository_reports_no_unspellable_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = gix::init(dir.path()).expect("init");
        // Non-ASCII but perfectly valid UTF-8: this must NOT be reported, or
        // every user outside ASCII gets a permanent warning about their files.
        std::fs::write(dir.path().join("zaświadczenie.pdf"), b"x").expect("write");
        let status = status_paths(&repo).expect("status");
        crate::git::commit::stage_and_commit(
            &repo,
            &crate::git::commit::StagedChange {
                added: status.untracked.clone(),
                ..Default::default()
            },
            &crate::provenance::Provenance::new(
                "p",
                "dev",
                "01",
                "host",
                crate::provenance::SyncSource::Manual,
            ),
            &profile(dir.path()),
            &signature(),
            &std::collections::BTreeMap::new(),
            None,
        )
        .expect("commit");

        assert!(unspellable_tracked_paths(&repo).expect("scan").is_empty());
    }
}
