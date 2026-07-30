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

use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use keeper_sync::{GitRequest, Result, SyncError, SyncPlatform};

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
    config_dir: PathBuf,
    data_dir: PathBuf,
    state_dir: PathBuf,
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
        let home = home_dir()?;
        let platform = Self::with_dirs(
            xdg_dir(std::env::var_os("XDG_CONFIG_HOME"), &home, ".config"),
            xdg_dir(std::env::var_os("XDG_DATA_HOME"), &home, ".local/share"),
            xdg_dir(std::env::var_os("XDG_STATE_HOME"), &home, ".local/state"),
        );
        for dir in [
            &platform.config_dir,
            &platform.data_dir,
            &platform.state_dir,
        ] {
            std::fs::create_dir_all(dir)
                .map_err(|err| SyncError::io("create daemon directory", dir, err))?;
        }
        Ok(platform)
    }

    /// Build against explicit directories, bypassing the environment.
    ///
    /// Used by tests, and by an operator who points every path at one tree.
    pub fn with_dirs(
        config_dir: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
        state_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            config_dir: config_dir.into(),
            data_dir: data_dir.into(),
            state_dir: state_dir.into(),
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
        &self.state_dir
    }

    /// `$XDG_CONFIG_HOME/keeper-sync/config.toml`.
    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE)
    }

    /// `$XDG_STATE_HOME/keeper-sync/keeper-syncd.log`.
    pub fn log_path(&self) -> PathBuf {
        self.state_dir.join(LOG_FILE)
    }

    /// `$XDG_CONFIG_HOME/keeper-sync/secrets/`.
    pub fn secrets_dir(&self) -> PathBuf {
        self.config_dir.join(SECRETS_DIR)
    }

    fn secret_path(&self, key: &str) -> PathBuf {
        self.secrets_dir().join(secret_file_name(key))
    }
}

/// Apply the XDG resolution rule to one base directory.
///
/// Per the XDG Base Directory specification an unset **or empty** variable
/// falls back to the default, and a relative path "should be considered
/// invalid and ignored" — a relative `XDG_DATA_HOME` would otherwise resolve
/// `sync.db` against whatever directory systemd happened to start us in, so
/// the daemon would silently use a different database per working directory.
fn xdg_dir(explicit: Option<OsString>, home: &Path, fallback: &str) -> PathBuf {
    let base = explicit
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(fallback));
    base.join(APP_DIR)
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            SyncError::Config(
                "HOME is not set, so the XDG base directories cannot be resolved; \
                 set HOME, or set XDG_CONFIG_HOME, XDG_DATA_HOME and XDG_STATE_HOME \
                 to absolute paths"
                    .to_owned(),
            )
        })
}

/// Environment-variable name carrying the secret for `key`.
///
/// Engine keys look like `sync/<ULID>/credential`, so folding every
/// non-alphanumeric character to `_` is injective over the keys that actually
/// occur; it is not injective in general, which is fine because the engine —
/// not a user — chooses these.
fn env_var_name(key: &str) -> String {
    let mut name = String::with_capacity(SECRET_ENV_PREFIX.len() + key.len());
    name.push_str(SECRET_ENV_PREFIX);
    for ch in key.chars() {
        name.push(if ch.is_ascii_alphanumeric() {
            ch.to_ascii_uppercase()
        } else {
            '_'
        });
    }
    name
}

