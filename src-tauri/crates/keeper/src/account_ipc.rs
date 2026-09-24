//! The organisation account and its config repository (Epic 82, AD-308–AD-316).
//!
//! `keeper-core` decides — the descriptor, the sign-in, the repository layout,
//! the sentence the UI shows — and `keeper-sync` moves git bytes; this module
//! is where the two meet and where the OS is touched. Nothing here chooses a
//! path to write or words a status: the plan comes from
//! `org_account::layout`, the status from `org_account::state::vm`.
//!
//! # No account, no change
//!
//! Every entry point starts from the descriptor (`~/.keeper/account.toml` on a
//! desktop with `HOME`, `<data dir>/account.toml` otherwise and on iOS). While
//! there is none, nothing here opens a connection, reads the keychain or
//! touches a layer; the Account section renders its signed-out state from the
//! same [`AccountVm`] every other state uses.
//!
//! # The order a sync runs in
//!
//! sign-in (only when the person asked) → the forge leg (`oauth` mode) →
//! fetch the clone (`data_dir/account/<id>/repo`, a blocking gix call on the
//! blocking pool) → resolve the person's directory against the sign-in's
//! `sub` → if it belongs to someone else, stop with no layers → otherwise
//! publish the create-only files for this person and device → install the two
//! account layers from the clone. At launch the last step runs first and
//! alone, from the clone already on disk and before any setting is read, so an
//! unreachable network means yesterday's settings rather than none; the
//! network half runs after, in the background.
//!
//! # No timers
//!
//! A sync runs at launch, on "Sync now", and when the window gains focus — the
//! frontend asks, and [`account_sync`] refuses to go to the network again
//! within [`SYNC_INTERVAL_MS`] unless forced. There is no interval here
//! (AD-62).

use std::collections::HashMap;
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keeper_core::config::{self as layers, AccountLayerSource};
use keeper_core::oauth::{OAuthCallback, OAuthFlowRegistry};
use keeper_core::org_account::descriptor::{self, AccountDescriptor, RepoAuthConfig, SetupInput};
use keeper_core::org_account::layout::{
    self, DeviceClass, DeviceEntry, PlanInput, RepoFiles, Resolution, UserRecord,
};
use keeper_core::org_account::session::{self, GitAuth, Identity};
use keeper_core::org_account::state::{
    self, AccountFacts, AccountIdentityVm, AccountPhase, AccountProblem, AccountSetupVm,
    AccountShareVm, AccountVm,
};
use keeper_core::org_account::{oidc, AccountError};
use keeper_core::platform::Platform;
use keeper_core::registry;
use keeper_core::vm::{IpcError, IpcErrorCode};
use keeper_sync::config_repo::{self, Author, PushResult, RepoAuth, RepoSpec, Write};
use keeper_sync::SyncError;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use crate::ipc::{to_ipc_error, AppState};

/// The event the webview opens the setup sheet on; the payload is the link.
pub const ACCOUNT_SETUP_EVENT: &str = "keeper://account-setup";

/// How long a sync that was not forced waits after the last attempt.
pub const SYNC_INTERVAL_MS: i64 = 15 * 60 * 1000;

/// Everything the Account section is drawn from, plus what a sync needs to
/// remember between runs. Never holds a token.
#[derive(Default)]
struct Inner {
    descriptor: Option<AccountDescriptor>,
    /// Sentences for a descriptor that did not load.
    faults: Vec<String>,
    /// Sentences for this person's files in the clone that keeper skips (a
    /// symlink or directory where a settings file belongs).
    repo_faults: Vec<String>,
    /// Where this run's setup link was fetched from, for "Add a device or a
    /// person". Memory only: after a relaunch the link carries the
    /// descriptor inline instead.
    source: Option<url::Url>,
    identity: Option<Identity>,
    /// The sign-in's grant is dead (a refresh was refused): the next sign-in
    /// the person asks for goes to the identity provider again.
    grant_dead: bool,
    forge_connected: bool,
    phase: AccountPhase,
    problem: Option<AccountProblem>,
    devices: Vec<DeviceEntry>,
    /// The name this install registered in the repository for this account
    /// (`account.<id>.device_slug`); `None` until a publish lands.
    this_device: Option<String>,
    /// The name chosen on the setup sheet (or carried over from another
    /// person's sign-in), used for the first registration only.
    wanted_device: Option<String>,
    last_synced_ms: Option<i64>,
    last_attempt_ms: Option<i64>,
    /// Setups resolved but not yet confirmed, by `setup_id`. Nothing is
    /// written before the person presses Continue.
    setups: HashMap<String, (AccountDescriptor, Option<url::Url>)>,
}

#[derive(Default)]
struct Runtime {
    inner: Mutex<Inner>,
    subscribers: Mutex<HashMap<String, Channel<AccountVm>>>,
    /// One sign-in or sync at a time: two fetches into one clone, or two
    /// sign-ins racing for one keychain item, would each undo the other.
    gate: tokio::sync::Mutex<()>,
    /// Ends the sign-in or sync in flight (the person pressed Cancel).
    cancel: tokio::sync::Notify,
    /// Bumped by every cancel, sign-out, forget and setup. Work that queued
    /// for the gate before the bump never starts: `Notify` only reaches work
    /// already running, and the gate is first come, first served.
    epoch: AtomicU64,
    /// Stops the blocking git transfer of the run in flight. Fresh for every
    /// run, so a new run can never un-cancel an old one's.
    interrupt: Mutex<Arc<AtomicBool>>,
    /// Held (shared) by every blocking git task; taken exclusively before the
    /// gate is let go, so nothing is still writing into the clone when the
    /// next run, a sign-out or a forget starts.
    blocking: Arc<tokio::sync::RwLock<()>>,
    /// A setup link that arrived before the webview subscribed (a link that
    /// launched keeper), delivered from [`account_subscribe`].
    pending_link: Mutex<Option<String>>,
    webview_listening: AtomicBool,
}

static RUNTIME: LazyLock<Runtime> = LazyLock::new(Runtime::default);

/// The account's HTTP client: discovery, token, UserInfo, the descriptor
/// fetch and the config repository's push. It follows no redirects — a
/// descriptor or token endpoint that redirects is refused, not followed.
static HTTP: LazyLock<Result<reqwest::Client, String>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("could not build the account's HTTP client: {error}"))
});

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The account's HTTP client, for the bots and drive credential paths too.
pub fn http() -> Result<&'static reqwest::Client, String> {
    HTTP.as_ref().map_err(Clone::clone)
}

/// The configured account, if any — for the credential paths that may use it.
pub fn descriptor() -> Option<AccountDescriptor> {
    lock(&RUNTIME.inner).descriptor.clone()
}

/// What About's destination list needs (AD-316, NFR-11).
pub fn egress_inputs() -> (Option<AccountDescriptor>, Option<url::Url>) {
    let inner = lock(&RUNTIME.inner);
    (inner.descriptor.clone(), inner.source.clone())
}

/// A valid sign-in access token for the configured account — the one API
/// every consumer of the account as a credential goes through (AD-310).
pub async fn access_token(platform: &dyn Platform) -> Result<String, AccountError> {
    let Some(d) = descriptor() else {
        return Err(AccountError::NeedsSignIn(
            "No account is set up on this device.".to_owned(),
        ));
    };
    let http = http().map_err(AccountError::Internal)?;
    oidc::access_token(platform, http, &d).await
}

/// A bot provider's bearer token: the account's access token when the
/// provider is set to "Use my account", else its keychain item as before.
///
/// The in-memory descriptor is the "an account is configured" flag: without
/// one, the keychain is read exactly as before — no registry row, no HTTP
/// client (NFR-92).
pub async fn bot_credential(
    platform: &dyn Platform,
    provider_id: &str,
    bot: Option<&str>,
) -> Result<Option<String>, AccountError> {
    let Some(account) = descriptor() else {
        return keeper_core::bots::resolve_token(platform, provider_id, bot)
            .map_err(|error| AccountError::Internal(error.to_string()));
    };
    let http = http().map_err(AccountError::Internal)?;
    keeper_core::bots::resolve_credential(platform, http, Some(&account), provider_id, bot).await
}

