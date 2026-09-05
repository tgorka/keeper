//! The voice port on iOS (Epic 62, Stories 62.4 and 62.6, AD-165–AD-167,
//! AD-171): the thinnest thing over `SFSpeechRecognizer`, `AVAudioEngine`,
//! `AVAudioSession` and `AVSpeechSynthesizer` that can carry out what
//! `keeper_core::voice` decides.
//!
//! # On the device or not at all (FR-402, NFR-50)
//!
//! Every recognition request sets `requiresOnDeviceRecognition = true`, and
//! [`Worker::availability`] refuses before any request is built when the
//! chosen locale's recogniser cannot work on this phone. There is no server
//! path in this file: no request without the flag, no retry without it, no
//! fallback. `docs/egress.md` names every destination keeper contacts, and
//! Apple's speech servers are not on it.
//!
//! # Which language (Epic 63)
//!
//! The same shape as `voice_macos.rs`: the port enumerates
//! `supportedLocales()` once, classifies each by
//! `supportsOnDeviceRecognition` (cached — it costs a recogniser per
//! locale — and refreshed when the person changes the choice), and hands
//! that list, the system locale and `bots.voice_locale` (via
//! [`VoicePort::set_locale`]) to `keeper_core::voice::locale::choose`, which
//! answers the locale to build the recogniser for or the refusal to show.
//! A Polish-language phone whose only on-device models are English is not
//! dead: the person is told which languages run here and can choose one.
//!
//! # One thread owns the frameworks
//!
//! The objc2 bindings for these classes are not `Send`, and the port must be
//! (`VoicePort: Send + Sync`, held in an `Arc` by the command layer). So the
//! port is a channel to one worker thread that owns every framework object,
//! and each trait method is a message. Results come back through the
//! [`EventSink`] the port was built with; that sink never touches this
//! thread. The alternative — `unsafe impl Send` on a wrapper — would be a
//! second kind of `unsafe` with a weaker story than "one thread, no
//! sharing", so it is not here.
//!
//! # Listening that outlives a request (Story 62.6, the driving brief)
//!
//! The phrase is armed for as long as a drive, and one `SFSpeechRecognizer`
//! request is not built for that: a task ends on its own after a final
//! result, and a request left running for a long time churns errors. So the
//! **capture** — the audio session, the engine, the tap — is one long-lived
//! thing, and the **request** is rolled underneath it: the tap appends to
//! whichever request is in [`RequestSlot`] right now, and the worker swaps
//! in a fresh one after [`REQUEST_ROLL_AFTER`] at the next quiet moment, at
//! [`REQUEST_LONGEST`] regardless, and immediately when a task reports a
//! final result or an error. The microphone never closes for a roll, so
//! nothing said across the seam is lost to a route change.
//!
//! # Interruptions re-arm, they do not end
//!
//! A phone call, Siri, or another app taking the microphone stops the engine
//! and posts `AVAudioSessionInterruptionNotification`. A port that reported
//! that as `Failed` would leave the phrase dead after the first call of the
//! drive — the defect this design exists to prevent. Instead the worker
//! remembers what the turn asked for (`wanted`), tears the dead capture down,
//! and rebuilds it when the interruption ends — or, when the end never comes
//! (Siri is known not to send one), every [`RESUME_RETRY`] while the request
//! stands. An engine configuration change (headphones, a car connecting)
//! and a media-services reset take the same path. The turn is told nothing:
//! its `Idle { listening_for_wake: true }` is the promise the port is keeping,
//! a few seconds late.
//!
//! # Sharing the speaker with the app in front
//!
//! While armed, the session is `.playAndRecord` with `mixWithOthers`: keeper
//! neither pauses nor quietens Maps or music, because a listener that ducked
//! the whole drive for a phrase it mostly does not hear would be turned off
//! and rightly. While keeper **speaks**, the options switch to `duckOthers`
//! (which implies mixing, per Apple's `AVAudioSession.CategoryOptions`
//! docs), so the answer sits over a quieter Maps prompt rather than a paused
//! one, and the volume comes back the moment the utterance ends. Bluetooth
//! HFP and A2DP are both allowed so a car kit is a route; the speaker is the
//! default when nothing else is. The mode stays `.default`: `.voiceChat`
//! would route to the receiver, and `.voicePrompt` is for playback-only
//! sessions.
//!
//! # Stale results are not failures
//!
//! Cancelling a recognition task delivers its handler an error. Every
//! request gets a serial, the handler captures it, and a result from a serial
//! that is no longer current is dropped — so a roll, a stop and the restart
//! inside `speak` never surface as `Failed`.
//!
//! # Barge-in (FR-403)
//!
//! `speak` rolls to a fresh request before the utterance begins and raises a
//! `speaking` flag; while it is up, any non-empty transcript is
//! `SpeechDetected` rather than `PartialHeard`. Echo cancellation
//! (`setVoiceProcessingEnabled`) is what keeps the app's own voice out of
//! that transcript. The synthesiser's end is polled from the worker loop
//! rather than through an `AVSpeechSynthesizerDelegate`, which would need an
//! Objective-C subclass declared from Rust — more blind code than a 250 ms
//! poll is worth.
//!
//! # Which voice (Epic 64, Story 64.2, AD-182, AD-188)
//!
//! The same shape as the macOS port, because the fault is the same: a
//! synthesiser with no voice set speaks in the system's language whatever
//! the text's. The port enumerates the installed voices' languages once
//! (`AVSpeechSynthesisVoice.speechVoices`, cached beside the locales),
//! detects the answer's dominant language on-device with
//! `NLLanguageRecognizer` constrained to those languages plus the listening
//! one, and `keeper_core::voice::speech::choose_voice` decides. The
//! utterance gets `voiceWithLanguage:` explicitly; `nil` is a refusal that
//! names the language and where a voice is downloaded, never a fall-through
//! to the default voice. No text leaves the phone to be classified.
//!
//! # Asking (FR-408)
//!
//! [`ConsentPort`] is the second port this type implements: it reads what
//! the OS recorded and shows one dialog at a time. *When* to ask is
//! `keeper_core::voice::authorization`; this file never decides it, and
//! [`Worker::availability`] never prompts. The dialogs need the main thread
//! free, so the command that triggers them is `async` and this worker blocks
//! instead.
//!
//! # `unsafe`
//!
//! Every objc2 method on these classes is `unsafe fn`. This file follows the
//! house rule for the shell crate: one `#[allow(unsafe_code)]` function per
//! concern, a `// SAFETY:` comment on each citing the Apple contract it
//! relies on, and a row in the audit inventory in
//! `docs/constraints-and-limitations.md`.
//!
//! # `SpeechAnalyzer`
//!
//! Not used. objc2-speech 0.3.2 does not bind the iOS 26 `SpeechAnalyzer` /
//! `SpeechTranscriber` API, so an availability-gated fast path would be
//! hand-written `msg_send!` against a Swift-first API on a host that cannot
//! compile it. `SFSpeechRecognizer` on-device is the floor and, today, the
//! whole of it.

#![cfg(target_os = "ios")]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use block2::RcBlock;
use keeper_core::voice::events::VoiceEventKind;
use keeper_core::voice::level::{self, Meter};
use keeper_core::voice::locale::{self, DeviceLocales};
use keeper_core::voice::{
    Ask, Consent, ConsentPort, EventSink, Permission, TurnEvent, VoicePlatform, VoicePort,
    VoiceUnavailable, WakePhrase,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObjectProtocol, ProtocolObject};
use objc2::AnyThread;
use objc2_avf_audio::{
    AVAudioEngine, AVAudioEngineConfigurationChangeNotification, AVAudioPCMBuffer, AVAudioSession,
    AVAudioSessionCategoryOptions, AVAudioSessionCategoryPlayAndRecord,
    AVAudioSessionInterruptionNotification, AVAudioSessionInterruptionOptionKey,
    AVAudioSessionInterruptionOptions, AVAudioSessionInterruptionType,
    AVAudioSessionInterruptionTypeKey, AVAudioSessionMediaServicesWereResetNotification,
    AVAudioSessionModeDefault, AVAudioSessionRecordPermission, AVAudioSessionSetActiveOptions,
    AVAudioTime, AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechUtterance,
};
use objc2_foundation::{
    NSArray, NSError, NSLocale, NSNotification, NSNotificationCenter, NSNumber, NSString,
};
use objc2_natural_language::NLLanguageRecognizer;
use objc2_speech::{
    SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask,
    SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus,
};

