//! This Mac as an agents host (story 90.6, AD-374, AD-378, AD-379): the
//! app's side of `keeper_agent::desktop`, and Settings › Agents' commands.
//!
//! It decides nothing. Which zones host, under which pin, which session
//! this Mac claims and when it hands one back are `keeper_agent`'s and
//! `keeper_core::agents`'. This module hands them the app's facts — the
//! account's login and device name, the sync profiles, `keeper.db`'s pins,
//! the Matrix accounts' homeservers — and the app's one 1 Hz interval
//! (`lib.rs`, AD-62) drives it through [`tick`]: no clock of its own.
//!
//! Desktop-only: a phone is never a host (P5).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use keeper_agent::desktop::{self, DesktopFacts, DesktopHost, TickGate};
use keeper_agent::seed as seeding;
use keeper_core::agents::copy::{self, AgentCopyVm, AgentPinReq};
use keeper_core::agents::pins;
use keeper_core::agents::proxy::AgentProxies;
use keeper_core::agents::room::AgentIcons;
use keeper_core::agents::seed::{
    self, AgentSeedOfferVm, AgentSeedPlanVm, AgentSeedReq, AgentSeedResultVm,
};
use keeper_core::bots::store;
use keeper_core::platform::Platform;
use keeper_core::registry;
use keeper_core::vm::{IpcError, IpcErrorCode};
use tauri::State;

use crate::ipc::{to_ipc_error, AppState};

/// How many ticks apart the app's facts are read again: a scan reads every
/// flagged zone, `keeper.db` and the keychain, which the clock's every tick
/// does not need.
const SCAN_EVERY_TICKS: u64 = 5;

/// How long quitting waits for running turns to get their final edits.
const QUIT_TURNS: Duration = Duration::from_secs(8);

/// How long quitting waits for the host itself before it gives up on it.
const QUIT_LOCK: Duration = Duration::from_secs(2);

/// How long Settings › Agents waits for the people a pin names to be
/// named; past it they show by Matrix id.
const NAMES_WITHIN: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Runtime {
    platform: OnceLock<Arc<dyn Platform>>,
    /// The account manager's agent marks, replaced from the souls on each
    /// scan (91.1 acceptance 6).
    icons: OnceLock<Arc<AgentIcons>>,
    /// The account manager's proxies — person and allowed drives — replaced
    /// from the agents' `agent.toml`s on each scan (91.2's dock).
    proxies: OnceLock<Arc<AgentProxies>>,
    /// The host; also held across a sign-in, so no tick rebuilds it while
    /// a copy's store is open for the sign-in.
    host: tokio::sync::Mutex<Option<DesktopHost>>,
    /// A tick is running: the next one is skipped, never queued.
    running: TickGate,
    ticks: AtomicU64,
    /// What the running host last found about each copy, for the rows.
    problems: Mutex<desktop::Problems>,
}

static RUNTIME: LazyLock<Runtime> = LazyLock::new(Runtime::default);

fn refusal(message: impl Into<String>) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: message.into(),
        account_id: None,
        retriable: false,
    }
}

/// Start hosting at app start, after the sync supervisor: the first scan
/// is the next tick's.
pub fn start(platform: Arc<dyn Platform>, icons: Arc<AgentIcons>, proxies: Arc<AgentProxies>) {
    let data_dir = match platform.data_dir() {
        Ok(dir) => dir,
        Err(error) => {
            tracing::warn!(%error, "agents: no data directory; this Mac hosts no agents");
            return;
        }
    };
    let base = crate::agent_ports::task_env(Arc::clone(&platform));
    let host = DesktopHost::new(base, data_dir, env!("CARGO_PKG_VERSION"));
    let _ = RUNTIME.icons.set(icons);
    let _ = RUNTIME.proxies.set(proxies);
    if RUNTIME.platform.set(platform).is_ok() {
        if let Ok(mut slot) = RUNTIME.host.try_lock() {
            *slot = Some(host);
        }
    }
}

