//! The audited `git` binary shim (Story 24.5, AD-41).
//!
//! This is the **only** module in the workspace that spawns a process, and it
//! exists because gitoxide implements none of what it does:
//!
//! * **push** — tracking issue #306 was closed `NOT_PLANNED` upstream on
//!   2026-07-22 and demoted to an unreviewed discussion; `gix::push` is a
//!   config enum with no transfer logic, and `Connection` offers only
//!   `ref_map()` and `prepare_fetch()`. It is not coming.
//! * **worktree mutation** — `gix::worktree` is read-only (`Proxy` can list,
//!   inspect and open; nothing creates, removes or prunes).
//! * **sparse-checkout patterns** — nothing in gitoxide ever reads
//!   `.git/info/sparse-checkout`; `gix_index::access::sparse` has zero
//!   consumers.
//! * **gc / repack** — no equivalent exists at any layer.
//!
//! Every invocation here is **argument-vectored**, never a shell string: a
//! profile name, a branch or a subpath is user-controlled text, and a shell
//! would let `; rm -rf` or a leading `-` become a second command or an extra
//! flag. Arguments are built by pure functions so the vector can be asserted in
//! a unit test without spawning anything.
//!
//! Two more hazards are closed here:
//!
//! * **Prompts.** A headless `keeper-syncd` has no terminal. Without
//!   `GIT_TERMINAL_PROMPT=0` a private remote makes `git` block forever on a
//!   username prompt that nobody will ever answer.
//! * **Credentials in diagnostics.** `git` echoes the URL it used in almost
//!   every failure, and a URL may carry `://user:token@`. Nothing leaves this
//!   module without passing through `scrub_userinfo` (NFR-26).

use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use crate::error::{Result, SyncError};
use crate::git::fetch::Credential;

/// Environment variables the credential helper below reads the secret from.
///
/// The environment, not the argument vector: `ps` shows every process's argv to
/// any user on the box, and a push runs for as long as the transfer takes. The
/// helper *string* is argv — it names these variables and carries no secret.
const CREDENTIAL_USERNAME_ENV: &str = "KEEPER_SYNC_CREDENTIAL_USERNAME";
const CREDENTIAL_SECRET_ENV: &str = "KEEPER_SYNC_CREDENTIAL_SECRET";

/// A `credential.helper` that answers from [`CREDENTIAL_USERNAME_ENV`] and
/// [`CREDENTIAL_SECRET_ENV`], and from nothing else.
///
/// `if`/`fi` rather than `test … && printf …`: a helper whose last command
/// fails exits non-zero, and git treats that as a broken helper rather than as
/// "no answer for this action". Only `get` is answered — `store` and `erase`
/// return successfully having done nothing, because the OS keychain owns the
/// secret's lifecycle and letting git approve a copy into another store is the
/// very leak this exists to close (the same reasoning as
/// [`super::fetch::static_credential`]).
const CREDENTIAL_HELPER: &str = concat!(
    "!f() { if test \"$1\" = get; then printf '%s\\n' ",
    "\"username=$KEEPER_SYNC_CREDENTIAL_USERNAME\" ",
    "\"password=$KEEPER_SYNC_CREDENTIAL_SECRET\"; fi; }; f"
);

/// Oldest `git` that can serve this engine.
///
/// 2.42 is where `sparse-checkout set --cone` and `ls-files --format` both
/// became dependable; below it a partial profile would silently materialize the
/// whole repository (AD-47).
pub const MIN_GIT_MAJOR: u32 = 2;
/// Minor component of the version floor. See [`MIN_GIT_MAJOR`].
pub const MIN_GIT_MINOR: u32 = 42;

/// Longest captured `stderr` kept in an error or a log line.
///
/// `git` can emit megabytes on a bad push; an error that big is unusable in a
/// notification and would bloat every journal row that stores it.
const STDERR_CAP: usize = 2_048;

/// What the discovered `git` binary can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitCapabilities {
    /// Major version as reported by `git --version`.
    pub major: u32,
    /// Minor version. Vendor suffixes (`2.39.5 (Apple Git-154)`) are ignored.
    pub minor: u32,
    /// `sparse-checkout set --cone` behaves as AD-47 requires.
    pub sparse_cone: bool,
    /// `ls-files --format` is available.
    pub ls_files_format: bool,
}

impl GitCapabilities {
    /// Whether this binary clears the 2.42 floor in full.
    pub fn meets_floor(&self) -> bool {
        self.sparse_cone && self.ls_files_format
    }
}

/// Which engine drives the verbs this module exists for (Epic 66, AD-198).
///
/// A desktop has a `git` binary and the shim spawns it. A phone has none —
/// iOS denies `posix_spawn` to third-party apps, so `Command::spawn` can only
/// ever return `Err` there — and keeper does what it can in-process with
/// gitoxide (fetch, fast-forward checkout, commit, its own push over HTTP) and
/// refuses the rest with a sentence. The variant is decided by the target at
/// compile time ([`GitEngine::HOST`]) and can be *set* by a test, which is what
/// lets the Linux suite cover the phone's branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitEngine {
    /// A `git` binary the host resolved; every verb spawns it.
    Binary,
    /// gitoxide alone. Nothing here spawns; the four verbs refuse by name.
    Gix,
}

impl GitEngine {
    /// The engine this build runs: gitoxide on a phone, a binary elsewhere.
    pub const HOST: GitEngine = if cfg!(target_os = "ios") {
        GitEngine::Gix
    } else {
        GitEngine::Binary
    };

    /// The word `SyncGitVm.engine` carries.
    pub fn name(self) -> &'static str {
        match self {
            Self::Binary => "git",
            Self::Gix => "gix",
        }
    }
}

/// One of the things the shim is asked to do, for the phone's refusal.
///
/// Coarser than the subcommand list on purpose: the sentence names the
/// in-process route keeper takes instead, and several subcommands share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// `git push`.
    Push,
    /// `merge`, `switch`, `symbolic-ref`, `rev-parse`: moving or reading
    /// `HEAD` and the working tree.
    Checkout,
    /// `merge-base`, `merge-base --is-ancestor`, `diff --name-only`: reading
    /// history.
    History,
    /// `worktree add|remove|prune`.
    Worktree,
    /// `sparse-checkout set|disable`.
    Sparse,
    /// `gc`.
    Gc,
    /// `git --version`.
    Probe,
}

/// The sentence a phone answers a shim verb with (AD-198).
///
/// Each names the device and the route keeper takes instead, or why there is
/// none — a refusal without a next step is a support ticket. Pure, so the
/// boundary test can assert every sentence without spawning anything.
pub fn phone_refusal(verb: Verb) -> String {
    let sentence = match verb {
        Verb::Push => {
            "this is a phone: keeper pushes with its own engine here, never through a git binary"
        }
        Verb::Checkout => {
            "this is a phone: keeper checks out with its own engine here, and a history it cannot \
             fast-forward is named rather than merged"
        }
        Verb::History => "this is a phone: keeper reads history with its own engine here",
        Verb::Worktree => {
            "this is a phone: a review lane needs a linked worktree, which only a git binary can \
             make — run this lane on the Mac"
        }
        Verb::Sparse => {
            "this is a phone: a folder here is fully virtual rather than sparse, so subpaths are \
             not applied"
        }
        Verb::Gc => "this is a phone: keeper does not repack here; the Mac keeps this folder's objects bounded",
        Verb::Probe => "this is a phone: there is no git binary to probe, and keeper does not need one",
    };
    sentence.to_owned()
}

/// A typed handle onto one `git` binary — or, on a phone, onto the refusal.
#[derive(Debug, Clone)]
pub struct GitCli {
    program: PathBuf,
    engine: GitEngine,
}

