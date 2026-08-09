//! The pure matcher behind vault-wide search (FR-118).
//!
//! Case- and diacritic-folded substring matching that reports **byte spans into
//! the original haystack**, because the caller highlights the user's bytes, not
//! a folded copy of them. The parallel walk over the vault is the shell's job;
//! this module sees one document at a time.
//!
//! Folding is hand-rolled rather than delegated. keeper has no unicode
//! normalisation dependency and AD-55 is emphatic about not acquiring one for
//! the notes phase, so the table below covers Latin-1 Supplement, Latin
//! Extended-A and the combining-mark blocks — which is every accent a Latin
//! script writer will type — and everything else falls through to
//! [`char::to_lowercase`]. A Cyrillic or CJK haystack therefore folds case but
//! keeps its characters, which is correct: those scripts have no diacritics to
//! strip.
//!
//! The match runs against a *streaming* fold, never a materialised folded copy
//! of the haystack: a 1 MiB note would otherwise cost a second megabyte plus an
//! offset map, and the cold-scan budget (NFR-28) has no room for either.

use std::cmp::Ordering;

/// How many characters of context a snippet keeps on each side of the match,
/// clipped to the matched line.
const SNIPPET_CONTEXT: usize = 48;

/// One match: where it is, and enough surrounding text to render a result row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// 1-based line number of the match start.
    pub line: u32,
    /// Byte range of the match in the **original** haystack.
    pub span: (usize, usize),
    /// The matched line, windowed to [`SNIPPET_CONTEXT`] characters either side
    /// and elided with `…` where it was cut.
    pub snippet: String,
}

/// Find up to `max_hits` non-overlapping, case- and diacritic-folded
/// occurrences of `needle` in `haystack`.
///
/// An empty needle matches nothing — an empty query should show the unfiltered
/// list, not every byte of every note.
pub fn find(haystack: &str, needle: &str, max_hits: usize) -> Vec<Hit> {
    let pattern: Vec<char> = fold_str(needle).chars().collect();
    if pattern.is_empty() || max_hits == 0 {
        return Vec::new();
    }

    let mut hits = Vec::new();
    let mut line: u32 = 1;
    // Newlines up to `counted` are already reflected in `line`; the counter only
    // ever moves forward, so line numbering stays O(n) over the whole haystack.
    let mut counted = 0usize;
    let mut at = 0usize;

    while at < haystack.len() {
        line += haystack[counted..at]
            .bytes()
            .filter(|b| *b == b'\n')
            .count() as u32;
        counted = at;

        if let Some(end) = match_at(haystack, at, &pattern) {
            hits.push(Hit {
                line,
                span: (at, end),
                snippet: snippet(haystack, at, end),
            });
            if hits.len() >= max_hits {
                break;
            }
            at = end;
        } else {
            at = next_boundary(haystack, at);
        }
    }

    hits
}

/// Byte offset of the character after the one starting at `at`.
fn next_boundary(s: &str, at: usize) -> usize {
    match s[at..].chars().next() {
        Some(c) => at + c.len_utf8(),
        None => s.len(),
    }
}

/// Try to match `pattern` (already folded) starting exactly at `start`.
/// Returns the end byte offset in `haystack` on success.
fn match_at(haystack: &str, start: usize, pattern: &[char]) -> Option<usize> {
    let mut pi = 0usize;

    for (off, c) in haystack[start..].char_indices() {
        let folded = fold_char(c);
        let expansion = folded.as_slice();

        if expansion.is_empty() {
            // A combining mark folds to nothing. It is transparent *inside* a
            // match ("cafe\u{301}" matches "cafe"), but a match may never begin
            // on one, or every mark in the document would be a match start.
            if pi == 0 {
                return None;
            }
            continue;
        }

        for (k, fc) in expansion.iter().enumerate() {
            if *fc != pattern[pi] {
                return None;
            }
            pi += 1;
            if pi == pattern.len() {
                // The needle must not end mid-expansion: searching for "s" in
                // "ß" would otherwise report a span covering half a character.
                if k + 1 != expansion.len() {
                    return None;
                }
                let mut end = start + off + c.len_utf8();
                // Trailing combining marks belong to the grapheme that just
                // matched. A highlight that stops before them cuts a character
                // in half on screen, which is what the user actually sees.
                while let Some(mark) = haystack[end..].chars().next() {
                    if !is_combining_mark(mark) {
                        break;
                    }
                    end += mark.len_utf8();
                }
                return Some(end);
            }
        }
    }

    None
}

