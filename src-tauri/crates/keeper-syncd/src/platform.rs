//! The daemon's [`SyncPlatform`] (Story 30.1, AD-52).
//!
//! The app implements this port by delegating to its `DesktopPlatform`
//! (Keychain, `fs4`, tauri's notifier). A headless box has none of those, so
//! the daemon implements the same port against the things a server does have:
//! the XDG base directories, the environment, `0600` files, `PATH`, and
//! `tracing`.
//!
//! Two rules here are load-bearing rather than incidental:
//!
//! * **A secret never enters the config.** `config.toml` is the file an
//!   operator copies into a dotfiles repo; the moment a token can live there,
//!   AD-53's "no credential in a config, a log line or a commit" is false. The
//!   only two sources are an environment variable and a per-key file that must
//!   be `0600` — checked, not assumed.
//! * **Unix-only, on purpose.** AD-52 calls `keeper-syncd` a Linux-first
//!   binary. Mode bits are the entire enforcement mechanism for the rule above,
//!   so this compiles against `std::os::unix` unconditionally rather than
//!   silently skipping the check on a platform that cannot express it.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use keeper_sync::xdg::{path_candidates, SecretStore, XdgDirs};
use keeper_sync::{GitRequest, OpenFileState, Result, SyncPlatform};

/// The per-application segment appended to each XDG base directory.
pub const APP_DIR: &str = "keeper-sync";
/// The daemon's TOML configuration, under the config dir.
pub const CONFIG_FILE: &str = "config.toml";
/// The daemon's own log file, under the state dir.
pub const LOG_FILE: &str = "keeper-syncd.log";
/// Per-key secret files, under the config dir.
pub const SECRETS_DIR: &str = "secrets";
/// Prefix for the environment-variable secret source.
pub const SECRET_ENV_PREFIX: &str = "KEEPER_SYNC_SECRET_";

/// What `doctor` and every failed command tell an operator to install.
///
/// A constant rather than an inline literal because it is the single most
/// likely message a new operator will ever see, and it has to survive as an
/// *instruction*: AD-41 makes `git` a hard prerequisite with no in-process
/// fallback, so "not found" without "here is how to get it" is a support
/// ticket. Also lets the wording be asserted without depending on whether the
/// test machine happens to have git installed.
///
/// Phrased as the *tail* of a sentence: `keeper_sync::git::resolve` owns the
/// lead clause (which candidates were tried, and what each of them said) and
/// appends this, so the daemon and the app cannot describe one fault two ways.
pub const GIT_ADVICE: &str = "install git 2.42 or newer with `apt install git`, \
     `dnf install git`, `pacman -S git` or `apk add git`, or set `[daemon] gitPath` in \
     config.toml, then re-run `keeper-syncd doctor`";

/// `SyncPlatform` over the XDG base directories.
#[derive(Debug, Clone)]
pub struct LinuxPlatform {
    dirs: XdgDirs,
    secrets: SecretStore,
    host_label: String,
    /// `[daemon] gitPath`, when the operator named a binary. `None` searches
    /// `PATH`. Held on the platform rather than read from the config at each
    /// call because `SyncPlatform::git_program` has no access to the config.
    git_path: Option<PathBuf>,
}

impl LinuxPlatform {
    /// Resolve the XDG directories from the environment and create them.
    ///
    /// Creating them here rather than lazily means a first run fails at
    /// startup, naming the directory it could not create, instead of half way
    /// through a sync.
    pub fn new() -> Result<Self> {
        Ok(Self::over(XdgDirs::resolve(APP_DIR)?))
    }

    /// Build against explicit directories, bypassing the environment.
    ///
    /// The tests' platform over a temporary tree.
    #[cfg(test)]
    pub fn with_dirs(
        config_dir: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
        state_dir: impl Into<PathBuf>,
    ) -> Self {
        Self::over(XdgDirs::with_dirs(config_dir, data_dir, state_dir))
    }

