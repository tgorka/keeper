//! The `keeper-media` block: a fenced code block in a note whose TOML body
//! names a recording, a transcript, a list of media parts or a config file in
//! the drive, and which keeper draws as a player with the transcript's lines
//! (Epic 88, AD-351…AD-357, D-30).
//!
//! This module is the grammar and every text keeper writes into a block, and
//! nothing else: parsing and its refusals, Normal Play Time, the byte-keeping
//! marker edits, clips, the stub's block, finding blocks in a note, and the
//! rewrite of an old recording stub's per-file embeds into one block. It reads
//! no file, for [`crate::notes`]' reason; resolving a block's names against a
//! drive and the recordings index is
//! [`crate::transcription::media::resolve_block`]'s, over facts the shell
//! hands it.
//!
//! **Only Rust reads a body.** The editor hands the body over verbatim and
//! splices back what comes out of here, so TypeScript never reads a key and
//! never joins a path (AD-351, AD-353).
//!
//! **The grammar is closed.** A key this module does not know is refused by
//! name: a misspelt `form = …` that were ignored would play the whole
//! recording, which is the costlier failure (AD-352).
//!
//! **An edit changes only the table it touches.** A marker is added, renamed
//! or removed through `toml_edit`, and every other byte of the body — its
//! comments, its blank lines, its spelling of times — stays (AD-354).

use std::borrow::Cow;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, Value};
use ts_rs::TS;

use crate::archive::recordings_fts::kind_for_file_name;
use crate::notes::frontmatter::Frontmatter;
use crate::notes::links;
use crate::notes::recording_note::{self, SESSION_KEY};
use crate::transcription::model::{Transcript, Utterance};
use crate::transcription::render;
use crate::transcription::words::fold;
use crate::vm::RecordingNoteTargetKind;

/// The info string's first word that makes a fence a media block.
pub const INFO_WORD: &str = "keeper-media";

/// The grammar version this keeper reads and writes.
pub const GRAMMAR_VERSION: i64 = 1;

/// The most characters a marker's name may hold.
pub const MAX_NAME_CHARS: usize = 80;

/// The largest `src` file keeper reads, in bytes.
pub const MAX_SRC_BYTES: u64 = 64 * 1024;

/// What a wikilink cannot carry, and so what a marker name may not hold: a
/// marker is linked as `[[note#name]]`.
const NAME_FORBIDDEN: [char; 7] = ['[', ']', '|', '#', '^', '\n', '\r'];

/// Why a block cannot be read, or an edit or a clip cannot be made. Each is
/// the sentence the block shows above its own source (UX-DR44).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlockRefusal {
    #[error("This block is not valid TOML: {0}")]
    Syntax(String),
    #[error("This block has `{key}`, which is not a keeper-media key.")]
    UnknownKey { key: String },
    #[error("A [[{table}]] has `{key}`, which is not one of its keys.")]
    UnknownTableKey { table: &'static str, key: String },
    #[error("This block was written by a newer keeper.")]
    NewerVersion,
    #[error(
        "This block names nothing to play: give it one of session, transcript, [[part]], src or record."
    )]
    NoSource,
    #[error("This block names both {first} and {second}; a block plays exactly one of them.")]
    TwoSources {
        first: &'static str,
        second: &'static str,
    },
    #[error("The file named by src names src itself; it must name what to play.")]
    NestedSrc,
    #[error("The file named by src cannot record; record = \"new\" belongs in the note.")]
    RecordInSrc,
    #[error("`{key}` must be {expected}.")]
    WrongType { key: String, expected: &'static str },
    #[error("`{text}` is not a time for `{key}`: write hh:mm:ss, mm:ss or seconds.")]
    BadTime { key: String, text: String },
    #[error("`from` must come before `to`.")]
    EmptyWindow,
    #[error("The moment {name} needs either `at`, or `from` and `to`.")]
    MarkerTimeless { name: String },
    #[error("The moment {name} has `at` and a window; give it one or the other.")]
    MarkerBoth { name: String },
    #[error("A moment's name cannot hold [ ] | # ^ or a line break.")]
    NameForbidden,
    #[error("A moment's name is 1 to 80 characters.")]
    NameLength,
    #[error("There is already a moment called {name} in this block.")]
    DuplicateName { name: String },
    #[error("No moment called {name} in this block.")]
    NoSuchMarker { name: String },
    #[error("A [[part]] needs its `file`.")]
    PartWithoutFile,
    #[error("Track numbers count from 1.")]
    TrackZero,
    #[error("This clip reaches outside the block's own window.")]
    ClipOutsideWindow,
    #[error("This transcript is not in a synced folder, so a note cannot name it.")]
    NotNameable,
}

/// Which pictures the player shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum MediaPicture {
    Screen,
    Camera,
    Both,
}

/// Which sounds the player plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum MediaSound {
    System,
    Microphone,
    Both,
}

/// A body, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub source: Source,
    pub title: Option<String>,
    /// Seconds on the source's clock; the window is `[from, to)`.
    pub from: Option<f64>,
    pub to: Option<f64>,
    pub picture: Option<MediaPicture>,
    pub sound: Option<MediaSound>,
    pub markers: Vec<Marker>,
}

/// The one thing a block plays.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// A recording's identity, `<device ULID>-<session ULID>`.
    Session(String),
    /// A transcript file, relative to the drive.
    Transcript(String),
    /// Media with no transcript, in play order.
    Parts(Vec<Part>),
    /// A `.toml` file in the drive holding a body of this grammar.
    Src(String),
    /// `record = "new"`: nothing recorded yet. The block's widget records a
    /// new session, and keeper rewrites this to `session` when it stops.
    Record,
}

/// One `[[part]]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// The main file, relative to the drive.
    pub file: String,
    /// A video filmed beside it, relative to the drive.
    pub camera: Option<String>,
    /// Where the part starts on the block's clock; `None` is where the
    /// previous part ends.
    pub offset: Option<f64>,
    /// The call's audio track, counted from 1 as a person counts.
    pub system: Option<u32>,
    /// The microphone's audio track, counted from 1.
    pub microphone: Option<u32>,
}

