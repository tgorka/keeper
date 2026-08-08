//! The note keeper writes *about* a recording, composed at the one moment it
//! will ever be written (Story 42.4, FR-142).
//!
//! Nobody documents a meeting an hour later. The minute the recording stops is
//! the entire window in which anything will be written about it, so finalize
//! composes a stub prefilled with everything keeper already knows and the stop
//! surface presents it with the cursor in the body. This module is the
//! *composition* half of that, and only that: it takes the session's facts and
//! returns a filename and the bytes of a note. It never opens a file, which is
//! the rule [`crate::notes`] states for the whole notes subsystem — every byte
//! of IO lives in the `keeper` shell (AD-55/AD-56).
//!
//! # What the stub is allowed to say
//!
//! Only facts keeper already holds. No transcription, no summarisation, no
//! inference of any kind — named here because "a note about a recording" invites
//! exactly that, and a machine-written paragraph in the one field a human was
//! about to write in is worse than a blank one. No tag normalisation against the
//! notes tag tree either (that is 42.5); tags are carried as stored.
//!
//! # Two rules the shape of this module is built around
//!
//! **The link is an identity, never a path.** `session:` carries
//! `meta.session_id` — `<device ULID>-<session ULID>`, minted once and never
//! rewritten (Story 40.3). A retitle (Story 40.4) renames the session folder and
//! leaves that byte-identical, so a note keyed on it survives the rename; a note
//! keyed on the folder would not.
//!
//! **No absolute path, anywhere.** FR-145's rule, the same one 42.1's index rows
//! obey: the recording's location is recorded relative to the destination root,
//! so the note is still true after the tree is cloned onto another machine. The
//! composer is never given an absolute path in the first place — that is
//! enforced by the signature of [`SessionFacts`], not by a filter.
//!
//! # Why the timestamps arrive in two representations
//!
//! Not redundancy — each carries something the other cannot, and this crate has
//! no calendar library to convert between them (AD-55 declines to acquire one).
//!
//! * `started_at` / `ended_at` are the manifest's RFC 3339 strings, whose offset
//!   the host already applied. They carry the **local calendar**, which is what
//!   the note's date and filename must be right about: a session at 00:30 local
//!   in UTC+2 belongs to that day, not to the one its UTC instant falls in.
//! * `started_ms` / `ended_ms` are the same two instants as epoch milliseconds.
//!   They carry the **absolute span**, which is the only honest way to measure a
//!   duration across an offset change without doing calendar arithmetic here.
//!
//! Both are derived by the shell from the same two stamps, so they cannot
//! disagree about which session they describe.

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::naming;
use crate::notes::templates::Stamp;

/// Everything about a session that the stub is allowed to state, as the shell
/// reads it off `manifest.json`.
///
/// Every field but the identity is optional, because every one of them is
/// genuinely absent for some real session: a quick capture has no title, a solo
/// screen recording has no participants, a pre-21.5 manifest has no stamps at
/// all. An absent fact produces **no line** — never a label with nothing after
/// it, which is the difference between a note that reads like it was written for
/// you and one that reads like a form you failed to fill in.
#[derive(Debug, Clone, Copy)]
pub struct SessionFacts<'a> {
    /// `meta.session_id` — the immutable `<device ULID>-<session ULID>` the
    /// `session:` link carries. The one fact that is never absent, because a
    /// note that cannot say which recording it is about is not a note about a
    /// recording.
    pub session_id: &'a str,
    /// `meta.title`, when the user set one.
    pub title: Option<&'a str>,
    /// `manifest.started_at` — RFC 3339, host offset already applied.
    pub started_at: Option<&'a str>,
    /// `manifest.ended_at` — RFC 3339, written at the terminal reconcile.
    pub ended_at: Option<&'a str>,
    /// The same instant as [`Self::started_at`], in epoch milliseconds.
    pub started_ms: Option<i64>,
    /// The same instant as [`Self::ended_at`], in epoch milliseconds.
    pub ended_ms: Option<i64>,
    /// `meta.participants` — free text, carried verbatim.
    pub participants: Option<&'a str>,
    /// `meta.tags` — carried as stored (42.5 owns resolution against the tag
    /// tree; doing it here would silently rewrite what the user typed).
    pub tags: &'a [String],
    /// The session folder **relative to the destination root**, `/`-separated.
    /// Not a `Path`, and not absolute: FR-145.
    pub relative_folder: Option<&'a str>,
}

