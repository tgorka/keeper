//! `HeadlessSyncPlatform`: keeper-sync's `SyncPlatform` on a server (story 90.3).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use keeper_sync::xdg::path_candidates;
use keeper_sync::{GitRequest, Result, SyncPlatform};

use super::platform::SecretMap;
use super::GIT_ADVICE;

/// `SyncPlatform` over agentd's data directory and its [`SecretMap`].
///
/// `bot_task_runner` keeps the trait's `None`: a `TaskKind::Bot` row on this
/// host records `NO_BOT_RUNNER_SENTENCE` (AD-224, AD-226, R9). `open_file_state`
/// keeps the refusing default too: agentd materialises, so it never releases
/// content and never needs the answer.
#[derive(Debug, Clone)]
pub struct HeadlessSyncPlatform {
    data_dir: PathBuf,
    host: String,
    secrets: Arc<SecretMap>,
}

impl HeadlessSyncPlatform {
    /// `host` is `agentd.toml`'s `host`: every commit this engine makes says so.
    pub fn new(
        data_dir: impl Into<PathBuf>,
        host: impl Into<String>,
        secrets: Arc<SecretMap>,
    ) -> HeadlessSyncPlatform {
        HeadlessSyncPlatform {
            data_dir: data_dir.into(),
            host: host.into(),
            secrets,
        }
    }

    pub fn secrets(&self) -> &Arc<SecretMap> {
        &self.secrets
    }
}

impl SyncPlatform for HeadlessSyncPlatform {
    fn data_dir(&self) -> Result<PathBuf> {
        Ok(self.data_dir.clone())
    }

    fn secret_get(&self, key: &str) -> Result<Option<String>> {
        self.secrets.get(key)
    }

    fn secret_set(&self, key: &str, value: &str) -> Result<()> {
        self.secrets.set(key, value)
    }

    fn secret_delete(&self, key: &str) -> Result<()> {
        self.secrets.delete(key)
    }

    fn notify(&self, title: &str, body: &str) {
        tracing::warn!(%title, %body, "sync notification");
    }

    fn now_ms(&self) -> i64 {
        // A pre-1970 clock reads as 0: every scheduled unit is due, the safe
        // direction (syncd's reading).
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as i64)
    }

    fn free_space(&self, _path: &Path) -> Option<u64> {
        None
    }

    fn git_program(&self) -> Result<PathBuf> {
        GitRequest::search(path_candidates("git"), GIT_ADVICE)
            .resolve()
            .program()
    }

    fn host_label(&self) -> String {
        self.host.clone()
    }
}