/// File name carrying the secret for `key`.
///
/// The same fold, minus the case change. It also makes traversal impossible:
/// `..` becomes `__`, so a key can never escape the secrets directory.
fn secret_file_name(key: &str) -> String {
    key.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

/// Strip a trailing newline from a secret.
///
/// `echo token > secret` and a heredoc both append one, and a credential never
/// legitimately ends in CR or LF — so this removes a near-universal footgun
/// without touching any byte a token could actually contain.
fn trim_secret(raw: &str) -> String {
    raw.trim_end_matches(['\r', '\n']).to_owned()
}

/// Reject a secret file any account but the owner can read.
///
/// `mode & 0o077` is the group+other bits: this is the same test `ssh` applies
/// to a private key, and for the same reason — a token readable by `nogroup`
/// on a shared box is already leaked.
fn check_secret_permissions(path: &Path, mode: u32) -> Result<()> {
    if mode & 0o077 != 0 {
        return Err(SyncError::Config(format!(
            "secret file {} is readable by group or others (mode {:04o}); \
             it must be 0600 — run: chmod 0600 {}",
            path.display(),
            mode & 0o7777,
            path.display()
        )));
    }
    Ok(())
}

/// Every `PATH` entry's `program`, in the order `PATH` lists them.
///
/// Candidates, not an answer: which of them is a *usable* git is decided by
/// probing (`keeper_sync::git::resolve`). This used to be `find_executable`,
/// which returned the first executable hit — and an executable file named `git`
/// is not the same thing as a git this engine can drive, which is the whole of
/// Story 34.14.
///
/// Written out rather than shelling out to `which`: resolving a hard
/// prerequisite by spawning another process that might equally be missing is
/// circular, and `which`'s exit codes differ between the shell builtin and the
/// binary.
fn path_candidates(program: &str) -> Vec<PathBuf> {
    candidates_in(std::env::var_os("PATH").as_deref(), program)
}

/// [`path_candidates`] over an explicit `PATH`-shaped value, for tests.
fn candidates_in(path_var: Option<&OsStr>, program: &str) -> Vec<PathBuf> {
    let Some(path_var) = path_var else {
        return Vec::new();
    };
    std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(program))
        .collect()
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
        Ok(self.data_dir.clone())
    }

    fn secret_get(&self, key: &str) -> Result<Option<String>> {
        // Environment first: it is how a container or a systemd
        // `LoadCredential=` drop-in injects a token without ever writing it to
        // a filesystem.
        if let Some(value) = std::env::var(env_var_name(key))
            .ok()
            .filter(|value| !value.is_empty())
        {
            return Ok(Some(trim_secret(&value)));
        }

        let path = self.secret_path(key);
        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(SyncError::io("stat secret file", &path, err)),
        };
        check_secret_permissions(&path, meta.permissions().mode())?;
        let raw = std::fs::read_to_string(&path)
            .map_err(|err| SyncError::io("read secret file", &path, err))?;
        Ok(Some(trim_secret(&raw)))
    }

    fn secret_set(&self, key: &str, value: &str) -> Result<()> {
        let dir = self.secrets_dir();
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|err| SyncError::io("create secrets directory", &dir, err))?;

        let path = self.secret_path(key);
        // `mode` applies only when the file is created, so an existing file
        // keeps whatever bits it had — hence the explicit chmod below. Creating
        // it restricted first means there is never a window in which the
        // credential exists on disk world-readable.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|err| SyncError::io("create secret file", &path, err))?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|err| SyncError::io("restrict secret file", &path, err))?;
        file.write_all(value.as_bytes())
            .map_err(|err| SyncError::io("write secret file", &path, err))?;

        if std::env::var_os(env_var_name(key)).is_some() {
            // Reads prefer the environment, so this write would be invisible.
            // Silence here is how an operator ends up debugging a rotated token
            // that "did not take".
            tracing::warn!(
                variable = %env_var_name(key),
                "a secret environment variable shadows the file just written; \
                 reads will keep returning the environment value"
            );
        }
        Ok(())
    }

    fn secret_delete(&self, key: &str) -> Result<()> {
        let path = self.secret_path(key);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            // Removing an absent secret succeeds, per the port's contract.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(SyncError::io("delete secret file", &path, err)),
        }
        if std::env::var_os(env_var_name(key)).is_some() {
            tracing::warn!(
                variable = %env_var_name(key),
                "the secret file was removed but the environment still supplies this secret"
            );
        }
        Ok(())
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
    fn xdg_falls_back_to_home_when_unset() {
        let home = Path::new("/home/dev");
        assert_eq!(
            xdg_dir(None, home, ".config"),
            PathBuf::from("/home/dev/.config/keeper-sync")
        );
        assert_eq!(
            xdg_dir(None, home, ".local/share"),
            PathBuf::from("/home/dev/.local/share/keeper-sync")
        );
        assert_eq!(
            xdg_dir(None, home, ".local/state"),
            PathBuf::from("/home/dev/.local/state/keeper-sync")
        );
    }

    #[test]
    fn xdg_honours_an_absolute_override() {
        assert_eq!(
            xdg_dir(
                Some(OsString::from("/srv/keeper/cfg")),
                Path::new("/home/dev"),
                ".config"
            ),
            PathBuf::from("/srv/keeper/cfg/keeper-sync")
        );
    }

    #[test]
    fn xdg_ignores_an_empty_or_relative_override() {
        let home = Path::new("/home/dev");
        // The spec says empty means "unset"...
        assert_eq!(
            xdg_dir(Some(OsString::new()), home, ".config"),
            PathBuf::from("/home/dev/.config/keeper-sync")
        );
        // ...and that a relative path is invalid. Honouring it would resolve
        // sync.db against the process CWD, giving a different database per
        // working directory.
        assert_eq!(
            xdg_dir(Some(OsString::from("relative/cfg")), home, ".config"),
            PathBuf::from("/home/dev/.config/keeper-sync")
        );
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
    fn a_secret_key_can_never_escape_the_secrets_directory() {
        let root = tempfile::tempdir().expect("temp dir");
        let platform = platform_at(root.path());

        let path = platform.secret_path("../../etc/shadow");

        assert_eq!(path.parent(), Some(platform.secrets_dir().as_path()));
        assert_eq!(
            path.file_name().and_then(OsStr::to_str),
            Some("______etc_shadow")
        );
    }

    #[test]
    fn the_secret_environment_variable_name_is_derived_from_the_key() {
        assert_eq!(
            env_var_name("sync/01PROFILE/credential"),
            "KEEPER_SYNC_SECRET_SYNC_01PROFILE_CREDENTIAL"
        );
    }

    #[test]
    fn path_candidates_are_every_path_entry_in_order() {
        // Candidates, not an answer: a non-executable file of the right name is
        // still offered to the prober, which is what turns "a stray `git` note
        // shadows the real binary" from a silent substitution into a reported
        // rejection.
        let root = tempfile::tempdir().expect("temp dir");
        let first = root.path().join("first");
        let second = root.path().join("second");
        let joined = std::env::join_paths([&first, &second]).expect("join paths");

        assert_eq!(
            candidates_in(Some(joined.as_os_str()), "git"),
            vec![first.join("git"), second.join("git")]
        );
        // An unset PATH must be an empty list, not a panic or a bare-name spawn.
        assert!(candidates_in(None, "git").is_empty());
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
}