impl Part {
    /// The call's track as the engine and `HTMLMediaElement.audioTracks`
    /// count it, from 0 (Q8).
    pub fn system_index(&self) -> Option<u32> {
        self.system.map(|track| track - 1)
    }

    /// The microphone's track, from 0 (Q8).
    pub fn microphone_index(&self) -> Option<u32> {
        self.microphone.map(|track| track - 1)
    }
}

/// A named moment, or a named window, on the source's clock.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub name: String,
    pub from: f64,
    /// `None` for a moment.
    pub to: Option<f64>,
}

const ROOT_KEYS: [&str; 12] = [
    "version",
    "session",
    "transcript",
    "part",
    "src",
    "title",
    "from",
    "to",
    "picture",
    "sound",
    "marker",
    "record",
];
const PART_KEYS: [&str; 5] = ["file", "camera", "offset", "system", "microphone"];
const MARKER_KEYS: [&str; 4] = ["name", "at", "from", "to"];
const SOURCE_KEYS: [&str; 5] = ["session", "transcript", "part", "src", "record"];

/// The one value `record` takes: the block has not recorded yet.
pub const RECORD_NEW: &str = "new";

/// Read a body.
pub fn parse(body: &str) -> Result<Block, BlockRefusal> {
    let table: toml::Table = toml::from_str(body)
        .map_err(|error| BlockRefusal::Syntax(first_line(&error.to_string())))?;
    if let Some(version) = table.get("version") {
        match version {
            toml::Value::Integer(version) if *version > GRAMMAR_VERSION => {
                return Err(BlockRefusal::NewerVersion)
            }
            toml::Value::Integer(version) if *version >= 1 => {}
            _ => {
                return Err(BlockRefusal::WrongType {
                    key: "version".to_owned(),
                    expected: "a whole number, 1",
                })
            }
        }
    }
    if let Some(key) = table.keys().find(|key| !ROOT_KEYS.contains(&key.as_str())) {
        return Err(BlockRefusal::UnknownKey { key: key.clone() });
    }
    let named: Vec<&'static str> = SOURCE_KEYS
        .iter()
        .copied()
        .filter(|key| table.contains_key(*key))
        .collect();
    let source = match named[..] {
        [] => return Err(BlockRefusal::NoSource),
        [first, second, ..] => return Err(BlockRefusal::TwoSources { first, second }),
        ["session"] => Source::Session(text(&table, "session")?),
        ["transcript"] => Source::Transcript(text(&table, "transcript")?),
        ["src"] => Source::Src(text(&table, "src")?),
        ["record"] => {
            if text(&table, "record")? != RECORD_NEW {
                return Err(BlockRefusal::WrongType {
                    key: "record".to_owned(),
                    expected: "\"new\"",
                });
            }
            Source::Record
        }
        _ => Source::Parts(parts(&table)?),
    };
    let block = Block {
        source,
        title: optional_text(&table, "title")?,
        from: optional_time(&table, "from")?,
        to: optional_time(&table, "to")?,
        picture: optional_choice(
            &table,
            "picture",
            "screen, camera or both",
            |word| match word {
                "screen" => Some(MediaPicture::Screen),
                "camera" => Some(MediaPicture::Camera),
                "both" => Some(MediaPicture::Both),
                _ => None,
            },
        )?,
        sound: optional_choice(
            &table,
            "sound",
            "system, microphone or both",
            |word| match word {
                "system" => Some(MediaSound::System),
                "microphone" => Some(MediaSound::Microphone),
                "both" => Some(MediaSound::Both),
                _ => None,
            },
        )?,
        markers: markers(&table)?,
    };
    check_window(block.from, block.to)?;
    check_unique(&block.markers)?;
    Ok(block)
}

/// Read the body of a `src` file: the same grammar, naming what to play
/// itself.
pub fn parse_src(body: &str) -> Result<Block, BlockRefusal> {
    let block = parse(body)?;
    match block.source {
        Source::Src(_) => Err(BlockRefusal::NestedSrc),
        Source::Record => Err(BlockRefusal::RecordInSrc),
        _ => Ok(block),
    }
}

impl Block {
    /// This block, whose source is `src`, over the file it names: the file's
    /// source, the block's own `title`, window and choices where it has them
    /// and the file's where it does not, and both sets of markers, whose
    /// names must not collide.
    pub fn over_src(self, file: Block) -> Result<Block, BlockRefusal> {
        let mut markers = file.markers;
        markers.extend(self.markers);
        check_unique(&markers)?;
        let block = Block {
            source: file.source,
            title: self.title.or(file.title),
            from: self.from.or(file.from),
            to: self.to.or(file.to),
            picture: self.picture.or(file.picture),
            sound: self.sound.or(file.sound),
            markers,
        };
        check_window(block.from, block.to)?;
        Ok(block)
    }

    /// The marker called `name`, compared as the transcription code folds
    /// case.
    pub fn marker(&self, name: &str) -> Option<&Marker> {
        let wanted = fold(name.trim());
        self.markers
            .iter()
            .find(|marker| fold(&marker.name) == wanted)
    }
}

fn first_line(message: &str) -> String {
    message
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(message)
        .trim()
        .to_owned()
}

fn text(table: &toml::Table, key: &str) -> Result<String, BlockRefusal> {
    match table.get(key) {
        Some(toml::Value::String(value)) => Ok(value.clone()),
        _ => Err(BlockRefusal::WrongType {
            key: key.to_owned(),
            expected: "text in quotes",
        }),
    }
}

fn optional_text(table: &toml::Table, key: &str) -> Result<Option<String>, BlockRefusal> {
    table.get(key).map(|_| text(table, key)).transpose()
}

fn optional_time(table: &toml::Table, key: &str) -> Result<Option<f64>, BlockRefusal> {
    table.get(key).map(|value| time_of(key, value)).transpose()
}

fn optional_choice<T>(
    table: &toml::Table,
    key: &str,
    expected: &'static str,
    choose: impl Fn(&str) -> Option<T>,
) -> Result<Option<T>, BlockRefusal> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    value
        .as_str()
        .and_then(choose)
        .map(Some)
        .ok_or(BlockRefusal::WrongType {
            key: key.to_owned(),
            expected,
        })
}

