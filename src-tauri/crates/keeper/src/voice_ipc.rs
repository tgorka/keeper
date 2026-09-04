//! The voice driving adapter (Epic 62, Story 62.4): every `voice_*` command.
//!
//! **No decisions live here.** The turn's state machine, the silence budget,
//! the phrase's normalisation and length rules, the barge-in order and the
//! re-arm rule are all [`keeper_core::voice`]. This module holds one
//! [`Turn`], one port, and one channel; a command translates its arguments
//! into a [`TurnEvent`] or a [`WakePhrase`], hands it to
//! [`Turn::drive`], and streams the resulting [`VoiceStateVm`] to the
//! webview — the way the bots surface streams its answer, with a `Channel`.
//!
//! # The one lock, and who never takes it
//!
//! [`voice()`] is a mutex over the turn and its watcher. The port's callbacks
//! — a recogniser result, a finished utterance — arrive on framework threads
//! through [`deliver`], which does **not** take the lock: it spawns the
//! transition onto the async runtime. A port method called from inside the
//! lock (`stop_listening` cancelling a task whose handler answers
//! synchronously with an error) would otherwise re-enter it.
//!
//! # The clock
//!
//! `Listening` has a silence budget ([`keeper_core::voice::silence_budget`]);
//! this module owns the timer that spends it. Every transition bumps a
//! generation and, where the new state has a budget, arms one sleep stamped
//! with that generation. A sleep that wakes to find the generation moved on
//! does nothing — the person spoke, or stopped, before it mattered.
//!
//! # Every target
//!
//! iOS has a port ([`crate::voice_ios`]) and so does macOS
//! ([`crate::voice_macos`], Story 63.4); every other target holds
//! [`AbsentPort`], whose every answer is [`VoiceUnavailable::Unsupported`],
//! so the command list is identical everywhere and a desktop that asks gets
//! a sentence rather than "command not found" (AD-27, the `sessions_ipc`
//! twin pattern with the twin folded into the port).

use std::sync::{Arc, Mutex, MutexGuard};

use keeper_core::registry;
use keeper_core::vm::{IpcError, IpcErrorCode, VoiceStateVm, VoiceUnavailableVm, VoiceWakeVm};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use keeper_core::voice::EventSink;
use keeper_core::voice::{
    locale, silence_budget, ConsentPort, Turn, TurnEvent, VoicePort, WakePhrase,
};
#[cfg(not(any(target_os = "ios", target_os = "macos")))]
use keeper_core::voice::{VoicePlatform, VoiceUnavailable};
use tauri::ipc::Channel;
use tauri::State;

use crate::ipc::{to_ipc_error, AppState};

/// The turn, its port, and the surface watching it.
struct Voice {
    turn: Turn,
    port: Arc<dyn VoicePort>,
    /// The channel `voice_watch` or `voice_start` last registered. One, not
    /// many: the surface is one control, and a second registration from a
    /// remounted pane replaces the first rather than doubling every snapshot.
    watcher: Option<Channel<VoiceStateVm>>,
    /// Which registration `watcher` is, so `voice_unwatch` from an unmount
    /// that lost the race to a remount is a no-op rather than a silencing.
    watch_serial: u64,
    /// Bumped on every transition; the silence timer checks it before firing.
    generation: u64,
}

/// The single voice state for the process.
fn voice() -> MutexGuard<'static, Voice> {
    static VOICE: std::sync::LazyLock<Mutex<Voice>> = std::sync::LazyLock::new(|| {
        let port = platform_port();
        Mutex::new(Voice {
            turn: Turn::new(port.platform()),
            port,
            watcher: None,
            watch_serial: 0,
            generation: 0,
        })
    });
    // A poisoned lock means a transition panicked. The turn is a value the
    // next event overwrites and the watcher a channel; refusing every later
    // command would be the worse failure — `bots_ipc::streams`'s reasoning.
    VOICE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The one iOS port: the same worker answers as [`VoicePort`] and as