    /// Secrets under `<config>/secrets/`, environment first, no credentials
    /// directory and no strict directory rule: syncd's store since Story 30.1.
    fn over(dirs: XdgDirs) -> Self {
        let secrets = SecretStore::new(SECRET_ENV_PREFIX, dirs.config.join(SECRETS_DIR));
        Self {
            dirs,
            secrets,
            host_label: read_host_label(),
            git_path: None,
        }
    }

    /// Bind the configured `[daemon] gitPath` (Story 34.14).
    ///
    /// Applied after the config is parsed, which is *after* the platform exists
    /// — the startup order is deliberate (the configured log level is an input
    /// to the logger, so the config is read before logging is initialised).
    ///
    /// An empty or all-whitespace path means **automatic**, not "an explicit
    /// path that happens to be empty". TOML's `gitPath = ""` deserializes to
    /// `Some(PathBuf::from(""))`, and taking the explicit branch on it produced
    /// a refusal that named an empty path — and, because naming a binary
    /// deliberately has no fallback, refused every git operation on the box. The
    /// app normalizes exactly this away in `keeper_core::registry`
    /// (`get_sync_git_path` filters on `value.trim().is_empty()`, with a test
    /// asserting that "cleared" and "never set" are one state), and two hosts
    /// reading the same setting must not disagree about what it says. Filtered
    /// here rather than in `config`, so both `git_resolution` (for `doctor`) and
    /// `SyncPlatform::git_program` (for the engine) get the same answer without
    /// either having to remember.
    pub fn with_git_path(mut self, git_path: Option<PathBuf>) -> Self {
        self.git_path = git_path.filter(|path| !path.to_string_lossy().trim().is_empty());
        self
    }

    /// Which `git` this daemon will drive, and what was rejected on the way.
    ///
    /// **Not cached, unlike the app's.** The app's resolution gates a UI surface
    /// that is re-read on every window handshake; this one is consulted once per
    /// process, by `Engine::open` (and once more by `doctor`, which is a
    /// one-shot command). Caching it would add interior mutability to a `Clone`
    /// platform to save a spawn that happens once.
    pub fn git_resolution(&self) -> keeper_sync::GitResolution {
        match &self.git_path {
            // Exactly this binary. A named git that cannot serve refuses; it is
            // never quietly replaced by one from `PATH`.
            Some(program) => GitRequest::explicit(program.clone(), GIT_ADVICE).resolve(),
            None => GitRequest::search(path_candidates("git"), GIT_ADVICE).resolve(),
        }
    }

    pub fn state_dir(&self) -> &Path {
        &self.dirs.state
    }

    /// `$XDG_CONFIG_HOME/keeper-sync/config.toml`.
    pub fn config_path(&self) -> PathBuf {
        self.dirs.config.join(CONFIG_FILE)
    }

    /// `$XDG_STATE_HOME/keeper-sync/keeper-syncd.log`.
    pub fn log_path(&self) -> PathBuf {
        self.dirs.state.join(LOG_FILE)
    }

    /// `$XDG_CONFIG_HOME/keeper-sync/secrets/`.
    pub fn secrets_dir(&self) -> PathBuf {
        self.secrets.dir().to_path_buf()
    }

    #[cfg(test)]
    fn secret_path(&self, key: &str) -> PathBuf {
        self.secrets.path_of(key)
    }
}

/// Pick a host label from the two places a Linux box publishes one.
fn host_label_from(etc_hostname: Option<String>, env_hostname: Option<String>) -> String {
    // `/etc/hostname` is the persistent, container-visible answer; `HOSTNAME`
    // is a shell convenience that systemd does not export to services.
    for value in [etc_hostname, env_hostname].into_iter().flatten() {
        let trimmed = value.lines().next().unwrap_or_default().trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    "unknown-host".to_owned()
}

fn read_host_label() -> String {
    // `/etc/hostname` is the persistent, container-visible answer on Linux and
    // `HOSTNAME` is a shell convenience, but macOS has NEITHER — a daemon there
    // stamped every commit with "unknown-host", which defeats the whole point
    // of provenance identifying the machine. `hostname(1)` is POSIX and present
    // on both, so it is the last resort before giving up.
    host_label_from(
        std::fs::read_to_string("/etc/hostname").ok(),
        std::env::var("HOSTNAME").ok().or_else(hostname_command),
    )
}

/// Ask `hostname(1)`. `None` when the binary is missing or says nothing.
fn hostname_command() -> Option<String> {
    let output = std::process::Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout);
    // macOS answers with the Bonjour name (`macbookpro.lan`); the leading
    // label is the useful part and keeps a commit trailer short.
    let short = name.trim().split('.').next().unwrap_or_default().trim();
    (!short.is_empty()).then(|| short.to_owned())
}

