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
//! # The turn is Rust's from the phrase to the last word (Epic 67, AD-205)
//!
//! Two hops of the hands-free turn used to be the webview's: sending what
//! was heard, and reading the answer aloud. On a phone whose screen is
//! locked the webview does not run, so the turn parked in `Heard` until
//! somebody opened keeper. Now [`transition`] performs [`Effect::SendText`]
//! itself — it hands the text to [`crate::bots_ipc::send_spoken`], which
//! opens the stream the way `bots_chat_send` does — and the stream's close
//! calls back into [`answer_complete`] (or [`answer_failed`]), which drives
//! the `Speak`. The webview observes the turn through the snapshots it
//! already watches and the stream events the shell forwards to it; it
//! drives nothing.
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

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use keeper_core::registry;
use keeper_core::vm::{IpcError, IpcErrorCode, VoiceStateVm, VoiceUnavailableVm, VoiceWakeVm};
use keeper_core::voice::events::VoiceEventKind;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use keeper_core::voice::EventSink;
use keeper_core::voice::{
    locale, should_rearm, silence_budget, ConsentPort, Effect, Turn, TurnEvent, TurnState,
    VoicePort, WakePhrase,
};
#[cfg(not(any(target_os = "ios", target_os = "macos")))]
use keeper_core::voice::{VoicePlatform, VoiceUnavailable};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

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
    /// Where the registry is, kept from [`boot`] for the re-arm hooks
    /// (Epic 65, AD-190): a lifecycle event and the port's own resume arrive
    /// with no `State` in hand. `None` only before boot.
    data_dir: Option<PathBuf>,
    /// The app, kept from [`boot`] so a `SendText` effect — which arrives
    /// from the port with nothing but the turn in hand — can reach the bots
    /// adapter and its store (Epic 67, AD-205). `None` only before boot.
    app: Option<AppHandle>,
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
            data_dir: None,
            app: None,
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
    /// No synthesiser, so no voice and no language to speak in.
    fn voices(&self) -> Vec<String> {
        Vec::new()
    }
    fn listening(&self) -> String {
        String::new()
    }
    fn detect_language(&self, _text: &str, _constraints: &[String]) -> Option<String> {
        None
    }
    fn speak(&self, _text: &str, _language: &str) -> Result<(), VoiceUnavailable> {
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
///
/// A [`TurnEvent::Level`] arrives at most ~25 times a second — the port's
/// `keeper_core::voice::level::Meter` is the limiter, so one spawned task
/// per reading is the whole cost here and no coalescing is needed. Every
/// other event is a transition.
#[cfg(any(target_os = "ios", target_os = "macos"))]
fn deliver(event: TurnEvent) {
    tauri::async_runtime::spawn(async move {
        match event {
            TurnEvent::Level(level) => meter(level),
            event => transition(event),
        }
    });
}

/// Record one level reading and stream the snapshot if it changed.
///
/// Not a transition: the generation is not bumped and the silence clock is
/// not touched. A level that moved would otherwise re-arm the
/// end-of-utterance pause on every reading, and a room with any noise in it
/// would never let a sentence end.
#[cfg(any(target_os = "ios", target_os = "macos"))]
fn meter(level: f32) {
    let mut voice = voice();
    let before = voice.turn.level();
    voice.turn.apply(TurnEvent::Level(level));
    if voice.turn.level() != before {
        push(&mut voice);
    }
}

/// Apply one event, stream the snapshot, and arm the silence clock.
///
/// Since Epic 67 (AD-205) this is also where a [`Effect::SendText`] is
/// carried out: the text the turn heard goes to the bots adapter on the
/// runtime, off this lock — `send_spoken` reads the store and the network,
/// and the turn's own `note_sent` will want the lock back.
fn transition(event: TurnEvent) {
    let mut voice = voice();
    let port = Arc::clone(&voice.port);
    let before = VoiceEventKind::turn(voice.turn.state());
    let logged = event.clone();
    let effects = voice.turn.drive(event, port.as_ref());
    crate::voice_log::transition(before, &logged, voice.turn.state(), &effects);
    #[cfg(target_os = "ios")]
    crate::voice_notify::answer(&effects);
    tracing::debug!(state = ?voice.turn.state(), ?effects, "voice: transition");
    after_change(&mut voice);
    let heard = effects.into_iter().find_map(|effect| match effect {
        Effect::SendText(text) => Some(text),
        _ => None,
    });
    if let Some(text) = heard {
        match voice.app.clone() {
            Some(app) => {
                drop(voice);
                tauri::async_runtime::spawn(async move {
                    crate::bots_ipc::send_spoken(&app, text).await;
                });
            }
            None => tracing::warn!("voice: heard a question before boot; nothing to send it with"),
        }
    }
}

/// The spoken turn's answer has finished arriving (Epic 67, AD-205): the
/// bots adapter calls this from the stream's clean close with the whole
/// answer, and the turn reads it aloud — the `Speak` effect, performed on
/// the port the way every other effect is. Only a turn that is waiting for
/// an answer moves; a typed conversation's close is nothing here.
pub fn answer_complete(text: String) {
    if voice().turn.awaiting_send() {
        transition(TurnEvent::AnswerDone(text));
    }
}

/// The spoken turn's stream ended without a clean answer — stopped, failed,
/// or never opened (Epic 67, AD-205). The turn ends on `reason`, the
/// sentence the surface shows beside the switch (AD-190) and the ring
/// records, and the microphone is released: a turn left in `Sending` with
/// nothing coming would hold the device open until somebody noticed. Only a
/// turn that is waiting for an answer moves.
pub fn answer_failed(reason: String) {
    if voice().turn.awaiting_send() {
        crate::voice_log::record(VoiceEventKind::Refused, Some(reason.clone()));
        transition(TurnEvent::Failed(reason));
    }
}

/// The spoken turn's stream was stopped by hand (Epic 67, AD-205): the
/// person pressed Stop on the answer, which is the question abandoned —
/// nothing is read aloud, the microphone is released and a switched-on
/// phrase is re-armed by the turn's own rule. Only a turn that is waiting
/// for an answer moves.
pub fn answer_stopped() {
    if voice().turn.awaiting_send() {
        transition(TurnEvent::Abandoned);
    }
}

/// The request for what the turn heard has left (Story 64.3, AD-186): the
/// bots adapter calls this as it spawns a turn's driver, whatever started
/// that turn. Only a turn in `Heard` moves — to `Sending` — so a typed
/// message leaving while no voice turn runs is nothing here, and nothing is
/// streamed or re-armed for it.
pub fn note_sent() {
    let awaiting = matches!(voice().turn.state(), TurnState::Heard { .. });
    if awaiting {
        transition(TurnEvent::Sent);
    }
}

/// The first token of the answer arrived (Story 64.3, AD-186): the bots
/// adapter calls this on the stream's first delta. Only a turn in `Sending`
/// that has not yet seen one moves, so a stream that is not the voice
/// turn's costs a lock and nothing else.
pub fn note_answer_chunk() {
    let thinking = matches!(
        voice().turn.state(),
        TurnState::Sending { answering: false }
    );
    if thinking {
        transition(TurnEvent::AnswerChunk);
    }
}

/// The language a send made right now is asked in, when it belongs to a
/// voice turn (Epic 64, AD-182): the listening locale in force —
/// `wake_vm`'s own expression — while `Turn::awaiting_send`, otherwise
/// `None`. The bots adapter reads it to decide whether the per-turn
/// instruction goes on the request; the rule is the turn's, read once.
pub fn spoken_turn(data_dir: &std::path::Path) -> Option<String> {
    let port = {
        let voice = voice();
        if !voice.turn.awaiting_send() {
            return None;
        }
        Arc::clone(&voice.port)
    };
    let chosen = registry::get_bots_voice_locale(data_dir)
        .map_err(|error| {
            tracing::warn!(%error, "voice: could not read bots.voice_locale for the spoken turn");
        })
        .ok()?;
    let locale::DeviceLocales { system, on_device } = port.locales();
    Some(locale::in_force(chosen.as_deref(), &system, &on_device))
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
///
/// Since Story 64.4 the pill window sees every snapshot first
/// (`voice_window::observe`) — a Rust-side fan-out rather than a second
/// registration, because the watcher slot is one deep on purpose and a
/// second `voice_watch` would evict the pane. `observe` only queues onto
/// the main thread, so it is safe under this lock. On the phone the island
/// (`voice_island::observe`, Story 65.5) and the lock-screen banner
/// (`voice_notify::observe`, Story 67.2) are the same fan-out.
fn push(voice: &mut Voice) {
    let snapshot = voice.turn.vm();
    #[cfg(desktop)]
    crate::voice_window::observe(&snapshot);
    #[cfg(target_os = "ios")]
    crate::voice_island::observe(&snapshot);
    #[cfg(target_os = "ios")]
    crate::voice_notify::observe(&snapshot);
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

/// Stop reading aloud. The turn ends as if the utterance had finished.
#[tauri::command]
pub fn voice_stop_speaking() -> Result<(), IpcError> {
    transition(TurnEvent::Silence);
    Ok(())
}

/// Once at boot from `lib.rs` — the port is process-wide and
/// `voice_availability` takes no state. Hands the persisted locale choice
/// to the port (Epic 63), so its next availability probe and request run
/// `keeper_core::voice::locale::choose` over it — [`voice_locale_set`] does
/// the same after a change — gives the turn its stop phrase (Epic 67,
/// AD-208), and keeps `data_dir` for [`voice_rearm`] and `app` for the
/// spoken send. No decision here: the port asks core which locale the
/// answer is, and `WakePhrase::parse_stop` decides what a stop phrase is.
pub fn boot(app: &AppHandle, data_dir: &Path) {
    let mut voice = voice();
    voice.data_dir = Some(data_dir.to_owned());
    voice.app = Some(app.clone());
    match registry::get_bots_voice_locale(data_dir) {
        Ok(requested) => {
            tracing::info!(?requested, "voice: locale choice loaded");
            voice.port.set_locale(requested);
        }
        Err(error) => {
            tracing::warn!(%error, "voice: could not read bots.voice_locale; choosing for the person");
            voice.port.set_locale(None);
        }
    }
    match registry::get_bots_stop_phrase(data_dir) {
        Ok(phrase) => {
            let parsed = WakePhrase::parse_stop(&phrase);
            if let Err(why) = &parsed {
                tracing::warn!(%why, phrase, "voice: the stored stop phrase is refused; no word stops an answer");
            }
            voice.turn.set_stop(parsed.ok());
        }
        Err(error) => {
            tracing::warn!(%error, "voice: could not read bots.stop_phrase; no word stops an answer");
            voice.turn.set_stop(None);
        }
    }
}

/// The wake VM as persisted plus what the port knows about locales: the
/// one in force is core's answer, the list is the port's cache. The stop
/// phrase and the voice target are read as stored.
fn wake_vm(
    data_dir: &std::path::Path,
    enabled: bool,
    phrase: String,
    port: &dyn VoicePort,
) -> Result<VoiceWakeVm, IpcError> {
    let locale_chosen = registry::get_bots_voice_locale(data_dir).map_err(to_ipc_error)?;
    let stop_phrase = registry::get_bots_stop_phrase(data_dir).map_err(to_ipc_error)?;
    let voice_target = registry::get_bots_voice_target(data_dir).map_err(to_ipc_error)?;
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
        stop_phrase,
        voice_target,
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
    match keeper_core::voice::perform(&effects, port.as_ref(), voice.turn.wake()) {
        Ok(()) => crate::voice_log::armed(voice.turn.wake()),
        Err(why) => {
            let message = why.message(&port.platform());
            crate::voice_log::record(VoiceEventKind::Refused, Some(message.clone()));
            let recovery = voice.turn.drive(TurnEvent::Failed(message), port.as_ref());
            tracing::warn!(
                ?why,
                ?recovery,
                "voice: the port refused to listen for the phrase"
            );
        }
    }
    after_change(voice);
}

/// Set the wake switch, phrase and stop phrase (FR-404, FR-405; Epic 67,
/// AD-208): validate both phrases with `keeper-core` — a refusal carries the
/// sentence saying what to type instead, and nothing is persisted — then
/// persist all three, give the turn its stop phrase and arm or disarm it.
/// Returns what was persisted, so the surface shows what will be listened
/// for.
#[tauri::command]
pub fn voice_wake_set(
    state: State<'_, AppState>,
    enabled: bool,
    phrase: String,
    stop_phrase: String,
) -> Result<VoiceWakeVm, IpcError> {
    let parsed = WakePhrase::parse(&phrase).map_err(|why| refused(why.to_string()))?;
    let stop = WakePhrase::parse_stop(&stop_phrase).map_err(|why| refused(why.to_string()))?;
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    registry::set_bots_wake_phrase(&data_dir, phrase.trim()).map_err(to_ipc_error)?;
    registry::set_bots_stop_phrase(&data_dir, stop_phrase.trim()).map_err(to_ipc_error)?;
    registry::set_bots_wake_enabled(&data_dir, enabled).map_err(to_ipc_error)?;
    let mut voice = voice();
    voice.turn.set_stop(Some(stop));
    arm(&mut voice, enabled.then_some(parsed));
    let port = Arc::clone(&voice.port);
    wake_vm(&data_dir, enabled, phrase.trim().to_owned(), port.as_ref())
}

/// Choose the bot a spoken turn goes to (Epic 67, AD-206): a pinned bot's
/// id, or `None` for "the pinned bot most recently talked to". Persisted as
/// given; which bot a turn actually reaches is
/// `keeper_core::bots::voice_target::resolve`'s answer at send time, so a
/// bot unpinned after being chosen is skipped rather than written to.
/// Returns the wake VM, whose `voice_target` is what was stored.
#[tauri::command]
pub fn voice_target_set(
    state: State<'_, AppState>,
    bot_id: Option<String>,
) -> Result<VoiceWakeVm, IpcError> {
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let chosen = bot_id.as_deref().map(str::trim).filter(|id| !id.is_empty());
    registry::set_bots_voice_target(&data_dir, chosen).map_err(to_ipc_error)?;
    let enabled = registry::get_bots_wake_enabled(&data_dir).map_err(to_ipc_error)?;
    let phrase = registry::get_bots_wake_phrase(&data_dir).map_err(to_ipc_error)?;
    let port = Arc::clone(&voice().port);
    wake_vm(&data_dir, enabled, phrase, port.as_ref())
}

/// Choose the recogniser's language (Epic 63): `None` is "choose for me".
/// Persisted as given and handed to the port, which asks
/// `keeper_core::voice::locale::choose` whether it can run here — a
/// language that cannot is recorded and refused, never silently replaced
/// by one that can, and the refusal names the ones that can. While the
/// phrase is listening, it is re-armed so the new language takes effect on
/// the next request rather than the next launch; while it was refused, a
/// language that can run here is what clears the refusal, and the phrase
/// is armed again by AD-190's rule ([`rearm_locked`]). Returns the wake VM,
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
    // Only the device held for the phrase is restarted here. Mid-turn the
    // new language reaches the request the turn's own end re-arms; a
    // `set_wake` there would touch nothing but bump the generation, and
    // orphan the silence clock of the turn in progress.
    if matches!(voice.turn.state(), TurnState::Idle) && voice.turn.armed() {
        let wake = voice.turn.wake().cloned();
        arm(&mut voice, wake);
    } else {
        rearm_locked(&mut voice, &data_dir)?;
    }
    let enabled = registry::get_bots_wake_enabled(&data_dir).map_err(to_ipc_error)?;
    let phrase = registry::get_bots_wake_phrase(&data_dir).map_err(to_ipc_error)?;
    let port = Arc::clone(&voice.port);
    wake_vm(&data_dir, enabled, phrase, port.as_ref())
}

/// Arm the phrase again from the persisted choice when AD-190's rule says
/// so (Epic 65, Story 65.2): the switch is on, nothing is listening for it,
/// and the port no longer refuses. The three facts are gathered here — the
/// registry's `bots.wake_enabled`, [`Turn::armed`], the port's availability
/// — and `keeper_core::voice::should_rearm` decides. Returns whether an arm
/// was run; a refused arm is reported through the turn as ever.
fn rearm_locked(voice: &mut Voice, data_dir: &Path) -> Result<bool, IpcError> {
    let intent = registry::get_bots_wake_enabled(data_dir).map_err(to_ipc_error)?;
    let armed = voice.turn.armed();
    let cleared = voice.port.availability().is_ok();
    if !should_rearm(intent, armed, cleared) {
        tracing::debug!(intent, armed, cleared, "voice: not re-arming");
        return Ok(false);
    }
    let phrase = registry::get_bots_wake_phrase(data_dir).map_err(to_ipc_error)?;
    let parsed = WakePhrase::parse(&phrase).map_err(|why| refused(why.to_string()))?;
    tracing::info!("voice: the refusal cleared; arming the phrase again");
    arm(voice, Some(parsed));
    Ok(true)
}

/// The re-arm entry point for what arrives with no `State` in hand: keeper
/// back in front (`lib.rs`, `RunEvent::Resumed` on the phone and the main
/// window's focus on the desktop) and the port's own resume. Never takes the
/// lock on the caller's thread — the port's worker would deadlock on its
/// own `start_listening`, and the event loop should not wait on a probe —
/// so the work is spawned onto the runtime, the way [`deliver`] does.
/// Before [`boot`] there is nothing to read, and nothing happens.
pub fn voice_rearm() {
    tauri::async_runtime::spawn(async {
        let mut voice = voice();
        let Some(data_dir) = voice.data_dir.clone() else {
            return;
        };
        if let Err(error) = rearm_locked(&mut voice, &data_dir) {
            tracing::warn!(?error, "voice: could not re-arm the phrase");
        }
    });
}

/// Ask for the recogniser and the microphone, by name, once, with the reason
/// the plist strings give — on this deliberate act and never at launch
/// (FR-408, AD-171). `None` when both are granted; otherwise why not, in the
/// sentence the surface shows with its remedy. A grant is one of the four
/// things that clear a refusal (AD-190), so it is followed by
/// [`rearm_locked`]: a phrase the person chose while the microphone was
/// not yet allowed is armed here, without the switch being touched.
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
    let mut voice = voice();
    let platform = voice.port.platform();
    if verdict.is_ok() {
        if let Some(data_dir) = voice.data_dir.clone() {
            rearm_locked(&mut voice, &data_dir)?;
        }
    }
    Ok(verdict.err().as_ref().map(|why| why.vm(&platform)))
}