/// Whether a provider has a credential to send: one set to the configured
/// account always has (the account says so itself when it cannot give one);
/// any other reads its keychain item. No account, no registry read.
pub fn provider_has_credential(platform: &dyn Platform, provider_id: &str) -> bool {
    if let Some(account_id) = account_id().as_deref() {
        let uses_account = platform.data_dir().ok().and_then(|dir| {
            registry::get_bots_provider_credential_source(&dir, provider_id, Some(account_id))
                .ok()
                .flatten()
        });
        if uses_account.as_deref() == Some("account") {
            return true;
        }
    }
    keeper_core::bots::resolve_token(platform, provider_id, None)
        .ok()
        .flatten()
        .is_some()
}

// ---------------------------------------------------------------------------
// Where things live
// ---------------------------------------------------------------------------

/// `~/.keeper/account.toml` on a desktop with `HOME` (the directory a person
/// already edits by hand), else `<data dir>/account.toml` — always on iOS.
fn descriptor_path(data_dir: &Path) -> PathBuf {
    #[cfg(desktop)]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".keeper")
            .join(descriptor::FILE_NAME);
    }
    data_dir.join(descriptor::FILE_NAME)
}

/// The account's own directory in the data dir; the clone is `repo` inside.
fn account_dir(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join("account").join(id)
}

fn clone_dir(data_dir: &Path, id: &str) -> PathBuf {
    account_dir(data_dir, id).join("repo")
}

/// The config repository's working tree, read-only, as `layout` sees it.
struct WorktreeFiles<'a>(&'a Path);

impl WorktreeFiles<'_> {
    /// `rel` under the tree, or `None` for anything that could leave it: an
    /// absolute path, `..`, or a symlink out of the clone.
    fn contained(&self, rel: &str) -> Option<PathBuf> {
        let rel = Path::new(rel);
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return None;
        }
        let path = self.0.join(rel);
        match (path.canonicalize(), self.0.canonicalize()) {
            (Ok(real), Ok(root)) if real.starts_with(&root) => Some(path),
            _ => None,
        }
    }
}

impl RepoFiles for WorktreeFiles<'_> {
    fn read(&self, rel: &str) -> Option<Vec<u8>> {
        std::fs::read(self.contained(rel)?).ok()
    }

    fn list_dir(&self, rel: &str) -> Vec<String> {
        let Some(dir) = self.contained(rel) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name != ".git")
            .collect();
        names.sort();
        names
    }

    /// Whether `rel` exists as anything but a regular file — a symlink
    /// (wherever it points) or a directory. Read from the entry itself, never
    /// through the link, so layout can skip it instead of planning a file the
    /// repository would refuse on every sync.
    fn is_non_regular(&self, rel: &str) -> bool {
        if !Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        {
            return false;
        }
        // `symlink_metadata` describes the link itself, which is never a file.
        std::fs::symlink_metadata(self.0.join(rel)).is_ok_and(|meta| !meta.is_file())
    }
}

// ---------------------------------------------------------------------------
// This device (AD-314): name and class come from keeper, never a token
// ---------------------------------------------------------------------------

#[cfg(target_os = "ios")]
struct IosDevice {
    class: DeviceClass,
    model: String,
}

/// Read once on the main thread at setup: `UIDevice` is main-thread-only and
/// neither answer changes while the app runs.
#[cfg(target_os = "ios")]
static IOS_DEVICE: std::sync::OnceLock<IosDevice> = std::sync::OnceLock::new();

/// Record this phone's idiom and model. Called from `lib.rs`'s setup, which
/// runs on the UIKit main thread.
#[cfg(target_os = "ios")]
pub fn init_device() {
    use objc2::MainThreadMarker;
    use objc2_ui_kit::{UIDevice, UIUserInterfaceIdiom};

    let Some(main) = MainThreadMarker::new() else {
        tracing::warn!("account: device class read off the main thread; keeping the default");
        return;
    };
    let device = UIDevice::currentDevice(main);
    let class = if device.userInterfaceIdiom() == UIUserInterfaceIdiom::Pad {
        DeviceClass::Tablet
    } else {
        DeviceClass::Mobile
    };
    let _ = IOS_DEVICE.set(IosDevice {
        class,
        model: device.model().to_string(),
    });
}

/// `desktop` on macOS, Linux and Windows; on iOS `tablet` for an iPad idiom,
/// else `mobile`.
#[cfg(target_os = "ios")]
pub fn device_class() -> DeviceClass {
    IOS_DEVICE
        .get()
        .map_or(DeviceClass::Mobile, |device| device.class)
}

/// `desktop` on macOS, Linux and Windows.
#[cfg(not(target_os = "ios"))]
pub fn device_class() -> DeviceClass {
    DeviceClass::Desktop
}

/// The name this device goes by in `account_id`'s repository: the one it
/// registered there when it has, else [`default_device_name`].
fn device_name(data_dir: &Path, account_id: &str) -> String {
    registry::get_account_device_slug(data_dir, account_id)
        .ok()
        .flatten()
        .unwrap_or_else(default_device_name)
}

/// On a phone, where there is no host name: the model plus a four-character
/// suffix (`iphone-3f2a`).
#[cfg(target_os = "ios")]
fn default_device_name() -> String {
    let model = IOS_DEVICE
        .get()
        .map_or("iphone", |device| device.model.as_str());
    let suffix = (ulid::Ulid::new().random() & 0xffff) as u16;
    layout::device_slug(&format!("{model}-{suffix:04x}"))
}

/// The short host name, folded to a slug.
#[cfg(not(target_os = "ios"))]
fn default_device_name() -> String {
    layout::device_slug(&keeper_core::config::read_host_label())
}

// ---------------------------------------------------------------------------
// The view model and its subscribers
// ---------------------------------------------------------------------------

fn facts(inner: &Inner) -> AccountFacts {
    AccountFacts {
        descriptor: inner.descriptor.clone(),
        faults: inner
            .faults
            .iter()
            .chain(&inner.repo_faults)
            .cloned()
            .collect(),
        identity: inner.identity.as_ref().map(|identity| AccountIdentityVm {
            login: identity.login.clone(),
            display_name: identity.display_name.clone(),
            email: identity.email.clone(),
            roles: identity.roles.clone(),
        }),
        phase: inner.phase,
        problem: inner.problem.clone(),
        devices: inner.devices.clone(),
        this_device: inner.this_device.clone(),
        last_synced_ms: inner.last_synced_ms,
        now_ms: now_ms(),
        forge_connected: inner.forge_connected,
        forge_needed: inner
            .descriptor
            .as_ref()
            .is_some_and(|d| matches!(d.config.auth, RepoAuthConfig::Oauth(_))),
        offers: Default::default(),
    }
}

fn current_vm() -> AccountVm {
    state::vm(&facts(&lock(&RUNTIME.inner)))
}

fn publish(vm: &AccountVm) {
    lock(&RUNTIME.subscribers).retain(|_, channel| channel.send(vm.clone()).is_ok());
}

/// Change the state, then tell every subscriber what it now reads as.
fn update(change: impl FnOnce(&mut Inner)) -> AccountVm {
    let vm = {
        let mut inner = lock(&RUNTIME.inner);
        change(&mut inner);
        state::vm(&facts(&inner))
    };
    publish(&vm);
    vm
}

fn problem_of(error: AccountError) -> Option<AccountProblem> {
    match error {
        AccountError::Cancelled => None,
        AccountError::Unreachable(_) => Some(AccountProblem::Offline),
        AccountError::NeedsSignIn(sentence) => Some(AccountProblem::NeedsSignIn(sentence)),
        AccountError::Refused(sentence) => Some(AccountProblem::Refused(sentence)),
        AccountError::Internal(sentence) => Some(AccountProblem::Failed(sentence)),
    }
}

fn repo_problem(error: &SyncError, oauth: bool) -> Option<AccountProblem> {
    match error {
        SyncError::Cancelled => None,
        SyncError::Network { .. } => Some(AccountProblem::Offline),
        SyncError::Auth { .. } => Some(AccountProblem::NeedsSignIn(
            if oauth {
                "Reconnect the repository."
            } else {
                "Sign in again to keep your settings in sync."
            }
            .to_owned(),
        )),
        SyncError::Forbidden { host } => Some(AccountProblem::Refused(format!(
            "{host} does not let your account write to the settings repository. Ask your administrator."
        ))),
        other => Some(AccountProblem::Failed(format!(
            "The settings repository could not be updated: {other}"
        ))),
    }
}

