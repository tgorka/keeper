//! What the note editor offers and underlines while a person writes a
//! `keeper-media` body by hand: the grammar's keys with where each may stand,
//! what its value is and a line saying what it does, and a refusal placed on
//! the key or line it is about.
//!
//! The catalogue lives beside the parser so the grammar keeps one owner: a key
//! added to [`super::ROOT_KEYS`] without a row here fails this module's tests.

use std::ops::Range;

use serde::Serialize;
use toml_edit::{Document, Item, TableLike, Value};
use ts_rs::TS;

use super::{
    check_name, check_window, fold, optional_time, parse, tables, time_of, BlockRefusal,
    RECORD_NEW, SOURCE_KEYS,
};

/// Where a key may be written: at the block's root, or inside one of its
/// `[[part]]` or `[[marker]]` tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum MediaKeyPlace {
    Root,
    Part,
    Marker,
}

/// What a key's value is: what the editor offers after its `=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum MediaValueKind {
    /// The grammar version, a whole number.
    Version,
    /// A recording's identity.
    Session,
    /// A transcript file in the drive.
    Transcript,
    /// An audio or video file in the drive.
    Media,
    /// A video file in the drive.
    Video,
    /// A `.toml` file in the drive holding a body of this grammar.
    Config,
    /// Text in quotes.
    Text,
    /// A time: `hh:mm:ss`, `mm:ss` or seconds.
    Time,
    /// One of the key's `values`.
    Choice,
    /// An audio track, counted from 1.
    Track,
    /// Not a value: `[[key]]` tables of their own keys.
    Tables,
}

/// One key of the grammar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaKeyVm {
    pub key: &'static str,
    pub place: MediaKeyPlace,
    pub value: MediaValueKind,
    /// What a `choice` may be, as written between the quotes; empty otherwise.
    pub values: Vec<&'static str>,
    /// Whether the key names what the block plays; a block has exactly one.
    pub source: bool,
    /// One line saying what the key does.
    pub doc: &'static str,
}

/// The grammar, as the editor offers it (`media_block_schema`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaBlockSchemaVm {
    pub keys: Vec<MediaKeyVm>,
}

/// What `picture` may be.
pub const PICTURES: [&str; 3] = ["screen", "camera", "both"];

/// What `sound` may be.
pub const SOUNDS: [&str; 3] = ["system", "microphone", "both"];

type Row = (
    &'static str,
    MediaKeyPlace,
    MediaValueKind,
    &'static [&'static str],
    &'static str,
);

/// Every key, in the order the editor offers them: the sources first, since
/// a block without one does not play.
const KEYS: [Row; 21] = {
    use MediaKeyPlace::{Marker, Part, Root};
    use MediaValueKind::*;
    [
        (
            "session",
            Root,
            Session,
            &[],
            "The recording to play, by its identity — found wherever the recordings index says it is.",
        ),
        (
            "transcript",
            Root,
            Transcript,
            &[],
            "A transcript file in this drive; the player plays the media it was made from.",
        ),
        (
            "part",
            Root,
            Tables,
            &[],
            "A media file to play with no transcript; several [[part]] tables play in order.",
        ),
        (
            "src",
            Root,
            Config,
            &[],
            "A .toml file in this drive holding these keys, so several notes can share one block.",
        ),
        (
            "record",
            Root,
            Choice,
            &[RECORD_NEW],
            "A block that records here: Start in the widget records a new session, and keeper rewrites this to session = \"<id>\" when it stops.",
        ),
        ("title", Root, Text, &[], "The block's own title, in place of the recording's."),
        ("from", Root, Time, &[], "Where the block starts on the source's clock."),
        ("to", Root, Time, &[], "Where the block stops on the source's clock."),
        ("picture", Root, Choice, &PICTURES, "Which pictures the player shows."),
        ("sound", Root, Choice, &SOUNDS, "Which sounds the player plays."),
        (
            "marker",
            Root,
            Tables,
            &[],
            "A named moment (name and at) or a named window (name, from and to).",
        ),
        (
            "version",
            Root,
            Version,
            &[],
            "The grammar version this block is written in; 1.",
        ),
        ("file", Part, Media, &[], "The part's audio or video file, relative to the drive."),
        ("camera", Part, Video, &[], "A video filmed beside the file, shown with it."),
        (
            "offset",
            Part,
            Time,
            &[],
            "Where the part starts on the block's clock; by default where the previous part ends.",
        ),
        ("system", Part, Track, &[], "The file's audio track holding the call, counted from 1."),
        (
            "microphone",
            Part,
            Track,
            &[],
            "The file's audio track holding the microphone, counted from 1.",
        ),
        ("name", Marker, Text, &[], "The moment's name; [[note#name]] links to it."),
        ("at", Marker, Time, &[], "When the moment is."),
        ("from", Marker, Time, &[], "Where the moment's window starts."),
        ("to", Marker, Time, &[], "Where the moment's window ends."),
    ]
};

