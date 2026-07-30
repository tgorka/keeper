//! Choosing *which* `git` binary to drive (Story 34.14).
//!
//! [`super::cli`] drives one binary; this decides which one it gets. They are
//! separate because the probe is identical on every host while the candidate
//! list is not: a Finder-launched macOS app inherits `launchctl`'s `PATH`, a
//! shell-launched one inherits the shell's, and `keeper-syncd` has only `PATH`.
//!
//! # Why "the first file called `git`" is the wrong answer
//!
//! Measured on a developer Mac with three gits installed:
//!
//! | Path | Version |
//! | --- | --- |
//! | `/opt/homebrew/bin/git` | 2.52.0 |
//! | `/usr/bin/git` | 2.50.1 |
//! | `/usr/local/bin/git` | **2.23.0** |
//!
//! `launchctl getenv PATH` — what a Finder- or Dock-launched app actually gets —
//! is `/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`, so the **broken one comes
//! first**. It is below the [`MIN_GIT_MAJOR`]/[`MIN_GIT_MINOR`] floor, and it
//! does not even answer `--version`: a stale `/usr/local/git/etc/gitconfig`
//! makes it exit non-zero with `bad config line 44`. Taking the first
//! `is_file()` hit therefore handed the engine a binary that cannot serve it, on
//! a machine with two that can.
//!
//! So resolution **probes**, in candidate order, and takes the first that clears
//! the floor. Probing costs a process per candidate, which is why every host
//! caches the answer rather than re-running the search per call.
//!
//! # A named binary is never silently replaced
//!
//! [`GitRequest::explicit`] holds exactly one candidate and reports
//! [`GitOrigin::Explicit`]. When it fails there is nothing to fall back to,
//! structurally — falling back would replace one silent substitution with
//! another, which is the defect this module exists to remove.

use std::path::{Path, PathBuf};

use crate::error::{Result, SyncError};
use crate::git::cli::{version_detail, GitCapabilities, MIN_GIT_MAJOR, MIN_GIT_MINOR};

/// Why one candidate was not chosen.
///
/// Four causes rather than one boolean because they need four different
/// sentences: "install git" is useless advice for a git that is installed and
/// too old, and "upgrade git" is useless for one whose `gitconfig` is broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitReject {
    /// Nothing at the path at all.
    ///
    /// Recorded only for a named path. A `PATH` directory that holds no `git`
    /// is not a rejected candidate, it is just a directory, and listing every
    /// one of them would bury the candidates that did exist.
    Absent,
    /// Present, and not something this process can execute — a directory, or a
    /// file without an executable bit. This is how a stray note called `git`
    /// shadows the real binary.
    NotExecutable,
    /// It ran, and could not answer `git --version`. Carries git's own
    /// `stderr`, folded to one line.
    Unusable { detail: String },
    /// It answered, below the floor.
    TooOld { major: u32, minor: u32 },
}

impl GitReject {
    /// The cause as a clause, for a line that has already named the path.
    pub fn describe(&self) -> String {
        match self {
            Self::Absent => "nothing is there".to_owned(),
            Self::NotExecutable => "not an executable file".to_owned(),
            Self::Unusable { detail } => detail.clone(),
            Self::TooOld { major, minor } => {
                format!("git {major}.{minor}, below the {MIN_GIT_MAJOR}.{MIN_GIT_MINOR} floor")
            }
        }
    }
}

/// One candidate and why it lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRejection {
    pub program: PathBuf,
    pub cause: GitReject,
}

impl GitRejection {
    /// `\<path\> (\<cause\>)`, the per-candidate line every message is built from.
    fn line(&self) -> String {
        format!("{} ({})", self.program.display(), self.cause.describe())
    }
}

/// The binary that won, and what it can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitChoice {
    pub program: PathBuf,
    pub capabilities: GitCapabilities,
}

/// Where the candidate list came from — which decides what a failure means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOrigin {
    /// A search over host-supplied candidates. The first that clears the floor
    /// wins; the rest are never probed.
    Search,
    /// One binary the owner named. A rejection is final.
    Explicit,
}

/// What to resolve, and what to tell a person when it cannot be resolved.
#[derive(Debug, Clone)]
pub struct GitRequest {
    candidates: Vec<PathBuf>,
    origin: GitOrigin,
    advice: String,
}

