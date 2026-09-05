//! What the lock screen says when keeper is not in front (Epic 67, Story
//! 67.2, FR-477–FR-479, AD-207).
//!
//! The island (`island`) is the richer surface, and the one a free team's
//! build cannot draw. A local notification is the surface every build has:
//! posted with one fixed identifier so each state replaces the last in
//! place, and only while keeper is not the app in front — in front, the
//! pane and the pill say the same thing and a banner over them would be
//! noise. The shell (`voice_notify`) reads the application state on the
//! main thread and posts; which words a state gets, and whether a state is
//! worth a banner at all, is decided here, pure over the island's [`Word`],
//! so it is tested on the dev host (AD-55/AD-56).
//!
//! The rules:
//!
//! - `Heard`, `Thinking`, `Answering`, `Speaking` and `Failed` each have a
//!   sentence. `Armed`, `Listening` and `Off` have none: the microphone dot
//!   already says the ear is open, a partial transcript changes twenty
//!   times a second, and a card being removed has nothing to say — those
//!   are the moments the shell *clears* the banner.
//! - The answer is spoken, not read: the banner carries its first sentence
//!   only, clipped at [`BODY_LIMIT`] on a word so a lock screen never shows
//!   half a word. A transcript is clipped the same way, for the same reason.
//! - Nothing is posted with keeper in front ([`should_post`]).

use super::island::Word;

/// The most characters a banner's body carries. A lock-screen banner shows
/// two or three lines; past that the system truncates it anyway, and it
/// truncates mid-word.
pub const BODY_LIMIT: usize = 120;

/// The mark a clipped body ends on.
const ELLIPSIS: char = '…';

/// One banner: what the system shows in bold and what under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    /// The bold line: the state, as a word the person can act on.
    pub title: String,
    /// The line under it: the words heard, the answer's first sentence, the
    /// failure's reason — or empty for a state that has no words yet.
    pub body: String,
}

/// The banner for `word`, with `detail` as the words that go with it — the
/// transcript for `Heard`, the answer for `Speaking`, the reason for
/// `Failed`, ignored otherwise — or `None` for a word that has none.
#[must_use]
pub fn sentence(word: Word, detail: &str) -> Option<Banner> {
    let (title, body) = match word {
        Word::Heard => ("Heard", clip(detail.trim())),
        Word::Thinking => ("Thinking", String::new()),
        Word::Answering => ("Answering", String::new()),
        Word::Speaking => ("Answer", clip(first_sentence(detail))),
        Word::Failed => ("Listening stopped", clip(detail.trim())),
        Word::Armed | Word::Listening | Word::Off => return None,
    };
    Some(Banner {
        title: title.to_owned(),
        body,
    })
}

/// Whether a banner for `word` is posted at all: never with keeper in
/// front, and never for a word that has no sentence.
#[must_use]
pub fn should_post(in_front: bool, word: Word) -> bool {
    !in_front && !matches!(word, Word::Armed | Word::Listening | Word::Off)
}

/// The first sentence of `text`: up to and including the first `.`, `!`
/// or `?` that ends a word, or the first line, whichever comes first. A
/// dot inside a number (`3.5`) does not end a sentence; an abbreviation's
/// (`e.g. this`) does, and the banner is a glance, not the answer.
fn first_sentence(text: &str) -> &str {
    let text = text.trim_start();
    let first_line = text.lines().next().unwrap_or_default();
    let mut end = first_line.len();
    for (index, mark) in first_line.char_indices() {
        if !matches!(mark, '.' | '!' | '?') {
            continue;
        }
        let after = index + mark.len_utf8();
        let ends_a_word = first_line[after..]
            .chars()
            .next()
            .is_none_or(|next| next.is_whitespace() || matches!(next, '.' | '!' | '?'));
        if ends_a_word {
            // Take the whole run of marks: `?!`, `...`.
            end = after
                + first_line[after..]
                    .chars()
                    .take_while(|next| matches!(next, '.' | '!' | '?'))
                    .map(char::len_utf8)
                    .sum::<usize>();
            break;
        }
    }
    first_line[..end].trim_end()
}

