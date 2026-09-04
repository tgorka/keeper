//! Reach: starting a voice turn while keeper is not in front (Epic 63,
//! Story 63.5, FR-420–FR-422, AD-174, AD-179).
//!
//! Three shell surfaces can start a turn without the window — an OS-global
//! hotkey, a menu-bar tray item and the `keeper://voice/talk` deep link — and
//! a fourth, the Settings section, is where the wake switch and the chord
//! live. Every decision the four share is here, so the shell is a call site
//! (AD-55/AD-56) and a fact this crate can test on any host is not four
//! copies of itself in a crate that compiles only on macOS:
//!
//! - **Whether a surface exists at all** ([`reach_present`]): the one runtime
//!   answer from `voice_availability`, and only `unsupported` is absence
//!   (AD-179, AD-27). `CapabilitiesVm` has no voice field and gains none.
//! - **What a surface shows** ([`VoiceFace`], [`tray_voice_labels`]): the
//!   three faces the mic control already wears — idle, listening, speaking —
//!   projected from the streamed [`VoiceStateVm`], with the tray's words
//!   composed here rather than in the tray.
//! - **What a press does** ([`reach_verb`]): a hotkey press or a deep link
//!   *asks to talk* — it starts a turn only from idle, so a second press, a
//!   Shortcut that fires twice or a held key never restarts or resets the
//!   silence budget of a turn already open (FR-422's idempotence). The tray
//!   item *toggles* — it is the one surface that also stops, because its label
//!   says which it will do.
//! - **Which link is voice's** ([`voice_link`], [`voice_link_ask`]): the
//!   grammar of `keeper://voice/…`, and the rule that one deep-link event
//!   asks at most once however many URLs it carries.
//!
//! What is deliberately not here: the Touch Bar. AD-174 refuses it — the
//! Control Strip accepts no third-party item, and reaching it means private
//! `DFRFoundation` in a build with library validation off — and the deep link
//! is the supported answer: a Shortcut placed on the Touch Bar's own Quick
//! Actions button opens `keeper://voice/talk`.

use crate::vm::{VoiceStateVm, VoiceUnavailableVm};

/// The deep link a Shortcut opens to start a turn (FR-422).
pub const VOICE_TALK_LINK: &str = "keeper://voice/talk";

/// Whether the reach surfaces exist, from the one runtime answer every voice
/// surface reads (AD-179). `None` is "voice works"; every refusal but
/// `unsupported` is a *state* — a permission to grant, a language to download
/// — with the surface present to say so, exactly as the mic control and the
/// wake switch decide it. Only a build without a port has nothing to show
/// (AD-27: absent, never disabled).
pub fn reach_present(availability: Option<&VoiceUnavailableVm>) -> bool {
    !matches!(availability, Some(VoiceUnavailableVm::Unsupported { .. }))
}

/// Which of the three faces a reach surface wears for a snapshot — the same
/// three the mic control wears (`micState` in `bot-voice-mic.tsx`): `heard`,
/// `sending` and `failed` are turns with the microphone already released, so
/// they show the idle face and offer a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceFace {
    /// No turn, or one past the point where the microphone is open.
    Idle,
    /// The microphone is open for a turn.
    Listening,
    /// The answer is being read aloud.
    Speaking,
}

impl From<&VoiceStateVm> for VoiceFace {
    fn from(state: &VoiceStateVm) -> Self {
        match state {
            VoiceStateVm::Listening { .. } => Self::Listening,
            VoiceStateVm::Speaking => Self::Speaking,
            VoiceStateVm::Idle { .. }
            | VoiceStateVm::Heard { .. }
            | VoiceStateVm::Sending
            | VoiceStateVm::Failed { .. } => Self::Idle,
        }
    }
}

/// What a surface asks for when pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachAsk {
    /// Start a turn if none is open; otherwise nothing. The hotkey and the
    /// deep link: a person reaching for keeper from another app wants to be
    /// heard, and a repeat of the same reach must not undo that.
    Talk,
    /// Start from idle, stop while listening, stop the answer while speaking.
    /// The tray item, whose label names which of the three it will do.
    Toggle,
}