/// The grammar's keys, where each may stand, what its value is and what it
/// does.
pub fn schema() -> MediaBlockSchemaVm {
    MediaBlockSchemaVm {
        keys: KEYS
            .iter()
            .map(|&(key, place, value, values, doc)| MediaKeyVm {
                key,
                place,
                value,
                values: values.to_vec(),
                source: place == MediaKeyPlace::Root && SOURCE_KEYS.contains(&key),
                doc,
            })
            .collect(),
    }
}

/// Why a body does not read, and where: what the editor underlines while the
/// block's source is open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaBlockProblemVm {
    /// The refusal's sentence, as the block shows it.
    pub message: String,
    /// The body's 1-based line the problem is on; `None` when it is about the
    /// block as a whole.
    pub line: Option<u32>,
    /// Where on that line, `[from, to)`, in UTF-16 code units — the editor's
    /// own — from the line's start.
    pub from: u32,
    pub to: u32,
}

/// `body`'s refusal placed on the key or line it is about, or `None` when the
/// body reads.
pub fn check(body: &str) -> Option<MediaBlockProblemVm> {
    let refusal = parse(body).err()?;
    let message = refusal.to_string();
    let Some(span) = problem_span(body, &refusal) else {
        return Some(MediaBlockProblemVm {
            message,
            line: None,
            from: 0,
            to: 0,
        });
    };
    let (line, from, to) = located(body, span);
    Some(MediaBlockProblemVm {
        message,
        line,
        from,
        to,
    })
}

/// Where a key stands: at the root, or in the `index`th table of an array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Root,
    Table(&'static str, usize),
}

/// The byte range of `body` that `refusal` is about.
fn problem_span(body: &str, refusal: &BlockRefusal) -> Option<Range<usize>> {
    if let BlockRefusal::Syntax(_) = refusal {
        return crate::toml_order::from_str::<toml::Table>(body)
            .err()?
            .span();
    }
    let table: toml::Table = crate::toml_order::from_str(body).ok()?;
    let (place, key) = offender(&table, refusal)?;
    let document = Document::parse(body).ok()?;
    let root = document.as_table();
    match place {
        Place::Root => key_span(root, key?),
        Place::Table(name, index) => {
            let (table, span) = nth_table(root.get(name)?, index)?;
            match key {
                Some(key) => key_span(table, key).or(span),
                None => span,
            }
        }
    }
}