impl GitCli {
    /// Bind to the binary the host resolved through
    /// [`SyncPlatform::git_program`](crate::platform::SyncPlatform::git_program).
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            engine: GitEngine::Binary,
        }
    }

    /// The phone's handle: no binary, every verb refused before a spawn.
    pub fn phone() -> Self {
        Self {
            program: PathBuf::new(),
            engine: GitEngine::Gix,
        }
    }

    /// Which engine this handle drives.
    pub fn engine(&self) -> GitEngine {
        self.engine
    }

    /// The one gate every verb passes before [`capture`] is reached.
    ///
    /// Checked on the handle rather than on `cfg!` so a test on the Linux box
    /// can build a phone handle and read the sentence back, which is the only
    /// way the phone's branch is ever asserted on a machine that has `git`.
    fn refuse_on_phone(&self, verb: Verb) -> Result<()> {
        match self.engine {
            GitEngine::Binary => Ok(()),
            GitEngine::Gix => Err(SyncError::GitMissing {
                reason: phone_refusal(verb),
            }),
        }
    }

    /// The binary this handle drives.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Version and floor check for this binary — what `doctor` reports.
    pub fn capabilities(&self) -> Result<GitCapabilities> {
        self.refuse_on_phone(Verb::Probe)?;
        probe(&self.program)
    }

    /// Push `refspec` to `remote`.
    ///
    /// There is deliberately **no force variant reachable from outside this
    /// module**. AD-50 forbids force-pushing a lane: a bot's work-in-progress
    /// must never overwrite review feedback a human already pushed. The arg
    /// builder still knows how to spell `--force` so the test can prove the
    /// public path does not use it.
    ///
    /// `credential` is the profile's own secret. It is the *only* one this push
    /// may use: the inherited `credential.helper` chain is cleared either way,
    /// so a profile with no credential fails as unauthenticated instead of
    /// quietly pushing as whichever account the OS git store holds for that
    /// host. See [`super::repo::clone`] for what that silent substitution looks
    /// like from the outside.
    pub fn push(
        &self,
        repo: &Path,
        remote: &str,
        refspec: &str,
        credential: Option<&Credential>,
    ) -> Result<()> {
        self.refuse_on_phone(Verb::Push)?;
        self.run_as("push", repo, &push_args(remote, refspec, false), credential)
            .map(drop)
    }

    /// Materialize a linked worktree at `path` on a new branch (AD-50).
    pub fn worktree_add(&self, repo: &Path, path: &Path, branch: &str) -> Result<()> {
        self.refuse_on_phone(Verb::Worktree)?;
        self.run("worktree add", repo, &worktree_add_args(path, branch)?)
            .map(drop)
    }

    /// Remove a linked worktree.
    pub fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<()> {
        self.refuse_on_phone(Verb::Worktree)?;
        self.run("worktree remove", repo, &worktree_remove_args(path)?)
            .map(drop)
    }

    /// Drop administrative records for worktrees whose directory is gone.
    pub fn worktree_prune(&self, repo: &Path) -> Result<()> {
        self.refuse_on_phone(Verb::Worktree)?;
        self.run("worktree prune", repo, &worktree_prune_args())
            .map(drop)
    }

    /// Restrict the checkout to `subpaths` in cone mode (AD-47).
    pub fn sparse_set(&self, repo: &Path, subpaths: &[String]) -> Result<()> {
        self.refuse_on_phone(Verb::Sparse)?;
        self.run("sparse-checkout set", repo, &sparse_set_args(subpaths)?)
            .map(drop)
    }

    /// Return to a full checkout.
    pub fn sparse_disable(&self, repo: &Path) -> Result<()> {
        self.refuse_on_phone(Verb::Sparse)?;
        self.run("sparse-checkout disable", repo, &sparse_disable_args())
            .map(drop)
    }

    /// Repack loose objects. Sync churn produces a lot of them and gitoxide has
    /// no maintenance path at all, so this is the only thing keeping a
    /// long-lived profile's object store bounded.
    pub fn gc(&self, repo: &Path) -> Result<()> {
        self.refuse_on_phone(Verb::Gc)?;
        self.run("gc", repo, &gc_args()).map(drop)
    }

    /// Advance the current branch to `reference`, refusing anything that is not
    /// a fast-forward.
    ///
    /// gitoxide implements no merge, reset, restore or switch workflow at all
    /// (`crate-status.md` lists the whole family as unimplemented), so applying
    /// fetched commits to the working tree is the fifth operation that has to
    /// shell out. `--ff-only` is deliberate: if the branches diverged this must
    /// fail loudly so the caller runs the conflict-copy path instead of
    /// silently creating a merge commit nobody asked for.
    pub fn merge_ff_only(&self, repo: &Path, reference: &str) -> Result<()> {
        self.refuse_on_phone(Verb::Checkout)?;
        self.run("merge --ff-only", repo, &merge_ff_only_args(reference)?)
            .map(drop)
    }

    /// Merge `reference`, resolving every content conflict in the remote's
    /// favour.
    ///
    /// This is only ever called *after* the local revision of each contested
    /// path has been preserved as a conflict copy (AD-43), so "theirs wins" is
    /// a naming decision, not a data-loss one.
    pub fn merge_theirs(&self, repo: &Path, reference: &str, message: &str) -> Result<()> {
        self.refuse_on_phone(Verb::Checkout)?;
        self.run(
            "merge -X theirs",
            repo,
            &merge_theirs_args(reference, message)?,
        )
        .map(drop)
    }

    /// The merge base of two commits, used to work out which side changed what.
    pub fn merge_base(&self, repo: &Path, a: &str, b: &str) -> Result<String> {
        self.refuse_on_phone(Verb::History)?;
        let out = self.run("merge-base", repo, &merge_base_args(a, b)?)?;
        Ok(out.trim().to_owned())
    }

    /// Check out `branch`, creating it at the current HEAD if it is new.
    ///
    /// The lane primitive (AD-50): a bot writes on a generated branch so the
    /// base branch is never touched and a human's review is never bypassed.
    /// gitoxide implements no `switch`/`checkout` workflow, so this is the
    /// shim's job.
    pub fn ensure_branch(&self, repo: &Path, branch: &str) -> Result<()> {
        self.refuse_on_phone(Verb::Checkout)?;
        let exists = self
            .run("rev-parse --verify", repo, &rev_parse_verify_args(branch)?)
            .is_ok();
        let args = switch_args(branch, !exists)?;
        self.run("switch", repo, &args).map(drop)
    }

    /// The branch HEAD currently points at, or `None` when detached.
    pub fn current_branch(&self, repo: &Path) -> Result<Option<String>> {
        self.refuse_on_phone(Verb::Checkout)?;
        let out = self.run(
            "symbolic-ref",
            repo,
            &[
                "symbolic-ref".to_owned(),
                "--quiet".to_owned(),
                "--short".to_owned(),
                "HEAD".to_owned(),
            ],
        );
        match out {
            Ok(text) => {
                let name = text.trim().to_owned();
                Ok((!name.is_empty()).then_some(name))
            }
            // A detached HEAD exits non-zero with no message; not a failure.
            Err(SyncError::GitCommand { .. }) => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// Is `ancestor` reachable from `descendant`?
    ///
    /// This is what separates the three shapes a fetch can leave behind, which
    /// a single "can fast-forward" boolean cannot: local behind (apply),
    /// local ahead (nothing to apply, just push), and genuinely diverged
    /// (conflict copies). Conflating "ahead" with "diverged" makes the engine
    /// merge-loop against a remote it is simply ahead of.
    ///
    /// `git merge-base --is-ancestor` answers with its exit status: 0 yes,
    /// 1 no. Only 1 is a real "no" — any other failure is propagated.
    pub fn is_ancestor(&self, repo: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
        self.refuse_on_phone(Verb::History)?;
        // Exit 1 is this command's ANSWER, not a failure, so it must not go
        // through the warn-logging path — the supervisor asks on every tick and
        // would otherwise fill the log with warnings about nothing.
        let args = is_ancestor_args(ancestor, descendant)?;
        let output = capture(&self.program, Some(repo), &args, None)?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => {
                let raw = String::from_utf8_lossy(&output.stderr);
                let stderr = truncate(&scrub_userinfo(&raw), STDERR_CAP);
                tracing::warn!(
                    subcommand = "merge-base --is-ancestor",
                    %stderr,
                    "git subcommand failed"
                );
                Err(SyncError::GitCommand {
                    subcommand: "merge-base --is-ancestor",
                    code: output.status.code().unwrap_or(-1),
                    stderr,
                })
            }
        }
    }

    /// Paths that differ between two commits, repository-relative.
    pub fn diff_names(&self, repo: &Path, from: &str, to: &str) -> Result<Vec<PathBuf>> {
        self.refuse_on_phone(Verb::History)?;
        let out = self.run("diff --name-only", repo, &diff_names_args(from, to)?)?;
        Ok(out
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect())
    }

    /// Run one subcommand, returning its stdout.
    ///
    /// `subcommand` is a `&'static str` label used only for the error variant,
    /// so it can never carry interpolated user data. `args` is the complete
    /// argument vector — there is no shell anywhere in this path.
    fn run(&self, subcommand: &'static str, repo: &Path, args: &[String]) -> Result<String> {
        self.run_as(subcommand, repo, args, None)
    }

    /// [`Self::run`], with a credential for the commands that talk to a remote.
    fn run_as(
        &self,
        subcommand: &'static str,
        repo: &Path,
        args: &[String],
        credential: Option<&Credential>,
    ) -> Result<String> {
        // `-c` settings have to precede the subcommand, so the vector is built
        // here rather than in the per-command arg builders — which stay pure
        // and testable.
        let args: Vec<String> = credential_config_args(credential)
            .into_iter()
            .chain(args.iter().cloned())
            .collect();
        let args = args.as_slice();
        let output = capture(&self.program, Some(repo), args, credential)?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() {
            return Ok(stdout);
        }

        // `git` writes its diagnostics to stderr but occasionally reports a
        // rejection on stdout (`push --porcelain`), so both are classified.
        let raw = format!("{}{stdout}", String::from_utf8_lossy(&output.stderr));
        let stderr = truncate(&scrub_userinfo(&raw), STDERR_CAP);
        tracing::warn!(subcommand, %stderr, "git subcommand failed");

        let hints: Vec<&str> = args.iter().map(String::as_str).collect();
        // The shim is handed a remote *name*, not a URL, so it has no host to
        // offer; whatever git echoed in its own message is the best source.
        if let Some(classified) = classify_message(&stderr, &repo_label(repo), None, &hints) {
            return Err(classified);
        }
        Err(SyncError::GitCommand {
            subcommand,
            // A signal-killed `git` has no exit code; -1 says "died" without
            // pretending to know why.
            code: output.status.code().unwrap_or(-1),
            stderr,
        })
    }
}