/// The IPC envelope for an account refusal; the sentence is the core's own.
pub(crate) fn account_ipc_error(error: AccountError) -> IpcError {
    let (code, retriable) = match &error {
        AccountError::NeedsSignIn(_) => (IpcErrorCode::OauthFailed, true),
        AccountError::Unreachable(_) => (IpcErrorCode::ServerUnreachable, true),
        AccountError::Refused(_) => (IpcErrorCode::Internal, false),
        AccountError::Cancelled => (IpcErrorCode::OauthCancelled, true),
        AccountError::Internal(_) => (IpcErrorCode::Internal, false),
    };
    IpcError {
        code,
        message: error.to_string(),
        account_id: None,
        retriable,
    }
}

fn refusal(sentence: impl Into<String>) -> IpcError {
    account_ipc_error(AccountError::Refused(sentence.into()))
}

// ---------------------------------------------------------------------------
// The clone on disk → the two account layers (no network)
// ---------------------------------------------------------------------------

/// What the person's directory in the clone turned out to be.
enum Found {
    NoClone,
    Missing,
    Mine,
    NotMine,
}

/// Resolve the person's directory in the clone and install or clear the
/// account layers to match. Reads files only.
fn apply_clone(
    inner: &mut Inner,
    clone: &Path,
    d: &AccountDescriptor,
    identity: &Identity,
    device: &str,
) -> Found {
    if !clone.join(".git").exists() {
        return Found::NoClone;
    }
    let files = WorktreeFiles(clone);
    match layout::resolve(
        &files,
        &identity.login,
        &identity.sub,
        &identity.iss,
        &d.config.identity_field,
    ) {
        Resolution::NotMine { recorded } => {
            // AD-313: another person's directory is never loaded.
            layers::install_account_layers(None);
            inner.devices.clear();
            inner.repo_faults.clear();
            inner.problem = Some(AccountProblem::Blocked {
                login: identity.login.clone(),
                recorded,
            });
            Found::NotMine
        }
        Resolution::Missing => {
            layers::install_account_layers(None);
            inner.devices.clear();
            inner.repo_faults.clear();
            Found::Missing
        }
        Resolution::Mine { .. } => {
            layers::install_account_layers(Some(layers::load_account_layers(
                &AccountLayerSource {
                    dir: clone.join(&identity.login),
                    device: device.to_owned(),
                    account_name: d.name.clone(),
                    repo_host: d.repo_host(),
                },
            )));
            inner.devices = layout::devices(&files, &identity.login);
            // A symlink or directory where one of this person's files belongs
            // is skipped rather than followed; say so, as it needs fixing in
            // the repository.
            inner.repo_faults = layout::unusable_files(&files, &identity.login, device);
            if matches!(inner.problem, Some(AccountProblem::Blocked { .. })) {
                inner.problem = None;
            }
            Found::Mine
        }
    }
}

/// Read `account.toml` into the runtime, and — when someone is signed in and
/// a clone exists — install the account layers from it. No network. Returns
/// whether the descriptor changed.
fn load_local(platform: &dyn Platform, inner: &mut Inner) -> bool {
    let Ok(data_dir) = platform.data_dir() else {
        return false;
    };
    let loaded = match descriptor::load(&descriptor_path(&data_dir)) {
        Ok(loaded) => {
            inner.faults.clear();
            layers::set_account_faults(Vec::new());
            loaded
        }
        Err(fault) => {
            tracing::error!(%fault, "account.toml: skipped");
            inner.faults = vec![fault.summary()];
            layers::set_account_faults(vec![fault]);
            None
        }
    };
    let same = match (&loaded, &inner.descriptor) {
        (Some(new), Some(old)) => new == old,
        (None, None) => true,
        _ => false,
    };
    if same {
        return false;
    }
    let faults = std::mem::take(&mut inner.faults);
    let setups = std::mem::take(&mut inner.setups);
    *inner = Inner {
        faults,
        setups,
        ..Inner::default()
    };
    layers::install_account_layers(None);
    let Some(d) = loaded else {
        return true;
    };
    // A session bound to other hosts or client than this descriptor, or
    // whose stored roles no longer meet `required_role`, is refused here
    // before any layer is installed from it.
    inner.identity = match session::identity(platform, &d) {
        Ok(identity) => identity,
        Err(error) => {
            tracing::warn!(%error, "account: the stored session was not restored");
            if matches!(
                error,
                AccountError::NeedsSignIn(_) | AccountError::Refused(_)
            ) {
                inner.problem = problem_of(error);
            }
            None
        }
    };
    inner.this_device = registry::get_account_device_slug(&data_dir, &d.id)
        .ok()
        .flatten();
    inner.last_synced_ms = registry::get_account_last_synced_ms(&data_dir, &d.id)
        .ok()
        .flatten();
    inner.forge_connected = session::forge_connected(platform, &d);
    if let Some(identity) = inner.identity.clone() {
        let device = inner
            .this_device
            .clone()
            .unwrap_or_else(default_device_name);
        apply_clone(inner, &clone_dir(&data_dir, &d.id), &d, &identity, &device);
    }
    inner.descriptor = Some(d);
    true
}

/// Launch: the account layers from the clone already on disk, before the
/// first setting they could affect is read. Called straight after
/// `config::install`; no network, and nothing at all without a descriptor.
pub fn boot(platform: &dyn Platform) {
    let mut inner = lock(&RUNTIME.inner);
    load_local(platform, &mut inner);
}

/// Launch, second half: the background sync, once the app is up.
pub fn kick(app: &AppHandle) {
    use tauri::Manager;

    if descriptor().is_none() {
        return;
    }
    let state = app.state::<AppState>();
    let platform = Arc::clone(&state.platform);
    let flows = Arc::clone(&state.account_flows);
    tauri::async_runtime::spawn(async move {
        sync(platform, flows, true).await;
    });
}

// ---------------------------------------------------------------------------
// The orchestration
// ---------------------------------------------------------------------------

/// The stop count to capture before waiting for the gate; see
/// [`cancellable`].
fn epoch() -> u64 {
    RUNTIME.epoch.load(Ordering::SeqCst)
}

/// The interrupt of the run holding the gate, for its blocking git calls.
fn interrupt() -> Arc<AtomicBool> {
    Arc::clone(&lock(&RUNTIME.interrupt))
}

/// A new interrupt for a run that now holds the gate.
fn fresh_interrupt() {
    *lock(&RUNTIME.interrupt) = Arc::new(AtomicBool::new(false));
}

/// Stop everything: work still queued for the gate never starts, the run in
/// flight is dropped, its blocking git transfer is told to stop and any
/// sign-in sheet is closed.
fn interrupt_all() {
    RUNTIME.epoch.fetch_add(1, Ordering::SeqCst);
    RUNTIME.cancel.notify_waiters();
    lock(&RUNTIME.interrupt).store(true, Ordering::SeqCst);
    crate::web_auth::cancel_all();
}

/// Wait until no blocking git task is still running.
async fn blocking_idle() {
    drop(RUNTIME.blocking.write().await);
}

/// Run `work` (the caller holds the gate) until it finishes or the person
/// cancels, unless a stop came while it was queued (`queued_at` is the
/// [`epoch`] from before the gate was awaited). Returns only once the
/// blocking git work the run started has stopped, so the gate is never let
/// go while something still writes into the clone.
async fn cancellable(queued_at: u64, work: impl Future<Output = ()>) {
    let cancelled = RUNTIME.cancel.notified();
    tokio::pin!(cancelled);
    cancelled.as_mut().enable();
    // Fresh before the check: a stop after the check sets this very token.
    fresh_interrupt();
    if epoch() != queued_at {
        return;
    }
    tokio::select! {
        biased;
        () = &mut cancelled => {
            update(|inner| inner.phase = AccountPhase::Idle);
        }
        () = work => {}
    }
    blocking_idle().await;
}