/// The matched line, windowed around `[start, end)` and elided where cut.
fn snippet(haystack: &str, start: usize, end: usize) -> String {
    let line_start = haystack[..start].rfind('\n').map_or(0, |i| i + 1);
    let mut line_end = haystack[end..]
        .find('\n')
        .map_or(haystack.len(), |i| end + i);
    if line_end > end && haystack.as_bytes()[line_end - 1] == b'\r' {
        line_end -= 1;
    }

    let from = haystack[line_start..start]
        .char_indices()
        .rev()
        .nth(SNIPPET_CONTEXT - 1)
        .map_or(line_start, |(i, _)| line_start + i);
    let to = haystack[end..line_end]
        .char_indices()
        .nth(SNIPPET_CONTEXT)
        .map_or(line_end, |(i, _)| end + i);

    let mut out = String::with_capacity(to - from + 8);
    if from > line_start {
        out.push('…');
    }
    out.push_str(&haystack[from..to]);
    if to < line_end {
        out.push('…');
    }
    out
}

// ---------------------------------------------------------------------------
// Folding
// ---------------------------------------------------------------------------

/// The folded expansion of one `char`: zero characters for a combining mark,
/// one in the common case, and up to three for the handful that grow (`ß` →
/// `ss`, `Æ` → `ae`, `ﬁ`-style lowercase expansions).
#[derive(Clone, Copy)]
struct Folded {
    buf: [char; 3],
    len: u8,
}

impl Folded {
    fn empty() -> Self {
        Self {
            buf: ['\0'; 3],
            len: 0,
        }
    }

    fn push(&mut self, c: char) {
        if (self.len as usize) < self.buf.len() {
            self.buf[self.len as usize] = c;
            self.len += 1;
        }
    }

    fn as_slice(&self) -> &[char] {
        &self.buf[..self.len as usize]
    }
}

/// Case- and diacritic-fold a single character.
fn fold_char(c: char) -> Folded {
    let mut out = Folded::empty();
    if is_combining_mark(c) {
        return out;
    }
    if let Some(ascii) = latin_fold(c) {
        for a in ascii.chars() {
            out.push(a);
        }
        return out;
    }
    for lc in c.to_lowercase() {
        // Lowercasing can *introduce* a mark (`İ` → `i` + U+0307); drop it here
        // so the two spellings of a dotted capital I fold to the same thing.
        if !is_combining_mark(lc) {
            out.push(lc);
        }
    }
    out
}

/// The folded characters of `s`, one at a time, without materialising the fold.
///
/// The one folding walk in the crate: [`fold_str`] and [`fold_cmp`] are both
/// this iterator, so a needle, a title comparison and a tag path can never end
/// up folded by two slightly different rules.
struct FoldChars<'a> {
    chars: std::str::Chars<'a>,
    /// The expansion of the character being drained; `ß` yields two.
    pending: Folded,
    /// How far into `pending` the caller has read.
    at: usize,
}

impl<'a> FoldChars<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            chars: s.chars(),
            pending: Folded::empty(),
            at: 0,
        }
    }
}

impl Iterator for FoldChars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        loop {
            if let Some(c) = self.pending.as_slice().get(self.at) {
                self.at += 1;
                return Some(*c);
            }
            // A combining mark folds to nothing, so an empty expansion is a
            // skip rather than the end of the string.
            self.pending = fold_char(self.chars.next()?);
            self.at = 0;
        }
    }
}

/// Fold a whole string. For short inputs only — needles, titles, tag paths.
/// The haystack side of [`find`] never allocates one of these.
pub(crate) fn fold_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    out.extend(FoldChars::new(s));
    out
}