/// Which key of which table `refusal` is about — `None` for the key when it
/// is the table itself — found the way [`parse`] reads, so the first place
/// that fails is the one refused.
fn offender<'r>(
    table: &toml::Table,
    refusal: &'r BlockRefusal,
) -> Option<(Place, Option<&'r str>)> {
    let parts = tables(table, "part").unwrap_or_default();
    let markers = tables(table, "marker").unwrap_or_default();
    let marker_named = |name: &str| {
        markers.iter().position(|marker| {
            marker
                .get("name")
                .and_then(toml::Value::as_str)
                .and_then(|written| check_name(written).ok())
                .is_some_and(|written| written == name)
        })
    };
    match refusal {
        BlockRefusal::UnknownKey { key } => Some((Place::Root, Some(key.as_str()))),
        BlockRefusal::UnknownTableKey { table, key } => {
            let name: &'static str = table;
            let within = if name == "part" { &parts } else { &markers };
            let index = within.iter().position(|each| each.contains_key(key))?;
            Some((Place::Table(name, index), Some(key.as_str())))
        }
        BlockRefusal::NewerVersion => Some((Place::Root, Some("version"))),
        BlockRefusal::NoSource => table
            .contains_key("part")
            .then_some((Place::Root, Some("part"))),
        BlockRefusal::TwoSources { second, .. } => Some((Place::Root, Some(*second))),
        BlockRefusal::WrongType { key, .. } | BlockRefusal::BadTime { key, .. } => {
            let key = key.as_str();
            let bad = |each: &toml::Table| each.get(key).is_some_and(|value| !valid(key, value));
            if bad(table) {
                return Some((Place::Root, Some(key)));
            }
            if let Some(index) = parts.iter().position(|part| bad(part)) {
                return Some((Place::Table("part", index), Some(key)));
            }
            let index = markers.iter().position(|marker| bad(marker))?;
            Some((Place::Table("marker", index), Some(key)))
        }
        BlockRefusal::EmptyWindow => {
            let from = optional_time(table, "from").ok().flatten();
            let to = optional_time(table, "to").ok().flatten();
            if check_window(from, to).is_err() {
                return Some((Place::Root, Some("to")));
            }
            let index = markers.iter().position(|marker| {
                let from = optional_time(marker, "from").ok().flatten();
                let to = optional_time(marker, "to").ok().flatten();
                from.is_some() && to.is_some() && check_window(from, to).is_err()
            })?;
            Some((Place::Table("marker", index), Some("to")))
        }
        BlockRefusal::MarkerBoth { name } => {
            Some((Place::Table("marker", marker_named(name)?), Some("at")))
        }
        BlockRefusal::MarkerTimeless { name } => {
            Some((Place::Table("marker", marker_named(name)?), Some("name")))
        }
        BlockRefusal::NameForbidden | BlockRefusal::NameLength => {
            let index = markers.iter().position(|marker| {
                marker
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|name| check_name(name).is_err())
            })?;
            Some((Place::Table("marker", index), Some("name")))
        }
        BlockRefusal::DuplicateName { name } => {
            let wanted = fold(name);
            let index = markers
                .iter()
                .enumerate()
                .filter(|(_, marker)| {
                    marker
                        .get("name")
                        .and_then(toml::Value::as_str)
                        .is_some_and(|written| fold(written.trim()) == wanted)
                })
                .nth(1)?
                .0;
            Some((Place::Table("marker", index), Some("name")))
        }
        BlockRefusal::PartWithoutFile => {
            let index = parts.iter().position(|part| !part.contains_key("file"))?;
            Some((Place::Table("part", index), None))
        }
        BlockRefusal::TrackZero => parts.iter().enumerate().find_map(|(index, part)| {
            ["system", "microphone"]
                .into_iter()
                .find(|key| matches!(part.get(*key), Some(toml::Value::Integer(0))))
                .map(|key| (Place::Table("part", index), Some(key)))
        }),
        _ => None,
    }
}

/// Whether `value` is one `key` takes — the check [`parse`] makes of it.
fn valid(key: &str, value: &toml::Value) -> bool {
    match key {
        "version" => matches!(value, toml::Value::Integer(version) if *version >= 1),
        "from" | "to" | "at" | "offset" => time_of(key, value).is_ok(),
        "system" | "microphone" => {
            matches!(value, toml::Value::Integer(track) if u32::try_from(*track).is_ok_and(|track| track > 0))
        }
        "part" | "marker" => value
            .as_array()
            .is_some_and(|items| items.iter().all(toml::Value::is_table)),
        "picture" => value.as_str().is_some_and(|word| PICTURES.contains(&word)),
        "sound" => value.as_str().is_some_and(|word| SOUNDS.contains(&word)),
        "record" => value.as_str() == Some(RECORD_NEW),
        _ => value.is_str(),
    }
}

/// The `index`th table of an array of tables, however written, and its span.
fn nth_table(item: &Item, index: usize) -> Option<(&dyn TableLike, Option<Range<usize>>)> {
    match item {
        Item::ArrayOfTables(tables) => {
            let table = tables.get(index)?;
            Some((table as &dyn TableLike, table.span()))
        }
        Item::Value(Value::Array(items)) => {
            let table = items.get(index)?.as_inline_table()?;
            Some((table as &dyn TableLike, table.span()))
        }
        _ => None,
    }
}

/// Where `key` is written in `table`: the key itself, else its value, else —
/// for an array of tables — its first table.
fn key_span(table: &dyn TableLike, key: &str) -> Option<Range<usize>> {
    let (written, item) = table.get_key_value(key)?;
    written
        .span()
        .or_else(|| item.span())
        .or_else(|| nth_table(item, 0).and_then(|(_, span)| span))
}