impl GitRequest {
    /// Probe `candidates` in order and take the first that clears the floor.
    ///
    /// `advice` is the host's own "here is how to get one": this crate knows the
    /// floor but not whether the box has `apt` or Homebrew, and a refusal
    /// without a next step is a support ticket (the reasoning `keeper-syncd`'s
    /// install advice was already written down for).
    pub fn search(candidates: Vec<PathBuf>, advice: impl Into<String>) -> Self {
        Self {
            candidates,
            origin: GitOrigin::Search,
            advice: advice.into(),
        }
    }

    /// Use exactly `program`. A rejection refuses; nothing else is tried.
    pub fn explicit(program: impl Into<PathBuf>, advice: impl Into<String>) -> Self {
        Self {
            candidates: vec![program.into()],
            origin: GitOrigin::Explicit,
            advice: advice.into(),
        }
    }

    /// Probe the candidates. Stops at the first one that clears the floor, so a
    /// healthy `PATH` whose first entry is git costs exactly one process.
    ///
    /// Each candidate is pinned to an absolute path before it is touched; see
    /// [`absolute`] for why that is correctness and not tidiness.
    pub fn resolve(self) -> GitResolution {
        let explicit = self.origin == GitOrigin::Explicit;
        let mut rejected = Vec::new();
        let mut chosen = None;

        for candidate in self.candidates {
            let program = match absolute(&candidate) {
                Ok(program) => program,
                Err(cause) => {
                    rejected.push(GitRejection {
                        program: candidate,
                        cause,
                    });
                    continue;
                }
            };
            match judge(&program) {
                Ok(capabilities) => {
                    chosen = Some(GitChoice {
                        program,
                        capabilities,
                    });
                    break;
                }
                // A `PATH` directory with no `git` in it is not news. A named
                // path that is not there is the whole message.
                Err(GitReject::Absent) if !explicit => {}
                Err(cause) => rejected.push(GitRejection { program, cause }),
            }
        }

        GitResolution {
            origin: self.origin,
            chosen,
            rejected,
            advice: self.advice,
        }
    }
}

/// One resolution attempt, in full: what was chosen, and what was not.
///
/// Kept as a value rather than collapsed to `Result<PathBuf>` because three
/// callers want three different things out of the same probe — the binary, a
/// capability answer, and a sentence a person can act on — and because a host
/// caches it (see the module doc on cost).
#[derive(Debug, Clone)]
pub struct GitResolution {
    origin: GitOrigin,
    chosen: Option<GitChoice>,
    rejected: Vec<GitRejection>,
    advice: String,
}

impl GitResolution {
    /// The binary the engine will drive, when one cleared the floor.
    pub fn chosen(&self) -> Option<&GitChoice> {
        self.chosen.as_ref()
    }

    /// Every candidate that was probed and rejected, in the order tried.
    pub fn rejected(&self) -> &[GitRejection] {
        &self.rejected
    }

    /// Whether the owner named this binary rather than asking for a search.
    pub fn is_explicit(&self) -> bool {
        self.origin == GitOrigin::Explicit
    }

    /// The chosen binary, or the [`refusal`](Self::refusal) as a typed error.
    ///
    /// This is what a `SyncPlatform::git_program` returns, so the engine's own
    /// refusal and the one a person reads are the same sentence.
    pub fn program(&self) -> Result<PathBuf> {
        match &self.chosen {
            Some(choice) => Ok(choice.program.clone()),
            None => Err(SyncError::GitMissing {
                reason: self.refusal(),
            }),
        }
    }

    /// `git 2.52 at /opt/homebrew/bin/git (clears the 2.42 floor)`.
    ///
    /// Worded as `keeper-syncd doctor` already words it, because it reports the
    /// identical fact and two spellings of one fact is how a support
    /// conversation goes wrong.
    pub fn summary(&self) -> Option<String> {
        self.chosen.as_ref().map(|choice| {
            format!(
                "git {}.{} at {} (clears the {}.{} floor)",
                choice.capabilities.major,
                choice.capabilities.minor,
                choice.program.display(),
                MIN_GIT_MAJOR,
                MIN_GIT_MINOR
            )
        })
    }

