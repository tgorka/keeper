//! Note template expansion (FR-100).
//!
//! A **closed** placeholder set — `{{date:FMT}}`, `{{time:FMT}}`, `{{title}}`,
//! `{{cursor}}`, `{{id}}` — and everything else is left exactly as written. That
//! is not laziness: templates are ordinary notes under `templates/`, they sync,
//! and an agent may write one. A template engine that evaluates arbitrary
//! expressions in a file an agent can author is a code-execution surface, and a
//! template engine that *guesses* at unknown braces silently eats a user's
//! literal `{{TODO}}`. Leaving the unknown alone is the only option that is both
//! safe and honest.
//!
//! Dates come from `ctx.now_local`, an RFC 3339 string the shell already has —
//! keeper-core carries no clock, and a pure function that reads the wall clock
//! could not be tested.

use std::fmt::Write as _;

/// What a template is allowed to know about the note being created.
#[derive(Debug, Clone, Default)]
pub struct TemplateCtx {
    /// The note's title, as `{{title}}`.
    pub title: String,
    /// The note's ULID, as `{{id}}`.
    pub id: String,
    /// Local wall-clock time as RFC 3339, e.g. `2026-08-02T14:35:09+02:00`.
    /// Local, not UTC: a journal entry written at 00:30 belongs to the day the
    /// writer thinks it is.
    pub now_local: String,
}

/// Expand `template`, returning the text and the byte offset the editor should
/// place the caret at.
///
/// The cursor offset is `None` when the template has no `{{cursor}}`; the first
/// occurrence wins and any further ones are simply removed, because two carets
/// is not a thing an editor can honour.
pub fn expand(template: &str, ctx: &TemplateCtx) -> (String, Option<usize>) {
    let stamp = Stamp::parse(&ctx.now_local);
    let mut out = String::with_capacity(template.len());
    let mut cursor: Option<usize> = None;
    let mut rest = template;

    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];

        let Some(close) = after.find("}}") else {
            // An unterminated `{{` is literal text, not a broken template.
            out.push_str(&rest[open..]);
            return (out, cursor);
        };

        let token = &after[..close];
        match resolve(token.trim(), ctx, stamp.as_ref()) {
            Resolved::Text(text) => out.push_str(&text),
            Resolved::Cursor => {
                if cursor.is_none() {
                    cursor = Some(out.len());
                }
            }
            // Re-emit the original bytes, spacing and all.
            Resolved::Unknown => {
                out.push_str("{{");
                out.push_str(token);
                out.push_str("}}");
            }
        }

        rest = &after[close + 2..];
    }

    out.push_str(rest);
    (out, cursor)
}

enum Resolved {
    Text(String),
    Cursor,
    Unknown,
}

fn resolve(token: &str, ctx: &TemplateCtx, stamp: Option<&Stamp>) -> Resolved {
    match token {
        "title" => Resolved::Text(ctx.title.clone()),
        "id" => Resolved::Text(ctx.id.clone()),
        "cursor" => Resolved::Cursor,
        "date" => stamp.map_or(Resolved::Unknown, |s| {
            Resolved::Text(render(s, "YYYY-MM-DD"))
        }),
        "time" => stamp.map_or(Resolved::Unknown, |s| Resolved::Text(render(s, "HH:mm"))),
        _ => {
            let format = token
                .strip_prefix("date:")
                .or_else(|| token.strip_prefix("time:"));
            match (format, stamp) {
                // An unparseable timestamp leaves the placeholder literal rather
                // than expanding to a wrong or empty date: a visible `{{date}}`
                // in a new note is a bug report, a silent 1970 is not.
                (Some(f), Some(s)) => Resolved::Text(render(s, f)),
                _ => Resolved::Unknown,
            }
        }
    }
}

/// A wall-clock instant, decomposed. No timezone maths happens here — the shell
/// already resolved the offset, and this only ever reformats what it was given.
///
/// Crate-visible rather than private because Story 42.4's recording-note stub
/// needs exactly this and nothing more: the manifest hands it `started_at` and
/// `ended_at` as RFC 3339 strings whose offset the shell already applied, and
/// the stub wants the calendar fields back out of them. Duplicating the slicing
/// there would be a second parser for a format keeper itself writes — and the
/// two would eventually disagree about what `2026-08-08T00:30:00+02:00` is the
/// date of, which is the one thing a note's filename must not be wrong about.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stamp {
    pub(crate) year: i32,
    pub(crate) month: u32,
    pub(crate) day: u32,
    pub(crate) hour: u32,
    pub(crate) minute: u32,
    pub(crate) second: u32,
}

impl Stamp {
    /// Slice an RFC 3339 timestamp into fields. Deliberately positional: the
    /// format is fixed-width by specification, and a parser combinator here
    /// would be more code defending against inputs keeper itself produces.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let bytes = s.as_bytes();
        if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }

        let year: i32 = s.get(0..4)?.parse().ok()?;
        let month: u32 = s.get(5..7)?.parse().ok()?;
        let day: u32 = s.get(8..10)?.parse().ok()?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }

        let has_time = bytes.len() >= 19
            && matches!(bytes[10], b'T' | b't' | b' ')
            && bytes[13] == b':'
            && bytes[16] == b':';
        let (hour, minute, second) = if has_time {
            (
                s.get(11..13)?.parse().ok()?,
                s.get(14..16)?.parse().ok()?,
                s.get(17..19)?.parse().ok()?,
            )
        } else {
            (0, 0, 0)
        };
        if hour > 23 || minute > 59 || second > 60 {
            return None;
        }

        Some(Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        })
    }
}