impl SyncPlatform for LinuxPlatform {
    fn data_dir(&self) -> Result<PathBuf> {
        Ok(self.dirs.data.clone())
    }

    /// Environment first, then the `0600` file: how a container or a systemd
    /// drop-in injects a token without ever writing it to a filesystem.
    fn secret_get(&self, key: &str) -> Result<Option<String>> {
        self.secrets.get(key)
    }

    fn secret_set(&self, key: &str, value: &str) -> Result<()> {
        self.secrets.set(key, value)
    }

    /// Removing an absent secret succeeds, per the port's contract.
    fn secret_delete(&self, key: &str) -> Result<()> {
        self.secrets.delete(key)
    }

    fn notify(&self, title: &str, body: &str) {
        // A daemon has no desktop notifier. `warn` rather than `info` because
        // the engine only calls this for something a human should see, and on a
        // server the log is the only surface there is.
        tracing::warn!(%title, %body, "sync notification");
    }

    fn now_ms(&self) -> i64 {
        // A pre-1970 clock makes `duration_since` fail. Reporting 0 then means
        // "every scheduled unit is due", which is the safe direction: work runs
        // early rather than never.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as i64)
    }

    fn free_space(&self, _path: &Path) -> Option<u64> {
        // `None` is "could not determine", which the port defines as permission
        // to proceed — the recording subsystem's fail-open precedent. A real
        // probe needs `statvfs`, and this crate deliberately carries no `fs4`
        // and no `libc`; the app supplies the real probe through its own
        // `DesktopPlatform`, and `keeper-syncd doctor` reports free space
        // separately via `df` so an operator still gets the number.
        None
    }

    /// Whether anything on this machine currently has `path` open (Story
    /// 56.11).
    ///
    /// **This is the platform that can answer it.** AD-52 makes `keeper-syncd`
    /// a Linux-first daemon, and Linux is the target where the kernel publishes
    /// every process's descriptor table in a directory `std` can read. So the
    /// port's `Unknown` default — the answer that refuses, and the answer this
    /// daemon gave until now — is overridden here rather than left in place.
    ///
    /// The answer comes from `/proc/<pid>/fd` matched by **device + inode
    /// identity**, in-process: no `lsof` spawn (AD-125 refuses one by name),
    /// no `libc`, no new dependency, nothing added for `free_space`'s note
    /// above to be inconsistent with. The shared walk lives in `keeper-sync`
    /// ([`keeper_sync::platform::probe_open_file_state`]) rather than here
    /// because the app implements the same override with the same call, and two
    /// hosts must not disagree about one folder.
    ///
    /// **The one narrowing, stated:** `/proc/<pid>/fd` is mode `0500`, so a
    /// process owned by another uid — root included — has a descriptor table
    /// this daemon cannot read. `Closed` therefore claims "no process whose
    /// descriptor table this process is permitted to read holds this inode
    /// open". Every blind spot that is *not* that — no procfs, no resolvable
    /// `/proc/self`, an unenterable `/proc/<pid>` (a `hidepid=1` mount, where
    /// this daemon therefore refuses every release), a descriptor table that
    /// stopped enumerating part-way, a target that cannot be stat'ed — answers
    /// `Unknown` and still refuses. A `hidepid=2` mount is not one of them and
    /// the walk's own doc says why.
    ///
    /// **This method compiles on macOS**, where `keeper-syncd` is also built
    /// and tested, and there the shared probe answers `Unknown`. That is why
    /// this crate's test for it is Linux-only.
    ///
    /// This is the override that ends the `OpenUnknown` refusal Story 56.4
    /// recorded: `keeper-syncd dehydrate` and Story 56.5's TTL sweep now reach
    /// the rename on this host instead of declining every candidate.
    fn open_file_state(&self, path: &Path) -> OpenFileState {
        keeper_sync::platform::probe_open_file_state(path)
    }

