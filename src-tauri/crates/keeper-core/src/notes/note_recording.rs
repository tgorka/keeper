//! A note that records (Epic 88, story 88.9): the texts keeper reads and
//! writes for a note whose `record = "new"` block started a recording.
//!
//! The block names its session from the moment it starts: the widget that
//! pressed Start splices [`finish_record_body`]'s body over its own fence, an
//! ordinary editor edit. While the session records, the note's frontmatter
//! `tags` carry [`RECORDING_TAG`] and `recording/<device>`, so the recording
//! note is findable from any Mac; when it stops the two tags go. keeper never
//! edits the note's body for a recording — only its tags, each a splice that
//! keeps every other byte (FR-121, AD-354). Pure text in, text out; the shell
//! reads and writes.

use toml_edit::Document;

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::index::IndexEntry;
use crate::notes::media_block::{self, BlockRefusal, Source};
use crate::notes::tags;

/// The tag every note that is recording carries.
pub const RECORDING_TAG: &str = "recording";

/// The frontmatter key the tags are written under.
const TAGS_KEY: &str = "tags";

/// The tag naming the device a note is recording on: `recording/<device>`.
pub fn device_tag(device: &str) -> String {
    format!("{RECORDING_TAG}/{device}")
}

/// The two tags a note recording on `device` carries, in the order they are
/// added.
fn recording_tags(device: &str) -> [String; 2] {
    [RECORDING_TAG.to_owned(), device_tag(device)]
}

/// Whether `written` is `wanted`, as tags compare: normalised, so `Recording`
/// written by hand is the same tag.
fn same_tag(written: &str, wanted: &str) -> bool {
    tags::normalise(written).is_some_and(|written| written == wanted)
}

/// The note's `tags` as a list, `Some(empty)` when there is no such key, and
/// `None` when the key holds something the frontmatter parser does not model
/// — which is never overwritten.
fn tags_of(note: &str) -> Option<Vec<FieldValue>> {
    let (front, _) = Frontmatter::parse(note);
    match front.get(TAGS_KEY) {
        None if front.keys().any(|key| key == TAGS_KEY) => None,
        None => Some(Vec::new()),
        Some(FieldValue::List(items)) => Some(items.clone()),
        Some(FieldValue::Map(_)) => None,
        Some(FieldValue::Str(text)) if text.trim().is_empty() => Some(Vec::new()),
        Some(scalar) => Some(vec![scalar.clone()]),
    }
}

/// `note` with `recording` and `recording/<device>` in its `tags`; the note
/// unchanged when it has both, or when its `tags` is not a list keeper can
/// read.
pub fn with_recording_tags(note: &str, device: &str) -> String {
    let Some(mut items) = tags_of(note) else {
        return note.to_owned();
    };
    let before = items.len();
    for tag in recording_tags(device) {
        if !items
            .iter()
            .any(|item| same_tag(&item.index_string(), &tag))
        {
            items.push(FieldValue::Str(tag));
        }
    }
    if items.len() == before {
        return note.to_owned();
    }
    Frontmatter::set_in(note, TAGS_KEY, FieldValue::List(items))
}

/// `note` without `recording/<device>` in its `tags`, and without `recording`
/// too unless another Mac's `recording/<x>` is still there — that note still
/// records elsewhere. A list left empty takes the key with it: it was keeper
/// that added it.
pub fn without_recording_tags(note: &str, device: &str) -> String {
    let Some(items) = tags_of(note) else {
        return note.to_owned();
    };
    let mine = device_tag(device);
    let others = format!("{RECORDING_TAG}/");
    let elsewhere = items.iter().any(|item| {
        tags::normalise(&item.index_string())
            .is_some_and(|tag| tag.starts_with(&others) && tag != mine)
    });
    let kept: Vec<FieldValue> = items
        .iter()
        .filter(|item| {
            let written = item.index_string();
            !same_tag(&written, &mine) && (elsewhere || !same_tag(&written, RECORDING_TAG))
        })
        .cloned()
        .collect();
    if kept.len() == items.len() {
        return note.to_owned();
    }
    if kept.is_empty() {
        return Frontmatter::remove_in(note, TAGS_KEY);
    }
    Frontmatter::set_in(note, TAGS_KEY, FieldValue::List(kept))
}