fn tables<'t>(table: &'t toml::Table, key: &str) -> Result<Vec<&'t toml::Table>, BlockRefusal> {
    let wrong = || BlockRefusal::WrongType {
        key: key.to_owned(),
        expected: "written as [[tables]]",
    };
    match table.get(key) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(items)) => items
            .iter()
            .map(|item| item.as_table().ok_or_else(wrong))
            .collect(),
        Some(_) => Err(wrong()),
    }
}

fn parts(table: &toml::Table) -> Result<Vec<Part>, BlockRefusal> {
    let tables = tables(table, "part")?;
    if tables.is_empty() {
        return Err(BlockRefusal::NoSource);
    }
    tables
        .into_iter()
        .map(|part| {
            if let Some(key) = part.keys().find(|key| !PART_KEYS.contains(&key.as_str())) {
                return Err(BlockRefusal::UnknownTableKey {
                    table: "part",
                    key: key.clone(),
                });
            }
            if !part.contains_key("file") {
                return Err(BlockRefusal::PartWithoutFile);
            }
            Ok(Part {
                file: text(part, "file")?,
                camera: optional_text(part, "camera")?,
                offset: optional_time(part, "offset")?,
                system: track(part, "system")?,
                microphone: track(part, "microphone")?,
            })
        })
        .collect()
}

fn track(table: &toml::Table, key: &str) -> Result<Option<u32>, BlockRefusal> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Integer(0)) => Err(BlockRefusal::TrackZero),
        Some(toml::Value::Integer(number)) if *number > 0 => u32::try_from(*number)
            .map(Some)
            .map_err(|_| BlockRefusal::WrongType {
                key: key.to_owned(),
                expected: "a track number",
            }),
        Some(_) => Err(BlockRefusal::WrongType {
            key: key.to_owned(),
            expected: "a track number, counted from 1",
        }),
    }
}

fn markers(table: &toml::Table) -> Result<Vec<Marker>, BlockRefusal> {
    tables(table, "marker")?
        .into_iter()
        .map(|marker| {
            if let Some(key) = marker
                .keys()
                .find(|key| !MARKER_KEYS.contains(&key.as_str()))
            {
                return Err(BlockRefusal::UnknownTableKey {
                    table: "marker",
                    key: key.clone(),
                });
            }
            let name = check_name(&text(marker, "name")?)?;
            let at = optional_time(marker, "at")?;
            let from = optional_time(marker, "from")?;
            let to = optional_time(marker, "to")?;
            match (at, from, to) {
                (Some(at), None, None) => Ok(Marker {
                    name,
                    from: at,
                    to: None,
                }),
                (None, Some(from), Some(to)) => {
                    check_window(Some(from), Some(to))?;
                    Ok(Marker {
                        name,
                        from,
                        to: Some(to),
                    })
                }
                (Some(_), _, _) => Err(BlockRefusal::MarkerBoth { name }),
                _ => Err(BlockRefusal::MarkerTimeless { name }),
            }
        })
        .collect()
}

fn check_window(from: Option<f64>, to: Option<f64>) -> Result<(), BlockRefusal> {
    match (from, to) {
        (Some(from), Some(to)) if from >= to => Err(BlockRefusal::EmptyWindow),
        (None, Some(to)) if to <= 0.0 => Err(BlockRefusal::EmptyWindow),
        _ => Ok(()),
    }
}

fn check_unique(markers: &[Marker]) -> Result<(), BlockRefusal> {
    for (index, marker) in markers.iter().enumerate() {
        let folded = fold(&marker.name);
        if markers[..index]
            .iter()
            .any(|earlier| fold(&earlier.name) == folded)
        {
            return Err(BlockRefusal::DuplicateName {
                name: marker.name.clone(),
            });
        }
    }
    Ok(())
}

/// A marker name as it will be written — trimmed — or why it cannot be one.
pub fn check_name(name: &str) -> Result<String, BlockRefusal> {
    let name = name.trim();
    if name.contains(NAME_FORBIDDEN) {
        return Err(BlockRefusal::NameForbidden);
    }
    let count = name.chars().count();
    if count == 0 || count > MAX_NAME_CHARS {
        return Err(BlockRefusal::NameLength);
    }
    Ok(name.to_owned())
}

fn time_of(key: &str, value: &toml::Value) -> Result<f64, BlockRefusal> {
    let bad = |text: String| BlockRefusal::BadTime {
        key: key.to_owned(),
        text,
    };
    match value {
        toml::Value::Integer(seconds) if *seconds >= 0 => Ok(*seconds as f64),
        toml::Value::Float(seconds) if seconds.is_finite() && *seconds >= 0.0 => Ok(*seconds),
        toml::Value::String(text) => parse_time(text).ok_or_else(|| bad(text.clone())),
        other => Err(bad(other.to_string())),
    }
}

/// A W3C Normal Play Time string — `ss[.f]`, `mm:ss[.f]` or `hh:mm:ss[.f]`,
/// with `mm` and `ss` exactly two digits below 60 and hours any number of
/// digits (Media Fragments §4.2.1) — in seconds. `npt:` may lead.
pub fn parse_time(text: &str) -> Option<f64> {
    let text = text.strip_prefix("npt:").unwrap_or(text);
    let fields: Vec<&str> = text.split(':').collect();
    let (hours, minutes, seconds) = match fields[..] {
        [seconds] => (None, None, seconds),
        [minutes, seconds] => (None, Some(minutes), seconds),
        [hours, minutes, seconds] => (Some(hours), Some(minutes), seconds),
        _ => return None,
    };
    let (whole, fraction) = match seconds.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (seconds, None),
    };
    let digits = |field: &str| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(whole) || fraction.is_some_and(|fraction| !digits(fraction)) {
        return None;
    }
    let sexagesimal = |field: &str| {
        (field.len() == 2 && digits(field))
            .then(|| field.parse::<u32>().ok())
            .flatten()
            .filter(|value| *value < 60)
    };
    let mut total: f64 = seconds.parse().ok()?;
    if let Some(minutes) = minutes {
        sexagesimal(whole)?;
        total += f64::from(sexagesimal(minutes)?) * 60.0;
    }
    if let Some(hours) = hours {
        if !digits(hours) {
            return None;
        }
        total += hours.parse::<f64>().ok()? * 3_600.0;
    }
    Some(total)
}

