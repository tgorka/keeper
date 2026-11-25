//! keeper's local socket in the app (story 123.3; MA/AD-4, A-2, DR-3, SR-4,
//! A-5, DR-17; macOS only, SR-20, XR-7).
//!
//! Edited by inspection: the shell crate does not build on the Linux dev
//! host, so CI's macOS `Rust (fmt, clippy, test)` job is this file's gate.
//! `keeper-cli debug app-start` is its test composition on Linux (SR-14):
//! the same order with the same calls.
//!
//! The order at start:
//! 1. [`take_lock`] in `run()`, before the app state, the plugins and the
//!    config windows exist: the lock is the data directory's, held for the
//!    whole run whether the socket is on or off. While another process holds
//!    it, ask again for up to ten seconds; then [`run_refused`] runs a keeper
//!    with no window, no webview and no app state (Tauri starts the config
//!    windows' webviews before `setup`, so a refusal inside `setup` would
//!    already have served their IPC), which names the holder and offers to
//!    try again. When the lock cannot be asked for here, open the data,
//!    leave the socket off and log one WARN ([`announce`]).
//! 2. The app is built and its `setup` opens the data.
//! 3. [`start`]: serve `<data dir>/wire/wire.sock` while `wire.enabled` is on
//!    (a debug build also with `KEEPER_WIRE_DEBUG_ON=1`), and stop within a
//!    second of it going off. At each switch-on the runtime is composed with
//!    the `graph` module, worker-free, over `Hq::sources()` with the person
//!    as caller, and each listed domain's replica is subscribed to the
//!    graph's units. Until 123.4 adds the reach list no domain is listed (the
//!    list starts empty, A-9), so nothing is subscribed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use keeper_app_api::DomainVm;
use keeper_core::platform::Platform as _;
use keeper_graph::module::{DomainBinding, GraphConfig, GraphModule};
use keeper_runtime::{AppInfo, Runtime};
use keeper_runtime_api::Module;
use keeper_wire::{Dispatch, Limits};
use keeper_wire_api::{Admission, DataLock};
use sha2::{Digest as _, Sha256};
use tauri_plugin_dialog::{DialogExt as _, MessageDialogButtons};

/// How long the app asks again while another process holds the lock.
const LOCK_WAIT: Duration = Duration::from_secs(10);

/// The one committed catalogue snapshot (SR-20), as keeper-cli embeds it.
const SNAPSHOT: &[u8] = include_bytes!("../../../../catalogue/snapshot.json");

/// The lock, kept for the whole run.
static LOCK: OnceLock<Arc<DataLock>> = OnceLock::new();

/// Why the lock could not be asked for, logged once logging is up.
static UNSUPPORTED: OnceLock<String> = OnceLock::new();

/// Take the data directory's lock before anything opens the data or builds a
/// window. `Some(sentence)`: another process holds it (or the directory is
/// unsafe), so open nothing and show the sentence. A data directory the
/// platform cannot name, or a lock that cannot be asked for, opens the data
/// with the socket off, so a broken lock file never keeps a person out of
/// keeper.
pub(crate) fn take_lock() -> Option<String> {
    let Ok(data_dir) = crate::ipc::DesktopPlatform.data_dir() else {
        return None;
    };
    let holder = format!("keeper (the app), pid {}", std::process::id());
    match keeper_wire_api::admit(&data_dir, &holder, LOCK_WAIT) {
        Admission::Held(lock) => {
            let _ = LOCK.set(Arc::new(lock));
            None
        }
        Admission::Unsupported(sentence) => {
            let _ = UNSUPPORTED.set(sentence);
            None
        }
        Admission::Refused(sentence) => Some(sentence),
    }
}

/// Log what the lock answered, after `debug_log::init`.
pub(crate) fn announce() {
    if let Some(sentence) = UNSUPPORTED.get() {
        tracing::warn!(code = "lock_unsupported", "{sentence}");
    }
}