/// How often the worker looks at the synthesiser and the capture, and the
/// longest a `stop`/`speak` message waits behind one look.
const SPEAK_POLL: Duration = Duration::from_millis(250);

/// How long after `speakUtterance` the synthesiser is given to report
/// `isSpeaking` before its silence counts as the utterance ending — the
/// queue-to-speaking gap on a cold voice.
const SPEAK_GRACE: Duration = Duration::from_millis(500);

/// Frames per tap buffer: ~64 ms at 16 kHz, the recogniser's native rate.
const TAP_FRAMES: u32 = 1024;

/// After this long on one request, roll to a fresh one at the next quiet
/// moment. Under Apple's one-minute guidance for a request, with room for
/// the quiet moment to arrive.
const REQUEST_ROLL_AFTER: Duration = Duration::from_secs(45);

/// A request is quiet when its last transcript is this old — the pause
/// between sentences, so a roll does not cut a word in half.
const REQUEST_ROLL_QUIET: Duration = Duration::from_millis(1500);

/// Roll regardless of quiet after this long: somebody talking without a
/// pause for a minute is rarer than a request that should not run that long.
const REQUEST_LONGEST: Duration = Duration::from_secs(58);

/// How often a listener the system took away is tried again while the turn
/// still wants it.
const RESUME_RETRY: Duration = Duration::from_secs(5);

/// Fresh requests that may fail to start in a row before the port stops
/// trying and tells the turn — bounded so a broken recogniser is a sentence,
/// not a loop.
const ROLL_FAILURES_TOLERATED: u32 = 3;

/// What the command layer asks the worker to do.
enum Command {
    Availability(SyncSender<Result<(), VoiceUnavailable>>),
    Locales(SyncSender<DeviceLocales>),
    /// The person's choice of locale; `None` is "choose for me".
    SetLocale(Option<String>),
    Consent(SyncSender<Result<Consent, VoiceUnavailable>>),
    Ask {
        ask: Ask,
        reply: SyncSender<Permission>,
    },
    Start {
        hints: Vec<String>,
        reply: SyncSender<Result<(), VoiceUnavailable>>,
    },
    Stop,
    /// The languages this phone has synthesiser voices for.
    Voices(SyncSender<Vec<String>>),
    /// The locale recognition runs in, for the voice an answer defaults to.
    Listening(SyncSender<String>),
    /// The dominant language of a text, from among `constraints`.
    Detect {
        text: String,
        constraints: Vec<String>,
        reply: SyncSender<Option<String>>,
    },
    Speak {
        text: String,
        language: String,
        reply: SyncSender<Result<(), VoiceUnavailable>>,
    },
    StopSpeaking,
    /// Something the system did to the audio, reported by an observer or a
    /// result handler, to be acted on from the worker's own thread.
    Audio(AudioNotice),
}

/// What the system did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioNotice {
    /// `AVAudioSessionInterruptionTypeBegan`: the engine is stopped.
    Interrupted,
    /// `AVAudioSessionInterruptionTypeEnded`, with Apple's resume hint.
    InterruptionEnded { should_resume: bool },
    /// `AVAudioEngineConfigurationChangeNotification`: the engine stopped
    /// because its I/O changed (a route, a format).
    EngineChanged,
    /// `AVAudioSessionMediaServicesWereResetNotification`: every audio
    /// object is invalid.
    MediaReset,
    /// The recognition task with this serial ended — a final result, or the
    /// error in `reason`.
    RequestEnded { serial: u64 },
}

/// The iOS [`VoicePort`] and [`ConsentPort`]: a sender to the worker thread.
pub struct IosVoicePort {
    commands: Sender<Command>,
}

impl IosVoicePort {
    /// Start the worker thread and hand it `sink`.
    pub fn new(sink: EventSink) -> Self {
        let (commands, inbox) = mpsc::channel();
        let worker_commands = commands.clone();
        let spawned = std::thread::Builder::new()
            .name("keeper-voice".to_owned())
            .spawn(move || Worker::new(sink, worker_commands).run(inbox));
        if let Err(error) = spawned {
            // The port still exists; every message will find the receiver
            // gone and answer `Unsupported` through `ask`.
            tracing::error!(%error, "voice: could not start the worker thread");
        }
        Self { commands }
    }

    /// Send a request that expects an answer and wait for it; `None` when
    /// the worker is gone.
    fn ask<T>(&self, build: impl FnOnce(SyncSender<T>) -> Command) -> Option<T> {
        let (reply, answer) = mpsc::sync_channel(1);
        if self.commands.send(build(reply)).is_err() {
            tracing::error!("voice: the worker thread is gone");
            return None;
        }
        answer.recv().ok().or_else(|| {
            tracing::error!("voice: the worker thread dropped a reply");
            None
        })
    }

    /// Send a request that expects no answer.
    fn tell(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::error!("voice: the worker thread is gone");
        }
    }
}

impl VoicePort for IosVoicePort {
    fn platform(&self) -> VoicePlatform {
        VoicePlatform::IOS
    }

    fn availability(&self) -> Result<(), VoiceUnavailable> {
        self.ask(Command::Availability)
            .unwrap_or(Err(VoiceUnavailable::Unsupported))
    }

    fn locales(&self) -> DeviceLocales {
        self.ask(Command::Locales).unwrap_or_default()
    }

    fn set_locale(&self, requested: Option<String>) {
        self.tell(Command::SetLocale(requested));
    }

    fn start_listening(&self, wake: Option<&WakePhrase>) -> Result<(), VoiceUnavailable> {
        let hints = wake
            .map(|w| w.words().map(str::to_owned).collect())
            .unwrap_or_default();
        self.ask(|reply| Command::Start { hints, reply })
            .unwrap_or(Err(VoiceUnavailable::Unsupported))
    }

    fn stop_listening(&self) {
        self.tell(Command::Stop);
    }

    fn voices(&self) -> Vec<String> {
        self.ask(Command::Voices).unwrap_or_default()
    }

    fn listening(&self) -> String {
        self.ask(Command::Listening).unwrap_or_default()
    }

    fn detect_language(&self, text: &str, constraints: &[String]) -> Option<String> {
        let text = text.to_owned();
        let constraints = constraints.to_vec();
        self.ask(|reply| Command::Detect {
            text,
            constraints,
            reply,
        })
        .flatten()
    }

    fn speak(&self, text: &str, language: &str) -> Result<(), VoiceUnavailable> {
        let text = text.to_owned();
        let language = language.to_owned();
        self.ask(|reply| Command::Speak {
            text,
            language,
            reply,
        })
        .unwrap_or(Err(VoiceUnavailable::Unsupported))
    }

    fn stop_speaking(&self) {
        self.tell(Command::StopSpeaking);
    }
}

impl ConsentPort for IosVoicePort {
    fn consent(&self) -> Result<Consent, VoiceUnavailable> {
        self.ask(Command::Consent)
            .unwrap_or(Err(VoiceUnavailable::Unsupported))
    }

    fn ask(&self, ask: Ask) -> Permission {
        // A worker that cannot answer has not asked, and the decision layer
        // treats an answer that is not `Granted` as the refusal it is.
        IosVoicePort::ask(self, |reply| Command::Ask { ask, reply }).unwrap_or(Permission::Denied)
    }
}

/// The request the tap feeds right now. Swapped by the worker on a roll;
/// read by the tap on the audio thread.
type RequestSlot = Mutex<Option<Retained<SFSpeechAudioBufferRecognitionRequest>>>;