/// Order two short strings by their folded form, allocating nothing.
///
/// This exists because the alternative is `fold_str(a).cmp(&fold_str(b))` inside
/// a comparator, and a comparator runs O(n log n) times: sorting a
/// ten-thousand-note vault by a folded title that way is a quarter of a million
/// throwaway `String`s for an answer that never needed one.
///
/// Folded-equal is reported as [`Ordering::Equal`], so a caller that needs a
/// *total* order over distinct notes must follow this with a term that cannot
/// tie — `Ábc` and `abc` are equal here by design, exactly as they are to
/// [`find`].
pub(crate) fn fold_cmp(a: &str, b: &str) -> Ordering {
    let mut left = FoldChars::new(a);
    let mut right = FoldChars::new(b);
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
            }
        }
    }
}

/// The combining-mark blocks a decomposed Latin string actually uses.
fn is_combining_mark(c: char) -> bool {
    let cp = c as u32;
    (0x0300..=0x036F).contains(&cp)      // Combining Diacritical Marks
        || (0x1AB0..=0x1AFF).contains(&cp)  // …Extended
        || (0x1DC0..=0x1DFF).contains(&cp)  // …Supplement
        || (0xFE20..=0xFE2F).contains(&cp) // Combining Half Marks
}

/// Lowercase ASCII equivalents for Latin-1 Supplement, U+00C0..=U+00FF.
/// An empty entry means "no ASCII equivalent" — the two division signs.
const LATIN1: [&str; 64] = [
    // À  Á    Â    Ã    Ä    Å    Æ     Ç    È    É    Ê    Ë    Ì    Í    Î    Ï
    "a", "a", "a", "a", "a", "a", "ae", "c", "e", "e", "e", "e", "i", "i", "i", "i",
    // Ð  Ñ    Ò    Ó    Ô    Õ    Ö    ×   Ø    Ù    Ú    Û    Ü    Ý    Þ     ß
    "d", "n", "o", "o", "o", "o", "o", "", "o", "u", "u", "u", "u", "y", "th", "ss",
    // à  á    â    ã    ä    å    æ     ç    è    é    ê    ë    ì    í    î    ï
    "a", "a", "a", "a", "a", "a", "ae", "c", "e", "e", "e", "e", "i", "i", "i", "i",
    // ð  ñ    ò    ó    ô    õ    ö    ÷   ø    ù    ú    û    ü    ý    þ     ÿ
    "d", "n", "o", "o", "o", "o", "o", "", "o", "u", "u", "u", "u", "y", "th", "y",
];

/// Lowercase ASCII equivalents for Latin Extended-A, U+0100..=U+017F.
const LATIN_EXT_A: [&str; 128] = [
    // Ā   ā    Ă    ă    Ą    ą    Ć    ć    Ĉ    ĉ    Ċ    ċ    Č    č    Ď    ď
    "a", "a", "a", "a", "a", "a", "c", "c", "c", "c", "c", "c", "c", "c", "d", "d",
    // Đ   đ    Ē    ē    Ĕ    ĕ    Ė    ė    Ę    ę    Ě    ě    Ĝ    ĝ    Ğ    ğ
    "d", "d", "e", "e", "e", "e", "e", "e", "e", "e", "e", "e", "g", "g", "g", "g",
    // Ġ   ġ    Ģ    ģ    Ĥ    ĥ    Ħ    ħ    Ĩ    ĩ    Ī    ī    Ĭ    ĭ    Į    į
    "g", "g", "g", "g", "h", "h", "h", "h", "i", "i", "i", "i", "i", "i", "i", "i",
    // İ   ı    Ĳ     ĳ     Ĵ    ĵ    Ķ    ķ    ĸ    Ĺ    ĺ    Ļ    ļ    Ľ    ľ    Ŀ
    "i", "i", "ij", "ij", "j", "j", "k", "k", "k", "l", "l", "l", "l", "l", "l", "l",
    // ŀ   Ł    ł    Ń    ń    Ņ    ņ    Ň    ň    ŉ    Ŋ    ŋ    Ō    ō    Ŏ    ŏ
    "l", "l", "l", "n", "n", "n", "n", "n", "n", "n", "n", "n", "o", "o", "o", "o",
    // Ő   ő    Œ     œ     Ŕ    ŕ    Ŗ    ŗ    Ř    ř    Ś    ś    Ŝ    ŝ    Ş    ş
    "o", "o", "oe", "oe", "r", "r", "r", "r", "r", "r", "s", "s", "s", "s", "s", "s",
    // Š   š    Ţ    ţ    Ť    ť    Ŧ    ŧ    Ũ    ũ    Ū    ū    Ŭ    ŭ    Ů    ů
    "s", "s", "t", "t", "t", "t", "t", "t", "u", "u", "u", "u", "u", "u", "u", "u",
    // Ű   ű    Ų    ų    Ŵ    ŵ    Ŷ    ŷ    Ÿ    Ź    ź    Ż    ż    Ž    ž    ſ
    "u", "u", "u", "u", "w", "w", "y", "y", "y", "z", "z", "z", "z", "z", "z", "s",
];

