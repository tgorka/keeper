//! The dictionary (AD-347): names and jargon the recognizer gets wrong,
//! applied after recognition as whole-word, case-insensitive alias → text
//! replacement, and grown from a person's own edits one accepted suggestion
//! at a time.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::bank::{
    json_bytes, now_stamp, term_path, BankDelete, BankError, BankPlan, BankWrite, DictionaryTerm,
    BANK_FILE_VERSION,
};
use super::model::AppliedTerm;
use super::words::{fold, same_folded, Word};

/// A one-word correction a person made that the dictionary could learn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DictionarySuggestion {
    pub from: String,
    pub to: String,
}

/// The letters-and-digits heart of a token: `"Keeper,"` → `"Keeper"`.
fn core(token: &str) -> &str {
    token.trim_matches(|c: char| !c.is_alphanumeric())
}

/// The punctuation around a token's core: `("(", "),")` for `"(keeper),"`.
fn edges(token: &str) -> (&str, &str) {
    let lead = token.len()
        - token
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .len();
    let trail = token.trim_end_matches(|c: char| !c.is_alphanumeric()).len();
    if lead >= trail {
        return ("", "");
    }
    (&token[..lead], &token[trail..])
}

/// One word of an alias, lowercased. A plain word (`gorker`) matches a
/// word's letters-and-digits heart; a word with punctuation of its own
/// (`c++`, `.net`) matches the whole word, less only punctuation around it
/// that the alias itself does not have — so `C++` never matches `C`.
struct PatternToken {
    text: String,
    plain: bool,
}

impl PatternToken {
    fn new(token: &str) -> Option<Self> {
        let heart = core(token);
        (!heart.is_empty()).then(|| Self {
            plain: heart == token,
            text: fold(token),
        })
    }

    /// `(lead, middle, trail)` of `word` when its middle is this token and
    /// the lead and trail are the word's own punctuation.
    fn split<'w>(&self, word: &'w str) -> Option<(&'w str, &'w str, &'w str)> {
        if self.plain {
            let middle = core(word);
            if fold(middle) != self.text {
                return None;
            }
            let (lead, trail) = edges(word);
            return Some((lead, middle, trail));
        }
        // Where the middle may start: before the word, or after any of its
        // leading punctuation; where it may end, likewise from the back.
        let mut starts = vec![0];
        for (at, c) in word.char_indices() {
            if c.is_alphanumeric() {
                break;
            }
            starts.push(at + c.len_utf8());
        }
        let mut ends = vec![word.len()];
        for (at, c) in word.char_indices().rev() {
            if c.is_alphanumeric() {
                break;
            }
            ends.push(at);
        }
        starts.into_iter().find_map(|start| {
            ends.iter()
                .copied()
                .filter(|end| *end >= start)
                .find(|end| fold(&word[start..*end]) == self.text)
                .map(|end| (&word[..start], &word[start..end], &word[end..]))
        })
    }
}

struct Pattern<'a> {
    tokens: Vec<PatternToken>,
    to: &'a str,
}

/// Apply every term to `words`: a run of whole words equal (ignoring case and
/// the words' own punctuation) to a term's alias — or to its own text in
/// another case — becomes one word carrying the term's text, keeping the
/// run's outer punctuation and span. Longer aliases win over shorter ones.
pub fn apply(words: &[Word], terms: &[DictionaryTerm]) -> (Vec<Word>, Vec<AppliedTerm>) {
    let mut patterns: Vec<Pattern> = terms
        .iter()
        .flat_map(|term| {
            std::iter::once(term.text.as_str())
                .chain(term.aliases.iter().map(String::as_str))
                .map(move |alias| Pattern {
                    tokens: alias
                        .split_whitespace()
                        .filter_map(PatternToken::new)
                        .collect(),
                    to: term.text.as_str(),
                })
        })
        .filter(|pattern| !pattern.tokens.is_empty() && !pattern.to.trim().is_empty())
        .collect();
    patterns.sort_by_key(|pattern| std::cmp::Reverse(pattern.tokens.len()));

    let mut out = Vec::with_capacity(words.len());
    let mut applied: Vec<AppliedTerm> = Vec::new();
    let mut index = 0;
    while index < words.len() {
        let found = patterns.iter().find_map(|pattern| {
            let run = words.get(index..index + pattern.tokens.len())?;
            let cuts: Vec<(&str, &str, &str)> = run
                .iter()
                .zip(&pattern.tokens)
                .map(|(word, token)| token.split(&word.text))
                .collect::<Option<_>>()?;
            Some((pattern, run, cuts))
        });
        let Some((pattern, run, cuts)) = found else {
            out.push(words[index].clone());
            index += 1;
            continue;
        };
        let from = cuts
            .iter()
            .map(|(_, middle, _)| *middle)
            .collect::<Vec<_>>()
            .join(" ");
        index += run.len();
        if from == pattern.to {
            out.extend_from_slice(run);
            continue;
        }
        let (lead, _, _) = cuts[0];
        let (_, _, trail) = cuts[cuts.len() - 1];
        out.push(Word {
            text: format!("{lead}{}{trail}", pattern.to),
            start: run[0].start,
            end: run[run.len() - 1].end,
            confidence: run
                .iter()
                .map(|word| word.confidence)
                .fold(f32::INFINITY, f32::min),
        });
        match applied
            .iter_mut()
            .find(|entry| entry.to == pattern.to && same_folded(&entry.from, &from))
        {
            Some(entry) => entry.count += 1,
            None => applied.push(AppliedTerm {
                from,
                to: pattern.to.to_owned(),
                count: 1,
            }),
        }
    }
    (out, applied)
}

