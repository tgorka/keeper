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

/// The fewest characters a sentence needs before the detector's answer for
/// it outweighs the answer's first choice of voice (Epic 68, AD-214).
///
/// A detector reads a short sentence — "Yes.", "3.5 euros." — as whatever
/// language shares its few words, and an answer whose voice flipped on each
/// of those would be DW-231's objection realised. Forty characters is a
/// clause with a verb in it, which is about where an on-device detector
/// stops guessing.
pub const CONFIDENT_CHARS: usize = 40;

/// Whether `detected` — the detector's answer for `sentence` — is confident
/// enough to change the voice mid-answer: the detector answered at all,
/// and the sentence is at least [`CONFIDENT_CHARS`] long. `Some` is the
/// language to choose the voice for; `None` keeps the answer's first
/// choice. Pure, so the rule is here and not in a port.
pub fn confident<'a>(detected: Option<&'a str>, sentence: &str) -> Option<&'a str> {
    let detected = detected.map(str::trim).filter(|d| !d.is_empty())?;
    (sentence.trim().chars().count() >= CONFIDENT_CHARS).then_some(detected)
}

/// The marks that end a sentence when they end a word.
const SENTENCE_MARKS: [char; 3] = ['.', '!', '?'];

/// An answer cut into sentences as it streams (Epic 68, Story 68.3, AD-214).
///
/// Fed chunk by chunk, it yields every sentence the text so far has
/// closed, in order, and holds the tail — the words after the last
/// boundary — until the next chunk closes it or [`Segmenter::flush`] says
/// the stream is over. The boundary rule is the lock-screen banner's
/// (`banner::first_sentence`, which now asks this type): a `.`, `!` or `?`
/// ends a sentence when it ends a word — the character after the run of
/// marks is whitespace — and a line break ends one too, because a markdown
/// heading or a list item is read as its own breath. A dot inside a number
/// (`3.5`) is followed by a digit and closes nothing; an abbreviation's
/// (`e.g. this`) is followed by a space and does, which is the same
/// treatment the banner gives it: a sentence read a beat early is a small
/// price, and no rule short of a dictionary tells `e.g.` from `etc.` at
/// the end of a sentence.
///
/// A mark at the very end of the buffered text closes nothing yet: the next
/// chunk may begin with the digit that makes it a number. Only `flush`
/// treats the end of the text as the end of a word.
#[derive(Debug, Default)]
pub struct Segmenter {
    tail: String,
}

impl Segmenter {
    /// An empty segmenter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed `chunk` and take every sentence it closed, trimmed, in order.
    /// Blank sentences — a run of line breaks — are not sentences.
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.tail.push_str(chunk);
        let mut sentences = Vec::new();
        while let Some(end) = boundary(&self.tail) {
            let rest = self.tail.split_off(end);
            let sentence = std::mem::replace(&mut self.tail, rest);
            let sentence = sentence.trim();
            if !sentence.is_empty() {
                sentences.push(sentence.to_owned());
            }
        }
        sentences
    }

    /// The stream is over: the tail, trimmed, when there is one.
    pub fn flush(&mut self) -> Option<String> {
        let tail = std::mem::take(&mut self.tail);
        let tail = tail.trim();
        (!tail.is_empty()).then(|| tail.to_owned())
    }
}

