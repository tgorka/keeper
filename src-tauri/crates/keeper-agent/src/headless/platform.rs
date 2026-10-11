//! `HeadlessPlatform`: keeper-core's `Platform` on a server (story 90.3).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use keeper_core::error::{CoreError, PlatformError};
use keeper_core::platform::Platform;
use keeper_core::vm::NotifyTarget;
use keeper_sync::xdg::SecretStore;

/// Which secret answers a keychain key.
///
/// keeper-core and keeper-sync ask for secrets by their own keys —
/// `bot_provider_token/{p}`, `bot_token/{p}/{bot}`, `sync/<pid>/credential`,
/// `agents/<user>/session`, `agents/<user>/sdk-passphrase`. A key the host
/// bound to a `secret:<name>` of `agentd.toml` (a provider's or a drive's
/// credential) is answered by that name and is read-only: `set` and `delete`
/// on it are refused. Every other key — a bot's own token, which agentd never
/// configures, and the copies' sessions and store passphrases `login` writes —
/// is a store file of its own.
#[derive(Debug)]
pub struct SecretMap {
    store: SecretStore,
    bound: RwLock<HashMap<String, String>>,
}

impl SecretMap {
    pub fn new(store: SecretStore) -> SecretMap {
        SecretMap {
            store,
            bound: RwLock::new(HashMap::new()),
        }
    }

    /// Answer `key` with `secret:<name>` from now on.
    pub fn bind(&self, key: impl Into<String>, name: impl Into<String>) {
        if let Ok(mut bound) = self.bound.write() {
            bound.insert(key.into(), name.into());
        }
    }

    /// The `secret:<name>` `key` is bound to, if any.
    fn bound_name(&self, key: &str) -> Option<String> {
        self.bound
            .read()
            .ok()
            .and_then(|bound| bound.get(key).cloned())
    }

    /// A bound key is the operator's secret, read-only to keeper: a core
    /// path that writes or deletes it would overwrite or remove the file
    /// `agentd.toml` names.
    fn writable(&self, key: &str) -> keeper_sync::Result<()> {
        match self.bound_name(key) {
            Some(name) => Err(keeper_sync::SyncError::Config(format!(
                "{key} is bound to secret:{name} by agentd.toml; change the file, not the store"
            ))),
            None => Ok(()),
        }
    }

    pub fn get(&self, key: &str) -> keeper_sync::Result<Option<String>> {
        self.store
            .get(&self.bound_name(key).unwrap_or_else(|| key.to_owned()))
    }

    pub fn set(&self, key: &str, value: &str) -> keeper_sync::Result<()> {
        self.writable(key)?;
        self.store.set(key, value)
    }

    pub fn delete(&self, key: &str) -> keeper_sync::Result<()> {
        self.writable(key)?;
        self.store.delete(key)
    }

    /// The store underneath.
    pub fn store(&self) -> &SecretStore {
        &self.store
    }
}

/// `Platform` over the XDG data directory and a [`SecretMap`].
#[derive(Debug, Clone)]
pub struct HeadlessPlatform {
    data_dir: PathBuf,
    secrets: Arc<SecretMap>,
}

impl HeadlessPlatform {
    pub fn new(data_dir: impl Into<PathBuf>, secrets: Arc<SecretMap>) -> HeadlessPlatform {
        HeadlessPlatform {
            data_dir: data_dir.into(),
            secrets,
        }
    }

    pub fn secrets(&self) -> &Arc<SecretMap> {
        &self.secrets
    }
}

fn keychain(error: keeper_sync::SyncError) -> CoreError {
    CoreError::Platform(PlatformError::Keychain(error.to_string()))
}

impl Platform for HeadlessPlatform {
    fn data_dir(&self) -> Result<PathBuf, CoreError> {
        Ok(self.data_dir.clone())
    }

    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        self.secrets.set(key, value).map_err(keychain)
    }

    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
        self.secrets.get(key).map_err(keychain)
    }

    fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
        self.secrets.delete(key).map_err(keychain)
    }

    fn open_url(&self, _url: &str) -> Result<(), CoreError> {
        Err(CoreError::Unsupported(
            "keeper-agentd has no browser to open".to_owned(),
        ))
    }

    fn notify(&self, title: &str, body: &str, _target: &NotifyTarget) -> Result<(), CoreError> {
        // A server has no notifier; its log is the surface.
        tracing::warn!(%title, %body, "notification");
        Ok(())
    }

    fn sidecar_path(&self, name: &str) -> Result<PathBuf, CoreError> {
        Err(CoreError::Unsupported(format!(
            "keeper-agentd bundles no sidecar ({name})"
        )))
    }

    fn exclude_from_backup(&self, _path: &Path) -> Result<(), CoreError> {
        Ok(())
    }

    fn set_badge_count(&self, _count: Option<u32>) -> Result<(), CoreError> {
        Ok(())
    }
}