/// [`ConsentPort`], so a permission dialog and a capture never race on two
/// threads that each own "the" audio session.
#[cfg(target_os = "ios")]
fn ios_port() -> Arc<crate::voice_ios::IosVoicePort> {
    static PORT: std::sync::LazyLock<Arc<crate::voice_ios::IosVoicePort>> =
        std::sync::LazyLock::new(|| Arc::new(crate::voice_ios::IosVoicePort::new(sink())));
    Arc::clone(&PORT)
}

/// The port for this target.
#[cfg(target_os = "ios")]
fn platform_port() -> Arc<dyn VoicePort> {
    ios_port()
}

/// The consent half for this target (FR-408).
#[cfg(target_os = "ios")]
fn platform_consent() -> Arc<dyn ConsentPort> {
    ios_port()
}

/// The one macOS port (Story 63.4): the same worker answers as [`VoicePort`]
/// and as [`ConsentPort`], for the same reason as on iOS — a permission
/// dialog and a capture never race on two threads.
#[cfg(target_os = "macos")]
fn macos_port() -> Arc<crate::voice_macos::MacVoicePort> {
    static PORT: std::sync::LazyLock<Arc<crate::voice_macos::MacVoicePort>> =
        std::sync::LazyLock::new(|| Arc::new(crate::voice_macos::MacVoicePort::new(sink())));
    Arc::clone(&PORT)
}

/// The port for this target.
#[cfg(target_os = "macos")]
fn platform_port() -> Arc<dyn VoicePort> {
    macos_port()
}

/// The consent half for this target (FR-408).
#[cfg(target_os = "macos")]
fn platform_consent() -> Arc<dyn ConsentPort> {
    macos_port()
}

/// The port for this target.
#[cfg(not(any(target_os = "ios", target_os = "macos")))]
fn platform_port() -> Arc<dyn VoicePort> {
    Arc::new(AbsentPort)
}

/// The consent half for this target: nothing to ask for.
#[cfg(not(any(target_os = "ios", target_os = "macos")))]
fn platform_consent() -> Arc<dyn ConsentPort> {
    Arc::new(AbsentPort)
}

/// The port every target without one holds: honest, non-panicking,
/// unsupported.
#[cfg(not(any(target_os = "ios", target_os = "macos")))]
struct AbsentPort;

#[cfg(not(any(target_os = "ios", target_os = "macos")))]
impl VoicePort for AbsentPort {
    /// No platform to name: this port covers every target without one, and
    /// its one sentence names no device.
    fn platform(&self) -> VoicePlatform {
        VoicePlatform::ABSENT
    }
    fn availability(&self) -> Result<(), VoiceUnavailable> {
        Err(VoiceUnavailable::Unsupported)
    }
    /// No recogniser, so no locale runs and no system locale is worth
    /// naming: the surface this feeds is absent on this target (AD-179).
    fn locales(&self) -> locale::DeviceLocales {
        locale::DeviceLocales::default()
    }
    fn set_locale(&self, _requested: Option<String>) {}
    fn start_listening(&self, _wake: Option<&WakePhrase>) -> Result<(), VoiceUnavailable> {
        Err(VoiceUnavailable::Unsupported)
    }
    fn stop_listening(&self) {}
    fn speak(&self, _text: &str) -> Result<(), VoiceUnavailable> {
        Err(VoiceUnavailable::Unsupported)
    }
    fn stop_speaking(&self) {}
}

#[cfg(not(any(target_os = "ios", target_os = "macos")))]
impl ConsentPort for AbsentPort {
    fn consent(&self) -> Result<keeper_core::voice::Consent, VoiceUnavailable> {
        Err(VoiceUnavailable::Unsupported)
    }
    fn ask(&self, _ask: keeper_core::voice::Ask) -> keeper_core::voice::Permission {
        keeper_core::voice::Permission::Denied
    }
}