/// The body of a `record = "new"` block, rewritten to name `session_id`:
/// `record` becomes `session` in its place, and every other byte stays.
pub fn finish_record_body(body: &str, session_id: &str) -> Result<String, BlockRefusal> {
    let block = media_block::parse(body)?;
    if block.source != Source::Record {
        return Err(BlockRefusal::WrongType {
            key: "record".to_owned(),
            expected: "\"new\"",
        });
    }
    let document =
        Document::parse(body).map_err(|error| BlockRefusal::Syntax(error.to_string()))?;
    let (key, item) = document
        .as_table()
        .get_key_value("record")
        .ok_or(BlockRefusal::NoSource)?;
    let (Some(key_span), Some(value_span)) = (key.span(), item.span()) else {
        return Err(BlockRefusal::NoSource);
    };
    let session = toml_edit::Value::from(session_id).to_string();
    let mut out = String::with_capacity(body.len() + session_id.len());
    out.push_str(&body[..key_span.start]);
    out.push_str("session");
    out.push_str(&body[key_span.end..value_span.start]);
    out.push_str(session.trim());
    out.push_str(&body[value_span.end..]);
    Ok(out)
}

/// Whether a note still names `session_id` in a media block: `disk` is the
/// note as it is on disk (`None` when it is gone), `live` whatever its open
/// editors hold — words typed but not yet saved count, but only while the
/// note exists: a deleted note's editor holds nothing a person can find. A
/// recording whose note names it needs no stub; one whose block was removed,
/// or whose note was deleted, does.
pub fn names_session<'a>(
    disk: Option<&str>,
    live: impl IntoIterator<Item = &'a str>,
    session_id: &str,
) -> bool {
    let names = |text: &str| {
        media_block::session_ids(text)
            .iter()
            .any(|id| id.trim() == session_id)
    };
    disk.is_some_and(|disk| names(disk) || live.into_iter().any(names))
}