/// Parse `git --version`.
///
/// A binary that cannot be executed, or that answers with something other than
/// a version, is reported as [`SyncError::GitMissing`] rather than
/// [`SyncError::GitCommand`]: for the caller both mean "there is no usable git
/// here", and collapsing them keeps `doctor` down to one branch.
pub fn version(program: &Path) -> Result<(u32, u32)> {
    version_detail(program).map_err(|detail| SyncError::GitMissing {
        reason: format!("{}: {detail}", program.display()),
    })
}

/// `git --version`, with the failure detail unwrapped from any sentence.
///
/// Two callers want two shapes of one fact. [`version`] names the program,
/// because a standalone `GitMissing` that does not say which binary failed is
/// unactionable. [`super::resolve`] renders one line per candidate and has
/// already printed the path, so it wants the detail alone.
///
/// The detail carries `git`'s own `stderr`, and that is the point rather than a
/// nicety: a git whose system `gitconfig` is malformed exits non-zero on
/// `--version` and says exactly why (`bad config line 44 in file …`). Reporting
/// only "exited non-zero" handed the user a fault that had named its own cause.
/// Scrubbed and capped like every other diagnostic that leaves this module, and
/// folded onto one line because it lands in a settings row.
pub(crate) fn version_detail(program: &Path) -> std::result::Result<(u32, u32), String> {
    let args = [String::from("--version")];
    let output = capture(program, None, &args, None).map_err(|err| err.to_string())?;
    if !output.status.success() {
        let stderr = one_line(&scrub_userinfo(&String::from_utf8_lossy(&output.stderr)));
        return Err(format!("`git --version` failed: {stderr}"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_version(&text).ok_or_else(|| {
        // Not "is not a version": `Python 3.13.0` *is* a version, and saying so
        // sent the reader looking for an upgrade instead of at the binary they
        // had pointed keeper at. What is missing is git's own `git version`
        // wording, so the detail says that.
        format!(
            "`git --version` answered `{}`, which is not git reporting its version",
            one_line(&text)
        )
    })
}

/// Version plus the 2.42 floor check.
pub fn probe(program: &Path) -> Result<GitCapabilities> {
    let (major, minor) = version(program)?;
    Ok(capabilities_of(major, minor))
}

/// Project a version onto the capability set, without spawning anything.
///
/// Split out so [`super::resolve`] derives the same capabilities from the same
/// version it just read, rather than probing a second time — and so the floor is
/// applied in exactly one place for both callers.
pub(crate) fn capabilities_of(major: u32, minor: u32) -> GitCapabilities {
    let clears = clears_floor(major, minor);
    GitCapabilities {
        major,
        minor,
        sparse_cone: clears,
        ls_files_format: clears,
    }
}

/// Whether `<major>.<minor>` is at or above the 2.42 floor.
///
/// Compared as a pair, not field by field: a naive `minor >= 42` would reject
/// git 3.0 and accept git 1.99.
fn clears_floor(major: u32, minor: u32) -> bool {
    (major, minor) >= (MIN_GIT_MAJOR, MIN_GIT_MINOR)
}

/// The one place a process is created.
///
/// Environment hardening lives here so no call site can forget it.
fn capture(
    program: &Path,
    cwd: Option<&Path>,
    args: &[String],
    credential: Option<&Credential>,
) -> Result<Output> {
    // Prepended rather than passed through: `-c` only counts before the
    // subcommand, and hardening a call site can forget is not hardening.
    let args: Vec<String> = repository_config_args(cwd)
        .into_iter()
        .chain(args.iter().cloned())
        .collect();
    let args = args.as_slice();

    let mut command = Command::new(program);
    if let Some(credential) = credential {
        // Read back only by `CREDENTIAL_HELPER`, which git spawns as a child of
        // this process and which therefore inherits them.
        command
            .env(CREDENTIAL_USERNAME_ENV, &credential.username)
            .env(CREDENTIAL_SECRET_ENV, &credential.secret);
    }
    command
        .args(args)
        // No terminal, no askpass, no GUI prompt: a private remote must fail
        // fast with an auth error instead of blocking a daemon forever.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("SSH_ASKPASS_REQUIRE", "never")
        // `git` localizes its diagnostics and the classifier below matches the
        // English wording, so the locale is pinned rather than translated.
        .env("LC_ALL", "C")
        .env("LANG", "C")
        // An inherited stdin is another way to hang: nothing here is
        // interactive.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    // Scrubbed even at trace level: an argument vector routinely carries the
    // remote URL, and a URL routinely carries a token.
    tracing::trace!(
        program = %program.display(),
        args = %scrub_userinfo(&args.join(" ")),
        "spawning git"
    );

    command.output().map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            SyncError::GitMissing {
                reason: format!("{} does not exist or is not executable", program.display()),
            }
        } else {
            SyncError::io("spawn git", program.to_path_buf(), source)
        }
    })
}

/// `git push` argument vector.
///
/// `--` ends option parsing so a remote name or refspec that begins with `-`
/// becomes a positional argument instead of a flag. `--porcelain` gives a
/// stable machine-readable report of what each ref did.
/// The `-c` settings that decide which credential a remote-touching `git` may
/// use, prepended to every invocation.
///
/// The empty `credential.helper` comes first and is not optional: git treats an
/// empty value as "reset the list", so this drops whatever the user's global,
/// system and repository config chained on — `osxkeychain`, a manager, a cache.
/// Leaving those in place is what lets a push authenticate as an unrelated
/// account that happens to be stored for the same host, which fails per
/// repository depending on that account's access and never says why.
/// A `core.hooksPath` that can never be a directory, so git finds no hook to
/// run and says nothing about it.
///
/// `/dev/null` is a character device, so every lookup below it fails with
/// `ENOTDIR` rather than merely being absent — which matters, because an
/// *absent* path is one a user or another process could later create, and this
/// value ends up on the command line of a process that writes to their
/// repository. The Windows spelling of the same trick is the `NUL` device.
#[cfg(not(windows))]
const NO_HOOKS_PATH: &str = "/dev/null/keeper-runs-no-repository-hooks";
#[cfg(windows)]
const NO_HOOKS_PATH: &str = r"NUL\keeper-runs-no-repository-hooks";