/// Where the port delivers what it heard: off the framework thread, onto the
/// runtime, into [`transition`].
#[cfg(any(target_os = "ios", target_os = "macos"))]
fn sink() -> EventSink {
    Arc::new(deliver)
}

/// Hand a port event to the turn without taking the lock on the caller's
/// thread.
#[cfg(any(target_os = "ios", target_os = "macos"))]
fn deliver(event: TurnEvent) {
    tauri::async_runtime::spawn(async move {
        transition(event);
    });
}

/// Apply one event, stream the snapshot, and arm the silence clock.
fn transition(event: TurnEvent) {
    let mut voice = voice();
    let port = Arc::clone(&voice.port);
    let effects = voice.turn.drive(event, port.as_ref());
    tracing::debug!(state = ?voice.turn.state(), ?effects, "voice: transition");
    after_change(&mut voice);
}

/// What every change does once the turn has moved: bump the generation,
/// push the snapshot, and spend the new state's silence budget if it has one.
fn after_change(state: &mut Voice) {
    state.generation = state.generation.wrapping_add(1);
    push(state);
    if let Some(budget) = silence_budget(state.turn.state()) {
        let generation = state.generation;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(budget).await;
            let stale = voice().generation != generation;
            if !stale {
                transition(TurnEvent::Silence);
            }
        });
    }
}

/// Stream the current snapshot to the watcher, dropping a watcher whose
/// webview has gone.
fn push(voice: &mut Voice) {
    let snapshot = voice.turn.vm();
    if let Some(channel) = &voice.watcher {
        if channel.send(snapshot).is_err() {
            voice.watcher = None;
        }
    }
}

/// The turn's current snapshot, for a surface that lives in Rust — the tray
/// item's tick (Story 63.5) reads it the way `ipc::recording_snapshot` is
/// read. No decision here; the same projection the watcher streams.
pub fn voice_snapshot() -> VoiceStateVm {
    voice().turn.vm()
}

/// A phrase refusal is the person's input, so it says what to type instead
/// and is `internal` only in the sense the taxonomy has no better word — the
/// message is the point.
fn refused(message: String) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message,
        account_id: None,
        retriable: false,
    }
}

/// Whether voice can work right now: `None` when it can, otherwise why, in
/// the sentence the surface shows (FR-402).
///
/// The verdict is logged: it is asked once per surface mount, and it is
/// the one line that tells a log reader whether voice is alive on this
/// phone — a build that shipped without its recogniser was once invisible
/// precisely because nothing wrote this down.
#[tauri::command]
pub fn voice_availability() -> Result<Option<VoiceUnavailableVm>, IpcError> {
    let port = Arc::clone(&voice().port);
    let refusal = port.availability().err();
    match &refusal {
        Some(why) => tracing::info!(?why, "voice: unavailable"),
        None => tracing::info!("voice: available"),
    }
    Ok(refusal.as_ref().map(|why| why.vm(&port.platform())))
}

/// Register `channel` as the watcher and return its id.
fn watch(voice: &mut Voice, channel: Channel<VoiceStateVm>) -> u64 {
    voice.watch_serial = voice.watch_serial.wrapping_add(1);
    voice.watcher = Some(channel);
    push(voice);
    voice.watch_serial
}

/// Watch the turn without starting one: `channel` receives the current
/// snapshot at once and one after every transition (FR-405 — the surface
/// shows listening whenever the device is open, including for the phrase).
/// Returns an id for [`voice_unwatch`].
#[tauri::command]
pub fn voice_watch(channel: Channel<VoiceStateVm>) -> Result<u64, IpcError> {
    Ok(watch(&mut voice(), channel))
}

/// Stop watching. An id that is not the current watcher's — the pane was
/// remounted before its unmount resolved — is a no-op; the turn itself is
/// untouched, because a surface going away does not disarm the phrase.
#[tauri::command]
pub fn voice_unwatch(id: u64) -> Result<(), IpcError> {
    let mut voice = voice();
    if voice.watch_serial == id {
        voice.watcher = None;
    }
    Ok(())
}