    /// Why nothing was chosen. Empty string when something was.
    ///
    /// Three shapes, because a person needs three different next steps:
    /// an explicitly named binary that failed (and was *not* replaced), a
    /// search that found candidates and could use none, and a search that found
    /// nothing at all.
    pub fn refusal(&self) -> String {
        if self.chosen.is_some() {
            return String::new();
        }
        let lines = self
            .rejected
            .iter()
            .map(GitRejection::line)
            .collect::<Vec<_>>()
            .join(", ");

        match self.origin {
            GitOrigin::Explicit => format!(
                "the git binary keeper was pointed at cannot run this engine: {lines}. \
                 keeper does not quietly use a different git when you name one — clear the \
                 setting to search PATH again, or {}",
                self.advice
            ),
            GitOrigin::Search if self.rejected.is_empty() => format!(
                "no `git` on PATH. keeper needs it to push, to merge and to manage worktrees, \
                 which nothing in-process can do (AD-41) — {}",
                self.advice
            ),
            GitOrigin::Search => format!(
                "no usable `git` on PATH. keeper needs git {MIN_GIT_MAJOR}.{MIN_GIT_MINOR} or \
                 newer to push, to merge and to manage worktrees (AD-41). Tried: {lines}. \
                 {}",
                capitalize(&self.advice)
            ),
        }
    }
}

/// Probe one candidate: is this a git that can serve the engine?
///
/// `program` arrives absolute (see [`absolute`]), which is what makes the file
/// probed here the same file [`super::cli`] will later execute.
fn judge(program: &Path) -> std::result::Result<GitCapabilities, GitReject> {
    if !program.exists() {
        return Err(GitReject::Absent);
    }
    if !is_executable(program) {
        return Err(GitReject::NotExecutable);
    }
    let (major, minor) =
        version_detail(program).map_err(|detail| GitReject::Unusable { detail })?;
    let capabilities = super::cli::capabilities_of(major, minor);
    if capabilities.meets_floor() {
        Ok(capabilities)
    } else {
        Err(GitReject::TooOld { major, minor })
    }
}

/// Pin a candidate to one absolute path, before anything probes or runs it.
///
/// The mismatch this closes: [`judge`] probes with no working directory, so a
/// relative path resolves against *this process's* cwd — while every real
/// operation runs `Command::current_dir(repo)`, and on unix a program path
/// containing a separator is resolved by the child, after it has chdir'd. The
/// two therefore named two different files. `[daemon] gitPath = "usr/bin/git"`
/// under systemd's default `WorkingDirectory=/` made `keeper-syncd doctor`
/// print `git 2.50 at usr/bin/git (clears the 2.42 floor)` and `Engine::open`
/// succeed, and then every git call failed with "does not exist or is not
/// executable": the diagnostic contradicted the behaviour it was reporting on.
/// Worse, had the synced folder held a matching relative path, the engine would
/// have executed a binary out of content that arrived from peers.
///
/// Absolute, not canonical. `fs::canonicalize` would also resolve the final
/// symlink, which changes two things this must not change: the path a person
/// reads back (macOS canonicalizes `/var` to `/private/var`, so `doctor` would
/// stop echoing the path the operator configured), and the name the program is
/// executed under — a multi-call binary or a wrapper reached through a symlink
/// branches on that name, and `is_executable` already follows symlinks on
/// purpose. Absoluteness is the whole of what the probe/execute mismatch needs.
fn absolute(program: &Path) -> std::result::Result<PathBuf, GitReject> {
    if program.is_absolute() {
        return Ok(program.to_owned());
    }
    // A process whose cwd has been removed cannot say what a relative path
    // means. Reported as `Unusable` rather than guessed at: guessing is the
    // failure mode above.
    std::env::current_dir()
        .map(|cwd| cwd.join(program))
        .map_err(|err| GitReject::Unusable {
            detail: format!(
                "`{}` is a relative path and this process has no working directory to \
                 resolve it against: {err}",
                program.display()
            ),
        })
}

/// Whether `path` names something this process could execute.
///
/// The executable bit on unix, `is_file` elsewhere. `metadata` follows symlinks
/// deliberately: `/usr/bin/git` is one on most distributions, and on macOS it is
/// a shim that execs the selected developer directory's copy.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        match std::fs::metadata(path) {
            Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Upper-case the first character, so host advice reads as its own sentence
