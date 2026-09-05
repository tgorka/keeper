//! Story 63.3 (FR-415, FR-416, AD-175): the voice port's platform-neutral
//! half knows two platforms. The sentences are written once and filled with
//! the platform's nouns; the half-duplex rule is a pure function the turn
//! honours; and the iOS wording is pinned letter for letter so a change to
//! it has to be deliberate.

use std::sync::Mutex;

use keeper_core::vm::VoiceUnavailableVm;
use keeper_core::voice::{
    may_record, Effect, Turn, TurnEvent, TurnState, VoicePlatform, VoicePort, VoiceUnavailable,
    WakePhrase,
};

// ---------------------------------------------------------------------------
// A fake port that names its platform and records its calls.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Start,
    Stop,
    Speak(String),
    StopSpeaking,
}

struct FakePort {
    platform: VoicePlatform,
    calls: Mutex<Vec<Call>>,
}

impl FakePort {
    fn on(platform: VoicePlatform) -> Self {
        Self {
            platform,
            calls: Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("fake port lock").clone()
    }
    fn record(&self, call: Call) {
        self.calls.lock().expect("fake port lock").push(call);
    }
}

impl VoicePort for FakePort {
    fn platform(&self) -> VoicePlatform {
        self.platform
    }
    fn availability(&self) -> Result<(), VoiceUnavailable> {
        Ok(())
    }
    fn locales(&self) -> keeper_core::voice::locale::DeviceLocales {
        keeper_core::voice::locale::DeviceLocales::default()
    }
    fn set_locale(&self, _requested: Option<String>) {}
    fn start_listening(&self, _wake: Option<&WakePhrase>) -> Result<(), VoiceUnavailable> {
        self.record(Call::Start);
        Ok(())
    }
    fn stop_listening(&self) {
        self.record(Call::Stop);
    }
    fn voices(&self) -> Vec<String> {
        vec!["en-US".to_owned()]
    }
    fn listening(&self) -> String {
        "en-US".to_owned()
    }
    fn detect_language(&self, _text: &str, _constraints: &[String]) -> Option<String> {
        None
    }
    fn speak(&self, text: &str, _language: &str) -> Result<(), VoiceUnavailable> {
        self.record(Call::Speak(text.to_owned()));
        Ok(())
    }
    fn stop_speaking(&self) {
        self.record(Call::StopSpeaking);
    }
}

fn phrase() -> WakePhrase {
    WakePhrase::parse("hej keeper").expect("test phrase parses")
}

/// Every state the machine has, one of each.
fn every_state() -> Vec<TurnState> {
    vec![
        TurnState::Idle,
        TurnState::Listening {
            heard: String::new(),
        },
        TurnState::Listening {
            heard: "hej".to_owned(),
        },
        TurnState::Heard {
            text: "hello".to_owned(),
        },
        TurnState::Sending { answering: false },
        TurnState::Speaking,
        TurnState::Failed {
            reason: "boom".to_owned(),
        },
    ]
}

// ---------------------------------------------------------------------------
// The half-duplex rule (AD-175).
// ---------------------------------------------------------------------------

/// The truth table: every state on every platform. The only cell that
/// differs between platforms is `Speaking`, and it is the whole point — a
/// Mac speaking while armed must not be recording.
#[test]
fn voice_may_record_truth_table_covers_every_state() {
    for state in every_state() {
        let expected_ios = !matches!(state, TurnState::Failed { .. });
        let expected_half_duplex = expected_ios && !matches!(state, TurnState::Speaking);
        assert_eq!(
            may_record(&VoicePlatform::IOS, &state),
            expected_ios,
            "iOS, {state:?}"
        );
        assert_eq!(
            may_record(&VoicePlatform::MACOS, &state),
            expected_half_duplex,
            "macOS, {state:?}"
        );
        assert_eq!(
            may_record(&VoicePlatform::ABSENT, &state),
            expected_half_duplex,
            "absent port, {state:?}"
        );
    }
    // Read through a slice rather than asserted directly: these fields are consts, and
    // clippy refuses both a bare `assert!` on a constant and an `assert_eq!(_, true)`.
    // The fact is still pinned — full duplex is iOS's alone.
    let duplex: Vec<bool> = [
        VoicePlatform::IOS,
        VoicePlatform::MACOS,
        VoicePlatform::ABSENT,
    ]
    .iter()
    .map(|platform| platform.full_duplex)
    .collect();
    assert_eq!(duplex, vec![true, false, false], "only iOS is full duplex");
    assert!(!may_record(&VoicePlatform::MACOS, &TurnState::Speaking));
    assert!(may_record(&VoicePlatform::IOS, &TurnState::Speaking));
}

/// On iOS the rule agrees with what the port has always done: for every
/// state a driven turn passes through — armed, so `Idle` holds the device —
/// the microphone is open exactly when the rule says it may be. Nothing on
/// iOS changed.
#[test]
fn voice_may_record_agrees_with_the_ios_port_in_every_state() {
    let port = FakePort::on(VoicePlatform::IOS);
    let mut turn = Turn::new(VoicePlatform::IOS);
    let mut seen = Vec::new();
    let mut check = |turn: &Turn| {
        seen.push(turn.state().clone());
        assert_eq!(
            turn.microphone_open(),
            may_record(turn.platform(), turn.state()),
            "iOS device open != may_record in {:?}",
            turn.state()
        );
    };
    turn.set_wake(Some(phrase()));
    check(&turn);
    for event in [
        TurnEvent::WakeMatched,
        TurnEvent::PartialHeard("what".to_owned()),
        TurnEvent::FinalHeard("what time is it".to_owned()),
        TurnEvent::Sent,
        TurnEvent::AnswerDone("noon".to_owned()),
        TurnEvent::SpeechDetected("hey".to_owned()),
        TurnEvent::Failed("mic gone".to_owned()),
    ] {
        turn.drive(event, &port);
        check(&turn);
    }
    // Every state of the machine was visited along the way.
    for state in every_state() {
        assert!(
            seen.iter()
                .any(|s| std::mem::discriminant(s) == std::mem::discriminant(&state)),
            "the walk never reached {state:?}"
        );
    }
    // And the iOS path is the one epic 62 pinned: the answer is spoken over
    // an open microphone, and barge-in restarts recognition on it.
    assert_eq!(
        port.calls(),
        vec![
            Call::Start,
            Call::Speak("noon".to_owned()),
            Call::StopSpeaking,
            Call::Start,
            Call::Stop,
        ]
    );
}

/// Speaking while armed, on a half-duplex platform: the device held for the
/// phrase is released before the answer is spoken, the port never hears
/// itself, and the phrase is listened for again once the answer ends.
#[test]
fn voice_half_duplex_turn_releases_the_armed_microphone_before_speaking() {
    let port = FakePort::on(VoicePlatform::MACOS);
    let mut turn = Turn::new(VoicePlatform::MACOS);
    turn.set_wake(Some(phrase()));
    assert!(turn.microphone_open());

    let effects = turn.drive(TurnEvent::AnswerDone("It is noon.".to_owned()), &port);
    assert_eq!(
        effects,
        vec![
            Effect::ReleaseMicrophone,
            Effect::Speak("It is noon.".to_owned())
        ]
    );
    assert_eq!(turn.state(), &TurnState::Speaking);
    assert!(
        !turn.microphone_open(),
        "a Mac does not record its own answer"
    );

    let effects = turn.drive(TurnEvent::Silence, &port);
    assert_eq!(
        effects,
        vec![Effect::ReleaseMicrophone, Effect::OpenMicrophone]
    );
    assert_eq!(turn.state(), &TurnState::Idle);
    assert!(
        turn.microphone_open(),
        "re-armed for the phrase once the answer ended"
    );
    assert_eq!(
        port.calls(),
        vec![
            Call::Stop,
            Call::Speak("It is noon.".to_owned()),
            Call::Stop,
            Call::Start,
        ]
    );
}

/// The same rule through a whole spoken turn: the device opened for
/// listening is closed before the answer plays, and never opened just to
/// speak.
#[test]
fn voice_half_duplex_turn_never_records_while_speaking() {
    let port = FakePort::on(VoicePlatform::MACOS);
    let mut turn = Turn::new(VoicePlatform::MACOS);
    turn.drive(TurnEvent::WakeMatched, &port);
    turn.drive(TurnEvent::FinalHeard("hi".to_owned()), &port);
    turn.drive(TurnEvent::Sent, &port);
    let effects = turn.drive(TurnEvent::AnswerDone("hello".to_owned()), &port);
    assert_eq!(
        effects,
        vec![Effect::ReleaseMicrophone, Effect::Speak("hello".to_owned())]
    );
    assert!(!turn.microphone_open());
    turn.drive(TurnEvent::Silence, &port);
    assert_eq!(turn.state(), &TurnState::Idle);
    assert!(!turn.microphone_open(), "no phrase set: nothing to re-arm");

    // An answer to a typed message, with nothing armed: only the speech.
    let effects = turn.drive(TurnEvent::AnswerDone("typed".to_owned()), &port);
    assert_eq!(effects, vec![Effect::Speak("typed".to_owned())]);
    assert!(!turn.microphone_open());
    assert!(
        !port.calls().windows(2).any(|pair| matches!(
            pair,
            [Call::Start, Call::Speak(_)] | [Call::Speak(_), Call::Start]
        )),
        "the device was open around a speech: {:?}",
        port.calls()
    );
}

/// A half-duplex platform still stops the speech first when the port does
/// report speech — and only then opens the device to listen.
#[test]
fn voice_half_duplex_barge_in_still_stops_speech_before_listening() {
    let mut turn = Turn::new(VoicePlatform::MACOS);
    turn.apply(TurnEvent::AnswerDone("long".to_owned()));
    let effects = turn.apply(TurnEvent::SpeechDetected("hey".to_owned()));
    assert_eq!(effects, vec![Effect::StopSpeaking, Effect::OpenMicrophone]);
    assert!(turn.microphone_open());
}

// ---------------------------------------------------------------------------
// The sentences.
// ---------------------------------------------------------------------------

fn no_model(locale: &str) -> VoiceUnavailable {
    VoiceUnavailable::NoOnDeviceModel {
        locale: locale.to_owned(),
    }
}

fn no_recognition(locale: &str) -> VoiceUnavailable {
    no_recognition_with(locale, &["en-ID", "en-PH", "en-SA", "en-US"])
}

fn no_recognition_with(locale: &str, on_device: &[&str]) -> VoiceUnavailable {
    VoiceUnavailable::NoOnDeviceRecognition {
        locale: locale.to_owned(),
        on_device: on_device.iter().map(|id| (*id).to_owned()).collect(),
    }
}

/// Every iOS sentence, letter for letter as epic 62 shipped it. A change
/// here is a change the phone's surface, `docs/ios.md` and the frontend
/// fixtures all see; it must be deliberate.
#[test]
fn voice_ios_sentences_are_unchanged() {
    let ios = VoicePlatform::IOS;
    assert_eq!(
        VoiceUnavailable::NotAuthorized.message(&ios),
        "keeper is not allowed to use the microphone or speech recognition on this phone — allow both under Settings > keeper"
    );
    assert_eq!(
        no_model("pl_PL").message(&ios),
        "on-device speech recognition for pl_PL is not on this phone — download that language under Settings > General > Keyboard > Dictation Languages; keeper never sends your voice to a server"
    );
    assert_eq!(
        VoiceUnavailable::NoMicrophone.message(&ios),
        "no microphone is available on this device"
    );
    assert_eq!(
        VoiceUnavailable::NoRecognizer.message(&ios),
        "this build of keeper was made without Apple's Speech framework, so it cannot recognise speech on any phone — no setting or language download changes that; install a build whose Xcode project links Speech.framework"
    );
    assert_eq!(
        VoiceUnavailable::Unsupported.message(&ios),
        "voice is not available in this build"
    );
}

/// The macOS absence states, each worded for the Mac from the verified
/// facts: the microphone grant under Privacy & Security, the on-device
/// model under Keyboard > Dictation.
#[test]
fn voice_macos_sentences_name_the_mac_and_its_settings() {
    let mac = VoicePlatform::MACOS;
    assert_eq!(
        VoiceUnavailable::NotAuthorized.message(&mac),
        "keeper is not allowed to use the microphone or speech recognition on this Mac — allow the microphone under System Settings > Privacy & Security > Microphone"
    );
    assert_eq!(
        no_model("pl_PL").message(&mac),
        "on-device speech recognition for pl_PL is not on this Mac — turn Dictation on and download that language under System Settings > Keyboard > Dictation; keeper never sends your voice to a server"
    );
    assert_eq!(
        no_recognition("pl-PL").message(&mac),
        "this Mac has no on-device speech recognition for pl-PL, and keeper never sends your voice to a server — turn Dictation on and download that language under System Settings > Keyboard > Dictation, which may add it, or choose a language this Mac can already run on its own: en-ID, en-PH, en-SA, en-US"
    );
    assert_eq!(
        no_recognition_with("pl-PL", &[]).message(&mac),
        "this Mac has no on-device speech recognition for pl-PL or for any other language, and keeper never sends your voice to a server — turn Dictation on and download that language under System Settings > Keyboard > Dictation, which may add it"
    );
    assert_eq!(
        VoiceUnavailable::NoMicrophone.message(&mac),
        "no microphone is available on this device"
    );
    assert_eq!(
        VoiceUnavailable::NoRecognizer.message(&mac),
        "this build of keeper was made without Apple's Speech framework, so it cannot recognise speech on any Mac — no setting or language download changes that; install a build whose Xcode project links Speech.framework"
    );
    assert_eq!(
        VoiceUnavailable::Unsupported.message(&mac),
        "voice is not available in this build"
    );
    for why in [
        VoiceUnavailable::NotAuthorized,
        no_model("pl_PL"),
        no_recognition("pl_PL"),
        VoiceUnavailable::NoRecognizer,
    ] {
        let message = why.message(&mac);
        for phone in ["this phone", "any phone"] {
            assert!(
                !message.contains(phone),
                "{why:?} on a Mac says {phone:?}: {message}"
            );
        }
    }
}

/// The two "no on-device model" causes are distinct absences: one sends the
/// person to a download that will add the model, the other says the OS has
/// no on-device asset for the language — the download *may* add one, and
/// the languages this device can already run are named so the person can
/// pick one instead. Neither offers a server, on either platform, and
/// neither claims a certainty the evidence does not give: the old sentence
/// said "downloading a language does not change that", which the OS's own
/// log ("No Assistant asset for language pl-PL") does not support.
#[test]
fn voice_no_on_device_recognition_is_distinct_from_no_model() {
    for platform in [VoicePlatform::IOS, VoicePlatform::MACOS] {
        let download = no_model("pl-PL").message(&platform);
        let asset = no_recognition("pl-PL").message(&platform);
        assert_ne!(download, asset);
        assert!(download.contains("download that language"), "{download}");
        assert!(asset.contains("download that language"), "{asset}");
        assert!(asset.contains("which may add it"), "{asset}");
        assert!(!asset.contains("does not change"), "{asset}");
        assert!(!asset.contains("only through a server"), "{asset}");
        for message in [&download, &asset] {
            assert!(message.contains("pl-PL"), "{message}");
            assert!(
                message.contains("keeper never sends your voice to a server"),
                "{message}"
            );
        }
    }
    match no_recognition("pl-PL").vm(&VoicePlatform::MACOS) {
        VoiceUnavailableVm::NoOnDeviceRecognition { locale, message } => {
            assert_eq!(locale, "pl-PL");
            assert_eq!(
                message,
                no_recognition("pl-PL").message(&VoicePlatform::MACOS)
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        no_model("pl-PL").vm(&VoicePlatform::MACOS),
        VoiceUnavailableVm::NoOnDeviceModel { .. }
    ));
}

/// The refusal for a language that cannot run names every language that
/// can, in the port's order, on every platform — and when none can, says
/// so instead of offering an empty choice.
#[test]
fn voice_no_on_device_recognition_names_the_languages_that_can_run() {
    for platform in [
        VoicePlatform::IOS,
        VoicePlatform::MACOS,
        VoicePlatform::ABSENT,
    ] {
        let message = no_recognition("pl-PL").message(&platform);
        assert!(
            message.ends_with("can already run on its own: en-ID, en-PH, en-SA, en-US"),
            "{message}"
        );
        let none = no_recognition_with("pl-PL", &[]).message(&platform);
        assert!(none.contains("or for any other language"), "{none}");
        assert!(!none.contains("choose a language"), "{none}");
        assert!(!none.contains(':'), "{none}");
    }
}

/// No sentence exists twice: a platform contributes nouns, and the words
/// around them are shared. Removing the platform's own clauses from each
/// platform's sentence leaves the same skeleton.
#[test]
fn voice_sentences_share_one_skeleton_across_platforms() {
    fn skeleton(message: &str, platform: &VoicePlatform) -> String {
        message
            .replace(platform.allow, "{allow}")
            .replace(platform.download, "{download}")
            .replace(&format!("this {}", platform.noun), "this {noun}")
            .replace(&format!("any {}", platform.noun), "any {noun}")
    }
    // The absent platform's noun is "device", the same word the noun-free
    // `NoMicrophone` sentence uses on every platform, so it is compared
    // against the Mac's skeleton after the Mac has been compared to iOS.
    for why in [
        VoiceUnavailable::NotAuthorized,
        no_model("pl_PL"),
        no_recognition("pl_PL"),
        VoiceUnavailable::NoMicrophone,
        VoiceUnavailable::NoRecognizer,
        VoiceUnavailable::Unsupported,
    ] {
        let ios = skeleton(&why.message(&VoicePlatform::IOS), &VoicePlatform::IOS);
        let mac = skeleton(&why.message(&VoicePlatform::MACOS), &VoicePlatform::MACOS);
        let absent = skeleton(&why.message(&VoicePlatform::ABSENT), &VoicePlatform::ABSENT);
        assert_eq!(ios, mac, "{why:?}");
        assert_eq!(skeleton(&mac, &VoicePlatform::ABSENT), absent, "{why:?}");
    }
}

/// A port with no platform to name still has a vocabulary, and it names no
/// operating system: the absent port's one sentence is the same everywhere.
#[test]
fn voice_absent_platform_names_no_os() {
    let absent = VoicePlatform::ABSENT;
    for word in ["iOS", "macOS", "phone", "Mac", "Apple"] {
        assert!(!absent.noun.contains(word), "{word}");
        assert!(!absent.allow.contains(word), "{word}");
        assert!(!absent.download.contains(word), "{word}");
    }
    assert_eq!(
        VoiceUnavailable::Unsupported.message(&absent),
        VoiceUnavailable::Unsupported.message(&VoicePlatform::IOS)
    );
}

/// A refusal the port raises mid-turn becomes the turn's `Failed` reason in
/// the platform's words, so a Mac that refuses says "this Mac".
#[test]
fn voice_turn_failure_reason_uses_the_turn_platform() {
    struct Refusing;
    impl VoicePort for Refusing {
        fn platform(&self) -> VoicePlatform {
            VoicePlatform::MACOS
        }
        fn availability(&self) -> Result<(), VoiceUnavailable> {
            Ok(())
        }
        fn locales(&self) -> keeper_core::voice::locale::DeviceLocales {
            keeper_core::voice::locale::DeviceLocales::default()
        }
        fn set_locale(&self, _requested: Option<String>) {}
        fn start_listening(&self, _wake: Option<&WakePhrase>) -> Result<(), VoiceUnavailable> {
            Err(VoiceUnavailable::NotAuthorized)
        }
        fn stop_listening(&self) {}
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
            Ok(())
        }
        fn stop_speaking(&self) {}
    }
    let port = Refusing;
    let mut turn = Turn::new(port.platform());
    turn.drive(TurnEvent::WakeMatched, &port);
    match turn.state() {
        TurnState::Failed { reason } => {
            assert_eq!(
                reason,
                &VoiceUnavailable::NotAuthorized.message(&VoicePlatform::MACOS)
            );
            assert!(reason.contains("this Mac"), "{reason}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}
