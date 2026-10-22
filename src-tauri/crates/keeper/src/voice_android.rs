//! The voice port on Android (Epic 98, story 98.4; AD-414, D-36): the
//! thinnest thing over `VoicePlugin.kt` in `gen/android` that can carry out
//! what `keeper_core::voice` decides.
//!
//! # On the device or not at all (FR-402, NFR-50)
//!
//! The Kotlin half records the microphone itself — `AudioRecord` on
//! `VOICE_COMMUNICATION`, with the device's `AcousticEchoCanceler` on it
//! where there is one — and feeds that capture to the on-device recogniser
//! (`SpeechRecognizer.createOnDeviceSpeechRecognizer`) through
//! `EXTRA_AUDIO_SOURCE`, in one segmented session. It never makes any other
//! kind of recogniser, and `keeper-core/tests/voice_on_device.rs` reads this
//! file and the Kotlin one and fails if it does or if either reaches for a
//! network API. Whether keeper can listen at all, and in which language, is
//! `keeper_core::voice::android::DeviceFacts::availability`'s answer over
//! what the plugin read off the device ([`DeviceFacts`]); there is no server
//! recogniser to fall back to.
//!
//! # Nothing here waits on the main thread
//!
//! A Kotlin command runs on Android's main thread, and a `voice_*` command
//! may arrive on it too. So the port answers every [`VoicePort`] question
//! from the facts it cached — read once when keeper starts, again when the
//! person changes the language, whenever keeper comes back in front, and
//! whenever a question finds none because a read failed — and hands every
//! action to one task that calls the plugin in order and never blocks its
//! caller. The three blocking calls are the ones made off that thread by
//! contract: [`VoicePort::availability`] (from `voice_availability`, run on
//! the blocking pool), which waits for the first facts at most
//! [`FACTS_WAIT`] — and not at all on the main thread, where a reach
//! surface may ask; and [`ConsentPort`]'s two, which `voice_authorize` runs
//! on the blocking pool for the permission dialog. Before the facts are in,
//! the answer is `VoiceUnavailable::NotReadYet` — no answer, never
//! `Unsupported` (`keeper_core::voice::android::answer`).
//!
//! # Results, sessions, segments and the last words
//!
//! The recogniser's results arrive on the plugin's channel tagged with the
//! session they belong to, named by `keeper_core::voice::android::Sessions`;
//! a result from a session the port has since rolled or stopped is dropped,
//! so a stop is never followed by words and a rolled session's open segment
//! never comes back as the person talking over the answer. The capture
//! stays up across sessions: a roll starts only a new recogniser session on
//! it. Every partial and segment result goes through
//! `keeper_core::voice::segments::Segments`, and the turn hears the joined
//! utterance — as `PartialHeard` while listening, as `SpeechDetected` (a
//! segment's end as `FinalHeard`) while an answer is read or paused.
//! [`VoicePort::finish_recognition`] closes the session's audio; the
//! recogniser's last segment and its end come back, and the port reports
//! the utterance as `FinalHeard` and starts a fresh session on the same
//! capture. A recogniser error is `keeper_core::voice::android::recovery`'s
//! to judge, with how long the session lived: start again, or stop and say
//! why.
//!
//! # Speaking
//!
//! Text-to-speech in a voice the engine has installed and that needs no
//! network (`DeviceFacts::voice_for`); a language with none is refused, so
//! the answer stays on the screen (AD-182). Each utterance read to its end
//! is `SentenceSpoken`; the last one, with nothing queued and nothing
//! paused, is `Silence`. Android's engine has no pause, so the plugin stops
//! and later reads on from the last word it reached. Before an answer is
//! read, and before a paused one goes on, the recogniser rolls to a fresh
//! session while the microphone is open, as the Apple ports start a fresh
//! request (Q11).
//!
//! # In front only
//!
//! Android gives an app with no microphone foreground service silence once
//! it is not in front, and keeper runs none. When the activity pauses the
//! port tells the turn `Failed` with `voice::android::LEFT_THE_FRONT`, which
//! releases the phrase, and until it resumes nothing is heard or read aloud
//! (`keeper_core::voice::android::act`). Whenever the facts come in with
//! keeper in front — at launch, and after every return — the switch the
//! person left on arms the phrase again through `voice_ipc::voice_rearm`,
//! whose `should_rearm` decides, as on the iPhone and the Mac (AD-168,
//! AD-190).

