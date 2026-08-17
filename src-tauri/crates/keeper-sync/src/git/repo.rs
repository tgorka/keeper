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
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime},
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
pub fn open(path: &Path, trust_full: bool) -> Result<gix::Repository> {
    let mut options = gix::open::Options::default();
    if trust_full {
        options = options.with(gix::sec::Trust::Full);
    }
    let mut repo = gix::open_opts(path, options)
        .map_err(|err| SyncError::Git(format!("open failed: {}", super::fetch::flatten(&err))))?;
    release_stale_index_lock(repo.git_dir());
    release_stale_ref_locks(repo.git_dir());
    drop_foreign_lfs_driver(&mut repo)?;
    Ok(repo)
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
    let known = still_unreadable(repo, remembered_unreadable(repo));
    let skip: Vec<PathBuf> = known.iter().map(|item| item.path.clone()).collect();

    let status = match status_paths_excluding(repo, &skip) {
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
            let mut status = status_paths_excluding(repo, &skip)?;
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

/// [`status_paths`], with `skip` held out of the walk entirely.
///
/// The exclusions are spelled `:(exclude,literal)<path>`: `literal` because a
/// synced folder is full of names that are also glob syntax — `[2026]`, `*`,
/// `?` all occur in real user content — and a pattern that quietly matched more
/// than the one path it names would hide unrelated files from every pass.
fn status_paths_excluding(repo: &gix::Repository, skip: &[PathBuf]) -> Result<RepoStatus> {
    use gix::status::{index_worktree::Item as WorktreeItem, plumbing::index_as_worktree, Item};

    // `flatten`, not `{err}`: a status that trips over one unreadable tracked
    // file reports "IO error while writing blob or reading file metadata or
    // changing filetype" in its top frame, and the errno that says *why* —
    // permission denied, EOF from a file rewritten mid-read — is two `source()`
    // hops down. gix never names the path, so the cause is all there is.
    let platform = repo
        .status(gix::progress::Discard)
        .map_err(|err| SyncError::Git(format!("status failed: {}", super::fetch::flatten(&err))))?
        .index_worktree_options_mut(|options| {
            if let Some(dirwalk) = options.dirwalk_options.as_mut() {
                dirwalk.set_emit_untracked(gix::dir::walk::EmissionMode::Matching);
            }
        });
    let patterns: Vec<gix::bstr::BString> = skip
        .iter()
        .map(|path| {
            let mut pattern = gix::bstr::BString::from(":(exclude,literal)");
            pattern.extend_from_slice(&gix::path::into_bstr(path.as_path()));
            pattern
        })
        .collect();
    let iter = platform
        .into_iter(patterns)
        .map_err(|err| SyncError::Git(format!("status failed: {}", super::fetch::flatten(&err))))?;

    let mut out = RepoStatus::default();
    for item in iter {
        let item = item.map_err(|err| {
            SyncError::Git(format!("status failed: {}", super::fetch::flatten(&err)))
        })?;
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
