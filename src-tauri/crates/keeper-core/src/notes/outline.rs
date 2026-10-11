//! A note's headings and lines as the editor's buffer counts them (stories
//! 91.2 and 91.3).
//!
//! The docked notes view tells the person's proxy which heading the caret is
//! under ([`heading_at`]); the proxy's surface tools open a note at a heading
//! ([`find_heading`]) and name lines a file read counted ([`body_lines`]).
//! Headings are ATX (`#` … `######`, indented up to three spaces as
//! CommonMark allows) and setext (a paragraph line underlined with `=` or
//! `-`) outside fenced code, fences read as [`super::chunk::chunk_body`]
//! reads them; lines count in the editor's buffer, which is the note's body
//! without its frontmatter — and, as the editor counts, a body ending in a
//! newline has an empty last line.

use super::line_bounds;
use super::live_editor::split_note;

/// One heading of a body: its 1-based line, its level and its text.
struct Heading<'a> {
    line: u32,
    level: usize,
    text: &'a str,
}

/// Every heading of `body` outside fenced code, in order, and how many lines
/// the body has.
fn headings(body: &str) -> (Vec<Heading<'_>>, u32) {
    let mut found = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    // The line above, when it could be a setext heading's text.
    let mut paragraph: Option<(u32, &str)> = None;
    let mut at = 0;
    let mut number = 0u32;
    while let Some((start, end, next)) = line_bounds(body, at) {
        at = next;
        number += 1;
        let text = &body[start..end];
        let trimmed = text.trim_start_matches(' ');
        let indent = text.len() - trimmed.len();
        let marker = trimmed.as_bytes().first().copied();
        let run = marker.map_or(0, |m| trimmed.bytes().take_while(|b| *b == m).count());
        let is_fence = indent <= 3 && matches!(marker, Some(b'`' | b'~')) && run >= 3;
        let above = paragraph.take();
        if let Some((m, n)) = fence {
            if is_fence && marker == Some(m) && run >= n && trimmed[run..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if is_fence {
            fence = marker.map(|m| (m, run));
            continue;
        }
        let level = trimmed.bytes().take_while(|b| *b == b'#').count();
        if indent <= 3
            && (1..=6).contains(&level)
            && (trimmed.len() == level || trimmed.as_bytes()[level].is_ascii_whitespace())
        {
            found.push(Heading {
                line: number,
                level,
                text: trimmed[level..].trim().trim_end_matches('#').trim(),
            });
            continue;
        }
        let underline =
            indent <= 3 && matches!(marker, Some(b'=' | b'-')) && trimmed[run..].trim().is_empty();
        if let (true, Some((line, above))) = (underline, above) {
            found.push(Heading {
                line,
                level: if marker == Some(b'=') { 1 } else { 2 },
                text: above.trim(),
            });
            continue;
        }
        let item = matches!(marker, Some(b'-' | b'*' | b'+' | b'>'))
            && trimmed
                .as_bytes()
                .get(1)
                .is_none_or(u8::is_ascii_whitespace);
        if indent <= 3 && !trimmed.trim().is_empty() && !item && !underline {
            paragraph = Some((number, text));
        }
    }
    (found, number)
}

/// Each heading's trail: the headings enclosing it and itself, joined by
/// ` › ` (`Plans › Q3`).
fn trails<'a>(found: &[Heading<'a>]) -> Vec<String> {
    let mut open: Vec<(usize, &str)> = Vec::new();
    found
        .iter()
        .map(|heading| {
            open.retain(|(depth, _)| *depth < heading.level);
            open.push((heading.level, heading.text));
            open.iter()
                .map(|(_, text)| *text)
                .collect::<Vec<_>>()
                .join(" › ")
        })
        .collect()
}

/// The heading the 1-based body `line` of `note` (a whole note, read from
/// disk) is under: [`heading_in`] over its body.
pub fn heading_at(note: &str, line: u32) -> Option<String> {
    heading_in(split_note(note).1, line)
}