/// The long-lived half of listening: the engine capturing into whichever
/// request the slot holds. Outlives every request; dropped only by
/// [`Worker::end_capture`].
struct Capture {
    engine: Retained<AVAudioEngine>,
    slot: Arc<RequestSlot>,
    /// Kept so the block outlives the tap it is installed as.
    _tap: RcBlock<dyn Fn(std::ptr::NonNull<AVAudioPCMBuffer>, std::ptr::NonNull<AVAudioTime>)>,
    /// The configuration-change observer for this engine, removed with it.
    change_observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

/// One recognition request and the task answering it. Rolled by
/// [`Worker::roll_request`]; the capture underneath it stays.
struct Recognition {
    /// Held so the task's recogniser outlives the task.
    _recognizer: Retained<SFSpeechRecognizer>,
    request: Retained<SFSpeechAudioBufferRecognitionRequest>,
    task: Retained<SFSpeechRecognitionTask>,
    serial: u64,
    started: Instant,
    /// Milliseconds since the worker's epoch of the last non-empty
    /// transcript, written by the result handler.
    last_heard: Arc<AtomicU64>,
}

/// The worker: owns every framework object, runs on its own thread.
struct Worker {
    sink: EventSink,
    /// The worker's own inbox, for observers and handlers to post notices.
    commands: Sender<Command>,
    epoch: Instant,
    /// What the turn asked for: `Some(hints)` from `start_listening` until
    /// `stop_listening`. The capture follows it, late if the system made it.
    wanted: Option<Vec<String>>,
    capture: Option<Capture>,
    recognition: Option<Recognition>,
    synthesizer: Option<Retained<AVSpeechSynthesizer>>,
    /// The serial of the current request; handlers from older serials are
    /// stale and dropped.
    current: Arc<AtomicU64>,
    /// Up while an utterance is being spoken: a transcript then is barge-in.
    speaking: Arc<AtomicBool>,
    /// When the current utterance was handed to the synthesiser.
    speaking_since: Option<Instant>,
    /// When the system took the capture away, for the retry clock; `None`
    /// while capturing or not wanted.
    suspended: Option<Instant>,
    /// Fresh requests that failed to start since the last that worked.
    failed_starts: u32,
    /// The person's choice of locale (`bots.voice_locale`); `None` is
    /// "choose for me". Handed to `locale::choose` with `locales`.
    requested: Option<String>,
    /// The enumeration of `supportedLocales()` by on-device support, filled
    /// on first need and refreshed on `SetLocale` — a deliberate act, and
    /// the moment a language the person just added to Dictation would be
    /// looked for. Never per probe.
    locales: Option<DeviceLocales>,
    /// The languages of `AVSpeechSynthesisVoice.speechVoices()`, each once,
    /// sorted; filled on first need and refreshed with `locales` (Epic 64).
    voices: Option<Vec<String>>,
    /// The session observers, removed when the worker ends.
    session_observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl Worker {
    fn new(sink: EventSink, commands: Sender<Command>) -> Self {
        let session_observers = observe_session(&commands);
        Self {
            sink,
            commands,
            epoch: Instant::now(),
            wanted: None,
            capture: None,
            recognition: None,
            synthesizer: None,
            current: Arc::new(AtomicU64::new(0)),
            speaking: Arc::new(AtomicBool::new(false)),
            speaking_since: None,
            suspended: None,
            failed_starts: 0,
            requested: None,
            locales: None,
            voices: None,
            session_observers,
        }
    }

    /// The loop: serve commands, and between them keep the capture alive
    /// and watch the synthesiser.
    fn run(mut self, inbox: Receiver<Command>) {
        loop {
            match inbox.recv_timeout(SPEAK_POLL) {
                Ok(Command::Availability(reply)) => {
                    let _ = reply.send(self.availability());
                }
                Ok(Command::Locales(reply)) => {
                    let _ = reply.send(self.locales().clone());
                }
                Ok(Command::SetLocale(requested)) => {
                    tracing::info!(?requested, "voice: locale choice set; re-enumerating");
                    self.requested = requested;
                    self.locales = None;
                    self.voices = None;
                }
                Ok(Command::Voices(reply)) => {
                    let _ = reply.send(self.voices().clone());
                }
                Ok(Command::Listening(reply)) => {
                    let _ = reply.send(self.listening());
                }
                Ok(Command::Detect {
                    text,
                    constraints,
                    reply,
                }) => {
                    let _ = reply.send(detect_language(&text, &constraints));
                }
                Ok(Command::Consent(reply)) => {
                    let _ = reply.send(recognizer_class().map(|()| Consent {
                        speech: speech_consent(),
                        microphone: microphone_consent(),
                    }));
                }
                Ok(Command::Ask { ask, reply }) => {
                    let answer = match ask {
                        Ask::Speech => ask_speech(),
                        Ask::Microphone => ask_microphone(),
                    };
                    let _ = reply.send(answer);
                }
                Ok(Command::Start { hints, reply }) => {
                    let _ = reply.send(self.start(hints));
                }
                Ok(Command::Stop) => self.stop(),
                Ok(Command::Speak {
                    text,
                    language,
                    reply,
                }) => {
                    let _ = reply.send(self.speak(&text, &language));
                }
                Ok(Command::StopSpeaking) => self.stop_speaking(),
                Ok(Command::Audio(notice)) => self.on_audio(notice),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.stop();
                    self.forget_session_observers();
                    return;
                }
            }
            self.tick();
        }
    }

    /// Start (or, while already capturing, restart on a fresh request)
    /// listening for `hints`.
    ///
    /// A session the system will not give up right now — a call in
    /// progress — is not a refusal: the request is recorded and the capture
    /// arrives when the interruption ends. Only what the person must act on
    /// (authorisation, a missing model, no microphone at all) is refused.
    fn start(&mut self, hints: Vec<String>) -> Result<(), VoiceUnavailable> {
        self.availability()?;
        self.wanted = Some(hints);
        self.failed_starts = 0;
        match self.arm() {
            Ok(()) => Ok(()),
            Err(error) => {
                tracing::warn!(%error, "voice: the capture did not start; will retry");
                crate::voice_log::record(VoiceEventKind::Refused, Some(error.clone()));
                self.suspend();
                Ok(())
            }
        }
    }

    /// What this phone knows about locales, from the cache or by
    /// enumerating once. A build without the recogniser class enumerates
    /// nothing and reports the system locale only; `availability` names
    /// that defect itself.
    fn locales(&mut self) -> &DeviceLocales {
        self.locales.get_or_insert_with(|| {
            let on_device = match recognizer_class() {
                Ok(()) => on_device_locales(),
                Err(_) => Vec::new(),
            };
            let locales = DeviceLocales {
                system: system_locale(),
                on_device,
            };
            tracing::info!(
                system = %locales.system,
                on_device = ?locales.on_device,
                "voice: on-device locales enumerated"
            );
            locales
        })
    }

    /// The locale to build the recogniser for, or the refusal — core's
    /// answer over the person's choice and this phone's enumeration.
    fn chosen(&mut self) -> Result<String, VoiceUnavailable> {
        let requested = self.requested.clone();
        let locales = self.locales();
        locale::choose(requested.as_deref(), &locales.system, &locales.on_device)
    }

    /// The locale recognition runs in, as the surface shows it — core's
    /// `in_force` over the same inputs as [`Worker::chosen`] (Epic 64: the
    /// language an answer is spoken in when its own cannot be told).
    fn listening(&mut self) -> String {
        let requested = self.requested.clone();
        let locales = self.locales();
        locale::in_force(requested.as_deref(), &locales.system, &locales.on_device)
    }

    /// The languages this phone has voices for, from the cache or by
    /// enumerating once.
    fn voices(&mut self) -> &Vec<String> {
        self.voices.get_or_insert_with(|| {
            let voices = voice_languages();
            tracing::info!(
                count = voices.len(),
                languages = ?voices,
                "voice: synthesiser voice languages enumerated"
            );
            voices
        })
    }

    /// Whether listening and speaking can work right now (FR-402).
    ///
    /// Order: the recogniser class itself, then authorisation (both the
    /// recogniser's and the microphone's), then an input route, then the
    /// chosen locale — `keeper_core::voice::locale::choose` over the cache —
    /// and a recogniser for it that can work on the device now.
    /// `NotDetermined` is `NotAuthorized`: this function never prompts —
    /// asking is [`ConsentPort`]'s, decided by
    /// `keeper_core::voice::authorization` (FR-408).
    fn availability(&mut self) -> Result<(), VoiceUnavailable> {
        recognizer_class()?;
        if speech_authorization() != SFSpeechRecognizerAuthorizationStatus::Authorized {
            return Err(VoiceUnavailable::NotAuthorized);
        }
        match microphone() {
            Microphone::Absent => return Err(VoiceUnavailable::NoMicrophone),
            Microphone::NotAuthorized => return Err(VoiceUnavailable::NotAuthorized),
            Microphone::Ready => {}
        }
        let chosen = self.chosen()?;
        match recognizer_for(&locale_named(&chosen)) {
            Recognizer::Ready(_) => Ok(()),
            Recognizer::NoModel => Err(VoiceUnavailable::NoOnDeviceModel { locale: chosen }),
            Recognizer::Absent | Recognizer::ServerOnly => {
                // The cache said this locale could run and the framework
                // now says otherwise: the enumeration is stale. Refresh it
                // and refuse with what is true now.
                self.locales = None;
                let on_device = self.locales().on_device.clone();
                Err(VoiceUnavailable::NoOnDeviceRecognition {
                    locale: chosen,
                    on_device,
                })
            }
        }
    }