/// The notes of an index still tagged as recording on `device`: what the
/// stale-tag sweep untags when nothing records on this Mac.
pub fn tagged_on<'a>(
    entries: &'a [IndexEntry],
    device: &str,
) -> impl Iterator<Item = &'a IndexEntry> + 'a {
    let wanted = tags::normalise(&device_tag(device));
    entries.iter().filter(move |entry| {
        wanted
            .as_deref()
            .is_some_and(|wanted| entry.tags.iter().any(|tag| tag == wanted))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01KYDKP6SN2HR4SJBJ9JTBVC2Z-01KYDM0000000000000000000A";

    #[test]
    fn a_record_block_is_rewritten_to_name_its_session_and_keeps_every_other_byte() {
        let body = "# the weekly one\ntitle = \"Kelly\"  # kept\nrecord   =   'new' # here\nsound = \"both\"\n";
        assert_eq!(
            finish_record_body(body, ID).expect("rewrites"),
            format!(
                "# the weekly one\ntitle = \"Kelly\"  # kept\nsession   =   \"{ID}\" # here\nsound = \"both\"\n"
            )
        );
    }

    #[test]
    fn only_a_record_block_is_finished() {
        assert!(finish_record_body("session = \"S\"\n", ID).is_err());
        assert!(finish_record_body("record = \"old\"\n", ID).is_err());
    }

    #[test]
    fn a_note_that_still_names_the_session_needs_no_stub() {
        let named = format!("---\ntitle: Standup\n---\nAgenda.\n\n  ```keeper-media\n  # mine\n  session = \"{ID}\"\n  ```\n");
        let recording = "Agenda.\n\n```keeper-media\nrecord = \"new\"\n```\n";
        let other = "```keeper-media\nsession = \"OTHER\"\n```\n";

        // On disk.
        assert!(names_session(Some(&named), [], ID));
        // Only in an open editor's words, not yet saved.
        assert!(names_session(Some(recording), [named.as_str()], ID));
        // A failed session's block keeps its name, so it is its note too.
        assert!(names_session(Some(&named), [recording], ID));

        // The block was removed, or never named it: a stub.
        assert!(!names_session(Some("Agenda.\n"), ["Agenda.\n"], ID));
        assert!(!names_session(Some(recording), [other], ID));
        // The note was deleted: a stub, even while an editor still holds it.
        assert!(!names_session(None, [], ID));
        assert!(!names_session(None, [named.as_str()], ID));
    }

    /// An index entry for `text`, tagged the way the vault index tags it.
    fn indexed(path: &str, text: &str) -> IndexEntry {
        let (front, body_offset) = Frontmatter::parse(text);
        IndexEntry {
            id: format!("id:{path}"),
            path: path.to_owned(),
            title: path.to_owned(),
            size: 1,
            mtime_ns: 1,
            ino: 1,
            created_ms: 0,
            updated_ms: 0,
            tags: tags::note_tags(&front, &text[body_offset..]),
            fields: std::collections::BTreeMap::new(),
            links: Vec::new(),
            link_predicates: std::collections::BTreeMap::new(),
            flags: Vec::new(),
            snippet: String::new(),
            order: crate::notes::order::NoteOrder::default(),
        }
    }

    #[test]
    fn the_sweep_untags_this_macs_stale_notes_and_leaves_another_macs_alone() {
        let here = with_recording_tags("---\ntags: [work]\n---\nLeft over.\n", "hesperia");
        let there = with_recording_tags("---\ntitle: B\n---\nRecording on orion.\n", "orion");
        let plain = "---\ntags: [recording]\n---\nAbout recordings.\n";
        let entries = vec![
            indexed("here.md", &here),
            indexed("there.md", &there),
            indexed("plain.md", plain),
        ];

        let swept: Vec<&str> = tagged_on(&entries, "hesperia")
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(swept, vec!["here.md"]);
        assert_eq!(
            without_recording_tags(&here, "hesperia"),
            "---\ntags: [work]\n---\nLeft over.\n"
        );
        assert_eq!(
            tagged_on(&entries, "orion")
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            vec!["there.md"],
            "orion's note is orion's to untag"
        );
    }

    #[test]
    fn tags_are_added_once_and_removed_without_touching_the_rest() {
        let note = "---\ntitle: A\n---\nBody\n";
        let tagged = with_recording_tags(note, "hesperia");
        assert_eq!(
            Frontmatter::parse(&tagged).0.as_list("tags"),
            Some(vec![
                "recording".to_owned(),
                "recording/hesperia".to_owned()
            ])
        );
        assert_eq!(with_recording_tags(&tagged, "hesperia"), tagged);
        assert_eq!(without_recording_tags(&tagged, "hesperia"), note);

        let listed = "---\ntags: [Recording, work]\n---\n";
        let added = with_recording_tags(listed, "hesperia");
        assert_eq!(
            Frontmatter::parse(&added).0.as_list("tags"),
            Some(vec![
                "Recording".to_owned(),
                "work".to_owned(),
                "recording/hesperia".to_owned()
            ])
        );
        assert_eq!(
            without_recording_tags(&added, "hesperia"),
            "---\ntags: [work]\n---\n"
        );
        // Another Mac's device tag is that Mac's to remove, and while it is
        // there the note still records: `recording` stays with it.
        let other = with_recording_tags(note, "orion");
        assert_eq!(
            Frontmatter::parse(&without_recording_tags(&other, "hesperia"))
                .0
                .as_list("tags"),
            Some(vec!["recording".to_owned(), "recording/orion".to_owned()])
        );
    }

    #[test]
    fn the_shared_recording_tag_stays_while_another_mac_records_in_the_note() {
        let both = "---\ntags: [recording, recording/orion, recording/hesperia]\n---\nBody\n";
        assert_eq!(
            Frontmatter::parse(&without_recording_tags(both, "hesperia"))
                .0
                .as_list("tags"),
            Some(vec!["recording".to_owned(), "recording/orion".to_owned()])
        );
        let orion_done = without_recording_tags(both, "orion");
        assert_eq!(
            without_recording_tags(&orion_done, "hesperia"),
            "Body\n",
            "the last Mac to stop takes `recording` with it"
        );
    }

    #[test]
    fn a_tags_value_keeper_cannot_read_is_left_alone() {
        let note = "---\ntags: !!set {a}\n---\n";
        assert_eq!(with_recording_tags(note, "d"), note);
        assert_eq!(without_recording_tags(note, "d"), note);
    }
}
