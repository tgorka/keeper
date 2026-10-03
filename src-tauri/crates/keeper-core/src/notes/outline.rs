//! A note's headings as the editor's caret sees them (story 91.2).
//!
//! The docked notes view tells the person's proxy which heading the caret is
//! under. Headings are ATX (`#` … `######`, indented up to three spaces as
//! CommonMark allows) outside fenced code, fences read as
//! [`super::chunk::chunk_body`] reads them; lines count in the editor's
//! buffer, which is the note's body without its frontmatter — and, as the
//! editor counts, a body ending in a newline has an empty last line.

use super::line_bounds;
use super::live_editor::split_note;

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
    let mut headings: Vec<(usize, &str)> = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut at = 0;
    let mut number = 0u32;
    while let Some((start, end, next)) = line_bounds(body, at) {
        number += 1;
        if number > line {
            break;
        }
        let text = &body[start..end];
        let trimmed = text.trim_start_matches(' ');
        let indent = text.len() - trimmed.len();
        let marker = trimmed.as_bytes().first().copied();
        let run = marker.map_or(0, |m| trimmed.bytes().take_while(|b| *b == m).count());
        let is_fence = indent <= 3 && matches!(marker, Some(b'`' | b'~')) && run >= 3;
        if let Some((m, n)) = fence {
            if is_fence && marker == Some(m) && run >= n && trimmed[run..].trim().is_empty() {
                fence = None;
            }
        } else if is_fence {
            fence = marker.map(|m| (m, run));
        } else {
            let level = trimmed.bytes().take_while(|b| *b == b'#').count();
            if indent <= 3
                && (1..=6).contains(&level)
                && (trimmed.len() == level || trimmed.as_bytes()[level].is_ascii_whitespace())
            {
                headings.retain(|(depth, _)| *depth < level);
                headings.push((level, trimmed[level..].trim().trim_end_matches('#').trim()));
            }
        }
        at = next;
    }
    // The editor's empty line after a final newline is the end of the note.
    let tail = number + 1 == line && body.ends_with('\n');
    if (number < line && !tail) || headings.is_empty() {
        return None;
    }
    Some(
        headings
            .iter()
            .map(|(_, heading)| *heading)
            .collect::<Vec<_>>()
            .join(" › "),
    )
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
}