/// `hh:mm:ss` for whole seconds, the one form keeper writes.
fn clock(total: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        total / 3_600,
        total / 60 % 60,
        total % 60
    )
}

/// `seconds`, rounded down to a whole second, as keeper writes a time.
pub fn format_time(seconds: f64) -> String {
    clock(seconds.max(0.0).floor() as u64)
}

/// `seconds`, rounded up — the end of a window drawn around lines.
fn format_time_up(seconds: f64) -> String {
    clock(seconds.max(0.0).ceil() as u64)
}

/// A TOML string literal for `text`.
fn quoted(text: &str) -> String {
    Value::from(text).to_string().trim().to_owned()
}

/// `body` fenced as a media block, ending with a newline.
pub fn fenced(body: &str) -> String {
    let mut out = format!("```{INFO_WORD}\n{body}");
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n");
    out
}

/// The optional root keys a block keeper writes lists, commented out, when
/// it does not set them: the grammar's own reference, where a person who
/// opens the block to write it by hand is looking. The parser skips them.
const KEY_HINTS: [(&str, &str); 5] = [
    ("title", "# title = \"\""),
    ("from", "# from = \"00:00:00\""),
    ("to", "# to = \"\""),
    (
        "picture",
        "# picture = \"both\"      # screen | camera | both",
    ),
    (
        "sound",
        "# sound = \"both\"        # system | microphone | both",
    ),
];

/// A marker's shape, for a block that has none.
const MARKER_HINT: &str = "# [[marker]]\n# name = \"\"\n# at = \"00:00:00\"\n";

/// What else a block may play, last in every block keeper writes.
const SOURCES_HINT: &str =
    "# sources: session | transcript | [[part]] file/camera/offset/system/microphone | src\n";

/// A block keeper writes: `keys` as `key = value` lines, each optional root
/// key it does not set commented out, then `tables` — its `[[part]]` and
/// `[[marker]]` tables, each opening with a blank line — then a marker's
/// shape when `markers` is false, and the sources. The hints sit above the
/// tables because a key under `[[part]]` belongs to the part: uncommented
/// there, `title` would be refused.
fn described(keys: &[(&str, String)], tables: &str, markers: bool) -> String {
    let mut out = String::new();
    for (key, value) in keys {
        let _ = writeln!(out, "{key} = {value}");
    }
    for (key, hint) in KEY_HINTS {
        if !keys.iter().any(|(set, _)| *set == key) {
            out.push_str(hint);
            out.push('\n');
        }
    }
    if !tables.is_empty() {
        out.push_str(tables);
        out.push('\n');
    }
    if !markers {
        out.push_str(MARKER_HINT);
    }
    out.push_str(SOURCES_HINT);
    fenced(&out)
}

/// A block naming a recording by its identity: what the stub of a
/// recording carries, and what a picked recording inserts (AD-357).
pub fn session_block(session_id: &str) -> String {
    described(&[("session", quoted(session_id))], "", false)
}

/// A block naming a transcript file, relative to the drive.
pub fn transcript_block(relative_path: &str) -> String {
    described(&[("transcript", quoted(relative_path))], "", false)
}

/// A block of one media file with no transcript, relative to the drive.
pub fn part_block(relative_path: &str) -> String {
    described(
        &[],
        &format!("\n[[part]]\nfile = {}\n", quoted(relative_path)),
        false,
    )
}

/// A block that records here (`record = "new"`): its widget starts a new
/// session, and keeper rewrites the block to name it when it stops.
pub fn record_block() -> String {
    described(&[("record", quoted(RECORD_NEW))], "", true)
}

/// What the person picked to play: a recording from the recordings index,
/// or a file in the note's drive (N3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum MediaPickReq {
    Session {
        session_id: String,
    },
    /// Relative to the drive, as the Files picker answers.
    File {
        relative_path: String,
    },
    /// Nothing yet: a block that records a new session (`record = "new"`).
    NewRecording,
}

/// An add, a rename or a removal of one marker, as the note's editor asks
/// for it (UX-DR125).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum MarkerEditReq {
    /// A moment at `from`, or the window `[from, to)`, in seconds. `from` is
    /// written rounded down and `to` rounded up.
    Add {
        name: String,
        from: f64,
        to: Option<f64>,
    },
    Rename {
        name: String,
        new_name: String,
    },
    Remove {
        name: String,
    },
}

/// `body` with one marker added, renamed or removed and every other byte as
/// it was (AD-354). A body that does not read is refused, so nothing is
/// handed back to splice over it.
pub fn edit(body: &str, request: &MarkerEditReq) -> Result<String, BlockRefusal> {
    let block = parse(body)?;
    let mut document: DocumentMut = body.parse().map_err(|error: toml_edit::TomlError| {
        BlockRefusal::Syntax(first_line(&error.to_string()))
    })?;
    match request {
        MarkerEditReq::Add { name, from, to } => {
            let name = check_name(name)?;
            if block.marker(&name).is_some() {
                return Err(BlockRefusal::DuplicateName { name });
            }
            let mut table = Table::new();
            table.insert("name", toml_edit::value(name.as_str()));
            match to {
                None => {
                    table.insert("at", toml_edit::value(format_time(*from)));
                }
                Some(to) => {
                    let (from, to) = (format_time(*from), format_time_up(*to));
                    if parse_time(&from) >= parse_time(&to) {
                        return Err(BlockRefusal::EmptyWindow);
                    }
                    table.insert("from", toml_edit::value(from));
                    table.insert("to", toml_edit::value(to));
                }
            }
            append_marker(body, &document, table)
        }
        MarkerEditReq::Rename { name, new_name } => {
            let new_name = check_name(new_name)?;
            let index = marker_index(&block, name)?;
            if block
                .marker(&new_name)
                .is_some_and(|other| fold(&other.name) != fold(&block.markers[index].name))
            {
                return Err(BlockRefusal::DuplicateName { name: new_name });
            }
            let table = marker_table_mut(&mut document, index)?;
            let Some(value) = table.get_mut("name").and_then(Item::as_value_mut) else {
                return Err(BlockRefusal::NoSuchMarker { name: name.clone() });
            };
            let decor = value.decor().clone();
            *value = Value::from(new_name);
            *value.decor_mut() = decor;
            Ok(document.to_string())
        }
        MarkerEditReq::Remove { name } => {
            let index = marker_index(&block, name)?;
            match document.get_mut("marker") {
                Some(Item::ArrayOfTables(array)) => {
                    array.remove(index);
                }
                Some(Item::Value(Value::Array(array))) => {
                    array.remove(index);
                }
                _ => return Err(BlockRefusal::NoSuchMarker { name: name.clone() }),
            }
            let emptied = match document.get("marker") {
                Some(Item::ArrayOfTables(array)) => array.is_empty(),
                Some(Item::Value(Value::Array(array))) => array.is_empty(),
                _ => false,
            };
            if emptied {
                document.remove("marker");
            }
            Ok(document.to_string())
        }
    }
}

