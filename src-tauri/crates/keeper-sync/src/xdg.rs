//! The XDG base directories and the env-or-`0600` secret store a headless
//! host runs on (AD-376, ruling R7).
//!
//! Two binaries share this: `keeper-syncd` (Story 30.1) and `keeper-agentd`
//! (Epic 90). They differ only in what they pass in — the application
//! directory, the environment prefix, whether a systemd credentials directory
//! is read and whether the secrets directory is held to the stricter rule —
//! so syncd's behaviour is exactly what it was before the move.
//!
//! Two rules here are load-bearing rather than incidental:
//!
//! * **A secret never enters a config file.** The only sources are a systemd
//!   credential (agentd only), an environment variable and a per-key file that
//!   must be `0600` — checked, not assumed.
//! * **Unix-only, on purpose.** Mode bits are the entire enforcement mechanism
//!   for the rule above, so this module compiles against `std::os::unix` and is
//!   absent elsewhere rather than silently skipping the check.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};

use crate::{Result, SyncError};

/// The three XDG base directories of one application, each already joined
/// with the application's own segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XdgDirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
}

impl XdgDirs {
    /// Resolve `$XDG_{CONFIG,DATA,STATE}_HOME/<app>` from the environment and
    /// create them.
    ///
    /// Creating them here rather than lazily means a first run fails at
    /// startup, naming the directory it could not create, instead of half way
    /// through a sync.
    pub fn resolve(app: &str) -> Result<XdgDirs> {
        let home = home_dir()?;
        let dirs = XdgDirs {
            config: xdg_dir(std::env::var_os("XDG_CONFIG_HOME"), &home, ".config", app),
            data: xdg_dir(
                std::env::var_os("XDG_DATA_HOME"),
                &home,
                ".local/share",
                app,
            ),
            state: xdg_dir(
                std::env::var_os("XDG_STATE_HOME"),
                &home,
                ".local/state",
                app,
            ),
        };
        for dir in [&dirs.config, &dirs.data, &dirs.state] {
            std::fs::create_dir_all(dir)
                .map_err(|err| SyncError::io("create daemon directory", dir, err))?;
        }
        Ok(dirs)
    }

    /// Explicit directories, bypassing the environment: tests, and an
    /// operator who points every path at one tree.
    pub fn with_dirs(
        config: impl Into<PathBuf>,
        data: impl Into<PathBuf>,
        state: impl Into<PathBuf>,
    ) -> XdgDirs {
        XdgDirs {
            config: config.into(),
            data: data.into(),
            state: state.into(),
        }
    }
}

/// Apply the XDG resolution rule to one base directory.
///
/// Per the XDG Base Directory specification an unset **or empty** variable
/// falls back to the default, and a relative path "should be considered
/// invalid and ignored" — a relative `XDG_DATA_HOME` would otherwise resolve
/// `sync.db` against whatever directory systemd happened to start us in, so
/// the daemon would silently use a different database per working directory.
pub fn xdg_dir(explicit: Option<OsString>, home: &Path, fallback: &str, app: &str) -> PathBuf {
    let base = explicit
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(fallback));
    base.join(app)
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