/// `text` when it fits [`BODY_LIMIT`]; otherwise the longest prefix that
/// ends on a whole word, with [`ELLIPSIS`] where the rest was. A single
/// word longer than the limit is clipped mid-word rather than dropped.
fn clip(text: &str) -> String {
    if text.chars().count() <= BODY_LIMIT {
        return text.to_owned();
    }
    let room = BODY_LIMIT - 1;
    let cut = text
        .char_indices()
        .nth(room)
        .map_or(text.len(), |(index, _)| index);
    let head = &text[..cut];
    let on_a_word = head.rfind(char::is_whitespace).unwrap_or(cut);
    let kept = if on_a_word == 0 {
        head
    } else {
        &head[..on_a_word]
    };
    let mut clipped = kept.trim_end().to_owned();
    clipped.push(ELLIPSIS);
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banner(title: &str, body: &str) -> Option<Banner> {
        Some(Banner {
            title: title.to_owned(),
            body: body.to_owned(),
        })
    }

    #[test]
    fn heard_carries_the_words() {
        assert_eq!(
            sentence(Word::Heard, " what is the weather tomorrow "),
            banner("Heard", "what is the weather tomorrow")
        );
    }

    #[test]
    fn thinking_and_answering_have_no_words_yet() {
        assert_eq!(sentence(Word::Thinking, "ignored"), banner("Thinking", ""));
        assert_eq!(
            sentence(Word::Answering, "ignored"),
            banner("Answering", "")
        );
    }

    #[test]
    fn speaking_carries_the_first_sentence_of_the_answer() {
        assert_eq!(
            sentence(
                Word::Speaking,
                "Tomorrow is sunny. Expect 24 degrees by noon, and a breeze."
            ),
            banner("Answer", "Tomorrow is sunny.")
        );
        assert_eq!(
            sentence(Word::Speaking, "Really?! Yes.\nSecond line."),
            banner("Answer", "Really?!")
        );
        // A markdown answer: the first line is the sentence.
        assert_eq!(
            sentence(Word::Speaking, "Three things\n\n- one\n- two"),
            banner("Answer", "Three things")
        );
        // A dot inside a number does not end the sentence.
        assert_eq!(
            sentence(Word::Speaking, "It costs 3.5 euros in Berlin. More."),
            banner("Answer", "It costs 3.5 euros in Berlin.")
        );
        assert_eq!(
            sentence(Word::Speaking, "No terminator at all"),
            banner("Answer", "No terminator at all")
        );
    }

    #[test]
    fn failed_carries_the_reason() {
        assert_eq!(
            sentence(Word::Failed, "The microphone is in use."),
            banner("Listening stopped", "The microphone is in use.")
        );
    }

    #[test]
    fn armed_listening_and_off_have_no_banner() {
        assert_eq!(sentence(Word::Armed, "nixie"), None);
        assert_eq!(sentence(Word::Listening, "partial words"), None);
        assert_eq!(sentence(Word::Off, ""), None);
    }

    #[test]
    fn a_long_sentence_is_clipped_on_a_word_with_an_ellipsis() {
        let long = "word ".repeat(40); // 200 chars, no terminator
        let Some(Banner { body, .. }) = sentence(Word::Speaking, &long) else {
            panic!("speaking has a banner");
        };
        assert!(body.chars().count() <= BODY_LIMIT, "{body:?}");
        assert!(body.ends_with("word…"), "{body:?}");
        assert!(!body.ends_with(" …"), "{body:?}");
        // The same clip for a transcript.
        let Some(Banner { body, .. }) = sentence(Word::Heard, &long) else {
            panic!("heard has a banner");
        };
        assert!(body.chars().count() <= BODY_LIMIT, "{body:?}");
        assert!(body.ends_with("word…"), "{body:?}");
    }

    #[test]
    fn exactly_the_limit_is_not_clipped_and_one_over_is() {
        let fits = "x".repeat(BODY_LIMIT);
        assert_eq!(sentence(Word::Heard, &fits).map(|b| b.body), Some(fits));
        let over = "x".repeat(BODY_LIMIT + 1);
        let body = sentence(Word::Heard, &over)
            .map(|b| b.body)
            .expect("a heard word has a banner");
        // One word longer than the limit: clipped mid-word, not dropped.
        assert_eq!(body.chars().count(), BODY_LIMIT);
        assert!(body.ends_with(ELLIPSIS));
    }

    #[test]
    fn clipping_counts_characters_not_bytes() {
        let polish = "żółć ".repeat(30); // 150 chars, 240 bytes
        let body = sentence(Word::Heard, &polish)
            .map(|b| b.body)
            .expect("a heard word has a banner");
        assert!(body.chars().count() <= BODY_LIMIT, "{body:?}");
        assert!(body.ends_with("żółć…"), "{body:?}");
    }

    #[test]
    fn in_front_posts_nothing_and_behind_posts_only_words_with_a_sentence() {
        for word in [
            Word::Heard,
            Word::Thinking,
            Word::Answering,
            Word::Speaking,
            Word::Failed,
        ] {
            assert!(!should_post(true, word), "{word:?} in front");
            assert!(should_post(false, word), "{word:?} behind");
        }
        for word in [Word::Armed, Word::Listening, Word::Off] {
            assert!(!should_post(true, word), "{word:?} in front");
            assert!(!should_post(false, word), "{word:?} behind");
        }
    }
}