/// Single-word substitutions between two versions of a text. A change of
/// case or punctuation only is not a word the recognizer got wrong, and a
/// rewrite of several words in a row is not a dictionary entry.
pub fn suggestions(before: &str, after: &str) -> Vec<DictionarySuggestion> {
    let old: Vec<&str> = before.split_whitespace().collect();
    let new: Vec<&str> = after.split_whitespace().collect();
    let key = |token: &str| fold(core(token));
    let old_keys: Vec<String> = old.iter().map(|token| key(token)).collect();
    let new_keys: Vec<String> = new.iter().map(|token| key(token)).collect();

    // lcs[i][j]: the longest common subsequence of old[i..] and new[j..].
    let mut lcs = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lcs[i][j] = if old_keys[i] == new_keys[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut found: Vec<DictionarySuggestion> = Vec::new();
    let mut seen = HashSet::new();
    let (mut i, mut j) = (0, 0);
    let (mut removed, mut added): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let mut flush = |removed: &mut Vec<usize>, added: &mut Vec<usize>| {
        if let ([from], [to]) = (removed.as_slice(), added.as_slice()) {
            let (from, to) = (core(old[*from]), core(new[*to]));
            if !from.is_empty() && !to.is_empty() && seen.insert((from, to)) {
                found.push(DictionarySuggestion {
                    from: from.to_owned(),
                    to: to.to_owned(),
                });
            }
        }
        removed.clear();
        added.clear();
    };
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old_keys[i] == new_keys[j] {
            flush(&mut removed, &mut added);
            i += 1;
            j += 1;
        } else if j >= new.len() || (i < old.len() && lcs[i + 1][j] >= lcs[i][j + 1]) {
            removed.push(i);
            i += 1;
        } else {
            added.push(j);
            j += 1;
        }
    }
    flush(&mut removed, &mut added);
    found
}

fn clean_aliases(text: &str, aliases: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::with_capacity(aliases.len());
    for alias in aliases {
        let alias = alias.trim();
        if alias.is_empty()
            || same_folded(alias, text)
            || kept.iter().any(|kept| same_folded(kept, alias))
        {
            continue;
        }
        kept.push(alias.to_owned());
    }
    kept
}

fn term_write(term: &DictionaryTerm) -> Result<BankWrite, BankError> {
    let rel_path = term_path(&term.id);
    Ok(BankWrite {
        bytes: json_bytes(term, &rel_path)?,
        rel_path,
    })
}

/// Create (`id: None`) or replace a term.
pub fn plan_save_term(
    terms: &[DictionaryTerm],
    id: Option<&str>,
    text: &str,
    aliases: &[String],
) -> Result<(DictionaryTerm, BankPlan), BankError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(BankError::EmptyTerm);
    }
    let (id, created_at) = match id {
        Some(id) => {
            let existing = terms
                .iter()
                .find(|term| term.id == id)
                .ok_or_else(|| BankError::UnknownTerm(id.to_owned()))?;
            (existing.id.clone(), existing.created_at.clone())
        }
        None => (ulid::Ulid::new().to_string(), now_stamp()),
    };
    let term = DictionaryTerm {
        version: BANK_FILE_VERSION,
        id,
        text: text.to_owned(),
        aliases: clean_aliases(text, aliases),
        created_at,
    };
    let plan = BankPlan {
        writes: vec![term_write(&term)?],
        deletes: Vec::new(),
    };
    Ok((term, plan))
}

pub fn plan_delete_term(terms: &[DictionaryTerm], id: &str) -> Result<BankPlan, BankError> {
    let term = terms
        .iter()
        .find(|term| term.id == id)
        .ok_or_else(|| BankError::UnknownTerm(id.to_owned()))?;
    Ok(BankPlan {
        writes: Vec::new(),
        deletes: vec![BankDelete {
            rel_path: term_path(&term.id),
        }],
    })
}