/// Environment-variable name carrying the secret for `key`, under `prefix`.
///
/// Engine keys look like `sync/<ULID>/credential`, so folding every
/// non-alphanumeric character to `_` is injective over the keys that actually
/// occur; it is not injective in general, which is fine because the host —
/// not a user — chooses these.
pub fn env_var_name(prefix: &str, key: &str) -> String {
    let mut name = String::with_capacity(prefix.len() + key.len());
    name.push_str(prefix);
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
pub fn secret_file_name(key: &str) -> String {
    key.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

/// Strip a trailing newline from a secret.
///
/// `echo token > secret` and a heredoc both append one, and a credential never
/// legitimately ends in CR or LF — so this removes a near-universal footgun
/// without touching any byte a token could actually contain.
pub fn trim_secret(raw: &str) -> String {
    raw.trim_end_matches(['\r', '\n']).to_owned()
}

/// Reject a secret file any account but the owner can read.
///
/// `mode & 0o077` is the group+other bits: this is the same test `ssh` applies
/// to a private key, and for the same reason — a token readable by `nogroup`
/// on a shared box is already leaked.
pub fn check_secret_permissions(path: &Path, mode: u32) -> Result<()> {
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

/// `O_NOFOLLOW`, as `OpenOptionsExt::custom_flags` takes it.
const NOFOLLOW: i32 = rustix::fs::OFlags::NOFOLLOW.bits() as i32;

/// Whether an `O_NOFOLLOW` open failed because the path is a symlink (`ELOOP`).
fn is_symlink_refusal(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error())
}

/// What the strict rule reads about a secrets directory (S-34).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirFacts {
    /// The full `st_mode`; only the permission bits are compared.
    pub mode: u32,
    pub uid: u32,
    pub is_symlink: bool,
}

/// The strict rule over a secrets directory: mode `0700`, owned by the
/// effective uid, and not a symlink.
///
/// Pure, so the "owned by another uid" arm is testable without root. A
/// symlinked directory is refused before its mode is read, because the mode a
/// symlink reports is not the target's.
pub fn check_secrets_dir(path: &Path, facts: DirFacts, euid: u32) -> Result<()> {
    let refuse = |reason: String| {
        Err(SyncError::Config(format!(
            "the secrets directory {} {reason}; it must be a real directory of mode 0700 \
             owned by the user this host runs as",
            path.display()
        )))
    };
    if facts.is_symlink {
        return refuse("is a symlink".to_owned());
    }
    if facts.uid != euid {
        return refuse(format!("is owned by uid {}, not by uid {euid}", facts.uid));
    }
    if facts.mode & 0o7777 != 0o700 {
        return refuse(format!(
            "has mode {:04o} — run: chmod 0700 {}",
            facts.mode & 0o7777,
            path.display()
        ));
    }
    Ok(())
}

/// A host's secrets: an optional systemd credentials directory, the
/// environment under one prefix, and a directory of `0600` files.
///
/// **The lookup order** (S-07). With a credentials directory (agentd):
/// `$CREDENTIALS_DIRECTORY/<name>`, then the environment, then the file.
/// Without one (syncd): the environment, then the file — exactly the order
/// `keeper-syncd` has always used.
#[derive(Clone)]
pub struct SecretStore {
    env_prefix: &'static str,
    credentials_dir: Option<PathBuf>,
    dir: PathBuf,
    strict_dir: bool,
    /// The prefixed variables [`SecretStore::scrub_env`] took out of the
    /// environment, by variable name. They answer where the environment did.
    held: HashMap<String, String>,
}

/// Names only: a held value never reaches a log line through `{:?}`.
impl std::fmt::Debug for SecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretStore")
            .field("env_prefix", &self.env_prefix)
            .field("credentials_dir", &self.credentials_dir)
            .field("dir", &self.dir)
            .field("strict_dir", &self.strict_dir)
            .field("held", &self.held.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl SecretStore {
    /// syncd's store: environment, then `<dir>/<file>`; no credentials
    /// directory and no directory check.
    pub fn new(env_prefix: &'static str, dir: impl Into<PathBuf>) -> SecretStore {
        SecretStore {
            env_prefix,
            credentials_dir: None,
            dir: dir.into(),
            strict_dir: false,
            held: HashMap::new(),
        }
    }

    /// Read `<credentials_dir>/<name>` first: systemd's `LoadCredential=`.
    pub fn with_credentials_dir(mut self, credentials_dir: Option<PathBuf>) -> SecretStore {
        self.credentials_dir = credentials_dir.filter(|dir| !dir.as_os_str().is_empty());
        self
    }

    /// Hold the secrets directory to the strict rule ([`check_secrets_dir`])
    /// and refuse a symlinked secret file (S-34).
    pub fn strict(mut self) -> SecretStore {
        self.strict_dir = true;
        self
    }

    /// The directory holding the `0600` files.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file a key is stored in.
    pub fn path_of(&self, key: &str) -> PathBuf {
        self.dir.join(secret_file_name(key))
    }

    /// The environment variable a key is read from.
    pub fn env_var_of(&self, key: &str) -> String {
        env_var_name(self.env_prefix, key)
    }

    /// The secret for `key`, or `None` when no source holds it.
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        if let Some(credentials) = &self.credentials_dir {
            let path = credentials.join(secret_file_name(key));
            match std::fs::read_to_string(&path) {
                Ok(raw) => return Ok(Some(trim_secret(&raw))),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(SyncError::io("read credential", &path, err)),
            }
        }

        // The environment: how a container or a drop-in injects a token
        // without ever writing it to a filesystem.
        let variable = self.env_var_of(key);
        let from_env = self
            .held
            .get(&variable)
            .cloned()
            .or_else(|| std::env::var(&variable).ok())
            .filter(|value| !value.is_empty());
        if let Some(value) = from_env {
            return Ok(Some(trim_secret(&value)));
        }

        if self.strict_dir && !self.check_dir_if_present()? {
            return Ok(None);
        }
        let path = self.path_of(key);
        let mut file = if self.strict_dir {
            // `O_NOFOLLOW`: a symlink at the path, planted before the read or
            // during it, is refused by the open itself.
            match std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(NOFOLLOW)
                .open(&path)
            {
                Ok(file) => file,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(err) if is_symlink_refusal(&err) => {
                    return Err(SyncError::Config(format!(
                    "secret file {} is a symlink; a secret file must be a real file of mode 0600",
                    path.display()
                )))
                }
                Err(err) => return Err(SyncError::io("open secret file", &path, err)),
            }
        } else {
            match std::fs::symlink_metadata(&path) {
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(err) => return Err(SyncError::io("stat secret file", &path, err)),
            }
            // A dangling link is an absent secret, as `metadata` always said.
            match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(err) => return Err(SyncError::io("open secret file", &path, err)),
            }
        };
        let meta = file
            .metadata()
            .map_err(|err| SyncError::io("stat secret file", &path, err))?;
        check_secret_permissions(&path, meta.permissions().mode())?;
        let mut raw = String::new();
        file.read_to_string(&mut raw)
            .map_err(|err| SyncError::io("read secret file", &path, err))?;
        Ok(Some(trim_secret(&raw)))
    }

    /// Write `value` to `key`'s file, `0600`, in a `0700` directory.
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
            .map_err(|err| SyncError::io("create secrets directory", &self.dir, err))?;
        if self.strict_dir {
            self.check_dir_if_present()?;
        }

        let path = self.path_of(key);
        // `mode` applies only when the file is created, so an existing file
        // keeps whatever bits it had — hence the explicit chmod below. Creating
        // it restricted first means there is never a window in which the
        // credential exists on disk world-readable.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        if self.strict_dir {
            // A planted symlink is refused by the open, before it truncates
            // or writes through to whatever the link names.
            options.custom_flags(NOFOLLOW);
        }
        let mut file = options.open(&path).map_err(|err| {
            if self.strict_dir && is_symlink_refusal(&err) {
                SyncError::Config(format!(
                    "secret file {} is a symlink; remove it before writing the secret",
                    path.display()
                ))
            } else {
                SyncError::io("create secret file", &path, err)
            }
        })?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|err| SyncError::io("restrict secret file", &path, err))?;
        file.write_all(value.as_bytes())
            .map_err(|err| SyncError::io("write secret file", &path, err))?;

        let variable = self.env_var_of(key);
        if self.held.contains_key(&variable) || std::env::var_os(&variable).is_some() {
            // Reads prefer the environment, so this write would be invisible.
            // Silence here is how an operator ends up debugging a rotated token
            // that "did not take".
            tracing::warn!(
                %variable,
                "a secret environment variable shadows the file just written; \
                 reads will keep returning the environment value"
            );
        }
        Ok(())
    }

    /// Remove `key`'s file. Removing an absent secret succeeds.
    pub fn delete(&self, key: &str) -> Result<()> {
        let path = self.path_of(key);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(SyncError::io("delete secret file", &path, err)),
        }
        let variable = self.env_var_of(key);
        if self.held.contains_key(&variable) || std::env::var_os(&variable).is_some() {
            tracing::warn!(
                %variable,
                "the secret file was removed but the environment still supplies this secret"
            );
        }
        Ok(())
    }

    /// Take every variable starting with the prefix out of the process's
    /// environment, holding its value in this store, so no child the process
    /// starts later inherits one (S-07).
    ///
    /// Every such variable is removed; one whose name or value is not text
    /// cannot be held, so it is named in the refusal rather than lost.
    ///
    /// Call it only while the process has one thread: another thread reading
    /// the environment while it changes is undefined behaviour in libc.
    pub fn scrub_env(&mut self) -> Result<()> {
        let prefix = OsStr::new(self.env_prefix);
        let names: Vec<OsString> = std::env::vars_os()
            .map(|(name, _)| name)
            .filter(|name| {
                name.as_encoded_bytes()
                    .starts_with(prefix.as_encoded_bytes())
            })
            .collect();
        let mut not_text = Vec::new();
        for name in names {
            match (
                name.to_str(),
                std::env::var_os(&name).map(OsString::into_string),
            ) {
                (Some(text), Some(Ok(value))) => {
                    self.held.insert(text.to_owned(), value);
                }
                (_, None) => {}
                _ => not_text.push(name.to_string_lossy().into_owned()),
            }
            std::env::remove_var(&name);
        }
        if not_text.is_empty() {
            Ok(())
        } else {
            Err(SyncError::Config(format!(
                "{} is not text, so it was removed from the environment and is not a secret; set it to the secret's text",
                not_text.join(", ")
            )))
        }
    }

    /// Apply the strict rule to the directory when it exists. `false` when
    /// there is no directory, so nothing in it can be read.
    fn check_dir_if_present(&self) -> Result<bool> {
        let meta = match std::fs::symlink_metadata(&self.dir) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(SyncError::io("stat secrets directory", &self.dir, err)),
        };
        check_secrets_dir(
            &self.dir,
            DirFacts {
                mode: meta.mode(),
                uid: meta.uid(),
                is_symlink: meta.file_type().is_symlink(),
            },
            rustix::process::geteuid().as_raw(),
        )?;
        Ok(true)
    }
}