/// What the shell does about an ask, in `voice_ipc`'s own verbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachVerb {
    /// `voice_start`: the same `WakeMatched` a matched phrase produces.
    Start,
    /// `voice_stop`: abandon the turn, release the microphone (NFR-51).
    Stop,
    /// `voice_stop_speaking`: end the answer as if it had finished.
    StopSpeaking,
}

/// The verb for an ask against the face the turn currently shows, or `None`
/// when the ask is already satisfied.
pub fn reach_verb(face: VoiceFace, ask: ReachAsk) -> Option<ReachVerb> {
    match (ask, face) {
        (_, VoiceFace::Idle) => Some(ReachVerb::Start),
        (ReachAsk::Talk, _) => None,
        (ReachAsk::Toggle, VoiceFace::Listening) => Some(ReachVerb::Stop),
        (ReachAsk::Toggle, VoiceFace::Speaking) => Some(ReachVerb::StopSpeaking),
    }
}

/// The two lines the tray's voice section shows: a status line that follows
/// the turn, and the verb the item performs when chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayVoiceLabels {
    /// Where the turn is, in a sentence (disabled line).
    pub status: String,
    /// What choosing the item does: the mic control's own three words.
    pub verb: &'static str,
}

/// The tray's words for a snapshot (FR-421). The status follows every state
/// the turn has, not only the three faces, because the tray is where a person
/// looks while keeper is hidden and "idle" over a message that is with the
/// model would be the tray lying in the direction that matters. The verb is
/// the face's, so it always names what [`reach_verb`] will do for a toggle.
pub fn tray_voice_labels(state: &VoiceStateVm) -> TrayVoiceLabels {
    let status = match state {
        VoiceStateVm::Idle {
            wake: Some(phrase),
            listening_for_wake: true,
        } => format!("Listening for \"{phrase}\""),
        VoiceStateVm::Idle { .. } => "Not listening".to_owned(),
        VoiceStateVm::Listening { heard } if heard.trim().is_empty() => "Listening".to_owned(),
        VoiceStateVm::Listening { heard } => format!("Listening: {}", heard.trim()),
        VoiceStateVm::Heard { .. } => "Heard".to_owned(),
        VoiceStateVm::Sending => "Sending what you said".to_owned(),
        VoiceStateVm::Speaking => "Speaking the answer".to_owned(),
        VoiceStateVm::Failed { reason } => format!("Could not listen: {reason}"),
    };
    let verb = match VoiceFace::from(state) {
        VoiceFace::Idle => "Talk",
        VoiceFace::Listening => "Cancel this question",
        VoiceFace::Speaking => "Stop this answer",
    };
    TrayVoiceLabels { status, verb }
}

/// What a `keeper://voice/…` link asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceLink {
    /// `keeper://voice/talk`: start a turn.
    Talk,
}

/// Parse one deep link. `None` for every URL that is not voice's — the OAuth
/// callback, a typo, another scheme — so the caller can hand it on. The path
/// is read leniently: a trailing slash, a query and a fragment are what a
/// Shortcut's URL field or a browser add, and none of them change the ask.
pub fn voice_link(url: &str) -> Option<VoiceLink> {
    let rest = url.strip_prefix("keeper://")?;
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if !host.eq_ignore_ascii_case("voice") {
        return None;
    }
    match path.trim_end_matches('/') {
        "talk" => Some(VoiceLink::Talk),
        _ => None,
    }
}

