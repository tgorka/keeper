//! A bounded plain-text preview, shared by note rows and their hover hints.

use crate::notes::naming::strip_atx_heading;

/// Remove presentation markup, fold whitespace and retain at most `budget`
/// Unicode scalar values. Malformed markup remains readable literal text.
///
/// `body` is a note's body with its frontmatter already removed, which is what
/// every caller holds: the frontmatter fence is found once, by whoever read the
/// file. Parsing it again here would be worse than redundant — `Fence::find`
/// reads any body that opens with `---` and later carries another `---` or a
/// bare `...` line as frontmatter, so a note starting with a thematic break
/// would lose its opening prose from the row and from its hint.
#[must_use]
pub fn prose(body: &str, budget: usize) -> String {
    let mut out = Preview::new(budget);
    let mut fence: Option<(char, usize)> = None;
    for line in body.lines() {
        if out.remaining == 0 {
            break;
        }
        let mut line = line.trim();
        if let Some((marker, width)) = fence {
            if line.chars().take_while(|c| *c == marker).count() >= width
                && line.trim_start_matches(marker).trim().is_empty()
            {
                fence = None;
            } else {
                out.text(line);
            }
        } else {
            // Quotes and lists may be nested; strip only actual prefixes, not
            // punctuation in prose or numbers such as 3.14.
            loop {
                if let Some(rest) = line.strip_prefix('>') {
                    line = rest.trim_start();
                } else if let Some(rest) = list_body(line) {
                    line = rest.trim_start();
                } else {
                    break;
                }
            }
            if let Some(marker @ ('`' | '~')) = line.chars().next() {
                let width = line.chars().take_while(|c| *c == marker).count();
                if width >= 3 {
                    fence = Some((marker, width));
                    out.space();
                    continue;
                }
            }
            // Setext underlines and thematic rules carry no preview text.
            if !line.is_empty()
                && (line.chars().all(|c| c == '=')
                    || line.chars().all(|c| c == '-')
                    || line.chars().all(|c| c == '*'))
            {
                continue;
            }
            inline(strip_atx_heading(line), &mut out);
        }
        out.space();
    }
    out.text
}

fn list_body(line: &str) -> Option<&str> {
    for prefix in ["- ", "* ", "+ ", "-\t", "*\t", "+\t"] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) {
            if rest.starts_with(char::is_whitespace) {
                return Some(rest);
            }
        }
    }
    None
}

struct Preview {
    text: String,
    remaining: usize,
    pending_space: bool,
}

impl Preview {
    fn new(budget: usize) -> Self {
        Self {
            text: String::new(),
            remaining: budget,
            pending_space: false,
        }
    }

    fn space(&mut self) {
        self.pending_space = !self.text.is_empty();
    }

    fn push(&mut self, ch: char) {
        if ch.is_whitespace() {
            self.space();
            return;
        }
        if self.pending_space && self.remaining > 1 {
            self.text.push(' ');
            self.remaining -= 1;
        } else if self.pending_space && self.remaining == 1 {
            // Never finish the preview on a trailing folded space.
            self.remaining = 0;
        }
        self.pending_space = false;
        if self.remaining > 0 {
            self.text.push(ch);
            self.remaining -= 1;
        }
    }

    fn text(&mut self, text: &str) {
        for ch in text.chars() {
            if self.remaining == 0 {
                break;
            }
            self.push(ch);
        }
    }
}

// Balanced destinations can contain parentheses and escaped delimiters. Byte
// positions come only from char_indices (or ASCII delimiters), never char counts.
fn closing(text: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (at, ch) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(at);
            }
        }
    }
    None
}