/// The `-c` settings every invocation *inside a repository* carries.
///
/// # Repository hooks are never run
///
/// keeper commits, merges and checks out through gitoxide, which has no hook
/// support at all — so for the whole write half of the engine the user's hooks
/// have never run. The four things the shim exists for (AD-41) are the only
/// operations that could still fire one, and firing one there would make the
/// engine's behaviour depend on which half of itself did the work.
///
/// The failure that made this load-bearing is not hypothetical. A `git lfs
/// install` run by hand inside a synced folder writes stock `pre-push`,
/// `post-checkout`, `post-commit` and `post-merge` hooks, and every one of them
/// begins by refusing to run unless `git-lfs` is on `PATH`. keeper *is* the LFS
/// implementation for a folder it manages — it registers its own
/// `filter.lfs.clean`/`smudge`, uploads through its own journal and prunes
/// local objects it has already replicated — so those hooks are wrong even when
/// git-lfs is installed: `git lfs pre-push` walks a store keeper has
/// deliberately emptied. On a desktop launch, where `PATH` is Finder's rather
/// than a shell's, they simply made every push fail with a message about a
/// binary keeper never needed.
///
/// A user's own hooks are silenced too, and that is the intended reading: an
/// unattended engine converging a folder in the background is not the event a
/// `pre-push` was written to gate. Nothing keeper does depends on a hook, so
/// there is no case in which running one is required and every case in which
/// running one is a surprise.
///
/// `cwd` gates this because it is exactly the "are we in a repository" question:
/// [`version_detail`] probes a binary with no repository in sight, and a `-c`
/// on that call would only make the diagnostic vector harder to read.
fn repository_config_args(cwd: Option<&Path>) -> Vec<String> {
    if cwd.is_none() {
        return Vec::new();
    }
    vec!["-c".to_owned(), format!("core.hooksPath={NO_HOOKS_PATH}")]
}

fn credential_config_args(credential: Option<&Credential>) -> Vec<String> {
    let mut args = vec!["-c".to_owned(), "credential.helper=".to_owned()];
    if credential.is_some() {
        args.push("-c".to_owned());
        args.push(format!("credential.helper={CREDENTIAL_HELPER}"));
    }
    args
}

fn push_args(remote: &str, refspec: &str, force: bool) -> Vec<String> {
    let mut args = vec!["push".to_owned(), "--porcelain".to_owned()];
    if force {
        args.push("--force".to_owned());
    }
    args.push("--".to_owned());
    args.push(remote.to_owned());
    args.push(refspec.to_owned());
    args
}

/// `git worktree add` argument vector.
///
/// `-b <branch>` consumes its value whatever it looks like, so the branch needs
/// no guard; the destination is positional and is therefore required to be
/// absolute, which also makes it impossible for it to start with `-`.
fn worktree_add_args(path: &Path, branch: &str) -> Result<Vec<String>> {
    Ok(vec![
        "worktree".to_owned(),
        "add".to_owned(),
        // A lane branch is ours alone; tracking a remote branch would make a
        // later push ambiguous.
        "--no-track".to_owned(),
        "-b".to_owned(),
        branch.to_owned(),
        absolute_arg(path)?,
    ])
}

/// `git worktree remove` argument vector.
fn worktree_remove_args(path: &Path) -> Result<Vec<String>> {
    Ok(vec![
        "worktree".to_owned(),
        "remove".to_owned(),
        absolute_arg(path)?,
    ])
}

/// `git worktree prune` argument vector.
fn worktree_prune_args() -> Vec<String> {
    vec!["worktree".to_owned(), "prune".to_owned()]
}

/// `git sparse-checkout set --cone` argument vector.
///
/// Cone mode takes bare directory paths, and `sparse-checkout set` has no `--`
/// separator, so a subpath beginning with `-` would be parsed as an option. It
/// is rejected instead: a directory literally named `-foo` cannot be a sparse
/// root, and that is a far smaller loss than an argument-injection hole.
fn sparse_set_args(subpaths: &[String]) -> Result<Vec<String>> {
    let mut args = vec![
        "sparse-checkout".to_owned(),
        "set".to_owned(),
        "--cone".to_owned(),
    ];
    args.reserve(subpaths.len());
    for subpath in subpaths {
        if subpath.starts_with('-') {
            return Err(SyncError::Config(format!(
                "sparse subpath must not begin with '-': {subpath}"
            )));
        }
        if subpath.trim().is_empty() {
            return Err(SyncError::Config(
                "sparse subpath must not be empty".to_owned(),
            ));
        }
        args.push(subpath.clone());
    }
    Ok(args)
}

/// `git sparse-checkout disable` argument vector.
fn sparse_disable_args() -> Vec<String> {
    vec!["sparse-checkout".to_owned(), "disable".to_owned()]
}

/// `git gc` argument vector.
fn gc_args() -> Vec<String> {
    vec!["gc".to_owned(), "--quiet".to_owned()]
}

/// Reject a ref name that could be read as an option.
///
/// `merge`, `merge-base` and `diff` all take refs positionally and none of them
/// accepts a `--` separator in the shape we need, so a ref beginning with `-`
/// is the one injection vector left open. Branch names come from a profile the
/// user edits, so this is not theoretical.
fn safe_ref(reference: &str) -> Result<String> {
    if reference.is_empty() || reference.starts_with('-') {
        return Err(SyncError::Config(format!(
            "refusing a git reference that could be parsed as an option: {reference:?}"
        )));
    }
    Ok(reference.to_owned())
}

/// `git merge --ff-only <ref>` argument vector.
fn merge_ff_only_args(reference: &str) -> Result<Vec<String>> {
    Ok(vec![
        "merge".to_owned(),
        "--ff-only".to_owned(),
        "--quiet".to_owned(),
        safe_ref(reference)?,
    ])
}

/// `git merge -X theirs -X no-renames <ref>` argument vector.
///
/// `--no-edit` and an explicit `-m` keep git from opening an editor, which
/// would hang a headless daemon forever.
///
/// # Rename detection is off, and that is what keeps the folder syncing
///
/// A rename is a delete plus an add of the same bytes, and keeper reconciles
/// machine-generated trees where nobody is going to read a rename as such: the
/// content is identical either way, and `-X theirs` already decides every
/// content conflict. What detection does add is a failure mode. Above
/// `merge.renameLimit` git gives up on it — "exhaustive rename detection was
/// skipped due to too many files" — and the fallback turns one side's
/// reorganization into rename/delete conflicts, one per path. Measured on the
/// folder that reported it, a housekeeping pass that moved 128,483 files out of
/// an inbox produced **138,311 unmerged paths** and `fatal: Exiting because of
/// an unresolved conflict`: an unattended engine cannot resolve that, so the
/// profile stopped syncing entirely and sat at "Idle · N waiting to sync" while
/// the queue behind it never moved.
///
/// With detection off the same pass merges cleanly — our deletion of the old
/// path and their addition of the new one are independent edits — and it is
/// also markedly faster, because the O(n²) similarity search never runs.
fn merge_theirs_args(reference: &str, message: &str) -> Result<Vec<String>> {
    Ok(vec![
        "merge".to_owned(),
        "--no-edit".to_owned(),
        "--quiet".to_owned(),
        // Adopting an existing folder gives the local side its own root
        // commit, so reconciling it with the remote is a merge of unrelated
        // histories. git refuses that by default as a safety net against a
        // mistyped remote; here it is the expected shape.
        "--allow-unrelated-histories".to_owned(),
        "-s".to_owned(),
        "ort".to_owned(),
        "-X".to_owned(),
        "theirs".to_owned(),
        "-X".to_owned(),
        "no-renames".to_owned(),
        "-m".to_owned(),
        // Passed verbatim, newlines and all. The message is a single argv
        // element handed to git without a shell, so a newline cannot start
        // another argument - and flattening it would fold the provenance
        // trailer block onto the subject line, where git stops recognising it
        // as trailers at all. Carriage returns still go: they would survive
        // into the stored message and show up as stray ^M.
        message.replace('\r', ""),
        safe_ref(reference)?,
    ])
}

/// `git merge-base <a> <b>` argument vector.
fn merge_base_args(a: &str, b: &str) -> Result<Vec<String>> {
    Ok(vec!["merge-base".to_owned(), safe_ref(a)?, safe_ref(b)?])
}

