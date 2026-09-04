//! Which voice reads an answer aloud, and what the model is told first
//! (Epic 64, Story 64.2, AD-182, AD-183, AD-188).
//!
//! Recognition and synthesis are different inventories. The Mac this was
//! measured on recognises four locales on its own, all English, and has 180
//! synthesiser voices, one of them Polish. So a person listening in English
//! whose bot answers in Polish *can* hear that answer in Polish — the epic
//! opened with the same Mac reading it in the default English voice, which
//! was unintelligible, because nothing chose a voice at all.
//!
//! Two halves, both here as pure functions the shell's port hands its facts
//! to. *Before the turn:* [`answer_instruction`] is the sentence a
//! voice-originated request carries, asking the model to answer in the
//! language the person is speaking. *After the answer:* [`choose_voice`]
//! takes what the port detected as the text's dominant language, the
//! listening locale, and the languages this device has voices for, and
//! answers the language to speak in — or the refusal that says the answer
//! stays on the screen and where a voice is downloaded.
//!
//! # The rules
//!
//! - **A detected language with a voice is spoken in that voice.** Among
//!   that language's voices, the listening locale's own wins (`en-US` over
//!   `en-GB` for an `en-US` listener), otherwise the first in the port's
//!   sorted list, so the answer is the same on every launch.
//! - **Undetermined means the listening language.** The detector could not
//!   tell — a short answer, a list of numbers — and the person's own
//!   language is the best guess a device can make. The listening locale's
//!   voice, or one sharing its language.
//! - **A detected language with no voice is a refusal, never a fallback.**
//!   [`super::VoiceUnavailable::NoVoice`] names the language and the
//!   platform's download page. The turn ends with the answer on the screen,
//!   which is what a person can act on; a wrong voice is not.
//! - **No voice at all for the listening language is the same refusal**,
//!   naming the listening language — a device that can listen in a language
//!   ordinarily speaks it, so this is the odd case, and it is said plainly.
//!
//! # Identifiers
//!
//! A detector answers language subtags (`pl`, `en`, `zh-Hans`); a
//! synthesiser lists voices by locale (`pl-PL`, `en-US`, `zh-CN`). The two
//! meet at [`locale::language`], the lowercased first subtag. Nothing here
//! is a platform `cfg` or a model weight (AD-183): the detector and the
//! inventory are the port's, the decision is this file's.

use super::locale::{self, same};
use super::VoiceUnavailable;

/// The language subtags the detector may answer from: every language this
/// device has a voice for, plus the listening language, each once, in the
/// order they were met. The listening language is included so a detector
/// on a device with no voice for it still names it — and the refusal then
/// names it too, rather than misreading the text as the nearest language
/// that does have a voice.
pub fn constraints(listening: &str, voices: &[String]) -> Vec<String> {
    let mut languages: Vec<String> = Vec::with_capacity(voices.len() + 1);
    for tag in voices.iter().map(String::as_str).chain([listening]) {
        let language = locale::language(tag);
        if !language.is_empty() && !languages.contains(&language) {
            languages.push(language);
        }
    }
    languages
}

/// The language to speak `text` in — an element of `voices`, spelled as the
/// port spelled it — or the refusal.
///
/// `detected` is the detector's answer for the text (`None` when it could
/// not tell), `listening` the locale recognition ran in, `voices` the
/// languages this device has voices for in the port's sorted order.
pub fn choose_voice(
    detected: Option<&str>,
    listening: &str,
    voices: &[String],
) -> Result<String, VoiceUnavailable> {
    let wanted = detected
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(listening);
    voice_for(wanted, listening, voices).ok_or_else(|| VoiceUnavailable::NoVoice {
        language: locale::canonical(wanted),
    })
}

/// The voice for `wanted`'s language: the listening locale's own where the
/// languages agree, otherwise the first of that language.
fn voice_for(wanted: &str, listening: &str, voices: &[String]) -> Option<String> {
    let tongue = locale::language(wanted);
    if tongue.is_empty() {
        return None;
    }
    let mut candidates = voices
        .iter()
        .filter(|voice| locale::language(voice) == tongue);
    let first = candidates.next()?;
    if same(first, listening) {
        return Some(first.clone());
    }
    Some(
        candidates
            .find(|voice| same(voice, listening))
            .unwrap_or(first)
            .clone(),
    )
}

/// The per-turn instruction a voice-originated request carries (AD-182):
/// answer in the language the person is speaking, named in the model's own
/// terms — the English name where this file knows it, the tag beside it
/// either way, so `pl-PL` is never mistaken for a country.
pub fn answer_instruction(listening: &str) -> String {
    format!(
        "The person asked this aloud and your answer will be read aloud to them. Answer in {}.",
        describe(listening)
    )
}

/// `tag` as a sentence names it: "Polish (pl-PL)", or "the language tagged
/// xx-XX" where the name is not known here. Public for the refusal's
/// sentence, which names the language the same way.
pub fn describe(tag: &str) -> String {
    let canonical = locale::canonical(tag);
    match language_name(&locale::language(tag)) {
        Some(name) => format!("{name} ({canonical})"),
        None => format!("the language tagged {canonical}"),
    }
}