fn inline(mut text: &str, out: &mut Preview) {
    // No recursion: deeply nested hand-written markup must not consume the
    // stack. Linked labels are streamed through the same scanner via a suffix
    // stack; each pending suffix owns no text.
    let mut suffixes = Vec::new();
    let mut previous = None;
    loop {
        if out.remaining == 0 {
            break;
        }
        let Some(ch) = text.chars().next() else {
            if let Some(suffix) = suffixes.pop() {
                text = suffix;
                continue;
            }
            break;
        };
        if ch == '\\' {
            if let Some(next) = text[1..].chars().next().filter(char::is_ascii_punctuation) {
                out.push(next);
                previous = Some(next);
                text = &text[1 + next.len_utf8()..];
                continue;
            }
        }
        let label_start = if text.starts_with("![") { 1 } else { 0 };
        if text[label_start..].starts_with('[') {
            let label = &text[label_start..];
            if let Some(end) = closing(label, '[', ']') {
                let after = &label[end + 1..];
                let destination_end = if after.starts_with('(') {
                    closing(after, '(', ')')
                } else if after.starts_with('[') {
                    closing(after, '[', ']')
                } else {
                    None
                };
                if let Some(destination_end) = destination_end {
                    suffixes.push(&after[destination_end + 1..]);
                    text = &label[1..end];
                    continue;
                }
            }
        }
        if ch == '`' {
            let width = text.bytes().take_while(|b| *b == b'`').count();
            let marker = &text[..width];
            if let Some(end) = text[width..].find(marker) {
                out.text(&text[width..width + end]);
                text = &text[width + end + width..];
                previous = Some('`');
                continue;
            }
        }
        if ch == '*' || ch == '_' {
            let width = text.chars().take_while(|c| *c == ch).count();
            let after = &text[width..];
            let intraword = ch == '_'
                && previous.is_some_and(char::is_alphanumeric)
                && after.starts_with(char::is_alphanumeric);
            if !intraword && !after.starts_with(char::is_whitespace) {
                let marker = &text[..width];
                if let Some(end) = after.find(marker).filter(|end| *end > 0) {
                    // A closing run can finish nested emphasis too: the last
                    // two stars of `**bold *italic***` close the outer strong.
                    let run = after[end..].chars().take_while(|c| *c == ch).count();
                    let end = end + run - width;
                    suffixes.push(&after[end + width..]);
                    text = &after[..end];
                    continue;
                }
            }
        }
        out.push(ch);
        previous = Some(ch);
        text = &text[ch.len_utf8()..];
    }
}

#[cfg(test)]
mod tests {
    use super::prose;

    #[test]
    fn markdown_becomes_prose_before_the_budget_is_applied() {
        // A body, not a whole note: whoever read the file removed the
        // frontmatter, and `prose` must not look for it again (a body that
        // opens with a thematic break would lose its first paragraph).
        let source = "# Heading #\nSetext\n======\n**bold** _emphasis_ and `code`\n[link](https://host/a_(b)) ![image](x.png)\n- bullet\n2. numbered\n> quote\n```rust\nlet x = 1;\n```\n";
        assert_eq!(
            prose(source, 300),
            "Heading Setext bold emphasis and code link image bullet numbered quote let x = 1;"
        );
        assert_eq!(prose("**😀é界** more", 3), "😀é界");
        assert_eq!(prose("word next", 5), "word");
        assert_eq!(prose(source, 0), "");
        // The defect this signature prevents: a body whose first line is a
        // thematic break and which later carries another one reads as
        // frontmatter to any fence parser.
        assert_eq!(
            prose("---\nfirst paragraph\n---\nsecond\n", 100),
            "first paragraph second"
        );
    }

    #[test]
    fn preserves_literal_punctuation_and_handles_incomplete_unicode_markup() {
        assert_eq!(
            prose("#tag snake_case 3.14 \\*literal\\*", 100),
            "#tag snake_case 3.14 *literal*"
        );
        assert_eq!(prose("**未完 [é](broken `😀", 100), "**未完 [é](broken `😀");
        assert_eq!(
            prose("[**linked** label][ref] ![é](image)\n> - nested", 100),
            "linked label é nested"
        );
        assert_eq!(
            prose("~~~text\n**literal**\n~~~\nafter", 100),
            "**literal** after"
        );
        assert_eq!(
            prose("**bold *italic*** and __strong__", 100),
            "bold italic and strong"
        );
    }

    #[test]
    fn every_unicode_prefix_is_safe_and_bounded() {
        let source = "---\nx: é\n---\n> **😀** [界](é(字)) `日` ![画](像)";
        for end in source
            .char_indices()
            .map(|(at, _)| at)
            .chain([source.len()])
        {
            for budget in 0..12 {
                let preview = prose(&source[..end], budget);
                assert!(preview.chars().count() <= budget);
                assert_eq!(preview.trim(), preview);
            }
        }
    }
}
