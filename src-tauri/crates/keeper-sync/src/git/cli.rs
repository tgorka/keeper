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

/// A typed handle onto one `git` binary.
#[derive(Debug, Clone)]
pub struct GitCli {
    program: PathBuf,
}

impl GitCli {
    /// Bind to the binary the host resolved through
    /// [`SyncPlatform::git_program`](crate::platform::SyncPlatform::git_program).
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }

    /// The binary this handle drives.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Version and floor check for this binary — what `doctor` reports.
    pub fn capabilities(&self) -> Result<GitCapabilities> {
        probe(&self.program)
    }

    /// Push `refspec` to `remote`.
    ///
    /// There is deliberately **no force variant reachable from outside this
    /// module**. AD-50 forbids force-pushing a lane: a bot's work-in-progress
    /// must never overwrite review feedback a human already pushed. The arg
    /// builder still knows how to spell `--force` so the test can prove the
    /// public path does not use it.
    pub fn push(&self, repo: &Path, remote: &str, refspec: &str) -> Result<()> {
        self.run("push", repo, &push_args(remote, refspec, false))
            .map(drop)
    }

    /// Materialize a linked worktree at `path` on a new branch (AD-50).
    pub fn worktree_add(&self, repo: &Path, path: &Path, branch: &str) -> Result<()> {
        self.run("worktree add", repo, &worktree_add_args(path, branch)?)
            .map(drop)
    }

    /// Remove a linked worktree.
    pub fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<()> {
        self.run("worktree remove", repo, &worktree_remove_args(path)?)
            .map(drop)
    }

    /// Drop administrative records for worktrees whose directory is gone.
    pub fn worktree_prune(&self, repo: &Path) -> Result<()> {
        self.run("worktree prune", repo, &worktree_prune_args())
            .map(drop)
    }

    /// Restrict the checkout to `subpaths` in cone mode (AD-47).
    pub fn sparse_set(&self, repo: &Path, subpaths: &[String]) -> Result<()> {
        self.run("sparse-checkout set", repo, &sparse_set_args(subpaths)?)
            .map(drop)
    }

    /// Return to a full checkout.
    pub fn sparse_disable(&self, repo: &Path) -> Result<()> {
        self.run("sparse-checkout disable", repo, &sparse_disable_args())
            .map(drop)
    }

    /// Repack loose objects. Sync churn produces a lot of them and gitoxide has
    /// no maintenance path at all, so this is the only thing keeping a
    /// long-lived profile's object store bounded.
    pub fn gc(&self, repo: &Path) -> Result<()> {
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
        self.run(
            "merge -X theirs",
            repo,
            &merge_theirs_args(reference, message)?,
        )
        .map(drop)
    }

    /// The merge base of two commits, used to work out which side changed what.
    pub fn merge_base(&self, repo: &Path, a: &str, b: &str) -> Result<String> {
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
        let exists = self
            .run("rev-parse --verify", repo, &rev_parse_verify_args(branch)?)
            .is_ok();
        let args = switch_args(branch, !exists)?;
        self.run("switch", repo, &args).map(drop)
    }

    /// The branch HEAD currently points at, or `None` when detached.
    pub fn current_branch(&self, repo: &Path) -> Result<Option<String>> {
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
        // Exit 1 is this command's ANSWER, not a failure, so it must not go
        // through the warn-logging path — the supervisor asks on every tick and
        // would otherwise fill the log with warnings about nothing.
        let args = is_ancestor_args(ancestor, descendant)?;
        let output = capture(&self.program, Some(repo), &args)?;
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
        let output = capture(&self.program, Some(repo), args)?;
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
    let args = [String::from("--version")];
    let output = capture(program, None, &args)?;
    let text = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(SyncError::GitMissing {
            reason: format!("{} --version exited non-zero", program.display()),
        });
    }
    parse_version(&text).ok_or_else(|| SyncError::GitMissing {
        reason: format!(
            "{} did not report a recognizable version",
            program.display()
        ),
    })
}

/// Version plus the 2.42 floor check.
pub fn probe(program: &Path) -> Result<GitCapabilities> {
    let (major, minor) = version(program)?;
    let clears = clears_floor(major, minor);
    Ok(GitCapabilities {
        major,
        minor,
        sparse_cone: clears,
        ls_files_format: clears,
    })
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
fn capture(program: &Path, cwd: Option<&Path>, args: &[String]) -> Result<Output> {
    let mut command = Command::new(program);
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

/// `git merge -X theirs <ref>` argument vector.
///
/// `--no-edit` and an explicit `-m` keep git from opening an editor, which
/// would hang a headless daemon forever.
fn merge_theirs_args(reference: &str, message: &str) -> Result<Vec<String>> {
    Ok(vec![
        "merge".to_owned(),
        "--no-edit".to_owned(),
        "--quiet".to_owned(),
        "-s".to_owned(),
        "ort".to_owned(),
        "-X".to_owned(),
        "theirs".to_owned(),
        "-m".to_owned(),
        message.replace(['\r', '\n'], " "),
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
/// Vendors append their own suffixes (`2.39.5 (Apple Git-154)`,
/// `2.53.0.windows.1`), so only the leading `<major>.<minor>` is read and the
/// rest is ignored.
pub(crate) fn parse_version(output: &str) -> Option<(u32, u32)> {
    let token = output
        .lines()
        .next()?
        .split_whitespace()
        .find(|token| token.starts_with(|c: char| c.is_ascii_digit()))?;
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

    const AUTH: [&str; 8] = [
        "authentication failed",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "invalid username or password",
        "permission denied (publickey",
        "403 forbidden",
        "the requested url returned error: 403",
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
            reason: first_line(text),
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

/// Name a repository directory for an error message.
fn repo_label(repo: &Path) -> String {
    repo.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.display().to_string())
}

#[cfg(test)]
mod tests {
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
}