#![cfg(target_os = "android")]

use std::sync::{Condvar, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use keeper_core::voice::android::{
    act, answer, recovery, DeviceFacts, Recovery, Sessions, LEFT_THE_FRONT,
};
use keeper_core::voice::level::Meter;
use keeper_core::voice::locale::{self, DeviceLocales};
use keeper_core::voice::segments::Segments;
use keeper_core::voice::{
    Ask, Consent, ConsentPort, EventSink, Permission, TurnEvent, VoicePlatform, VoicePort,
    VoiceUnavailable, WakePhrase,
};
use serde_json::{json, Value};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::Wry;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

/// The longest [`VoicePort::availability`] waits for the device's facts the
/// first time: the plugin reads them when keeper starts, and the
/// text-to-speech engine takes a moment to come up.
pub const FACTS_WAIT: Duration = Duration::from_secs(5);

/// The main thread, as [`set_up`] found it: a wait there would hold up the
/// very thread the plugin answers on, so nothing waits there.
static MAIN: OnceLock<ThreadId> = OnceLock::new();

/// The registered `VoicePlugin`, set once by [`plugin`]'s setup.
static PLUGIN: OnceLock<PluginHandle<Wry>> = OnceLock::new();

/// The one task that calls the plugin, in order.
static CALLS: OnceLock<UnboundedSender<(&'static str, Value)>> = OnceLock::new();

/// The channel the plugin sends its events through, for the life of the
/// process.
static EVENTS: LazyLock<Channel<Value>> = LazyLock::new(|| {
    Channel::new(|body: InvokeResponseBody| {
        match body.deserialize::<Value>() {
            Ok(event) => crate::voice_ipc::android_port().received(&event),
            Err(error) => tracing::warn!(%error, "voice: an event keeper cannot read"),
        }
        Ok(())
    })
});

/// The Tauri plugin that registers the Kotlin `VoicePlugin` (class
/// `dev.tgorka.keeper.VoicePlugin` in the app module).
pub fn plugin() -> TauriPlugin<Wry> {
    Builder::new("keeper-voice")
        .setup(|_app, api| {
            let handle = api.register_android_plugin("dev.tgorka.keeper", "VoicePlugin")?;
            let _ = PLUGIN.set(handle);
            Ok(())
        })
        .build()
}

/// keeper is set up (`setup()`): start the task that calls the plugin, hand
/// the plugin the event channel, and read the device's facts.
pub fn set_up() {
    let _ = MAIN.set(std::thread::current().id());
    let (calls, mut queue) = unbounded_channel::<(&'static str, Value)>();
    if CALLS.set(calls).is_err() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let Some(plugin) = PLUGIN.get() else {
            tracing::warn!("voice: the voice plugin is not registered");
            return;
        };
        while let Some((command, payload)) = queue.recv().await {
            if let Err(error) = plugin
                .run_mobile_plugin_async::<Value>(command, payload)
                .await
            {
                tracing::warn!(command, %error, "voice: the plugin refused a call");
            }
        }
    });
    call("attach", json!({ "events": &*EVENTS }));
    crate::voice_ipc::android_port().refresh();
}

/// Queue one plugin call behind the ones before it.
fn call(command: &'static str, payload: Value) {
    match CALLS.get() {
        Some(calls) => {
            if calls.send((command, payload)).is_err() {
                tracing::warn!(command, "voice: the plugin's call task has stopped");
            }
        }
        None => tracing::warn!(command, "voice: a call before the voice plugin was set up"),
    }
}

/// What the port keeps between calls.
struct State {
    /// The device as the plugin last read it; `None` before the first read
    /// that succeeded.
    facts: Option<DeviceFacts>,
    /// A read of the facts is under way.
    reading: bool,
    /// keeper's activity is in front: set when the port is made, cleared by
    /// the plugin's `background`, set again by its `front`.
    in_front: bool,
    /// `voice.locale`, as [`VoicePort::set_locale`] last said.
    requested: Option<String>,
    /// Which recogniser session is heard, and whether a capture is up.
    sessions: Sessions,
    /// When the current session was started, for [`recovery`].
    session_started: Instant,
    /// The language the capture's recogniser runs in.
    locale: String,
    /// The results of the recogniser's current session.
    segments: Segments,
    /// The finish whose last words are awaited, if any.
    finishing: Option<u64>,
    /// Sessions in a row that died at once, as [`recovery`] last counted.
    errors_in_a_row: u32,
    /// An answer is being read aloud, or is paused.
    speaking: bool,
    /// The answer is paused.
    paused: bool,
    /// Utterances handed to the engine and not yet read to their end.
    queued: u32,
    /// The capture's level meter and its clock.
    meter: Meter,
    started: Instant,
}

impl State {
    /// Roll the recogniser to a fresh session on the capture that is up,
    /// queued behind what was asked before; nothing with no capture up.
    fn roll(&mut self) {
        if let Some(session) = self.sessions.roll() {
            self.session_started = Instant::now();
            call(
                "again",
                json!({ "session": session, "locale": self.locale }),
            );
        }
    }
}

/// The Android port: [`VoicePort`] and [`ConsentPort`] in one, over the
/// plugin.
pub struct AndroidVoicePort {
    sink: EventSink,
    state: Mutex<State>,
    /// Signalled when facts arrive.
    facts_read: Condvar,
}

impl AndroidVoicePort {
    /// The port, reporting through `sink`.
    pub fn new(sink: EventSink) -> Self {
        Self {
            sink,
            state: Mutex::new(State {
                facts: None,
                reading: false,
                in_front: true,
                requested: None,
                sessions: Sessions::new(),
                session_started: Instant::now(),
                locale: String::new(),
                segments: Segments::new(),
                finishing: None,
                errors_in_a_row: 0,
                speaking: false,
                paused: false,
                queued: 0,
                meter: Meter::new(),
                started: Instant::now(),
            }),
            facts_read: Condvar::new(),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read the device's facts again, in the background.
    pub fn refresh(&self) {
        self.state().reading = true;
        tauri::async_runtime::spawn(async move {
            let Some(plugin) = PLUGIN.get() else {
                crate::voice_ipc::android_port().state().reading = false;
                return;
            };
            let read = plugin
                .run_mobile_plugin_async::<DeviceFacts>("facts", json!({}))
                .await;
            let port = crate::voice_ipc::android_port();
            port.state().reading = false;
            match read {
                Ok(facts) => port.facts_in(facts),
                // Read again on the next question that finds none, on the
                // next language change and on the next return to the front.
                Err(error) => tracing::warn!(
                    error = ?error,
                    "voice: the device's facts were not read; every voice question stays unanswered until they are"
                ),
            }
        });
    }

    /// The facts are in: keep them, wake whoever waits for them, send the
    /// wake VM again — its languages and its sentence beside the switch are
    /// read off them — and, with keeper in front, let the switch the person
    /// left on arm the phrase again (`voice_rearm`; `should_rearm` decides).
    fn facts_in(&self, facts: DeviceFacts) {
        tracing::info!(
            sdk_int = facts.sdk_int,
            recogniser = facts.recogniser,
            installed = facts.installed.len(),
            echo_canceller = facts.echo_canceller,
            voices = facts.voices.len(),
            "voice: the device's facts"
        );
        let in_front = {
            let mut state = self.state();
            state.facts = Some(facts);
            state.in_front
        };
        self.facts_read.notify_all();
        tauri::async_runtime::spawn_blocking(crate::voice_ipc::announce_wake);
        if in_front {
            crate::voice_ipc::voice_rearm();
        }
    }

    /// The facts, waiting for the first read at most [`FACTS_WAIT`] — never
    /// on the main thread, where the read itself runs. With none (a read
    /// failed, or none was made), a read is started for the next question.
    fn facts_waiting(&self) -> Option<DeviceFacts> {
        let state = self.state();
        let mut state = if MAIN.get() == Some(&std::thread::current().id()) {
            state
        } else {
            self.facts_read
                .wait_timeout_while(state, FACTS_WAIT, |state| state.facts.is_none())
                .unwrap_or_else(PoisonError::into_inner)
                .0
        };
        let facts = state.facts.clone();
        if facts.is_none() && !state.reading {
            state.reading = true;
            drop(state);
            self.refresh();
        }
        facts
    }

    fn platform_of(state: &State) -> VoicePlatform {
        let class = crate::android::device_class();
        match &state.facts {
            Some(facts) => facts.platform(class),
            None => VoicePlatform::android(class, false),
        }
    }

    fn emit(&self, event: TurnEvent) {
        (self.sink)(event);
    }

    /// An event from the plugin.
    pub fn received(&self, event: &Value) {
        let text = |key: &str| event[key].as_str().unwrap_or_default().to_owned();
        let mut state = self.state();
        let current = event["session"]
            .as_u64()
            .is_some_and(|session| state.sessions.hears(session));
        match event["kind"].as_str() {
            Some("partial") if current => {
                state.errors_in_a_row = 0;
                let heard = state.segments.partial(&text("text"));
                let speaking = state.speaking;
                drop(state);
                self.heard(heard, speaking, false);
            }
            Some("segment") if current => {
                state.errors_in_a_row = 0;
                let heard = state.segments.segment(&text("text"));
                if state.finishing.is_some() {
                    return;
                }
                let speaking = state.speaking;
                if speaking {
                    state.segments = Segments::new();
                }
                drop(state);
                self.heard(heard, speaking, true);
            }
            Some("ended") if current => {
                self.session_over(state);
            }
            Some("error") if current => {
                let code = event["code"].as_i64().unwrap_or_default();
                let code = i32::try_from(code).unwrap_or(i32::MAX);
                let facts = state.facts.clone().unwrap_or_default();
                let lived = state.session_started.elapsed();
                let verdict = recovery(code, state.errors_in_a_row, lived, &state.locale, &facts);
                tracing::info!(
                    code,
                    ?lived,
                    ?verdict,
                    "voice: the recogniser ended in an error"
                );
                match verdict {
                    Recovery::Again { in_a_row } => {
                        state.errors_in_a_row = in_a_row;
                        self.session_over(state);
                    }
                    Recovery::Refuse(why) => {
                        let last = state.finishing.take().map(|_| state.segments.utterance());
                        state.sessions.close();
                        let message = why.message(&Self::platform_of(&state));
                        drop(state);
                        call("stop", json!({}));
                        if let Some(last) = last {
                            self.emit(TurnEvent::FinalHeard(last));
                        }
                        self.emit(TurnEvent::Failed(message));
                    }
                }
            }
            Some("level") if current => {
                let rms = event["rms"].as_f64().unwrap_or_default() as f32;
                let now = state.started.elapsed();
                if let Some(level) = state.meter.feed(rms, now) {
                    drop(state);
                    self.emit(TurnEvent::Level(level));
                }
            }
            Some("spoken") if state.speaking => {
                state.queued = state.queued.saturating_sub(1);
                let drained = state.queued == 0 && !state.paused;
                if drained {
                    state.speaking = false;
                }
                drop(state);
                self.emit(TurnEvent::SentenceSpoken);
                if drained {
                    self.emit(TurnEvent::Silence);
                }
            }
            Some("background") => {
                let (listening, speaking) = (state.sessions.listening(), state.speaking);
                state.in_front = false;
                state.sessions.close();
                state.speaking = false;
                state.paused = false;
                state.queued = 0;
                state.finishing = None;
                drop(state);
                if listening {
                    call("stop", json!({}));
                }
                if speaking {
                    call("stopSpeaking", json!({}));
                }
                if listening || speaking {
                    self.emit(TurnEvent::Failed(LEFT_THE_FRONT.to_owned()));
                }
            }
            Some("front") => {
                state.in_front = true;
                drop(state);
                self.refresh();
            }
            Some("partial" | "segment" | "ended" | "error" | "level" | "spoken") => {}
            other => tracing::warn!(?other, "voice: an event of a kind keeper does not know"),
        }
    }

    /// Report what the recogniser has heard so far.
    fn heard(&self, heard: String, speaking: bool, segment_closed: bool) {
        let event = match (speaking, segment_closed) {
            (true, true) => TurnEvent::FinalHeard(heard),
            (true, false) if heard.trim().is_empty() => return,
            (true, false) => TurnEvent::SpeechDetected(heard),
            (false, _) => TurnEvent::PartialHeard(heard),
        };
        self.emit(event);
    }

    /// The recogniser's session is over: deliver the last words a finish
    /// was waiting for, and start a fresh session on the same capture.
    fn session_over(&self, mut state: MutexGuard<'_, State>) {
        let last = state.finishing.take().map(|_| state.segments.utterance());
        if last.is_some() {
            state.segments = Segments::new();
        }
        state.roll();
        drop(state);
        if let Some(last) = last {
            self.emit(TurnEvent::FinalHeard(last));
        }
    }

    /// Hand `text` to the engine: now, dropping what it was reading, or
    /// after it. Nothing is read behind another app or before the facts are
    /// in (`voice::android::act`).
    fn say(&self, text: &str, language: &str, now: bool) -> Result<(), VoiceUnavailable> {
        let mut state = self.state();
        let voice = act(state.facts.as_ref(), state.in_front)?.voice_for(language)?;
        if now {
            state.queued = 1;
            state.paused = false;
            // A fresh session for what is said over this answer, so what
            // the person said before it is not heard again (R98AV1-01).
            state.segments = Segments::new();
            state.roll();
        } else {
            state.queued += 1;
        }
        state.speaking = true;
        drop(state);
        call(
            "speak",
            json!({ "text": text, "language": voice, "flush": now }),
        );
        Ok(())
    }

    /// The microphone grant, read now (blocking: off the main thread only).
    fn microphone_now(&self) -> Option<String> {
        let plugin = PLUGIN.get()?;
        let answer: Value = plugin
            .run_mobile_plugin("checkPermissions", json!({}))
            .map_err(|error| tracing::warn!(%error, "voice: the microphone grant was not read"))
            .ok()?;
        answer["microphone"].as_str().map(str::to_owned)
    }

    fn remember_microphone(&self, granted: &str) {
        if let Some(facts) = self.state().facts.as_mut() {
            granted.clone_into(&mut facts.microphone);
        }
    }
}

impl VoicePort for AndroidVoicePort {
    fn platform(&self) -> VoicePlatform {
        Self::platform_of(&self.state())
    }

    fn availability(&self) -> Result<(), VoiceUnavailable> {
        let facts = self.facts_waiting();
        let requested = self.state().requested.clone();
        answer(facts.as_ref(), requested.as_deref()).map(|_| ())
    }

    fn locales(&self) -> DeviceLocales {
        self.state()
            .facts
            .as_ref()
            .map(DeviceFacts::locales)
            .unwrap_or_default()
    }

    fn set_locale(&self, requested: Option<String>) {
        self.state().requested = requested;
        self.refresh();
    }

    fn start_listening(&self, _wake: Option<&WakePhrase>) -> Result<(), VoiceUnavailable> {
        let mut state = self.state();
        let locale =
            act(state.facts.as_ref(), state.in_front)?.availability(state.requested.as_deref())?;
        let session = state.sessions.open();
        state.session_started = Instant::now();
        state.locale = locale;
        state.segments = Segments::new();
        state.finishing = None;
        state.errors_in_a_row = 0;
        state.meter = Meter::new();
        state.started = Instant::now();
        let payload = json!({ "session": session, "locale": state.locale });
        drop(state);
        call("listen", payload);
        Ok(())
    }

    fn stop_listening(&self) {
        let mut state = self.state();
        if !state.sessions.listening() {
            return;
        }
        state.sessions.close();
        state.finishing = None;
        drop(state);
        call("stop", json!({}));
    }

    fn voices(&self) -> Vec<String> {
        self.state()
            .facts
            .as_ref()
            .map(DeviceFacts::voices)
            .unwrap_or_default()
    }

    fn listening(&self) -> String {
        let state = self.state();
        let DeviceLocales { system, on_device } = state
            .facts
            .as_ref()
            .map(DeviceFacts::locales)
            .unwrap_or_default();
        locale::in_force(state.requested.as_deref(), &system, &on_device)
    }

    /// No detector: Android's language identification is the system's
    /// text classifier, which may be a vendor service keeper cannot hold to
    /// the device, so the answer is read in the listening language — the
    /// one the model was asked to answer in (`speech::answer_instruction`).
    fn detect_language(&self, _text: &str, _constraints: &[String]) -> Option<String> {
        None
    }

    fn speak(&self, text: &str, language: &str) -> Result<(), VoiceUnavailable> {
        self.say(text, language, true)
    }

    fn enqueue(&self, text: &str, language: &str) -> Result<(), VoiceUnavailable> {
        self.say(text, language, false)
    }

    fn stop_speaking(&self) {
        let mut state = self.state();
        state.speaking = false;
        state.paused = false;
        state.queued = 0;
        drop(state);
        call("stopSpeaking", json!({}));
    }

    fn pause_speaking(&self) {
        let mut state = self.state();
        if !state.speaking {
            return;
        }
        state.paused = true;
        drop(state);
        call("pauseSpeaking", json!({}));
    }

    fn continue_speaking(&self) {
        let mut state = self.state();
        state.paused = false;
        // The words that paused the answer are settled: a fresh session,
        // so the next utterance over the answer is heard on its own (Q11).
        state.segments = Segments::new();
        state.roll();
        let nothing_left = state.speaking && state.queued == 0;
        if nothing_left {
            state.speaking = false;
        }
        drop(state);
        call("continueSpeaking", json!({}));
        if nothing_left {
            self.emit(TurnEvent::Silence);
        }
    }

    fn finish_recognition(&self, finish: u64) {
        let mut state = self.state();
        if !state.sessions.listening() {
            drop(state);
            self.emit(TurnEvent::NothingToFinish(finish));
            return;
        }
        state.finishing = Some(finish);
        let session = state.sessions.current();
        drop(state);
        call("finish", json!({ "session": session }));
        self.emit(TurnEvent::AudioEnded {
            finish,
            at_ms: crate::voice_log::now_ms(),
        });
    }
}

impl ConsentPort for AndroidVoicePort {
    /// The microphone grant as Android holds it now; there is no separate
    /// speech grant (`voice::android::DeviceFacts::consent`). Before the
    /// facts are in there is no answer yet, never "unsupported".
    fn consent(&self) -> Result<Consent, VoiceUnavailable> {
        let mut facts = self.facts_waiting().ok_or(VoiceUnavailable::NotReadYet)?;
        if let Some(granted) = self.microphone_now() {
            self.remember_microphone(&granted);
            facts.microphone = granted;
        }
        Ok(facts.consent())
    }

    /// Android's microphone dialog, waited for. Speech is never asked: it
    /// has no grant of its own here.
    fn ask(&self, ask: Ask) -> Permission {
        if ask == Ask::Speech {
            return Permission::Granted;
        }
        let Some(plugin) = PLUGIN.get() else {
            return Permission::Denied;
        };
        let answer: Value = match plugin.run_mobile_plugin(
            "requestPermissions",
            json!({ "permissions": ["microphone"] }),
        ) {
            Ok(answer) => answer,
            Err(error) => {
                tracing::warn!(%error, "voice: the microphone dialog did not answer");
                return Permission::Denied;
            }
        };
        let granted = answer["microphone"].as_str().unwrap_or_default().to_owned();
        self.remember_microphone(&granted);
        keeper_core::voice::android::permission(&granted)
    }
}