fn repo_auth(auth: GitAuth) -> RepoAuth {
    match auth {
        GitAuth::None => RepoAuth::None,
        GitAuth::Basic { username, password } => RepoAuth::Basic { username, password },
        GitAuth::Bearer(token) => RepoAuth::Bearer(token),
    }
}

/// Run blocking git work on the blocking pool, counted in
/// [`Runtime::blocking`] until it returns — even when the run that started
/// it was cancelled and no longer waits for it.
async fn on_blocking_pool<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let held = Arc::clone(&RUNTIME.blocking).read_owned().await;
    tokio::task::spawn_blocking(move || {
        let _held = held;
        work()
    })
    .await
    .map_err(|error| format!("the repository task ended unexpectedly: {error}"))
}

/// Run a `keeper-sync` publish on the blocking pool: gix's transport has no
/// async path. The future is built there and driven by the runtime's own
/// handle, so only its inputs have to cross threads.
async fn off_runtime<T, F>(work: impl FnOnce() -> F + Send + 'static) -> Result<T, String>
where
    T: Send + 'static,
    F: Future<Output = T>,
{
    let handle = tokio::runtime::Handle::current();
    on_blocking_pool(move || handle.block_on(work())).await
}

/// The git credential for the config repository, per `config.auth`.
async fn repo_credential(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
) -> Result<RepoAuth, AccountError> {
    let token = match &d.config.auth {
        RepoAuthConfig::None => return Ok(RepoAuth::None),
        RepoAuthConfig::Oauth(_) => oidc::forge_token(platform, http, d).await?,
        RepoAuthConfig::Same { .. } => oidc::access_token(platform, http, d).await?,
    };
    Ok(repo_auth(session::git_credential(d, &token)))
}

/// Drop the forge connection: the next interactive sign-in runs the forge's
/// consent again instead of re-sending a token the forge has refused.
fn disconnect_forge(platform: &dyn Platform, d: &AccountDescriptor) {
    if let Err(error) = session::forget_forge(platform, &d.id) {
        tracing::warn!(%error, "account: the forge connection could not be removed");
    }
    update(|inner| inner.forge_connected = false);
}

/// Run one repository operation with `auth`. In `oauth` mode a forge that
/// refuses the token (401) gets one forced refresh and one retry with the
/// new token (which `auth` then holds); when that is refused as well, or the
/// refresh fails, the forge connection is dropped so the next interactive
/// sign-in reconnects it (spec §3.5).
async fn with_forge_retry<T, F, Fut>(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
    auth: &mut RepoAuth,
    op: F,
) -> Result<Result<T, SyncError>, String>
where
    F: Fn(RepoAuth) -> Fut,
    Fut: Future<Output = Result<Result<T, SyncError>, String>>,
{
    let first = op(auth.clone()).await;
    let refused = |outcome: &Result<Result<T, SyncError>, String>| {
        matches!(outcome, Ok(Err(SyncError::Auth { .. })))
    };
    if !matches!(d.config.auth, RepoAuthConfig::Oauth(_)) || !refused(&first) {
        return first;
    }
    let outcome = match oidc::forge_token_refreshed(platform, http, d).await {
        Ok(token) => {
            *auth = repo_auth(session::git_credential(d, &token));
            op(auth.clone()).await
        }
        Err(error) => {
            tracing::warn!(%error, "account: the forge token could not be refreshed");
            first
        }
    };
    if refused(&outcome) {
        disconnect_forge(platform, d);
    }
    outcome
}

/// A refused sign-in (a role taken away, a username keeper cannot use): the
/// account's layers stop applying until a sign-in is accepted again, and the
/// session is no longer trusted for the next background sync.
fn refuse(inner: &mut Inner) {
    layers::install_account_layers(None);
    inner.identity = None;
    inner.devices.clear();
    inner.repo_faults.clear();
}

/// Sign in (when `interactive`), connect the forge, fetch, resolve, publish
/// this person's and this device's files, and install the layers — each step
/// reported through the subscription as it starts.
async fn converge(platform: Arc<dyn Platform>, flows: Arc<OAuthFlowRegistry>, interactive: bool) {
    let Ok(data_dir) = platform.data_dir() else {
        return;
    };
    let Some(d) = descriptor() else {
        return;
    };
    let http = match http() {
        Ok(http) => http,
        Err(sentence) => {
            update(|inner| inner.problem = Some(AccountProblem::Failed(sentence)));
            return;
        }
    };
    update(|inner| inner.last_attempt_ms = Some(now_ms()));
    let oauth = matches!(d.config.auth, RepoAuthConfig::Oauth(_));

    // 1. Who is signing in. A background sync never opens a browser.
    let (known, grant_dead) = {
        let inner = lock(&RUNTIME.inner);
        (inner.identity.clone(), inner.grant_dead)
    };
    let identity = match &known {
        Some(identity) if !(interactive && grant_dead) => identity.clone(),
        _ if !interactive => return,
        _ => {
            update(|inner| {
                inner.phase = AccountPhase::SigningIn;
                inner.problem = None;
            });
            match oidc::sign_in(platform.as_ref(), &flows, http, &d).await {
                Ok(identity) => {
                    // Someone else signed in on this device: nothing of the
                    // previous person's — layers, forge connection, device
                    // registration, sync time — carries over.
                    let someone_else = known
                        .as_ref()
                        .is_some_and(|k| k.iss != identity.iss || k.sub != identity.sub);
                    if someone_else {
                        layers::install_account_layers(None);
                        if let Err(error) = registry::forget_account_state(&data_dir, &d.id) {
                            tracing::warn!(%error, "account: the previous person's state stayed");
                        }
                    }
                    // Core drops a forge item that belonged to another person.
                    let forge_connected = session::forge_connected(platform.as_ref(), &d);
                    update(|inner| {
                        if someone_else {
                            inner.devices.clear();
                            inner.repo_faults.clear();
                            inner.last_synced_ms = None;
                            // The device keeps its name, but registers it anew
                            // in the new person's directory.
                            inner.wanted_device =
                                inner.this_device.take().or(inner.wanted_device.take());
                        }
                        inner.forge_connected = forge_connected;
                        inner.identity = Some(identity.clone());
                        inner.grant_dead = false;
                    });
                    identity
                }
                Err(error) => {
                    update(|inner| {
                        if matches!(error, AccountError::Refused(_)) {
                            refuse(inner);
                        }
                        inner.phase = AccountPhase::Idle;
                        inner.problem = problem_of(error);
                    });
                    return;
                }
            }
        }
    };

    // 2. The forge leg, `oauth` mode only: its own consent, in the same sheet.
    let forge_connected = lock(&RUNTIME.inner).forge_connected;
    if oauth && !forge_connected {
        if !interactive {
            update(|inner| {
                inner.phase = AccountPhase::Idle;
                inner.problem = Some(AccountProblem::NeedsSignIn(
                    "Reconnect the repository.".to_owned(),
                ));
            });
            return;
        }
        update(|inner| inner.phase = AccountPhase::SigningIn);
        if let Err(error) =
            oidc::forge_connect(platform.as_ref(), &flows, http, &d, &identity.login).await
        {
            update(|inner| {
                inner.phase = AccountPhase::Idle;
                inner.problem = problem_of(error);
            });
            return;
        }
        update(|inner| inner.forge_connected = true);
    }

    // 3. The repository credential.
    update(|inner| inner.phase = AccountPhase::Syncing);
    let mut auth = match repo_credential(platform.as_ref(), http, &d).await {
        Ok(auth) => auth,
        Err(error) => {
            update(|inner| {
                match &error {
                    AccountError::NeedsSignIn(_) if oauth => inner.forge_connected = false,
                    AccountError::NeedsSignIn(_) => inner.grant_dead = true,
                    AccountError::Refused(_) => refuse(inner),
                    _ => {}
                }
                inner.phase = AccountPhase::Idle;
                inner.problem = problem_of(error);
            });
            return;
        }
    };

    // 4. Fetch. An unreachable repository keeps the layers already in force.
    let spec = RepoSpec {
        url: d.config.url.clone(),
        branch: d.config.branch.clone(),
        dir: clone_dir(&data_dir, &d.id),
    };
    if let Err(error) = std::fs::create_dir_all(account_dir(&data_dir, &d.id)) {
        update(|inner| {
            inner.phase = AccountPhase::Idle;
            inner.problem = Some(AccountProblem::Failed(format!(
                "keeper could not make a folder for the settings repository: {error}"
            )));
        });
        return;
    }
    let fetched = with_forge_retry(platform.as_ref(), http, &d, &mut auth, |auth| {
        let (spec, interrupt) = (spec.clone(), interrupt());
        on_blocking_pool(move || config_repo::clone_or_fetch(&spec, &auth, &interrupt))
    })
    .await;
    match fetched {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, "account: the settings repository could not be fetched");
            update(|inner| {
                inner.phase = AccountPhase::Idle;
                inner.problem = repo_problem(&error, oauth);
            });
            return;
        }
        Err(sentence) => {
            update(|inner| {
                inner.phase = AccountPhase::Idle;
                inner.problem = Some(AccountProblem::Failed(sentence));
            });
            return;
        }
    }

    // 5. Whose directory it is. Someone else's: stop, with no layers.
    //    A device not yet registered for this account takes a name no other
    //    device in the person's directory holds (two machines with one host
    //    name must not share a device record).
    let (registered, wanted) = {
        let inner = lock(&RUNTIME.inner);
        (inner.this_device.clone(), inner.wanted_device.clone())
    };
    let device = match &registered {
        Some(device) => device.clone(),
        None => {
            let device = layout::free_device_slug(
                &WorktreeFiles(&spec.dir),
                &identity.login,
                &wanted.unwrap_or_else(default_device_name),
                false,
            );
            update(|inner| inner.wanted_device = Some(device.clone()));
            device
        }
    };
    let found = {
        let mut inner = lock(&RUNTIME.inner);
        apply_clone(&mut inner, &spec.dir, &d, &identity, &device)
    };
    if matches!(found, Found::NotMine) {
        update(|inner| inner.phase = AccountPhase::Idle);
        return;
    }

    // 6. Publish what is missing for this person and this device —
    //    create-only, own directory only, planned by keeper-core against
    //    whatever tip each attempt commits on.
    let login = identity.login.clone();
    let user = UserRecord {
        login: login.clone(),
        display_name: identity.display_name.clone(),
        sub: identity.sub.clone(),
        issuer: identity.iss.clone(),
    };
    let identity_field = d.config.identity_field.clone();
    let plan_device = device.clone();
    let class = device_class();
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let plan = move |root: &Path| -> Vec<Write> {
        let files = WorktreeFiles(root);
        layout::plan(
            &files,
            &PlanInput {
                login: &login,
                user: &user,
                identity_field: &identity_field,
                device: &plan_device,
                class,
                platform: std::env::consts::OS,
                now_rfc3339: &now,
            },
        )
        .into_iter()
        .filter(|write| layout::is_own_path(&login, &write.rel))
        .map(|write| Write {
            rel: PathBuf::from(write.rel),
            bytes: write.bytes,
            replace: false,
        })
        .collect()
    };
    let author = Author {
        name: identity.display_name.clone(),
        email: identity
            .email
            .clone()
            .unwrap_or_else(|| format!("{}@keeper.invalid", identity.login)),
    };
    let message = format!("{}: register {device}", identity.login);
    let pushed = with_forge_retry(platform.as_ref(), http, &d, &mut auth, |auth| {
        let (spec, author, message, plan, interrupt) = (
            spec.clone(),
            author.clone(),
            message.clone(),
            plan.clone(),
            interrupt(),
        );
        off_runtime(move || async move {
            config_repo::commit_and_push(http, &spec, &auth, &author, &message, plan, &interrupt)
                .await
        })
    })
    .await;
    let problem = match pushed {
        Ok(Ok(PushResult::Pushed { head })) => {
            tracing::info!(%head, "account: published this device's files");
            None
        }
        Ok(Ok(PushResult::NothingToDo)) => None,
        Ok(Err(error)) => {
            tracing::warn!(%error, "account: this device's files could not be published");
            repo_problem(&error, oauth)
        }
        Err(sentence) => Some(AccountProblem::Failed(sentence)),
    };

    // 7. The layers, from the clone as it now stands. A first registration
    //    counts only once the repository holds it.
    let synced = now_ms();
    if problem.is_none() {
        if let Err(error) = registry::set_account_last_synced_ms(&data_dir, &d.id, synced) {
            tracing::warn!(%error, "account: could not record the sync time");
        }
        if registered.is_none() {
            if let Err(error) = registry::set_account_device_slug(&data_dir, &d.id, &device) {
                tracing::warn!(%error, "account: could not remember this device's name");
            }
        }
    }
    update(|inner| {
        apply_clone(inner, &spec.dir, &d, &identity, &device);
        if problem.is_none() {
            inner.last_synced_ms = Some(synced);
            if registered.is_none() {
                inner.this_device = Some(device.clone());
                inner.wanted_device = None;
            }
        }
        if !matches!(inner.problem, Some(AccountProblem::Blocked { .. })) {
            inner.problem = problem;
        }
        inner.phase = AccountPhase::Idle;
    });
}