fn marker_index(block: &Block, name: &str) -> Result<usize, BlockRefusal> {
    let wanted = fold(name.trim());
    block
        .markers
        .iter()
        .position(|marker| fold(&marker.name) == wanted)
        .ok_or_else(|| BlockRefusal::NoSuchMarker {
            name: name.trim().to_owned(),
        })
}

/// The `index`th marker as an editable table, written either way TOML
/// allows: `[[marker]]` tables or an inline array.
fn marker_table_mut(
    document: &mut DocumentMut,
    index: usize,
) -> Result<&mut dyn toml_edit::TableLike, BlockRefusal> {
    let missing = || BlockRefusal::NoSuchMarker {
        name: String::new(),
    };
    match document.get_mut("marker") {
        Some(Item::ArrayOfTables(array)) => array
            .get_mut(index)
            .map(|table| table as &mut dyn toml_edit::TableLike)
            .ok_or_else(missing),
        Some(Item::Value(Value::Array(array))) => match array.get_mut(index) {
            Some(Value::InlineTable(table)) => Ok(table as &mut dyn toml_edit::TableLike),
            _ => Err(missing()),
        },
        _ => Err(missing()),
    }
}

/// Append a `[[marker]]` table. With `[[marker]]` tables already, or none,
/// the new one is text appended after every other byte, so nothing above it
/// moves. An inline `marker = [...]` array gains an inline table instead,
/// because a `[[marker]]` after it would redefine the key.
fn append_marker(body: &str, document: &DocumentMut, table: Table) -> Result<String, BlockRefusal> {
    if let Some(Item::Value(Value::Array(_))) = document.get("marker") {
        let mut document = document.clone();
        if let Some(Item::Value(Value::Array(array))) = document.get_mut("marker") {
            array.push(table.into_inline_table());
        }
        return Ok(document.to_string());
    }
    let mut out = body.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.trim().is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str("[[marker]]\n");
    for (key, item) in table.iter() {
        if let Some(value) = item.as_value() {
            let _ = writeln!(out, "{key} = {}", value.to_string().trim());
        }
    }
    Ok(out)
}

/// A clip's window as the person typed it; either end may be left empty.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClipWindow {
    pub from: Option<f64>,
    pub to: Option<f64>,
}

impl ClipWindow {
    /// Read `from` and `to` as typed. An empty or absent end is no end.
    pub fn parse(from: Option<&str>, to: Option<&str>) -> Result<Self, BlockRefusal> {
        let end = |key: &str, text: Option<&str>| match text.map(str::trim) {
            None | Some("") => Ok(None),
            Some(text) => parse_time(text)
                .map(Some)
                .ok_or_else(|| BlockRefusal::BadTime {
                    key: key.to_owned(),
                    text: text.to_owned(),
                }),
        };
        let window = Self {
            from: end("from", from)?,
            to: end("to", to)?,
        };
        check_window(window.from, window.to)?;
        Ok(window)
    }
}

/// A composed clip: the Markdown for the clipboard, and how many of the
/// transcript's lines its window holds (UX-DR126).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaClipVm {
    pub markdown: String,
    pub lines: u32,
}

/// A clip of the block `body`: its source keys verbatim, its title and
/// choices, the new window, and the block's markers that lie wholly inside
/// it (AD-355). `transcript` is the block's, when it has one: it counts the
/// lines and, with `words`, writes them as a folded callout after the fence
/// (AD-356).
pub fn clip_block(
    body: &str,
    window: ClipWindow,
    transcript: Option<&Transcript>,
    words: bool,
) -> Result<MediaClipVm, BlockRefusal> {
    let block = parse(body)?;
    let document: DocumentMut = body.parse().map_err(|error: toml_edit::TomlError| {
        BlockRefusal::Syntax(first_line(&error.to_string()))
    })?;
    let from = window.from.or(block.from);
    let to = window.to.or(block.to);
    let outside = block
        .from
        .is_some_and(|start| from.is_none_or(|from| from < start))
        || block.to.is_some_and(|end| to.is_none_or(|to| to > end));
    if outside {
        return Err(BlockRefusal::ClipOutsideWindow);
    }
    check_window(from, to)?;

    let mut keys: Vec<(&str, String)> = Vec::new();
    for key in ["session", "transcript", "src", "title"] {
        if let Some(value) = document.get(key).and_then(Item::as_value) {
            keys.push((key, bare(value)));
        }
    }
    let from = from.unwrap_or(0.0);
    keys.push(("from", quoted(&format_time(from))));
    if let Some(to) = to {
        keys.push(("to", quoted(&format_time_up(to))));
    }
    for key in ["picture", "sound"] {
        if let Some(value) = document.get(key).and_then(Item::as_value) {
            keys.push((key, bare(value)));
        }
    }
    let mut tables = String::new();
    if let Some(parts) = document.get("part") {
        for part in tables_of(parts) {
            tables.push_str("\n[[part]]\n");
            for (key, value) in part {
                let _ = writeln!(tables, "{key} = {}", bare(value));
            }
        }
    }
    let inside = |marker: &Marker| {
        marker.from >= from
            && to.is_none_or(|to| match marker.to {
                None => marker.from < to,
                Some(end) => end <= to,
            })
    };
    let mut kept_markers = false;
    if let Some(items) = document.get("marker") {
        for (marker, table) in block.markers.iter().zip(tables_of(items)) {
            if !inside(marker) {
                continue;
            }
            kept_markers = true;
            tables.push_str("\n[[marker]]\n");
            for (key, value) in table {
                let _ = writeln!(tables, "{key} = {}", bare(value));
            }
        }
    }
    let title = block.title.as_deref();
    Ok(with_words(
        described(&keys, &tables, kept_markers),
        title,
        from,
        to,
        transcript,
        words,
    ))
}

