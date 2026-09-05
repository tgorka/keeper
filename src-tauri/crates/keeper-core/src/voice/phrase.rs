//! The wake phrase (FR-404, AD-168) and the stop phrase (FR-480, AD-208):
//! what a person typed, made safe to match against what a recogniser heard.
//!
//! Two decisions live here and nowhere else. **What counts as the same
//! phrase**: a recogniser writes "Hej, Keeper." and a person typed "hej keeper",
//! and those must be one phrase, so both sides go through the same
//! [`normalise`] — case folded, diacritics stripped, punctuation treated as a
//! space, whitespace collapsed. **What is too short to be safe**: a phrase of
//! two or three letters fires on ordinary talk — "go", "ok", "hey" — and a
//! person whose phone starts a turn every time somebody says "go" would turn
//! the feature off and be right to. So [`WakePhrase::parse`] refuses with a
//! sentence that says what to type instead.
//!
//! The stop phrase is the same type parsed by a shorter rule
//! ([`WakePhrase::parse_stop`], [`STOP_MIN_LETTERS`]): it is matched only
//! while keeper is speaking, so a false match costs the rest of an answer
//! rather than an open microphone in someone's car, and the word people say
//! to stop something — "stop", four letters — must be allowed.
//!
//! Matching is whole-word: the phrase may sit anywhere in a sentence, but a
//! transcript that merely contains the phrase inside a longer word ("hej
//! keepers") is not a match — a wake word that fires on its own plural is a
//! wake word that fires on something the person did not say.

use std::fmt;

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// The fewest words a phrase may have. **One is allowed on purpose.** The
/// phrase this was written for is "nixie": a single distinct word a
/// recogniser hears whole, said by someone driving with another app in
/// front. Two common words ("ok go") are not safer than one uncommon one —
/// what makes a phrase safe is letters the recogniser can tell from noise,
/// which [`MIN_LETTERS`] guards, not a word count.
pub const MIN_WORDS: usize = 1;

/// The fewest letters a phrase may have, spaces excluded. Below this the
/// recogniser cannot tell it from noise, and neither can the person next to
/// the phone: "go", "ok", "hey" and "okay" are all things people say to each
/// other, and a two-to-four-letter phrase would start a turn on them. Five
/// is where "nixie" sits and where a phrase stops being an everyday word.
pub const MIN_LETTERS: usize = 5;

/// The fewest letters a stop phrase may have. Three: "stop" and "przestań"
/// pass, and the two-letter words a recogniser mishears in a quiet room
/// ("no", "ok") do not — a two-letter stop word would end answers on the
/// tail of the answer itself (AD-209 measures that tail; it does not trust
/// it to be silent).
pub const STOP_MIN_LETTERS: usize = 3;

/// The most words a phrase may have. Above this it is a sentence, which is
/// hard to say the same way twice and harder for a recogniser to hear whole.
pub const MAX_WORDS: usize = 5;

/// Why a typed phrase was not accepted. Every message says what to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PhraseRefused {
    /// Nothing survived normalisation — empty, whitespace, or only punctuation.
    #[error("type a phrase to listen for — for example \"nixie\"")]
    Empty,
    /// Fewer than [`MIN_WORDS`] words. Unreachable while `MIN_WORDS` is one —
    /// an empty phrase is [`PhraseRefused::Empty`] first — and kept so the
    /// rule is a number rather than a missing branch.
    #[error("use at least {MIN_WORDS} words, like \"nixie\"")]
    TooFewWords {
        /// How many words were typed.
        words: usize,
    },
    /// Fewer than `minimum` letters across all words — [`MIN_LETTERS`] for
    /// a wake phrase, [`STOP_MIN_LETTERS`] for a stop phrase.
    #[error(
        "use at least {minimum} letters in total — \"{normalised}\" is too short for the recogniser to tell from noise"
    )]
    TooShort {
        /// How many letters were counted.
        letters: usize,
        /// How many the rule asked for.
        minimum: usize,
        /// The phrase as it would have been matched.
        normalised: String,
    },
    /// More than [`MAX_WORDS`] words.
    #[error(
        "keep it to {MAX_WORDS} words or fewer — a longer phrase is hard to say the same way twice"
    )]
    TooManyWords {
        /// How many words were typed.
        words: usize,
    },
}