/// The heading the 1-based `line` of `body` (the editor's buffer, or as much
/// of it as runs through `line`) is under, as the trail of its enclosing
/// headings joined by ` › ` (`Plans › Q3`); `None` above the first heading
/// or past the end.
pub fn heading_in(body: &str, line: u32) -> Option<String> {
    let (found, lines) = headings(body);
    // The editor's empty line after a final newline is the end of the note.
    let tail = lines + 1 == line && body.ends_with('\n');
    if line > lines && !tail {
        return None;
    }
    let above = found
        .iter()
        .take_while(|heading| heading.line <= line)
        .count();
    trails(&found[..above]).pop()
}

/// The section of `note` under the heading `wanted`, as its first and last
/// body lines: from the heading to the line before the next heading of the
/// same or a higher level, or the end. `wanted` is a heading's text or its
/// trail (`Plans › Q3`, as [`heading_at`] gives it); the exact text first,
/// then ignoring case, the first match winning. `None` when no heading
/// outside fenced code and frontmatter has it.
pub fn find_heading(note: &str, wanted: &str) -> Option<(u32, u32)> {
    let (_, body) = split_note(note);
    let (found, lines) = headings(body);
    let trails = trails(&found);
    let wanted = wanted.trim();
    let exact = |at: usize| found[at].text == wanted || trails[at] == wanted;
    let folded = |at: usize| {
        found[at].text.to_lowercase() == wanted.to_lowercase()
            || trails[at].to_lowercase() == wanted.to_lowercase()
    };
    let at = (0..found.len())
        .find(|at| exact(*at))
        .or_else(|| (0..found.len()).find(|at| folded(*at)))?;
    let heading = &found[at];
    let end = found[at + 1..]
        .iter()
        .find(|next| next.level <= heading.level)
        .map_or(lines, |next| next.line - 1);
    Some((heading.line, end))
}

/// Why a range of a file's lines names no lines of its body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinesRefusal {
    #[error("The range starts after it ends: lines are 1-based and inclusive, from ≤ to.")]
    Backwards,
    #[error("Lines 1–{frontmatter} are the note's frontmatter, which the editor does not show; name lines after them.")]
    InFrontmatter { frontmatter: u32 },
    #[error("The note has {lines} lines; the range ends past them.")]
    PastEnd { lines: u32 },
}

