//! Words from recognizer pieces. Parakeet answers in SentencePiece pieces —
//! `▁Hel`, `lo`, `,` — each with its own timing; a transcript speaks in words.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::engine::AsrToken;

/// The SentencePiece word-start marker.
const WORD_START: char = '\u{2581}';

/// One word with its timing (seconds) and the recognizer's confidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub confidence: f32,
}

/// Join pieces into words: a piece starting with `▁` (or whitespace) opens a
/// word, any other piece continues the current one — which is how a trailing
/// `,` or `.` lands on the word before it. The first piece opens a word even
/// without the marker. A word's confidence is its weakest piece's.
pub fn words_from_tokens(tokens: &[AsrToken]) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    let mut open = false;
    for token in tokens {
        let starts_word = token.text.starts_with([WORD_START, ' ', '\t', '\n']);
        let piece = token
            .text
            .trim_start_matches([WORD_START, ' ', '\t', '\n'])
            .trim_end();
        if starts_word || !open {
            if piece.is_empty() {
                // A bare marker: the next piece opens the word.
                open = false;
                continue;
            }
            words.push(Word {
                text: piece.to_owned(),
                start: token.start,
                end: token.end,
                confidence: token.confidence,
            });
            open = true;
        } else if let Some(word) = words.last_mut() {
            word.text.push_str(piece);
            word.end = word.end.max(token.end);
            word.confidence = word.confidence.min(token.confidence);
        }
    }
    words
}

/// The text of a run of words, single-spaced.
pub fn join_words(words: &[Word]) -> String {
    let mut text = String::with_capacity(words.iter().map(|word| word.text.len() + 1).sum());
    for word in words {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&word.text);
    }
    text
}

/// The one case folding transcription compares text by: Unicode, because
/// `Łukasz` and `łukasz` are one name.
pub(crate) fn fold(text: &str) -> String {
    text.to_lowercase()
}

/// Whether two texts differ only in case, by [`fold`].
pub(crate) fn same_folded(left: &str, right: &str) -> bool {
    left == right || fold(left) == fold(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(text: &str, start: f64, end: f64, confidence: f32) -> AsrToken {
        AsrToken {
            text: text.to_owned(),
            start,
            end,
            confidence,
        }
    }

    #[test]
    fn pieces_join_into_words_with_punctuation_on_the_word_before() {
        let tokens = [
            token("Hel", 0.0, 0.1, 0.9),
            token("lo", 0.1, 0.2, 0.8),
            token(",", 0.2, 0.25, 0.99),
            token("▁wor", 0.4, 0.5, 0.7),
            token("ld", 0.5, 0.6, 0.95),
            token(".", 0.6, 0.62, 0.99),
            token("▁", 0.7, 0.7, 1.0),
            token("Tak", 0.8, 0.9, 0.6),
        ];
        let words = words_from_tokens(&tokens);
        let texts: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();
        assert_eq!(texts, ["Hello,", "world.", "Tak"]);
        assert_eq!((words[0].start, words[0].end), (0.0, 0.25));
        assert_eq!(
            words[0].confidence, 0.8,
            "a word is as sure as its weakest piece"
        );
        assert_eq!((words[1].start, words[1].end), (0.4, 0.62));
        assert_eq!(
            words[2].start, 0.8,
            "a bare marker opens the next piece's word"
        );
    }
}