/// Called from the app's 1 Hz interval (AD-62): the host's tick runs off
/// the interval, and a tick still running skips this one.
pub fn tick() {
    let Some(platform) = RUNTIME.platform.get() else {
        return;
    };
    let Some(pass) = RUNTIME.running.enter() else {
        return;
    };
    let scan = RUNTIME
        .ticks
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(SCAN_EVERY_TICKS);
    let platform = Arc::clone(platform);
    tauri::async_runtime::spawn(async move {
        let _pass = pass;
        let facts = if scan {
            let scanned = tokio::task::spawn_blocking(move || {
                let facts = facts(platform.as_ref());
                let icons = desktop::agent_icons(&facts);
                let proxies = desktop::agent_proxies(&facts);
                (facts, icons, proxies)
            });
            match scanned.await {
                Ok((facts, icons, proxies)) => {
                    if let Some(marks) = RUNTIME.icons.get() {
                        marks.replace(icons);
                    }
                    if let Some(known) = RUNTIME.proxies.get() {
                        known.replace(proxies);
                    }
                    Some(facts)
                }
                Err(error) => {
                    tracing::error!(%error, "agents: this Mac's facts could not be read");
                    None
                }
            }
        } else {
            None
        };
        {
            let mut slot = RUNTIME.host.lock().await;
            if let Some(host) = slot.as_mut() {
                host.tick(facts).await;
                *RUNTIME
                    .problems
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = host.problems().clone();
            }
        }
    });
}

/// Quit, first half: running turns get their final edits, bounded. Before
/// the sync engine's own quit, so their lines are committed and pushed.
pub fn stop_turns_for_quit() {
    tauri::async_runtime::block_on(async {
        let Ok(mut slot) = tokio::time::timeout(QUIT_LOCK, RUNTIME.host.lock()).await else {
            tracing::warn!("agents: the host was busy at quit; its claims lapse");
            return;
        };
        if let Some(host) = slot.as_mut() {
            host.stop_turns(QUIT_TURNS).await;
        }
    });
}

/// Quit, second half, after the drives were pushed: the manifest is
/// withdrawn and every claim released, so another host takes over within
/// two ticks (AD-378).
pub fn release_for_quit() {
    tauri::async_runtime::block_on(async {
        let Ok(mut slot) = tokio::time::timeout(QUIT_LOCK, RUNTIME.host.lock()).await else {
            return;
        };
        if let Some(host) = slot.as_mut() {
            host.release().await;
        }
    });
}

/// The app's facts now. Blocking: `keeper.db`, the engine's profiles.
fn facts(platform: &dyn Platform) -> DesktopFacts {
    let (login, device) = crate::account_ipc::host_identity();
    let profiles = crate::sync::engine_if_open()
        .and_then(|engine| engine.list_profiles().ok())
        .unwrap_or_default();
    let data_dir = platform.data_dir().ok();
    let pins = data_dir
        .as_deref()
        .map(|dir| {
            pins::pins(dir).unwrap_or_else(|error| {
                tracing::warn!(%error, "agents: this Mac's pins could not be read; nothing is pinned");
                BTreeMap::new()
            })
        })
        .unwrap_or_default();
    let homeservers = data_dir
        .as_deref()
        .and_then(|dir| registry::list_accounts(dir).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|row| (row.user_id, row.homeserver_url))
        .collect();
    DesktopFacts {
        login,
        device,
        profiles,
        pins,
        homeservers,
    }
}

fn platform_of(state: &AppState) -> Arc<dyn Platform> {
    Arc::clone(&state.platform)
}

fn data_dir(platform: &dyn Platform) -> Result<PathBuf, IpcError> {
    platform.data_dir().map_err(to_ipc_error)
}

/// The rows for `facts`, with the people the pins name by display name
/// where the person's own Matrix account can read one.
async fn rows(state: &AppState, facts: DesktopFacts) -> Result<Vec<AgentCopyVm>, IpcError> {
    let platform = platform_of(state);
    let dir = data_dir(platform.as_ref())?;
    let problems = RUNTIME
        .problems
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let (listed, provider_rows) = {
        let platform = Arc::clone(&platform);
        let facts = facts.clone();
        let problems = problems.clone();
        tokio::task::spawn_blocking(move || {
            let rows = store::list_providers(&dir)
                .map(|l| l.rows)
                .unwrap_or_default();
            let listed = desktop::listing(
                &facts,
                &rows,
                platform.as_ref(),
                &desktop::Names::new(),
                &problems,
            );
            (listed, rows)
        })
        .await
        .map_err(|error| refusal(error.to_string()))?
    };
    let people = desktop::people(&listed);
    if people.is_empty() {
        return Ok(listed);
    }
    let names = state.accounts.display_names(&people, NAMES_WITHIN).await;
    if names.is_empty() {
        return Ok(listed);
    }
    Ok(desktop::listing(
        &facts,
        &provider_rows,
        platform.as_ref(),
        &names,
        &problems,
    ))
}