/// The moment.js token subset Obsidian users already have in their templates.
/// `MM` is the month and `mm` the minute — case-sensitive, as moment defines it.
/// Longest first, so `YYYY` is never read as two `YY`s.
const TOKENS: [&str; 7] = ["YYYY", "YY", "MM", "DD", "HH", "mm", "ss"];

fn render(stamp: &Stamp, pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut rest = pattern;

    'outer: while !rest.is_empty() {
        for token in TOKENS {
            if let Some(tail) = rest.strip_prefix(token) {
                match token {
                    "YYYY" => {
                        let _ = write!(out, "{:04}", stamp.year);
                    }
                    "YY" => {
                        let _ = write!(out, "{:02}", stamp.year.rem_euclid(100));
                    }
                    "MM" => {
                        let _ = write!(out, "{:02}", stamp.month);
                    }
                    "DD" => {
                        let _ = write!(out, "{:02}", stamp.day);
                    }
                    "HH" => {
                        let _ = write!(out, "{:02}", stamp.hour);
                    }
                    "mm" => {
                        let _ = write!(out, "{:02}", stamp.minute);
                    }
                    _ => {
                        let _ = write!(out, "{:02}", stamp.second);
                    }
                }
                rest = tail;
                continue 'outer;
            }
        }

        // Not a token: copy one character through verbatim.
        let Some(c) = rest.chars().next() else { break };
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> TemplateCtx {
        TemplateCtx {
            title: "Weekly review".to_owned(),
            id: "01J8ZQ0000000000000000000A".to_owned(),
            now_local: "2026-08-02T14:35:09+02:00".to_owned(),
        }
    }

    #[test]
    fn expansion_reports_the_cursor_offset_and_removes_the_marker() {
        let (text, cursor) = expand("# {{title}}\n\n{{cursor}}\n", &ctx());
        assert_eq!(text, "# Weekly review\n\n\n");
        assert_eq!(cursor, Some("# Weekly review\n\n".len()));
        // The offset is a real byte index into the expanded text.
        let at = cursor.unwrap_or_default();
        assert_eq!(&text[..at], "# Weekly review\n\n");
    }

    #[test]
    fn cursor_offset_is_a_byte_index_after_multibyte_expansion() {
        let mut c = ctx();
        c.title = "Café ☕".to_owned();
        let (text, cursor) = expand("{{title}}{{cursor}}!", &c);
        assert_eq!(text, "Café ☕!");
        assert_eq!(cursor, Some("Café ☕".len()));
    }

    #[test]
    fn only_the_first_cursor_survives_and_the_rest_vanish() {
        let (text, cursor) = expand("a{{cursor}}b{{cursor}}c", &ctx());
        assert_eq!(text, "abc");
        assert_eq!(cursor, Some(1));
    }

    #[test]
    fn no_cursor_placeholder_means_no_offset() {
        let (text, cursor) = expand("plain body", &ctx());
        assert_eq!(text, "plain body");
        assert_eq!(cursor, None);
    }

    #[test]
    fn an_unknown_placeholder_is_left_literal_byte_for_byte() {
        let (text, _) = expand("{{TODO}} and {{ weather:oslo }} and {{}}", &ctx());
        assert_eq!(text, "{{TODO}} and {{ weather:oslo }} and {{}}");
    }

    #[test]
    fn an_unterminated_brace_pair_is_literal_text() {
        let (text, _) = expand("half open {{title", &ctx());
        assert_eq!(text, "half open {{title");
    }

    #[test]
    fn dates_and_times_use_the_moment_token_subset() {
        let (text, _) = expand(
            "{{date}} {{time}} | {{date:YYYY/MM/DD}} {{time:HH:mm:ss}} {{date:YY}}",
            &ctx(),
        );
        assert_eq!(text, "2026-08-02 14:35 | 2026/08/02 14:35:09 26");
    }

    #[test]
    fn month_and_minute_tokens_do_not_collide() {
        let (text, _) = expand("{{date:MM-mm}}", &ctx());
        assert_eq!(text, "08-35");
    }

    #[test]
    fn literal_text_inside_a_format_survives() {
        let (text, _) = expand("{{date:[week of] YYYY-MM-DD}}", &ctx());
        assert_eq!(text, "[week of] 2026-08-02");
    }

    #[test]
    fn whitespace_inside_the_braces_is_tolerated_for_known_tokens() {
        let (text, _) = expand("{{ title }}", &ctx());
        assert_eq!(text, "Weekly review");
    }

    #[test]
    fn an_unparseable_timestamp_leaves_date_placeholders_visible() {
        let broken = TemplateCtx {
            title: "T".to_owned(),
            id: "I".to_owned(),
            now_local: "not a timestamp".to_owned(),
        };
        let (text, _) = expand("{{date}} {{date:YYYY}} {{title}}", &broken);
        assert_eq!(text, "{{date}} {{date:YYYY}} T");
    }

    #[test]
    fn a_date_only_timestamp_still_expands_dates() {
        let c = TemplateCtx {
            now_local: "2026-08-02".to_owned(),
            ..ctx()
        };
        let (text, _) = expand("{{date}} {{time}}", &c);
        assert_eq!(text, "2026-08-02 00:00");
    }

    #[test]
    fn an_empty_template_expands_to_nothing() {
        assert_eq!(expand("", &ctx()), (String::new(), None));
    }
}