/// A composed stub: what to call the file, and what to put in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteStub {
    /// `YYYY-MM-DD-<slug>.md`, free of every name in the `taken` set the caller
    /// supplied.
    pub filename: String,
    /// The whole file — frontmatter block, a blank separator line, then the
    /// body.
    pub contents: String,
    /// Byte offset of the **body's first byte** in [`Self::contents`].
    ///
    /// One past what [`Frontmatter::parse`] calls the body offset, for the
    /// reason `create_note` adds `+ 1` to its caret hint: the parser's offset
    /// lands on the blank line that separates the block from the prose, and the
    /// prose is what the user was invited to write in.
    pub body_offset: usize,
}

/// Compose the stub for one session.
///
/// `taken` is the set of file names that already exist where this stub will be
/// written, exactly as [`naming::note_filename`] wants it — and the reason this
/// function can promise a free name while never touching a directory. The caller
/// obtains it by **reading that directory**. It must not be derived from a
/// timestamp: two sessions stopped in the same minute share a minute-resolution
/// stamp, so a stamp is not a name, and treating it as one would have the second
/// session overwrite the first's note (AC5).
///
/// Total: there is no input for which this refuses to produce a stub. A session
/// with no title, no stamps, no participants and no tags still gets a named file
/// with a heading, because the alternative — losing the one minute in which
/// anything would have been written — is the failure this story exists to
/// prevent.
pub fn compose(facts: &SessionFacts<'_>, taken: &[String]) -> NoteStub {
    // The start stamp decides the day; the end stamp is the fallback for a
    // manifest that somehow carries only one. They differ across midnight, and
    // "when it started" is what a human means by when a meeting was.
    let start = facts.started_at.and_then(Stamp::parse);
    let end = facts.ended_at.and_then(Stamp::parse);
    let date = start.or(end).map(iso_date).unwrap_or_default();
    let title = title_of(facts, &date);

    let mut pairs: Vec<(String, FieldValue)> = Vec::with_capacity(9);
    pairs.push(("title".to_owned(), FieldValue::Str(title.clone())));
    push_text(&mut pairs, "date", &date);
    if let Some(stamp) = start {
        push_text(&mut pairs, "start", &clock(stamp));
    }
    if let Some(stamp) = end {
        push_text(&mut pairs, "end", &clock(stamp));
    }
    if let Some(text) = duration_text(facts.started_ms, facts.ended_ms) {
        push_text(&mut pairs, "duration", &text);
    }
    push_text(&mut pairs, "participants", facts.participants.unwrap_or(""));

    // Blank entries dropped rather than emitted: `meta.tags` is a UI split of a
    // comma-separated field, so a trailing comma leaves an empty string behind,
    // and `tags: ["work", ""]` would put a nameless tag in the vault's tag tree.
    let tags: Vec<FieldValue> = facts
        .tags
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .map(|tag| FieldValue::Str(tag.to_owned()))
        .collect();
    if !tags.is_empty() {
        pairs.push(("tags".to_owned(), FieldValue::List(tags)));
    }

    // Last, and in this order, because these two are keeper's own bookkeeping
    // rather than anything the writer typed: the identity that outlives the
    // folder name, then where the folder was relative to its root.
    pairs.push((
        "session".to_owned(),
        FieldValue::Str(facts.session_id.to_owned()),
    ));
    push_text(
        &mut pairs,
        "recording",
        facts.relative_folder.unwrap_or_default(),
    );

    let front = Frontmatter::serialise_new(&pairs);
    // A heading and then room. Composed the way `create_note` composes a note —
    // `format!("{front}\n{body}")` — because a stub that assembled its own
    // frontmatter differently from every other note keeper writes would be the
    // one note the vault's parser had a special case for.
    let body = format!("# {title}\n\n");
    let body_offset = front.len() + 1;
    let contents = format!("{front}\n{body}");

    NoteStub {
        // The USER's title, not the date-derived heading: `note_filename`
        // already prefixes the date, so feeding it a date-titled session would
        // name the file `2026-08-08-2026-08-08.md`. An absent title falls
        // through `naming::slug`'s own fallback word instead — the same answer
        // every other unnameable note in the vault gets, rather than a second
        // convention invented here.
        filename: naming::note_filename(facts.title.unwrap_or_default(), &date, taken),
        contents,
        body_offset,
    }
}