async fn facts_now(state: &AppState) -> Result<DesktopFacts, IpcError> {
    let platform = platform_of(state);
    tokio::task::spawn_blocking(move || facts(platform.as_ref()))
        .await
        .map_err(|error| refusal(error.to_string()))
}

/// Settings › Agents: one row per agent of this person's flagged drives.
#[tauri::command]
pub async fn agents_copies(state: State<'_, AppState>) -> Result<Vec<AgentCopyVm>, IpcError> {
    let facts = facts_now(&state).await?;
    rows(&state, facts).await
}

/// Sign `agent` of the flagged folder `profile_id` in on this Mac. The
/// first sign-in for a drive pins `pin`, which must be what `_drive.toml`
/// says now (what the person was shown); a drive already pinned keeps its
/// pin. The copy's session and store passphrase go to the keychain under
/// `agents/<user>/…`, reusing its device id.
#[tauri::command]
pub async fn agents_copy_sign_in(
    state: State<'_, AppState>,
    profile_id: String,
    agent: String,
    password: String,
    pin: Option<AgentPinReq>,
) -> Result<AgentCopyVm, IpcError> {
    let facts = facts_now(&state).await?;
    let login = facts
        .login
        .clone()
        .ok_or_else(|| refusal(copy::NO_ACCOUNT))?;
    let device = facts
        .device
        .clone()
        .ok_or_else(|| refusal(copy::NO_DEVICE))?;
    let pinned = facts.pins.contains_key(&profile_id);
    // The folder's files are read off the async runtime, as `rows` reads
    // them.
    let (home, new_pin) = {
        let profiles = facts.profiles.clone();
        let (profile_id, agent) = (profile_id.clone(), agent.clone());
        tokio::task::spawn_blocking(move || {
            let (profile, decl, home) =
                desktop::find_home(&profiles, &profile_id, &agent).map_err(refusal)?;
            copy::hosts_principal(&decl, &login).map_err(refusal)?;
            let new_pin = match (pinned, &pin) {
                (true, _) => None,
                (false, Some(shown)) => {
                    Some(copy::pin_from_shown(&decl, &profile.remote_url, shown).map_err(refusal)?)
                }
                (false, None) => return Err(refusal(copy::CHANGED_SINCE_SHOWN)),
            };
            Ok((home, new_pin))
        })
        .await
        .map_err(|error| refusal(error.to_string()))??
    };
    let user = home.config.matrix_user.clone();
    let homeserver = desktop::homeserver_for(&user, &facts.homeservers)
        .ok_or_else(|| refusal(desktop::no_homeserver(&user)))?;
    let platform = platform_of(&state);
    {
        // The host stops while the copy's store is open for the sign-in;
        // the next scan builds it again with the copy.
        let mut slot = RUNTIME.host.lock().await;
        if let Some(host) = slot.as_mut() {
            host.stop().await;
        }
        desktop::sign_in(
            platform.as_ref(),
            &homeserver,
            &user,
            &password,
            &format!("{}@{device}", home.config.id),
        )
        .await
        .map_err(refusal)?;
    }
    if let Some(new_pin) = new_pin {
        let dir = data_dir(platform.as_ref())?;
        let profile_id = profile_id.clone();
        tokio::task::spawn_blocking(move || pins::set_pin(&dir, &profile_id, &new_pin, now_ms()))
            .await
            .map_err(|error| refusal(error.to_string()))?
            .map_err(to_ipc_error)?;
    }
    let facts = facts_now(&state).await?;
    rows(&state, facts)
        .await?
        .into_iter()
        .find(|row| row.profile_id == profile_id && row.agent == agent)
        .ok_or_else(|| refusal(format!("{agent} is no longer in that folder.")))
}

