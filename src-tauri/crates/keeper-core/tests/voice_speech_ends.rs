//! Story 97.3 #5 (AD-411): the segmenter's sentences carry where they end.

use keeper_core::voice::speech::{sentence_ends, AnswerFollower, Segmenter, Sentence};

/// 97.3 #5 (AD-411): every sentence the segmenter hands out carries
/// its end in the whole text fed, in Unicode scalar values, past its
/// marks and before the space or line break after them, whatever chunks
/// the text came in; a blank line is no sentence, a dot inside a number
/// closes nothing, and the unclosed tail ends where its last character
/// does. The host's count is the same cut ([`sentence_ends`]).
#[test]
fn segmenter_reports_sentence_ends() {
    for (text, ends) in [
        ("One. Two three!\nFour", vec![4, 15, 20]),
        ("Zażółć. Gęślą.", vec![7, 14]),
        ("It costs 3.5 zł.\n\n\nNext 😀 one?  Yes", vec![16, 30, 35]),
        ("A heading\nThen text.", vec![9, 20]),
        ("  \n", vec![]),
        ("", vec![]),
    ] {
        assert_eq!(sentence_ends(text), ends, "{text:?}");
        let at: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain([text.len()])
            .collect();
        for (n, &first) in at.iter().enumerate() {
            for &second in &at[n..] {
                let mut segmenter = Segmenter::new();
                let mut said = segmenter.push(&text[..first]);
                said.extend(segmenter.push(&text[first..second]));
                said.extend(segmenter.push(&text[second..]));
                said.extend(segmenter.flush());
                let got: Vec<usize> = said.iter().map(|s| s.end).collect();
                assert_eq!(got, ends, "{text:?} cut at {first} and {second}");
                for sentence in &said {
                    let upto: String = text.chars().take(sentence.end).collect();
                    assert!(
                        upto.ends_with(&sentence.text),
                        "{sentence:?} ends where its words do in {text:?}"
                    );
                }
            }
        }
    }

    // An agent's answer read from its edits: the ends are in the text the
    // message reads as when each sentence is handed out, a rewrite
    // included.
    let mut follower = AnswerFollower::new();
    assert_eq!(
        follower.edit("$a", "Zażółć. Gę"),
        vec![Sentence {
            text: "Zażółć.".to_owned(),
            end: 7
        }]
    );
    assert_eq!(
        follower.edit("$a", "Zażółć gęślą. Jaźń! Do"),
        vec![Sentence {
            text: "Jaźń!".to_owned(),
            end: 19
        }]
    );
    assert_eq!(
        follower.finish(),
        Some(Sentence {
            text: "Do".to_owned(),
            end: 22
        })
    );
}