/// The English name of a language subtag, for the languages Apple's
/// dictation and synthesiser inventories cover. Not a locale database: a
/// subtag missing here is still named by its tag.
fn language_name(subtag: &str) -> Option<&'static str> {
    Some(match subtag {
        "ar" => "Arabic",
        "bg" => "Bulgarian",
        "ca" => "Catalan",
        "cs" => "Czech",
        "da" => "Danish",
        "de" => "German",
        "el" => "Greek",
        "en" => "English",
        "es" => "Spanish",
        "fi" => "Finnish",
        "fr" => "French",
        "he" => "Hebrew",
        "hi" => "Hindi",
        "hr" => "Croatian",
        "hu" => "Hungarian",
        "id" => "Indonesian",
        "it" => "Italian",
        "ja" => "Japanese",
        "ko" => "Korean",
        "ms" => "Malay",
        "nb" | "no" => "Norwegian",
        "nl" => "Dutch",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "ro" => "Romanian",
        "ru" => "Russian",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "sv" => "Swedish",
        "th" => "Thai",
        "tr" => "Turkish",
        "uk" => "Ukrainian",
        "vi" => "Vietnamese",
        "yue" => "Cantonese",
        "zh" => "Chinese",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::VoicePlatform;

    fn list(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    /// A slice of hesperia's 180 voices: English in several locales, one
    /// Polish, one German.
    fn hesperia() -> Vec<String> {
        list(&["de-DE", "en-AU", "en-GB", "en-US", "pl-PL"])
    }

    fn refused(result: Result<String, VoiceUnavailable>) -> String {
        match result {
            Err(VoiceUnavailable::NoVoice { language }) => language,
            other => panic!("expected a NoVoice refusal, got {other:?}"),
        }
    }

    /// The case the epic opens with: listening in English, the bot answered
    /// in Polish, the Mac has a Polish voice.
    #[test]
    fn a_polish_answer_with_a_polish_voice_is_spoken_in_polish() {
        assert_eq!(
            choose_voice(Some("pl"), "en-US", &hesperia()).expect("a voice"),
            "pl-PL"
        );
    }

    /// The same answer on a device without the voice: the refusal names
    /// Polish and the download page, and nothing is spoken in English.
    #[test]
    fn a_polish_answer_with_no_polish_voice_is_refused_by_name() {
        let voices = list(&["en-GB", "en-US"]);
        let language = refused(choose_voice(Some("pl"), "en-US", &voices));
        assert_eq!(language, "pl");
        let sentence = VoiceUnavailable::NoVoice { language }.message(&VoicePlatform::MACOS);
        assert!(sentence.contains("no voice for Polish (pl)"), "{sentence}");
        assert!(sentence.contains("stays on the screen"), "{sentence}");
        assert!(
            sentence.contains("System Settings > Accessibility > Spoken Content"),
            "{sentence}"
        );
    }

    /// The detector could not tell: the listening language's own voice.
    #[test]
    fn undetermined_is_spoken_in_the_listening_language() {
        assert_eq!(
            choose_voice(None, "en-US", &hesperia()).expect("a voice"),
            "en-US"
        );
        assert_eq!(
            choose_voice(Some(""), "pl_PL", &hesperia()).expect("a voice"),
            "pl-PL"
        );
    }

    /// A Polish-listening device whose bot answered in English, with an
    /// English voice: English, and the listener's own locale of it is not
    /// in play, so the first English voice in the sorted list.
    #[test]
    fn an_english_answer_on_a_polish_listener_takes_an_english_voice() {
        assert_eq!(
            choose_voice(Some("en"), "pl-PL", &hesperia()).expect("a voice"),
            "en-AU"
        );
    }

    /// Among a language's voices, the listening locale's own wins.
    #[test]
    fn the_listening_locale_wins_among_its_languages_voices() {
        assert_eq!(
            choose_voice(Some("en"), "en-GB", &hesperia()).expect("a voice"),
            "en-GB"
        );
        assert_eq!(
            choose_voice(Some("en"), "en_us", &hesperia()).expect("a voice"),
            "en-US"
        );
    }

    /// The detector's spelling and the synthesiser's meet at the subtag.
    #[test]
    fn a_script_subtag_still_finds_the_languages_voice() {
        let voices = list(&["en-US", "zh-CN", "zh-TW"]);
        assert_eq!(
            choose_voice(Some("zh-Hans"), "en-US", &voices).expect("a voice"),
            "zh-CN"
        );
    }

    /// Listening in a language with no voice and no detection: the refusal
    /// names the listening language, not "undetermined".
    #[test]
    fn no_voice_for_the_listening_language_is_refused_by_that_name() {
        let voices = list(&["en-US"]);
        assert_eq!(refused(choose_voice(None, "pl-PL", &voices)), "pl-PL");
        assert_eq!(refused(choose_voice(None, "", &voices)), "");
    }

    /// The detector's set: every voice language plus the listening one,
    /// each once, whatever the spelling of the voices' locales.
    #[test]
    fn constraints_are_voice_languages_plus_the_listening_language() {
        assert_eq!(constraints("en_US", &hesperia()), list(&["de", "en", "pl"]));
        assert_eq!(
            constraints("pl-PL", &list(&["en-GB", "en-US"])),
            list(&["en", "pl"])
        );
        assert_eq!(constraints("", &[]), Vec::<String>::new());
    }

    /// The instruction names the language in the model's terms, with the
    /// tag, and says why it is being asked.
    #[test]
    fn the_instruction_names_the_listening_language() {
        assert_eq!(
            answer_instruction("en-US"),
            "The person asked this aloud and your answer will be read aloud to them. Answer in English (en-US)."
        );
        assert!(answer_instruction("pl_PL").ends_with("Answer in Polish (pl-PL)."));
        assert!(answer_instruction("xx-XX").ends_with("Answer in the language tagged xx-XX."));
    }
}
