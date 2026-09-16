//! The text of a task's prompt, without its metadata and opening title.

use super::{frontmatter::Frontmatter, naming::strip_atx_heading};

/// Remove frontmatter and exactly one leading ATX heading. Everything after
/// the heading's line ending is borrowed verbatim, including blank lines.
pub fn body_after_heading(source: &str) -> &str {
    let (_, offset) = Frontmatter::parse(source);
    let body = &source[offset..];
    let mut at = 0;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            at += line.len();
            continue;
        }
        return if strip_atx_heading(trimmed) != trimmed {
            &body[at + line.len()..]
        } else {
            body
        };
    }
    body
}

#[cfg(test)]
mod tests {
    use super::body_after_heading;

    #[test]
    fn removes_metadata_and_only_first_heading_preserving_remainder() {
        let source = "---\ntitle: Task\n---\n\n# Title\r\n\r\n  café 🦀\r\n## Keep me\n\tend  ";
        assert_eq!(
            body_after_heading(source),
            "\r\n  café 🦀\r\n## Keep me\n\tend  "
        );
    }

    #[test]
    fn non_heading_body_is_verbatim() {
        for body in [
            "",
            " \n\t",
            "\n#tag\nbody",
            "prose\n# Later\n",
            "---\nunterminated",
        ] {
            assert_eq!(body_after_heading(body), body);
        }
    }

    #[test]
    fn heading_only_and_empty_heading_have_no_body() {
        assert_eq!(body_after_heading("# Title"), "");
        assert_eq!(body_after_heading("#\nrest\n"), "rest\n");
    }

    #[test]
    fn frontmatter_without_heading_preserves_body_spacing() {
        assert_eq!(
            body_after_heading("---\ntitle: Task\n---\n\n prose  \n"),
            "\n prose  \n"
        );
    }
}