/// What a clip from the transcript viewer names its meeting by (AD-355): the
/// recording's identity when the transcript's folder holds a manifest with
/// one, else the transcript's path relative to the synced folder holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipSource {
    Session(String),
    Transcript(String),
}

/// A clip of a whole transcript, or of a window of it, from the viewer.
pub fn clip_transcript(
    source: &ClipSource,
    window: ClipWindow,
    transcript: &Transcript,
    words: bool,
) -> MediaClipVm {
    let mut keys = vec![match source {
        ClipSource::Session(id) => ("session", quoted(id)),
        ClipSource::Transcript(path) => ("transcript", quoted(path)),
    }];
    if let Some(from) = window.from {
        keys.push(("from", quoted(&format_time(from))));
    }
    if let Some(to) = window.to {
        keys.push(("to", quoted(&format_time_up(to))));
    }
    with_words(
        described(&keys, "", false),
        None,
        window.from.unwrap_or(0.0),
        window.to,
        Some(transcript),
        words,
    )
}

fn bare(value: &Value) -> String {
    let mut value = value.clone();
    value.decor_mut().clear();
    value.to_string()
}

/// The key-value pairs of each table in an array of tables, however written.
fn tables_of(item: &Item) -> Vec<Vec<(String, &Value)>> {
    match item {
        Item::ArrayOfTables(array) => array
            .iter()
            .map(|table| {
                table
                    .iter()
                    .filter_map(|(key, item)| Some((key.to_owned(), item.as_value()?)))
                    .collect()
            })
            .collect(),
        Item::Value(Value::Array(array)) => array
            .iter()
            .filter_map(Value::as_inline_table)
            .map(|table| {
                table
                    .iter()
                    .map(|(key, value)| (key.to_owned(), value))
                    .collect()
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether `utterance` overlaps the window `[from, to)`.
fn overlaps(utterance: &Utterance, from: f64, to: Option<f64>) -> bool {
    utterance.end > from && to.is_none_or(|to| utterance.start < to)
}

fn with_words(
    fence: String,
    title: Option<&str>,
    from: f64,
    to: Option<f64>,
    transcript: Option<&Transcript>,
    words: bool,
) -> MediaClipVm {
    let Some(transcript) = transcript else {
        return MediaClipVm {
            markdown: fence,
            lines: 0,
        };
    };
    let lines: Vec<&Utterance> = transcript
        .utterances
        .iter()
        .filter(|utterance| overlaps(utterance, from, to))
        .collect();
    let count = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    if !words || lines.is_empty() {
        return MediaClipVm {
            markdown: fence,
            lines: count,
        };
    }
    let mut markdown = fence;
    let title = title
        .or(transcript.source.title.as_deref())
        .unwrap_or("Transcript");
    let end = to.unwrap_or(transcript.duration);
    let _ = writeln!(
        markdown,
        "> [!transcript]- {title} · {}–{}",
        format_time(from),
        format_time_up(end)
    );
    for utterance in lines {
        let _ = writeln!(markdown, "> {}", render::line(transcript, utterance));
    }
    MediaClipVm {
        markdown,
        lines: count,
    }
}

/// One media block found in a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundBlock {
    /// The fence's body, its indentation removed.
    pub body: String,
    /// The opening fence's line, 1-based.
    pub first_line: usize,
    /// The closing fence's line, 1-based — or the note's last line, for a
    /// fence the note never closes.
    pub last_line: usize,
}

/// Every media block in `note`, in document order. Fences of other kinds are
/// stepped over whole, so a block quoted inside a ` ```markdown ` example is
/// not one.
pub fn blocks(note: &str) -> Vec<FoundBlock> {
    let lines: Vec<&str> = note.lines().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some((indent, marker, run, info)) = opening(lines[index]) else {
            index += 1;
            continue;
        };
        let start = index;
        let mut end = lines.len();
        for (offset, line) in lines[start + 1..].iter().enumerate() {
            if closes(line, marker, run) {
                end = start + 1 + offset;
                break;
            }
        }
        if info.split_whitespace().next() == Some(INFO_WORD) {
            let mut body = String::new();
            for line in &lines[start + 1..end.min(lines.len())] {
                body.push_str(strip_indent(line, indent));
                body.push('\n');
            }
            found.push(FoundBlock {
                body,
                first_line: start + 1,
                last_line: (end + 1).min(lines.len()),
            });
        }
        index = end + 1;
    }
    found
}

/// A line that opens a fence: its indentation, the fence character, the
/// run's length and the info string.
fn opening(line: &str) -> Option<(usize, char, usize, &str)> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - trimmed.len();
    let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let run = trimmed.chars().take_while(|c| *c == marker).count();
    if run < 3 {
        return None;
    }
    let info = trimmed[run..].trim();
    if marker == '`' && info.contains('`') {
        return None;
    }
    Some((indent, marker, run, info))
}

fn closes(line: &str, marker: char, run: usize) -> bool {
    let trimmed = line.trim();
    trimmed.chars().take_while(|c| *c == marker).count() >= run
        && trimmed.chars().all(|c| c == marker)
}

fn strip_indent(line: &str, indent: usize) -> &str {
    let spaces = line
        .bytes()
        .take(indent)
        .take_while(|byte| *byte == b' ' || *byte == b'\t')
        .count();
    &line[spaces..]
}

/// The recordings the media blocks of `note` name by identity — what the
/// attachments panel counts as in the note (AD-357). A block naming its
/// recording through `src` is not read here: that needs the file.
pub fn session_ids(note: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for block in blocks(note) {
        if let Ok(Block {
            source: Source::Session(id),
            ..
        }) = parse(&block.body)
        {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Each media block of `note` with its byte range, from its opening fence's
/// first byte to its closing fence's last, without the line break after it.
fn spanned_blocks(note: &str) -> Vec<((usize, usize), FoundBlock)> {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(note.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let end_of = |line: usize| {
        let start = starts[line - 1];
        start + note[start..].find('\n').unwrap_or(note.len() - start)
    };
    blocks(note)
        .into_iter()
        .map(|block| {
            (
                (starts[block.first_line - 1], end_of(block.last_line)),
                block,
            )
        })
        .collect()
}

/// What a recording note's frontmatter says about its own recording: how a
/// block naming that recording is told apart in the notes list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnRecording<'a> {
    pub session: &'a str,
    pub title: Option<&'a str>,
    /// The stub's `duration:`, as it wrote it.
    pub duration: Option<&'a str>,
}

impl<'a> OwnRecording<'a> {
    /// The recording `front` is about, or `None` for a note about none.
    pub fn of(front: &'a Frontmatter) -> Option<Self> {
        let fact = |key: &str| {
            front
                .as_string(key)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        };
        Some(Self {
            session: fact(SESSION_KEY)?,
            title: fact("title"),
            duration: fact("duration"),
        })
    }
}

/// The one line a block reads as in the notes list and search results,
/// never its source: `▶ Media`, the title — the block's own, else its
/// note's recording's when it names that recording — and how long it plays:
/// its window, else that recording's duration. A body that does not read is
/// `▶ Media` alone.
pub fn summary(body: &str, own: Option<OwnRecording<'_>>) -> String {
    let mut out = String::from("▶ Media");
    let Ok(block) = parse(body) else {
        return out;
    };
    let own =
        own.filter(|own| matches!(&block.source, Source::Session(id) if id.trim() == own.session));
    let title = block
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .or(own.and_then(|own| own.title));
    let length = match (block.from, block.to) {
        (from, Some(to)) => Some(span_text(to - from.unwrap_or(0.0))),
        (None, None) => own.and_then(|own| own.duration).map(str::to_owned),
        (Some(_), None) => None,
    };
    for fact in title.into_iter().chain(length.as_deref()) {
        out.push_str(" · ");
        out.push_str(fact);
    }
    out
}

/// A length as a player shows it: `m:ss`, or `h:mm:ss` from an hour.
fn span_text(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (hours, minutes, seconds) = (total / 3_600, total % 3_600 / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `text` with each media block in it replaced by its [`summary`]: what a
/// search excerpt is cut from, so a hit near a block never shows its source.
pub fn summarised_blocks(text: &str) -> Cow<'_, str> {
    let found = spanned_blocks(text);
    if found.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for ((start, end), block) in found {
        out.push_str(&text[cursor..start]);
        out.push_str(&summary(&block.body, None));
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    Cow::Owned(out)
}

/// Where a marker link lands: the ordinal of the media block holding the
/// marker among the note's media blocks, and the marker's times.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaMarkerHitVm {
    /// 0-based, among the note's media blocks in document order.
    pub block: u32,
    /// The marker's own spelling.
    pub name: String,
    pub from: f64,
    pub to: Option<f64>,
}

/// The first of `blocks` — each already read, `src` merged — holding a
/// marker called `name` (AD-354). `None` when none does, or a block did not
/// read.
pub fn find_marker<'b>(
    blocks: impl IntoIterator<Item = Option<&'b Block>>,
    name: &str,
) -> Option<MediaMarkerHitVm> {
    blocks.into_iter().enumerate().find_map(|(index, block)| {
        let marker = block?.marker(name)?;
        Some(MediaMarkerHitVm {
            block: u32::try_from(index).ok()?,
            name: marker.name.clone(),
            from: marker.from,
            to: marker.to,
        })
    })
}

/// A replacement of whole lines of a note: `text` in place of lines
/// `first_line..=last_line` (1-based), without their final line break, or
/// the lines deleted with their line breaks when `text` is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LineEditVm {
    pub first_line: u32,
    pub last_line: u32,
    pub text: Option<String>,
}

fn line_edit(first: usize, last: usize, text: Option<String>) -> LineEditVm {
    LineEditVm {
        first_line: u32::try_from(first).unwrap_or(u32::MAX),
        last_line: u32::try_from(last).unwrap_or(u32::MAX),
        text,
    }
}

/// An embed of `note` found on a line: its 1-based line and its target.
fn embeds(note: &str) -> Vec<(usize, links::RawLink)> {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(note.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    links::extract(note)
        .into_iter()
        .filter(|link| link.embed)
        .map(|link| {
            let line = starts.partition_point(|start| *start <= link.span.0);
            (line, link)
        })
        .collect()
}

/// The line at 1-based `number`, without its line break.
fn line_at(note: &str, number: usize) -> Option<&str> {
    note.lines().nth(number.checked_sub(1)?)
}

/// "Play in a player" on the recording embed at line `line` of a recording
/// note (N2): every embed of that recording's media in the note becomes one
/// block naming the recording, at the first of them. `is_session_media`
/// answers whether an embed target is one of the recording's playable
/// files. Empty when the embed at `line` is not one.
pub fn session_embeds_to_block(
    note: &str,
    session_id: &str,
    line: usize,
    is_session_media: impl Fn(&str) -> bool,
) -> Vec<LineEditVm> {
    let own: Vec<(usize, links::RawLink)> = embeds(note)
        .into_iter()
        .filter(|(_, link)| is_session_media(&link.target))
        .collect();
    if !own.iter().any(|(at, _)| *at == line) {
        return Vec::new();
    }
    let block = session_block(session_id);
    let mut edits: Vec<LineEditVm> = Vec::new();
    let mut placed = false;
    let mut last_line = 0;
    for (number, _) in &own {
        if *number == last_line {
            continue;
        }
        last_line = *number;
        let Some(text) = line_at(note, *number) else {
            continue;
        };
        let rest = without_embeds(text, &own, *number, note);
        if !placed {
            edits.push(line_edit(
                *number,
                *number,
                Some(around(&rest, block.trim_end())),
            ));
            placed = true;
        } else if rest.trim().is_empty() {
            edits.push(line_edit(*number, *number, None));
        } else {
            edits.push(line_edit(*number, *number, Some(rest.trim().to_owned())));
        }
    }
    edits
}

/// "Play in a player" on a plain media embed at line `line` whose target
/// resolved to `drive_relative`: the embed becomes a one-part block.
pub fn file_embed_to_block(
    note: &str,
    line: usize,
    target: &str,
    drive_relative: &str,
) -> Option<LineEditVm> {
    let text = line_at(note, line)?;
    let (at, link) = embeds(note)
        .into_iter()
        .find(|(at, link)| *at == line && link.target == target)?;
    let rest = without_embeds(text, &[(at, link)], line, note);
    Some(line_edit(
        line,
        line,
        Some(around(&rest, part_block(drive_relative).trim_end())),
    ))
}

/// `text` (line `number` of `note`) with the embeds among `drop` on it cut
/// out.
fn without_embeds(
    text: &str,
    drop: &[(usize, links::RawLink)],
    number: usize,
    note: &str,
) -> String {
    let line_start: usize = note
        .split_inclusive('\n')
        .take(number.saturating_sub(1))
        .map(str::len)
        .sum();
    let mut out = String::new();
    let mut cursor = 0;
    for (_, link) in drop.iter().filter(|(at, _)| *at == number) {
        let (start, end) = (
            link.span.0.saturating_sub(line_start),
            link.span.1.saturating_sub(line_start).min(text.len()),
        );
        if start >= cursor && start <= text.len() {
            out.push_str(&text[cursor..start]);
            cursor = end;
        }
    }
    out.push_str(&text[cursor.min(text.len())..]);
    out
}

/// `block` on lines of its own, with what shared its line kept above it.
fn around(rest: &str, block: &str) -> String {
    let rest = rest.trim();
    if rest.is_empty() {
        block.to_owned()
    } else {
        format!("{rest}\n{block}")
    }
}

/// What the rewrite of one old recording stub decided (N5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adoption {
    /// Not a recording note, or none of its own embeds and no bare block of
    /// its own is left: nothing to do, and nothing written.
    Untouched,
    /// The stub's run of per-file embeds, or its bare block, became the
    /// block a stub carries now; the note's new text.
    Changed(String),
    /// The note embeds its recording's files, but not in the shape keeper
    /// wrote: somebody edited them, and keeper leaves them alone.
    Skipped,
}

/// The answer of `recording_notes_adopt_media_block`: how many notes
/// changed (or would, on a dry run), and the notes left alone because their
/// embeds were edited by hand, relative to the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaAdoptionVm {
    pub changed: u32,
    pub skipped: Vec<String>,
}

/// A recording stub's per-file embeds of its own recording, rewritten into
/// the one block a stub carries now (N5, AD-357). Only the exact run the
/// stub composer wrote is replaced — the videos of `files:`, one embed per
/// line, in that order — and every other byte stays. A stub with no embed
/// of its own has its bare block — the three lines stubs carried before a
/// block listed its optional keys — rewritten into [`session_block`]'s
/// shape instead. Run again, it finds neither and leaves the note alone.
pub fn adopt(note: &str) -> Adoption {
    let (front, body_offset) = Frontmatter::parse(note);
    if !recording_note::is_recording_note(&front) {
        return Adoption::Untouched;
    }
    let Some(session_id) = front.as_string(SESSION_KEY).map(str::trim) else {
        return Adoption::Untouched;
    };
    let files = front.as_list("files").unwrap_or_default();
    let body = &note[body_offset..];
    let own = links::extract(body)
        .into_iter()
        .filter(|link| link.embed && files.contains(&link.target))
        .count();
    if own == 0 {
        return describe_bare_blocks(note, body_offset, session_id);
    }
    let names: Vec<&str> = files.iter().map(|file| file.trim()).collect();
    let run = recording_note::video_embeds(&names);
    let run = run.trim_end_matches('\n');
    let expected = run.lines().count();
    if run.is_empty() || own != expected {
        return Adoption::Skipped;
    }
    let mut starts = body.match_indices(run).filter(|(at, _)| {
        (*at == 0 || body.as_bytes()[at - 1] == b'\n') && body[at + run.len()..].starts_with('\n')
    });
    let (Some((at, _)), None) = (starts.next(), starts.next()) else {
        return Adoption::Skipped;
    };
    let at = body_offset + at;
    let mut out = String::with_capacity(note.len());
    out.push_str(&note[..at]);
    out.push_str(session_block(session_id).trim_end_matches('\n'));
    out.push_str(&note[at + run.len()..]);
    Adoption::Changed(out)
}

/// Every fence of `note`'s body that is exactly the bare block keeper once
/// wrote for `session_id`, rewritten as [`session_block`]; a block anybody
/// wrote a key into is not one, nor is one quoted inside another fence.
fn describe_bare_blocks(note: &str, body_offset: usize, session_id: &str) -> Adoption {
    let bare = format!("```{INFO_WORD}\nsession = {}\n```", quoted(session_id));
    let body = &note[body_offset..];
    let spans: Vec<(usize, usize)> = spanned_blocks(body)
        .into_iter()
        .map(|(span, _)| span)
        .filter(|(start, end)| body[*start..*end] == bare)
        .collect();
    if spans.is_empty() {
        return Adoption::Untouched;
    }
    let block = session_block(session_id);
    let block = block.trim_end_matches('\n');
    let mut out = String::with_capacity(note.len() + spans.len() * block.len());
    out.push_str(&note[..body_offset]);
    let mut cursor = 0;
    for (start, end) in spans {
        out.push_str(&body[cursor..start]);
        out.push_str(block);
        cursor = end;
    }
    out.push_str(&body[cursor..]);
    Adoption::Changed(out)
}

/// Whether `name` is a file a media block plays, by keeper's one extension
/// table.
pub fn is_playable(name: &str) -> bool {
    matches!(
        kind_for_file_name(name),
        RecordingNoteTargetKind::Video | RecordingNoteTargetKind::Audio
    )
}

mod hints;
pub use hints::{
    check, schema, MediaBlockProblemVm, MediaBlockSchemaVm, MediaKeyPlace, MediaKeyVm,
    MediaValueKind,
};

#[cfg(test)]
mod tests;