    /// Bring the capture up if it is down, then roll to a fresh request.
    fn arm(&mut self) -> Result<(), String> {
        if self.capture.is_none() {
            configure_session(ARMED)?;
            self.capture = Some(start_capture(&self.commands, Arc::clone(&self.sink))?);
        }
        self.roll_request()
    }

    /// Note that the capture is gone and the retry clock runs.
    fn suspend(&mut self) {
        self.end_recognition();
        self.end_capture();
        self.suspended = Some(Instant::now());
    }

    /// End recognition, stop capture, and let the session go.
    fn stop(&mut self) {
        self.wanted = None;
        self.suspended = None;
        self.end_recognition();
        self.end_capture();
        release_session();
    }

    fn end_recognition(&mut self) {
        // Bump the serial first so the cancelled task's error is stale on
        // arrival.
        self.current.fetch_add(1, Ordering::SeqCst);
        if let Some(recognition) = self.recognition.take() {
            end_request(&recognition);
        }
    }

    fn end_capture(&mut self) {
        if let Some(capture) = self.capture.take() {
            stop_capture(&capture);
        }
    }

    /// Replace the current request with a fresh one on the same capture.
    fn roll_request(&mut self) -> Result<(), String> {
        self.end_recognition();
        let chosen = self
            .chosen()
            .map_err(|why| why.message(&VoicePlatform::IOS))?;
        let Some(capture) = &self.capture else {
            return Err("no capture to roll on".to_owned());
        };
        let hints = self.wanted.clone().unwrap_or_default();
        let serial = self.current.load(Ordering::SeqCst);
        let last_heard = Arc::new(AtomicU64::new(self.millis()));
        let handler = self.result_handler(serial, Arc::clone(&last_heard));
        match start_request(
            &locale_named(&chosen),
            &hints,
            &handler,
            &capture.slot,
            serial,
            last_heard,
        ) {
            Ok(recognition) => {
                crate::voice_log::record(VoiceEventKind::Rolled, None);
                self.recognition = Some(recognition);
                self.failed_starts = 0;
                Ok(())
            }
            Err(error) => {
                self.failed_starts += 1;
                Err(error)
            }
        }
    }

    /// Read `text` aloud in the voice for `language` — core's choice over
    /// this phone's inventory (Epic 64, AD-182); recognition rolls to a
    /// fresh request so that the first transcript to arrive is the person,
    /// not the tail of what they said before, and other audio ducks for
    /// the duration. A language the framework answers no voice for is
    /// refused before anything rolls or ducks: the default voice would be
    /// the wrong language.
    fn speak(&mut self, text: &str, language: &str) -> Result<(), VoiceUnavailable> {
        let Some(voice) = voice_for_language(language) else {
            tracing::warn!(
                language,
                "voice: no synthesiser voice for the chosen language"
            );
            return Err(VoiceUnavailable::NoVoice {
                language: language.to_owned(),
            });
        };
        tracing::info!(
            language,
            voice = %voice_name(&voice),
            "voice: utterance voice chosen"
        );
        if self.capture.is_some() {
            if let Err(error) = self.roll_request() {
                tracing::warn!(%error, "voice: could not roll the request before speaking");
            }
            if let Err(error) = set_session_options(DUCKING) {
                tracing::debug!(%error, "voice: others did not duck");
            }
        }
        let synthesizer = self.synthesizer.get_or_insert_with(new_synthesizer);
        self.speaking.store(true, Ordering::SeqCst);
        self.speaking_since = Some(Instant::now());
        speak_text(synthesizer, text, &voice);
        Ok(())
    }

    fn stop_speaking(&mut self) {
        if let Some(synthesizer) = &self.synthesizer {
            stop_speech(synthesizer);
        }
        self.speech_over();
    }

    /// The utterance is over, one way or another: flag down, others back
    /// to full volume.
    fn speech_over(&mut self) {
        self.speaking.store(false, Ordering::SeqCst);
        self.speaking_since = None;
        if self.capture.is_some() {
            if let Err(error) = set_session_options(ARMED) {
                tracing::debug!(%error, "voice: others did not un-duck");
            }
        }
    }

    /// Between commands: the retry clock, the roll clock, the synthesiser.
    fn tick(&mut self) {
        if self.wanted.is_some() {
            if let Some(since) = self.suspended {
                if since.elapsed() >= RESUME_RETRY {
                    self.resume();
                }
            } else if self
                .capture
                .as_ref()
                .is_some_and(|c| !engine_running(&c.engine))
            {
                // Stopped without a word from the system — Siri does this.
                tracing::info!("voice: the engine stopped on its own; resuming");
                crate::voice_log::record(
                    VoiceEventKind::Resumed,
                    Some("the engine stopped on its own".to_owned()),
                );
                self.resume();
            } else if self.roll_due() {
                if let Err(error) = self.roll_request() {
                    self.roll_failed(error);
                }
            }
        }
        self.watch_speech_end();
    }

    /// Whether the current request has run long enough to be replaced.
    fn roll_due(&self) -> bool {
        let Some(recognition) = &self.recognition else {
            return self.capture.is_some();
        };
        let age = recognition.started.elapsed();
        if age >= REQUEST_LONGEST {
            return true;
        }
        if age < REQUEST_ROLL_AFTER {
            return false;
        }
        let quiet_for = Duration::from_millis(
            self.millis()
                .saturating_sub(recognition.last_heard.load(Ordering::SeqCst)),
        );
        quiet_for >= REQUEST_ROLL_QUIET
    }

    /// Rebuild the capture after the system took it; on refusal, wait for
    /// the next tick of the retry clock.
    fn resume(&mut self) {
        self.end_recognition();
        self.end_capture();
        match self.arm() {
            Ok(()) => {
                if self.suspended.take().is_some() {
                    tracing::info!("voice: listening resumed after an interruption");
                    crate::voice_log::record(VoiceEventKind::Resumed, None);
                }
            }
            Err(error) => {
                tracing::debug!(%error, "voice: not yet; will retry");
                self.end_capture();
                self.suspended = Some(Instant::now());
            }
        }
    }

    /// A fresh request would not start. Bounded: after enough in a row the
    /// turn is told and the port stops wanting.
    fn roll_failed(&mut self, error: String) {
        if self.failed_starts >= ROLL_FAILURES_TOLERATED {
            tracing::warn!(%error, "voice: recognition keeps failing to start");
            crate::voice_log::record(VoiceEventKind::Refused, Some(error.clone()));
            self.stop();
            (self.sink)(TurnEvent::Failed(error));
        } else {
            tracing::debug!(%error, "voice: recognition did not restart; retrying");
            self.suspend();
        }
    }

    /// What the system did, acted on here rather than in the observer.
    fn on_audio(&mut self, notice: AudioNotice) {
        tracing::debug!(?notice, "voice: audio notice");
        match notice {
            AudioNotice::Interrupted => {
                crate::voice_log::record(VoiceEventKind::InterruptionBegun, None);
                if self.speaking_since.is_some() {
                    // The answer is gone with the speaker; the turn ends as
                    // if it had finished.
                    self.stop_speaking();
                    (self.sink)(TurnEvent::Silence);
                }
                if self.wanted.is_some() {
                    self.suspend();
                }
            }
            AudioNotice::InterruptionEnded { should_resume } => {
                // Apple's hint is about playback etiquette; a microphone the
                // person armed comes back either way.
                tracing::info!(should_resume, "voice: interruption ended");
                crate::voice_log::record(
                    VoiceEventKind::InterruptionEnded,
                    Some(format!("should_resume={should_resume}")),
                );
                if self.wanted.is_some() {
                    self.resume();
                }
            }
            AudioNotice::EngineChanged => {
                if self.wanted.is_some() && self.suspended.is_none() {
                    self.resume();
                }
            }
            AudioNotice::MediaReset => {
                self.synthesizer = None;
                self.speech_over();
                if self.wanted.is_some() {
                    self.resume();
                }
            }
            AudioNotice::RequestEnded { serial } => {
                let current = self
                    .recognition
                    .as_ref()
                    .is_some_and(|r| r.serial == serial);
                if current && self.wanted.is_some() && self.suspended.is_none() {
                    if let Err(error) = self.roll_request() {
                        self.roll_failed(error);
                    }
                }
            }
        }
    }