/// *Review readers*: pin `pin` for the folder `profile_id`, but only when it
/// is what `_drive.toml` says now. Nothing re-pins without this tap.
#[tauri::command]
pub async fn agents_drive_repin(
    state: State<'_, AppState>,
    profile_id: String,
    pin: AgentPinReq,
) -> Result<Vec<AgentCopyVm>, IpcError> {
    let facts = facts_now(&state).await?;
    let login = facts
        .login
        .clone()
        .ok_or_else(|| refusal(copy::NO_ACCOUNT))?;
    let dir = data_dir(platform_of(&state).as_ref())?;
    let profiles = facts.profiles;
    // The folder's `_drive.toml` and `keeper.db` are files: read and written
    // off the async runtime, as `rows` reads them.
    tokio::task::spawn_blocking(move || {
        let (profile, decl) = desktop::find_drive(&profiles, &profile_id).map_err(refusal)?;
        copy::hosts_principal(&decl, &login).map_err(refusal)?;
        let new_pin = copy::pin_from_shown(&decl, &profile.remote_url, &pin).map_err(refusal)?;
        pins::set_pin(&dir, &profile_id, &new_pin, now_ms()).map_err(to_ipc_error)
    })
    .await
    .map_err(|error| refusal(error.to_string()))??;
    let facts = facts_now(&state).await?;
    rows(&state, facts).await
}

/// Settings › Agents › *Set up agents* (UX-DR133): every synced folder that
/// keeps agents, with what its form starts from, the catalogue, and the
/// person's own bots — none chosen for them (S-20).
#[tauri::command]
pub async fn agents_seed_offer(state: State<'_, AppState>) -> Result<AgentSeedOfferVm, IpcError> {
    let facts = facts_now(&state).await?;
    let dir = data_dir(platform_of(&state).as_ref())?;
    tokio::task::spawn_blocking(move || {
        let providers = store::list_providers(&dir)
            .map(|l| l.rows)
            .unwrap_or_default();
        let bots = store::list_bots(&dir).unwrap_or_default();
        let accounts: Vec<String> = facts
            .homeservers
            .iter()
            .map(|(user, _)| user.clone())
            .collect();
        seeding::offer(
            &facts.profiles,
            &facts.pins,
            &accounts,
            facts.login.as_deref(),
            seed::bot_choices(&providers, &bots),
        )
    })
    .await
    .map_err(|error| refusal(error.to_string()))
}

/// What `req` would write into its folder's agents zone and what it would
/// leave; refused, with its sentence, when the zone would host nothing.
#[tauri::command]
pub async fn agents_seed_plan(
    state: State<'_, AppState>,
    req: AgentSeedReq,
) -> Result<AgentSeedPlanVm, IpcError> {
    let facts = facts_now(&state).await?;
    tokio::task::spawn_blocking(move || {
        let (choices, zone) = seeding::desktop_choices(
            &facts.profiles,
            facts.pins.get(&req.profile_id),
            facts.login.as_deref(),
            &req,
        )
        .map_err(refusal)?;
        Ok(seeding::plan_vm(&seeding::plan_at(&choices, &zone)))
    })
    .await
    .map_err(|error| refusal(error.to_string()))?
}

/// Write `req`'s seed, never over a file. It writes the zone only: each
/// agent written signs in on its own Settings › Agents row.
#[tauri::command]
pub async fn agents_seed_apply(
    state: State<'_, AppState>,
    req: AgentSeedReq,
) -> Result<AgentSeedResultVm, IpcError> {
    let facts = facts_now(&state).await?;
    tokio::task::spawn_blocking(move || {
        let (choices, zone) = seeding::desktop_choices(
            &facts.profiles,
            facts.pins.get(&req.profile_id),
            facts.login.as_deref(),
            &req,
        )
        .map_err(refusal)?;
        let applied = seeding::apply(seeding::plan_at(&choices, &zone), &zone).map_err(refusal)?;
        Ok(seeding::result_vm(&req.profile_id, &choices, applied))
    })
    .await
    .map_err(|error| refusal(error.to_string()))?
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