/// `git rev-parse --verify <ref>` argument vector.
fn rev_parse_verify_args(reference: &str) -> Result<Vec<String>> {
    Ok(vec![
        "rev-parse".to_owned(),
        "--verify".to_owned(),
        "--quiet".to_owned(),
        format!("refs/heads/{}", safe_ref(reference)?),
    ])
}

/// `git switch [-c] <branch>` argument vector.
fn switch_args(branch: &str, create: bool) -> Result<Vec<String>> {
    let mut args = vec!["switch".to_owned(), "--quiet".to_owned()];
    if create {
        args.push("-c".to_owned());
    }
    args.push(safe_ref(branch)?);
    Ok(args)
}

/// `git merge-base --is-ancestor <a> <b>` argument vector.
fn is_ancestor_args(ancestor: &str, descendant: &str) -> Result<Vec<String>> {
    Ok(vec![
        "merge-base".to_owned(),
        "--is-ancestor".to_owned(),
        safe_ref(ancestor)?,
        safe_ref(descendant)?,
    ])
}

/// `git diff --name-only <from> <to>` argument vector.
fn diff_names_args(from: &str, to: &str) -> Result<Vec<String>> {
    Ok(vec![
        "diff".to_owned(),
        "--name-only".to_owned(),
        safe_ref(from)?,
        safe_ref(to)?,
    ])
}

/// Render an absolute path as a command argument.
///
/// Relative paths are refused because they would resolve against the
/// repository's directory rather than the caller's intent, and because an
/// absolute path can never be mistaken for an option.
fn absolute_arg(path: &Path) -> Result<String> {
    if !path.is_absolute() {
        return Err(SyncError::Config(format!(
            "worktree path must be absolute, got {}",
            path.display()
        )));
    }
    path.to_str().map(str::to_owned).ok_or_else(|| {
        SyncError::Config(format!(
            "worktree path is not valid UTF-8 and cannot be passed to git: {}",
            path.display()
        ))
    })
}

/// Parse the first line of `git --version`.
///
/// Two obligations, in this order, and the first one is the whole point: the
/// line must **identify itself as git**, and only then is a version read out of
/// it. Reading the first digit-leading token on its own accepted any executable
/// that prints a version — `python3 --version` answers `Python 3.13.0`, which
/// parsed as `(3, 13)`, cleared the 2.42 floor, and made
/// [`super::resolve::GitResolution::summary`] report `git 3.13` for a binary
/// that is not git. The engine then drove it for every push, merge and worktree
/// call, so the first symptom was an unclassifiable `GitCommand` failure deep
/// inside a sync rather than a rejection at resolution time — and a wrapper or
/// shim called `git` ahead of the real one on `PATH`, the exact fault Story
/// 34.14 exists to catch, was accepted in silence.
///
/// `git version` is the only wording git has ever printed here, on every
/// platform and in every locale (the string is not translated), so requiring it
/// costs nothing a real git has. What follows it varies: vendors append their
/// own suffixes (`2.39.5 (Apple Git-154)`, `2.53.0.windows.1`), so only the
/// leading `<major>.<minor>` is read and the rest is ignored.
pub(crate) fn parse_version(output: &str) -> Option<(u32, u32)> {
    const IDENTITY: &str = "git version";
    let token = output
        .lines()
        .next()?
        .trim_start()
        .strip_prefix(IDENTITY)?
        .split_whitespace()
        .next()
        .filter(|token| token.starts_with(|c: char| c.is_ascii_digit()))?;
    let mut parts = token.split('.');
    let major = parts.next()?.parse().ok()?;
    // A hypothetical bare `git version 3` is still a usable answer.
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

/// Remove `://user:password@` userinfo from every URL in `text`.
///
/// The whole userinfo goes, not just the password: with token auth the *user*
/// field is routinely the secret (a GitHub PAT paired with a dummy password).
pub(crate) fn scrub_userinfo(text: &str) -> String {
    const MARK: &str = "://";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(idx) = rest.find(MARK) {
        let (head, tail) = rest.split_at(idx + MARK.len());
        out.push_str(head);
        // The authority ends at the first delimiter; anything after that is a
        // path or prose and must not be scanned for an `@`.
        let authority_end = tail
            .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace())
            .unwrap_or(tail.len());
        match tail[..authority_end].rfind('@') {
            Some(at) => {
                out.push_str("***@");
                rest = &tail[at + 1..];
            }
            None => {
                out.push_str(&tail[..authority_end]);
                rest = &tail[authority_end..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Cut `text` to at most `cap` bytes, on a character boundary.
pub(crate) fn truncate(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.trim().to_owned();
    }
    let end = text
        .char_indices()
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx <= cap)
        .last()
        .unwrap_or(0);
    format!("{}…", text[..end].trim_end())
}

/// Extract a host from a remote URL.
///
/// `url::Url` handles every real scheme, but git's most common remote form —
/// `git@host:path` — is **not** a URL at all (no scheme, and the `:` introduces
/// a path rather than a port), so it is parsed by hand.
pub(crate) fn host_from_url(raw: &str) -> Option<String> {
    let raw = raw.trim().trim_matches(['\'', '"']);
    let parsed = url::Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned));
    if parsed.is_some() {
        return parsed;
    }

    // scp-style `[user@]host:path`. A `/` before the `:` means this is a plain
    // filesystem path, which has no host.
    let after_user = raw.rsplit_once('@').map_or(raw, |(_, rest)| rest);
    let (host, path) = after_user.split_once(':')?;
    if host.is_empty() || host.contains('/') || path.is_empty() {
        return None;
    }
    Some(host.to_owned())
}

/// Map a `git` or transport diagnostic onto a typed error.
///
/// Returns `None` when the text matches none of the shapes we understand, so
/// the caller can fall back to its own generic variant. Order matters: an HTTP
/// 403 arrives wrapped in `unable to access '<url>'`, which also looks like a
/// network failure, so authentication is decided first.
///
/// `label` names the repository for [`SyncError::Diverged`] — this module knows
/// the working directory, not the profile, and AD-42 binds exactly one profile
/// to one folder, so the folder identifies it unambiguously.
///
/// `host` is the caller's own answer for "which remote was this", used verbatim
/// when it has one. `url_hints` are strings that *may* contain a URL (an
/// argument vector), tried only after the message itself.
pub(crate) fn classify_message(
    text: &str,
    label: &str,
    host: Option<&str>,
    url_hints: &[&str],
) -> Option<SyncError> {
    let lower = text.to_ascii_lowercase();

    const AUTH: [&str; 12] = [
        "authentication failed",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "invalid username or password",
        "permission denied (publickey",
        "403 forbidden",
        "the requested url returned error: 403",
        // gitoxide's HTTP transport turns a 401 into an `io::Error` and reports
        // it as "An IO error occurred when talking to the server" — wording
        // that matches the NETWORK family far better than the truth. Only the
        // status in the wrapped cause tells them apart, and without these two
        // needles a rejected credential is classified `Git`, which is
        // `Transient`: the profile then retries a credential that will never
        // start working, forever.
        "received http status 401",
        "received http status 403",
        // With the helper chain cleared (see `credential_config_args`) a
        // profile that has no credential of its own reaches gitoxide's built-in
        // prompt, which then fails on the missing tty. "Failed to open terminal
        // at /dev/tty" is the last line of that story and matches nothing here;
        // the first line is the whole of it, and it is an auth problem, not a
        // retryable one.
        "failed to obtain credentials",
        "couldn't obtain username",
    ];
    if AUTH.iter().any(|needle| lower.contains(needle)) {
        return Some(SyncError::Auth {
            host: host_from_text(text, host, url_hints),
        });
    }

    const DIVERGED: [&str; 5] = [
        "non-fast-forward",
        "updates were rejected",
        "fetch first",
        "! [rejected]",
        "tip of your current branch is behind",
    ];
    if DIVERGED.iter().any(|needle| lower.contains(needle)) {
        return Some(SyncError::Diverged {
            profile: label.to_owned(),
            // Every line, not the first one. `git push` leads with "error:
            // failed to push some refs to '<url>'" — a summary naming the
            // remote and nothing else — and puts the reason underneath, or on
            // stdout under `--porcelain`. Reporting only the summary made every
            // rejected push in the field read identically whatever had
            // happened, telling the reader the one thing they already knew
            // (DW-207). This is the case [`one_line`] was written for: line one
            // is the symptom, line two is the cause, and a settings row shows
            // one line — so they are joined rather than chosen between.
            reason: one_line(text),
        });
    }

    const NETWORK: [&str; 10] = [
        "could not resolve host",
        "connection refused",
        "connection timed out",
        "connection reset",
        "network is unreachable",
        "failed to connect to",
        "unable to access",
        "the remote end hung up unexpectedly",
        "early eof",
        "operation timed out",
    ];
    if NETWORK.iter().any(|needle| lower.contains(needle)) {
        return Some(SyncError::Network {
            host: host_from_text(text, host, url_hints),
            reason: first_line(text),
        });
    }

    None
}

/// Best-effort host for an error message.
///
/// The caller's own answer wins when it has one: after a redirect the URL in
/// the message may name a host the user has never heard of, while the profile
/// names the one they configured. Otherwise: git's own
/// `Could not resolve host: <host>`, then any URL it quoted, then the
/// arguments we passed it.
fn host_from_text(text: &str, host: Option<&str>, url_hints: &[&str]) -> String {
    if let Some(host) = host.map(str::trim).filter(|host| !host.is_empty()) {
        return host.to_owned();
    }

    const RESOLVE: &str = "Could not resolve host: ";
    if let Some(idx) = text.find(RESOLVE) {
        let tail = &text[idx + RESOLVE.len()..];
        let found = tail
            .split(|c: char| c.is_whitespace() || c == '\'' || c == '"')
            .find(|token| !token.is_empty());
        if let Some(found) = found {
            return found.to_owned();
        }
    }

    for token in text.split('\'').skip(1).step_by(2) {
        if let Some(host) = host_from_url(token) {
            return host;
        }
    }
    for hint in url_hints {
        if let Some(host) = host_from_url(hint) {
            return host;
        }
    }
    "unknown".to_owned()
}

/// First non-empty line, for a one-line `reason` field.
fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("git reported no detail")
        .to_owned()
}