    /// While an utterance is out, notice it ending and say so once.
    fn watch_speech_end(&mut self) {
        let Some(since) = self.speaking_since else {
            return;
        };
        if since.elapsed() < SPEAK_GRACE {
            return;
        }
        let still = self
            .synthesizer
            .as_ref()
            .is_some_and(|synthesizer| is_speaking(synthesizer));
        if !still {
            self.speech_over();
            (self.sink)(TurnEvent::Silence);
        }
    }

    fn millis(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// The block the recogniser calls with each result. Captures the serial
    /// it belongs to, the current-serial cell, the speaking flag, the sink
    /// and the worker's inbox — all `Send`, none of them a framework object.
    fn result_handler(
        &self,
        serial: u64,
        last_heard: Arc<AtomicU64>,
    ) -> RcBlock<dyn Fn(*mut SFSpeechRecognitionResult, *mut NSError)> {
        let current = Arc::clone(&self.current);
        let speaking = Arc::clone(&self.speaking);
        let sink = Arc::clone(&self.sink);
        let commands = self.commands.clone();
        let epoch = self.epoch;
        RcBlock::new(
            move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
                if current.load(Ordering::SeqCst) != serial {
                    return;
                }
                match read_result(result, error) {
                    Ok(Some((text, is_final))) => {
                        if !text.trim().is_empty() {
                            let now =
                                u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
                            last_heard.store(now, Ordering::SeqCst);
                        }
                        if speaking.load(Ordering::SeqCst) {
                            if !text.trim().is_empty() {
                                sink(TurnEvent::SpeechDetected);
                            }
                        } else if is_final {
                            sink(TurnEvent::FinalHeard(text));
                        } else {
                            sink(TurnEvent::PartialHeard(text));
                        }
                        if is_final {
                            let _ =
                                commands.send(Command::Audio(AudioNotice::RequestEnded { serial }));
                        }
                    }
                    Ok(None) => {}
                    Err(reason) => {
                        // A task's own end is the port's to survive, not the
                        // turn's to fail on: roll, and only a roll that keeps
                        // failing reaches the sink.
                        tracing::debug!(%reason, "voice: the recognition task ended");
                        let _ = commands.send(Command::Audio(AudioNotice::RequestEnded { serial }));
                    }
                }
            },
        )
    }