    fn git_program(&self) -> Result<PathBuf> {
        self.git_resolution().program()
    }

    fn host_label(&self) -> String {
        self.host_label.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_sync::xdg::candidates_in;
    use keeper_sync::SyncError;
    use std::os::unix::fs::PermissionsExt as _;

    fn write_with_mode(path: &Path, contents: &str, mode: u32) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
        std::fs::write(path, contents).expect("write");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }

    fn platform_at(root: &Path) -> LinuxPlatform {
        LinuxPlatform::with_dirs(root.join("config"), root.join("data"), root.join("state"))
    }

    #[test]
    fn a_real_hostname_is_resolvable_on_this_machine() {
        // Regression: on macOS neither /etc/hostname nor $HOSTNAME exists, so
        // the daemon stamped every commit with "unknown-host" and provenance
        // identified nothing at all. Whatever platform this runs on must yield
        // a usable name.
        let label = read_host_label();
        assert_ne!(label, "unknown-host", "the host must be identifiable here");
        assert!(!label.is_empty());
        assert!(
            !label.contains('.'),
            "the short label is used, got {label:?}"
        );
    }

    #[test]
    fn a_group_or_world_readable_secret_is_refused() {
        let root = tempfile::tempdir().expect("temp dir");
        let platform = platform_at(root.path());
        let key = "sync/01PROFILE/credential";
        write_with_mode(&platform.secret_path(key), "s3cret\n", 0o644);

        let err = platform
            .secret_get(key)
            .expect_err("a world-readable token must never be used");

        assert_eq!(err.code(), "config");
        let message = err.to_string();
        assert!(
            message.contains("0600"),
            "must name the required mode: {message}"
        );
        // The diagnostic must not become the leak it is complaining about.
        assert!(
            !message.contains("s3cret"),
            "must not echo the secret: {message}"
        );
    }

    #[test]
    fn a_group_readable_only_secret_is_also_refused() {
        let root = tempfile::tempdir().expect("temp dir");
        let platform = platform_at(root.path());
        let key = "sync/01PROFILE/credential";
        write_with_mode(&platform.secret_path(key), "s3cret", 0o640);

        assert!(platform.secret_get(key).is_err());
    }

    #[test]
    fn a_0600_secret_is_accepted_and_its_trailing_newline_stripped() {
        let root = tempfile::tempdir().expect("temp dir");
        let platform = platform_at(root.path());
        let key = "sync/01PROFILE/credential";
        write_with_mode(&platform.secret_path(key), "s3cret\n", 0o600);

        assert_eq!(
            platform.secret_get(key).expect("read"),
            Some("s3cret".to_owned())
        );
    }

    #[test]
    fn an_absent_secret_is_none_and_deleting_it_succeeds() {
        let root = tempfile::tempdir().expect("temp dir");
        let platform = platform_at(root.path());

        assert_eq!(
            platform.secret_get("sync/nope/credential").expect("get"),
            None
        );
        platform
            .secret_delete("sync/nope/credential")
            .expect("deleting an absent secret must succeed");
    }