/// Every non-empty line, folded onto one with `; ` and capped.
///
/// [`first_line`] is right for a `GitCommand` whose first line is the failure;
/// it is wrong for a broken `gitconfig`, where line one is the symptom
/// (`could not expand include path …`) and line two is the cause (`bad config
/// line 44 in file …`). A settings row and a notification are both one line, so
/// the lines are joined rather than picked between.
pub(crate) fn one_line(text: &str) -> String {
    let joined = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    if joined.is_empty() {
        "git reported no detail".to_owned()
    } else {
        truncate(&joined, STDERR_CAP)
    }
}

/// Name a repository directory for an error message.
pub(crate) fn repo_label(repo: &Path) -> String {
    repo.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.display().to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_merge_message_keeps_the_blank_line_its_trailers_depend_on() {
        // git only recognises a trailer block in the LAST paragraph. Flattening
        // the message put Keeper-Device on the subject line, where
        // `git log --format=%(trailers:...)` returns nothing and the merge
        // becomes the one unattributable commit in the history.
        let message = "sync(media): merge remote changes\n\nKeeper-Profile: media\nKeeper-Device: electra (01K)\n";
        let args = merge_theirs_args("refs/remotes/origin/main", message).expect("args");
        let rendered = &args[args.iter().position(|a| a == "-m").expect("has -m") + 1];

        assert!(
            rendered.contains("\n\nKeeper-Profile:"),
            "got: {rendered:?}"
        );
        assert_eq!(rendered, message);
    }

    #[test]
    fn a_carriage_return_never_reaches_the_stored_message() {
        // A CRLF-authored profile name would otherwise leave stray ^M in every
        // merge commit.
        let args = merge_theirs_args("refs/heads/main", "subject\r\n\r\nKeeper-Profile: x\r\n")
            .expect("args");
        let rendered = &args[args.iter().position(|a| a == "-m").expect("has -m") + 1];
        assert!(!rendered.contains('\r'));
        assert!(rendered.contains("\n\nKeeper-Profile: x"));
    }

    use super::*;

    #[test]
    fn push_is_argument_vectored_and_never_forces() {
        assert_eq!(
            push_args("origin", "refs/heads/main:refs/heads/main", false),
            [
                "push",
                "--porcelain",
                "--",
                "origin",
                "refs/heads/main:refs/heads/main"
            ]
        );
    }

    /// One side moving a file while the other deletes it must MERGE, not stop.
    ///
    /// This is the shape a housekeeping pass takes: a script moves files out of
    /// an inbox and publishes the moves through a branch, while the local clone
    /// records only the removals. With rename detection on, git calls every one
    /// of those a rename/delete conflict — 138,311 of them on the folder that
    /// reported this — and an unattended engine has nothing to resolve them
    /// with, so the profile stops syncing. Driven through the real `git`, not
    /// asserted on the argument vector: the vector proves the flag is passed,
    /// and only git proves the flag is the right one.
    #[test]
    fn a_file_the_remote_moved_and_we_deleted_merges_instead_of_conflicting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(root.join("inbox")).expect("mkdir");
        std::fs::write(root.join("inbox/report.pdf"), b"the same bytes either way").expect("write");
        git(&["add", "-A"]);
        git(&["commit", "-qm", "base"]);

        // Their side: the housekeeping move, on a branch standing in for the
        // remote.
        git(&["checkout", "-q", "-b", "remote"]);
        std::fs::create_dir_all(root.join("records")).expect("mkdir");
        std::fs::rename(
            root.join("inbox/report.pdf"),
            root.join("records/report.pdf"),
        )
        .expect("move");
        git(&["add", "-A"]);
        git(&["commit", "-qm", "move it out of the inbox"]);

        // Our side: the same removal, and nothing else.
        git(&["checkout", "-q", "main"]);
        std::fs::remove_file(root.join("inbox/report.pdf")).expect("delete");
        git(&["add", "-A"]);
        git(&["commit", "-qm", "the drop is empty"]);

        GitCli::new(PathBuf::from("git"))
            .merge_theirs(root, "refs/heads/remote", "sync: merge remote changes\n")
            .expect("a move on one side and a delete on the other is not a conflict");
        assert!(
            root.join("records/report.pdf").is_file(),
            "the destination the remote published has to land here"
        );
        assert!(
            !root.join("inbox/report.pdf").exists(),
            "and the emptied inbox stays empty"
        );
    }

    #[test]
    fn the_inherited_credential_helper_chain_is_always_cleared() {
        // Without the reset, git falls through to the user's own helpers and a
        // push authenticates as whatever account the OS store holds for that
        // host — which fails per repository, depending on that account's
        // access, and reports itself as anything but an auth problem.
        assert_eq!(
            credential_config_args(None),
            ["-c", "credential.helper="],
            "a profile with no credential must not borrow one"
        );

        let credential = Credential {
            username: "tok3n".to_owned(),
            secret: "s3cret".to_owned(),
        };
        let args = credential_config_args(Some(&credential));
        assert_eq!(
            args[..2],
            ["-c", "credential.helper="],
            "the reset must come first, or the inherited helpers stay in the list"
        );
        assert_eq!(args.len(), 4);
        assert!(args[3].starts_with("credential.helper=!"));
    }

    #[test]
    fn a_repository_invocation_always_disables_hooks() {
        // A `git lfs install` run by hand in a synced folder leaves hooks that
        // refuse to run without git-lfs on PATH, which is how a desktop launch
        // lost every push. Nothing keeper does needs a hook, so none run.
        let args = repository_config_args(Some(Path::new("/w/folder")));
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "-c");
        assert!(
            args[1].starts_with("core.hooksPath="),
            "the hook path must be overridden, got {}",
            args[1]
        );
        assert!(
            !Path::new(args[1].trim_start_matches("core.hooksPath=")).is_dir(),
            "the sentinel must not be a directory anything could put a hook in"
        );

        // Probing a binary is not repository work, and a `-c` there would only
        // clutter the one diagnostic that gets read by a human.
        assert!(repository_config_args(None).is_empty());
    }

    #[test]
    fn the_credential_never_reaches_the_argument_vector() {
        // `ps` shows argv to every user on the box, and a push runs as long as
        // the transfer does. The helper names the variables; the values travel
        // in the environment.
        let credential = Credential {
            username: "tok3n".to_owned(),
            secret: "s3cret".to_owned(),
        };
        let rendered = credential_config_args(Some(&credential)).join(" ");

        assert!(!rendered.contains("tok3n"), "username leaked: {rendered}");
        assert!(!rendered.contains("s3cret"), "secret leaked: {rendered}");
        assert!(rendered.contains(CREDENTIAL_USERNAME_ENV));
        assert!(rendered.contains(CREDENTIAL_SECRET_ENV));
    }

    #[test]
    fn a_rejected_credential_is_authentication_not_a_retryable_transport_fault() {
        // gitoxide reports a 401 as "An IO error occurred when talking to the
        // server", with the status only in the wrapped cause. Read as a
        // transport fault it is `Transient`, and the profile then retries a
        // credential that will never begin to work.
        for status in ["401", "403"] {
            let message = format!(
                "An IO error occurred when talking to the server: Received HTTP status {status}"
            );
            let err = classify_message(&message, "profile", Some("git.example.com"), &[])
                .unwrap_or_else(|| panic!("{status} must classify"));

            assert_eq!(err.code(), "auth", "{message}");
            assert_eq!(
                err.retriability(),
                crate::error::Retriability::Permanent,
                "a rejected credential must park the profile, not spin against the host"
            );
        }
    }

    #[test]
    fn a_missing_credential_is_authentication_not_a_broken_terminal() {
        // What a profile with no stored secret hits once the inherited helper
        // chain is cleared: gitoxide falls through to its own prompt and the
        // prompt dies on the missing tty. Classified on the first line rather
        // than the last, or the profile would retry a secret nobody has yet.
        let message = "Failed to obtain credentials: Couldn't obtain Username for \
             https://git.example.com: : Failed to open terminal at \"/dev/tty\" for writing \
             prompt, or to write it: Device not configured (os error 6)";
        let err = classify_message(message, "profile", Some("git.example.com"), &[])
            .expect("a missing credential must classify");

        assert_eq!(err.code(), "auth");
        assert_eq!(err.retriability(), crate::error::Retriability::Permanent);
    }

    #[test]
    fn the_force_flag_exists_only_below_the_public_api() {
        // The builder can spell it; `GitCli::push` never asks for it (AD-50).
        let forced = push_args("origin", "main", true);
        assert!(forced.contains(&"--force".to_owned()));
        assert!(!push_args("origin", "main", false).contains(&"--force".to_owned()));
    }

    #[test]
    fn a_hostile_remote_name_stays_a_positional_argument() {
        let args = push_args("--upload-pack=/bin/sh", "main", false);
        let dashdash = args
            .iter()
            .position(|a| a == "--")
            .expect("the separator must be present");
        assert!(
            args.iter().position(|a| a == "--upload-pack=/bin/sh") > Some(dashdash),
            "a remote that looks like a flag must land after `--`: {args:?}"
        );
    }

    #[test]
    fn worktree_arguments_are_built_correctly() {
        let args = worktree_add_args(Path::new("/tmp/lane"), "keeper/p/01H").expect("absolute");
        assert_eq!(
            args,
            [
                "worktree",
                "add",
                "--no-track",
                "-b",
                "keeper/p/01H",
                "/tmp/lane"
            ]
        );
        assert_eq!(
            worktree_remove_args(Path::new("/tmp/lane")).expect("absolute"),
            ["worktree", "remove", "/tmp/lane"]
        );
        assert_eq!(worktree_prune_args(), ["worktree", "prune"]);
    }

    #[test]
    fn a_relative_worktree_path_is_refused_before_a_process_exists() {
        let err = worktree_add_args(Path::new("lane"), "b").expect_err("must reject");
        assert_eq!(err.code(), "config");
    }

    #[test]
    fn sparse_set_uses_cone_mode_and_rejects_an_option_shaped_subpath() {
        assert_eq!(
            sparse_set_args(&["docs".to_owned(), "src/app".to_owned()]).expect("plain subpaths"),
            ["sparse-checkout", "set", "--cone", "docs", "src/app"]
        );
        let err = sparse_set_args(&["--cone".to_owned()]).expect_err("must reject");
        assert_eq!(err.code(), "config");
        assert!(sparse_set_args(&["  ".to_owned()]).is_err());
    }

    #[test]
    fn gc_and_sparse_disable_vectors() {
        assert_eq!(gc_args(), ["gc", "--quiet"]);
        assert_eq!(sparse_disable_args(), ["sparse-checkout", "disable"]);
    }

    #[test]
    fn scrubbing_removes_the_password_and_the_username() {
        let scrubbed =
            scrub_userinfo("fatal: unable to access 'https://alice:s3cr3t@git.example.com/x.git/'");
        assert!(!scrubbed.contains("s3cr3t"), "{scrubbed}");
        assert!(!scrubbed.contains("alice"), "{scrubbed}");
        assert!(scrubbed.contains("git.example.com"), "{scrubbed}");
        assert!(scrubbed.contains("***@"), "{scrubbed}");
    }

    #[test]
    fn scrubbing_leaves_a_url_without_userinfo_intact() {
        let text = "fatal: unable to access 'https://git.example.com/x.git/'";
        assert_eq!(scrub_userinfo(text), text);
    }

    #[test]
    fn scrubbing_does_not_reach_past_the_authority_for_an_at_sign() {
        // The `@` lives in the path, not in userinfo; the host must survive.
        let text = "https://git.example.com/~user@home/x.git";
        assert_eq!(scrub_userinfo(text), text);
    }

    #[test]
    fn scrubbing_handles_several_urls_in_one_message() {
        let scrubbed = scrub_userinfo(
            "tried 'https://a:1@x.example/r' then 'https://b:2@y.example/r' and gave up",
        );
        assert!(
            !scrubbed.contains(":1@") && !scrubbed.contains(":2@"),
            "{scrubbed}"
        );
        assert!(scrubbed.contains("x.example") && scrubbed.contains("y.example"));
    }

    #[test]
    fn authentication_failures_become_auth_with_the_host() {
        let err = classify_message(
            "remote: Invalid username or password.\nfatal: Authentication failed for 'https://git.example.com/x.git/'",
            "tgdrive",
            None,
            &[],
        )
        .expect("classified");
        match err {
            SyncError::Auth { host } => assert_eq!(host, "git.example.com"),
            other => panic!("expected Auth, got {other:?}"),
        }
    }

    #[test]
    fn dns_failures_become_network_with_the_host() {
        let err = classify_message(
            "fatal: unable to access 'https://nope.example.com/x.git/': Could not resolve host: nope.example.com",
            "tgdrive",
            None,
            &[],
        )
        .expect("classified");
        match err {
            SyncError::Network { host, .. } => assert_eq!(host, "nope.example.com"),
            other => panic!("expected Network, got {other:?}"),
        }
    }

    #[test]
    fn a_rejected_push_becomes_diverged_not_a_generic_failure() {
        let err = classify_message(
            " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs\nhint: Updates were rejected because the remote contains work that you do not have locally.",
            "tgdrive",
            None,
            &[],
        )
        .expect("classified");
        match err {
            SyncError::Diverged { profile, .. } => assert_eq!(profile, "tgdrive"),
            other => panic!("expected Diverged, got {other:?}"),
        }
    }

    /// What the field actually shows, and why it showed nothing useful.
    ///
    /// `git push --porcelain` puts its rejection on stdout and leads stderr
    /// with a summary naming only the remote. keeper concatenates the two, so
    /// the *first* line is that summary — and reporting it meant every rejected
    /// push read "error: failed to push some refs to '<url>'" whatever had
    /// happened, which is the one thing the reader already knew (DW-207).
    #[test]
    fn a_rejection_is_reported_by_the_line_that_says_why() {
        let err = classify_message(
            "error: failed to push some refs to 'https://forge.example.com/o/r.git'\n\
             To https://forge.example.com/o/r.git\n\
             !\trefs/heads/main:refs/heads/main\t[rejected] (fetch first)\n\
             Done",
            "neuradrive",
            None,
            &[],
        )
        .expect("classified");
        match err {
            SyncError::Diverged { reason, .. } => {
                assert!(
                    reason.contains("fetch first"),
                    "the line that diagnoses it has to reach the reader: {reason}"
                );
                assert!(
                    reason.contains("failed to push some refs"),
                    "and so does git's own account of what was refused: {reason}"
                );
            }
            other => panic!("expected Diverged, got {other:?}"),
        }
    }

    #[test]
    fn a_403_is_auth_even_though_it_arrives_wrapped_in_unable_to_access() {
        // Both the auth and the network needle match; auth must win, because a
        // retry storm against a git host is how an account gets locked.
        let err = classify_message(
            "fatal: unable to access 'https://git.example.com/x.git/': The requested URL returned error: 403",
            "p",
            None,
            &[],
        )
        .expect("classified");
        assert_eq!(err.code(), "auth");
    }

    #[test]
    fn an_unrecognized_message_is_left_to_the_caller() {
        assert!(classify_message("error: something new happened", "p", None, &[]).is_none());
    }

    #[test]
    fn the_host_falls_back_to_the_argument_vector() {
        let err = classify_message(
            "fatal: Authentication failed",
            "p",
            None,
            &["push", "--", "git@git.example.com:team/x.git", "main"],
        )
        .expect("classified");
        match err {
            SyncError::Auth { host } => assert_eq!(host, "git.example.com"),
            other => panic!("expected Auth, got {other:?}"),
        }
    }

    #[test]
    fn scp_style_remotes_yield_a_host_and_local_paths_do_not() {
        assert_eq!(
            host_from_url("git@git.example.com:team/x.git").as_deref(),
            Some("git.example.com")
        );
        assert_eq!(
            host_from_url("ssh://git@git.example.com:22/team/x.git").as_deref(),
            Some("git.example.com")
        );
        assert_eq!(host_from_url("/srv/repos/x.git"), None);
        assert_eq!(host_from_url("../x.git"), None);
    }

    #[test]
    fn version_parsing_accepts_every_shape_a_vendor_ships() {
        assert_eq!(parse_version("git version 2.53.0\n"), Some((2, 53)));
        assert_eq!(
            parse_version("git version 2.39.5 (Apple Git-154)"),
            Some((2, 39))
        );
        assert_eq!(parse_version("git version 2.45.1.windows.1"), Some((2, 45)));
    }

    #[test]
    fn version_parsing_rejects_garbage_instead_of_guessing() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("not a git binary at all"), None);
        assert_eq!(parse_version("git version banana"), None);
    }

    #[test]
    fn a_version_from_something_that_is_not_git_is_not_a_git_version() {
        // The defect this guards: the token scan alone accepted any executable
        // that prints a version, so a `git` on PATH that was really a wrapper,
        // a shim, or the wrong binary entirely cleared the floor and was driven
        // for every push and merge.
        assert_eq!(parse_version("Python 3.13.0\n"), None);
        assert_eq!(parse_version("git-lfs/3.5.1 (GitHub; linux amd64)"), None);
        assert_eq!(
            parse_version("hub version 2.14.2\ngit version 2.52.0"),
            None
        );
        // The identity has to be on the line the version is read from, not
        // anywhere in the output: a wrapper is free to mention git afterwards.
        assert_eq!(parse_version("mygit 1.2.3 (git version 2.52.0)"), None);
    }

    #[test]
    fn the_floor_is_2_42_and_is_compared_as_a_pair() {
        assert!(clears_floor(2, 42), "the floor itself must pass");
        assert!(!clears_floor(2, 41), "2.41 silently ignores --cone");
        assert!(
            !clears_floor(1, 99),
            "a field-by-field check would accept this"
        );
        assert!(
            clears_floor(3, 0),
            "a field-by-field check would reject this"
        );
        assert!(clears_floor(2, 53));
    }

    #[test]
    fn stderr_is_capped_on_a_character_boundary() {
        let long = "é".repeat(4_000);
        let cut = truncate(&long, STDERR_CAP);
        assert!(cut.len() <= STDERR_CAP + 4, "cap exceeded: {}", cut.len());
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn probing_a_missing_binary_reports_git_missing() {
        let err = probe(Path::new("/nonexistent/keeper-sync/git")).expect_err("must fail");
        assert_eq!(err.code(), "gitMissing");
        assert!(err.needs_user_action());
    }

    /// The phone's branch, asserted on a machine that has `git` (AD-198).
    ///
    /// Every verb is driven against a directory that is NOT a repository and
    /// against a program that does not exist: were a spawn to happen it would
    /// fail with git's own `not a git repository` or the shim's `does not
    /// exist or is not executable`, and the assertion on the sentence would
    /// fail with it. So the sentences below prove the refusal came first.
    #[test]
    fn a_phone_refuses_every_verb_before_spawning() {
        let phone = GitCli::phone();
        assert_eq!(phone.engine(), GitEngine::Gix);
        let nowhere = Path::new("/nonexistent/keeper-sync/phone-repo");
        let sentence = |result: std::result::Result<(), SyncError>| -> String {
            match result.expect_err("a phone refuses") {
                SyncError::GitMissing { reason } => reason,
                other => panic!("a refusal is `GitMissing`, got {other:?}"),
            }
        };

        assert_eq!(
            sentence(phone.push(nowhere, "origin", "refs/heads/main:refs/heads/main", None)),
            phone_refusal(Verb::Push)
        );
        assert_eq!(
            sentence(phone.merge_ff_only(nowhere, "refs/remotes/origin/main")),
            phone_refusal(Verb::Checkout)
        );
        assert_eq!(
            sentence(phone.merge_theirs(nowhere, "refs/remotes/origin/main", "m")),
            phone_refusal(Verb::Checkout)
        );
        assert_eq!(
            sentence(phone.ensure_branch(nowhere, "lane")),
            phone_refusal(Verb::Checkout)
        );
        assert_eq!(
            sentence(phone.current_branch(nowhere).map(drop)),
            phone_refusal(Verb::Checkout)
        );
        assert_eq!(
            sentence(phone.is_ancestor(nowhere, "a", "b").map(drop)),
            phone_refusal(Verb::History)
        );
        assert_eq!(
            sentence(phone.merge_base(nowhere, "a", "b").map(drop)),
            phone_refusal(Verb::History)
        );
        assert_eq!(
            sentence(phone.diff_names(nowhere, "a", "b").map(drop)),
            phone_refusal(Verb::History)
        );
        assert_eq!(
            sentence(phone.worktree_add(nowhere, Path::new("/tmp/lane"), "lane")),
            phone_refusal(Verb::Worktree)
        );
        assert_eq!(
            sentence(phone.worktree_remove(nowhere, Path::new("/tmp/lane"))),
            phone_refusal(Verb::Worktree)
        );
        assert_eq!(
            sentence(phone.worktree_prune(nowhere)),
            phone_refusal(Verb::Worktree)
        );
        assert_eq!(
            sentence(phone.sparse_set(nowhere, &["notes".to_owned()])),
            phone_refusal(Verb::Sparse)
        );
        assert_eq!(
            sentence(phone.sparse_disable(nowhere)),
            phone_refusal(Verb::Sparse)
        );
        assert_eq!(sentence(phone.gc(nowhere)), phone_refusal(Verb::Gc));
        assert_eq!(
            sentence(phone.capabilities().map(drop)),
            phone_refusal(Verb::Probe)
        );
    }

    /// Each sentence names the device and either the in-process route or the
    /// reason there is none — the AD-27 shape of a refusal.
    #[test]
    fn every_phone_refusal_names_the_phone_and_a_next_step() {
        for verb in [
            Verb::Push,
            Verb::Checkout,
            Verb::History,
            Verb::Worktree,
            Verb::Sparse,
            Verb::Gc,
            Verb::Probe,
        ] {
            let sentence = phone_refusal(verb);
            assert!(
                sentence.starts_with("this is a phone:"),
                "{verb:?}: {sentence}"
            );
            assert!(
                sentence.contains("own engine")
                    || sentence.contains("Mac")
                    || sentence.contains("not"),
                "{verb:?} names no route and no reason: {sentence}"
            );
        }
        assert!(phone_refusal(Verb::Push).contains("pushes with its own engine"));
    }

    #[test]
    fn the_host_engine_is_the_binary_off_the_phone() {
        // This suite never runs on iOS; the constant is the one place the
        // target decides, and everything else reads it off the handle.
        assert_eq!(GitEngine::HOST, GitEngine::Binary);
        assert_eq!(GitEngine::Binary.name(), "git");
        assert_eq!(GitEngine::Gix.name(), "gix");
        assert_eq!(
            GitCli::new(PathBuf::from("/usr/bin/git")).engine(),
            GitEngine::Binary
        );
    }
}