/// One sync: at most every [`SYNC_INTERVAL_MS`] unless `force`, never while
/// another runs, never without someone signed in.
async fn sync(
    platform: Arc<dyn Platform>,
    flows: Arc<OAuthFlowRegistry>,
    force: bool,
) -> AccountVm {
    let queued_at = epoch();
    {
        let inner = lock(&RUNTIME.inner);
        let due = inner
            .last_attempt_ms
            .is_none_or(|at| now_ms().saturating_sub(at) >= SYNC_INTERVAL_MS);
        if inner.descriptor.is_none() || inner.identity.is_none() || !(force || due) {
            return state::vm(&facts(&inner));
        }
    }
    let Ok(_gate) = RUNTIME.gate.try_lock() else {
        return current_vm();
    };
    cancellable(queued_at, converge(platform, flows, false)).await;
    current_vm()
}

/// Sign in and sync, waiting for anything already running — unless a
/// cancel, sign-out or forget comes while it waits.
async fn sign_in_and_sync(platform: Arc<dyn Platform>, flows: Arc<OAuthFlowRegistry>) -> AccountVm {
    let queued_at = epoch();
    let _gate = RUNTIME.gate.lock().await;
    cancellable(queued_at, converge(platform, flows, true)).await;
    current_vm()
}

/// End anything in flight or queued, and wait until it has let go —
/// including the blocking git work a cancelled run leaves behind.
async fn stop_running() -> tokio::sync::MutexGuard<'static, ()> {
    interrupt_all();
    let gate = RUNTIME.gate.lock().await;
    blocking_idle().await;
    gate
}

// ---------------------------------------------------------------------------
// Deep links
// ---------------------------------------------------------------------------

/// `keeper://setup?…` — the one-input bootstrap (a link, or the Camera app
/// reading a QR code).
pub fn is_setup_link(url: &url::Url) -> bool {
    url.scheme() == "keeper" && url.host_str() == Some("setup")
}

/// Whether a `keeper://` deep link answers an organisation-account sign-in
/// (`keeper://oauth/<id>/…`, the descriptor's redirects and the sign-out
/// return) rather than Matrix's one fixed `keeper://oauth/callback`. The two
/// wait in separate registries, so neither's cancel can end the other's.
pub fn is_account_callback(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| {
        url.scheme() == "keeper" && url.host_str() == Some("oauth") && url.path() != "/callback"
    })
}

/// Hand a setup link to the webview, which opens the confirmation sheet. A
/// link that launched keeper arrives before anything listens, so it waits
/// for [`account_subscribe`].
pub fn setup_link_arrived(app: &AppHandle, link: &str) {
    if RUNTIME.webview_listening.load(Ordering::SeqCst) {
        if let Err(error) = app.emit(ACCOUNT_SETUP_EVENT, link) {
            tracing::warn!(%error, "account: could not hand the setup link to the window");
        }
    } else {
        *lock(&RUNTIME.pending_link) = Some(link.to_owned());
    }
}

