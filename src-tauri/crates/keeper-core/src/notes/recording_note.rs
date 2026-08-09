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
//! about to write in is worse than a blank one. The session's own tags are
//! carried **as stored**, because normalising them would rewrite the user's own
//! text in one of the two places Story 42.5 promises it survives. The single
//! tag keeper adds of its own accord is [`RECORDINGS_TAG`], and that one is
//! resolved through 42.5's vocabulary rather than written as a literal (Story
//! 43.2).
//!
//! # Why the body embeds the videos while the frontmatter lists the files
//!
//! `files:` is a machine's list and an embed is what a person sees; both name
//! the same strings, and neither is derived from the other by joining anything
//! (AD-65 — the embed is the `files:` entry verbatim, in the same
//! relative-to-the-destination-root frame, so FR-145 holds in the body for the
//! same reason it holds in the block).
//!
//! The embeds go **below the heading**, never above it.
//! `notes_vault::note_title` falls back to the body's first line, so an embed
//! written at offset zero becomes the note's displayed title. That is not a
//! hypothetical: Story 43.7's panel inserts at the caret, a caret at zero put
//! an embed above `# Title`, and the owner's vault has a note called
//! `![[recordings/…/screen-0000.mov]]` because of it.
//!
//! Videos only, decided by [`kind_for_file_name`] rather than by a second
//! extension table here. `manifest.json` and `events.log` are reachable through
//! `files:` and Story 43.7's panel already, and embedding either would put a
//! chip that shows nothing where the recording is supposed to be.
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

use crate::archive::recordings_fts::kind_for_file_name;
use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::naming;
use crate::notes::tags;
use crate::notes::templates::Stamp;
use crate::vm::RecordingNoteTargetKind;

/// The frontmatter key whose presence makes a note a recording note.
///
/// Not a folder name, not a tag, not a filename convention. `session:` is the
/// only marker because it is the only one keeper mints and nobody can move: a
/// Story 40.4 retitle renames the session folder and rewrites nothing here, and
/// a user is free to rename the note file itself. Keying the predicate on
/// anything else would let a note quietly stop being a recording note because
/// somebody tidied a folder.
pub const SESSION_KEY: &str = "session";

/// Whether a note's frontmatter says keeper wrote it about a recording.
///
/// A blank value does not count. [`Frontmatter::parse`] keeps a key whose value
/// is empty, so a bare `session:` line would otherwise mark a note as a
/// recording whose session can never be resolved — the one state no caller can
/// do anything with, and the one that would put a dead Reveal button on a note.
pub fn is_recording_note(fm: &Frontmatter) -> bool {
    fm.as_string(SESSION_KEY)
        .is_some_and(|id| !id.trim().is_empty())
}