/// The byte index just past the first sentence `text` has closed — past its
/// run of marks, or past its line break — or `None` while none is closed.
fn boundary(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().peekable();
    while let Some((index, mark)) = chars.next() {
        if mark == '\n' {
            return Some(index + 1);
        }
        if !SENTENCE_MARKS.contains(&mark) {
            continue;
        }
        // The whole run of marks: `?!`, `...`.
        let mut end = index + mark.len_utf8();
        while let Some(&(next_index, next)) = chars.peek() {
            if SENTENCE_MARKS.contains(&next) {
                end = next_index + next.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        // A run at the end of the text is not yet known to end a word.
        match text[end..].chars().next() {
            Some(after) if after.is_whitespace() => return Some(end),
            _ => {}
        }
    }
    None
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

    // -- The segmenter (Epic 68, AD-214) ------------------------------------

    fn feed(chunks: &[&str]) -> (Vec<String>, Option<String>) {
        let mut segmenter = Segmenter::new();
        let mut sentences = Vec::new();
        for chunk in chunks {
            sentences.extend(segmenter.push(chunk));
        }
        let rest = segmenter.flush();
        (sentences, rest)
    }

    /// A sentence split across chunks is yielded once, whole, when its
    /// boundary arrives; the rest waits.
    #[test]
    fn a_sentence_split_across_chunks_is_yielded_once_it_closes() {
        let mut segmenter = Segmenter::new();
        assert_eq!(segmenter.push("Tomorrow is"), Vec::<String>::new());
        assert_eq!(segmenter.push(" sunny."), Vec::<String>::new());
        assert_eq!(
            segmenter.push(" Expect 24 degrees. And"),
            vec![
                "Tomorrow is sunny.".to_owned(),
                "Expect 24 degrees.".to_owned()
            ]
        );
        assert_eq!(segmenter.flush(), Some("And".to_owned()));
        assert_eq!(segmenter.flush(), None);
    }

    /// A dot inside a number closes nothing — including when the chunk ends
    /// on the dot and the digit is in the next one.
    #[test]
    fn a_dot_in_a_number_does_not_end_a_sentence() {
        let (sentences, rest) = feed(&["It costs 3", ".", "5 euros in Berlin. More", " soon."]);
        assert_eq!(sentences, list(&["It costs 3.5 euros in Berlin."]));
        // The last mark closes nothing until the stream is over: the next
        // chunk could have been a digit.
        assert_eq!(rest, Some("More soon.".to_owned()));
    }

    /// Abbreviations are treated as the banner treats them: `e.g.` ends a
    /// sentence, and the words after it are the next one.
    #[test]
    fn abbreviations_end_a_sentence_as_the_banner_has_it() {
        let (sentences, rest) = feed(&["Use a fruit, e.g. an apple. Done!"]);
        assert_eq!(sentences, list(&["Use a fruit, e.g.", "an apple."]));
        assert_eq!(rest, Some("Done!".to_owned()));
    }

    /// A run of marks is one boundary; a line break is a boundary too, and
    /// blank lines are not sentences.
    #[test]
    fn mark_runs_and_line_breaks_are_boundaries() {
        let (sentences, rest) = feed(&["Really?! Yes.\nThree things\n\n- one\n- two"]);
        assert_eq!(
            sentences,
            list(&["Really?!", "Yes.", "Three things", "- one"])
        );
        assert_eq!(rest, Some("- two".to_owned()));
    }

    /// A stream that ends mid-sentence: nothing is yielded early, and the
    /// flush hands the tail over. Nothing at all yields nothing.
    #[test]
    fn a_stream_ending_mid_sentence_is_held_until_the_flush() {
        let (sentences, rest) = feed(&["No terminator", " at all"]);
        assert_eq!(sentences, Vec::<String>::new());
        assert_eq!(rest, Some("No terminator at all".to_owned()));
        assert_eq!(feed(&["", "  \n"]), (Vec::new(), None));
    }

    /// A detected language changes the voice mid-answer only on a sentence
    /// long enough to be sure about; a short one keeps the first choice.
    #[test]
    fn confidence_needs_a_detection_and_a_long_enough_sentence() {
        let long = "Jutro będzie słonecznie i ciepło, około dwudziestu stopni.";
        assert!(long.chars().count() >= CONFIDENT_CHARS);
        assert_eq!(confident(Some("pl"), long), Some("pl"));
        assert_eq!(confident(Some("pl"), "Tak."), None);
        assert_eq!(confident(None, long), None);
        assert_eq!(confident(Some(" "), long), None);
    }
}