/// Accept a suggestion: `from` becomes an alias of the term spelled `to`,
/// which is created when the dictionary has none.
pub fn plan_accept_suggestion(
    terms: &[DictionaryTerm],
    from: &str,
    to: &str,
) -> Result<(DictionaryTerm, BankPlan), BankError> {
    let to = to.trim();
    match terms.iter().find(|term| same_folded(&term.text, to)) {
        Some(term) => {
            let mut aliases = term.aliases.clone();
            aliases.push(from.to_owned());
            plan_save_term(terms, Some(&term.id), &term.text, &aliases)
        }
        None => plan_save_term(terms, None, to, &[from.to_owned()]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<Word> {
        text.split_whitespace()
            .enumerate()
            .map(|(index, text)| Word {
                text: text.to_owned(),
                start: index as f64,
                end: index as f64 + 0.5,
                confidence: 0.9,
            })
            .collect()
    }

    fn term(text: &str, aliases: &[&str]) -> DictionaryTerm {
        DictionaryTerm {
            version: 1,
            id: text.to_owned(),
            text: text.to_owned(),
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
            created_at: String::new(),
        }
    }

    fn texts(words: &[Word]) -> Vec<&str> {
        words.iter().map(|word| word.text.as_str()).collect()
    }

    #[test]
    fn a_multi_word_alias_in_any_case_becomes_the_term_and_keeps_its_punctuation() {
        let (out, applied) = apply(
            &words("so NEURA drive, and neura drive again"),
            &[term("NeuraDrive", &["neura drive"])],
        );
        assert_eq!(
            texts(&out),
            ["so", "NeuraDrive,", "and", "NeuraDrive", "again"]
        );
        assert_eq!(
            (out[1].start, out[1].end),
            (1.0, 2.5),
            "the run's span is kept"
        );
        assert_eq!(
            applied,
            [AppliedTerm {
                from: "NEURA drive".to_owned(),
                to: "NeuraDrive".to_owned(),
                count: 2
            }]
        );
    }

    #[test]
    fn an_alias_never_matches_part_of_a_word() {
        let (out, applied) = apply(&words("Tomasz and tomato"), &[term("Tom", &["tom"])]);
        assert_eq!(texts(&out), ["Tomasz", "and", "tomato"]);
        assert!(applied.is_empty());
    }

    #[test]
    fn the_terms_own_text_in_another_case_is_respelled_and_the_right_case_is_left_alone() {
        let (out, applied) = apply(&words("gorka met Gorka"), &[term("Gorka", &[])]);
        assert_eq!(texts(&out), ["Gorka", "met", "Gorka"]);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].count, 1);
    }

    #[test]
    fn suggestions_are_single_word_substitutions_only() {
        assert_eq!(
            suggestions("we met tom Gorker today.", "We met Tom Gorka today!"),
            [DictionarySuggestion {
                from: "Gorker".to_owned(),
                to: "Gorka".to_owned()
            }],
            "case-only and punctuation-only changes are not suggestions"
        );
        assert!(suggestions("the key per app", "the keeper app").is_empty());
        assert!(suggestions("hello there", "hello there friend").is_empty());
    }

    #[test]
    fn accepting_a_suggestion_extends_an_existing_term() {
        let terms = [term("Gorka", &["Gorker"])];
        let (updated, plan) = plan_accept_suggestion(&terms, "gorkah", "gorka").expect("accept");
        assert_eq!(updated.id, "Gorka");
        assert_eq!(updated.aliases, ["Gorker", "gorkah"]);
        assert_eq!(plan.writes[0].rel_path, "dictionary/Gorka.json");
        let (created, _) = plan_accept_suggestion(&terms, "keper", "Keeper").expect("accept");
        assert_eq!(
            (created.text.as_str(), created.aliases.as_slice()),
            ("Keeper", &["keper".to_owned()][..])
        );
    }

    #[test]
    fn a_term_with_punctuation_of_its_own_matches_only_the_whole_word() {
        let terms = [term("C++", &[]), term(".NET", &[])];
        let (out, applied) = apply(&words("C++, plan C .NET net"), &terms);
        assert_eq!(texts(&out), ["C++,", "plan", "C", ".NET", "net"]);
        assert!(applied.is_empty(), "{applied:?}");

        let (out, applied) = apply(&words("c++ and (.net),"), &terms);
        assert_eq!(texts(&out), ["C++", "and", "(.NET),"]);
        assert_eq!(applied.len(), 2);

        let (out, _) = apply(&words("see sharp, then c"), &[term("C#", &["see sharp"])]);
        assert_eq!(
            texts(&out),
            ["C#,", "then", "c"],
            "a plain alias still matches on the word's heart"
        );
    }

    #[test]
    fn case_is_folded_beyond_ascii() {
        let terms = [term("łukasz", &[])];
        let (updated, _) = plan_accept_suggestion(&terms, "lukasz", "Łukasz").expect("accept");
        assert_eq!(
            updated.id, "łukasz",
            "the existing term grows, no duplicate"
        );
        assert_eq!(updated.aliases, ["lukasz"]);
        let (saved, _) = plan_save_term(&[], None, "Łódź", &["ŁÓDŹ".to_owned(), "lodz".to_owned()])
            .expect("save");
        assert_eq!(saved.aliases, ["lodz"]);
        let (out, _) = apply(&words("ŁUKASZ"), &[term("Łukasz", &[])]);
        assert_eq!(texts(&out), ["Łukasz"]);
    }
}