// ---------------------------------------------------------------------------
// Drives and bot providers (AD-315)
// ---------------------------------------------------------------------------

/// The profile id in a sync credential key, `sync/<pid>/credential`.
fn credential_profile(key: &str) -> Option<&str> {
    key.strip_prefix("sync/")?.strip_suffix("/credential")
}

/// The drive credential when the drive uses the account: `None` when `key`
/// is not a drive credential or the drive keeps its own token in the
/// keychain; otherwise the account's access token, sent in keeper-sync's own
/// spelling like any stored token. A drive is only answered for the account
/// it was set to use (`account:<id>`), never for one that replaced it.
///
/// `secret_get` is synchronous and is called from the sync engine's async
/// path, so a refresh must not park a runtime worker: on a multi-thread
/// runtime `block_in_place` hands this worker's other tasks to the rest of
/// the pool first; elsewhere the refresh runs on a thread of its own.
pub fn drive_credential(platform: &Arc<dyn Platform>, key: &str) -> Option<Result<String, String>> {
    use tokio::runtime::{Handle, RuntimeFlavor};

    let profile = credential_profile(key)?;
    // No account, no change: the drive's own keychain item, read as before.
    let account_id = account_id()?;
    let data_dir = platform.data_dir().ok()?;
    let source = registry::get_sync_credential_source(&data_dir, profile, Some(&account_id))
        .ok()
        .flatten();
    if source.as_deref() != Some("account") {
        return None;
    }
    let platform = Arc::clone(platform);
    let refresh = move || {
        tauri::async_runtime::block_on(async move { access_token(platform.as_ref()).await })
    };
    let answer = match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            Ok(tokio::task::block_in_place(refresh))
        }
        // A current-thread runtime cannot lend its thread out, and blocking
        // on the app's runtime from inside it would panic.
        Ok(_) => std::thread::spawn(refresh).join(),
        Err(_) => Ok(refresh()),
    };
    Some(match answer {
        Ok(Ok(token)) => Ok(token),
        Ok(Err(error)) => Err(format!("the account credential is unavailable: {error}")),
        Err(_) => Err("the account credential could not be read".to_owned()),
    })
}

/// What the IPC answers for a stored source: core answers `account` only
/// for a row bound to the configured account.
fn credential_source_value(stored: Option<String>) -> String {
    match stored.as_deref() {
        Some("account") => "account".to_owned(),
        _ => "keychain".to_owned(),
    }
}

/// A source the frontend sent, and the account it binds to: `account`
/// needs a configured account, whose id the row then records.
fn parse_source(
    source: &str,
    account_id: Option<String>,
) -> Result<(Option<&'static str>, Option<String>), IpcError> {
    match source {
        "keychain" => Ok((None, None)),
        "account" => match account_id {
            Some(id) => Ok((Some("account"), Some(id))),
            None => Err(refusal("No account is set up on this device.")),
        },
        other => Err(refusal(format!(
            "\"{other}\" is not a credential source; use \"keychain\" or \"account\"."
        ))),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// The account as the Account section draws it. Re-reads `account.toml`,
/// so a hand edit shows the next time Settings opens (spec §2.3).
#[tauri::command]
pub async fn account_state(state: State<'_, AppState>) -> Result<AccountVm, IpcError> {
    if let Ok(_gate) = RUNTIME.gate.try_lock() {
        let changed = {
            let mut inner = lock(&RUNTIME.inner);
            load_local(state.platform.as_ref(), &mut inner)
        };
        if changed {
            let vm = current_vm();
            publish(&vm);
            return Ok(vm);
        }
    }
    Ok(current_vm())
}

/// Stream every change of [`AccountVm`]; the current one is sent at once.
/// Also delivers a setup link that launched keeper (see [`setup_link_arrived`]).
#[tauri::command]
pub fn account_subscribe(app: AppHandle, channel: Channel<AccountVm>) -> Result<String, IpcError> {
    let id = ulid::Ulid::new().to_string();
    channel
        .send(current_vm())
        .map_err(|error| refusal(format!("the account subscription could not start: {error}")))?;
    lock(&RUNTIME.subscribers).insert(id.clone(), channel);
    RUNTIME.webview_listening.store(true, Ordering::SeqCst);
    if let Some(link) = lock(&RUNTIME.pending_link).take() {
        setup_link_arrived(&app, &link);
    }
    Ok(id)
}

#[tauri::command]
pub fn account_unsubscribe(subscription_id: String) -> Result<(), IpcError> {
    lock(&RUNTIME.subscribers).remove(&subscription_id);
    Ok(())
}

/// Read a pasted link, a `keeper://setup` link or a bare `https://` URL,
/// fetch the descriptor when it is a URL, and describe it for the
/// confirmation sheet. Nothing is written.
#[tauri::command]
pub async fn account_setup_resolve(
    state: State<'_, AppState>,
    input: String,
) -> Result<AccountSetupVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let parsed = descriptor::parse_setup_input(input.trim())
        .map_err(|error| account_ipc_error(AccountError::from(error)))?;
    let (d, source) = match parsed {
        SetupInput::Url(url) => {
            let http =
                http().map_err(|sentence| account_ipc_error(AccountError::Internal(sentence)))?;
            (
                descriptor::fetch(http, &url)
                    .await
                    .map_err(account_ipc_error)?,
                Some(url),
            )
        }
        SetupInput::Inline(d) => (d, None),
    };
    // Registered means this install published a device for *this* account;
    // a forgotten account, or another one, leaves the name free to edit.
    let registered = registry::get_account_device_slug(&data_dir, &d.id)
        .ok()
        .flatten()
        .is_some();
    let setup_id = ulid::Ulid::new().to_string();
    let vm = state::setup_vm(
        setup_id.clone(),
        &d,
        descriptor().as_ref(),
        device_name(&data_dir, &d.id),
        device_class(),
        registered,
    );
    let mut inner = lock(&RUNTIME.inner);
    // One sheet at a time: a second paste replaces the first.
    inner.setups.clear();
    inner.setups.insert(setup_id, (d, source));
    Ok(vm)
}

/// The person pressed Continue: write `account.toml`, then sign in, connect
/// the forge, sync and install the layers. Progress arrives through the
/// subscription; the answer is where it ended.
#[tauri::command]
pub async fn account_setup_confirm(
    state: State<'_, AppState>,
    setup_id: String,
    device_name: String,
) -> Result<AccountVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let Some((d, source)) = lock(&RUNTIME.inner).setups.remove(&setup_id) else {
        return Err(refusal("This setup has expired. Paste the link again."));
    };
    let slug = layout::device_slug(&device_name);

    let gate = stop_running().await;
    // One account per install. The session, the drives and bot providers set
    // to use it, its per-account state and its clone belong to the
    // descriptor they were made under: a different account, or the same id
    // pointing at another identity provider, client or repository, starts
    // from none of them — its tokens must never reach the new hosts.
    let replaced = descriptor().filter(|previous| d.replaces(previous));
    if let Some(previous) = replaced {
        if let Ok(http) = http() {
            // The new sign-in's sheet opens next; a provider sign-out page
            // for the old account would only stand in its way.
            if let Err(error) = oidc::sign_out(state.platform.as_ref(), http, &previous).await {
                tracing::warn!(%error, "account: the previous account could not be signed out");
            }
        }
        forget_local_state(&data_dir, &previous.id)?;
    }
    let path = descriptor_path(&data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            refusal(format!(
                "keeper could not create {}: {error}",
                parent.display()
            ))
        })?;
    }
    descriptor::store(&path, &d).map_err(to_ipc_error)?;
    layers::install_account_layers(None);
    layers::set_account_faults(Vec::new());
    let identity = session::identity(state.platform.as_ref(), &d)
        .ok()
        .flatten();
    let forge_connected = session::forge_connected(state.platform.as_ref(), &d);
    let last_synced_ms = registry::get_account_last_synced_ms(&data_dir, &d.id)
        .ok()
        .flatten();
    // The name on the sheet is registered by the first publish that lands,
    // not here: a Continue whose sync never reached the repository must not
    // make the device look registered.
    let this_device = registry::get_account_device_slug(&data_dir, &d.id)
        .ok()
        .flatten();
    let wanted_device = this_device.is_none().then_some(slug);
    update(|inner| {
        let setups = std::mem::take(&mut inner.setups);
        *inner = Inner {
            descriptor: Some(d),
            source,
            identity,
            forge_connected,
            this_device,
            wanted_device,
            last_synced_ms,
            setups,
            ..Inner::default()
        };
        // A fresh setup always asks the identity provider, even when a
        // session for this account is still in the keychain.
        inner.grant_dead = true;
    });
    drop(gate);
    Ok(sign_in_and_sync(
        Arc::clone(&state.platform),
        Arc::clone(&state.account_flows),
    )
    .await)
}