/// A validated, normalised phrase — the wake phrase, or the stop phrase.
///
/// Constructed only through [`WakePhrase::parse`] or
/// [`WakePhrase::parse_stop`], so holding one means the phrase passed its
/// length rules and is already in matching form. The inner string is the
/// normalised phrase — what [`WakePhrase::as_str`] returns and what the
/// surface shows back to the person as "listening for …".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WakePhrase(String);

impl WakePhrase {
    /// Normalise `raw` and check it against the wake phrase's length rules.
    pub fn parse(raw: &str) -> Result<Self, PhraseRefused> {
        Self::parse_at_least(raw, MIN_LETTERS)
    }

    /// Normalise `raw` and check it against the stop phrase's length rules
    /// (AD-208): the same words-and-letters shape, [`STOP_MIN_LETTERS`]
    /// letters instead of [`MIN_LETTERS`].
    pub fn parse_stop(raw: &str) -> Result<Self, PhraseRefused> {
        Self::parse_at_least(raw, STOP_MIN_LETTERS)
    }

    fn parse_at_least(raw: &str, min_letters: usize) -> Result<Self, PhraseRefused> {
        let normalised = normalise(raw);
        if normalised.is_empty() {
            return Err(PhraseRefused::Empty);
        }
        let words = normalised.split(' ').count();
        if words < MIN_WORDS {
            return Err(PhraseRefused::TooFewWords { words });
        }
        if words > MAX_WORDS {
            return Err(PhraseRefused::TooManyWords { words });
        }
        let letters = normalised.chars().filter(|c| *c != ' ').count();
        if letters < min_letters {
            return Err(PhraseRefused::TooShort {
                letters,
                minimum: min_letters,
                normalised,
            });
        }
        Ok(Self(normalised))
    }

    /// The phrase in matching form: lower-case, no diacritics, single spaces.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The phrase's words, for a recogniser that accepts vocabulary hints.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.0.split(' ')
    }

    /// Whether `transcript` contains the phrase as whole words, anywhere.
    ///
    /// The transcript goes through the same [`normalise`] as the phrase did,
    /// so "Hej, Kééper — what time is it?" matches "hej keeper", and "hej
    /// keepers" does not.
    pub fn matches(&self, transcript: &str) -> bool {
        let heard = normalise(transcript);
        if heard.is_empty() {
            return false;
        }
        // Whole-word containment without allocating a padded copy of the
        // phrase: every occurrence must sit on word boundaries on both sides.
        heard.match_indices(self.as_str()).any(|(at, hit)| {
            let before_ok = at == 0 || heard.as_bytes()[at - 1] == b' ';
            let end = at + hit.len();
            let after_ok = end == heard.len() || heard.as_bytes()[end] == b' ';
            before_ok && after_ok
        })
    }
}

impl fmt::Display for WakePhrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Fold `text` into matching form.
///
/// NFD decomposition splits "é" into "e" plus a combining acute; dropping the
/// combining marks leaves "e". A handful of letters have no decomposition and
/// are folded by hand ([`fold_letter`]) — "ł" is the one that matters for the
/// person this was written for. Everything that is not a letter or a digit is
/// a word boundary, so punctuation the recogniser adds ("hej, keeper.") does
/// not stop a match, and runs of boundaries collapse to one space.
pub fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut boundary = false;
    for c in text.nfd() {
        if is_combining_mark(c) {
            continue;
        }
        if let Some(folded) = fold_letter(c) {
            push_word_chars(&mut out, &mut boundary, folded.chars());
        } else {
            push_word_chars(&mut out, &mut boundary, std::iter::once(c));
        }
    }
    out
}

/// Append `chars` to `out`, lower-casing letters and turning every
/// non-alphanumeric character into a pending single space.
fn push_word_chars(out: &mut String, boundary: &mut bool, chars: impl Iterator<Item = char>) {
    for c in chars {
        if c.is_alphanumeric() {
            if *boundary && !out.is_empty() {
                out.push(' ');
            }
            *boundary = false;
            out.extend(c.to_lowercase());
        } else {
            *boundary = true;
        }
    }
}

/// Letters that NFD leaves whole because they are letters in their own right,
/// folded to what a person typing without the key would type instead.
fn fold_letter(c: char) -> Option<&'static str> {
    Some(match c {
        'ł' | 'Ł' => "l",
        'ø' | 'Ø' => "o",
        'đ' | 'Đ' | 'ð' | 'Ð' => "d",
        'ß' => "ss",
        'æ' | 'Æ' => "ae",
        'œ' | 'Œ' => "oe",
        'þ' | 'Þ' => "th",
        'ı' => "i",
        _ => return None,
    })
}
