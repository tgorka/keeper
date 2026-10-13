//! Story 97.3 #3 (AD-411, Q2, Q14): what a backchannel is, per language, and
//! what an utterance over an answer settles as.

use keeper_core::voice::backchannel::{at_speech_end, is_backchannel, settle, Utterance};

/// 97.3 #3: the list is the voice locale's language's. "no" is a
/// backchannel in Polish and an objection in English; "mhm" is one in
/// every language; two words are one only when the pair is listed; the
/// duration speaks only when there are no words, and only with the
/// models' measure of the speech.
#[test]
fn backchannel_list_is_per_language() {
    assert!(is_backchannel("no", Some(400), "pl"));
    assert!(is_backchannel("No.", None, "pl-PL"));
    assert!(!is_backchannel("no", Some(400), "en"));
    assert!(!is_backchannel("no", Some(400), "en-US"));
    for language in ["en-US", "pl-PL", "de-DE", ""] {
        assert!(is_backchannel("Mhm.", None, language), "{language}");
        assert!(is_backchannel("aha", Some(900), language), "{language}");
        assert!(
            !is_backchannel("mhm mhm", Some(400), language),
            "{language}"
        );
    }
    assert!(is_backchannel("Uh-huh.", None, "en-US"));
    assert!(is_backchannel("Tak!", None, "pl-PL"));
    assert!(!is_backchannel("tak", None, "en-US"));
    assert!(!is_backchannel("yeah right", Some(400), "en-US"));
    assert!(!is_backchannel("yeah", None, "de-DE"));
    // The duration: under 600 ms of wordless speech, with the models.
    assert!(is_backchannel("", Some(599), "en-US"));
    assert!(!is_backchannel("", Some(600), "en-US"));
    assert!(!is_backchannel(" ", None, "en-US"));
}

/// The utterance settled: wordless and short is a false onset, wordless
/// and under 600 ms a backchannel by its duration, wordless and long a
/// blank question (the turn ends); words are the list's.
#[test]
fn an_utterance_settles_by_its_words_then_its_duration() {
    assert_eq!(settle("", None, "en-US"), Utterance::FalseOnset);
    assert_eq!(settle("", Some(249), "en-US"), Utterance::FalseOnset);
    assert_eq!(
        settle("", Some(250), "en-US"),
        Utterance::BackchannelByDuration
    );
    assert_eq!(
        settle("", Some(599), "en-US"),
        Utterance::BackchannelByDuration
    );
    assert_eq!(
        settle("", Some(600), "en-US"),
        Utterance::Question(String::new())
    );
    assert_eq!(
        settle("yeah", Some(2_000), "en-US"),
        Utterance::BackchannelByWords
    );
    assert_eq!(
        settle(" no, wait ", Some(300), "en-US"),
        Utterance::Question("no, wait".to_owned())
    );
    assert_eq!(
        settle("no", Some(300), "en-US"),
        Utterance::Question("no".to_owned())
    );

    // At the speech end only the wordless short burst is decided.
    assert_eq!(at_speech_end("", 100), Some(Utterance::FalseOnset));
    assert_eq!(
        at_speech_end("", 400),
        Some(Utterance::BackchannelByDuration)
    );
    assert_eq!(at_speech_end("", 600), None);
    assert_eq!(at_speech_end("mhm", 300), None);
}