/// Sign in again (after "Sign in again", or from the signed-out state).
#[tauri::command]
pub async fn account_sign_in(state: State<'_, AppState>) -> Result<AccountVm, IpcError> {
    if descriptor().is_none() {
        return Err(refusal("No account is set up on this device."));
    }
    update(|inner| inner.grant_dead = true);
    Ok(sign_in_and_sync(
        Arc::clone(&state.platform),
        Arc::clone(&state.account_flows),
    )
    .await)
}

/// Close the sign-in sheet and end the flow waiting on it — or the sign-in
/// still queued behind a sync, which then never starts.
#[tauri::command]
pub fn account_cancel_sign_in() -> Result<(), IpcError> {
    interrupt_all();
    Ok(())
}

/// Fetch and publish, at most every 15 minutes unless `force` ("Sync now").
#[tauri::command]
pub async fn account_sync(state: State<'_, AppState>, force: bool) -> Result<AccountVm, IpcError> {
    Ok(sync(
        Arc::clone(&state.platform),
        Arc::clone(&state.account_flows),
        force,
    )
    .await)
}

/// Rename this device: both of its files in the person's directory move in
/// one commit, and the name is remembered on this install. A device not yet
/// registered only changes the name its first registration will use.
///
/// Refused — as the command's error only, leaving the account's status as it
/// was — when the directory in the clone is not this person's, or the
/// repository will not take the move.
#[tauri::command]
pub async fn account_rename_device(
    state: State<'_, AppState>,
    name: String,
) -> Result<AccountVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let to = layout::device_slug(&name);
    let _gate = RUNTIME.gate.lock().await;
    let (d, identity, from) = {
        let inner = lock(&RUNTIME.inner);
        (
            inner.descriptor.clone(),
            inner.identity.clone(),
            inner.this_device.clone(),
        )
    };
    let Some(d) = d else {
        return Err(refusal("No account is set up on this device."));
    };
    let clone = clone_dir(&data_dir, &d.id);
    let Some(from) = from else {
        return Ok(update(|inner| {
            inner.wanted_device = Some(to.clone());
            if let Some(identity) = identity.as_ref() {
                apply_clone(inner, &clone, &d, identity, &to);
            }
        }));
    };
    if from == to {
        return Ok(current_vm());
    }
    if let (Some(identity), true) = (&identity, clone.join(".git").exists()) {
        let files = WorktreeFiles(&clone);
        let mine = matches!(
            layout::resolve(
                &files,
                &identity.login,
                &identity.sub,
                &identity.iss,
                &d.config.identity_field,
            ),
            Resolution::Mine { .. }
        );
        if !mine {
            return Err(refusal(format!(
                "This device was not renamed: {}/ in the settings repository is not yours.",
                identity.login
            )));
        }
        let ops =
            layout::plan_rename(&files, &identity.login, &from, &to).map_err(account_ipc_error)?;
        let moves: Vec<(PathBuf, PathBuf)> = ops
            .into_iter()
            .filter(|op| {
                layout::is_own_path(&identity.login, &op.from)
                    && layout::is_own_path(&identity.login, &op.to)
            })
            .map(|op| (PathBuf::from(op.from), PathBuf::from(op.to)))
            .collect();
        if !moves.is_empty() {
            let http =
                http().map_err(|sentence| account_ipc_error(AccountError::Internal(sentence)))?;
            let mut auth = repo_credential(state.platform.as_ref(), http, &d)
                .await
                .map_err(account_ipc_error)?;
            let spec = RepoSpec {
                url: d.config.url.clone(),
                branch: d.config.branch.clone(),
                dir: clone.clone(),
            };
            let author = Author {
                name: identity.display_name.clone(),
                email: identity
                    .email
                    .clone()
                    .unwrap_or_else(|| format!("{}@keeper.invalid", identity.login)),
            };
            let message = format!("{}: rename {from} to {to}", identity.login);
            fresh_interrupt();
            let moved = with_forge_retry(state.platform.as_ref(), http, &d, &mut auth, |auth| {
                let (spec, author, message, moves, interrupt) = (
                    spec.clone(),
                    author.clone(),
                    message.clone(),
                    moves.clone(),
                    interrupt(),
                );
                off_runtime(move || async move {
                    config_repo::move_and_push(
                        http, &spec, &auth, &author, &message, &moves, &interrupt,
                    )
                    .await
                })
            })
            .await
            .map_err(|sentence| account_ipc_error(AccountError::Internal(sentence)))?;
            if let Err(error) = moved {
                return Err(refusal(format!(
                    "This device could not be renamed: {error}"
                )));
            }
        }
    }
    registry::set_account_device_slug(&data_dir, &d.id, &to).map_err(to_ipc_error)?;
    Ok(update(|inner| {
        inner.this_device = Some(to.clone());
        if let Some(identity) = identity.as_ref() {
            apply_clone(inner, &clone, &d, identity, &to);
        }
    }))
}

/// The setup link for another device or another person, and its QR code.
#[tauri::command]
pub fn account_share() -> Result<AccountShareVm, IpcError> {
    let (d, source) = egress_inputs();
    let Some(d) = d else {
        return Err(refusal("No account is set up on this device."));
    };
    let link = descriptor::setup_link(&d, source.as_ref());
    let qr_svg = keeper_core::bridges::login::qr_svg(&link);
    Ok(AccountShareVm { link, qr_svg })
}

/// Show the provider's end-session page in the auth session the sign-in
/// used (`start_web_auth`: the sheet on Apple platforms, never a URL in an
/// opener's argv there). Its return, `keeper://oauth/<id>/signed-out` with
/// the request's `state`, is awaited under that state so it is consumed
/// rather than left unmatched; a return carrying any other state resolves
/// nothing.
fn present_end_session(
    platform: &dyn Platform,
    flows: Arc<OAuthFlowRegistry>,
    end: oidc::EndSession,
) {
    let returned = flows.register(end.state.clone());
    if let Err(error) = platform.start_web_auth(&end.url, &end.callback_scheme) {
        flows.remove(&end.state);
        tracing::warn!(%error, "account: could not open the provider's sign-out page");
        return;
    }
    tauri::async_runtime::spawn(async move {
        let outcome = tokio::time::timeout(Duration::from_secs(300), returned).await;
        flows.remove(&end.state);
        tracing::debug!(
            returned = matches!(outcome, Ok(Ok(OAuthCallback::Redirect(_)))),
            "account: the provider's sign-out page closed"
        );
    });
}

/// Remove what this device keeps for account `account_id`: the drives' and
/// bot providers' "use my account" rows bound to it, its per-account
/// registry state and its clone. The server's repository is untouched.
fn forget_local_state(data_dir: &Path, account_id: &str) -> Result<(), IpcError> {
    registry::clear_credential_sources(data_dir, account_id).map_err(to_ipc_error)?;
    registry::forget_account_state(data_dir, account_id).map_err(to_ipc_error)?;
    let local = account_dir(data_dir, account_id);
    match std::fs::remove_dir_all(&local) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(refusal(format!(
            "keeper could not remove {}: {error}",
            local.display()
        ))),
    }
}