/// What the stub calls the session, in its `title:` field and its heading.
///
/// **Not what the filename is slugged from** — that is the user's own title, so
/// that the date `note_filename` already prefixes is not said twice.
///
/// An untitled session takes its **date**: never an empty heading, and never
/// the word "Untitled" in the prose, because a date is a true thing to call a
/// recording nobody named. With no stamp either (a manifest that predates Story
/// 21.5), the identity stands in — unlovely, but it is the one fact always
/// present, and a heading is never allowed to be blank.
fn title_of(facts: &SessionFacts<'_>, date: &str) -> String {
    if let Some(title) = facts.title.map(str::trim).filter(|t| !t.is_empty()) {
        return title.to_owned();
    }
    if !date.is_empty() {
        return date.to_owned();
    }
    facts.session_id.to_owned()
}

/// Push `key: value`, or nothing at all when the value is blank.
///
/// The whole "omit, do not label" rule funnels through here so it cannot be
/// applied to one field and forgotten on the next.
fn push_text(pairs: &mut Vec<(String, FieldValue)>, key: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    pairs.push((key.to_owned(), FieldValue::Str(value.to_owned())));
}

fn iso_date(stamp: Stamp) -> String {
    format!("{:04}-{:02}-{:02}", stamp.year, stamp.month, stamp.day)
}

fn clock(stamp: Stamp) -> String {
    format!("{:02}:{:02}", stamp.hour, stamp.minute)
}