    fn forget_session_observers(&mut self) {
        for observer in self.session_observers.drain(..) {
            forget_observer(&observer);
        }
    }
}

// ---------------------------------------------------------------------------
// The session's shape.
// ---------------------------------------------------------------------------

/// The options while armed: mixable, so the app in front keeps its audio;
/// speaker by default; car kits and headsets as routes.
///
/// Apple, `AVAudioSession.CategoryOptions`: `mixWithOthers` "allows other
/// applications to play in the background while your app has both audio
/// input and output enabled"; without it, activating `.playAndRecord`
/// interrupts every other session — which is the "kills Maps" case.
const ARMED: AVAudioSessionCategoryOptions = AVAudioSessionCategoryOptions::MixWithOthers
    .union(AVAudioSessionCategoryOptions::DefaultToSpeaker)
    .union(AVAudioSessionCategoryOptions::AllowBluetoothHFP)
    .union(AVAudioSessionCategoryOptions::AllowBluetoothA2DP);

/// The options while keeper speaks: the same routes, and others ducked.
///
/// Apple, the same page: `duckOthers` "reduces the volume of any music
/// currently being played" and "setting this option will also make your
/// session mixable with others". So Maps' prompt is quieter under keeper's
/// answer, not paused, and comes back to full volume when the utterance
/// ends and [`ARMED`] is set again.
const DUCKING: AVAudioSessionCategoryOptions = AVAudioSessionCategoryOptions::DuckOthers
    .union(AVAudioSessionCategoryOptions::DefaultToSpeaker)
    .union(AVAudioSessionCategoryOptions::AllowBluetoothHFP)
    .union(AVAudioSessionCategoryOptions::AllowBluetoothA2DP);

// ---------------------------------------------------------------------------
// The FFI, one function per concern.
// ---------------------------------------------------------------------------

/// Whether `SFSpeechRecognizer` is registered in this process at all.
///
/// objc2 reaches the class by name at run time, and `objc2-speech`'s
/// `#[link(name = "Speech", kind = "framework")]` sits on an empty extern
/// block that never reaches Xcode's link line (`libapp.a` is linked by the
/// generated project, not by rustc). So a project that does not list
/// `Speech.framework` builds, ships every `voice_*` command, and has no
/// recogniser class — and every call below this one would abort the worker
/// with "class not found". The check is first on every path that touches
/// the class, and the refusal is the build's, said once at error level so
/// the log names it even before a surface asks.
fn recognizer_class() -> Result<(), VoiceUnavailable> {
    if AnyClass::get(c"SFSpeechRecognizer").is_some() {
        return Ok(());
    }
    tracing::error!(
        "voice: SFSpeechRecognizer is not registered — this build was linked without Speech.framework (gen/apple/project.yml must list it)"
    );
    Err(VoiceUnavailable::NoRecognizer)
}

/// `SFSpeechRecognizer.authorizationStatus`.
#[allow(unsafe_code)]
fn speech_authorization() -> SFSpeechRecognizerAuthorizationStatus {
    // SAFETY: `+[SFSpeechRecognizer authorizationStatus]` is a class method
    // that reads the app's recorded authorisation and returns an enum; it
    // takes no arguments, touches no caller-owned memory, and Apple documents
    // it as callable from any thread. It never prompts.
    unsafe { SFSpeechRecognizer::authorizationStatus() }
}

/// The recogniser's recorded permission, as the decision layer reads it.
fn speech_consent() -> Permission {
    match speech_authorization() {
        SFSpeechRecognizerAuthorizationStatus::NotDetermined => Permission::NotDetermined,
        SFSpeechRecognizerAuthorizationStatus::Authorized => Permission::Granted,
        _ => Permission::Denied,
    }
}

/// What the audio session says about the microphone.
enum Microphone {
    Ready,
    Absent,
    NotAuthorized,
}

/// `AVAudioSession.isInputAvailable` and `.recordPermission`.
///
/// `recordPermission` is deprecated in favour of `AVAudioApplication`, which
/// is iOS 17; the floor is iOS 16, so the older property is the one every
/// supported phone has.
#[allow(unsafe_code)]
#[allow(deprecated)]
fn microphone() -> Microphone {
    // SAFETY: `+[AVAudioSession sharedInstance]` returns the process
    // singleton, retained here for the call's duration. `isInputAvailable`
    // and `recordPermission` are read-only properties documented as
    // thread-safe; neither prompts.
    let session = unsafe { AVAudioSession::sharedInstance() };
    if !unsafe { session.isInputAvailable() } {
        return Microphone::Absent;
    }
    if unsafe { session.recordPermission() } != AVAudioSessionRecordPermission::Granted {
        return Microphone::NotAuthorized;
    }
    Microphone::Ready
}

/// The microphone's recorded permission, as the decision layer reads it.
#[allow(unsafe_code)]
#[allow(deprecated)]
fn microphone_consent() -> Permission {
    // SAFETY: as in `microphone`: the retained singleton and a read-only,
    // thread-safe property that never prompts.
    let session = unsafe { AVAudioSession::sharedInstance() };
    match unsafe { session.recordPermission() } {
        AVAudioSessionRecordPermission::Undetermined => Permission::NotDetermined,
        AVAudioSessionRecordPermission::Granted => Permission::Granted,
        _ => Permission::Denied,
    }
}

/// `+[SFSpeechRecognizer requestAuthorization:]`: the recogniser's dialog,
/// shown once by the OS, waited for here (FR-408).
#[allow(unsafe_code)]
fn ask_speech() -> Permission {
    // SAFETY: a class method taking a completion block, which Apple documents
    // as executing asynchronously and calling the handler exactly once "at
    // some point later" on an unspecified queue — never on the caller, which
    // is why this blocks a worker and not the main thread. The block is
    // copied by the call and captures only a `SyncSender`. Apple's stated
    // precondition — `NSSpeechRecognitionUsageDescription` present, or the
    // app crashes — is met by `gen/apple/project.yml`.
    let (reply, answer) = mpsc::sync_channel::<SFSpeechRecognizerAuthorizationStatus>(1);
    let handler = RcBlock::new(move |status: SFSpeechRecognizerAuthorizationStatus| {
        let _ = reply.send(status);
    });
    unsafe { SFSpeechRecognizer::requestAuthorization(&handler) };
    match answer.recv() {
        Ok(SFSpeechRecognizerAuthorizationStatus::Authorized) => Permission::Granted,
        Ok(SFSpeechRecognizerAuthorizationStatus::NotDetermined) => Permission::NotDetermined,
        Ok(_) => Permission::Denied,
        Err(_) => {
            tracing::error!("voice: the recogniser's authorisation handler never answered");
            Permission::Denied
        }
    }
}

/// `-[AVAudioSession requestRecordPermission:]`: the microphone's dialog,
/// shown once by the OS, waited for here (FR-408). Deprecated for
/// `AVAudioApplication` (iOS 17); the floor is 16.
#[allow(unsafe_code)]
#[allow(deprecated)]
fn ask_microphone() -> Permission {
    // SAFETY: the retained singleton and a completion block Apple documents
    // as "called immediately if permission has already been granted or
    // denied", otherwise once the dialog is dismissed, "in a different thread
    // context" — so a blocking wait here is off the main thread by
    // construction. The block captures only a `SyncSender`.
    // `NSMicrophoneUsageDescription` is in `gen/apple/project.yml`.
    let (reply, answer) = mpsc::sync_channel::<bool>(1);
    let handler = RcBlock::new(move |granted: Bool| {
        let _ = reply.send(granted.as_bool());
    });
    let session = unsafe { AVAudioSession::sharedInstance() };
    unsafe { session.requestRecordPermission(&handler) };
    match answer.recv() {
        Ok(true) => Permission::Granted,
        Ok(false) => Permission::Denied,
        Err(_) => {
            tracing::error!("voice: the microphone's permission handler never answered");
            Permission::Denied
        }
    }
}

/// What the framework has for a locale.
enum Recognizer {
    /// No recogniser at all: the locale is not in `supportedLocales`.
    Absent,
    /// A recogniser that reports `supportsOnDeviceRecognition == false`:
    /// the OS has no on-device asset for the language. Adding the language
    /// under Settings > General > Keyboard > Dictation Languages may change
    /// that; the enumeration is refreshed when the person changes the
    /// choice.
    ServerOnly,
    /// A recogniser that can run on this phone but is not available right
    /// now (`isAvailable == false`).
    NoModel,
    /// A recogniser that can work now, without the network.
    Ready(Retained<SFSpeechRecognizer>),
}

/// The system locale as the OS spells it (`en_US`, an underscore — the
/// framework's own list says `en-US`; `keeper_core::voice::locale`
/// normalises the two before comparing them).
fn system_locale() -> String {
    NSLocale::currentLocale().localeIdentifier().to_string()
}

/// An `NSLocale` for an identifier from the enumeration or the setting.
/// `+[NSLocale localeWithLocaleIdentifier:]` accepts both separators.
fn locale_named(identifier: &str) -> Retained<NSLocale> {
    NSLocale::localeWithLocaleIdentifier(&NSString::from_str(identifier))
}

/// Every locale in `supportedLocales()` whose recogniser can run on this
/// phone, as the framework spells them, sorted by identifier. A locale
/// whose model is merely not downloaded yet (`isAvailable == false`) is in
/// the list: it can run here, and `availability` names the download.
///
/// Constructs one recogniser per supported locale, so the caller caches
/// the answer.
#[allow(unsafe_code)]
fn on_device_locales() -> Vec<String> {
    // SAFETY: `+[SFSpeechRecognizer supportedLocales]` returns a set the
    // binding retains; `allObjects` copies it into an array the binding
    // also retains, and `to_vec` retains each element before the array is
    // released. Neither prompts nor touches the network: Apple documents
    // the set as static — the locales the keyboard's dictation supports.
    // Classifying each is `recognizer_for`, whose contract is its own.
    let supported = unsafe { SFSpeechRecognizer::supportedLocales() };
    let mut locales: Vec<String> = supported
        .allObjects()
        .to_vec()
        .iter()
        .filter(|locale| {
            matches!(
                recognizer_for(locale),
                Recognizer::Ready(_) | Recognizer::NoModel
            )
        })
        .map(|locale| locale.localeIdentifier().to_string())
        .collect();
    locales.sort_unstable();
    locales
}

/// The recogniser for `locale`, classified for the absences the sentences
/// name.
#[allow(unsafe_code)]
fn recognizer_for(locale: &NSLocale) -> Recognizer {
    // SAFETY: `-[SFSpeechRecognizer initWithLocale:]` consumes the fresh
    // allocation and returns nil when the locale has no recogniser, which
    // objc2 surfaces as `None`. `supportsOnDeviceRecognition` and
    // `isAvailable` are read-only properties on the retained object. None
    // prompts or touches the network: Apple documents the first property as
    // the precondition a request's `requiresOnDeviceRecognition` needs to be
    // honoured, and the second as whether the recogniser can be used now.
    let Some(recognizer) =
        (unsafe { SFSpeechRecognizer::initWithLocale(SFSpeechRecognizer::alloc(), locale) })
    else {
        return Recognizer::Absent;
    };
    if !unsafe { recognizer.supportsOnDeviceRecognition() } {
        return Recognizer::ServerOnly;
    }
    if !unsafe { recognizer.isAvailable() } {
        return Recognizer::NoModel;
    }
    Recognizer::Ready(recognizer)
}

/// Put the shared session in `.playAndRecord` with `options` and activate
/// it.
#[allow(unsafe_code)]
fn configure_session(options: AVAudioSessionCategoryOptions) -> Result<(), String> {
    // SAFETY: `AVAudioSessionCategoryPlayAndRecord` and
    // `AVAudioSessionModeDefault` are Apple's process-lifetime extern
    // `NSString` constants; reading them carries no other obligation, and a
    // nil (which no iOS ships) is answered rather than dereferenced.
    // `setCategory:mode:options:error:` and `setActive:error:` take the
    // retained singleton and documented enum values; a refusal — including
    // `AVAudioSessionErrorCodeCannotInterruptOthers` during a call — is
    // returned as `NSError`, never undefined behaviour.
    let session = unsafe { AVAudioSession::sharedInstance() };
    let category = unsafe { AVAudioSessionCategoryPlayAndRecord }
        .ok_or("AVAudioSessionCategoryPlayAndRecord is nil")?;
    let mode = unsafe { AVAudioSessionModeDefault }.ok_or("AVAudioSessionModeDefault is nil")?;
    unsafe { session.setCategory_mode_options_error(category, mode, options) }
        .map_err(|error| error.localizedDescription().to_string())?;
    unsafe { session.setActive_error(true) }
        .map_err(|error| error.localizedDescription().to_string())
}

/// Change the active session's options without deactivating it: ducking on
/// for an utterance, off after.
#[allow(unsafe_code)]
fn set_session_options(options: AVAudioSessionCategoryOptions) -> Result<(), String> {
    // SAFETY: as in `configure_session`, minus activation. Apple allows
    // `setCategory:mode:options:error:` on an active session; the category
    // and mode are unchanged, so no route changes, and if one did the
    // engine's configuration-change notification rebuilds the capture.
    let session = unsafe { AVAudioSession::sharedInstance() };
    let category = unsafe { AVAudioSessionCategoryPlayAndRecord }
        .ok_or("AVAudioSessionCategoryPlayAndRecord is nil")?;
    let mode = unsafe { AVAudioSessionModeDefault }.ok_or("AVAudioSessionModeDefault is nil")?;
    unsafe { session.setCategory_mode_options_error(category, mode, options) }
        .map_err(|error| error.localizedDescription().to_string())
}

/// Deactivate the shared session, un-ducking whatever was ducked.
#[allow(unsafe_code)]
fn release_session() {
    // SAFETY: the retained singleton and a documented option flag;
    // deactivation while I/O still runs is reported as `NSError`
    // (`AVAudioSessionErrorCodeIsBusy`) and the session is deactivated
    // anyway, which is what a release wants.
    let session = unsafe { AVAudioSession::sharedInstance() };
    if let Err(error) = unsafe {
        session.setActive_withOptions_error(
            false,
            AVAudioSessionSetActiveOptions::NotifyOthersOnDeactivation,
        )
    } {
        tracing::debug!(%error, "voice: the audio session did not deactivate cleanly");
    }
}

/// Start capturing into whatever request the slot holds.
///
/// The engine is built new per capture rather than kept: voice processing
/// may only be toggled on a stopped engine, and a fresh one is always
/// stopped. The tap reads the slot on every buffer, so a request can be
/// rolled underneath it without touching the engine.
///
/// The tap also meters (Story 64.3, AD-186): the RMS of channel 0 of each
/// buffer goes to a `keeper_core::voice::level::Meter`, and only the
/// readings its limiter lets through — at most ~25 a second, only while
/// the level moves — reach `sink` as [`TurnEvent::Level`]. The recogniser
/// still receives every buffer, untouched, before the meter looks at it.
#[allow(unsafe_code)]
// The slot crosses to the audio thread inside a block, which Rust cannot see
// and clippy therefore flags; the SAFETY comment below is the argument.
#[allow(clippy::arc_with_non_send_sync)]
fn start_capture(commands: &Sender<Command>, sink: EventSink) -> Result<Capture, String> {
    // SAFETY: every object below is freshly allocated or retained here and
    // outlives every call made on it. `setVoiceProcessingEnabled:error:` is
    // called before `startAndReturnError:`, which Apple requires. The tap
    // block is copied by `installTapOnBus:…` and also kept in `Capture` so
    // it cannot be freed while installed; on each call it locks the slot and
    // appends the buffer to the request there, which Apple's SpeakToMe
    // sample does from the same thread the tap runs on, and the buffer
    // pointer is non-null for the duration of the tap call by that method's
    // contract. The slot's mutex is the only thing shared between the audio
    // thread and the worker, and Objective-C retain/release is thread-safe,
    // so the swap under the lock is sound. The configuration-change observer
    // is registered for this engine only and removed in `stop_capture`;
    // its block captures a `Sender` and reads nothing from the notification.
    // Failures surface as `NSError`. The meter reads the buffer after the
    // append: `floatChannelData` is null when the format is not float (the
    // tap is installed with the input node's own format, which is float on
    // the phone's inputs, but the null is checked rather than assumed); when
    // non-null it points at `stride`-interleaved samples of which
    // `frameLength * stride` are valid for the duration of the tap call, so
    // the slice built over them is read only inside the call and never
    // stored. The meter's mutex is touched by the audio thread alone; the
    // sink is `Send + Sync` by its type.
    let engine = unsafe { AVAudioEngine::new() };
    let input = unsafe { engine.inputNode() };
    unsafe { input.setVoiceProcessingEnabled_error(true) }
        .map_err(|error| error.localizedDescription().to_string())?;
    let format = unsafe { input.outputFormatForBus(0) };

    let slot: Arc<RequestSlot> = Arc::new(Mutex::new(None));
    let fed = Arc::clone(&slot);
    let meter = Mutex::new(Meter::new());
    let epoch = Instant::now();
    let tap = RcBlock::new(
        move |buffer: std::ptr::NonNull<AVAudioPCMBuffer>,
              _when: std::ptr::NonNull<AVAudioTime>| {
            let request = fed.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(request) = request.as_ref() {
                unsafe { request.appendAudioPCMBuffer(buffer.as_ref()) };
            }
            drop(request);
            let buffer = unsafe { buffer.as_ref() };
            let channels = unsafe { buffer.floatChannelData() };
            if channels.is_null() {
                return;
            }
            let frames = unsafe { buffer.frameLength() } as usize;
            let stride = unsafe { buffer.stride() }.max(1);
            let samples =
                unsafe { std::slice::from_raw_parts((*channels).as_ptr(), frames * stride) };
            let rms = level::rms(samples.iter().copied().step_by(stride));
            let reading = meter
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .feed(rms, epoch.elapsed());
            if let Some(level) = reading {
                sink(TurnEvent::Level(level));
            }
        },
    );
    unsafe {
        input.installTapOnBus_bufferSize_format_block(
            0,
            TAP_FRAMES,
            Some(&*format),
            RcBlock::as_ptr(&tap),
        )
    };
    unsafe { engine.prepare() };
    if let Err(error) = unsafe { engine.startAndReturnError() } {
        unsafe { input.removeTapOnBus(0) };
        return Err(error.localizedDescription().to_string());
    }

    let inbox = commands.clone();
    let on_change = RcBlock::new(move |_note: std::ptr::NonNull<NSNotification>| {
        let _ = inbox.send(Command::Audio(AudioNotice::EngineChanged));
    });
    let change_observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(AVAudioEngineConfigurationChangeNotification),
            Some(&engine),
            None,
            &on_change,
        )
    };
    Ok(Capture {
        engine,
        slot,
        _tap: tap,
        change_observer,
    })
}