/// Lines `from`–`to` of the file `note`, numbered as `drive_read` numbers
/// them (1-based, inclusive, the frontmatter counted), as the editor's
/// buffer numbers them, with their text joined by `\n` (no terminator after
/// the last). A range inside the frontmatter, or past the end, is refused.
pub fn body_lines(note: &str, from: u32, to: u32) -> Result<(u32, u32, String), LinesRefusal> {
    if from == 0 || from > to {
        return Err(LinesRefusal::Backwards);
    }
    let (frontmatter, body) = split_note(note);
    let mut skipped = 0u32;
    let mut at = 0;
    while let Some((_, _, next)) = line_bounds(frontmatter, at) {
        skipped += 1;
        at = next;
    }
    if from <= skipped {
        return Err(LinesRefusal::InFrontmatter {
            frontmatter: skipped,
        });
    }
    let (first, last) = (from - skipped, to - skipped);
    let mut text: Vec<&str> = Vec::new();
    let mut at = 0;
    let mut number = 0u32;
    while let Some((start, end, next)) = line_bounds(body, at) {
        number += 1;
        if (first..=last).contains(&number) {
            text.push(&body[start..end]);
        }
        at = next;
    }
    if last > number {
        return Err(LinesRefusal::PastEnd {
            lines: skipped + number,
        });
    }
    Ok((first, last, text.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "---\ntitle: x\n---\nintro\n# Plans\ntext\n## Q3\n```\n# not a heading\n```\nmore\n# Done\n";

    #[test]
    fn the_caret_is_under_the_trail_of_its_headings() {
        assert_eq!(heading_at(NOTE, 1), None, "above the first heading");
        assert_eq!(heading_at(NOTE, 2).as_deref(), Some("Plans"));
        assert_eq!(heading_at(NOTE, 4).as_deref(), Some("Plans › Q3"));
        // A `#` line in a fence is code, not a heading.
        assert_eq!(heading_at(NOTE, 6).as_deref(), Some("Plans › Q3"));
        assert_eq!(heading_at(NOTE, 8).as_deref(), Some("Plans › Q3"));
        assert_eq!(heading_at(NOTE, 9).as_deref(), Some("Done"));
        // A body ending in a newline: the editor's empty last line, where a
        // new note's caret is put, is under the last heading.
        assert_eq!(heading_at(NOTE, 10).as_deref(), Some("Done"));
        assert_eq!(heading_at(NOTE, 11), None, "past the end");
        assert_eq!(
            heading_at("# Standup\n\n## Agenda\n", 4).as_deref(),
            Some("Standup › Agenda")
        );
        assert_eq!(heading_at("# Standup\n## Agenda", 3), None, "no line 3");
        // CommonMark: up to three spaces before a heading; four is code.
        assert_eq!(heading_at("  # Plans\ntext\n", 2).as_deref(), Some("Plans"));
        assert_eq!(heading_at("    # Plans\ntext\n", 2), None);
        // The editor's buffer is the body already: a rule on its first line
        // is not frontmatter.
        assert_eq!(
            heading_in("---\n# Plans\n---\ntext", 4).as_deref(),
            Some("Plans")
        );
    }

    const HEADINGS: &str = "---\ntitle: Heading in frontmatter\n# Fake\n---\nIntro\n\nBudget\n======\n\nlines\n\nRisks\n---\nrisk one\n# budget\n```\n# Hidden\n```\n## Q3\nnumbers\n- item\n---\n";

    #[test]
    fn find_heading_reads_atx_and_setext_and_prefers_the_exact_text() {
        // Body lines: 1 Intro, 2 blank, 3 Budget, 4 ======, 5 blank, 6
        // lines, 7 blank, 8 Risks, 9 ---, 10 risk one, 11 # budget, 12–14
        // the fence, 15 ## Q3, 16 numbers, 17 - item, 18 ---.
        // A setext heading, level 1, runs to the next level-1 heading.
        assert_eq!(find_heading(HEADINGS, "Budget"), Some((3, 10)));
        // A level-2 setext heading ends at the next heading of level ≤ 2.
        assert_eq!(find_heading(HEADINGS, "Risks"), Some((8, 10)));
        // The exact text before a case-insensitive match, whichever comes
        // first in the note.
        assert_eq!(find_heading(HEADINGS, "budget"), Some((11, 18)));
        assert_eq!(find_heading(HEADINGS, "RISKS"), Some((8, 10)));
        // A trail names its heading too.
        assert_eq!(find_heading(HEADINGS, "budget › Q3"), Some((15, 18)));
        // Inside a fence, inside the frontmatter, or nowhere: none.
        assert_eq!(find_heading(HEADINGS, "Hidden"), None);
        assert_eq!(find_heading(HEADINGS, "Fake"), None);
        assert_eq!(find_heading(HEADINGS, "Heading in frontmatter"), None);
        assert_eq!(find_heading(HEADINGS, "Missing"), None);
        // A list item over `---` is a list and a rule, not a heading.
        assert_eq!(find_heading(HEADINGS, "- item"), None);
        assert_eq!(find_heading(HEADINGS, "item"), None);
    }

    #[test]
    fn file_lines_become_the_buffers_lines() {
        // Lines 1–3 of NOTE are its frontmatter; line 4 is the body's first.
        assert_eq!(
            body_lines(NOTE, 4, 5),
            Ok((1, 2, "intro\n# Plans".to_owned()))
        );
        assert_eq!(body_lines(NOTE, 12, 12), Ok((9, 9, "# Done".to_owned())));
        assert_eq!(
            body_lines(NOTE, 3, 5),
            Err(LinesRefusal::InFrontmatter { frontmatter: 3 })
        );
        assert_eq!(
            body_lines(NOTE, 11, 13),
            Err(LinesRefusal::PastEnd { lines: 12 })
        );
        assert_eq!(body_lines(NOTE, 5, 4), Err(LinesRefusal::Backwards));
        assert_eq!(body_lines(NOTE, 0, 4), Err(LinesRefusal::Backwards));
        // A note with no frontmatter counts as it is.
        assert_eq!(body_lines("a\nb\nc", 2, 3), Ok((2, 3, "b\nc".to_owned())));
    }
}