/// The tag every recording note carries so a *human* can find one (Story 43.2,
/// FR-147).
///
/// [`SESSION_KEY`] is the machine's predicate and it is invisible in the vault:
/// browsing the tag tree shows a note's own tags and nothing saying what KIND
/// of note it is, so the notes keeper writes are the only ones a person cannot
/// reach without already knowing they exist.
///
/// Spelled here in the canonical form [`tags::normalise`] produces, and put
/// through that function anyway before it is emitted: the vocabulary is the
/// authority on what this tag is, not this constant. The test
/// `the_kind_tag_is_already_what_the_vocabulary_makes_of_it` pins the two
/// together, so a future normalisation rule cannot leave this file emitting a
/// tag the tree files under a different name.
pub const RECORDINGS_TAG: &str = "recordings";

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
    /// The session's files — every segment, and the manifest that describes
    /// them — each **relative to the destination root**, `/`-separated. The
    /// same frame [`Self::relative_folder`] is in, so a reader resolves any one
    /// of them on its own rather than by joining it to the folder above it.
    ///
    /// Not a `Path`, and not absolute, for the reason `relative_folder` is not:
    /// FR-145 is enforced by this signature rather than by a filter, because a
    /// filter is a thing that can be forgotten on the next field added here.
    pub files: &'a [&'a str],
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
    /// Byte offset of the **body's first byte** in [`Self::contents`] — the
    /// heading, which is also the first byte the writer is allowed to edit.
    ///
    /// One past what [`Frontmatter::parse`] calls the body offset, for the
    /// reason `create_note` adds `+ 1` to its caret hint: the parser's offset
    /// lands on the blank line that separates the block from the prose, and the
    /// prose is what the user was invited to write in.
    ///
    /// **It stays the heading even now that the body carries embeds**, and the
    /// stop surface's caret — placed at the END of the body it slices off here
    /// — is what lands below them. Moving this offset past the embeds would
    /// look like a better caret hint and would instead move the embeds into the
    /// read-only head that surface renders, making the one thing keeper just
    /// wrote into someone's note the one thing they cannot delete. It would
    /// also disagree with `RecordingNoteStubVm::body_offset`, which the shell
    /// recomputes from [`Frontmatter::parse`] against the file on disk: two
    /// same-named offsets meaning two different positions is a split that goes
    /// wrong silently.
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

    let mut pairs: Vec<(String, FieldValue)> = Vec::with_capacity(10);
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
    let mut tags: Vec<FieldValue> = facts
        .tags
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .map(|tag| FieldValue::Str(tag.to_owned()))
        .collect();

    // Keeper's kind tag goes **after** the session's own, for the reason the
    // three bookkeeping keys below come last: what the writer typed leads, and
    // what keeper added trails it. Prepending would also displace the user's
    // first tag from the one position a truncated property row is sure to show,
    // and would make keeper's addition look like something they chose first.
    if let Some(kind) = kind_tag(facts.tags) {
        tags.push(FieldValue::Str(kind));
    }
    if !tags.is_empty() {
        pairs.push(("tags".to_owned(), FieldValue::List(tags)));
    }

    // Last, and in this order, because these three are keeper's own bookkeeping
    // rather than anything the writer typed: the identity that outlives the
    // folder name, then where the folder was relative to its root, then what is
    // inside it — in that same frame, so the reader never has to join the two.
    pairs.push((
        SESSION_KEY.to_owned(),
        FieldValue::Str(facts.session_id.to_owned()),
    ));
    push_text(
        &mut pairs,
        "recording",
        facts.relative_folder.unwrap_or_default(),
    );

    // Blank entries dropped and the key omitted when nothing survives, exactly
    // as `tags` above and for the same reason: `- ` under `files:` is a
    // nameless entry, which is worse than no key at all — nobody can act on it,
    // and nobody can tell from the note what it was supposed to have been.
    //
    // Trimmed once and kept, because the body's embeds must name the same
    // strings the block does. Two independent passes over `facts.files` would
    // be two places for the next filter to be added to and one for it to be
    // forgotten.
    let files: Vec<&str> = facts
        .files
        .iter()
        .map(|file| file.trim())
        .filter(|file| !file.is_empty())
        .collect();
    if !files.is_empty() {
        pairs.push((
            "files".to_owned(),
            FieldValue::List(
                files
                    .iter()
                    .map(|file| FieldValue::Str((*file).to_owned()))
                    .collect(),
            ),
        ));
    }

    let front = Frontmatter::serialise_new(&pairs);
    // A heading, then the recording, then room. Composed the way `create_note`
    // composes a note — `format!("{front}\n{body}")` — because a stub that
    // assembled its own frontmatter differently from every other note keeper
    // writes would be the one note the vault's parser had a special case for.
    let body = format!("# {title}\n\n{}", video_embeds(&files));
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