/// `span` as the body's 1-based line it starts on and its columns there in
/// UTF-16 code units, cut at the line's end. An empty span is its whole line;
/// on an empty line, the block as a whole.
fn located(body: &str, span: Range<usize>) -> (Option<u32>, u32, u32) {
    let start = floor_boundary(body, span.start.min(body.len()));
    let line_start = body[..start].rfind('\n').map_or(0, |at| at + 1);
    let line_end = body[start..].find('\n').map_or(body.len(), |at| start + at);
    let line_end = if body[..line_end].ends_with('\r') {
        line_end - 1
    } else {
        line_end
    };
    let line_text = &body[line_start..line_end];
    if line_text.trim().is_empty() {
        return (None, 0, 0);
    }
    let number = u32::try_from(body[..start].matches('\n').count() + 1).unwrap_or(u32::MAX);
    let start = start.min(line_end);
    let end = floor_boundary(body, span.end.clamp(start, line_end));
    let units = |text: &str| u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX);
    if end == start {
        return (Some(number), 0, units(line_text));
    }
    let from = units(&body[line_start..start]);
    (Some(number), from, from + units(&body[start..end]))
}

/// `at`, moved back to the start of the character it falls in.
fn floor_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::super::{MARKER_KEYS, PART_KEYS, ROOT_KEYS};
    use super::*;

    const ID: &str = "01K0DEVICE0000000000000000-01K0SESSION00000000000000";

    fn keys_at(place: MediaKeyPlace) -> Vec<&'static str> {
        let mut keys: Vec<&str> = schema()
            .keys
            .iter()
            .filter(|key| key.place == place)
            .map(|key| key.key)
            .collect();
        keys.sort_unstable();
        keys
    }

    fn sorted(keys: &[&'static str]) -> Vec<&'static str> {
        let mut keys = keys.to_vec();
        keys.sort_unstable();
        keys
    }

    /// Where the problem is, as the text it underlines.
    fn underlined(body: &str) -> (String, Option<String>) {
        let problem = check(body).expect("the body is refused");
        let Some(line) = problem.line else {
            return (problem.message, None);
        };
        let text: Vec<u16> = body
            .lines()
            .nth(line as usize - 1)
            .expect("the line is in the body")
            .encode_utf16()
            .collect();
        let marked = String::from_utf16(&text[problem.from as usize..problem.to as usize])
            .expect("whole characters");
        (problem.message, Some(format!("{line}:{marked}")))
    }

    #[test]
    fn the_schema_lists_exactly_the_keys_the_parser_takes_where_it_takes_them() {
        assert_eq!(keys_at(MediaKeyPlace::Root), sorted(&ROOT_KEYS));
        assert_eq!(keys_at(MediaKeyPlace::Part), sorted(&PART_KEYS));
        assert_eq!(keys_at(MediaKeyPlace::Marker), sorted(&MARKER_KEYS));
        let sources: Vec<&str> = schema()
            .keys
            .iter()
            .filter(|key| key.source)
            .map(|key| key.key)
            .collect();
        assert_eq!(sorted(&sources), sorted(&SOURCE_KEYS));
    }

    #[test]
    fn every_offered_choice_reads_and_every_key_says_what_it_does() {
        for key in schema().keys {
            assert!(!key.doc.is_empty(), "{} has no doc", key.key);
            assert_eq!(
                key.value == MediaValueKind::Choice,
                !key.values.is_empty(),
                "{}",
                key.key
            );
            for value in key.values {
                let body = if key.source {
                    format!("{} = \"{value}\"", key.key)
                } else {
                    format!("session = \"{ID}\"\n{} = \"{value}\"", key.key)
                };
                assert!(parse(&body).is_ok(), "{body}");
            }
        }
    }

    #[test]
    fn a_body_that_reads_has_no_problem() {
        assert_eq!(
            check(&format!("session = \"{ID}\"\nfrom = \"00:01:00\"")),
            None
        );
    }

    #[test]
    fn an_unknown_root_key_is_underlined_by_name() {
        assert_eq!(
            underlined(&format!("session = \"{ID}\"\nform = \"00:01:00\"")),
            (
                "This block has `form`, which is not a keeper-media key.".to_owned(),
                Some("2:form".to_owned())
            )
        );
    }

    #[test]
    fn a_table_key_is_found_in_the_table_that_has_it() {
        let body = "[[part]]\nfile = \"a.mp4\"\n\n[[part]]\nfile = \"b.mp4\"\ntitle = \"B\"\n";
        assert_eq!(underlined(body).1, Some("6:title".to_owned()));
    }

    #[test]
    fn a_bad_time_is_the_first_place_the_parser_refuses_it() {
        let body = format!(
            "session = \"{ID}\"\nfrom = \"00:01:00\"\n\n[[marker]]\nname = \"Intro\"\nat = \"1:2:3\"\n"
        );
        assert_eq!(
            underlined(&body),
            (
                "`1:2:3` is not a time for `at`: write hh:mm:ss, mm:ss or seconds.".to_owned(),
                Some("6:at".to_owned())
            )
        );
        let root = format!("session = \"{ID}\"\nfrom = \"soon\"\n\n[[marker]]\nname = \"Intro\"\nfrom = \"00:00:01\"\nto = \"00:00:02\"\n");
        assert_eq!(underlined(&root).1, Some("2:from".to_owned()));
    }

    #[test]
    fn a_second_source_is_underlined_not_the_first() {
        let body = format!("session = \"{ID}\"\ntranscript = \"a.transcript.json\"");
        assert_eq!(underlined(&body).1, Some("2:transcript".to_owned()));
    }

    #[test]
    fn a_wrong_choice_is_underlined_on_its_key() {
        let body = format!("session = \"{ID}\"\npicture = \"screens\"");
        assert_eq!(
            underlined(&body),
            (
                "`picture` must be screen, camera or both.".to_owned(),
                Some("2:picture".to_owned())
            )
        );
    }

    #[test]
    fn a_marker_problem_lands_on_that_marker() {
        let both = format!(
            "session = \"{ID}\"\n[[marker]]\nname = \"A\"\nat = 1\n[[marker]]\nname = \"B\"\nat = 2\nfrom = 1\nto = 3\n"
        );
        assert_eq!(underlined(&both).1, Some("7:at".to_owned()));
        let twice = format!(
            "session = \"{ID}\"\n[[marker]]\nname = \"Intro\"\nat = 1\n[[marker]]\nname = \"intro\"\nat = 2\n"
        );
        assert_eq!(underlined(&twice).1, Some("6:name".to_owned()));
        let window = format!("session = \"{ID}\"\n[[marker]]\nname = \"A\"\nfrom = 5\nto = 3\n");
        assert_eq!(underlined(&window).1, Some("5:to".to_owned()));
    }

    #[test]
    fn a_part_without_its_file_is_its_table() {
        let body = "[[part]]\nfile = \"a.mp4\"\n[[part]]\ncamera = \"b.mp4\"\n";
        assert_eq!(underlined(body).1, Some("3:[[part]]".to_owned()));
    }

    #[test]
    fn a_track_zero_is_underlined_on_its_key() {
        let body = "[[part]]\nfile = \"a.mp4\"\nmicrophone = 0\n";
        assert_eq!(underlined(body).1, Some("3:microphone".to_owned()));
    }

    #[test]
    fn a_syntax_error_is_placed_where_toml_stopped_on_its_line() {
        let body = format!("session = \"{ID}\"\ntitle = \"Kelly\ntitle2 = 1\n");
        let problem = check(&body).expect("refused");
        assert!(problem.message.starts_with("This block is not valid TOML"));
        assert_eq!(problem.line, Some(2));
    }

    #[test]
    fn a_block_with_nothing_to_play_is_the_block_as_a_whole() {
        let problem = check("title = \"Kelly\"").expect("refused");
        assert_eq!((problem.line, problem.from, problem.to), (None, 0, 0));
    }

    #[test]
    fn columns_are_counted_in_utf16_units() {
        let body = format!("title = \"Zażółć 🎬\"\nsession = \"{ID}\"\nwhat = 1");
        let problem = check(&body).expect("refused");
        assert_eq!((problem.line, problem.from, problem.to), (Some(3), 0, 4));
        let (line, from, to) = located("a = \"🎬\" x", 9..10);
        assert_eq!((line, from, to), (Some(1), 7, 8));
    }
}
