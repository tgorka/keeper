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

use keeper_sync::{Result, SyncError, SyncPlatform};

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

/// What `doctor` and every failed command print on a box with no `git`.
///
/// A constant rather than an inline literal because it is the single most
/// likely message a new operator will ever see, and it has to survive as an
/// *instruction*: AD-41 makes `git` a hard prerequisite with no in-process
/// fallback, so "not found" without "here is how to get it" is a support
/// ticket. Also lets the wording be asserted without depending on whether the
/// test machine happens to have git installed.
pub const GIT_MISSING_REASON: &str = "no `git` executable on PATH. keeper-syncd needs it for \
     push, worktree and sparse-checkout operations (AD-41). Install it with `apt install git`, \
     `dnf install git`, `pacman -S git` or `apk add git`, then re-run `keeper-syncd doctor`";

/// `SyncPlatform` over the XDG base directories.
#[derive(Debug, Clone)]
pub struct LinuxPlatform {
    config_dir: PathBuf,
    data_dir: PathBuf,
    state_dir: PathBuf,
    host_label: String,
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

/// Find `program` on a `PATH`-shaped variable.
///
/// Written out rather than shelling out to `which`: resolving a hard
/// prerequisite by spawning another process that might equally be missing is
/// circular, and `which`'s exit codes differ between the shell builtin and the
/// binary.
fn find_executable(path_var: Option<&OsStr>, program: &str) -> Option<PathBuf> {
    let path_var = path_var?;
    std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    // `metadata` follows symlinks, which is what we want: `/usr/bin/git` is a
    // symlink on plenty of distributions.
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
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
    host_label_from(
        std::fs::read_to_string("/etc/hostname").ok(),
        std::env::var("HOSTNAME").ok(),
    )
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
        find_executable(std::env::var_os("PATH").as_deref(), "git").ok_or_else(|| {
            SyncError::GitMissing {
                reason: GIT_MISSING_REASON.to_owned(),
            }
        })
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
    fn path_resolution_finds_an_executable_and_skips_a_plain_file() {
        let root = tempfile::tempdir().expect("temp dir");
        let empty = root.path().join("empty");
        let real = root.path().join("real");
        std::fs::create_dir_all(&empty).expect("empty dir");
        std::fs::create_dir_all(&real).expect("real dir");
        // A non-executable file of the right name must not win: that is exactly
        // how a stray `git` note in a directory would shadow the real binary.
        write_with_mode(&empty.join("git"), "not a program", 0o644);
        write_with_mode(&real.join("git"), "#!/bin/sh\n", 0o755);

        let joined = std::env::join_paths([&empty, &real]).expect("join paths");

        assert_eq!(
            find_executable(Some(joined.as_os_str()), "git"),
            Some(real.join("git"))
        );
    }

    #[test]
    fn an_empty_or_gitless_path_resolves_nothing() {
        let root = tempfile::tempdir().expect("temp dir");
        assert_eq!(find_executable(Some(root.path().as_os_str()), "git"), None);
        // An unset PATH must be a miss, not a panic or a bare-name execution.
        assert_eq!(find_executable(None, "git"), None);
    }

    #[test]
    fn the_missing_git_message_tells_an_operator_what_to_install() {
        // Asserted on the constant, not on `git_program()`, so the guarantee
        // holds on a build machine that happens to have git installed.
        let err = SyncError::GitMissing {
            reason: GIT_MISSING_REASON.to_owned(),
        };

        assert_eq!(err.code(), "gitMissing");
        assert!(
            err.needs_user_action(),
            "no git is a human's problem to fix"
        );
        for expected in ["apt install git", "dnf install git", "pacman -S git"] {
            assert!(
                GIT_MISSING_REASON.contains(expected),
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
