//! The headless host's process layer: its directories, its secrets, its own
//! sync engine and its provider rows (AD-375, AD-376, AD-377; story 90.3).
//!
//! `keeper-agentd` runs as its principal's own OS user. Everything it keeps is
//! under that user's XDG directories, named `keeper-agentd`, never syncd's:
//!
//! ```text
//! $XDG_CONFIG_HOME/keeper-agentd/agentd.toml
//! $XDG_DATA_HOME/keeper-agentd/{sync.db, keeper.db, .keeper-agentd, drives/<id>/, agents/<user>/sdk}
//! $XDG_STATE_HOME/keeper-agentd/secrets/      0700, one 0600 file per secret
//! ```

mod engine;
mod harden;
mod platform;
mod providers;
mod sync_platform;

use std::path::PathBuf;

pub use engine::{
    enforce_mounts, open_engine, zone_verdicts, AgentdEngine, MountedDrive, ZoneVerdict,
};
pub use harden::{harden_process, HardenError};
pub use platform::{HeadlessPlatform, SecretMap};
pub use providers::{apply_providers, AppliedProvider};
pub use sync_platform::HeadlessSyncPlatform;

use keeper_sync::xdg::{SecretStore, XdgDirs};

/// The per-application segment of every XDG directory.
pub const APP_DIR: &str = "keeper-agentd";
/// The secrets directory, under the state directory (C7).
pub const SECRETS_DIR: &str = "secrets";
/// The environment's secret prefix: `KEEPER_AGENTD_SECRET_<NAME>`.
pub const SECRET_ENV_PREFIX: &str = "KEEPER_AGENTD_SECRET_";
/// The file beside `sync.db` that says agentd created it (R7).
pub const MARKER_FILE: &str = ".keeper-agentd";
/// The checkouts, under the data directory.
pub const DRIVES_DIR: &str = "drives";

/// What a missing `git` tells the operator: the tail of
/// `keeper_sync::git::resolve`'s sentence.
pub const GIT_ADVICE: &str = "install git 2.42 or newer with `apt install git`, \
     `dnf install git`, `pacman -S git` or `apk add git`, then restart keeper-agentd";

/// agentd's secret store over its directories (C7, S-07, S-34): the systemd
/// credentials directory when the unit gives one, then the environment, then
/// `$XDG_STATE_HOME/keeper-agentd/secrets/`, held to the strict rule.
pub fn secret_store(dirs: &XdgDirs) -> SecretStore {
    SecretStore::new(SECRET_ENV_PREFIX, dirs.state.join(SECRETS_DIR))
        .with_credentials_dir(std::env::var_os("CREDENTIALS_DIRECTORY").map(PathBuf::from))
        .strict()
}

/// Where drive `id` is checked out.
pub fn drive_path(dirs: &XdgDirs, id: &str) -> PathBuf {
    dirs.data.join(DRIVES_DIR).join(id)
}

/// The error a headless start ends in, each with the exit code `keeper-agentd`
/// gives it (syncd's numbers: 1 runtime, 2 configuration, 3 no git).
#[derive(Debug, thiserror::Error)]
pub enum HeadlessError {
    #[error("{0}")]
    Mount(#[from] keeper_core::agents::mount::MountRefusal),
    #[error(
        "{path} was not created by keeper-agentd (there is no {MARKER_FILE} beside it), so it is never opened: \
         another engine's database would have its running work requeued. Move it away, or point XDG_DATA_HOME elsewhere."
    )]
    ForeignDatabase { path: PathBuf },
    #[error("{0}")]
    Config(String),
    /// A failure while running that a restart may cure.
    #[error("{0}")]
    Runtime(String),
    #[error(transparent)]
    Sync(#[from] keeper_sync::SyncError),
    #[error(transparent)]
    Core(#[from] keeper_core::error::CoreError),
}

impl HeadlessError {
    /// `keeper-agentd`'s exit code for this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            HeadlessError::Mount(_)
            | HeadlessError::ForeignDatabase { .. }
            | HeadlessError::Config(_) => 2,
            HeadlessError::Sync(keeper_sync::SyncError::GitMissing { .. }) => 3,
            HeadlessError::Sync(keeper_sync::SyncError::Config(_)) => 2,
            HeadlessError::Runtime(_) | HeadlessError::Sync(_) | HeadlessError::Core(_) => 1,
        }
    }
}