/// How long the session ran, or `None` when that cannot be said.
///
/// A missing stamp and a non-positive span are both "cannot be said": a session
/// whose end precedes its start is a clock that moved under it, and inventing
/// `-3m` or `0m` from it would be stating a fact keeper does not have. The line
/// is omitted instead, like every other absent fact here.
fn duration_text(started_ms: Option<i64>, ended_ms: Option<i64>) -> Option<String> {
    let span = ended_ms?.checked_sub(started_ms?)?;
    if span <= 0 {
        return None;
    }
    let seconds = span / 1_000;
    let (hours, minutes, seconds) = (seconds / 3_600, (seconds % 3_600) / 60, seconds % 60);
    Some(if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01K0DEVICE0000000000000000-01K0SESSION00000000000000";

    /// A fully-populated session. Tests that are about one absent fact clear
    /// exactly that field, so what each one is testing is the diff.
    fn facts() -> SessionFacts<'static> {
        SessionFacts {
            session_id: ID,
            title: Some("Quarterly review"),
            started_at: Some("2026-08-08T14:23:45+02:00"),
            ended_at: Some("2026-08-08T15:07:45+02:00"),
            started_ms: Some(1_754_655_825_000),
            ended_ms: Some(1_754_658_465_000),
            participants: Some("Jane Doe, Sam"),
            tags: &[],
            relative_folder: Some("2026/keeper-rec 2026-08-08 14.23.45"),
        }
    }

    /// AC1, and the reason this test is shaped like
    /// `a_keeper_authored_block_round_trips_its_own_keys`: "it parses" is not
    /// the promise. Every key must read back the value it was given, and the
    /// body offset must be exact to the byte — a scanner that split a line on
    /// every colon rather than the first would lose `start: 14:23` here exactly
    /// as it would lose an RFC 3339 `id` there.
    #[test]
    fn the_composed_stub_round_trips_every_key_through_the_notes_parser() {
        let tags = ["work".to_owned(), "quarterly".to_owned()];
        let stub = compose(
            &SessionFacts {
                tags: &tags,
                ..facts()
            },
            &[],
        );
        let source = &stub.contents;
        let (fm, body) = Frontmatter::parse(source);

        assert_eq!(fm.as_string("title"), Some("Quarterly review"));
        assert_eq!(fm.as_string("date"), Some("2026-08-08"));
        assert_eq!(
            fm.as_string("start"),
            Some("14:23"),
            "a clock value keeps every colon after the first"
        );
        assert_eq!(fm.as_string("end"), Some("15:07"));
        assert_eq!(fm.as_string("duration"), Some("44m"));
        assert_eq!(fm.as_string("participants"), Some("Jane Doe, Sam"));
        assert_eq!(
            fm.as_list("tags"),
            Some(vec!["work".to_owned(), "quarterly".to_owned()])
        );
        assert_eq!(
            fm.as_string("session"),
            Some(ID),
            "the link carries the immutable identity, not a path"
        );
        assert_eq!(
            fm.as_string("recording"),
            Some("2026/keeper-rec 2026-08-08 14.23.45")
        );

        assert_eq!(
            &source[body..],
            "\n# Quarterly review\n\n",
            "the body offset is exact and the prose survives byte-identically"
        );
        assert_eq!(
            &source[stub.body_offset..],
            "# Quarterly review\n\n",
            "the stub's own offset skips the separator line and lands on the prose"
        );
    }

    /// AC4, asserted the way `no_column_anywhere_carries_the_destination_root`
    /// asserts it: against the actual root string, so the test fails if any
    /// field starts carrying one rather than only if the field we thought of
    /// does.
    #[test]
    fn no_line_of_the_stub_carries_an_absolute_path() {
        let root = "/Users/jane/Movies/keeper";
        let stub = compose(&facts(), &[]);
        assert!(
            !stub.contents.contains(root),
            "the destination root must not appear anywhere in the stub"
        );
        assert!(
            !stub.contents.contains("/Users/"),
            "nor any other absolute prefix"
        );
        assert!(
            stub.contents
                .contains("recording: 2026/keeper-rec 2026-08-08 14.23.45"),
            "the location is recorded, but relative to the destination root"
        );
    }

    /// AC5. The second session is one second later — the same minute, so the
    /// same minute-resolution stamp — and its name is distinct because the
    /// caller handed in what the directory already holds.
    #[test]
    fn two_sessions_stopped_in_the_same_minute_get_distinct_filenames() {
        let first = compose(&facts(), &[]);
        let second = compose(
            &SessionFacts {
                session_id: "01K0DEVICE0000000000000000-01K0SECOND0000000000000000",
                started_at: Some("2026-08-08T14:23:46+02:00"),
                ended_at: Some("2026-08-08T15:07:46+02:00"),
                ..facts()
            },
            // Exactly what a read of the destination directory returns once the
            // first stub is on disk.
            std::slice::from_ref(&first.filename),
        );

        assert_eq!(first.filename, "2026-08-08-quarterly-review.md");
        assert_eq!(second.filename, "2026-08-08-quarterly-review-2.md");
        assert_ne!(first.filename, second.filename);
    }

    /// The same, one layer meaner: a name that differs only in case is one file
    /// on APFS and NTFS, so it must still count as taken.
    #[test]
    fn a_taken_name_that_differs_only_in_case_still_forces_a_new_one() {
        let stub = compose(&facts(), &["2026-08-08-QUARTERLY-REVIEW.MD".to_owned()]);
        assert_eq!(stub.filename, "2026-08-08-quarterly-review-2.md");
    }

    #[test]
    fn an_untitled_session_is_titled_by_its_date_and_never_left_headingless() {
        let stub = compose(
            &SessionFacts {
                title: None,
                ..facts()
            },
            &[],
        );
        let (fm, body) = Frontmatter::parse(&stub.contents);

        assert_eq!(fm.as_string("title"), Some("2026-08-08"));
        assert_eq!(&stub.contents[body..], "\n# 2026-08-08\n\n");
        assert_eq!(
            stub.filename, "2026-08-08-untitled.md",
            "the date is the heading, but the filename says it once, not twice"
        );
        assert!(
            !stub.contents.contains("# \n"),
            "an empty heading is the one thing an untitled session must not get"
        );
    }

    /// A title that is only whitespace is untitled — "untitled" has one
    /// representation, the way `SessionManifest::set_title` decided it does.
    #[test]
    fn a_whitespace_only_title_is_treated_as_no_title_at_all() {
        let stub = compose(
            &SessionFacts {
                title: Some("   "),
                ..facts()
            },
            &[],
        );
        assert_eq!(
            Frontmatter::parse(&stub.contents).0.as_string("title"),
            Some("2026-08-08")
        );
    }

    #[test]
    fn a_session_with_no_participants_and_no_tags_omits_those_lines_entirely() {
        let stub = compose(
            &SessionFacts {
                participants: None,
                tags: &[],
                ..facts()
            },
            &[],
        );
        let (fm, _) = Frontmatter::parse(&stub.contents);

        assert_eq!(fm.get("participants"), None);
        assert_eq!(fm.get("tags"), None);
        assert!(
            !stub.contents.contains("participants:"),
            "an absent fact is omitted, not emitted as an empty label"
        );
        assert!(!stub.contents.contains("tags:"));
        // What it does still say, so the omission is not mistaken for the
        // composer having given up.
        assert_eq!(fm.as_string("title"), Some("Quarterly review"));
        assert_eq!(fm.as_string("session"), Some(ID));
    }

    #[test]
    fn a_blank_participants_string_is_omitted_the_same_way_an_absent_one_is() {
        let stub = compose(
            &SessionFacts {
                participants: Some("  "),
                ..facts()
            },
            &[],
        );
        assert!(!stub.contents.contains("participants:"));
    }

    /// `meta.tags` is a UI split of a comma-separated field, so a trailing comma
    /// really does arrive as an empty element.
    #[test]
    fn empty_tags_are_dropped_and_an_all_blank_tag_list_omits_the_line() {
        let mixed = ["work".to_owned(), "  ".to_owned(), " late ".to_owned()];
        let stub = compose(
            &SessionFacts {
                tags: &mixed,
                ..facts()
            },
            &[],
        );
        assert_eq!(
            Frontmatter::parse(&stub.contents).0.as_list("tags"),
            Some(vec!["work".to_owned(), "late".to_owned()])
        );

        let blank = ["".to_owned(), "   ".to_owned()];
        let stub = compose(
            &SessionFacts {
                tags: &blank,
                ..facts()
            },
            &[],
        );
        assert!(
            !stub.contents.contains("tags:"),
            "a list with nothing in it is no list"
        );
    }

    /// A manifest that predates Story 21.5 has no stamps at all. The stub must
    /// still be a named file with a non-empty heading, because the alternative
    /// is losing the note.
    #[test]
    fn a_session_with_no_stamps_still_gets_a_name_and_a_heading() {
        let stub = compose(
            &SessionFacts {
                title: None,
                started_at: None,
                ended_at: None,
                started_ms: None,
                ended_ms: None,
                ..facts()
            },
            &[],
        );
        let (fm, _) = Frontmatter::parse(&stub.contents);

        assert_eq!(fm.as_string("title"), Some(ID));
        assert_eq!(fm.get("date"), None);
        assert_eq!(fm.get("start"), None);
        assert_eq!(fm.get("duration"), None);
        assert_eq!(
            stub.filename, "untitled.md",
            "no date means no date prefix, never a `-` with nothing before it"
        );
    }

    #[test]
    fn the_day_is_the_one_the_session_started_on_not_the_one_it_ended_on() {
        let stub = compose(
            &SessionFacts {
                title: None,
                started_at: Some("2026-08-08T23:40:00+02:00"),
                ended_at: Some("2026-08-09T00:20:00+02:00"),
                started_ms: Some(1_754_689_200_000),
                ended_ms: Some(1_754_691_600_000),
                ..facts()
            },
            &[],
        );
        let (fm, _) = Frontmatter::parse(&stub.contents);

        assert_eq!(fm.as_string("date"), Some("2026-08-08"));
        assert_eq!(fm.as_string("start"), Some("23:40"));
        assert_eq!(fm.as_string("end"), Some("00:20"));
        assert_eq!(fm.as_string("duration"), Some("40m"));
    }

    /// The local calendar is what the note's date must be right about. This is
    /// the case that a UTC epoch conversion would get wrong, which is why the
    /// composer reads the offset-applied stamp instead.
    #[test]
    fn a_session_just_after_local_midnight_is_dated_locally_not_in_utc() {
        let stub = compose(
            &SessionFacts {
                title: None,
                started_at: Some("2026-08-09T00:30:00+02:00"),
                ended_at: Some("2026-08-09T01:00:00+02:00"),
                started_ms: Some(1_754_692_200_000),
                ended_ms: Some(1_754_694_000_000),
                ..facts()
            },
            &[],
        );
        assert_eq!(
            Frontmatter::parse(&stub.contents).0.as_string("date"),
            Some("2026-08-09"),
            "the instant is 2026-08-08T22:30Z, but the recording happened on the 9th"
        );
    }

    #[test]
    fn duration_reads_in_hours_minutes_or_seconds_and_is_omitted_when_unknowable() {
        assert_eq!(
            duration_text(Some(0), Some(3_600_000)),
            Some("1h 00m".into())
        );
        assert_eq!(
            duration_text(Some(0), Some(3_840_000)),
            Some("1h 04m".into())
        );
        assert_eq!(duration_text(Some(0), Some(2_640_000)), Some("44m".into()));
        assert_eq!(duration_text(Some(0), Some(5_000)), Some("5s".into()));
        assert_eq!(duration_text(None, Some(5_000)), None);
        assert_eq!(duration_text(Some(0), None), None);
        assert_eq!(
            duration_text(Some(5_000), Some(0)),
            None,
            "a clock that moved under the session is not a negative duration"
        );
        assert_eq!(duration_text(Some(5_000), Some(5_000)), None);
    }

    /// A title full of YAML punctuation is the case that turns a stub into an
    /// unparseable note. The serialiser quotes it; this asserts the value
    /// survives the round trip rather than that any particular quoting happened.
    #[test]
    fn a_title_full_of_yaml_punctuation_still_round_trips() {
        let hostile = r#"Re: "budget" #2, [draft] — 50% {done}"#;
        let stub = compose(
            &SessionFacts {
                title: Some(hostile),
                ..facts()
            },
            &[],
        );
        let (fm, body) = Frontmatter::parse(&stub.contents);

        assert_eq!(fm.as_string("title"), Some(hostile));
        assert_eq!(&stub.contents[body..], format!("\n# {hostile}\n\n"));
        assert_eq!(
            stub.filename, "2026-08-08-re-budget-2-draft-50-done.md",
            "and the filename is Windows-safe, through the shared slug"
        );
    }

    /// The offset is a byte index into `contents`, and a non-ASCII title moves
    /// it. Asserted by slicing, because an off-by-one here would panic on a
    /// character boundary rather than quietly mis-place a cursor.
    #[test]
    fn the_body_offset_is_a_byte_index_that_survives_a_non_ascii_title() {
        let stub = compose(
            &SessionFacts {
                title: Some("Café résumé"),
                ..facts()
            },
            &[],
        );
        assert_eq!(&stub.contents[stub.body_offset..], "# Café résumé\n\n");
        assert!(stub.contents[..stub.body_offset].ends_with("---\n\n"));
    }
}