fn latin_fold(c: char) -> Option<&'static str> {
    let cp = c as u32;
    let entry = if (0x00C0..=0x00FF).contains(&cp) {
        LATIN1[(cp - 0x00C0) as usize]
    } else if (0x0100..=0x017F).contains(&cp) {
        LATIN_EXT_A[(cp - 0x0100) as usize]
    } else {
        return None;
    };
    if entry.is_empty() {
        None
    } else {
        Some(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case_and_latin_diacritics() {
        assert_eq!(fold_str("Café DÉJÀ Vu"), "cafe deja vu");
        assert_eq!(fold_str("Straße"), "strasse");
        assert_eq!(fold_str("Łódź"), "lodz");
        assert_eq!(fold_str("ŒUVRE"), "oeuvre");
    }

    #[test]
    fn folds_decomposed_and_precomposed_to_the_same_thing() {
        // NFC "é" and NFD "e" + U+0301 are the same word to a searching human.
        assert_eq!(fold_str("caf\u{e9}"), fold_str("cafe\u{301}"));
    }

    #[test]
    fn leaves_scripts_without_diacritics_alone() {
        assert_eq!(fold_str("日本語のノート"), "日本語のノート");
        assert_eq!(fold_str("ПРИВЕТ"), "привет");
    }

    #[test]
    fn spans_point_into_the_original_bytes_not_the_folded_copy() {
        let hay = "Meeting about the Café renovation";
        let hits = find(hay, "cafe", 10);
        assert_eq!(hits.len(), 1);
        let (s, e) = hits[0].span;
        assert_eq!(&hay[s..e], "Café");
    }

    #[test]
    fn matches_across_a_combining_mark_without_swallowing_it() {
        let hay = "cafe\u{301} society";
        let hits = find(hay, "café", 10);
        assert_eq!(hits.len(), 1);
        let (s, e) = hits[0].span;
        assert_eq!(&hay[s..e], "cafe\u{301}");
    }

    #[test]
    fn refuses_to_match_half_of_an_expanded_character() {
        // "ß" folds to "ss"; a needle of "s" must not report a span that cuts
        // the character in half.
        assert!(find("Straße", "s", 10)
            .iter()
            .all(|h| &"Straße"[h.span.0..h.span.1] != "ß"));
    }

    #[test]
    fn reports_one_based_line_numbers() {
        let hay = "alpha\nbeta\ngamma needle\ndelta";
        let hits = find(hay, "NEEDLE", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 3);
    }

    #[test]
    fn hits_do_not_overlap_and_respect_max_hits() {
        let hay = "aaaa";
        assert_eq!(find(hay, "aa", 10).len(), 2);
        assert_eq!(find(hay, "aa", 1).len(), 1);
        assert_eq!(find(hay, "aa", 0).len(), 0);
    }

    #[test]
    fn an_empty_needle_matches_nothing() {
        assert!(find("anything at all", "", 10).is_empty());
    }

    #[test]
    fn snippet_windows_the_line_and_marks_where_it_cut() {
        let long = "x".repeat(200);
        let hay = format!("{long} needle {long}");
        let hits = find(&hay, "needle", 1);
        assert_eq!(hits.len(), 1);
        let snip = &hits[0].snippet;
        assert!(snip.starts_with('…'), "{snip}");
        assert!(snip.ends_with('…'), "{snip}");
        assert!(snip.contains("needle"));
    }

    #[test]
    fn snippet_stops_at_the_line_and_drops_a_carriage_return() {
        let hits = find("before\r\nthe needle here\r\nafter", "needle", 1);
        assert_eq!(hits[0].snippet, "the needle here");
    }
}