/// when it follows a full stop and as a clause when it follows a dash.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake `git` in its own directory, so a candidate list is a list of
    /// directories exactly as `PATH` is. Follows `keeper-syncd`'s
    /// `find_executable` fixtures: a script, a mode, a temp dir.
    fn fake_git(root: &Path, dir: &str, body: &str, mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let bin = root.join(dir);
        std::fs::create_dir_all(&bin).expect("fixture dir");
        let program = bin.join("git");
        std::fs::write(&program, body).expect("fixture script");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(mode))
            .expect("fixture mode");
        program
    }

    /// A git that answers `--version` with `version`, like a real one.
    fn good_git(root: &Path, dir: &str, version: &str) -> PathBuf {
        fake_git(
            root,
            dir,
            &format!("#!/bin/sh\necho 'git version {version}'\n"),
            0o755,
        )
    }

    /// The owner's actual `/usr/local/bin/git` 2.23: a stale system
    /// `gitconfig` makes it exit non-zero on `--version`, saying why on stderr.
    const BROKEN_CONFIG_STDERR: &str = "#!/bin/sh\n\
        echo \"error: could not expand include path '~/.gitcinclude'\" >&2\n\
        echo 'fatal: bad config line 44 in file /usr/local/git/etc/gitconfig' >&2\n\
        exit 128\n";

    const ADVICE: &str = "install git 2.42 or newer";

    #[test]
    fn a_broken_git_ahead_of_a_good_one_does_not_win() {
        // The owner's PATH, in order: the 2.23 that cannot even report its
        // version comes first, and the Homebrew 2.52 comes last.
        let root = tempfile::tempdir().expect("temp dir");
        let broken = fake_git(root.path(), "usr-local-bin", BROKEN_CONFIG_STDERR, 0o755);
        let good = good_git(root.path(), "opt-homebrew-bin", "2.52.0");

        let resolution = GitRequest::search(vec![broken.clone(), good.clone()], ADVICE).resolve();

        assert_eq!(
            resolution.chosen().map(|c| c.program.clone()),
            Some(good),
            "the first git that WORKS must win, not the first file called git"
        );
        // The rejection is kept: a person who wonders why keeper is not using
        // the git they think it is has to be able to find out.
        assert_eq!(resolution.rejected().len(), 1);
        assert_eq!(resolution.rejected()[0].program, broken);
    }

    #[test]
    fn a_too_old_git_ahead_of_a_good_one_does_not_win() {
        let root = tempfile::tempdir().expect("temp dir");
        let old = good_git(root.path(), "old", "2.23.0");
        let new = good_git(root.path(), "new", "2.42.0");

        let resolution = GitRequest::search(vec![old.clone(), new.clone()], ADVICE).resolve();

        assert_eq!(resolution.chosen().map(|c| c.program.clone()), Some(new));
        assert_eq!(
            resolution.rejected()[0].cause,
            GitReject::TooOld {
                major: 2,
                minor: 23
            }
        );
    }

    #[test]
    fn the_three_ways_a_git_can_fail_are_reported_distinctly() {
        // Absent, unusable and too-old need three different next steps, so they
        // must not collapse into one "no git" answer.
        let root = tempfile::tempdir().expect("temp dir");
        let absent = root.path().join("nowhere").join("git");
        let broken = fake_git(root.path(), "broken", BROKEN_CONFIG_STDERR, 0o755);
        let old = good_git(root.path(), "old", "2.23.0");
        let note = fake_git(root.path(), "note", "a reminder to install git\n", 0o644);

        assert_eq!(judge(&absent), Err(GitReject::Absent));
        assert_eq!(judge(&note), Err(GitReject::NotExecutable));
        assert_eq!(
            judge(&old),
            Err(GitReject::TooOld {
                major: 2,
                minor: 23
            })
        );

        let unusable = judge(&broken).expect_err("a git that cannot answer --version");
        match unusable {
            GitReject::Unusable { detail } => {
                // git named its own cause; dropping it left "exited non-zero".
                assert!(
                    detail.contains("bad config line 44"),
                    "git's own stderr must survive: {detail}"
                );
                assert!(
                    !detail.contains('\n'),
                    "the detail lands in a one-line row: {detail}"
                );
            }
            other => panic!("expected Unusable, got {other:?}"),
        }
    }

    #[test]
    fn a_path_directory_without_git_is_not_reported_as_a_rejection() {
        // Every `PATH` entry is a candidate, and most of them have no git.
        // Listing them would bury the ones that mattered.
        let root = tempfile::tempdir().expect("temp dir");
        let good = good_git(root.path(), "bin", "2.50.1");

        let resolution =
            GitRequest::search(vec![root.path().join("empty").join("git"), good], ADVICE).resolve();

        assert!(resolution.rejected().is_empty());
        assert!(resolution.chosen().is_some());
    }

    #[test]
    fn an_explicit_path_below_the_floor_refuses_and_does_not_fall_back() {
        // The hard rule of Story 34.14: the defect being fixed is a silent
        // substitution, so a named binary that cannot serve must never be
        // replaced by one that can.
        let root = tempfile::tempdir().expect("temp dir");
        let old = good_git(root.path(), "old", "2.23.0");
        good_git(root.path(), "new", "2.52.0");

        let resolution = GitRequest::explicit(old.clone(), ADVICE).resolve();

        assert!(resolution.chosen().is_none(), "no fallback, ever");
        assert!(resolution.is_explicit());
        let refusal = resolution.refusal();
        assert!(refusal.contains(&old.display().to_string()), "{refusal}");
        assert!(refusal.contains("2.23"), "{refusal}");
        assert!(
            refusal.contains("does not quietly use a different git"),
            "the refusal must say it is not substituting: {refusal}"
        );
        assert!(refusal.contains(ADVICE), "{refusal}");
        assert_eq!(
            resolution.program().expect_err("must refuse").code(),
            "gitMissing"
        );
    }

    #[test]
    fn an_explicit_path_that_is_not_there_is_reported_rather_than_skipped() {
        let root = tempfile::tempdir().expect("temp dir");
        let missing = root.path().join("nowhere").join("git");

        let resolution = GitRequest::explicit(missing.clone(), ADVICE).resolve();

        assert_eq!(resolution.rejected().len(), 1);
        assert_eq!(resolution.rejected()[0].cause, GitReject::Absent);
        assert!(resolution
            .refusal()
            .contains(&missing.display().to_string()));
    }

    #[test]
    fn an_empty_search_says_there_is_no_git_and_how_to_get_one() {
        let resolution = GitRequest::search(Vec::new(), ADVICE).resolve();

        let refusal = resolution.refusal();
        assert!(refusal.contains("no `git` on PATH"), "{refusal}");
        assert!(refusal.contains(ADVICE), "{refusal}");
        // Naming the operations is what stops "why does keeper need git?" from
        // becoming "let me turn that off".
        assert!(refusal.contains("push"), "{refusal}");
        assert!(refusal.contains("merge"), "{refusal}");
        assert_eq!(resolution.summary(), None);
    }

    #[test]
    fn a_search_that_found_only_unusable_gits_lists_each_one() {
        let root = tempfile::tempdir().expect("temp dir");
        let old = good_git(root.path(), "old", "2.23.0");
        let broken = fake_git(root.path(), "broken", BROKEN_CONFIG_STDERR, 0o755);

        let resolution = GitRequest::search(vec![old.clone(), broken.clone()], ADVICE).resolve();

        let refusal = resolution.refusal();
        assert!(refusal.contains("no usable `git` on PATH"), "{refusal}");
        for program in [&old, &broken] {
            assert!(
                refusal.contains(&program.display().to_string()),
                "every candidate must be named: {refusal}"
            );
        }
        assert!(refusal.contains("2.42"), "the floor: {refusal}");
    }

    #[test]
    fn a_chosen_git_is_summarized_the_way_doctor_reports_it() {
        let root = tempfile::tempdir().expect("temp dir");
        let good = good_git(root.path(), "bin", "2.52.0");

        let resolution = GitRequest::search(vec![good.clone()], ADVICE).resolve();

        let summary = resolution.summary().expect("a chosen git has a summary");
        assert!(summary.contains("git 2.52"), "{summary}");
        assert!(summary.contains(&good.display().to_string()), "{summary}");
        assert!(summary.contains("clears the 2.42 floor"), "{summary}");
        assert_eq!(resolution.refusal(), "", "a success refuses nothing");
        assert_eq!(resolution.program().expect("chosen"), good);
    }

    #[test]
    fn a_vendor_suffixed_version_is_accepted() {
        // Apple and Windows both append their own text; only major.minor is
        // read. Both shapes end to end, because the `git version` identity the
        // probe now insists on is what these have to keep clearing.
        let root = tempfile::tempdir().expect("temp dir");
        let apple = good_git(root.path(), "apple", "2.50.1 (Apple Git-155)");
        let windows = good_git(root.path(), "windows", "2.45.1.windows.1");

        for (program, expected) in [(apple, (2, 50)), (windows, (2, 45))] {
            let resolution = GitRequest::search(vec![program], ADVICE).resolve();

            assert_eq!(
                resolution
                    .chosen()
                    .map(|c| (c.capabilities.major, c.capabilities.minor)),
                Some(expected)
            );
        }
    }

    #[test]
    fn a_binary_that_prints_a_version_but_is_not_git_does_not_win() {
        // The scenario Story 34.14 exists for: something called `git` earlier on
        // PATH than the real one. `python3 --version` answers `Python 3.13.0`,
        // which the bare token scan read as 3.13 — above the floor — so
        // `summary()` announced `git 3.13 at …` for a binary that cannot serve
        // one of the pushes, merges or worktree calls the engine would then make
        // with it, and the first symptom arrived deep inside a sync.
        let root = tempfile::tempdir().expect("temp dir");
        let impostor = fake_git(
            root.path(),
            "shim",
            "#!/bin/sh\necho 'Python 3.13.0'\n",
            0o755,
        );
        let real = good_git(root.path(), "real", "2.52.0");

        let resolution = GitRequest::search(vec![impostor.clone(), real.clone()], ADVICE).resolve();

        assert_eq!(
            resolution.chosen().map(|c| c.program.clone()),
            Some(real),
            "a version is not an identity: only a git may win"
        );
        // The same rejection shape as a git that cannot answer `--version` at
        // all, so no caller needs a fifth branch to report it.
        assert_eq!(resolution.rejected().len(), 1);
        assert_eq!(resolution.rejected()[0].program, impostor);
        match &resolution.rejected()[0].cause {
            GitReject::Unusable { detail } => assert!(
                detail.contains("Python 3.13.0") && detail.contains("not git"),
                "the rejection must quote what the impostor answered: {detail}"
            ),
            other => panic!("expected Unusable, got {other:?}"),
        }
    }

    #[test]
    fn a_relative_candidate_is_resolved_once_against_the_working_directory() {
        // `[daemon] gitPath = "usr/bin/git"` under systemd's default
        // `WorkingDirectory=/` was probed against this process's cwd and then
        // executed against the repository (unix resolves a program path
        // containing a separator after the child chdirs), so `doctor` reported a
        // git that every later call then failed to find — and a matching
        // relative path inside the synced folder would have been executed out of
        // content that arrived from peers.
        //
        // The fixture lives *under* the cwd so the relative name needs no `..`,
        // and nothing here calls `set_current_dir`: that is process-global state
        // every other test in this binary shares.
        let cwd = std::env::current_dir().expect("cwd");
        let root = tempfile::tempdir_in(&cwd).expect("temp dir under the cwd");
        let absolute = good_git(root.path(), "bin", "2.50.1");
        let relative = absolute
            .strip_prefix(&cwd)
            .expect("the fixture is under the cwd")
            .to_owned();
        assert!(relative.is_relative(), "the fixture name must be relative");

        let resolution = GitRequest::explicit(relative, ADVICE).resolve();

        let summary = resolution
            .summary()
            .expect("a relative path still names a usable git");
        assert!(
            summary.contains(&absolute.display().to_string()),
            "the summary must name the file that will actually be executed: {summary}"
        );
        assert_eq!(
            resolution.program().expect("chosen"),
            absolute,
            "the engine runs git with `current_dir(repo)`, so a program the \
             engine receives relative would be resolved against the repository"
        );
    }

    #[test]
    fn probing_stops_at_the_first_git_that_clears_the_floor() {
        // Cost, not correctness: a candidate after the winner is never spawned,
        // which is what keeps a healthy PATH at one process per resolution.
        // Proven by a script that would fail the whole test if it ever ran.
        let root = tempfile::tempdir().expect("temp dir");
        let good = good_git(root.path(), "good", "2.52.0");
        let witness = root.path().join("ran");
        let after = fake_git(
            root.path(),
            "after",
            &format!(
                "#!/bin/sh\ntouch {}\necho 'git version 2.52.0'\n",
                witness.display()
            ),
            0o755,
        );

        let resolution = GitRequest::search(vec![good.clone(), after], ADVICE).resolve();

        assert_eq!(resolution.chosen().map(|c| c.program.clone()), Some(good));
        assert!(!witness.exists(), "a later candidate must not be spawned");
    }
}