/// Sign out: revoke and delete the session, stop applying the account's
/// layers, and show the provider's end-session page. The clone stays on
/// disk as the last synced settings; the repository is not touched.
#[tauri::command]
pub async fn account_sign_out(state: State<'_, AppState>) -> Result<AccountVm, IpcError> {
    let _gate = stop_running().await;
    let Some(d) = descriptor() else {
        return Ok(current_vm());
    };
    let http = http().map_err(|sentence| account_ipc_error(AccountError::Internal(sentence)))?;
    let end_session = oidc::sign_out(state.platform.as_ref(), http, &d).await;
    layers::install_account_layers(None);
    let vm = update(|inner| {
        inner.identity = None;
        inner.grant_dead = false;
        inner.forge_connected = false;
        inner.devices.clear();
        inner.repo_faults.clear();
        inner.problem = None;
        inner.phase = AccountPhase::Idle;
    });
    match end_session {
        Ok(Some(end)) => present_end_session(
            state.platform.as_ref(),
            Arc::clone(&state.account_flows),
            end,
        ),
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "account: sign-out finished with an error"),
    }
    Ok(vm)
}

/// Forget the account on this device: sign out, delete `account.toml`, the
/// local clone, the credential choices bound to it and its per-account
/// state, and clear the layers. The server's repository is untouched.
#[tauri::command]
pub async fn account_forget(state: State<'_, AppState>) -> Result<AccountVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let _gate = stop_running().await;
    if let Some(d) = descriptor() {
        if let Ok(http) = http() {
            if let Err(error) = oidc::sign_out(state.platform.as_ref(), http, &d).await {
                tracing::warn!(%error, "account: sign-out while forgetting finished with an error");
            }
        }
        forget_local_state(&data_dir, &d.id)?;
    }
    let path = descriptor_path(&data_dir);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(refusal(format!(
                "keeper could not remove {}: {error}",
                path.display()
            )))
        }
    }
    layers::install_account_layers(None);
    layers::set_account_faults(Vec::new());
    Ok(update(|inner| *inner = Inner::default()))
}

/// The configured account's id, the only one a credential choice can bind
/// to or be answered for.
fn account_id() -> Option<String> {
    lock(&RUNTIME.inner)
        .descriptor
        .as_ref()
        .map(|d| d.id.clone())
}

#[tauri::command]
pub fn sync_credential_source_get(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<String, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    registry::get_sync_credential_source(&data_dir, &profile_id, account_id().as_deref())
        .map(credential_source_value)
        .map_err(to_ipc_error)
}

#[tauri::command]
pub fn sync_credential_source_set(
    state: State<'_, AppState>,
    profile_id: String,
    source: String,
) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let (source, bound_to) = parse_source(&source, account_id())?;
    registry::set_sync_credential_source(&data_dir, &profile_id, source, bound_to.as_deref())
        .map_err(to_ipc_error)
}

#[tauri::command]
pub fn bots_provider_credential_source_get(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<String, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    registry::get_bots_provider_credential_source(&data_dir, &provider_id, account_id().as_deref())
        .map(credential_source_value)
        .map_err(to_ipc_error)
}

#[tauri::command]
pub fn bots_provider_credential_source_set(
    state: State<'_, AppState>,
    provider_id: String,
    source: String,
) -> Result<(), IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let (source, bound_to) = parse_source(&source, account_id())?;
    registry::set_bots_provider_credential_source(
        &data_dir,
        &provider_id,
        source,
        bound_to.as_deref(),
    )
    .map_err(to_ipc_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("keeper-account-ipc-{name}-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(root.join("tgorka/devices")).expect("tree");
        std::fs::write(root.join("tgorka/user.toml"), b"login = \"tgorka\"\n").expect("file");
        std::fs::create_dir_all(root.join(".git")).expect("git dir");
        root
    }

    #[test]
    fn the_worktree_reads_inside_the_clone_and_nothing_outside_it() {
        let root = tree("read");
        let files = WorktreeFiles(&root);
        assert_eq!(
            files.read("tgorka/user.toml").as_deref(),
            Some(&b"login = \"tgorka\"\n"[..])
        );
        assert_eq!(files.read("../etc/passwd"), None);
        assert_eq!(files.read("/etc/passwd"), None);
        assert_eq!(files.read("tgorka/../../x"), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_clone_is_not_followed() {
        let root = tree("link");
        let outside =
            std::env::temp_dir().join(format!("keeper-account-ipc-outside-{}", ulid::Ulid::new()));
        std::fs::write(&outside, b"secret").expect("outside");
        std::os::unix::fs::symlink(&outside, root.join("tgorka/keeper.toml")).expect("symlink");
        assert_eq!(WorktreeFiles(&root).read("tgorka/keeper.toml"), None);
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_file(&outside).ok();
    }

    /// The git directory is the transport's business, never a person or a
    /// device the layout could mistake it for.
    #[test]
    fn listing_the_root_hides_the_git_directory_and_is_sorted() {
        let root = tree("list");
        std::fs::create_dir_all(root.join("_template")).expect("dir");
        assert_eq!(
            WorktreeFiles(&root).list_dir(""),
            vec!["_template", "tgorka"]
        );
        assert!(WorktreeFiles(&root).list_dir("nobody").is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn only_a_drive_credential_key_names_a_profile() {
        assert_eq!(credential_profile("sync/p1/credential"), Some("p1"));
        assert_eq!(credential_profile("sync/p1/lfs"), None);
        assert_eq!(credential_profile("bot_provider_token/p1"), None);
    }

    #[test]
    fn a_credential_source_is_keychain_unless_it_says_account() {
        assert_eq!(credential_source_value(None), "keychain");
        assert_eq!(
            credential_source_value(Some("account".to_owned())),
            "account"
        );
        assert_eq!(credential_source_value(Some("junk".to_owned())), "keychain");
    }

    /// "Use my account" binds the row to the account configured now; with
    /// none there is nothing to bind to, and the choice is refused rather
    /// than stored unbound.
    #[test]
    fn choosing_the_account_binds_it_and_needs_one() {
        assert!(matches!(
            parse_source("keychain", Some("acme".to_owned())),
            Ok((None, None))
        ));
        assert!(matches!(
            parse_source("account", Some("acme".to_owned())),
            Ok((Some("account"), Some(id))) if id == "acme"
        ));
        assert!(parse_source("account", None).is_err());
        assert!(parse_source("Account", Some("acme".to_owned())).is_err());
    }

    #[test]
    fn only_keeper_setup_is_a_setup_link() {
        let setup = url::Url::parse("keeper://setup?d=abc").expect("url");
        let oauth = url::Url::parse("keeper://oauth/acme/callback?state=s").expect("url");
        let other = url::Url::parse("https://setup/?d=abc").expect("url");
        assert!(is_setup_link(&setup));
        assert!(!is_setup_link(&oauth));
        assert!(!is_setup_link(&other));
    }

    /// The account's callbacks and Matrix's go to separate registries, so a
    /// Matrix cancel never ends an account sign-in (and the reverse).
    #[test]
    fn account_callbacks_are_told_apart_from_matrix_ones() {
        for account in [
            "keeper://oauth/acme/callback?code=c&state=s",
            "keeper://oauth/acme/forge/callback?code=c&state=s",
            "keeper://oauth/acme/signed-out?state=s",
            "keeper://oauth/callback/callback?state=s",
        ] {
            assert!(is_account_callback(account), "{account}");
        }
        for other in [
            "keeper://oauth/callback?code=c&state=s",
            "keeper://oauth/callback",
            "keeper://setup?d=abc",
            "https://oauth/acme/callback?state=s",
            "not a url",
        ] {
            assert!(!is_account_callback(other), "{other}");
        }
    }

    /// A symlink is reported as not a regular file whether its target is
    /// inside the clone or not, so layout skips it instead of planning a file
    /// the repository would refuse forever.
    #[cfg(unix)]
    #[test]
    fn a_symlink_or_directory_is_not_a_regular_file() {
        let root = tree("nonregular");
        std::os::unix::fs::symlink(
            root.join("tgorka/user.toml"),
            root.join("tgorka/keeper.toml"),
        )
        .expect("symlink");
        let files = WorktreeFiles(&root);
        assert!(files.is_non_regular("tgorka/keeper.toml"));
        assert!(files.is_non_regular("tgorka/devices"));
        assert!(!files.is_non_regular("tgorka/user.toml"));
        assert!(!files.is_non_regular("tgorka/keeper.mac.toml"));
        assert!(!files.is_non_regular("../tgorka/keeper.toml"));
        std::fs::remove_dir_all(&root).ok();
    }
}