/// Start a turn by hand (FR-401, FR-407). Snapshots go to the watcher
/// [`voice_watch`] registered — the surface keeps one stream, and a start
/// that replaced it would leave the wake chip on a dead channel and the
/// pane's unwatch a no-op against a serial it never saw.
#[tauri::command]
pub fn voice_start() -> Result<(), IpcError> {
    transition(TurnEvent::WakeMatched);
    Ok(())
}

/// Abandon the turn, whatever state it is in; the microphone is released
/// (NFR-51).
#[tauri::command]
pub fn voice_stop() -> Result<(), IpcError> {
    transition(TurnEvent::Abandoned);
    Ok(())
}

/// Read an answer aloud (FR-403). From a turn that heard something, this is
/// the answer to what was heard; from idle, it is the answer to something
/// typed.
#[tauri::command]
pub fn voice_speak(text: String) -> Result<(), IpcError> {
    transition(TurnEvent::AnswerDone(text));
    Ok(())
}

/// Stop reading aloud. The turn ends as if the utterance had finished.
#[tauri::command]
pub fn voice_stop_speaking() -> Result<(), IpcError> {
    transition(TurnEvent::Silence);
    Ok(())
}

/// Hand the persisted locale choice to the port (Epic 63), so its next
/// availability probe and request run `keeper_core::voice::locale::choose`
/// over it. Called once at boot from `lib.rs` — the port is process-wide
/// and `voice_availability` takes no state — and again by
/// [`voice_locale_set`] after a change. No decision here: the port asks
/// core which locale the answer is.
pub fn load_locale(data_dir: &std::path::Path) {
    match registry::get_bots_voice_locale(data_dir) {
        Ok(requested) => {
            tracing::info!(?requested, "voice: locale choice loaded");
            voice().port.set_locale(requested);
        }
        Err(error) => {
            tracing::warn!(%error, "voice: could not read bots.voice_locale; choosing for the person");
            voice().port.set_locale(None);
        }
    }
}

/// The wake VM as persisted plus what the port knows about locales: the
/// one in force is core's answer, the list is the port's cache.
fn wake_vm(
    data_dir: &std::path::Path,
    enabled: bool,
    phrase: String,
    port: &dyn VoicePort,
) -> Result<VoiceWakeVm, IpcError> {
    let locale_chosen = registry::get_bots_voice_locale(data_dir).map_err(to_ipc_error)?;
    let locale::DeviceLocales { system, on_device } = port.locales();
    Ok(VoiceWakeVm {
        enabled,
        phrase,
        // The port's own platform, not one const for every target: a Mac was
        // showing iOS's sentence under its switch (screenshot, 2026-09-04).
        limits: port.platform().limits.to_owned(),
        locale: locale::in_force(locale_chosen.as_deref(), &system, &on_device),
        locale_chosen,
        on_device_locales: on_device,
    })
}

/// The wake switch and phrase as persisted (FR-404, FR-405), with the
/// sentence about what listening costs (FR-406) and the recogniser's
/// language (Epic 63). Reads only: whether the device is open is the
/// turn's, streamed over the watcher.
#[tauri::command]
pub fn voice_wake_get(state: State<'_, AppState>) -> Result<VoiceWakeVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let enabled = registry::get_bots_wake_enabled(&data_dir).map_err(to_ipc_error)?;
    let phrase = registry::get_bots_wake_phrase(&data_dir).map_err(to_ipc_error)?;
    let port = Arc::clone(&voice().port);
    wake_vm(&data_dir, enabled, phrase, port.as_ref())
}

