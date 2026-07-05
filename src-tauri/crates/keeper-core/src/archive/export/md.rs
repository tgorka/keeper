//! Pure Markdown transcript renderer (Story 5.5, FR-35).
//!
//! Renders a chronological, human-readable transcript: one entry per logical
//! message (the edit-chain **root**, deduped), each with sender, a human
//! timestamp, and the **final edited text** — or a redaction stub when the row is
//! withheld, or a relative `media/…` link for a media message. This module is
//! pure: it takes prepared [`TranscriptEntry`] values and returns a `String`,
//! touching no filesystem, DB, session, or network. The orchestrator
//! ([`super`]) resolves the final text / redaction / media link per root and
//! feeds them here.

/// One rendered transcript line for a logical message (Story 5.5). The orchestrator
/// builds these from the edit-chain roots; the renderer only formats them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    /// The sender's Matrix user id.
    pub sender: String,
    /// The message's origin timestamp in ms since the Unix epoch.
    pub timestamp: i64,
    /// The rendered body of this entry (final edited text, a redaction stub, or a
    /// media caption). Never the withheld content when deletions are honored.
    pub body: String,
    /// The relative `media/…` link for a media message, or `None` for a text-only
    /// entry. Emitted regardless of whether the bytes were actually copied.
    pub media_link: Option<String>,
}

/// Format a ms-epoch timestamp as an ISO-8601 UTC string (`YYYY-MM-DDTHH:MM:SSZ`)
/// without pulling in a date crate. Deterministic and dependency-free so the
/// transcript is reproducible and the renderer stays pure. A negative timestamp
/// (pre-1970) is clamped to the epoch — an archived `origin_ts` is never negative
/// in practice.
pub fn format_timestamp(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    let days = secs / 86_400;
    let sod = secs % 86_400;
    let (hh, mm, ss) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Convert a count of days since 1970-01-01 to a `(year, month, day)` civil date
/// (Howard Hinnant's `civil_from_days` algorithm — exact, no leap-year edge bugs).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Render the transcript entries into a Markdown document (Story 5.5).
///
/// Emits a `title` header (with an offline/provenance note and an optional
/// `media_note`), then one entry per logical message in the given (chronological)
/// order: `**sender** — timestamp` followed by the body and, when present, a
/// relative media link. An empty entry list still yields a valid header-only
/// document (the empty-scope case).
///
/// The message body is emitted as a **blockquote** (each line prefixed `> `) so a
/// remotely-authored body — which is untrusted — cannot forge the transcript's own
/// structure (a body line like `**@someone** — 2020…` or `# heading` renders as
/// quoted content, visibly distinct from the un-quoted real entry headers, instead
/// of spoofing another entry).
pub fn render_markdown(
    title: &str,
    entries: &[TranscriptEntry],
    media_note: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(title);
    out.push_str("\n\n");
    out.push_str("_Exported from your local keeper archive._\n\n");
    if let Some(note) = media_note {
        out.push('_');
        out.push_str(note);
        out.push_str("_\n\n");
    }
    for entry in entries {
        out.push_str("**");
        out.push_str(&escape_inline(&entry.sender));
        out.push_str("** — ");
        out.push_str(&format_timestamp(entry.timestamp));
        out.push('\n');
        out.push('\n');
        if !entry.body.is_empty() {
            // Quote every body line so untrusted content can't break out of its
            // entry (an empty body is skipped above).
            for line in entry.body.lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
            out.push('\n');
        }
        if let Some(link) = &entry.media_link {
            out.push('[');
            out.push_str(&escape_inline(link));
            out.push_str("](");
            out.push_str(link);
            out.push_str(")\n\n");
        }
    }
    out
}

/// Escape the Markdown inline metacharacters that would corrupt a `**…**` / link
/// wrapper. Applied to short identifiers/links only (not the message body, which is
/// rendered as authored text).
fn escape_inline(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_only_for_empty_entries() {
        let md = render_markdown("My Chat", &[], None);
        assert!(md.starts_with("# My Chat\n"));
        assert!(md.contains("Exported from your local keeper archive"));
    }

    #[test]
    fn media_note_is_rendered_in_header_when_present() {
        let md = render_markdown("T", &[], Some("Media files were not included."));
        assert!(md.contains("_Media files were not included._"));
    }

    #[test]
    fn body_is_blockquoted_so_it_cannot_forge_entry_structure() {
        // An untrusted body that tries to inject a heading and a fake entry header.
        let entries = vec![TranscriptEntry {
            sender: "@a:e.org".to_owned(),
            timestamp: 0,
            body: "# fake heading\n**@victim:e.org** — 2020-01-01T00:00:00Z".to_owned(),
            media_link: None,
        }];
        let md = render_markdown("T", &entries, None);
        // Every body line is quoted; neither injected line appears at column 0.
        assert!(md.contains("> # fake heading"));
        assert!(md.contains("> **@victim:e.org** — 2020-01-01T00:00:00Z"));
        assert!(!md.contains("\n# fake heading"));
        assert!(!md.contains("\n**@victim:e.org** — 2020-01-01T00:00:00Z"));
    }

    #[test]
    fn renders_sender_timestamp_and_body_in_order() {
        let entries = vec![
            TranscriptEntry {
                sender: "@a:e.org".to_owned(),
                timestamp: 0,
                body: "first".to_owned(),
                media_link: None,
            },
            TranscriptEntry {
                sender: "@b:e.org".to_owned(),
                timestamp: 1_000,
                body: "second".to_owned(),
                media_link: None,
            },
        ];
        let md = render_markdown("T", &entries, None);
        let first = md.find("first").expect("first present");
        let second = md.find("second").expect("second present");
        assert!(first < second, "chronological order preserved");
        assert!(md.contains("@a:e.org"));
        assert!(md.contains("1970-01-01T00:00:00Z"));
    }

    #[test]
    fn media_entry_renders_relative_link() {
        let entries = vec![TranscriptEntry {
            sender: "@a:e.org".to_owned(),
            timestamp: 0,
            body: String::new(),
            media_link: Some("media/$e1-cat.png".to_owned()),
        }];
        let md = render_markdown("T", &entries, None);
        assert!(md.contains("](media/$e1-cat.png)"), "relative link emitted");
    }

    #[test]
    fn format_timestamp_is_deterministic_utc() {
        // 2021-06-01T00:00:00Z = 1622505600000 ms.
        assert_eq!(format_timestamp(1_622_505_600_000), "2021-06-01T00:00:00Z");
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00Z");
    }
}