/// Tear one capture down: observer off, tap off, engine stopped, slot
/// emptied.
#[allow(unsafe_code)]
fn stop_capture(capture: &Capture) {
    // SAFETY: the objects are the retained ones `start_capture` built;
    // `removeObserver:` takes the token that registration returned,
    // `removeTapOnBus:` and `stop` are documented as safe while the engine
    // runs and harmless when it does not.
    forget_observer(&capture.change_observer);
    let input = unsafe { capture.engine.inputNode() };
    unsafe { input.removeTapOnBus(0) };
    unsafe { capture.engine.stop() };
    *capture
        .slot
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

/// `AVAudioEngine.isRunning`.
#[allow(unsafe_code)]
fn engine_running(engine: &AVAudioEngine) -> bool {
    // SAFETY: a read-only property on the retained engine.
    unsafe { engine.isRunning() }
}

/// Start a fresh on-device recognition request on `slot` for `locale` —
/// the one `keeper_core::voice::locale::choose` answered, never the
/// system's — and the task that answers it through `handler`.
#[allow(unsafe_code)]
fn start_request(
    locale: &NSLocale,
    hints: &[String],
    handler: &RcBlock<dyn Fn(*mut SFSpeechRecognitionResult, *mut NSError)>,
    slot: &RequestSlot,
    serial: u64,
    last_heard: Arc<AtomicU64>,
) -> Result<Recognition, String> {
    // SAFETY: the request is freshly allocated and retained by the slot and
    // the returned `Recognition`; `requiresOnDeviceRecognition` is set before
    // the task is created, and the recogniser's `supportsOnDeviceRecognition`
    // was checked first, which is the precondition Apple names for the flag
    // to be honoured. The handler block is copied by the call. Placing the
    // request in the slot before the task exists means the first buffers
    // reach it, which Apple permits (audio may be appended before the task
    // starts).
    let Recognizer::Ready(recognizer) = recognizer_for(locale) else {
        return Err(format!(
            "no on-device recogniser for {}",
            locale.localeIdentifier()
        ));
    };
    let request = unsafe { SFSpeechAudioBufferRecognitionRequest::new() };
    unsafe { request.setRequiresOnDeviceRecognition(true) };
    unsafe { request.setShouldReportPartialResults(true) };
    if !hints.is_empty() {
        let strings: Vec<Retained<NSString>> =
            hints.iter().map(|hint| NSString::from_str(hint)).collect();
        unsafe { request.setContextualStrings(&NSArray::from_retained_slice(&strings)) };
    }
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(request.clone());
    let task = unsafe { recognizer.recognitionTaskWithRequest_resultHandler(&request, handler) };
    Ok(Recognition {
        _recognizer: recognizer,
        request,
        task,
        serial,
        started: Instant::now(),
        last_heard,
    })
}

/// End one request: audio ended, task cancelled. The capture stays.
#[allow(unsafe_code)]
fn end_request(recognition: &Recognition) {
    // SAFETY: the retained objects `start_request` built; `endAudio` and
    // `cancel` are idempotent. The cancelled task's handler receives an
    // error whose serial is stale by then.
    unsafe { recognition.request.endAudio() };
    unsafe { recognition.task.cancel() };
}

/// Read one recogniser callback: `Ok(Some((text, is_final)))`, `Ok(None)` for
/// a callback with nothing in it, `Err(reason)` for an error.
#[allow(unsafe_code)]
fn read_result(
    result: *mut SFSpeechRecognitionResult,
    error: *mut NSError,
) -> Result<Option<(String, bool)>, String> {
    // SAFETY: Apple's contract for `resultHandler` is that each pointer is
    // either nil or a valid object for the duration of the call; both are
    // null-checked and only read, never retained past the call.
    if let Some(error) = unsafe { error.as_ref() } {
        return Err(error.localizedDescription().to_string());
    }
    let Some(result) = (unsafe { result.as_ref() }) else {
        return Ok(None);
    };
    let text = unsafe { result.bestTranscription() };
    let text = unsafe { text.formattedString() }.to_string();
    let is_final = unsafe { result.isFinal() };
    Ok(Some((text, is_final)))
}

/// Watch the shared session for interruptions and media resets, posting each
/// to the worker's inbox.
#[allow(unsafe_code)]
fn observe_session(
    commands: &Sender<Command>,
) -> Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>> {
    // SAFETY: `addObserverForName:object:queue:usingBlock:` copies the block
    // and returns an opaque observer token that must be passed to
    // `removeObserver:` before it is dropped, which `forget_observer` does.
    // With a nil queue the block runs on the posting thread; it captures only
    // a `Sender`, reads the notification's `userInfo` (Apple: a dictionary
    // whose interruption-type value is an `NSNumber`) through null-checked
    // safe accessors, and never touches a worker-owned object. The
    // notification names are Apple's process-lifetime constants; a nil one
    // (which no iOS ships) means no observer rather than a null deref.
    let mut observers = Vec::with_capacity(2);
    let center = NSNotificationCenter::defaultCenter();
    let session = unsafe { AVAudioSession::sharedInstance() };

    if let Some(name) = unsafe { AVAudioSessionInterruptionNotification } {
        let inbox = commands.clone();
        let on_interruption = RcBlock::new(move |note: std::ptr::NonNull<NSNotification>| {
            let notification = unsafe { note.as_ref() };
            if let Some(notice) = interruption_notice(notification) {
                let _ = inbox.send(Command::Audio(notice));
            }
        });
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(name),
                Some(&session),
                None,
                &on_interruption,
            )
        };
        observers.push(token);
    }

    if let Some(name) = unsafe { AVAudioSessionMediaServicesWereResetNotification } {
        let inbox = commands.clone();
        let on_reset = RcBlock::new(move |_note: std::ptr::NonNull<NSNotification>| {
            let _ = inbox.send(Command::Audio(AudioNotice::MediaReset));
        });
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &on_reset)
        };
        observers.push(token);
    }
    observers
}

