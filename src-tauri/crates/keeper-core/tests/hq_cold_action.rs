//! A launch that runs no setup — an action on a notification starting keeper
//! in the background on the iPhone — starts the agents server the EFFECTIVE
//! `agents.server_url` names (R1066K-03): the settings layers are installed
//! first (`config::install_launch_layers`, the call `hq_ipc::ensure` makes
//! through the shell's `install_settings_layers`), so an address only a file
//! sets is followed, and a stored address a file shadows is not.
//!
//! Its own test binary: the layer stack is process-wide and installed once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_core::error::CoreError;
use keeper_core::hq::routes::HttpRoutes;
use keeper_core::hq::server::ServerUrl;
use keeper_core::hq::syncular::SyncularOpener;
use keeper_core::hq::Hq;
use keeper_core::platform::Platform;

const HOST: &str = "kalypso";
const LAYERED: &str = "https://agents.layer.example.org";
const STORED: &str = "https://agents.stored.example.org";

struct Phone {
    data_dir: PathBuf,
    keychain: Mutex<HashMap<String, String>>,
}

impl Platform for Phone {
    fn data_dir(&self) -> Result<PathBuf, CoreError> {
        Ok(self.data_dir.clone())
    }
    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        self.keychain
            .lock()
            .expect("keychain")
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }
    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
        Ok(self.keychain.lock().expect("keychain").get(key).cloned())
    }
    fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
        self.keychain.lock().expect("keychain").remove(key);
        Ok(())
    }
    fn open_url(&self, _: &str) -> Result<(), CoreError> {
        Ok(())
    }
    fn notify(&self, _: &str, _: &str, _: &keeper_core::vm::NotifyTarget) -> Result<(), CoreError> {
        Ok(())
    }
    fn sidecar_path(&self, _: &str) -> Result<PathBuf, CoreError> {
        Err(CoreError::Unsupported("no sidecars".to_owned()))
    }
    fn exclude_from_backup(&self, _: &Path) -> Result<(), CoreError> {
        Ok(())
    }
    fn set_badge_count(&self, _: Option<u32>) -> Result<(), CoreError> {
        Ok(())
    }
}

/// The address the agents server a cold launch on `data_dir` starts with:
/// the service built as `hq_ipc::ensure` builds it, started, its status read.
async fn cold_start(data_dir: &Path) -> Option<String> {
    let phone = Arc::new(Phone {
        data_dir: data_dir.to_owned(),
        keychain: Mutex::new(HashMap::new()),
    });
    let hq = Hq::new(
        phone as Arc<dyn Platform>,
        Arc::new(SyncularOpener),
        Arc::new(HttpRoutes::new()),
        format!("keeper on {HOST}"),
        || HOST.to_owned(),
    );
    if let Err(error) = hq.start().await {
        panic!("the service did not start: {error}");
    }
    let mut status = hq.status();
    // The status is published as the start settles; give it a moment.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let server = status.borrow().server.clone();
        if server.is_some() || tokio::time::Instant::now() > deadline {
            hq.shutdown().await;
            return server;
        }
        let _ = tokio::time::timeout(Duration::from_millis(100), status.changed()).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cold_notification_action_starts_the_server_the_settings_layers_name() {
    let home = tempfile::tempdir().expect("home");
    let keeper = keeper_core::config::keeper_dir(home.path());
    std::fs::create_dir_all(&keeper).expect("~/.keeper");
    std::fs::write(
        keeper.join("keeper.toml"),
        format!("[settings]\n\"agents.server_url\" = \"{LAYERED}\"\n"),
    )
    .expect("layer file");

    // A layer-only address: no row in this device's settings.
    let layer_only = tempfile::tempdir().expect("data");
    // A layer over a different stored address.
    let shadowed = tempfile::tempdir().expect("data");
    let stored = ServerUrl::parse(STORED)
        .expect("parses")
        .expect("an address");
    keeper_core::registry::set_agents_server_url(shadowed.path(), Some(&stored)).expect("stored");

    // What the background launch does before it builds the service.
    assert!(keeper_core::config::install_launch_layers(
        Some(home.path()),
        HOST
    ));
    assert!(
        !keeper_core::config::install_launch_layers(Some(home.path()), HOST),
        "once per process: setup after a background launch reads nothing again"
    );

    let layered = ServerUrl::parse(LAYERED)
        .expect("parses")
        .expect("an address");
    for (data_dir, case) in [
        (layer_only.path(), "a layer-only address"),
        (shadowed.path(), "a layer over a stored address"),
    ] {
        assert_eq!(
            cold_start(data_dir).await.as_deref(),
            Some(layered.as_str()),
            "{case}"
        );
    }
}
