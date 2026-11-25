//! The switch (story 123.3, NFR-E123-1, DR-1): a composition root serves the
//! socket while its switch is on and stops within a second of it going off.
//! The switch is the root's to read (the app: `wire.enabled` from its
//! settings table, never a synced file; a debug build may also be switched on
//! by [`debug_override`]). The lock is taken before the switch is first read
//! and is held whether the socket is on or off.

use std::sync::Arc;
use std::time::Duration;

use keeper_wire_api::DataLock;
use tokio::sync::watch;

use crate::{Dispatch, Hooks, Limits, Server};

/// How often [`follow_switch`] reads the switch.
pub const SWITCH_POLL: Duration = Duration::from_millis(200);

/// The debug-build override's variable.
pub const DEBUG_ON_VAR: &str = "KEEPER_WIRE_DEBUG_ON";

/// Whether `KEEPER_WIRE_DEBUG_ON=1` switches the socket on. Read only in a
/// debug build; a release build answers `false` whatever the environment.
#[must_use]
pub fn debug_override() -> bool {
    #[cfg(debug_assertions)]
    {
        std::env::var_os(DEBUG_ON_VAR).is_some_and(|value| value == "1")
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

/// Serve `<data dir>/wire/wire.sock` while `on()` answers `true`, read every
/// [`SWITCH_POLL`]; stop it within a second of `on()` turning `false`, and
/// when `quit` turns `true` (then return). At each switch-on, `compose` builds
/// what the socket dispatches to (the root subscribes its listed domains
/// there); a refusal is logged once per switch-on and asked again at the next
/// read. Must run inside a tokio runtime.
pub async fn follow_switch<On, Compose>(
    lock: Arc<DataLock>,
    on: On,
    compose: Compose,
    limits: Limits,
    mut quit: watch::Receiver<bool>,
) where
    On: Fn() -> bool,
    Compose: Fn() -> Result<Arc<dyn Dispatch>, String>,
{
    let mut serving: Option<Server> = None;
    let mut warned = false;
    loop {
        if *quit.borrow() {
            break;
        }
        if on() {
            if serving.is_none() {
                let started = compose().and_then(|dispatch| {
                    Server::start(
                        Arc::clone(&lock),
                        dispatch,
                        limits.clone(),
                        Hooks::default(),
                    )
                    .map_err(|error| error.to_string())
                });
                match started {
                    Ok(server) => {
                        serving = Some(server);
                        warned = false;
                    }
                    Err(error) if !warned => {
                        tracing::warn!(%error, "keeper's socket is switched on but did not start");
                        warned = true;
                    }
                    Err(_) => {}
                }
            }
        } else {
            warned = false;
            if let Some(server) = serving.take() {
                server.stop().await;
            }
        }
        tokio::select! {
            () = tokio::time::sleep(SWITCH_POLL) => {}
            changed = quit.changed() => {
                // A dropped sender is a quit too.
                if changed.is_err() {
                    break;
                }
            }
        }
    }
    if let Some(server) = serving.take() {
        server.stop().await;
    }
}