/// Every `PATH` entry's `program`, in the order `PATH` lists them.
///
/// Candidates, not an answer: which of them is a *usable* git is decided by
/// probing (`crate::git::resolve`). Written out rather than shelling out to
/// `which`: resolving a hard prerequisite by spawning another process that
/// might equally be missing is circular.
pub fn path_candidates(program: &str) -> Vec<PathBuf> {
    candidates_in(std::env::var_os("PATH").as_deref(), program)
}

/// [`path_candidates`] over an explicit `PATH`-shaped value.
pub fn candidates_in(path_var: Option<&OsStr>, program: &str) -> Vec<PathBuf> {
    let Some(path_var) = path_var else {
        return Vec::new();
    };
    std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(program))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "keeper-sync";

    fn write_with_mode(path: &Path, contents: &str, mode: u32) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
        std::fs::write(path, contents).expect("write");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }

    #[test]
    fn xdg_falls_back_to_home_when_unset() {
        let home = Path::new("/home/dev");
        assert_eq!(
            xdg_dir(None, home, ".config", APP),
            PathBuf::from("/home/dev/.config/keeper-sync")
        );
        assert_eq!(
            xdg_dir(None, home, ".local/share", APP),
            PathBuf::from("/home/dev/.local/share/keeper-sync")
        );
        assert_eq!(
            xdg_dir(None, home, ".local/state", APP),
            PathBuf::from("/home/dev/.local/state/keeper-sync")
        );
    }

    #[test]
    fn xdg_honours_an_absolute_override() {
        assert_eq!(
            xdg_dir(
                Some(OsString::from("/srv/keeper/cfg")),
                Path::new("/home/dev"),
                ".config",
                APP
            ),
            PathBuf::from("/srv/keeper/cfg/keeper-sync")
        );
    }

    #[test]
    fn xdg_ignores_an_empty_or_relative_override() {
        let home = Path::new("/home/dev");
        // The spec says empty means "unset"...
        assert_eq!(
            xdg_dir(Some(OsString::new()), home, ".config", APP),
            PathBuf::from("/home/dev/.config/keeper-sync")
        );
        // ...and that a relative path is invalid. Honouring it would resolve
        // sync.db against the process CWD, giving a different database per
        // working directory.
        assert_eq!(
            xdg_dir(Some(OsString::from("relative/cfg")), home, ".config", APP),
            PathBuf::from("/home/dev/.config/keeper-sync")
        );
    }

    #[test]
    fn a_secret_key_can_never_escape_the_secrets_directory() {
        let store = SecretStore::new("KEEPER_SYNC_SECRET_", "/c/secrets");

        let path = store.path_of("../../etc/shadow");

        assert_eq!(path.parent(), Some(Path::new("/c/secrets")));
        assert_eq!(
            path.file_name().and_then(OsStr::to_str),
            Some("______etc_shadow")
        );
    }

    #[test]
    fn the_secret_environment_variable_name_is_derived_from_the_key() {
        assert_eq!(
            env_var_name("KEEPER_SYNC_SECRET_", "sync/01PROFILE/credential"),
            "KEEPER_SYNC_SECRET_SYNC_01PROFILE_CREDENTIAL"
        );
        assert_eq!(
            env_var_name("KEEPER_AGENTD_SECRET_", "cliproxy"),
            "KEEPER_AGENTD_SECRET_CLIPROXY"
        );
    }

    #[test]
    fn a_group_or_world_readable_secret_file_is_refused_naming_the_mode() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("token");
        write_with_mode(&path, "s3cret\n", 0o644);

        let message = check_secret_permissions(&path, 0o100644)
            .expect_err("0644 must be refused")
            .to_string();

        assert!(
            message.contains("it must be 0600 — run: chmod 0600"),
            "{message}"
        );
        assert!(check_secret_permissions(&path, 0o100640).is_err());
        assert!(check_secret_permissions(&path, 0o100600).is_ok());
        assert!(check_secret_permissions(&path, 0o100400).is_ok());
    }

    #[test]
    fn a_trailing_newline_is_trimmed_and_nothing_else() {
        assert_eq!(trim_secret("tok\r\n"), "tok");
        assert_eq!(trim_secret(" tok \n\n"), " tok ");
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

    /// S-07's order, with all three sources present, then two, then one.
    /// Each case uses its own key, so no other test's variable is touched.
    #[test]
    fn systemd_credentials_win_then_the_environment_then_a_0600_file() {
        let root = tempfile::tempdir().expect("temp dir");
        let credentials = root.path().join("credentials");
        let secrets = root.path().join("secrets");
        std::fs::create_dir_all(&credentials).expect("credentials dir");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&secrets)
            .expect("secrets dir");
        let store = SecretStore::new("KEEPER_XDG_TEST_ORDER_", &secrets)
            .with_credentials_dir(Some(credentials.clone()))
            .strict();

        let key = "order";
        std::fs::write(credentials.join(key), "from-credential\n").expect("credential");
        std::env::set_var(store.env_var_of(key), "from-env");
        write_with_mode(&store.path_of(key), "from-file\n", 0o600);
        assert_eq!(
            store.get(key).expect("three"),
            Some("from-credential".to_owned())
        );

        std::fs::remove_file(credentials.join(key)).expect("drop credential");
        assert_eq!(store.get(key).expect("two"), Some("from-env".to_owned()));

        std::env::remove_var(store.env_var_of(key));
        assert_eq!(store.get(key).expect("one"), Some("from-file".to_owned()));

        std::fs::remove_file(store.path_of(key)).expect("drop file");
        assert_eq!(store.get(key).expect("none"), None);
    }

    /// syncd's store never looks in a credentials directory, even when the
    /// process has one, and keeps the environment ahead of the file.
    #[test]
    fn the_credentials_directory_is_never_read_without_being_given() {
        let root = tempfile::tempdir().expect("temp dir");
        let secrets = root.path().join("secrets");
        let store = SecretStore::new("KEEPER_XDG_TEST_SYNCD_", &secrets);
        let key = "sync/01P/credential";
        // A credentials directory beside it holding the key: never read.
        let credentials = root.path().join("credentials");
        write_with_mode(&credentials.join(secret_file_name(key)), "cred", 0o600);
        write_with_mode(&store.path_of(key), "file", 0o600);

        assert_eq!(store.get(key).expect("file"), Some("file".to_owned()));
        std::env::set_var(store.env_var_of(key), "env");
        assert_eq!(store.get(key).expect("env"), Some("env".to_owned()));
        std::env::remove_var(store.env_var_of(key));
    }

    /// S-34 on real files, and over the pure check for the uid arm.
    #[test]
    fn a_secrets_directory_must_be_0700_owned_and_real() {
        let root = tempfile::tempdir().expect("temp dir");
        let euid = rustix::process::geteuid().as_raw();

        // A 0755 directory is refused, naming the path and the mode.
        let loose = root.path().join("loose");
        std::fs::create_dir(&loose).expect("loose dir");
        std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        write_with_mode(&loose.join("tok"), "v", 0o600);
        let strict = SecretStore::new("KEEPER_XDG_TEST_STRICT_", &loose).strict();
        let message = strict.get("tok").expect_err("0755 refused").to_string();
        assert!(message.contains(&loose.display().to_string()), "{message}");
        assert!(message.contains("mode 0755"), "{message}");
        // syncd's store, without the strict rule, reads the same directory.
        let lenient = SecretStore::new("KEEPER_XDG_TEST_STRICT_", &loose);
        assert_eq!(lenient.get("tok").expect("lenient"), Some("v".to_owned()));

        // A symlinked directory is refused, naming the reason.
        let real = root.path().join("real");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&real)
            .expect("real dir");
        write_with_mode(&real.join("tok"), "v", 0o600);
        let linked = root.path().join("linked");
        std::os::unix::fs::symlink(&real, &linked).expect("symlink dir");
        let message = SecretStore::new("KEEPER_XDG_TEST_STRICT_", &linked)
            .strict()
            .get("tok")
            .expect_err("symlinked dir refused")
            .to_string();
        assert!(message.contains(&linked.display().to_string()), "{message}");
        assert!(message.contains("is a symlink"), "{message}");

        // A symlinked secret file inside a good directory is refused.
        let target = root.path().join("elsewhere");
        write_with_mode(&target, "stolen", 0o600);
        std::os::unix::fs::symlink(&target, real.join("linked")).expect("symlink file");
        let good = SecretStore::new("KEEPER_XDG_TEST_STRICT_", &real).strict();
        let message = good
            .get("linked")
            .expect_err("symlinked file refused")
            .to_string();
        assert!(message.contains("is a symlink"), "{message}");
        assert_eq!(good.get("tok").expect("real file"), Some("v".to_owned()));

        // Owned by another uid: the pure check.
        let message = check_secrets_dir(
            &real,
            DirFacts {
                mode: 0o040700,
                uid: euid.wrapping_add(1),
                is_symlink: false,
            },
            euid,
        )
        .expect_err("foreign owner refused")
        .to_string();
        assert!(message.contains("is owned by uid"), "{message}");
        assert!(check_secrets_dir(
            &real,
            DirFacts {
                mode: 0o040700,
                uid: euid,
                is_symlink: false,
            },
            euid,
        )
        .is_ok());
    }

    #[test]
    fn scrub_takes_the_prefixed_variables_and_keeps_answering_them() {
        let root = tempfile::tempdir().expect("temp dir");
        let mut store = SecretStore::new("KEEPER_XDG_TEST_SCRUB_", root.path().join("s"));
        std::env::set_var("KEEPER_XDG_TEST_SCRUB_X", "v");
        std::env::set_var("KEEPER_XDG_TEST_SCRUBBED_NOT", "kept");

        store.scrub_env().expect("every value is text");

        assert_eq!(std::env::var_os("KEEPER_XDG_TEST_SCRUB_X"), None);
        assert_eq!(store.get("x").expect("held"), Some("v".to_owned()));
        // The prefix is matched as a prefix of the name, nothing wider.
        assert_eq!(
            std::env::var("KEEPER_XDG_TEST_SCRUBBED_NOT")
                .ok()
                .as_deref(),
            Some("kept")
        );
        std::env::remove_var("KEEPER_XDG_TEST_SCRUBBED_NOT");
    }

    #[test]
    fn scrub_names_a_value_that_is_not_text_and_still_removes_it() {
        use std::os::unix::ffi::OsStrExt as _;
        let root = tempfile::tempdir().expect("temp dir");
        let mut store = SecretStore::new("KEEPER_XDG_TEST_NOTEXT_", root.path().join("s"));
        std::env::set_var("KEEPER_XDG_TEST_NOTEXT_BAD", OsStr::from_bytes(b"\xff\xfe"));
        std::env::set_var("KEEPER_XDG_TEST_NOTEXT_GOOD", "v");

        let message = store.scrub_env().expect_err("refused").to_string();

        assert!(message.contains("KEEPER_XDG_TEST_NOTEXT_BAD"), "{message}");
        assert!(!message.contains("GOOD"), "{message}");
        assert_eq!(std::env::var_os("KEEPER_XDG_TEST_NOTEXT_BAD"), None);
        assert_eq!(store.get("good").expect("held"), Some("v".to_owned()));
    }

    /// `set` opens with `O_NOFOLLOW`: a link planted at the secret's path is
    /// refused, and the file it names is neither truncated nor written.
    #[test]
    fn a_strict_set_never_writes_through_a_symlink() {
        let root = tempfile::tempdir().expect("temp dir");
        let secrets = root.path().join("secrets");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&secrets)
            .expect("secrets dir");
        let target = root.path().join("operator");
        write_with_mode(&target, "the operator's", 0o600);
        let store = SecretStore::new("KEEPER_XDG_TEST_SETLINK_", &secrets).strict();
        std::os::unix::fs::symlink(&target, store.path_of("tok")).expect("plant");

        let message = store.set("tok", "new").expect_err("refused").to_string();

        assert!(message.contains("is a symlink"), "{message}");
        assert_eq!(
            std::fs::read_to_string(&target).expect("target"),
            "the operator's"
        );
    }
}