/// Arm or disarm the turn for `wake` and carry the effects out on the port.
/// A port that refuses to open for the phrase is reported through the turn,
/// which releases whatever was half-opened; the choice itself stays
/// recorded.
fn arm(voice: &mut Voice, wake: Option<WakePhrase>) {
    let effects = voice.turn.set_wake(wake);
    let port = Arc::clone(&voice.port);
    if let Err(why) = keeper_core::voice::perform(&effects, port.as_ref(), voice.turn.wake()) {
        let recovery = voice.turn.drive(
            TurnEvent::Failed(why.message(&port.platform())),
            port.as_ref(),
        );
        tracing::warn!(
            ?why,
            ?recovery,
            "voice: the port refused to listen for the phrase"
        );
    }
    after_change(voice);
}

/// Set the wake switch and phrase (FR-404, FR-405): validate the phrase with
/// `keeper-core` — a refusal carries the sentence saying what to type instead,
/// and nothing is persisted — then persist both and arm or disarm the turn.
/// Returns what was persisted, so the surface shows what will be listened for.
#[tauri::command]
pub fn voice_wake_set(
    state: State<'_, AppState>,
    enabled: bool,
    phrase: String,
) -> Result<VoiceWakeVm, IpcError> {
    let parsed = WakePhrase::parse(&phrase).map_err(|why| refused(why.to_string()))?;
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    registry::set_bots_wake_phrase(&data_dir, phrase.trim()).map_err(to_ipc_error)?;
    registry::set_bots_wake_enabled(&data_dir, enabled).map_err(to_ipc_error)?;
    let mut voice = voice();
    arm(&mut voice, enabled.then_some(parsed));
    let port = Arc::clone(&voice.port);
    wake_vm(&data_dir, enabled, phrase.trim().to_owned(), port.as_ref())
}

/// Choose the recogniser's language (Epic 63): `None` is "choose for me".
/// Persisted as given and handed to the port, which asks
/// `keeper_core::voice::locale::choose` whether it can run here — a
/// language that cannot is recorded and refused, never silently replaced
/// by one that can, and the refusal names the ones that can. While the
/// phrase is armed, listening is re-armed so the new language takes effect
/// on the next request rather than the next launch. Returns the wake VM,
/// whose `locale` is the one now in force.
#[tauri::command]
pub fn voice_locale_set(
    state: State<'_, AppState>,
    locale: Option<String>,
) -> Result<VoiceWakeVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let requested = locale
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned);
    registry::set_bots_voice_locale(&data_dir, requested.as_deref()).map_err(to_ipc_error)?;
    let mut voice = voice();
    voice.port.set_locale(requested);
    let armed = voice.turn.wake().cloned();
    if armed.is_some() {
        arm(&mut voice, armed);
    }
    let enabled = registry::get_bots_wake_enabled(&data_dir).map_err(to_ipc_error)?;
    let phrase = registry::get_bots_wake_phrase(&data_dir).map_err(to_ipc_error)?;
    let port = Arc::clone(&voice.port);
    wake_vm(&data_dir, enabled, phrase, port.as_ref())
}

/// Ask for the recogniser and the microphone, by name, once, with the reason
/// the plist strings give — on this deliberate act and never at launch
/// (FR-408, AD-171). `None` when both are granted; otherwise why not, in the
/// sentence the surface shows with its remedy.
///
/// `async` on purpose: a sync command runs on the main thread, and the OS
/// draws its permission dialog there — the first-boot hang `lib.rs` records
/// for the notification prompt. `spawn_blocking` keeps the wait off it while
/// the port's worker blocks on the answer. When to ask, and in which order,
/// is `keeper_core::voice::authorization`; this is the call site.
#[tauri::command]
pub async fn voice_authorize() -> Result<Option<VoiceUnavailableVm>, IpcError> {
    let consent = platform_consent();
    let verdict = tauri::async_runtime::spawn_blocking(move || {
        keeper_core::voice::authorize(consent.as_ref())
    })
    .await
    .map_err(|error| IpcError {
        code: IpcErrorCode::Internal,
        message: format!("the permission request did not finish: {error}"),
        account_id: None,
        retriable: true,
    })?;
    let platform = voice().port.platform();
    Ok(verdict.err().as_ref().map(|why| why.vm(&platform)))
}