/// Read an interruption notification's `userInfo` into a notice.
#[allow(unsafe_code)]
fn interruption_notice(notification: &NSNotification) -> Option<AudioNotice> {
    // SAFETY: the two keys are Apple's process-lifetime `NSString`
    // constants, read once; every lookup below is a safe, null-checked
    // accessor on the retained dictionary, and the number is downcast before
    // it is read.
    let info = notification.userInfo()?;
    let type_key = unsafe { AVAudioSessionInterruptionTypeKey }?;
    let kind = info
        .objectForKey(type_key)
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .map(|number| AVAudioSessionInterruptionType(number.unsignedIntegerValue()))?;
    if kind == AVAudioSessionInterruptionType::Began {
        return Some(AudioNotice::Interrupted);
    }
    let should_resume = unsafe { AVAudioSessionInterruptionOptionKey }
        .and_then(|key| info.objectForKey(key))
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .map(|number| AVAudioSessionInterruptionOptions(number.unsignedIntegerValue()))
        .is_some_and(|options| options.contains(AVAudioSessionInterruptionOptions::ShouldResume));
    Some(AudioNotice::InterruptionEnded { should_resume })
}

/// `-[NSNotificationCenter removeObserver:]` for one registration token.
#[allow(unsafe_code)]
fn forget_observer(observer: &ProtocolObject<dyn NSObjectProtocol>) {
    // SAFETY: the token is the one registration returned, still retained by
    // the caller for this call; removing it twice is harmless.
    let center = NSNotificationCenter::defaultCenter();
    let object: &AnyObject = observer.as_ref();
    unsafe { center.removeObserver(object) };
}

/// `[[AVSpeechSynthesizer alloc] init]`.
#[allow(unsafe_code)]
fn new_synthesizer() -> Retained<AVSpeechSynthesizer> {
    // SAFETY: a plain allocation with no arguments.
    unsafe { AVSpeechSynthesizer::new() }
}

/// The languages of every installed voice (`+[AVSpeechSynthesisVoice
/// speechVoices]`), as the framework spells them (`pl-PL`), each once,
/// sorted; the caller caches the answer.
#[allow(unsafe_code)]
fn voice_languages() -> Vec<String> {
    // SAFETY: `speechVoices` returns an array the binding retains, and
    // `language` is a read-only property on each retained voice. Neither
    // prompts nor touches the network: the list is the voices installed on
    // this phone.
    let voices = unsafe { AVSpeechSynthesisVoice::speechVoices() };
    let mut languages: Vec<String> = voices
        .to_vec()
        .iter()
        .map(|voice| unsafe { voice.language() }.to_string())
        .collect();
    languages.sort_unstable();
    languages.dedup();
    languages
}

/// `+[AVSpeechSynthesisVoice voiceWithLanguage:]` for `language`: the
/// system's default voice for that language, or `None` when it has none —
/// which the caller refuses rather than letting the utterance take the
/// default voice of another language.
#[allow(unsafe_code)]
fn voice_for_language(language: &str) -> Option<Retained<AVSpeechSynthesisVoice>> {
    // SAFETY: a class method taking a BCP-47 string; it answers nil for a
    // language with no voice, which objc2 surfaces as `None`. It neither
    // prompts nor touches the network.
    unsafe { AVSpeechSynthesisVoice::voiceWithLanguage(Some(&NSString::from_str(language))) }
}

/// The voice's `name`, for the log line that says which voice spoke.
#[allow(unsafe_code)]
fn voice_name(voice: &AVSpeechSynthesisVoice) -> String {
    // SAFETY: a read-only property on the retained voice.
    unsafe { voice.name() }.to_string()
}

/// The dominant language of `text` by `NLLanguageRecognizer`, constrained
/// to `constraints` (language subtags, `pl`, `en`), or `None` when the
/// recogniser cannot tell. On-device: `NaturalLanguage` has no network
/// path, and no text leaves this phone to be classified (AD-188).
#[allow(unsafe_code)]
fn detect_language(text: &str, constraints: &[String]) -> Option<String> {
    if constraints.is_empty() {
        return None;
    }
    // SAFETY: a plain allocation; `setLanguageConstraints:` copies the
    // array of language strings; `processString:` reads the string and
    // `dominantLanguage` answers a retained string or nil. All are
    // documented on the object itself, and none prompt or touch the
    // network.
    let recognizer = unsafe { NLLanguageRecognizer::new() };
    let languages: Vec<Retained<NSString>> = constraints
        .iter()
        .map(|language| NSString::from_str(language))
        .collect();
    unsafe { recognizer.setLanguageConstraints(&NSArray::from_retained_slice(&languages)) };
    unsafe { recognizer.processString(&NSString::from_str(text)) };
    let dominant = unsafe { recognizer.dominantLanguage() }?.to_string();
    // `NLLanguageUndetermined` is spelled `und`; the binding may hand it
    // over rather than nil, and it means the same thing.
    (dominant != "und" && !dominant.is_empty()).then_some(dominant)
}

/// `speakUtterance:` with `voice` set on the utterance explicitly (Epic
/// 64, AD-182) — never the default voice, which is the language of the
/// system and not necessarily of the text.
#[allow(unsafe_code)]
fn speak_text(synthesizer: &AVSpeechSynthesizer, text: &str, voice: &AVSpeechSynthesisVoice) {
    // SAFETY: the utterance is freshly allocated from a Rust string and
    // retained across the call; `setVoice:` retains the voice on it;
    // `speakUtterance:` copies what it needs and queues it. All are
    // documented as callable from any thread.
    let utterance = unsafe {
        AVSpeechUtterance::initWithString(AVSpeechUtterance::alloc(), &NSString::from_str(text))
    };
    unsafe { utterance.setVoice(Some(voice)) };
    unsafe { synthesizer.speakUtterance(&utterance) };
}

/// `stopSpeakingAtBoundary:AVSpeechBoundaryImmediate` — mid-word, which is
/// what barge-in means.
#[allow(unsafe_code)]
fn stop_speech(synthesizer: &AVSpeechSynthesizer) {
    // SAFETY: a documented enum value on the retained synthesiser; returns
    // whether anything was stopped, which is not needed.
    let _ = unsafe { synthesizer.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate) };
}

/// `isSpeaking`: true while an utterance is queued or sounding.
#[allow(unsafe_code)]
fn is_speaking(synthesizer: &AVSpeechSynthesizer) -> bool {
    // SAFETY: a read-only property on the retained synthesiser.
    unsafe { synthesizer.isSpeaking() }
}