    #[test]
    fn a_written_secret_round_trips_and_lands_at_0600() {
        let root = tempfile::tempdir().expect("temp dir");
        let platform = platform_at(root.path());
        let key = "sync/01PROFILE/credential";

        platform.secret_set(key, "s3cret").expect("set");

        assert_eq!(
            platform.secret_get(key).expect("get"),
            Some("s3cret".to_owned())
        );
        let mode = std::fs::metadata(platform.secret_path(key))
            .expect("stat")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "a secret must never be written readable"
        );
    }

    #[test]
    fn secret_set_tightens_a_pre_existing_loose_file() {
        let root = tempfile::tempdir().expect("temp dir");
        let platform = platform_at(root.path());
        let key = "sync/01PROFILE/credential";
        write_with_mode(&platform.secret_path(key), "old", 0o644);

        platform.secret_set(key, "rotated").expect("set");

        let mode = std::fs::metadata(platform.secret_path(key))
            .expect("stat")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(
            platform.secret_get(key).expect("get"),
            Some("rotated".to_owned())
        );
    }

    #[test]
    fn resolution_skips_a_broken_git_for_a_good_one_further_down_path() {
        // The daemon's half of Story 34.14, in the shape the old
        // `find_executable` tests had: fixture directories holding fake gits.
        let root = tempfile::tempdir().expect("temp dir");
        let shadow = root.path().join("shadow");
        let real = root.path().join("real");
        std::fs::create_dir_all(&shadow).expect("shadow dir");
        std::fs::create_dir_all(&real).expect("real dir");
        // A `git` that is executable and answers with a version below the floor —
        // `find_executable` returned exactly this one, and the engine refused it.
        write_with_mode(
            &shadow.join("git"),
            "#!/bin/sh\necho 'git version 2.23.0'\n",
            0o755,
        );
        write_with_mode(
            &real.join("git"),
            "#!/bin/sh\necho 'git version 2.52.0'\n",
            0o755,
        );
        let joined = std::env::join_paths([&shadow, &real]).expect("join paths");

        let resolution =
            GitRequest::search(candidates_in(Some(joined.as_os_str()), "git"), GIT_ADVICE)
                .resolve();

        assert_eq!(
            resolution.program().expect("a usable git"),
            real.join("git")
        );
        assert_eq!(resolution.rejected().len(), 1);
        assert_eq!(resolution.rejected()[0].program, shadow.join("git"));
    }

    #[test]
    fn a_configured_git_path_is_obeyed_exactly_and_never_replaced() {
        let root = tempfile::tempdir().expect("temp dir");
        let named = root.path().join("git-2.23");
        write_with_mode(&named, "#!/bin/sh\necho 'git version 2.23.0'\n", 0o755);
        let platform =
            LinuxPlatform::with_dirs("/c", "/d", "/s").with_git_path(Some(named.clone()));

        let resolution = platform.git_resolution();

        assert!(resolution.is_explicit());
        assert!(
            resolution.chosen().is_none(),
            "a named git below the floor must refuse, not fall back to PATH"
        );
        let err = platform.git_program().expect_err("must refuse");
        assert_eq!(err.code(), "gitMissing");
        let message = err.to_string();
        assert!(message.contains(&named.display().to_string()), "{message}");
        assert!(message.contains("2.23"), "{message}");
        assert!(message.contains("gitPath"), "{message}");
    }

    #[test]
    fn an_empty_or_blank_git_path_means_automatic_just_as_it_does_in_the_app() {
        // TOML's `gitPath = ""` deserializes to `Some("")`, and treating that as
        // an explicit choice refused every git operation on the box while naming
        // an empty path — an explicit request has no fallback by design. The app
        // reads the same setting back as `None` (`keeper_core::registry`
        // asserts "cleared" and "never set" are one state), so an operator who
        // empties the field must get automatic resolution on both hosts.
        for blank in ["", "   ", "\t\n"] {
            let platform = LinuxPlatform::with_dirs("/c", "/d", "/s")
                .with_git_path(Some(PathBuf::from(blank)));

            let resolution = platform.git_resolution();

            assert!(
                !resolution.is_explicit(),
                "`gitPath = {blank:?}` must search PATH, not name a binary"
            );
            // Whatever this build box has on PATH, the refusal for a search is
            // never the explicit one that says a named git was not replaced.
            let refusal = resolution.refusal();
            assert!(
                !refusal.contains("does not quietly use a different git"),
                "a blank path must not refuse as if a binary had been named: {refusal}"
            );
        }
    }

    #[test]
    fn the_git_advice_tells_an_operator_what_to_install() {
        // Asserted on the constant, not on `git_program()`, so the guarantee
        // holds on a build machine that happens to have git installed.
        let err = SyncError::GitMissing {
            reason: GIT_ADVICE.to_owned(),
        };

        assert_eq!(err.code(), "gitMissing");
        assert!(
            err.needs_user_action(),
            "no git is a human's problem to fix"
        );
        for expected in [
            "apt install git",
            "dnf install git",
            "pacman -S git",
            "gitPath",
        ] {
            assert!(
                GIT_ADVICE.contains(expected),
                "the message must name {expected}"
            );
        }
    }

    #[test]
    fn the_host_label_prefers_etc_hostname_and_trims_it() {
        assert_eq!(
            host_label_from(
                Some("build-box\n".to_owned()),
                Some("shell-name".to_owned())
            ),
            "build-box"
        );
        // A multi-line /etc/hostname is malformed but happens; take line one.
        assert_eq!(
            host_label_from(Some("first\nsecond\n".to_owned()), None),
            "first"
        );
    }

    #[test]
    fn the_host_label_falls_back_through_env_to_a_placeholder() {
        assert_eq!(
            host_label_from(Some("   \n".to_owned()), Some("shell-name".to_owned())),
            "shell-name"
        );
        assert_eq!(host_label_from(None, None), "unknown-host");
    }

    #[test]
    fn free_space_is_none_so_callers_proceed() {
        // The port defines `None` as permission to proceed; a daemon that
        // refused to sync because it cannot statvfs would be worse than one
        // that fills the disk and says so.
        let platform = LinuxPlatform::with_dirs("/c", "/d", "/s");
        assert_eq!(platform.free_space(Path::new("/d")), None);
    }

    #[test]
    fn the_clock_is_wall_clock_milliseconds() {
        let platform = LinuxPlatform::with_dirs("/c", "/d", "/s");
        // Sometime after 2020 and before 2100 — enough to catch a seconds/
        // milliseconds or micros mix-up, which is the plausible bug here.
        let now = platform.now_ms();
        assert!(now > 1_577_836_800_000, "{now}");
        assert!(now < 4_102_444_800_000, "{now}");
    }

    /// The daemon answers the open-file question for real (Story 56.11).
    ///
    /// Fails for the state this crate shipped in until now: the port's
    /// `Unknown` default, which made `keeper-syncd dehydrate` and the TTL sweep
    /// refuse `OpenUnknown` on the very platform AD-52 built this binary for.
    /// Both halves in one test, so a probe stuck on either answer is caught —
    /// `Open` while a real descriptor is alive, `Closed` once it is dropped.
    ///
    /// `#[cfg(target_os = "linux")]`, and not decoration: the macOS gate runs
    /// `cargo test --workspace` (`scripts/check-macos.sh`), this crate builds
    /// for `aarch64-apple-darwin` in the release workflow, and there the shared
    /// probe answers `Unknown` — so an ungated version of this test would fail
    /// the macOS gate for a reason that is the documented, correct behaviour.
    /// A Linux host with no readable process table skips for the reason
    /// `keeper-sync`'s own probe tests state.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_daemon_answers_the_open_file_question() {
        if std::fs::read_dir("/proc/self/fd").is_err() {
            return;
        }
        let root = tempfile::tempdir().expect("temp dir");
        let platform = platform_at(root.path());

        let target = root.path().join("clip.mp4");
        std::fs::write(&target, b"the content a reader is part-way through").expect("write");

        let held = std::fs::File::open(&target).expect("hold a real descriptor");
        assert_eq!(
            platform.open_file_state(&target),
            OpenFileState::Open,
            "the override is wired up, so the release refuses while somebody reads"
        );

        drop(held);
        assert_eq!(
            platform.open_file_state(&target),
            OpenFileState::Closed,
            "and the daemon may release once nothing holds it"
        );
    }
}