/// The lock is held elsewhere (or the directory is unsafe): run a keeper
/// that opens nothing — no config window, so no webview and no IPC, no app
/// state, none of the data's plugins — says so, and offers to try again (a
/// restart asks for the lock afresh) or to quit.
pub(crate) fn run_refused(mut context: tauri::Context<tauri::Wry>, sentence: String) {
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            // Off the main thread: the dialog's answer arrives once the event
            // loop runs.
            std::thread::spawn(move || {
                let again = handle
                    .dialog()
                    .message(format!(
                        "{sentence}\n\nQuit the other keeper, then try again."
                    ))
                    .title("keeper is already open")
                    .buttons(MessageDialogButtons::OkCancelCustom(
                        "Try again".to_owned(),
                        "Quit".to_owned(),
                    ))
                    .blocking_show();
                if again {
                    handle.restart();
                } else {
                    handle.exit(0);
                }
            });
            Ok(())
        })
        .build(context);
    match app {
        Ok(app) => app.run(|_, _| {}),
        Err(error) => tracing::error!(%error, "keeper could not show why it did not open"),
    }
}

/// Whether the switch is on: `wire.enabled` from this device's settings
/// table (a `SessionState` key no file sets, DR-1), or a debug build's
/// override.
fn switched_on(data_dir: &Path) -> bool {
    let enabled = keeper_core::registry::get_setting(data_dir, "wire.enabled")
        .ok()
        .flatten()
        .is_some_and(|value| value.trim() == "1");
    enabled || keeper_wire::debug_override()
}

/// The runtime the socket dispatches to, composed at a switch-on.
fn compose(data_dir: &Path) -> Result<Arc<dyn Dispatch>, String> {
    let sources = crate::hq_ipc::service()
        .map(|hq| hq.sources())
        .unwrap_or_default();
    // The reach list is 123.4's; until then it is empty (A-9).
    let listed: Vec<String> = Vec::new();
    for (database, followed) in keeper_graph::client::follow(&sources, &listed) {
        if let Err(error) = followed {
            tracing::warn!(%database, %error, "the graph of a listed domain was not subscribed");
        }
    }
    let server = keeper_core::registry::get_agents_server_url(data_dir)
        .ok()
        .flatten()
        .unwrap_or_default();
    let state = keeper_ports::server::ServerUrl::parse(&server)
        .ok()
        .flatten()
        .map_or_else(
            || data_dir.join("hq"),
            |url| keeper_core::hq::databases::server_dir(data_dir, &url),
        );
    let domains: Vec<DomainBinding> = sources
        .into_iter()
        .map(|source| DomainBinding {
            server: server.clone(),
            database: source.database.clone(),
            actor: source.actor.clone(),
            client: source.client.clone(),
            source: Ok(source),
        })
        .collect();
    let info = AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        build: option_env!("KEEPER_BUILD_SHA").unwrap_or("dev").to_owned(),
        snapshot_digest: format!("{:x}", Sha256::digest(SNAPSHOT)),
        domains: domains
            .iter()
            .map(|d| DomainVm {
                server: d.server.clone(),
                domain: d.database.clone(),
            })
            .collect(),
    };
    // Worker-free, with the person as caller and no `human-reads` (A-5).
    let config = GraphConfig {
        state,
        worker: None,
        macos_evidence: false,
        worker_sha256: None,
        ledger: None,
    };
    let modules: Vec<Arc<dyn Module>> = vec![
        keeper_runtime::app_module(),
        Arc::new(GraphModule::new(config, domains)),
    ];
    let (runtime, _): (Runtime, _) =
        Runtime::build(info, modules).map_err(|error| error.to_string())?;
    Ok(Arc::new(runtime.wire_dispatcher()))
}

/// Follow the switch for the rest of the run. Does nothing when the lock is
/// not this process's (the socket stays off).
pub(crate) fn start(data_dir: PathBuf) {
    let Some(lock) = LOCK.get().cloned() else {
        return;
    };
    // The app never quits the switch: the process's end stops it, and the
    // kernel releases the lock with it.
    let (quit, watching) = tokio::sync::watch::channel(false);
    tauri::async_runtime::spawn(async move {
        let _quit = quit;
        let on_dir = data_dir.clone();
        keeper_wire::follow_switch(
            lock,
            move || switched_on(&on_dir),
            move || compose(&data_dir),
            Limits::default(),
            watching,
        )
        .await;
    });
}