/// [`RECORDINGS_TAG`] in canonical form, or `None` when the session already
/// carries it under some spelling of its own.
///
/// **The question is asked of [`tags::normalise`], never of the strings.**
/// `Recordings`, `recordings ` and `#recordings` are all already this tag, and
/// emitting a second spelling beside one of them is precisely the twin node
/// Story 42.5 exists to prevent — the tree would show `recordings` counted
/// twice off one note, or worse, the writer would see keeper disagree with them
/// about their own vocabulary. What the user typed is left byte-identical;
/// keeper only declines to say it again.
///
/// Returning `None` for an unnormalisable constant rather than panicking keeps
/// [`compose`] total, which is the promise this whole module makes: there is no
/// session for which the one minute in which a note would have been written is
/// lost to a composer that refused.
fn kind_tag(own: &[String]) -> Option<String> {
    let kind = tags::normalise(RECORDINGS_TAG)?;
    let already = own
        .iter()
        .any(|tag| tags::normalise(tag).as_deref() == Some(kind.as_str()));
    (!already).then_some(kind)
}

/// The session's videos as Obsidian embeds, one per line, with a blank line
/// after them — or an empty string when the session has none.
///
/// **Empty, not blank.** A session that recorded only audio, or whose segments
/// all failed to express themselves relative to the root, gets the body it got
/// before this story: `# Title` and one blank line. An embed block that
/// collapsed to nothing must not leave the separator it would have needed
/// behind, because a stub is the one note nobody proofreads before saving.
///
/// **Ledger order, never a sort**, for the reason `files:` is in ledger order:
/// sorting would lift `camera-0000.mov` above the screen segment it was
/// recorded beside, and the pair reads as one player (Story 44.1).
///
/// **The `files:` string verbatim.** Nothing is joined onto it and nothing is
/// re-derived from `recording:`; the note is written in one frame and every
/// path in it stays in that frame, which is what makes it still true after the
/// tree is cloned (FR-145).
fn video_embeds(files: &[&str]) -> String {
    let mut out = String::new();
    for file in files
        .iter()
        .filter(|file| matches!(kind_for_file_name(file), RecordingNoteTargetKind::Video))
        .filter(|file| wikilink_can_name(file))
    {
        out.push_str("![[");
        out.push_str(file);
        out.push_str("]]\n");
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Whether `![[file]]` would still mean *this* file.
///
/// Obsidian's wikilink grammar consumes each of these characters: `]` closes
/// the link, `|` starts an alias, `#` starts a heading reference, `^` a block
/// reference, and a newline ends it outright. A file name containing one is
/// legal on APFS and would produce an embed pointing at some shorter path that
/// does not exist — a broken player in place of the recording, which is worse
/// than no embed. A newline is the sharper case: it would put a second body
/// line into the note that keeper did not write.
///
/// Such a file is still listed under `files:` and still one press away in Story
/// 43.7's panel, so nothing about it becomes unreachable — keeper only declines
/// to write a link it knows is wrong.
fn wikilink_can_name(file: &str) -> bool {
    !file.contains(['\n', '\r', '[', ']', '|', '#', '^'])
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
    use crate::notes::links;

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
            files: &[],
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
            Some(vec![
                "work".to_owned(),
                "quarterly".to_owned(),
                RECORDINGS_TAG.to_owned()
            ])
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

    /// The files are the session's own ledger order — the screen track, then
    /// the camera track beside it, then the manifest that describes them — and
    /// never a sort. Sorting would lift `camera-0000.mov` above the screen
    /// segment it accompanies, and the note would read like a directory
    /// listing rather than like a recording.
    #[test]
    fn every_file_of_a_session_is_listed_under_its_folder_in_ledger_order() {
        let files = [
            "2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov",
            "2026/keeper-rec 2026-08-08 14.23.45/camera-0000.mov",
            "2026/keeper-rec 2026-08-08 14.23.45/manifest.json",
        ];
        let stub = compose(
            &SessionFacts {
                files: &files,
                ..facts()
            },
            &[],
        );
        let (fm, _) = Frontmatter::parse(&stub.contents);

        assert_eq!(
            fm.as_list("files"),
            Some(
                files
                    .iter()
                    .map(|file| (*file).to_owned())
                    .collect::<Vec<_>>()
            ),
            "every file reads back through the parser, in the order it arrived"
        );
        assert!(
            stub.contents.contains(concat!(
                "recording: 2026/keeper-rec 2026-08-08 14.23.45\n",
                "files:\n",
                "  - 2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov\n",
                "  - 2026/keeper-rec 2026-08-08 14.23.45/camera-0000.mov\n",
                "  - 2026/keeper-rec 2026-08-08 14.23.45/manifest.json\n",
            )),
            "the list follows `recording:` immediately, one file per line: {}",
            stub.contents
        );
    }

    /// The two tracks and the manifest, exactly as a two-camera session hands
    /// them over.
    const TWO_TRACKS: [&str; 3] = [
        "2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov",
        "2026/keeper-rec 2026-08-08 14.23.45/camera-0000.mov",
        "2026/keeper-rec 2026-08-08 14.23.45/manifest.json",
    ];

    /// Story 44.2's whole claim, as the exact body: the heading, a blank line,
    /// both tracks in the ledger's order, and the line the writer's caret lands
    /// on. `manifest.json` is in `files:` and is deliberately not here — it is
    /// already reachable through the attachment panel, and an embed of it is a
    /// chip that shows nothing where the recording should be.
    ///
    /// Asserted as the whole body rather than with `contains`, because "an
    /// extra blank line crept in" and "the embeds ended up in the wrong half of
    /// the note" both pass a `contains`.
    #[test]
    fn a_two_track_session_embeds_both_videos_under_the_heading_in_ledger_order() {
        let stub = compose(
            &SessionFacts {
                files: &TWO_TRACKS,
                ..facts()
            },
            &[],
        );

        assert_eq!(
            &stub.contents[stub.body_offset..],
            concat!(
                "# Quarterly review\n",
                "\n",
                "![[2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov]]\n",
                "![[2026/keeper-rec 2026-08-08 14.23.45/camera-0000.mov]]\n",
                "\n",
            )
        );
    }

    /// Read back with the vault's own link parser rather than with `contains`,
    /// because the promise is that Obsidian renders these: a string that merely
    /// occurs in the body is not an embed, and `links::extract` is the reader
    /// that decides. Each target is compared against the `files:` entry itself,
    /// so a composer that ever joined a root onto a subpath (AD-65) or
    /// re-derived the path from `recording:` fails here rather than on the
    /// second machine the vault is cloned onto.
    #[test]
    fn the_embeds_read_back_as_embeds_naming_the_files_key_byte_for_byte() {
        let stub = compose(
            &SessionFacts {
                files: &TWO_TRACKS,
                ..facts()
            },
            &[],
        );
        let listed = Frontmatter::parse(&stub.contents)
            .0
            .as_list("files")
            .expect("this session has files");
        let links = links::extract(&stub.contents[stub.body_offset..]);

        assert_eq!(links.len(), 2, "the manifest is listed, never embedded");
        assert!(
            links.iter().all(|link| link.embed),
            "`![[…]]`, not `[[…]]` — a mention renders nothing"
        );
        assert!(
            links.iter().all(|link| link.alias.is_none()),
            "no alias: an embed with one names a different target"
        );
        assert_eq!(
            links
                .iter()
                .map(|link| link.target.clone())
                .collect::<Vec<_>>(),
            vec![listed[0].clone(), listed[1].clone()],
            "each embed names the note's own `files:` entry, in the ledger's order"
        );
    }

    /// The failure this story is not allowed to reproduce. Story 43.7's panel
    /// inserts at the caret, a caret at zero put an embed above `# Title`, and
    /// `notes_vault::note_title` falls back to the body's first line — so the
    /// owner's vault has a note called `![[recordings/…/screen-0000.mov]]`.
    ///
    /// Asserted through [`naming::title_from_body`], which IS that fallback,
    /// rather than through a `starts_with('#')` of our own: a rule the test
    /// restates is a rule the test can be wrong about.
    #[test]
    fn the_heading_is_still_the_title_under_the_rule_the_vault_falls_back_to() {
        let stub = compose(
            &SessionFacts {
                files: &TWO_TRACKS,
                ..facts()
            },
            &[],
        );

        assert_eq!(
            naming::title_from_body(&stub.contents[stub.body_offset..]),
            "Quarterly review"
        );
        // And over the body the indexer actually slices — the parser's offset,
        // separator line included — so the stub's own offset is not what is
        // holding the heading up.
        let (_, at) = Frontmatter::parse(&stub.contents);
        assert_eq!(
            naming::title_from_body(&stub.contents[at..]),
            "Quarterly review"
        );

        // An untitled session is the same claim with nothing to hide behind:
        // its heading is a date, and a date is still not an embed.
        let untitled = compose(
            &SessionFacts {
                title: None,
                files: &TWO_TRACKS,
                ..facts()
            },
            &[],
        );
        assert_eq!(
            naming::title_from_body(&untitled.contents[untitled.body_offset..]),
            "2026-08-08"
        );
    }

    /// A session that recorded no video gets the body it got before this story,
    /// byte for byte. An embed block that collapsed to nothing must not leave
    /// the blank line it would have needed behind — a stub is the one note
    /// nobody proofreads before saving, and a trailing blank is the kind of
    /// thing that survives into every note a person owns.
    #[test]
    fn a_session_with_no_video_embeds_nothing_and_gains_no_blank_line() {
        let bare = compose(&facts(), &[]);
        let untouched = bare.contents[bare.body_offset..].to_owned();
        assert_eq!(untouched, "# Quarterly review\n\n");

        let audio_only = [
            "2026/keeper-rec 2026-08-08 14.23.45/mix-0000.m4a",
            "2026/keeper-rec 2026-08-08 14.23.45/manifest.json",
        ];
        let metadata_only = [
            "2026/keeper-rec 2026-08-08 14.23.45/manifest.json",
            "2026/keeper-rec 2026-08-08 14.23.45/events.log",
        ];
        for files in [&audio_only, &metadata_only] {
            let stub = compose(&SessionFacts { files, ..facts() }, &[]);
            assert_eq!(
                &stub.contents[stub.body_offset..],
                untouched,
                "no video means the 42.4 body, unchanged: {files:?}"
            );
            assert_eq!(
                Frontmatter::parse(&stub.contents).0.as_list("files"),
                Some(files.iter().map(|f| (*f).to_owned()).collect::<Vec<_>>()),
                "the files are still listed — only the body is empty of them"
            );
        }
    }

    /// Every kind Story 43.5 names, in one session. What reaches the body is
    /// cross-checked against [`kind_for_file_name`] itself rather than against a
    /// list written out here, so a second extension table can never be added to
    /// this module and diverge from the attachment panel's answer.
    #[test]
    fn only_the_files_43_5_calls_video_are_embedded() {
        let files = [
            "2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov",
            "2026/keeper-rec 2026-08-08 14.23.45/whiteboard.png",
            "2026/keeper-rec 2026-08-08 14.23.45/mix-0000.m4a",
            "2026/keeper-rec 2026-08-08 14.23.45/manifest.json",
            "2026/keeper-rec 2026-08-08 14.23.45/events.log",
            "2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov.bak",
            "2026/keeper-rec 2026-08-08 14.23.45/camera-0000.MOV",
        ];
        let stub = compose(
            &SessionFacts {
                files: &files,
                ..facts()
            },
            &[],
        );
        let embedded: Vec<String> = links::extract(&stub.contents[stub.body_offset..])
            .into_iter()
            .map(|link| link.target)
            .collect();

        assert_eq!(
            embedded,
            vec![
                "2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov".to_owned(),
                // A file copied in from another machine is the same video; a
                // backup of one is not, because the LAST extension decides.
                "2026/keeper-rec 2026-08-08 14.23.45/camera-0000.MOV".to_owned(),
            ]
        );
        for file in files {
            assert_eq!(
                embedded.iter().any(|target| target == file),
                matches!(kind_for_file_name(file), RecordingNoteTargetKind::Video),
                "the body embeds exactly 43.5's videos, and {file} disagrees"
            );
        }
    }

    /// A file name Obsidian's wikilink grammar would eat. `]` closes the link,
    /// `|` starts an alias, `#` starts a heading reference, and a newline ends
    /// the link and puts a line into the note keeper did not write. Each is
    /// legal on APFS, and an embed built from one points at a shorter path that
    /// does not exist — a broken player in place of the recording.
    ///
    /// It stays in `files:`, so it stays one press away in Story 43.7's panel.
    /// Keeper only declines to write a link it already knows is wrong.
    #[test]
    fn a_name_a_wikilink_cannot_express_is_listed_but_never_embedded() {
        let files = [
            "2026/keeper-rec 2026-08-08 14.23.45/take [2].mov",
            "2026/keeper-rec 2026-08-08 14.23.45/a|b.mov",
            "2026/keeper-rec 2026-08-08 14.23.45/take #3.mov",
            "2026/keeper-rec 2026-08-08 14.23.45/one\n# Not the title.mov",
            "2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov",
        ];
        let stub = compose(
            &SessionFacts {
                files: &files,
                ..facts()
            },
            &[],
        );
        let body = &stub.contents[stub.body_offset..];

        assert_eq!(
            links::extract(body)
                .into_iter()
                .map(|link| link.target)
                .collect::<Vec<_>>(),
            vec!["2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov".to_owned()],
            "only the name a wikilink can carry is embedded: {body:?}"
        );
        assert!(
            !body.contains("Not the title"),
            "and no name put a line of its own into the body: {body:?}"
        );
        assert_eq!(
            naming::title_from_body(body),
            "Quarterly review",
            "the heading is still the first line the vault reads"
        );
        assert_eq!(
            stub.contents.matches("![[").count(),
            1,
            "the skipped names left no half-written embed behind"
        );
    }

    /// Where the caret hint points, said out loud so moving it fails here.
    ///
    /// `body_offset` is the head/body split, and it stays on the heading. The
    /// caret a person actually gets is the END of the slice taken from it — the
    /// stop surface's `setSelectionRange(value.length, …)` — so it lands on the
    /// blank line BELOW the embeds. Below and not above: keeper's prefill is
    /// context and the sentence goes after it, and a caret above the embeds
    /// would push them down the page on the first keystroke, which is the note
    /// no longer opening as the recording.
    ///
    /// Moving `body_offset` past the embeds would look like a better caret hint
    /// and would instead move them into the head that surface renders read-only,
    /// making the one thing keeper just wrote the one thing nobody can delete.
    #[test]
    fn the_body_offset_is_the_heading_and_the_writers_line_is_below_the_embeds() {
        let stub = compose(
            &SessionFacts {
                files: &TWO_TRACKS,
                ..facts()
            },
            &[],
        );

        assert!(
            stub.contents[..stub.body_offset].ends_with("---\n\n"),
            "everything before the offset is keeper's block and its separator"
        );
        assert!(
            stub.contents[stub.body_offset..].starts_with("# Quarterly review\n"),
            "the offset lands on the heading, which therefore stays editable"
        );

        let editable = &stub.contents[stub.body_offset..];
        assert!(
            editable.contains("![[2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov]]"),
            "the embeds are inside the editable half, not in the read-only head"
        );
        assert!(
            editable.ends_with("camera-0000.mov]]\n\n"),
            "the caret at the end of that half sits under the last embed: {editable:?}"
        );
    }

    /// Omitted, never labelled: a session that closed no segment must not leave
    /// `files:` standing over nothing. Asserted against the rendered text and
    /// not against a length, because an empty list and a bare key both measure
    /// zero and both are exactly what this forbids.
    #[test]
    fn a_session_with_no_files_carries_no_files_key_at_all() {
        let stub = compose(
            &SessionFacts {
                files: &[],
                ..facts()
            },
            &[],
        );

        assert!(
            !stub.contents.contains("files"),
            "the key is absent from the block entirely, not present and empty: {}",
            stub.contents
        );
        assert_eq!(Frontmatter::parse(&stub.contents).0.as_list("files"), None);
    }

    /// The same rule the tags list obeys, for the same reason: a nameless entry
    /// in a note is worse than no key, because nothing can be done with it and
    /// the note does not even say what went missing.
    #[test]
    fn a_blank_file_entry_is_dropped_rather_than_listed_nameless() {
        let files = [
            "2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov",
            "   ",
            "2026/keeper-rec 2026-08-08 14.23.45/manifest.json",
        ];
        let stub = compose(
            &SessionFacts {
                files: &files,
                ..facts()
            },
            &[],
        );
        let (fm, _) = Frontmatter::parse(&stub.contents);

        assert_eq!(
            fm.as_list("files"),
            Some(vec![files[0].to_owned(), files[2].to_owned()])
        );
        assert!(
            stub.contents.contains(concat!(
                "files:\n",
                "  - 2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov\n",
                "  - 2026/keeper-rec 2026-08-08 14.23.45/manifest.json\n",
            )),
            "the two real files are adjacent, so the blank produced no line at all: {}",
            stub.contents
        );

        // Nothing but blanks is nothing: the key goes with them.
        let all_blank = compose(
            &SessionFacts {
                files: &["", "  "],
                ..facts()
            },
            &[],
        );
        assert!(
            !all_blank.contents.contains("files"),
            "a list that emptied out takes its key with it: {}",
            all_blank.contents
        );
    }

    /// FR-145 for the new key, asserted the way
    /// `no_line_of_the_stub_carries_an_absolute_path` asserts it for the rest of
    /// the block: over every value the list actually holds, so a file that
    /// arrived absolute is caught here whichever one it is.
    #[test]
    fn no_file_in_the_list_is_written_as_an_absolute_path() {
        let files = [
            "2026/keeper-rec 2026-08-08 14.23.45/screen-0000.mov",
            "2026/keeper-rec 2026-08-08 14.23.45/manifest.json",
        ];
        let stub = compose(
            &SessionFacts {
                files: &files,
                ..facts()
            },
            &[],
        );
        let listed = Frontmatter::parse(&stub.contents)
            .0
            .as_list("files")
            .expect("this session has files");

        assert_eq!(listed.len(), 2);
        for path in &listed {
            assert!(
                !path.starts_with('/'),
                "a note never carries an absolute path, got {path}"
            );
            assert!(
                path.starts_with("2026/keeper-rec 2026-08-08 14.23.45/"),
                "each file is in the same frame as `recording:` — relative to the destination \
                 root — got {path}"
            );
        }
        assert!(
            !stub.contents.contains("/Users/"),
            "and no absolute prefix reaches any other line either"
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

    /// `participants:` is still an omit-do-not-label field. `tags:` no longer
    /// can be: Story 43.2 gives every stub the kind tag, so a session carrying
    /// none of its own has a tag list of exactly one.
    #[test]
    fn a_session_with_no_participants_omits_that_line_and_still_carries_its_kind_tag() {
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
        assert!(
            !stub.contents.contains("participants:"),
            "an absent fact is omitted, not emitted as an empty label"
        );
        assert!(
            stub.contents.contains("tags:"),
            "a session with no tags of its own is still findable as a recording"
        );
        assert_eq!(fm.as_list("tags"), Some(vec![RECORDINGS_TAG.to_owned()]));
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
    fn empty_tags_are_dropped_and_an_all_blank_tag_list_leaves_only_the_kind_tag() {
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
            Some(vec![
                "work".to_owned(),
                "late".to_owned(),
                RECORDINGS_TAG.to_owned()
            ])
        );

        let blank = ["".to_owned(), "   ".to_owned()];
        let stub = compose(
            &SessionFacts {
                tags: &blank,
                ..facts()
            },
            &[],
        );
        assert_eq!(
            Frontmatter::parse(&stub.contents).0.as_list("tags"),
            Some(vec![RECORDINGS_TAG.to_owned()]),
            "a list with nothing in it is the kind tag alone, never a nameless entry"
        );
    }

    /// The constant is only allowed to be a literal because it is *also* what
    /// the vocabulary makes of it. If a future normalisation rule ever changed
    /// that, this file would emit a tag the tag tree files under a different
    /// name — the exact twin-node failure Story 42.5 closed — and it would show
    /// up here rather than in somebody's sidebar.
    #[test]
    fn the_kind_tag_is_already_what_the_vocabulary_makes_of_it() {
        assert_eq!(
            tags::normalise(RECORDINGS_TAG).as_deref(),
            Some(RECORDINGS_TAG)
        );
    }

    /// AC, first half (Story 43.2, FR-147): the stub says what KIND of note it
    /// is, and it says so *after* the session's own tags, which arrive
    /// untouched and in the order the user typed them.
    #[test]
    fn the_kind_tag_follows_the_sessions_own_tags_and_changes_none_of_them() {
        let own = [
            "Zeta".to_owned(),
            "client/Acme ".to_owned(),
            "alpha".to_owned(),
        ];
        let stub = compose(
            &SessionFacts {
                tags: &own,
                ..facts()
            },
            &[],
        );
        assert_eq!(
            Frontmatter::parse(&stub.contents).0.as_list("tags"),
            Some(vec![
                "Zeta".to_owned(),
                "client/Acme".to_owned(),
                "alpha".to_owned(),
                RECORDINGS_TAG.to_owned()
            ]),
            "the user's own text and their own order survive; keeper's tag trails it"
        );
    }

    /// AC, second half: the tag is the vocabulary's, not a literal. Every one
    /// of these spellings already *is* `recordings` to the tag tree, so
    /// appending keeper's own would put two chips on one note that resolve to
    /// one node — and the writer would see keeper disagree with them about
    /// their own vocabulary.
    #[test]
    fn a_session_that_already_says_recordings_in_any_spelling_gets_exactly_one() {
        for spelling in [
            "recordings",
            "Recordings",
            "RECORDINGS",
            "recordings ",
            "  Recordings",
            "#recordings",
        ] {
            let own = ["work".to_owned(), spelling.to_owned()];
            let stub = compose(
                &SessionFacts {
                    tags: &own,
                    ..facts()
                },
                &[],
            );
            let written = Frontmatter::parse(&stub.contents)
                .0
                .as_list("tags")
                .expect("a stub always carries a tag list");

            assert_eq!(
                written,
                vec!["work".to_owned(), spelling.trim().to_owned()],
                "{spelling:?}: keeper adds nothing beside a tag that is already this one"
            );
            let canonical = tags::normalise_all(written.iter().map(String::as_str));
            assert_eq!(
                canonical.iter().filter(|t| *t == RECORDINGS_TAG).count(),
                1,
                "{spelling:?}: exactly one node in the tag tree, not a near-identical pair"
            );
        }
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

    /// The predicate the Recordings lens and the properties panel both key on,
    /// asserted against a stub this module composed rather than against a
    /// hand-written block — so the writer and the reader cannot drift apart.
    #[test]
    fn a_composed_stub_is_recognised_as_a_recording_note() {
        let stub = compose(&facts(), &[]);
        let (fm, _) = Frontmatter::parse(&stub.contents);
        assert!(is_recording_note(&fm));
    }

    /// A note that merely mentions files is somebody's own note. Keeper does not
    /// claim it, list it under Recordings, or put a recording's buttons in it.
    #[test]
    fn a_note_without_a_session_is_not_a_recording_note() {
        let (fm, _) = Frontmatter::parse(
            "---\ntitle: Groceries\nfiles:\n  - list.txt\nrecording: 2026/whatever\n---\n\nbody\n",
        );
        assert!(!is_recording_note(&fm));
    }

    /// A bare `session:` parses to a key with an empty value, and a recording
    /// whose identity is blank can never be resolved — so it is not one.
    #[test]
    fn a_blank_session_value_is_not_an_identity() {
        let (fm, _) = Frontmatter::parse("---\ntitle: Half a stub\nsession:   \n---\n\nbody\n");
        assert!(!is_recording_note(&fm));
    }
}