/// One deep-link event's URLs, reduced to at most one ask. macOS delivers
/// every URL opened in one gesture as one event; a Shortcut that opens the
/// link twice, or a person who double-clicks, asks once.
pub fn voice_link_ask<'a>(urls: impl IntoIterator<Item = &'a str>) -> Option<ReachAsk> {
    urls.into_iter()
        .find_map(voice_link)
        .map(|VoiceLink::Talk| ReachAsk::Talk)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::voice::{Turn, TurnEvent, VoicePlatform, VoicePort, VoiceUnavailable, WakePhrase};

    fn unsupported() -> VoiceUnavailableVm {
        VoiceUnavailableVm::Unsupported {
            message: "voice is not available in this build".to_owned(),
        }
    }

    fn not_authorized() -> VoiceUnavailableVm {
        VoiceUnavailableVm::NotAuthorized {
            message: "allow it".to_owned(),
        }
    }

    #[test]
    fn surfaces_exist_unless_the_answer_is_unsupported() {
        assert!(reach_present(None), "voice works: every surface exists");
        assert!(
            reach_present(Some(&not_authorized())),
            "a permission to grant is a state the surface shows, not absence"
        );
        assert!(
            reach_present(Some(&VoiceUnavailableVm::NoMicrophone {
                message: "no microphone".to_owned(),
            })),
            "no microphone is a state too"
        );
        assert!(
            !reach_present(Some(&unsupported())),
            "a build without a port has nothing to show (AD-27)"
        );
    }

    #[test]
    fn the_face_is_the_mic_controls() {
        let idle = VoiceStateVm::Idle {
            wake: None,
            listening_for_wake: false,
        };
        assert_eq!(VoiceFace::from(&idle), VoiceFace::Idle);
        assert_eq!(
            VoiceFace::from(&VoiceStateVm::Listening {
                heard: String::new()
            }),
            VoiceFace::Listening
        );
        assert_eq!(
            VoiceFace::from(&VoiceStateVm::Speaking),
            VoiceFace::Speaking
        );
        // The microphone is released in these three, so they offer a start.
        for released in [
            VoiceStateVm::Heard {
                text: "hi".to_owned(),
            },
            VoiceStateVm::Sending,
            VoiceStateVm::Failed {
                reason: "nope".to_owned(),
            },
        ] {
            assert_eq!(VoiceFace::from(&released), VoiceFace::Idle, "{released:?}");
        }
    }

    #[test]
    fn talk_starts_only_from_idle_and_toggle_stops_the_rest() {
        assert_eq!(
            reach_verb(VoiceFace::Idle, ReachAsk::Talk),
            Some(ReachVerb::Start)
        );
        assert_eq!(reach_verb(VoiceFace::Listening, ReachAsk::Talk), None);
        assert_eq!(reach_verb(VoiceFace::Speaking, ReachAsk::Talk), None);
        assert_eq!(
            reach_verb(VoiceFace::Idle, ReachAsk::Toggle),
            Some(ReachVerb::Start)
        );
        assert_eq!(
            reach_verb(VoiceFace::Listening, ReachAsk::Toggle),
            Some(ReachVerb::Stop)
        );
        assert_eq!(
            reach_verb(VoiceFace::Speaking, ReachAsk::Toggle),
            Some(ReachVerb::StopSpeaking)
        );
    }

    #[test]
    fn the_tray_label_follows_idle_listening_speaking() {
        let idle = tray_voice_labels(&VoiceStateVm::Idle {
            wake: None,
            listening_for_wake: false,
        });
        assert_eq!(idle.status, "Not listening");
        assert_eq!(idle.verb, "Talk");

        let armed = tray_voice_labels(&VoiceStateVm::Idle {
            wake: Some("hey nixie".to_owned()),
            listening_for_wake: true,
        });
        assert_eq!(armed.status, "Listening for \"hey nixie\"");
        assert_eq!(
            armed.verb, "Talk",
            "the phrase is not a turn; a press starts one"
        );

        let listening = tray_voice_labels(&VoiceStateVm::Listening {
            heard: String::new(),
        });
        assert_eq!(listening.status, "Listening");
        assert_eq!(listening.verb, "Cancel this question");

        let forming = tray_voice_labels(&VoiceStateVm::Listening {
            heard: " what time ".to_owned(),
        });
        assert_eq!(forming.status, "Listening: what time");

        let speaking = tray_voice_labels(&VoiceStateVm::Speaking);
        assert_eq!(speaking.status, "Speaking the answer");
        assert_eq!(speaking.verb, "Stop this answer");

        let sending = tray_voice_labels(&VoiceStateVm::Sending);
        assert_eq!(sending.status, "Sending what you said");
        assert_eq!(sending.verb, "Talk");

        let failed = tray_voice_labels(&VoiceStateVm::Failed {
            reason: "the microphone went away".to_owned(),
        });
        assert_eq!(failed.status, "Could not listen: the microphone went away");
        assert_eq!(failed.verb, "Talk");
    }

    #[test]
    fn the_talk_link_is_read_leniently_and_nothing_else_is_voices() {
        assert_eq!(voice_link(VOICE_TALK_LINK), Some(VoiceLink::Talk));
        assert_eq!(voice_link("keeper://voice/talk/"), Some(VoiceLink::Talk));
        assert_eq!(voice_link("keeper://VOICE/talk?x=1"), Some(VoiceLink::Talk));
        assert_eq!(voice_link("keeper://voice/talk#top"), Some(VoiceLink::Talk));
        assert_eq!(voice_link("keeper://voice"), None);
        assert_eq!(voice_link("keeper://voice/"), None);
        assert_eq!(voice_link("keeper://voice/talking"), None);
        assert_eq!(voice_link("keeper://voice/stop"), None);
        assert_eq!(voice_link("keeper://oauth/callback?state=abc"), None);
        assert_eq!(voice_link("https://example.test/voice/talk"), None);
        assert_eq!(voice_link(""), None);
    }

    #[test]
    fn one_event_asks_at_most_once() {
        assert_eq!(
            voice_link_ask([VOICE_TALK_LINK, VOICE_TALK_LINK, "keeper://voice/talk/"]),
            Some(ReachAsk::Talk)
        );
        assert_eq!(
            voice_link_ask(["keeper://oauth/callback?state=x", VOICE_TALK_LINK]),
            Some(ReachAsk::Talk)
        );
        assert_eq!(voice_link_ask(["keeper://oauth/callback?state=x"]), None);
        assert_eq!(voice_link_ask(std::iter::empty()), None);
    }

    /// A port that opens whenever asked and counts how often it was.
    struct CountingPort {
        opened: AtomicUsize,
    }

    impl VoicePort for CountingPort {
        fn platform(&self) -> VoicePlatform {
            VoicePlatform::MACOS
        }
        fn availability(&self) -> Result<(), VoiceUnavailable> {
            Ok(())
        }
        fn locales(&self) -> crate::voice::locale::DeviceLocales {
            crate::voice::locale::DeviceLocales::default()
        }
        fn set_locale(&self, _requested: Option<String>) {}
        fn start_listening(&self, _wake: Option<&WakePhrase>) -> Result<(), VoiceUnavailable> {
            self.opened.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn stop_listening(&self) {}
        fn speak(&self, _text: &str) -> Result<(), VoiceUnavailable> {
            Ok(())
        }
        fn stop_speaking(&self) {}
    }

    /// Drive one ask the way the shell does: read the face, decide, act.
    fn reach(turn: &mut Turn, port: &dyn VoicePort, ask: ReachAsk) {
        let face = VoiceFace::from(&turn.vm());
        match reach_verb(face, ask) {
            Some(ReachVerb::Start) => {
                turn.drive(TurnEvent::WakeMatched, port);
            }
            Some(ReachVerb::Stop) => {
                turn.drive(TurnEvent::Abandoned, port);
            }
            Some(ReachVerb::StopSpeaking) => {
                turn.drive(TurnEvent::Silence, port);
            }
            None => {}
        }
    }

    #[test]
    fn a_deep_link_fired_twice_starts_exactly_one_turn() {
        let port = CountingPort {
            opened: AtomicUsize::new(0),
        };
        let mut turn = Turn::new(port.platform());
        // Two URLs in one event: one ask.
        let ask = voice_link_ask([VOICE_TALK_LINK, VOICE_TALK_LINK]).expect("a talk ask");
        reach(&mut turn, &port, ask);
        // A second event a moment later, the turn still open: no second start.
        let ask = voice_link_ask([VOICE_TALK_LINK]).expect("a talk ask");
        reach(&mut turn, &port, ask);
        assert_eq!(port.opened.load(Ordering::SeqCst), 1, "one microphone open");
        assert!(
            matches!(turn.state(), crate::voice::TurnState::Listening { .. }),
            "still the first turn: {:?}",
            turn.state()
        );
    }

    #[test]
    fn the_tray_toggle_stops_what_it_started() {
        let port = CountingPort {
            opened: AtomicUsize::new(0),
        };
        let mut turn = Turn::new(port.platform());
        reach(&mut turn, &port, ReachAsk::Toggle);
        assert!(matches!(
            turn.state(),
            crate::voice::TurnState::Listening { .. }
        ));
        reach(&mut turn, &port, ReachAsk::Toggle);
        assert_eq!(turn.state(), &crate::voice::TurnState::Idle);
        assert_eq!(port.opened.load(Ordering::SeqCst), 1);
    }
}
